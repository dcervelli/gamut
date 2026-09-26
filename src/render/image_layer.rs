//! Draws the image into the linear working-space target.
//!
//! Everything color-related that varies per frame lives in one uniform, so
//! changing exposure, the window or the colormap costs a buffer write rather
//! than a re-decode. Resampling is chosen the same way: which filter to run
//! and which level of the coarse chain to read are two more fields in it.

use std::sync::Arc;

use anyhow::{Result, anyhow};
use bytemuck::{Pod, Zeroable};

use super::gpu::{self, Fullscreen};
use super::placement::{Glass, Placement, Upscale};
use super::reduce::{self, Level, Reducer};
use super::shader_codes;
use super::upload::{self, Capabilities};
use crate::image::gain_map::GainMap;
use crate::image::orient::Turn;
use crate::image::{
    AlphaMode, Channels, DecodedImage,
    display::{Colormap, Display, Headroom},
};

/// How many entries a false-color ramp is written to the device as. The
/// shader interpolates between neighbors, so this is what bounds how far a
/// value on screen can be from `Colormap::color` of the same value: a
/// thousand steps puts it under what a 16-bit target resolves.
const RAMP_LENGTH: u32 = 1024;

/// Layout must match `struct Params` in shaders/image.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    offset: [f32; 2],
    scale: [f32; 2],
    window: [f32; 2],
    texels_per_pixel: [f32; 2],
    extent: [f32; 2],
    marks: u32,
    /// Quarter turns clockwise the texture is read through — see
    /// `shader_codes::turn`.
    turn: u32,
    /// Column-major, each column padded to 16 bytes, as WGSL wants a mat3x3.
    primaries: [[f32; 4]; 3],
    swizzle: u32,
    alpha_mode: u32,
    colormap: u32,
    resampler: u32,
    /// 0 for no lift, else the gain map's channel count; 0 on a coarse
    /// level, which holds lifted light already.
    lift: u32,
    /// Which level of the coarse chain `source` is: 0 for the picture as
    /// uploaded, whose marks are judged live, else the level whose marks
    /// are bound beside it.
    level: u32,
    map_size: [f32; 2],
    base_offset: [f32; 4],
    alternate_offset: [f32; 4],
    /// The circle the quad is cut to, `(x, y, radius, cut)` in target
    /// pixels, and a radius of zero for a quad that is not cut; `cut` is
    /// which part of the circle this quad is — see [`Cut`].
    clip: [f32; 4],
    /// Where the image itself lands, `(x, y, width, height)` in target
    /// pixels, for the cut quad: its own corners are the circle's square, so
    /// the shader reads the image's place off this instead.
    picture: [f32; 4],
}

/// A picture's gain map on the device: the map as uploaded, and the table
/// the lift is read through at the weight the surface asks for, which is
/// written again when the weight changes. See `image::gain_map`.
struct Lift {
    map: Arc<GainMap>,
    _map_texture: wgpu::Texture,
    lut: wgpu::Texture,
    group: wgpu::BindGroup,
    /// The weight the table on the device was made at.
    weight: f32,
    base_offset: [f32; 3],
    alternate_offset: [f32; 3],
}

/// What a pass that reads the image as uploaded binds and is told, to lift
/// it: the gain map and its table, or the blanks with the lift switched
/// off.
#[derive(Clone, Copy)]
pub struct Lifted<'a> {
    pub group: &'a wgpu::BindGroup,
    /// 0 for no lift, else the map's channel count.
    pub code: u32,
    pub map_size: [f32; 2],
    pub base_offset: [f32; 4],
    pub alternate_offset: [f32; 4],
}

impl<'a> Lifted<'a> {
    fn of(lift: Option<&'a Lift>, blank: &'a wgpu::BindGroup) -> Self {
        match lift {
            Some(lift) => Self {
                group: &lift.group,
                code: u32::from(lift.map.channels),
                map_size: [lift.map.width as f32, lift.map.height as f32],
                base_offset: padded(lift.base_offset),
                alternate_offset: padded(lift.alternate_offset),
            },
            None => Self {
                group: blank,
                code: 0,
                map_size: [1.0, 1.0],
                base_offset: [0.0; 4],
                alternate_offset: [0.0; 4],
            },
        }
    }
}

fn padded(offset: [f32; 3]) -> [f32; 4] {
    [offset[0], offset[1], offset[2], 0.0]
}

/// What the marks' coarse chain is stored in: a byte each for the share of
/// a level's texel at white and at black.
const MARKS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg8Unorm;

