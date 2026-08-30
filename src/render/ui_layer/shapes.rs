//! The geometry half of the UI layer: every quad and polygon in the frame,
//! in the order it was emitted, as runs of one instanced draw each.

use bytemuck::{Pod, Zeroable};

use super::{Blend, Shape};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct QuadInstance {
    /// x, y, width, height in physical pixels.
    rect: [f32; 4],
    /// Linear, straight alpha.
    color: [f32; 4],
    corner: f32,
    _pad: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PolyVertex {
    /// x, y in physical pixels.
    position: [f32; 2],
    /// Linear, straight alpha.
    color: [f32; 4],
}

/// Which buffer and pipeline family a run of geometry draws from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Quad,
    Poly,
}

/// A stretch of one buffer that can go down in a single draw.
struct Run {
    kind: Kind,
    blend: Blend,
    start: u32,
    end: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ViewportUniform {
    size: [f32; 2],
    _pad: [f32; 2],
}

const INITIAL_QUADS: usize = 64;
const INITIAL_VERTICES: usize = 1024;

pub(super) struct Shapes {
    /// One per [`Blend`], indexed by it.
    pipelines: [wgpu::RenderPipeline; 2],
    poly_pipelines: [wgpu::RenderPipeline; 2],
    viewport_buffer: wgpu::Buffer,
    viewport_group: wgpu::BindGroup,
    instances: wgpu::Buffer,
    instance_capacity: usize,
    vertices: wgpu::Buffer,
    vertex_capacity: usize,
    /// Contiguous stretches of geometry sharing a kind and a blend mode, in
    /// the order the frame emitted them. Splitting the draw this way rather
    /// than batching by mode is what keeps shapes painting in the order they
    /// were added; a frame of plain quads still gets a single draw.
    runs: Vec<Run>,
}

impl Shapes {
    pub(super) fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ui quads"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/ui.wgsl").into()),
        });

        let viewport_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ui viewport"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let viewport_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ui viewport"),
            size: size_of::<ViewportUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let viewport_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ui viewport"),
            layout: &viewport_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: viewport_buffer.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ui quads"),
            bind_group_layouts: &[Some(&viewport_layout)],
            immediate_size: 0,
        });

        let quad_attributes =
            wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32];
        let poly_attributes = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4];
        let pipeline_for = |label, blend, kind| {
            let (entry, buffer) = match kind {
                Kind::Quad => (
                    ("vs_main", "fs_main"),
                    wgpu::VertexBufferLayout {
                        array_stride: size_of::<QuadInstance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &quad_attributes,
                    },
                ),
                Kind::Poly => (
                    ("vs_poly", "fs_poly"),
                    wgpu::VertexBufferLayout {
                        array_stride: size_of::<PolyVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &poly_attributes,
                    },
                ),
            };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(entry.0),
                    compilation_options: Default::default(),
                    buffers: &[Some(buffer)],
                },
                primitive: wgpu::PrimitiveState {
                    topology: match kind {
                        Kind::Quad => wgpu::PrimitiveTopology::TriangleStrip,
                        Kind::Poly => wgpu::PrimitiveTopology::TriangleList,
                    },
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry.1),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: target_format,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };

        // Both shaders premultiply, so colour and coverage accumulate into a
        // transparent target, which is what the compositor wants. Screen
        // weights the source by what is already there, which only works on
        // premultiplied colour — hence the convention.
        const SCREEN: wgpu::BlendState = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::OneMinusDst,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::OneMinusDstAlpha,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        const OVER: wgpu::BlendState = wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING;

        let pipelines = [
            pipeline_for("ui quads", OVER, Kind::Quad),
            pipeline_for("ui quads (screen)", SCREEN, Kind::Quad),
        ];
        let poly_pipelines = [
            pipeline_for("ui polygons", OVER, Kind::Poly),
            pipeline_for("ui polygons (screen)", SCREEN, Kind::Poly),
        ];

        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ui instances"),
            size: (INITIAL_QUADS * size_of::<QuadInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ui vertices"),
            size: (INITIAL_VERTICES * size_of::<PolyVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipelines,
            poly_pipelines,
            viewport_buffer,
            viewport_group,
            instances,
            instance_capacity: INITIAL_QUADS,
            vertices,
            vertex_capacity: INITIAL_VERTICES,
            runs: Vec::new(),
        }
    }

    pub(super) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        shapes: &[Shape],
        physical: [u32; 2],
        scale: f32,
    ) {
        queue.write_buffer(
            &self.viewport_buffer,
            0,
            bytemuck::bytes_of(&ViewportUniform {
                size: [physical[0] as f32, physical[1] as f32],
                _pad: [0.0; 2],
            }),
        );

        let mut instances: Vec<QuadInstance> = Vec::new();
        let mut vertices: Vec<PolyVertex> = Vec::new();
        self.runs.clear();
        for shape in shapes {
            let (kind, blend, added) = match shape {
                Shape::Quad(quad) => {
                    instances.push(QuadInstance {
                        rect: [
                            quad.rect.x * scale,
                            quad.rect.y * scale,
                            quad.rect.width * scale,
                            quad.rect.height * scale,
                        ],
                        color: quad.color.to_linear(),
                        corner: quad.corner * scale,
                        _pad: [0.0; 3],
                    });
                    (Kind::Quad, quad.blend, 1)
                }
                Shape::Poly(poly) => {
                    let color = poly.color.to_linear();
                    vertices.extend(poly.vertices.iter().map(|point| PolyVertex {
                        position: [point[0] * scale, point[1] * scale],
                        color,
                    }));
                    (Kind::Poly, poly.blend, poly.vertices.len() as u32)
                }
            };
            match self.runs.last_mut() {
                Some(run) if run.kind == kind && run.blend == blend => run.end += added,
                _ => {
                    let start = match kind {
                        Kind::Quad => instances.len() as u32 - added,
                        Kind::Poly => vertices.len() as u32 - added,
                    };
                    self.runs.push(Run {
                        kind,
                        blend,
                        start,
                        end: start + added,
                    });
                }
            }
        }

        if instances.len() > self.instance_capacity {
            self.instance_capacity = instances.len().next_power_of_two();
            self.instances = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("ui instances"),
                size: (self.instance_capacity * size_of::<QuadInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if vertices.len() > self.vertex_capacity {
            self.vertex_capacity = vertices.len().next_power_of_two();
            self.vertices = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("ui vertices"),
                size: (self.vertex_capacity * size_of::<PolyVertex>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if !instances.is_empty() {
            queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&instances));
        }
        if !vertices.is_empty() {
            queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(&vertices));
        }
    }

    pub(super) fn render(&self, pass: &mut wgpu::RenderPass<'_>) {
        if !self.runs.is_empty() {
            pass.set_bind_group(0, &self.viewport_group, &[]);
            for run in &self.runs {
                match run.kind {
                    Kind::Quad => {
                        pass.set_pipeline(&self.pipelines[run.blend as usize]);
                        pass.set_vertex_buffer(0, self.instances.slice(..));
                        // One instance per quad, four corners each.
                        pass.draw(0..4, run.start..run.end);
                    }
                    Kind::Poly => {
                        pass.set_pipeline(&self.poly_pipelines[run.blend as usize]);
                        pass.set_vertex_buffer(0, self.vertices.slice(..));
                        pass.draw(run.start..run.end, 0..1);
                    }
                }
            }
        }
    }
}
