//! Puts the image and UI targets onto the surface.
//!
//! The only stage that knows what the display can accept.

use bytemuck::{Pod, Zeroable};

use super::output::Output;
use crate::image::display::{Colormap, Display};

/// Layout must match `struct Params` in shaders/composite.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    tone_map: u32,
    encoding: u32,
    white_scale: f32,
    _pad: f32,
}

pub struct Composite {
    pipeline: wgpu::RenderPipeline,
    params: wgpu::Buffer,
    params_group: wgpu::BindGroup,
    targets_layout: wgpu::BindGroupLayout,
    targets_group: Option<wgpu::BindGroup>,
}

impl Composite {
    pub fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("composite"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/composite.wgsl").into()),
        });

        let params_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("composite params"),
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

        // Both targets are read with `textureLoad`, so no sampler and no
        // filterability requirement.
        let entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let targets_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("composite targets"),
            entries: &[entry(0), entry(1)],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("composite"),
            bind_group_layouts: &[Some(&params_layout), Some(&targets_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("composite"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
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
                    format: surface_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("composite params"),
            size: size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let params_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("composite params"),
            layout: &params_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: params.as_entire_binding(),
            }],
        });

        Self {
            pipeline,
            params,
            params_group,
            targets_layout,
            targets_group: None,
        }
    }

    /// Called whenever the offscreen targets are recreated.
    pub fn bind_targets(
        &mut self,
        device: &wgpu::Device,
        image_target: &wgpu::TextureView,
        ui_target: &wgpu::TextureView,
    ) {
        self.targets_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("composite targets"),
            layout: &self.targets_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(image_target),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(ui_target),
                },
            ],
        }));
    }

    pub fn prepare(&self, queue: &wgpu::Queue, display: &Display, output: &Output) {
        // False colour is already display-referred: a tone curve on top of a
        // colormap would distort the mapping the viewer is reading values off.
        let tone_map = if display.colormap == Colormap::Gray {
            display.tone_map.index()
        } else {
            0
        };

        queue.write_buffer(
            &self.params,
            0,
            bytemuck::bytes_of(&Params {
                tone_map,
                encoding: output.encoding,
                white_scale: 1.0,
                _pad: 0.0,
            }),
        );
    }

    pub fn render(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(targets) = &self.targets_group else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.params_group, &[]);
        pass.set_bind_group(1, targets, &[]);
        pass.draw(0..3, 0..1);
    }
}