/// The coarse chain of the marks on the picture's texels, while the marks
/// are on and a coarse level is being drawn. Each texel of its first level
/// is the share of the `STEP` by `STEP` block of the picture under it that
/// `judge` in `shaders/image.wgsl` puts at white, and at black, under the
/// window in force; `fs_marks` writes that level outright from the picture,
/// and the reducer makes the rest from it as it makes the picture's chain
/// from the picture. Every coarse level of the picture is bound beside the
/// marks at the same level, so that a minified draw reads the share of the
/// texels under a pixel that are at each end, where a test of their average
/// would find an end only under a pixel every texel of which is at it, and
/// a scattered shadow would vanish the moment the view dropped below 1:1.
///
/// There is no level 0: at the picture's own level the shader judges the
/// texels under a pixel live, from the picture, so nothing is stored per
/// texel of the picture, and the chain is a sixteenth of the marks it stands
/// for — about a bit a texel. The marks are the window's and the lift's, so
/// the chain is written again when either changes, and dropped with the
/// picture's chain when its texels change. It is dropped altogether while
/// the marks are off, or nothing coarse is drawn, so a picture whose marks
/// are never asked for pays nothing for them.
struct Marks {
    _first_texture: wgpu::Texture,
    /// The chain's first level, the picture's level 1.
    first: wgpu::TextureView,
    /// The levels after it, reduced from the first.
    rest: Vec<Level>,
    /// The picture as uploaded, bound beside the blank, for `fs_marks` to
    /// read it through: its own bind group, since the pass writes the chain
    /// and cannot also have the picture's level-1 group, which names it.
    reading: wgpu::BindGroup,
    /// What the marks were written under.
    key: MarksKey,
}

/// What the marks on the texels depend on: the window as the shader applies
/// it, exposure included, and the weight of the gain map's lift.
#[derive(Clone, Copy, PartialEq)]
struct MarksKey {
    window: (f32, f32),
    weight: f32,
}

impl Marks {
    /// The first level's texture for a picture of `size`, unwritten, and the
    /// binding the pass writes it from.
    fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        picture: &wgpu::TextureView,
        blank_marks: &wgpu::TextureView,
        size: [u32; 2],
        key: MarksKey,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("marks"),
            size: wgpu::Extent3d {
                width: size[0].div_ceil(reduce::STEP).max(1),
                height: size[1].div_ceil(reduce::STEP).max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: MARKS_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let first = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            _first_texture: texture,
            first,
            rest: Vec::new(),
            reading: binding(device, layout, picture, blank_marks),
            key,
        }
    }

    /// The marks at coarse level `level` of the picture, 1 being the first.
    fn at(&self, level: usize) -> Option<&wgpu::TextureView> {
        match level {
            0 => None,
            1 => Some(&self.first),
            _ => self.rest.get(level - 2).map(|level| &level.view),
        }
    }
}

/// One uploaded image and the constants that describe it.
pub struct GpuImage {
    size: [u32; 2],
    /// Kept beside its view so that another frame of the same shape can be
    /// written into it in place.
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    /// Bind groups indexed by level: 0 is the image as uploaded, the rest are
    /// the coarse chain, each beside the marks at the same level — or the
    /// blank texel, while the marks are off. Empty past the first until a
    /// view zooms out far enough to want it, since most never do.
    bindings: Vec<wgpu::BindGroup>,
    levels: Vec<Level>,
    /// The marks' coarse chain, while the marks are on and a coarse level
    /// is drawn.
    marks: Option<Marks>,
    /// Set once the chain has been built, which is not the same as its being
    /// non-empty: an image only a few texels across has no levels to make.
    chain_built: bool,
    level_format: wgpu::TextureFormat,
    swizzle: u32,
    alpha: AlphaMode,
    primaries: [[f32; 4]; 3],
    /// The gain map, where the picture has one.
    lift: Option<Lift>,
    pub format: wgpu::TextureFormat,
    pub precision_note: Option<&'static str>,
}

/// One draw's worth of constants, and the binding that points at them.
///
/// One per draw rather than one buffer rewritten between draws: the image and
/// the minimap's thumbnail are two quads in the same pass, so both sets have
/// to be live at once.
struct Slot {
    buffer: wgpu::Buffer,
    group: wgpu::BindGroup,
}

/// What one frame draws: the view, the minimap's thumbnail when it is on
/// screen, and the loupe's glass while it is up. Passed together because
/// they share a pass, a texture and a coarse chain, and because whether the
/// chain is needed at all is a question about all of them. With them, the
/// two things about the frame that the shader's marks on clipped pixels
/// depend on.
#[derive(Clone, Copy)]
pub struct Draw {
    pub view: Placement,
    pub thumbnail: Option<Placement>,
    pub loupe: Option<Glass>,
    /// Whether the pixels the window has taken to black or to white are
    /// painted in the warning colors — while the key for it is held.
    pub mark_clipped: bool,
    /// What decides whether white is being clipped at all, which is the
    /// display's to say — see `Display::clips_white`.
    pub headroom: Headroom,
    /// How much of a gain map's lift the picture gets, from none to all of
    /// it: the weight the surface's room above white asks for.
    pub lift: f32,
    /// How far the picture is turned: both quads read the texture turned.
    pub turn: Turn,
}

impl Draw {
    /// A draw with nothing marked, on an SDR surface: what a test draws.
    #[cfg(test)]
    pub fn plain(view: Placement, thumbnail: Option<Placement>) -> Self {
        Self {
            view,
            thumbnail,
            loupe: None,
            mark_clipped: false,
            headroom: Headroom::None,
            lift: 0.0,
            turn: Turn::NONE,
        }
    }
}

