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

use super::upload::alpha_code;

/// Size ratio between one level and the next, per axis.
pub const STEP: u32 = 4;

/// Layout must match `struct Params` in shaders/reduce.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    extent: [f32; 2],
    step: f32,
    swizzle: u32,
    alpha_mode: u32,
    _pad: [u32; 3],
}

/// What a chain is built from: the image as uploaded, plus what the shader
/// needs to know to read it.
pub struct Source<'a> {
    pub view: &'a wgpu::TextureView,
    pub size: [u32; 2],
    /// What the levels themselves are stored in, from `level_format`.
    pub format: wgpu::TextureFormat,
    pub swizzle: u32,
    pub alpha: AlphaMode,
}

/// One level, kept alongside its texture so that dropping the chain frees it.
pub struct Level {
    _texture: wgpu::Texture,
    pub view: wgpu::TextureView,
}

pub struct Reducer {
    shader: wgpu::ShaderModule,
    params_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    /// One per target format, since a render pipeline is tied to one. There
    /// are only a handful of formats `level_format` can name, so this settles
    /// after the first few images.
    pipelines: Vec<(wgpu::TextureFormat, wgpu::RenderPipeline)>,
}

impl Reducer {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("reduce"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/reduce.wgsl").into()),
        });

        let params_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("reduce params"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("reduce source"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("reduce"),
            bind_group_layouts: &[Some(&params_layout), Some(&texture_layout)],
            immediate_size: 0,
        });

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
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("reduce"),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        self.pipelines.push((format, pipeline));
        self.pipelines.len() - 1
    }

    /// Reduces `source` all the way down to a single texel, returning the
    /// levels largest first. Level `k` of the result stands for a reduction of
    /// `STEP.pow(k + 1)` per axis.
    pub fn build(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: Source<'_>,
    ) -> Vec<Level> {
        let Source {
            view: input,
            size,
            format,
            swizzle,
            alpha,
        } = source;
        let pipeline = self.pipeline(device, format);
        let mut levels: Vec<Level> = Vec::new();

        let mut divisor = 1u32;
        while size[0].div_ceil(divisor) > 1 || size[1].div_ceil(divisor) > 1 {
            // The extent a level covers is the image divided by the reduction
            // so far, which stops being a whole number as soon as the image
            // size is not a multiple of STEP. Sizes round up, so the last texel
            // of a row is a partial one and the shader weights it accordingly.
            let extent = [
                size[0] as f32 / divisor as f32,
                size[1] as f32 / divisor as f32,
            ];
            divisor *= STEP;
            let width = size[0].div_ceil(divisor).max(1);
            let height = size[1].div_ceil(divisor).max(1);

            let params = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("reduce params"),
                size: size_of::<Params>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: true,
            });
            params
                .slice(..)
                .get_mapped_range_mut()
                .expect("a buffer mapped at creation is always mappable")
                .copy_from_slice(bytemuck::bytes_of(&Params {
                    extent,
                    step: STEP as f32,
                    swizzle,
                    // Only the first pass reads the image as it was uploaded;
                    // every level it writes is premultiplied already.
                    alpha_mode: if levels.is_empty() {
                        alpha_code(alpha)
                    } else {
                        level_alpha_code(alpha)
                    },
                    _pad: [0; 3],
                }));
            params.unmap();

            let params_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("reduce params"),
                layout: &self.params_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params.as_entire_binding(),
                }],
            });

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
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

            {
                let input = levels.last().map_or(input, |level| &level.view);
                let texture_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("reduce source"),
                    layout: &self.texture_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(input),
                    }],
                });

                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("reduce"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                pass.set_pipeline(&self.pipelines[pipeline].1);
                pass.set_bind_group(0, &params_group, &[]);
                pass.set_bind_group(1, &texture_group, &[]);
                pass.draw(0..4, 0..1);
            }

            levels.push(Level {
                _texture: texture,
                view,
            });
        }
        levels
    }
}

/// The format the chain is stored in.
///
/// Float, so that a level holds linear light with no transfer function to
/// think about, and premultiplied colour without an 8-bit floor under it. Half
/// floats everywhere except above a 32-bit float source, where the range and
/// the low bits are the point of the file.
pub fn level_format(source: wgpu::TextureFormat) -> wgpu::TextureFormat {
    use wgpu::TextureFormat as F;
    match source {
        F::R32Float => F::R32Float,
        F::Rg32Float => F::Rg32Float,
        F::Rgba32Float => F::Rgba32Float,
        F::R8Unorm | F::R16Unorm | F::R16Float => F::R16Float,
        F::Rg8Unorm | F::Rg16Unorm | F::Rg16Float => F::Rg16Float,
        _ => F::Rgba16Float,
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

/// What a coarse level holds. Straight alpha has been multiplied through on
/// the way in; an image whose alpha channel is meaningless keeps it that way,
/// since dividing the colour back out by it would be nonsense.
pub fn level_alpha_code(alpha: AlphaMode) -> u32 {
    match alpha {
        AlphaMode::Opaque => 0,
        AlphaMode::Straight | AlphaMode::Premultiplied => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
