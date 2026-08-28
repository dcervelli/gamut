//! The interface layer: a draw list, and the text machinery behind it.
//!
//! The UI is deliberately not part of image rendering. It draws into its own
//! sRGB target, so widget code works in ordinary sRGB colours and logical
//! pixels and never has to know whether the image beside it is a JPEG or a
//! scene-linear EXR being tone mapped for an HDR display. The compositor is
//! the only thing that sees both.
//!
//! There is no widget toolkit here on purpose. [`UiFrame`] is a display list
//! that batches into a single instanced draw call plus one text pass, so
//! adding panels, sliders or histograms later is a matter of emitting more
//! primitives, not of touching the renderer.

mod renderer;

pub use renderer::UiRenderer;

/// A rectangle in logical pixels, origin top-left.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    pub fn inset(&self, dx: f32, dy: f32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            width: (self.width - 2.0 * dx).max(0.0),
            height: (self.height - 2.0 * dy).max(0.0),
        }
    }
}

/// An sRGB colour with straight alpha, the way UI code likes to think about
/// colour. Conversion to whatever the pipeline needs happens on the way to
/// the GPU.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 255)
    }

    pub fn with_alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }

    /// Linear components, straight alpha. The quad shader writes into an sRGB
    /// target, which re-encodes on write.
    fn to_linear(self) -> [f32; 4] {
        fn channel(value: u8) -> f32 {
            let v = value as f32 / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        }
        [
            channel(self.r),
            channel(self.g),
            channel(self.b),
            self.a as f32 / 255.0,
        ]
    }
}

pub(crate) struct QuadItem {
    rect: Rect,
    color: Color,
    corner: f32,
}

pub(crate) struct TextItem {
    text: String,
    at: [f32; 2],
    size: f32,
    color: Color,
    max_width: Option<f32>,
}

/// One frame's worth of interface, in logical pixels.
pub struct UiFrame {
    size: [f32; 2],
    quads: Vec<QuadItem>,
    texts: Vec<TextItem>,
}

impl UiFrame {
    pub fn new(size: [f32; 2]) -> Self {
        Self {
            size,
            quads: Vec::new(),
            texts: Vec::new(),
        }
    }

    /// The area available, in logical pixels.
    pub fn size(&self) -> [f32; 2] {
        self.size
    }

    pub fn rect(&mut self, rect: Rect, color: Color) {
        self.quads.push(QuadItem {
            rect,
            color,
            corner: 0.0,
        });
    }

    pub fn rounded_rect(&mut self, rect: Rect, corner: f32, color: Color) {
        self.quads.push(QuadItem {
            rect,
            color,
            corner,
        });
    }

    /// Draws `text` with its top-left corner at `at`.
    pub fn text(&mut self, at: [f32; 2], size: f32, color: Color, text: impl Into<String>) {
        self.texts.push(TextItem {
            text: text.into(),
            at,
            size,
            color,
            max_width: None,
        });
    }

    /// As [`UiFrame::text`], but clipped to `max_width` rather than allowed to
    /// run past the edge of a panel.
    pub fn text_clipped(
        &mut self,
        at: [f32; 2],
        size: f32,
        color: Color,
        max_width: f32,
        text: impl Into<String>,
    ) {
        self.texts.push(TextItem {
            text: text.into(),
            at,
            size,
            color,
            max_width: Some(max_width),
        });
    }
}