pub struct ImageLayer {
    pipeline: wgpu::RenderPipeline,
    /// The same shader with no blending: what the loupe's glass is drawn
    /// with, so that it replaces the view under it — the picture's edge,
    /// magnified, has the backdrop past it rather than the view showing
    /// through, and a translucent picture has the checkerboard under it
    /// rather than the view — where source-over could only lay itself on
    /// top. The circle's edge is the exception: a pixel it crosses has to
    /// blend the glass over the view, or it blends it over the backdrop
    /// and draws a hairline of the backdrop's color around the glass, so
    /// the last pixel of the circle is a second quad drawn blending.
    replacing: wgpu::RenderPipeline,
    /// The same shader's `fs_marks`, writing the first level of the marks'
    /// chain from the picture.
    marking: wgpu::RenderPipeline,
    texture_layout: wgpu::BindGroupLayout,
    lift_layout: wgpu::BindGroupLayout,
    /// What a picture with no gain map binds in the map's place: a texel of
    /// each, never read, since the lift is switched off with them.
    blank_lift: wgpu::BindGroup,
    /// What is bound in the marks' place where there are none: beside the
    /// picture as uploaded, whose marks are judged live, and beside every
    /// level while the marks are off. A texel of zeros, never read.
    blank_marks: wgpu::TextureView,
    /// The false-color ramps, one row per map, written once from
    /// `Colormap::color` — the same values the readout names, so that the
    /// screen and the swatch cannot disagree.
    ramps: wgpu::BindGroup,
    main: Slot,
    thumbnail: Slot,
    /// The loupe's glass: the disc, drawn replacing, and the band along
    /// its edge, drawn blending.
    loupe: Slot,
    rim: Slot,
    /// The constants the marks are written under: the picture whole, a
    /// block of `STEP` by `STEP` texels to the pixel, at the window in force.
    marks_slot: Slot,
    reducer: Reducer,
    image: Option<GpuImage>,
    /// Which of the current image's bind groups the next draw reads, decided
    /// in `prepare` from the zoom.
    level: usize,
    /// The same for the thumbnail, and `None` on a frame with no minimap on
    /// screen, which is what leaves its quad undrawn.
    thumbnail_level: Option<usize>,
    /// And for the loupe's glass, `None` while the loupe is down.
    loupe_level: Option<usize>,
}

impl GpuImage {
    /// Whether the image has one channel of color, which is what decides
    /// whether a false color is on it.
    pub fn is_gray(&self) -> bool {
        self.swizzle < 2
    }
}

