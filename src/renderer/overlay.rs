//! The status bar text, drawn with glyphon on top of everything else.

use anyhow::Result;
use glyphon::{
    Attrs, Buffer, Cache, Color, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache,
    TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};

/// Logical (unscaled) pixels.
const FONT_SIZE: f32 = 14.0;
const LINE_HEIGHT: f32 = 18.0;
const PADDING_X: f32 = 12.0;
const PADDING_Y: f32 = 7.0;

pub struct Overlay {
    font_system: FontSystem,
    swash: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    renderer: TextRenderer,
    buffer: Buffer,
    text: String,
    scale: f32,
}

impl Overlay {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        scale: f32,
    ) -> Self {
        let mut font_system = FontSystem::new();
        let cache = Cache::new(device);
        let viewport = Viewport::new(device, &cache);
        let mut atlas = TextAtlas::new(device, queue, &cache, format);
        let renderer =
            TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
        let buffer = Buffer::new(&mut font_system, Self::metrics(scale));

        Self {
            font_system,
            swash: SwashCache::new(),
            viewport,
            atlas,
            renderer,
            buffer,
            text: String::new(),
            scale,
        }
    }

    fn metrics(scale: f32) -> Metrics {
        Metrics::new(FONT_SIZE * scale, LINE_HEIGHT * scale)
    }

    /// Height of the bar in physical pixels.
    pub fn bar_height(&self) -> f32 {
        (LINE_HEIGHT + 2.0 * PADDING_Y) * self.scale
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        scale: f32,
        text: &str,
    ) -> Result<()> {
        if scale != self.scale {
            self.scale = scale;
            self.buffer.set_metrics(Self::metrics(scale));
            self.text.clear();
        }

        let bar_height = self.bar_height();
        let text_width = (width as f32 - 2.0 * PADDING_X * scale).max(1.0);
        self.buffer.set_size(Some(text_width), Some(bar_height));

        if self.text != text {
            self.text.clear();
            self.text.push_str(text);
            let attrs = Attrs::new().family(Family::SansSerif);
            // One line, clipped rather than wrapped: the bar is a fixed height.
            self.buffer.set_wrap(glyphon::Wrap::None);
            self.buffer.set_text(text, &attrs, Shaping::Advanced, None);
        }
        self.buffer.shape_until_scroll(&mut self.font_system, false);

        self.viewport.update(queue, Resolution { width, height });

        let top = height as f32 - bar_height + PADDING_Y * scale;
        let area = TextArea {
            buffer: &self.buffer,
            left: PADDING_X * scale,
            top,
            scale: 1.0,
            bounds: TextBounds {
                left: 0,
                top: (height as f32 - bar_height) as i32,
                right: width as i32,
                bottom: height as i32,
            },
            default_color: Color::rgb(235, 235, 235),
            custom_glyphs: &[],
        };

        self.renderer.prepare(
            device,
            queue,
            &mut self.font_system,
            &mut self.atlas,
            &self.viewport,
            [area],
            &mut self.swash,
        )?;
        Ok(())
    }

    pub fn render(&self, pass: &mut wgpu::RenderPass<'_>) -> Result<()> {
        self.renderer.render(&self.atlas, &self.viewport, pass)?;
        Ok(())
    }

    pub fn trim(&mut self) {
        self.atlas.trim();
    }
}
