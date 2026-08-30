//! Draws the image into the linear working-space target.
//!
//! Everything colour-related that varies per frame lives in one uniform, so
//! changing exposure, the window or the colormap costs a buffer write rather
//! than a re-decode. Resampling is chosen the same way: which filter to run
//! and which level of the coarse chain to read are two more fields in it.

use anyhow::{Result, anyhow};
use bytemuck::{Pod, Zeroable};

use super::gpu::{self, Fullscreen};
use super::placement::Placement;
use super::reduce::{self, Level, Reducer};
use super::shader_codes;
use super::upload::{self, Capabilities};
use crate::image::{AlphaMode, DecodedImage, display::Display};

/// Layout must match `struct Params` in shaders/image.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    offset: [f32; 2],
    scale: [f32; 2],
    window: [f32; 2],
    texels_per_pixel: [f32; 2],
    extent: [f32; 2],
    _pad: [f32; 2],
    /// Column-major, each column padded to 16 bytes, as WGSL wants a mat3x3.
    primaries: [[f32; 4]; 3],
    swizzle: u32,
    alpha_mode: u32,
    colormap: u32,
    resampler: u32,
}

/// One uploaded image and the constants that describe it.
pub struct GpuImage {
    size: [u32; 2],
    view: wgpu::TextureView,
    /// Bind groups indexed by level: 0 is the image as uploaded, the rest are
    /// the coarse chain. Empty past the first until a view zooms out far
    /// enough to want it, since most never do.
    bindings: Vec<wgpu::BindGroup>,
    levels: Vec<Level>,
    /// Set once the chain has been built, which is not the same as its being
    /// non-empty: an image only a few texels across has no levels to make.
    chain_built: bool,
    level_format: wgpu::TextureFormat,
    swizzle: u32,
    alpha: AlphaMode,
    primaries: [[f32; 4]; 3],
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

/// What one frame draws: the view, and the minimap's thumbnail when it is on
/// screen. Passed together because they share a pass, a texture and a coarse
/// chain, and because whether the chain is needed at all is a question about
/// the pair of them.
#[derive(Clone, Copy)]
pub struct Draw {
    pub view: Placement,
    pub thumbnail: Option<Placement>,
}

pub struct ImageLayer {
    pipeline: wgpu::RenderPipeline,
    texture_layout: wgpu::BindGroupLayout,
    main: Slot,
    thumbnail: Slot,
    reducer: Reducer,
    image: Option<GpuImage>,
    /// Which of the current image's bind groups the next draw reads, decided
    /// in `prepare` from the zoom.
    level: usize,
    /// The same for the thumbnail, and `None` on a frame with no minimap on
    /// screen, which is what leaves its quad undrawn.
    thumbnail_level: Option<usize>,
}

impl ImageLayer {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("image layer"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/image.wgsl").into()),
        });

        let params_layout =
            gpu::uniform_layout(device, "image params", wgpu::ShaderStages::VERTEX_FRAGMENT);
        // No sampler: the shader loads texels and weights them itself, which
        // is what lets one pipeline serve an area filter, an antialiased
        // nearest and a bicubic. Every format `upload::plan` can produce is
        // filterable all the same, and so is every format `reduce` writes.
        let texture_layout = gpu::texture_layout(device, "image texture", 1, true);
        let pipeline_layout =
            gpu::pipeline_layout(device, "image layer", &[&params_layout, &texture_layout]);
        let pipeline = gpu::fullscreen_pipeline(
            device,
            Fullscreen {
                label: "image layer",
                shader: &shader,
                layout: &pipeline_layout,
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                format: target_format,
                // The shader emits premultiplied colour.
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            },
        );

        Self {
            pipeline,
            texture_layout,
            main: Slot::new(device, &params_layout, "image params"),
            thumbnail: Slot::new(device, &params_layout, "thumbnail params"),
            reducer: Reducer::new(device),
            image: None,
            level: 0,
            thumbnail_level: None,
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
    }

