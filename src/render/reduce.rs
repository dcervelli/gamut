//! The coarse chain a minifying draw reads from.
//!
//! An exact area filter reads every source texel the window covers, whatever
//! the zoom: pleasant at twenty-four megapixels, gigabytes a frame at the
//! texture size limit, and paid again on every wheel notch. Each level here is
//! an exact 4x4 area average of the level above it, so a draw can start from
//! one within a factor of four of the size it wants and read at most sixteen
//! texels per output pixel however far out the view is.
//!
//! Four per axis rather than a mipmap's two is what makes that cheap: levels
//! shrink by sixteen in area, so the whole chain is a fifteenth of the image's
//! own texture where a mip chain is a third of it. It is built the first time
//! a view zooms out far enough to want it, and thrown away with the image.

use bytemuck::{Pod, Zeroable};

use crate::image::AlphaMode;

use super::gpu::{self, Fullscreen};
use super::image_layer::Lifted;
use super::shader_codes;

/// Size ratio between one level and the next, per axis.
pub const STEP: u32 = 4;

/// Layout must match `struct Params` in shaders/reduce.wgsl, which the
/// test at the foot of this file holds it to.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Pod, Zeroable)]
struct Params {
    extent: [f32; 2],
    step: f32,
    swizzle: u32,
    alpha_mode: u32,
    lift: u32,
    map_size: [f32; 2],
    base_offset: [f32; 4],
    alternate_offset: [f32; 4],
}

/// What a chain is built from: the image as uploaded, plus what the shader
/// needs to know to read it.
pub struct Source<'a> {
    pub view: &'a wgpu::TextureView,
    pub size: [u32; 2],
    /// How much of the texture the picture occupies, in its texels, where
    /// that is not the whole of it: a chain begun from a level of another
    /// chain, whose last row and column stand for partial blocks and are
    /// weighted as such. `None` for a texture the picture fills.
    pub extent: Option<[f32; 2]>,
    /// What the levels themselves are stored in: `Layout::level_format` for
    /// a picture's chain, the marks' own format for theirs.
    pub format: wgpu::TextureFormat,
    pub swizzle: u32,
    pub alpha: AlphaMode,
    /// The gain map the first pass lifts the image through, where it has
    /// one, so that every level holds lifted light. The levels after the
    /// first read light already lifted.
    pub lift: Lifted<'a>,
}

/// One level: its texture, and everything the pass that writes it was
/// recorded with, kept so that the level can be written again — under
/// another window, say — without any of it being made again. Dropping the
/// chain frees the lot.
pub struct Level {
    texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    params: wgpu::Buffer,
    /// What `params` holds, so that a level written again under the same
    /// constants stages no copy for them.
    written: Params,
    params_group: wgpu::BindGroup,
    /// What the pass reads: the source, or the level above.
    input: wgpu::TextureView,
    input_group: wgpu::BindGroup,
}

impl Level {
    /// Whether a texture of this size and format is what the level has.
    fn holds(&self, width: u32, height: u32, format: wgpu::TextureFormat) -> bool {
        self.texture.width() == width
            && self.texture.height() == height
            && self.texture.format() == format
    }
}

pub struct Reducer {
    shader: wgpu::ShaderModule,
    params_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    /// One per target format, since a render pipeline is tied to one. There
    /// are only a handful of formats `Layout::level_format` can name, so
    /// this settles after the first few images.
    pipelines: Vec<(wgpu::TextureFormat, wgpu::RenderPipeline)>,
}

