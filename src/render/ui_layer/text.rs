//! The text half of the UI layer: glyphon, and the one thing the interface
//! asks of it besides drawing, which is how wide a label will be.

use anyhow::Result;
use glyphon::{
    Attrs, Cache, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache, TextArea,
    TextAtlas, TextBounds, TextRenderer, Viewport, Wrap,
};

use super::TextItem;

pub(super) struct Text {
    font_system: FontSystem,
    swash: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    renderer: TextRenderer,
    /// Reused across frames; glyphon needs a live `Buffer` per text run for
    /// the duration of `prepare`.
    buffers: Vec<glyphon::Buffer>,
    /// How many of `buffers` the last frame used, and so how many to draw.
    count: usize,
    /// Scratch buffer for measurement, kept out of the drawn set.
    measure_buffer: glyphon::Buffer,
}

impl Text {
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
    ) -> Self {
        let mut font_system = FontSystem::new();
        let cache = Cache::new(device);
        let viewport = Viewport::new(device, &cache);
        let mut atlas = TextAtlas::new(device, queue, &cache, target_format);
        let renderer =
            TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
        let measure_buffer = glyphon::Buffer::new(&mut font_system, Metrics::new(14.0, 18.0));
        Self {
            font_system,
            swash: SwashCache::new(),
            viewport,
            atlas,
            renderer,
            buffers: Vec::new(),
            count: 0,
            measure_buffer,
        }
    }

    /// Width and height of `text` in logical pixels, for laying out anything
    /// that has to sit next to it. `wrap_at` breaks it across lines the way
    /// [`UiFrame::text_wrapped`](super::UiFrame::text_wrapped) will, for
    /// anything that has to know how tall a paragraph comes out.
    pub(super) fn measure(&mut self, text: &str, size: f32, wrap_at: Option<f32>) -> [f32; 2] {
        self.measure_buffer
            .set_metrics(Metrics::new(size, size * 1.3));
        self.measure_buffer.set_size(wrap_at, None);
        self.measure_buffer.set_wrap(match wrap_at {
            Some(_) => Wrap::Word,
            None => Wrap::None,
        });
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

    pub(super) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texts: &[TextItem],
        physical: [u32; 2],
        scale: f32,
    ) -> Result<()> {
        // Disjoint field borrows: shaping needs the font system mutably while
        // the text areas below hold the buffers immutably.
        let Self {
            font_system,
            swash,
            atlas,
            renderer,
            viewport,
            buffers,
            count,
            ..
        } = self;

        while buffers.len() < texts.len() {
            buffers.push(glyphon::Buffer::new(font_system, Metrics::new(14.0, 18.0)));
        }
        *count = texts.len();

        let attrs = Attrs::new().family(Family::SansSerif);
        for (buffer, item) in buffers.iter_mut().zip(texts) {
            let size = item.size * scale;
            buffer.set_metrics(Metrics::new(size, size * 1.3));
            buffer.set_wrap(if item.wrap { Wrap::Word } else { Wrap::None });
            buffer.set_size(item.max_width.map(|w| w * scale), None);
            buffer.set_text(&item.text, &attrs, Shaping::Advanced, None);
            buffer.shape_until_scroll(font_system, false);
        }

        viewport.update(
            queue,
            Resolution {
                width: physical[0],
                height: physical[1],
            },
        );

        let areas = buffers
            .iter()
            .take(texts.len())
            .zip(texts)
            .map(|(buffer, item)| {
                let left = item.at[0] * scale;
                let top = item.at[1] * scale;
                let right = item
                    .max_width
                    .map(|w| left + w * scale)
                    .unwrap_or(physical[0] as f32);
                // Glyphs are cut to the run's own width, or to the panel the
                // run scrolls inside where it has one. Partly cut: glyphon
                // trims the quad and its texture coordinates together, so a
                // line half over the edge is drawn half rather than dropped.
                let bounds = match item.clip {
                    Some(clip) => TextBounds {
                        left: (clip.x * scale).floor() as i32,
                        top: (clip.y * scale).floor() as i32,
                        right: (clip.right() * scale).ceil() as i32,
                        bottom: (clip.bottom() * scale).ceil() as i32,
                    },
                    None => TextBounds {
                        left: left.floor() as i32,
                        top: top.floor() as i32,
                        right: right.ceil() as i32,
                        bottom: physical[1] as i32,
                    },
                };
                TextArea {
                    buffer,
                    left,
                    top,
                    scale: 1.0,
                    bounds,
                    default_color: glyphon::Color::rgba(
                        item.color.r,
                        item.color.g,
                        item.color.b,
                        item.color.a,
                    ),
                    custom_glyphs: &[],
                }
            });

        renderer.prepare(device, queue, font_system, atlas, viewport, areas, swash)?;
        Ok(())
    }

    pub(super) fn render(&self, pass: &mut wgpu::RenderPass<'_>) -> Result<()> {
        if self.count > 0 {
            self.renderer.render(&self.atlas, &self.viewport, pass)?;
        }
        Ok(())
    }

    pub(super) fn trim(&mut self) {
        self.atlas.trim();
    }
}