    /// `draw.thumbnail`, when there is one, is the minimap's copy of the same
    /// image: a second quad, drawn from the same texture in the same pass, so
    /// that it is tone mapped and windowed exactly as the image it stands for.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        draw: Draw,
        target: [f32; 2],
        display: &Display,
    ) {
        let Draw { view, thumbnail } = draw;
        let Some(image) = &mut self.image else {
            return;
        };
        let window = display.transform();

        let factor = shrink(view);
        // The thumbnail is shrunk far harder than the view ever is, so it is
        // what decides whether the chain is needed at all.
        let coarsest = thumbnail.map_or(factor, |thumbnail| factor.max(shrink(thumbnail)));
        if coarsest > reduce::STEP as f32 && !image.chain_built {
            image.levels = self.reducer.build(
                device,
                encoder,
                reduce::Source {
                    view: &image.view,
                    size: image.size,
                    format: image.level_format,
                    swizzle: image.swizzle,
                    alpha: image.alpha,
                },
            );
            for level in &image.levels {
                image
                    .bindings
                    .push(binding(device, &self.texture_layout, &level.view));
            }
            image.chain_built = true;
        }

        self.level = reduce::level_for(factor, image.levels.len());
        self.main.write(
            queue,
            params_for(image, view, target, display, window, self.level),
        );

        self.thumbnail_level =
            thumbnail.map(|thumbnail| reduce::level_for(shrink(thumbnail), image.levels.len()));
        if let (Some(thumbnail), Some(level)) = (thumbnail, self.thumbnail_level) {
            self.thumbnail.write(
                queue,
                params_for(image, thumbnail, target, display, window, level),
            );
        }
    }

    pub fn render(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(image) = &self.image else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        // The thumbnail goes down second: it sits over the content area, and
        // a view zoomed in past the panels' edges is drawn under it.
        let draws = [
            Some((&self.main, self.level)),
            self.thumbnail_level.map(|level| (&self.thumbnail, level)),
        ];
        for (slot, level) in draws.into_iter().flatten() {
            let Some(binding) = image.bindings.get(level) else {
                continue;
            };
            pass.set_bind_group(0, &slot.group, &[]);
            pass.set_bind_group(1, binding, &[]);
            pass.draw(0..4, 0..1);
        }
    }
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
        let rows_per_band = (BAND_BYTES / row.max(1)).clamp(1, image.height as usize);
        let mut y = 0u32;
        while y < image.height {
            let band = rows_per_band.min((image.height - y) as usize) as u32;
            let start = y as usize * row;
            let end = start + band as usize * row;
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
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
                    width: image.width,
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

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bindings = vec![binding(&self.device, &self.layout, &view)];

        Ok(GpuImage {
            size: [image.width, image.height],
            view,
            bindings,
            levels: Vec::new(),
            chain_built: false,
            level_format: reduce::level_format(plan.format),
            swizzle: shader_codes::swizzle(image.channels()),
            alpha: image.alpha,
            primaries: to_columns(image.color.primaries.to_bt709()),
            format: plan.format,
            precision_note: plan.precision_note,
        })
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

/// The constants for one quad: where it goes on the target, and how the
/// shader is to read and resample the level it draws from.
fn params_for(
    image: &GpuImage,
    placement: Placement,
    target: [f32; 2],
    display: &Display,
    window: (f32, f32),
    level: usize,
) -> Params {
    let divisor = (reduce::STEP as f32).powi(level as i32);
    let extent = [
        image.size[0] as f32 / divisor,
        image.size[1] as f32 / divisor,
    ];
    // Read off the quad rather than from the zoom, so that the filters and
    // the geometry cannot drift apart.
    let texels_per_pixel = [
        extent[0] / placement.width.max(1e-6),
        extent[1] / placement.height.max(1e-6),
    ];

    Params {
        offset: [
            placement.x / target[0] * 2.0 - 1.0,
            1.0 - placement.y / target[1] * 2.0,
        ],
        scale: [
            placement.width / target[0] * 2.0,
            placement.height / target[1] * 2.0,
        ],
        window: [window.0, window.1],
        texels_per_pixel,
        extent,
        _pad: [0.0; 2],
        primaries: image.primaries,
        swizzle: image.swizzle,
        alpha_mode: if level == 0 {
            shader_codes::alpha(image.alpha)
        } else {
            shader_codes::level_alpha(image.alpha)
        },
        colormap: shader_codes::colormap(display.colormap),
        resampler: shader_codes::resampler(placement.zoom, placement.upscale),
    }
}

fn binding(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    gpu::texture_group(device, "image texture", layout, &[view])
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