impl Reducer {
    /// `lift_layout` is the image layer's binding for a gain map and its
    /// table, which the first pass reads through as the draw does.
    pub fn new(device: &wgpu::Device, lift_layout: &wgpu::BindGroupLayout) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("reduce"),
            source: wgpu::ShaderSource::Wgsl(super::REDUCE_SHADER.into()),
        });

        let params_layout =
            gpu::uniform_layout(device, "reduce params", wgpu::ShaderStages::FRAGMENT);
        let texture_layout = gpu::texture_layout(device, "reduce source", 1, true);
        let pipeline_layout = gpu::pipeline_layout(
            device,
            "reduce",
            &[&params_layout, &texture_layout, lift_layout],
        );

        Self {
            shader,
            params_layout,
            texture_layout,
            pipeline_layout,
            pipelines: Vec::new(),
        }
    }

    fn pipeline(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat) -> usize {
        if let Some(index) = self
            .pipelines
            .iter()
            .position(|(existing, _)| *existing == format)
        {
            return index;
        }
        let pipeline = gpu::fullscreen_pipeline(
            device,
            Fullscreen {
                label: "reduce",
                shader: &self.shader,
                layout: &self.pipeline_layout,
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                format,
                blend: None,
            },
        );
        self.pipelines.push((format, pipeline));
        self.pipelines.len() - 1
    }

    /// Reduces `source` all the way down to a single texel, returning the
    /// levels largest first. Level `k` of the result stands for a reduction of
    /// `STEP.pow(k + 1)` per axis.
    pub fn build(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        source: Source<'_>,
    ) -> Vec<Level> {
        let mut levels = Vec::new();
        self.build_into(device, queue, encoder, source, &mut levels);
        levels
    }

    /// As [`Reducer::build`], into `levels`, which may hold a chain built
    /// before: a level whose texture is already the size and format this
    /// chain wants is written again rather than made again — its constants
    /// rewritten and its input rebound only where they differ — and the
    /// rest are made, or let go. Answers whether the set of textures
    /// changed, which is what a bind group naming them has to know.
    ///
    /// For the marks' chain, which is written again under every window as
    /// the exposure is stepped: same size every time, so nothing is
    /// allocated per step.
    pub fn build_into(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        source: Source<'_>,
        levels: &mut Vec<Level>,
    ) -> bool {
        let Source {
            view: source,
            size,
            extent: occupied,
            format,
            swizzle,
            alpha,
            lift,
        } = source;
        let occupied = occupied.unwrap_or([size[0] as f32, size[1] as f32]);
        let pipeline = self.pipeline(device, format);
        let mut changed = false;

        let mut index = 0;
        let mut divisor = 1u32;
        while size[0].div_ceil(divisor) > 1 || size[1].div_ceil(divisor) > 1 {
            // The extent a level covers is the image divided by the reduction
            // so far, which stops being a whole number as soon as the image
            // size is not a multiple of STEP. Sizes round up, so the last texel
            // of a row is a partial one and the shader weights it accordingly.
            let extent = [occupied[0] / divisor as f32, occupied[1] / divisor as f32];
            divisor *= STEP;
            let width = size[0].div_ceil(divisor).max(1);
            let height = size[1].div_ceil(divisor).max(1);

            let params = Params {
                extent,
                step: STEP as f32,
                swizzle,
                // Only the first pass reads the image as it was uploaded;
                // every level it writes is premultiplied already, and
                // lifted already.
                alpha_mode: if index == 0 {
                    shader_codes::alpha(alpha)
                } else {
                    shader_codes::level_alpha(alpha)
                },
                lift: if index == 0 { lift.code } else { 0 },
                map_size: lift.map_size,
                base_offset: lift.base_offset,
                alternate_offset: lift.alternate_offset,
            };
            let input = if index == 0 {
                source.clone()
            } else {
                levels[index - 1].view.clone()
            };

            match levels.get_mut(index) {
                Some(level) if level.holds(width, height, format) => {
                    if level.written != params {
                        queue.write_buffer(&level.params, 0, bytemuck::bytes_of(&params));
                        level.written = params;
                    }
                    if level.input != input {
                        level.input_group = self.input_group(device, &input);
                        level.input = input;
                    }
                }
                kept => {
                    let level = self.level(device, width, height, format, params, input);
                    match kept {
                        Some(kept) => *kept = level,
                        None => levels.push(level),
                    }
                    changed = true;
                }
            }

            let level = &levels[index];
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("reduce"),
                color_attachments: &[Some(gpu::attachment(
                    &level.view,
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                ))],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipelines[pipeline].1);
            pass.set_bind_group(0, &level.params_group, &[]);
            pass.set_bind_group(1, &level.input_group, &[]);
            pass.set_bind_group(2, lift.group, &[]);
            pass.draw(0..4, 0..1);
            drop(pass);

            index += 1;
        }
        if levels.len() > index {
            levels.truncate(index);
            changed = true;
        }
        changed
    }

    /// The binding a pass reads `input` through.
    fn input_group(&self, device: &wgpu::Device, input: &wgpu::TextureView) -> wgpu::BindGroup {
        gpu::texture_group(device, "reduce source", &self.texture_layout, &[input])
    }

    /// A level made afresh: a texture of `width` by `height` in `format`,
    /// and the pass's constants and bindings.
    fn level(
        &self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        params: Params,
        input: wgpu::TextureView,
    ) -> Level {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("reduce params"),
            size: size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: true,
        });
        buffer
            .slice(..)
            .get_mapped_range_mut()
            .expect("a buffer mapped at creation is always mappable")
            .copy_from_slice(bytemuck::bytes_of(&params));
        buffer.unmap();
        let params_group = gpu::buffer_group(device, "reduce params", &self.params_layout, &buffer);

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("coarse level"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let input_group = self.input_group(device, &input);

        Level {
            texture,
            view,
            params: buffer,
            written: params,
            params_group,
            input,
            input_group,
        }
    }
}