impl ImageLayer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("image layer"),
            source: wgpu::ShaderSource::Wgsl(super::IMAGE_SHADER.into()),
        });

        let params_layout =
            gpu::uniform_layout(device, "image params", wgpu::ShaderStages::VERTEX_FRAGMENT);
        // No sampler: the shader loads texels and weights them itself, which
        // is what lets one pipeline serve an area filter, an antialiased
        // nearest and a bicubic. Every format `upload::plan` can produce is
        // filterable all the same, and so is every format `reduce` writes.
        // The picture at the level the draw reads, and the marks on it at
        // the same level.
        let texture_layout = gpu::texture_layout(device, "image texture", 2, true);
        let blank_marks = blank_marks(device);
        // Loaded rather than sampled as well, so nothing here has to be
        // filterable — which the 32-bit float table could not be on every
        // device.
        let lift_layout = gpu::texture_layout(device, "gain map", 2, false);
        let blank_lift = blank_lift(device, &lift_layout);
        let ramps_layout = gpu::texture_layout(device, "ramps", 1, false);
        let ramps = ramps(device, queue, &ramps_layout);
        let pipeline_layout = gpu::pipeline_layout(
            device,
            "image layer",
            &[&params_layout, &texture_layout, &lift_layout, &ramps_layout],
        );
        let pipeline = gpu::fullscreen_pipeline(
            device,
            Fullscreen {
                label: "image layer",
                shader: &shader,
                layout: &pipeline_layout,
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                format: target_format,
                // The shader emits premultiplied color.
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            },
        );
        let replacing = gpu::fullscreen_pipeline(
            device,
            Fullscreen {
                label: "image layer, replacing",
                shader: &shader,
                layout: &pipeline_layout,
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                format: target_format,
                blend: None,
            },
        );
        let marking = gpu::fullscreen_pipeline_entry(
            device,
            Fullscreen {
                label: "image layer, marks",
                shader: &shader,
                layout: &pipeline_layout,
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                format: MARKS_FORMAT,
                blend: None,
            },
            "fs_marks",
        );

        Self {
            pipeline,
            replacing,
            marking,
            reducer: Reducer::new(device, &lift_layout),
            texture_layout,
            lift_layout,
            blank_lift,
            blank_marks,
            ramps,
            main: Slot::new(device, &params_layout, "image params"),
            thumbnail: Slot::new(device, &params_layout, "thumbnail params"),
            loupe: Slot::new(device, &params_layout, "loupe params"),
            rim: Slot::new(device, &params_layout, "loupe rim params"),
            marks_slot: Slot::new(device, &params_layout, "marks params"),
            image: None,
            level: 0,
            thumbnail_level: None,
            loupe_level: None,
        }
    }

    pub fn current(&self) -> Option<&GpuImage> {
        self.image.as_ref()
    }

    /// A handle that turns decoded images into [`GpuImage`]s. Held apart from
    /// the layer so it can be sent to the thread doing the decoding: the
    /// repack and the copy across are both proportional to the pixel count,
    /// and neither belongs on the thread drawing frames.
    pub fn uploader(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        capabilities: Capabilities,
    ) -> Upload {
        Upload {
            device: device.clone(),
            queue: queue.clone(),
            layout: self.texture_layout.clone(),
            lift_layout: self.lift_layout.clone(),
            blank_marks: self.blank_marks.clone(),
            capabilities,
        }
    }

    /// Puts an uploaded image on screen, replacing whatever was there.
    ///
    /// Dropping the previous image here is what keeps the coarse chain
    /// bounded: only one is ever alive, and it goes with the image it
    /// describes rather than accumulating as files are stepped through.
    pub fn install(&mut self, image: GpuImage) {
        self.image = Some(image);
        self.level = 0;
        self.thumbnail_level = None;
        self.loupe_level = None;
    }

    /// Takes the image off, and its coarse chain with it: nothing is drawn
    /// until another is installed.
    pub fn remove(&mut self) {
        self.image = None;
        self.level = 0;
        self.thumbnail_level = None;
        self.loupe_level = None;
    }

    /// Writes `image`'s pixels into the texture already on screen, for the
    /// next frame of an animation. Answers `false` where the picture on
    /// screen is not the same shape — a different size, or a layout that
    /// would store differently — and the caller uploads afresh instead.
    pub fn refill(&mut self, upload: &Upload, image: &DecodedImage) -> Result<bool> {
        let Some(held) = &mut self.image else {
            return Ok(false);
        };
        upload.refill(held, image)
    }

    /// `draw.thumbnail`, when there is one, is the minimap's copy of the same
    /// image: a second quad, drawn from the same texture in the same pass, so
    /// that it is tone mapped and windowed exactly as the image it stands for
    /// — and marked exactly as it is, where the marks are on. `draw.loupe`,
    /// while the loupe is up, is a third: the image magnified about the
    /// point under the pointer, and cut to the glass.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        draw: Draw,
        target: [f32; 2],
        display: &Display,
    ) {
        let Draw {
            view,
            thumbnail,
            loupe,
            mark_clipped,
            headroom,
            lift: weight,
            turn,
        } = draw;
        let Some(image) = &mut self.image else {
            return;
        };
        let window = display.transform();
        let marks = shader_codes::marks(
            mark_clipped,
            mark_clipped && display.clips_white(image.is_gray(), headroom),
        );

        // A lift at another weight is another table, written over the one
        // on the device — and the coarse chain, reduced from light lifted
        // by the old one, goes with it, to be built again from the new, as
        // do the marks, which were judged on the old light.
        if let Some(lift) = &mut image.lift
            && lift.weight != weight
        {
            lift.write(queue, weight);
            image.levels.clear();
            image.bindings.truncate(1);
            image.chain_built = false;
            image.marks = None;
        }

        let factor = shrink(view);
        // The thumbnail is shrunk far harder than the view ever is, so it is
        // what decides whether the chain is needed at all; the loupe, being
        // the view magnified, never asks for more than the view does.
        let coarsest = thumbnail
            .into_iter()
            .chain(loupe.map(|glass| glass.placement))
            .fold(factor, |coarsest, placement| {
                coarsest.max(shrink(placement))
            });
        let coarse = coarsest > reduce::STEP as f32;

        // The marks' chain, where the marks are on and a coarse level is
        // drawn: written under this frame's window and lift unless it already
        // is, and let go otherwise. Every level's bind group names the marks
        // at that level, so a change to the chain is a change to the
        // bindings.
        let wanted = (mark_clipped && coarse).then_some(MarksKey {
            window,
            weight: image.lift.as_ref().map_or(0.0, |lift| lift.weight),
        });
        if image.marks.as_ref().map(|marks| marks.key) != wanted {
            match wanted {
                Some(key) => write_marks(
                    device,
                    queue,
                    encoder,
                    Marking {
                        pipeline: &self.marking,
                        slot: &self.marks_slot,
                        reducer: &mut self.reducer,
                        layout: &self.texture_layout,
                        blank_lift: &self.blank_lift,
                        blank_marks: &self.blank_marks,
                        ramps: &self.ramps,
                    },
                    image,
                    display,
                    key,
                ),
                None => image.marks = None,
            }
            rebind(device, &self.texture_layout, &self.blank_marks, image);
        }

        if coarse && !image.chain_built {
            image.levels = self.reducer.build(
                device,
                encoder,
                reduce::Source {
                    view: &image.view,
                    size: image.size,
                    extent: None,
                    format: image.level_format,
                    swizzle: image.swizzle,
                    alpha: image.alpha,
                    lift: Lifted::of(image.lift.as_ref(), &self.blank_lift),
                },
            );
            image.chain_built = true;
            rebind(device, &self.texture_layout, &self.blank_marks, image);
        }

        let lifted = Lifted::of(image.lift.as_ref(), &self.blank_lift);
        self.level = reduce::level_for(factor, image.levels.len());
        let quad = Quad {
            target,
            display,
            window,
            marks,
            lifted,
            turn,
        };
        self.main
            .write(queue, params_for(image, view, None, self.level, quad));

        self.thumbnail_level =
            thumbnail.map(|thumbnail| reduce::level_for(shrink(thumbnail), image.levels.len()));
        if let (Some(thumbnail), Some(level)) = (thumbnail, self.thumbnail_level) {
            self.thumbnail
                .write(queue, params_for(image, thumbnail, None, level, quad));
        }

        self.loupe_level =
            loupe.map(|glass| reduce::level_for(shrink(glass.placement), image.levels.len()));
        if let (Some(glass), Some(level)) = (loupe, self.loupe_level) {
            for (slot, cut) in [(&self.loupe, Cut::Disc), (&self.rim, Cut::Band)] {
                slot.write(
                    queue,
                    params_for(image, glass.placement, Some((glass, cut)), level, quad),
                );
            }
        }
    }

    pub fn render(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(image) = &self.image else {
            return;
        };
        // The loupe's glass goes down over the view, replacing rather than
        // blending: what is inside the circle is the glass and nothing
        // else. Its rim — the last pixel of the circle — goes down after
        // it, blending, so that the circle's edge is the glass feathered
        // over the view. The thumbnail goes down last and over all of it:
        // it sits in its corner of the content area, and a view zoomed in
        // past the panels' edges is drawn under it, as is a glass that
        // wanders into the corner — the map stays readable, and the loupe
        // is the thing that moves.
        let draws = [
            Some((&self.main, self.level, &self.pipeline)),
            self.loupe_level
                .map(|level| (&self.loupe, level, &self.replacing)),
            self.loupe_level
                .map(|level| (&self.rim, level, &self.pipeline)),
            self.thumbnail_level
                .map(|level| (&self.thumbnail, level, &self.pipeline)),
        ];
        let lift = image
            .lift
            .as_ref()
            .map_or(&self.blank_lift, |lift| &lift.group);
        for (slot, level, pipeline) in draws.into_iter().flatten() {
            let Some(binding) = image.bindings.get(level) else {
                continue;
            };
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &slot.group, &[]);
            pass.set_bind_group(1, binding, &[]);
            pass.set_bind_group(2, lift, &[]);
            pass.set_bind_group(3, &self.ramps, &[]);
            pass.draw(0..4, 0..1);
        }
    }
}

