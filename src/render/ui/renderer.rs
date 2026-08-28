//! GPU side of the UI layer: one instanced draw for every rectangle, then
//! glyphon for the text, both into the UI target.

use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use glyphon::{
    Attrs, Cache, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache, TextArea,
    TextAtlas, TextBounds, TextRenderer, Viewport, Wrap,
};

use super::UiFrame;

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
struct ViewportUniform {
    size: [f32; 2],
    _pad: [f32; 2],
}

const INITIAL_QUADS: usize = 64;

pub struct UiRenderer {
    pipeline: wgpu::RenderPipeline,
    viewport_buffer: wgpu::Buffer,
    viewport_group: wgpu::BindGroup,
    instances: wgpu::Buffer,
    instance_capacity: usize,
    instance_count: u32,

    font_system: FontSystem,
    swash: SwashCache,
    text_viewport: Viewport,
    atlas: TextAtlas,
    text_renderer: TextRenderer,
    /// Reused across frames; glyphon needs a live `Buffer` per text run for
    /// the duration of `prepare`.
    text_buffers: Vec<glyphon::Buffer>,
    text_count: usize,
    /// Scratch buffer for measurement, kept out of the drawn set.
    measure_buffer: glyphon::Buffer,
}

impl UiRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
    ) -> Self {
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

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ui quads"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<QuadInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x4,
                        1 => Float32x4,
                        2 => Float32,
                    ],
                })],
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
                    // Accumulates premultiplied colour and coverage into a
                    // transparent target, which is what the compositor wants.
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ui instances"),
            size: (INITIAL_QUADS * size_of::<QuadInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mut font_system = FontSystem::new();
        let cache = Cache::new(device);
        let text_viewport = Viewport::new(device, &cache);
        let mut atlas = TextAtlas::new(device, queue, &cache, target_format);
        let text_renderer =
            TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
        let measure_buffer = glyphon::Buffer::new(&mut font_system, Metrics::new(14.0, 18.0));

        Self {
            pipeline,
            viewport_buffer,
            viewport_group,
            instances,
            instance_capacity: INITIAL_QUADS,
            instance_count: 0,
            font_system,
            swash: SwashCache::new(),
            text_viewport,
            atlas,
            text_renderer,
            text_buffers: Vec::new(),
            text_count: 0,
            measure_buffer,
        }
    }

    /// Width and height of `text` in logical pixels, for laying out anything
    /// that has to sit next to it.
    pub fn measure(&mut self, text: &str, size: f32) -> [f32; 2] {
        self.measure_buffer
            .set_metrics(Metrics::new(size, size * 1.3));
        self.measure_buffer.set_size(None, None);
        self.measure_buffer.set_wrap(Wrap::None);
        self.measure_buffer.set_text(
            text,
            &Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
            None,
        );
        self.measure_buffer
            .shape_until_scroll(&mut self.font_system, false);

        let mut width: f32 = 0.0;
        let mut height: f32 = 0.0;
        for run in self.measure_buffer.layout_runs() {
            width = width.max(run.line_w);
            height += run.line_height;
        }
        [width, height]
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &UiFrame,
        physical: [u32; 2],
        scale: f32,
    ) -> Result<()> {
        queue.write_buffer(
            &self.viewport_buffer,
            0,
            bytemuck::bytes_of(&ViewportUniform {
                size: [physical[0] as f32, physical[1] as f32],
                _pad: [0.0; 2],
            }),
        );

        let instances: Vec<QuadInstance> = frame
            .quads
            .iter()
            .map(|quad| QuadInstance {
                rect: [
                    quad.rect.x * scale,
                    quad.rect.y * scale,
                    quad.rect.width * scale,
                    quad.rect.height * scale,
                ],
                color: quad.color.to_linear(),
                corner: quad.corner * scale,
                _pad: [0.0; 3],
            })
            .collect();

        if instances.len() > self.instance_capacity {
            self.instance_capacity = instances.len().next_power_of_two();
            self.instances = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("ui instances"),
                size: (self.instance_capacity * size_of::<QuadInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if !instances.is_empty() {
            queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&instances));
        }
        self.instance_count = instances.len() as u32;

        self.prepare_text(device, queue, frame, physical, scale)
    }

    fn prepare_text(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &UiFrame,
        physical: [u32; 2],
        scale: f32,
    ) -> Result<()> {
        // Disjoint field borrows: shaping needs the font system mutably while
        // the text areas below hold the buffers immutably.
        let Self {
            font_system,
            swash,
            atlas,
            text_renderer,
            text_viewport,
            text_buffers,
            text_count,
            ..
        } = self;

        while text_buffers.len() < frame.texts.len() {
            text_buffers.push(glyphon::Buffer::new(font_system, Metrics::new(14.0, 18.0)));
        }
        *text_count = frame.texts.len();

        let attrs = Attrs::new().family(Family::SansSerif);
        for (buffer, item) in text_buffers.iter_mut().zip(&frame.texts) {
            let size = item.size * scale;
            buffer.set_metrics(Metrics::new(size, size * 1.3));
            buffer.set_wrap(Wrap::None);
            buffer.set_size(item.max_width.map(|w| w * scale), None);
            buffer.set_text(&item.text, &attrs, Shaping::Advanced, None);
            buffer.shape_until_scroll(font_system, false);
        }

        text_viewport.update(
            queue,
            Resolution {
                width: physical[0],
                height: physical[1],
            },
        );

        let areas = text_buffers
            .iter()
            .take(frame.texts.len())
            .zip(&frame.texts)
            .map(|(buffer, item)| {
                let left = item.at[0] * scale;
                let top = item.at[1] * scale;
                let right = item
                    .max_width
                    .map(|w| left + w * scale)
                    .unwrap_or(physical[0] as f32);
                TextArea {
                    buffer,
                    left,
                    top,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: left.floor() as i32,
                        top: top.floor() as i32,
                        right: right.ceil() as i32,
                        bottom: physical[1] as i32,
                    },
                    default_color: glyphon::Color::rgba(
                        item.color.r,
                        item.color.g,
                        item.color.b,
                        item.color.a,
                    ),
                    custom_glyphs: &[],
                }
            });

        text_renderer.prepare(
            device,
            queue,
            font_system,
            atlas,
            text_viewport,
            areas,
            swash,
        )?;
        Ok(())
    }

    pub fn render(&self, pass: &mut wgpu::RenderPass<'_>) -> Result<()> {
        if self.instance_count > 0 {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.viewport_group, &[]);
            pass.set_vertex_buffer(0, self.instances.slice(..));
            pass.draw(0..4, 0..self.instance_count);
        }
        if self.text_count > 0 {
            self.text_renderer
                .render(&self.atlas, &self.text_viewport, pass)?;
        }
        Ok(())
    }

    pub fn trim(&mut self) {
        self.atlas.trim();
    }
}