/// The level a view minifying by `factor` per axis should read.
///
/// Level 0 is the image as uploaded, and each one below it divides by `STEP`,
/// so what is left for the draw's own filter to do is always under `STEP` —
/// sixteen texels an output pixel at worst — until the chain runs out on a
/// very small image, where there is nothing left to alias anyway.
pub fn level_for(factor: f32, available: usize) -> usize {
    if !factor.is_finite() || factor <= 1.0 {
        return 0;
    }
    let level = (factor.log2() / (STEP as f32).log2()).floor().max(0.0) as usize;
    level.min(available)
}

#[cfg(test)]
mod tests {
    use super::super::shader_codes::wgsl;
    use super::*;

    /// The uniform is laid out as the shader reads it: every member at the
    /// offset WGSL gives it, and the two the same size.
    #[test]
    fn the_params_are_laid_out_as_the_shader_reads_them() {
        wgsl::assert_params_match(
            super::super::REDUCE_SHADER,
            wgsl::fields!(Params:
                extent, step, swizzle, alpha_mode, lift, map_size, base_offset, alternate_offset
            ),
            size_of::<Params>(),
        );
    }

    /// What the draw is left to do once it has picked a level.
    fn remainder(factor: f32, level: usize) -> f32 {
        factor / (STEP as f32).powi(level as i32)
    }

    #[test]
    fn a_magnifying_view_reads_the_image_itself() {
        assert_eq!(level_for(0.25, 4), 0);
        assert_eq!(level_for(1.0, 4), 0);
    }

    /// The property the whole chain exists for: however far out the view is
    /// zoomed, the draw is left with at most `STEP` texels per pixel per axis.
    #[test]
    fn the_remainder_never_exceeds_one_step() {
        let mut factor = 1.0;
        while factor < 4096.0 {
            let level = level_for(factor, 8);
            let left = remainder(factor, level);
            assert!(
                (1.0..=STEP as f32 + 1e-3).contains(&left),
                "factor {factor} left {left} at level {level}"
            );
            factor *= 1.05;
        }
    }

    #[test]
    fn a_chain_that_runs_out_hands_the_rest_to_the_draw() {
        // Two levels cover a factor of sixteen; the rest is the draw's.
        let level = level_for(64.0, 2);
        assert_eq!(level, 2);
        assert!((remainder(64.0, level) - 4.0).abs() < 1e-3);
    }

    /// The reason the step is four rather than a mipmap's two: at four, the
    /// whole chain is a fifteenth of the image it describes, where a mip chain
    /// is a third of it — and it still leaves the draw no more than sixteen
    /// texels a pixel to average.
    #[test]
    fn the_chain_costs_a_fifteenth_of_the_image() {
        // The largest image any common GPU will hold at all.
        let (width, height) = (16384u64, 16384u64);
        let mut divisor = 1u64;
        let mut texels = 0u64;
        loop {
            divisor *= STEP as u64;
            let level = (
                width.div_ceil(divisor).max(1),
                height.div_ceil(divisor).max(1),
            );
            texels += level.0 * level.1;
            if level == (1, 1) {
                break;
            }
        }
        let source = width * height;
        assert!(
            texels < source / 14,
            "chain of {texels} texels against an image of {source}"
        );
    }

    #[test]
    fn levels_stand_for_whole_powers_of_the_step() {
        assert_eq!(level_for(4.0, 8), 1);
        assert_eq!(level_for(15.9, 8), 1);
        assert_eq!(level_for(16.0, 8), 2);
    }
}