impl Lift {
    /// The map and its table on the device, the table at no weight until
    /// the first draw says otherwise.
    fn upload(upload: &Upload, map: &Arc<GainMap>) -> Result<Self> {
        let channels = match map.channels {
            1 => Channels::Gray,
            _ => Channels::Rgb,
        };
        let components = if map.channels == 1 { 1 } else { 4 };
        let plan = upload::Plan {
            format: if map.channels == 1 {
                wgpu::TextureFormat::R8Unorm
            } else {
                wgpu::TextureFormat::Rgba8Unorm
            },
            pixels: upload::expand_u8(&map.data, channels, components, u8::MAX),
            bytes_per_row: map.width * components as u32,
            precision_note: None,
        };
        let map_texture = upload.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("gain map"),
            size: wgpu::Extent3d {
                width: map.width,
                height: map.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: plan.format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        upload.fill(&map_texture, &plan, map.width, map.height)?;

        let lut = upload.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("gain table"),
            size: wgpu::Extent3d {
                width: 256,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let group = gpu::texture_group(
            &upload.device,
            "gain map",
            &upload.lift_layout,
            &[
                &map_texture.create_view(&wgpu::TextureViewDescriptor::default()),
                &lut.create_view(&wgpu::TextureViewDescriptor::default()),
            ],
        );
        let mut lift = Self {
            map: Arc::clone(map),
            _map_texture: map_texture,
            lut,
            group,
            weight: 0.0,
            base_offset: [0.0; 3],
            alternate_offset: [0.0; 3],
        };
        lift.write(&upload.queue, 0.0);
        Ok(lift)
    }

    /// Writes the table at `weight` over the one on the device.
    fn write(&mut self, queue: &wgpu::Queue, weight: f32) {
        let table = self.map.table(weight);
        let texels = table.texels();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.lut,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&texels),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256 * 16),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 256,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        (self.base_offset, self.alternate_offset) = table.offsets();
        self.weight = table.weight();
    }
}

/// Every false-color ramp on the device: `Colormap::color` sampled along
/// its length, in linear light, one row per map at the row
/// `shader_codes::colormap` names. The gray row is the identity, and is
/// never read, since no false color is the shader's default arm.
fn ramps(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
) -> wgpu::BindGroup {
    let rows = Colormap::ALL.len() as u32;
    let mut texels = vec![[0.0f32; 4]; (RAMP_LENGTH * rows) as usize];
    for map in Colormap::ALL {
        let row = shader_codes::colormap(map) as usize * RAMP_LENGTH as usize;
        for (index, texel) in texels[row..row + RAMP_LENGTH as usize]
            .iter_mut()
            .enumerate()
        {
            let [r, g, b] = map.color(index as f32 / (RAMP_LENGTH - 1) as f32);
            *texel = [r, g, b, 1.0];
        }
    }
    let size = wgpu::Extent3d {
        width: RAMP_LENGTH,
        height: rows,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ramps"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        bytemuck::cast_slice(&texels),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(RAMP_LENGTH * 16),
            rows_per_image: Some(rows),
        },
        size,
    );
    gpu::texture_group(
        device,
        "ramps",
        layout,
        &[&texture.create_view(&wgpu::TextureViewDescriptor::default())],
    )
}

