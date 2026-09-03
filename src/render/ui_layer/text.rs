//! The text half of the UI layer: glyphon, and the one thing the interface
//! asks of it besides drawing, which is how wide a label will be.

use anyhow::Result;
use glyphon::{
    Attrs, Cache, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache, TextArea,
    TextAtlas, TextBounds, TextRenderer, Viewport, Wrap,
};

use super::{Face, LAYERS, TextItem, Weight};

pub(super) struct Text {
    font_system: FontSystem,
    swash: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    /// One glyph pass per layer of the frame. A renderer is prepared whole
    /// and drawn whole, so a layer whose words have to go down after another
    /// layer's shapes needs a renderer of its own; the atlas and the shaping
    /// they all share is where the cost of the fonts actually is.
    layers: [Layer; LAYERS],
    /// Scratch buffer for measurement, kept out of the drawn set.
    measure_buffer: glyphon::Buffer,
}

/// One layer's worth of glyphs.
struct Layer {
    renderer: TextRenderer,
    /// Reused across frames; glyphon needs a live `Buffer` per text run for
    /// the duration of `prepare`.
    buffers: Vec<glyphon::Buffer>,
    /// How many of `buffers` the last frame used, and so how many to draw.
    count: usize,
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
        let layers = std::array::from_fn(|_| Layer {
            renderer: TextRenderer::new(
                &mut atlas,
                device,
                wgpu::MultisampleState::default(),
                None,
            ),
            buffers: Vec::new(),
            count: 0,
        });
        let measure_buffer = glyphon::Buffer::new(&mut font_system, Metrics::new(14.0, 18.0));
        Self {
            font_system,
            swash: SwashCache::new(),
            viewport,
            atlas,
            layers,
            measure_buffer,
        }
    }

    /// Width and height of `text` in logical pixels, for laying out anything
    /// that has to sit next to it. `wrap_at` breaks it across lines the way
    /// [`UiFrame::text_wrapped`](super::UiFrame::text_wrapped) will, for
    /// anything that has to know how tall a paragraph comes out.
    pub(super) fn measure(
        &mut self,
        text: &str,
        size: f32,
        face: Face,
        wrap_at: Option<f32>,
    ) -> [f32; 2] {
        self.measure_buffer
            .set_metrics(Metrics::new(size, size * 1.3));
        self.measure_buffer.set_size(wrap_at, None);
        self.measure_buffer.set_wrap(match wrap_at {
            Some(_) => Wrap::Word,
            None => Wrap::None,
        });
        self.measure_buffer.set_text(
            text,
            &Attrs::new().family(family(face)),
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

    /// How far below the top of a laid-out run the middle of its capitals
    /// sits, at `size`.
    ///
    /// What a label is placed by when it has to sit level with a mark beside
    /// it. A run is laid out in a box that reserves room under the baseline
    /// for descenders, and cosmic-text centres that whole box in the line; so
    /// centring the line in a button leaves a label with no descenders in it
    /// — a percentage, a count of pixels — visibly low against the mark
    /// beside it. The eye levels text on its capitals, so that is what this
    /// measures.
    pub(super) fn cap_centre(&mut self, size: f32, face: Face) -> f32 {
        // Disjoint field borrows: the run below holds the buffer while the
        // font it was shaped with is looked up.
        let Self {
            font_system,
            measure_buffer,
            ..
        } = self;
        measure_buffer.set_metrics(Metrics::new(size, size * 1.3));
        measure_buffer.set_size(None, None);
        measure_buffer.set_wrap(Wrap::None);
        // Shaped rather than worked out from the size alone, so that the
        // answer comes from whichever font a label will actually be set in.
        measure_buffer.set_text(
            "H",
            &Attrs::new().family(family(face)),
            Shaping::Advanced,
            None,
        );
        measure_buffer.shape_until_scroll(font_system, false);

        let Some(run) = measure_buffer.layout_runs().next() else {
            return size / 2.0;
        };
        let cap = run
            .glyphs
            .first()
            .and_then(|glyph| font_system.get_font(glyph.font_id, glyphon::Weight::NORMAL))
            .map(|font| font.as_swash().metrics(&[]).scale(size).cap_height)
            // A face that declares no cap height: most set their capitals at
            // about seven tenths of the em.
            .filter(|cap| *cap > 0.0)
            .unwrap_or(size * 0.7);
        (run.line_y - run.line_top) - cap / 2.0
    }

    /// `texts` is one slice of runs per layer of the frame, in the order they
    /// are drawn.
    pub(super) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texts: [&[TextItem]; LAYERS],
        physical: [u32; 2],
        scale: f32,
    ) -> Result<()> {
        // Disjoint field borrows: shaping needs the font system mutably while
        // the text areas below hold the buffers immutably.
        let Self {
            font_system,
            swash,
            atlas,
            viewport,
            layers,
            ..
        } = self;

        viewport.update(
            queue,
            Resolution {
                width: physical[0],
                height: physical[1],
            },
        );

        for (layer, texts) in layers.iter_mut().zip(texts) {
            layer.prepare(
                device,
                queue,
                font_system,
                swash,
                atlas,
                viewport,
                texts,
                physical,
                scale,
            )?;
        }
        Ok(())
    }

    pub(super) fn render(&self, pass: &mut wgpu::RenderPass<'_>, layer: usize) -> Result<()> {
        let layer = &self.layers[layer];
        if layer.count > 0 {
            layer.renderer.render(&self.atlas, &self.viewport, pass)?;
        }
        Ok(())
    }

    pub(super) fn trim(&mut self) {
        self.atlas.trim();
    }
}

/// The font family a face asks for. Both are whatever the system offers
/// under the name; nothing is shipped with the binary.
fn family(face: Face) -> Family<'static> {
    match face {
        Face::Sans => Family::SansSerif,
        Face::Mono => Family::Monospace,
    }
}

impl Layer {
    #[allow(clippy::too_many_arguments)]
    fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        font_system: &mut FontSystem,
        swash: &mut SwashCache,
        atlas: &mut TextAtlas,
        viewport: &Viewport,
        texts: &[TextItem],
        physical: [u32; 2],
        scale: f32,
    ) -> Result<()> {
        let Self {
            renderer,
            buffers,
            count,
        } = self;

        while buffers.len() < texts.len() {
            buffers.push(glyphon::Buffer::new(font_system, Metrics::new(14.0, 18.0)));
        }
        *count = texts.len();

        for (buffer, item) in buffers.iter_mut().zip(texts) {
            let size = item.size * scale;
            buffer.set_metrics(Metrics::new(size, size * 1.3));
            buffer.set_wrap(if item.wrap { Wrap::Word } else { Wrap::None });
            buffer.set_size(item.max_width.map(|w| w * scale), None);
            let attrs = Attrs::new().family(family(item.face));
            let attrs = match item.weight {
                Weight::Regular => attrs,
                Weight::Bold => attrs.weight(glyphon::Weight::BOLD),
            };
            buffer.set_text(&item.text, &attrs, Shaping::Advanced, None);
            buffer.shape_until_scroll(font_system, false);
        }

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
}
