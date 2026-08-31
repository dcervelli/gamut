//! The geometry half of the UI layer: every quad and polygon in the frame,
//! in the order it was emitted, as runs of one instanced draw each.

use bytemuck::{Pod, Zeroable};

use super::{Blend, LAYERS, Shape};
use crate::render::gpu::{self, GrowableBuffer};

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
    instances: GrowableBuffer,
    vertices: GrowableBuffer,
    /// Contiguous stretches of geometry sharing a kind and a blend mode, in
    /// the order the frame emitted them. Splitting the draw this way rather
    /// than batching by mode is what keeps shapes painting in the order they
    /// were added; a frame of plain quads still gets a single draw.
    runs: Vec<Run>,
    /// Which of `runs` belong to each layer, in drawing order.
    layers: [std::ops::Range<usize>; LAYERS],
}

impl Shapes {
    pub(super) fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ui quads"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/ui.wgsl").into()),
        });

        let viewport_layout =
            gpu::uniform_layout(device, "ui viewport", wgpu::ShaderStages::VERTEX);
        let viewport_buffer = gpu::uniform_buffer::<ViewportUniform>(device, "ui viewport");
        let viewport_group =
            gpu::buffer_group(device, "ui viewport", &viewport_layout, &viewport_buffer);
        let pipeline_layout = gpu::pipeline_layout(device, "ui quads", &[&viewport_layout]);

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

        Self {
            pipelines,
            poly_pipelines,
            viewport_buffer,
            viewport_group,
            instances: GrowableBuffer::new::<QuadInstance>(device, "ui instances", INITIAL_QUADS),
            vertices: GrowableBuffer::new::<PolyVertex>(device, "ui vertices", INITIAL_VERTICES),
            runs: Vec::new(),
            layers: [const { 0..0 }; LAYERS],
        }
    }

    /// `layers` is one slice of shapes per layer of the frame, in the order
    /// they are drawn. Runs never merge across a layer boundary — the text of
    /// the layer below goes down between them.
    pub(super) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layers: [&[Shape]; LAYERS],
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
        for (layer, shapes) in layers.iter().enumerate() {
            let first = self.runs.len();
            for shape in *shapes {
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
                // Only ever into a run this layer opened: the layer below's
                // words are drawn between the two, so a run carried across
                // the boundary would put its shapes on the wrong side of them.
                let open = self.runs.len() > first;
                match self.runs.last_mut() {
                    Some(run) if open && run.kind == kind && run.blend == blend => run.end += added,
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
            self.layers[layer] = first..self.runs.len();
        }

        self.instances.write(device, queue, &instances);
        self.vertices.write(device, queue, &vertices);
    }

    pub(super) fn render(&self, pass: &mut wgpu::RenderPass<'_>, layer: usize) {
        let runs = &self.runs[self.layers[layer].clone()];
        if runs.is_empty() {
            return;
        }
        pass.set_bind_group(0, &self.viewport_group, &[]);
        for run in runs {
            match run.kind {
                Kind::Quad => {
                    pass.set_pipeline(&self.pipelines[run.blend as usize]);
                    pass.set_vertex_buffer(0, self.instances.slice());
                    // One instance per quad, four corners each.
                    pass.draw(0..4, run.start..run.end);
                }
                Kind::Poly => {
                    pass.set_pipeline(&self.poly_pipelines[run.blend as usize]);
                    pass.set_vertex_buffer(0, self.vertices.slice());
                    pass.draw(run.start..run.end, 0..1);
                }
            }
        }
    }
}