/// A texel of marks, for the frames the marks are off: zeros, as the device
/// leaves a fresh texture, and never read.
fn blank_marks(device: &wgpu::Device) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("no marks"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: MARKS_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

/// A texel of map and a texel of table, for the pictures that have neither.
fn blank_lift(device: &wgpu::Device, layout: &wgpu::BindGroupLayout) -> wgpu::BindGroup {
    let texel = |label: &str, format: wgpu::TextureFormat| {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default())
    };
    gpu::texture_group(
        device,
        "no gain map",
        layout,
        &[
            &texel("no gain map", wgpu::TextureFormat::R8Unorm),
            &texel("no gain table", wgpu::TextureFormat::Rgba32Float),
        ],
    )
}

/// How much of an image `write_texture` stages at once. 64 MiB keeps the
/// mappable staging buffer small while still copying in few enough calls that
/// the per-call overhead is nothing beside the decode that produced the bytes.
const BAND_BYTES: usize = 64 * 1024 * 1024;

/// The GPU half of opening a file: everything needed to turn decoded samples
/// into a texture, and nothing that has to stay on one thread. Every field is
/// a handle wgpu shares internally, so a clone costs a reference count.
#[derive(Clone)]
pub struct Upload {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    lift_layout: wgpu::BindGroupLayout,
    blank_marks: wgpu::TextureView,
    capabilities: Capabilities,
}

impl Upload {
    /// Repacks `image` into a format the device can filter and copies it
    /// across. The result draws nothing until [`ImageLayer::install`] takes
    /// it, which is what lets this run while another image is on screen.
    pub fn run(&self, image: &DecodedImage) -> Result<GpuImage> {
        let limit = self.device.limits().max_texture_dimension_2d;
        if image.width > limit || image.height > limit {
            return Err(anyhow!(
                "{}x{} exceeds this GPU's {limit}x{limit} texture limit",
                image.width,
                image.height
            ));
        }

        let plan = upload::plan(image, self.capabilities);

        let size = wgpu::Extent3d {
            width: image.width,
            height: image.height,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("image"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: plan.format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        self.fill(&texture, &plan, image.width, image.height)?;

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bindings = vec![binding(
            &self.device,
            &self.layout,
            &view,
            &self.blank_marks,
        )];
        let lift = match &image.gain_map {
            Some(map) => Some(Lift::upload(self, map)?),
            None => None,
        };

        Ok(GpuImage {
            size: [image.width, image.height],
            texture,
            view,
            bindings,
            levels: Vec::new(),
            marks: None,
            chain_built: false,
            level_format: reduce::level_format(plan.format),
            swizzle: shader_codes::swizzle(image.channels()),
            alpha: image.alpha,
            primaries: to_columns(image.color.primaries.to_bt709()),
            lift,
            format: plan.format,
            precision_note: plan.precision_note,
        })
    }

    /// Writes `image` into the texture `held` already has, in place of what
    /// is there: the next frame of an animation, whose every frame is the
    /// shape of the first. Answers `false`, and writes nothing, where it is
    /// not that shape — the size differs, or the layout would store as a
    /// different format — which the caller answers with a fresh upload.
    ///
    /// Filling in place rather than allocating is what makes a frame cost a
    /// copy and nothing more. The coarse chain was reduced from the old
    /// pixels and is let go of here; the next minified draw builds it again
    /// from the new ones, as it built the first.
    pub fn refill(&self, held: &mut GpuImage, image: &DecodedImage) -> Result<bool> {
        let plan = upload::plan(image, self.capabilities);
        // A gain map is a picture's own; a frame with one, or one written
        // over a picture with one, is uploaded afresh.
        if held.size != [image.width, image.height]
            || held.format != plan.format
            || held.lift.is_some()
            || image.gain_map.is_some()
        {
            return Ok(false);
        }
        self.fill(&held.texture, &plan, image.width, image.height)?;
        held.levels.clear();
        held.bindings.truncate(1);
        held.chain_built = false;
        // Judged on the old frame's texels. The binding kept still names the
        // old marks, and is not read for them until `prepare` has written
        // new ones and bound those.
        held.marks = None;
        held.swizzle = shader_codes::swizzle(image.channels());
        held.alpha = image.alpha;
        held.primaries = to_columns(image.color.primaries.to_bt709());
        Ok(true)
    }

    /// Copies `plan`'s bytes into `texture`, which is `width` by `height`
    /// of `plan.format`.
    fn fill(
        &self,
        texture: &wgpu::Texture,
        plan: &upload::Plan<'_>,
        width: u32,
        height: u32,
    ) -> Result<()> {
        // The bytes are copied into staging here and reach the texture at the
        // next submit, whichever thread makes it. That is always a later one
        // than this: the image is installed before it is ever drawn from.
        //
        // Written in horizontal bands rather than in one call. `write_texture`
        // stages a buffer the size of the copy, so a single call for a
        // multi-gigabyte image would ask the driver for a multi-gigabyte
        // mappable buffer — over `max_buffer_size` on most devices, and a
        // needless allocation spike even where it fits. A band is bounded no
        // matter how large the image. The error scope turns a driver refusal
        // (out of memory, or a size still past a limit) into an error the
        // loader walks past, rather than the uncaptured-error panic it would
        // otherwise be.
        let oom_scope = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let validation_scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);

        let bytes = plan.pixels.as_bytes();
        let row = plan.bytes_per_row as usize;
        let rows_per_band = (BAND_BYTES / row.max(1)).clamp(1, height as usize);
        let mut y = 0u32;
        while y < height {
            let band = rows_per_band.min((height - y) as usize) as u32;
            let start = y as usize * row;
            let end = start + band as usize * row;
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: 0, y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &bytes[start..end],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(plan.bytes_per_row),
                    rows_per_image: Some(band),
                },
                wgpu::Extent3d {
                    width,
                    height: band,
                    depth_or_array_layers: 1,
                },
            );
            y += band;
        }

