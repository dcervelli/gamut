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

mod shapes;
mod text;

use anyhow::Result;

use shapes::Shapes;
use text::Text;

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

    /// Whether `point` is inside, for hit-testing a click against a widget.
    /// Half-open, so abutting rectangles cannot both claim the same pixel.
    pub fn contains(&self, point: [f32; 2]) -> bool {
        point[0] >= self.x
            && point[0] < self.right()
            && point[1] >= self.y
            && point[1] < self.bottom()
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

    /// Linear components, straight alpha: what both the quad shader — which
    /// writes into an sRGB target and so re-encodes on write — and the
    /// compositor's backdrop want.
    pub(crate) fn to_linear(self) -> [f32; 4] {
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

/// How a shape combines with what the frame has already put down.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Blend {
    /// Ordinary painter's compositing: the shape covers what is under it.
    Over,
    /// `source + destination * (1 - source)`, which climbs towards white
    /// without ever clipping: red over green reads yellow, all three read
    /// neutral. For overlapping plots that each want to stay visible.
    ///
    /// Meant for opaque colours. Alpha still controls coverage, so a
    /// translucent shape screens proportionately less, but a colour dimmed by
    /// its alpha rather than by its components will not read as intended.
    Screen,
}

pub(crate) struct QuadItem {
    rect: Rect,
    color: Color,
    corner: f32,
    blend: Blend,
}

/// A filled shape, already cut into triangles: three vertices per triangle,
/// in logical pixels.
pub(crate) struct PolyItem {
    vertices: Vec<[f32; 2]>,
    color: Color,
    blend: Blend,
}

/// Geometry in the order it was emitted, so that a polygon drawn after a
/// panel lands on top of it, the same as a quad would.
pub(crate) enum Shape {
    Quad(QuadItem),
    Poly(PolyItem),
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
    shapes: Vec<Shape>,
    texts: Vec<TextItem>,
}

impl UiFrame {
    pub fn new() -> Self {
        Self {
            shapes: Vec::new(),
            texts: Vec::new(),
        }
    }

    pub fn rect(&mut self, rect: Rect, color: Color) {
        self.rect_blended(rect, color, Blend::Over);
    }

    /// As [`UiFrame::rect`], but combined with what is under it by `blend`
    /// rather than simply covering it.
    pub fn rect_blended(&mut self, rect: Rect, color: Color, blend: Blend) {
        self.shapes.push(Shape::Quad(QuadItem {
            rect,
            color,
            corner: 0.0,
            blend,
        }));
    }

    pub fn rounded_rect(&mut self, rect: Rect, corner: f32, color: Color) {
        self.shapes.push(Shape::Quad(QuadItem {
            rect,
            color,
            corner,
            blend: Blend::Over,
        }));
    }

    /// Fills the region between the polyline `top` — left to right, in
    /// logical pixels — and the horizontal line `baseline`.
    ///
    /// The outline is triangulated here rather than approximated with a run
    /// of rectangles: a plot drawn as one polygon has no interior edges to
    /// feather, which is what otherwise leaves a column of seams down it.
    pub fn area(&mut self, top: &[[f32; 2]], baseline: f32, color: Color, blend: Blend) {
        let mut vertices = Vec::with_capacity((top.len().saturating_sub(1)) * 6);
        for pair in top.windows(2) {
            let (left, right) = (pair[0], pair[1]);
            // Flat stretches sitting on the baseline enclose nothing.
            if left[1] >= baseline && right[1] >= baseline {
                continue;
            }
            let (left_foot, right_foot) = ([left[0], baseline], [right[0], baseline]);
            vertices.extend_from_slice(&[left, left_foot, right_foot, left, right_foot, right]);
        }
        if !vertices.is_empty() {
            self.shapes.push(Shape::Poly(PolyItem {
                vertices,
                color,
                blend,
            }));
        }
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

impl Default for UiFrame {
    fn default() -> Self {
        Self::new()
    }
}

/// The GPU side of the layer: every shape in one instanced draw, then the
/// text on top of it, both into the UI target.
pub struct UiRenderer {
    shapes: Shapes,
    text: Text,
}

impl UiRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
    ) -> Self {
        Self {
            shapes: Shapes::new(device, target_format),
            text: Text::new(device, queue, target_format),
        }
    }

    /// Width and height of `text` in logical pixels, for laying out anything
    /// that has to sit next to it.
    pub fn measure(&mut self, text: &str, size: f32) -> [f32; 2] {
        self.text.measure(text, size)
    }

    /// `physical` is the target size in device pixels; `scale` takes the
    /// frame's logical coordinates to it.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &UiFrame,
        physical: [u32; 2],
        scale: f32,
    ) -> Result<()> {
        self.shapes
            .prepare(device, queue, &frame.shapes, physical, scale);
        self.text
            .prepare(device, queue, &frame.texts, physical, scale)
    }

    pub fn render(&self, pass: &mut wgpu::RenderPass<'_>) -> Result<()> {
        self.shapes.render(pass);
        self.text.render(pass)
    }

    pub fn trim(&mut self) {
        self.text.trim();
    }
}
