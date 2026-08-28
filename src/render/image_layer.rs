//! Draws the image into the linear working-space target.
//!
//! Everything colour-related that varies per frame lives in one uniform, so
//! changing exposure, the window or the colormap costs a buffer write rather
//! than a re-decode.

use anyhow::Result;
use bytemuck::{Pod, Zeroable};

use super::upload::{self, Capabilities};
use crate::image::{DecodedImage, display::Display};
use crate::view::Placement;

/// Layout must match `struct Params` in shaders/image.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    offset: [f32; 2],
    scale: [f32; 2],
    window: [f32; 2],
    _pad: [f32; 2],
    /// Column-major, each column padded to 16 bytes, as WGSL wants a mat3x3.
    primaries: [[f32; 4]; 3],
    swizzle: u32,
    alpha_mode: u32,
    colormap: u32,
    _pad2: u32,
}

/// One uploaded image and the constants that describe it.
pub struct GpuImage {
    nearest: wgpu::BindGroup,
    linear: wgpu::BindGroup,
    swizzle: u32,
    alpha_mode: u32,
    primaries: [[f32; 4]; 3],
    pub format: wgpu::TextureFormat,
    pub precision_note: Option<&'static str>,
}

pub struct ImageLayer {
    pipeline: wgpu::RenderPipeline,
    texture_layout: wgpu::BindGroupLayout,
    params: wgpu::Buffer,
    params_group: wgpu::BindGroup,
    nearest_sampler: wgpu::Sampler,
    linear_sampler: wgpu::Sampler,
    image: Option<GpuImage>,
}

impl ImageLayer {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("image layer"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/image.wgsl").into()),
        });

        let params_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("image params"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        // Every format `upload::plan` can produce is filterable — that is one
        // of the constraints it works under — so a single layout serves all
        // of them.
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("image texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("image layer"),
            bind_group_layouts: &[Some(&params_layout), Some(&texture_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("image layer"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
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
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    // The shader emits premultiplied colour.
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("image params"),
            size: size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let params_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("image params"),
            layout: &params_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: params.as_entire_binding(),
            }],
        });

        let sampler = |label: &str, filter: wgpu::FilterMode| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some(label),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: filter,
                min_filter: filter,
                ..Default::default()
            })
        };

        Self {
            pipeline,
            texture_layout,
            params,
            params_group,
            nearest_sampler: sampler("nearest", wgpu::FilterMode::Nearest),
            linear_sampler: sampler("linear", wgpu::FilterMode::Linear),
            image: None,
        }
    }

    pub fn current(&self) -> Option<&GpuImage> {
        self.image.as_ref()
    }

    pub fn set_image(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: &DecodedImage,
        capabilities: Capabilities,
    ) -> Result<()> {
        let plan = upload::plan(image, capabilities);

        let size = wgpu::Extent3d {
            width: image.width,
            height: image.height,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("image"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: plan.format,
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
            plan.pixels.as_bytes(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(plan.bytes_per_row),
                rows_per_image: Some(image.height),
            },
            size,
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let group = |label: &str, sampler: &wgpu::Sampler| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &self.texture_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            })
        };

        self.image = Some(GpuImage {
            nearest: group("image nearest", &self.nearest_sampler),
            linear: group("image linear", &self.linear_sampler),
            swizzle: upload::swizzle_code(image.channels()),
            alpha_mode: upload::alpha_code(image.alpha),
            primaries: to_columns(image.color.primaries.to_bt709()),
            format: plan.format,
            precision_note: plan.precision_note,
        });
        Ok(())
    }

    pub fn prepare(
        &self,
        queue: &wgpu::Queue,
        placement: Placement,
        target: [f32; 2],
        display: &Display,
    ) {
        let Some(image) = &self.image else {
            return;
        };
        let (low, gain) = display.transform();

        queue.write_buffer(
            &self.params,
            0,
            bytemuck::bytes_of(&Params {
                offset: [
                    placement.x / target[0] * 2.0 - 1.0,
                    1.0 - placement.y / target[1] * 2.0,
                ],
                scale: [
                    placement.width / target[0] * 2.0,
                    placement.height / target[1] * 2.0,
                ],
                window: [low, gain],
                _pad: [0.0; 2],
                primaries: image.primaries,
                swizzle: image.swizzle,
                alpha_mode: image.alpha_mode,
                colormap: display.colormap.index(),
                _pad2: 0,
            }),
        );
    }

    pub fn render(&self, pass: &mut wgpu::RenderPass<'_>, placement: Placement) {
        let Some(image) = &self.image else {
            return;
        };
        // Magnification shows the pixel grid; minification is smoothed.
        let group = if placement.zoom >= 1.0 {
            &image.nearest
        } else {
            &image.linear
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.params_group, &[]);
        pass.set_bind_group(1, group, &[]);
        pass.draw(0..4, 0..1);
    }
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