        // Popped in reverse order to creation, as the scope stack requires.
        let validation = pollster::block_on(validation_scope.pop());
        let out_of_memory = pollster::block_on(oom_scope.pop());
        if let Some(error) = validation {
            return Err(anyhow!("the GPU rejected the image: {error}"));
        }
        if let Some(error) = out_of_memory {
            return Err(anyhow!("not enough GPU memory for the image: {error}"));
        }
        Ok(())
    }
}

impl Slot {
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, label: &str) -> Self {
        let buffer = gpu::uniform_buffer::<Params>(device, label);
        let group = gpu::buffer_group(device, label, layout, &buffer);
        Self { buffer, group }
    }

    fn write(&self, queue: &wgpu::Queue, params: Params) {
        queue.write_buffer(&self.buffer, 0, bytemuck::bytes_of(&params));
    }
}

/// How much a draw has to shrink the image by, in source texels per output
/// pixel. Below one it is magnifying and reads the image itself; above it,
/// the coarse chain does everything past a factor of four so that the
/// filter's tap count stays small.
fn shrink(placement: Placement) -> f32 {
    if placement.zoom > 0.0 {
        1.0 / placement.zoom
    } else {
        1.0
    }
}

/// What every quad of one frame shares: the target they are drawn on, and
/// the display, marks, lift and turn they are all drawn under.
#[derive(Clone, Copy)]
struct Quad<'a> {
    target: [f32; 2],
    display: &'a Display,
    window: (f32, f32),
    marks: u32,
    lifted: Lifted<'a>,
    turn: Turn,
}

/// Which part of the loupe's circle a quad draws. The two together are
/// the glass; which pipeline draws each is `ImageLayer::render`'s business.
/// Twinned by `clip.w` in `shaders/image.wgsl`.
#[derive(Clone, Copy)]
enum Cut {
    /// The disc, to a pixel short of the edge: hard-cut, and replacing.
    Disc,
    /// The last pixel of the circle, feathered at the edge: blending.
    Band,
}

/// The constants for one quad: where it goes on the target, and how the
/// shader is to read and resample the level it draws from. `placement` is
/// where the image lands; `glass`, for the loupe's quads, is the circle the
/// quad is cut to and which part of it this quad is, and the quad itself is
/// then the circle's square rather than the image, which runs far past it.
fn params_for(
    image: &GpuImage,
    placement: Placement,
    glass: Option<(Glass, Cut)>,
    level: usize,
    quad: Quad<'_>,
) -> Params {
    let Quad {
        target,
        display,
        window,
        marks,
        lifted,
        turn,
    } = quad;
    let corners = glass.map_or(placement, |(glass, _)| glass.bounds());
    let divisor = (reduce::STEP as f32).powi(level as i32);
    let extent = [
        image.size[0] as f32 / divisor,
        image.size[1] as f32 / divisor,
    ];
    // Read off the quad rather than from the zoom, so that the filters and
    // the geometry cannot drift apart. Each of the texture's axes against
    // the side of the quad it runs along: a picture on its side runs its
    // columns down the screen.
    let [across, down] = turn.size([placement.width, placement.height]);
    let texels_per_pixel = [extent[0] / across.max(1e-6), extent[1] / down.max(1e-6)];

    Params {
        offset: [
            corners.x / target[0] * 2.0 - 1.0,
            1.0 - corners.y / target[1] * 2.0,
        ],
        scale: [
            corners.width / target[0] * 2.0,
            corners.height / target[1] * 2.0,
        ],
        window: [window.0, window.1],
        texels_per_pixel,
        extent,
        marks,
        turn: shader_codes::turn(turn),
        primaries: image.primaries,
        swizzle: image.swizzle,
        alpha_mode: if level == 0 {
            shader_codes::alpha(image.alpha)
        } else {
            shader_codes::level_alpha(image.alpha)
        },
        colormap: shader_codes::colormap(display.colormap()),
        resampler: shader_codes::resampler(placement.zoom, placement.upscale),
        // Only the image as uploaded is lifted; a coarse level was reduced
        // from lifted light.
        lift: if level == 0 { lifted.code } else { 0 },
        level: level as u32,
        map_size: lifted.map_size,
        base_offset: lifted.base_offset,
        alternate_offset: lifted.alternate_offset,
        clip: glass.map_or([0.0; 4], |(glass, cut)| {
            let cut = match cut {
                Cut::Disc => 0.0,
                Cut::Band => 1.0,
            };
            [glass.center[0], glass.center[1], glass.radius, cut]
        }),
        picture: [placement.x, placement.y, placement.width, placement.height],
    }
}

/// The picture at one level, beside the marks on it at the same level.
fn binding(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
    marks: &wgpu::TextureView,
) -> wgpu::BindGroup {
    gpu::texture_group(device, "image texture", layout, &[view, marks])
}

/// Every level's bind group made again: the picture at that level beside
/// the marks at it, or beside the blank texel — at level 0 always, whose
/// marks are judged live, and at every level while the marks are off. Done
/// whenever either chain changes, since a bind group names both.
fn rebind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    blank_marks: &wgpu::TextureView,
    image: &mut GpuImage,
) {
    let marks = image.marks.as_ref();
    let picture = std::iter::once(&image.view).chain(image.levels.iter().map(|level| &level.view));
    image.bindings = picture
        .enumerate()
        .map(|(level, view)| {
            let mark = marks
                .and_then(|marks| marks.at(level))
                .unwrap_or(blank_marks);
            binding(device, layout, view, mark)
        })
        .collect();
}

/// What writing the marks takes from the layer, so that it can be done
/// while the picture is borrowed from it.
struct Marking<'a> {
    pipeline: &'a wgpu::RenderPipeline,
    slot: &'a Slot,
    reducer: &'a mut Reducer,
    layout: &'a wgpu::BindGroupLayout,
    blank_lift: &'a wgpu::BindGroup,
    blank_marks: &'a wgpu::TextureView,
    ramps: &'a wgpu::BindGroup,
}

/// Writes the marks' chain for `image` under `key`: the first level through
/// `fs_marks`, which judges every texel of the picture as uploaded and
/// writes each block's shares — a `STEP` by `STEP` block to the pixel, so
/// `texels_per_pixel` is the block, and a target the size of the picture
/// over `STEP` so the quad fills the level's texture whatever it rounds to —
/// and the rest reduced from it as the picture's chain is from the picture:
/// three straight channels, the third empty, no lift, the marks being judged
/// on lifted light already, and the first level's own extent, so that its
/// last row and column weigh what they stand for.
fn write_marks(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    marking: Marking<'_>,
    image: &mut GpuImage,
    display: &Display,
    key: MarksKey,
) {
    let size = image.size;
    let step = reduce::STEP as f32;
    let target = [size[0] as f32 / step, size[1] as f32 / step];
    let lifted = Lifted::of(image.lift.as_ref(), marking.blank_lift);
    marking.slot.write(
        queue,
        params_for(
            image,
            Placement {
                x: 0.0,
                y: 0.0,
                width: target[0],
                height: target[1],
                zoom: 1.0,
                upscale: Upscale::Nearest,
            },
            None,
            0,
            Quad {
                target,
                display,
                window: key.window,
                marks: 0,
                lifted,
                turn: Turn::NONE,
            },
        ),
    );

    if image.marks.is_none() {
        image.marks = Some(Marks::new(
            device,
            marking.layout,
            &image.view,
            marking.blank_marks,
            size,
            key,
        ));
    }
    let Some(marks) = &mut image.marks else {
        return;
    };
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("marks"),
            color_attachments: &[Some(gpu::attachment(
                &marks.first,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            ))],
            ..Default::default()
        });
        pass.set_pipeline(marking.pipeline);
        pass.set_bind_group(0, &marking.slot.group, &[]);
        pass.set_bind_group(1, &marks.reading, &[]);
        pass.set_bind_group(2, lifted.group, &[]);
        pass.set_bind_group(3, marking.ramps, &[]);
        pass.draw(0..4, 0..1);
    }
    marks.rest = marking.reducer.build(
        device,
        encoder,
        reduce::Source {
            view: &marks.first,
            size: [
                size[0].div_ceil(reduce::STEP).max(1),
                size[1].div_ceil(reduce::STEP).max(1),
            ],
            extent: Some(target),
            format: MARKS_FORMAT,
            swizzle: shader_codes::swizzle(Channels::Rgb),
            alpha: AlphaMode::Opaque,
            lift: Lifted::of(None, marking.blank_lift),
        },
    );
    marks.key = key;
}

/// WGSL matrices are column-major with 16-byte column stride, while
/// `Primaries::to_bt709` is written out in rows for readability.
fn to_columns(rows: [[f32; 3]; 3]) -> [[f32; 4]; 3] {
    let mut columns = [[0.0f32; 4]; 3];
    for (column_index, column) in columns.iter_mut().enumerate() {
        for (row_index, row) in rows.iter().enumerate() {
            column[row_index] = row[column_index];
        }
    }
    columns
}
