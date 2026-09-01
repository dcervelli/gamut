//! The interface layer: a draw list, and the text machinery behind it.
//!
//! The UI is deliberately not part of image rendering. It draws into its own
//! sRGB target, so widget code works in ordinary sRGB colours and logical
//! pixels and never has to know whether the image beside it is a JPEG or a
//! scene-linear EXR being tone mapped for an HDR display. The compositor is
//! the only thing that sees both.
//!
//! There is no widget toolkit here on purpose. [`UiFrame`] is a display list
//! that batches into a handful of instanced draw calls and a text pass per
//! layer, so adding panels, sliders or histograms later is a matter of
//! emitting more primitives, not of touching the renderer.

mod popup;
mod shapes;
mod text;

use anyhow::Result;

pub use popup::{Popup, PopupGrid, PopupSection};

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

    /// The interface colour that shows a linear working-space value: the
    /// inverse of [`Color::to_linear`], for the readouts that have to put a
    /// piece of the display's own output on a panel.
    ///
    /// An SDR reading of it. On an HDR output the surface carries more range
    /// than a panel can show, and a swatch on a panel is the thing that has
    /// to sit beside the words without glowing.
    pub fn from_linear(color: [f32; 3]) -> Self {
        fn channel(value: f32) -> u8 {
            let v = value.clamp(0.0, 1.0);
            let encoded = if v <= 0.0031308 {
                v * 12.92
            } else {
                1.055 * v.powf(1.0 / 2.4) - 0.055
            };
            (encoded * 255.0).round() as u8
        }
        Self::rgb(channel(color[0]), channel(color[1]), channel(color[2]))
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

/// How heavily a run is set. Almost everything is [`Weight::Regular`]: the
/// interface talking about itself. Bold is for the file's own name, which is
/// the one thing in the window that is not.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Weight {
    Regular,
    Bold,
}

/// Which face a run is set in. The interface is sans throughout but for the
/// numbers that change under the pointer: a run whose glyphs must not shift
/// sideways as its digits change asks for [`Face::Mono`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Face {
    Sans,
    Mono,
}

pub(crate) struct TextItem {
    text: String,
    at: [f32; 2],
    size: f32,
    color: Color,
    face: Face,
    weight: Weight,
    /// The width the text is laid out into: it breaks at this when `wrap` is
    /// set, and is cut off at it when not.
    max_width: Option<f32>,
    wrap: bool,
    /// What the glyphs are cut to, when that is not simply the run's own
    /// width: a panel whose text scrolls under its own edges.
    clip: Option<Rect>,
}

/// How many layers a frame is drawn in. Two: what the interface is, and
/// whatever is floating over it at the moment.
pub(crate) const LAYERS: usize = 2;

/// One layer of a frame. Shapes keep the order they were added in, but the
/// text of a layer is drawn after all of its shapes — the glyph pass is
/// prepared whole, and one per layer is what it costs — so a shape can only
/// cover words that are on a layer below it.
#[derive(Default)]
struct Layer {
    shapes: Vec<Shape>,
    texts: Vec<TextItem>,
}

/// One frame's worth of interface, in logical pixels.
pub struct UiFrame {
    layers: [Layer; LAYERS],
    /// Which layer primitives are being added to.
    current: usize,
    /// Device pixels to the logical one. Everything here is laid out in
    /// logical pixels and converted on the way to the GPU; this is kept only
    /// so that [`UiFrame::hairline`] can put a line on the device's own grid,
    /// which is the one place the difference between the two is visible.
    scale: f32,
}

impl UiFrame {
    pub fn new(scale: f32) -> Self {
        Self {
            layers: Default::default(),
            current: 0,
            scale,
        }
    }

    /// Runs `draw` on the layer that floats over everything emitted so far,
    /// and returns to the layer that was current.
    ///
    /// What is drawn inside covers the words on the panels underneath as well
    /// as the panels themselves, which is what an open menu has to do — it is
    /// the thing being looked at while it is open, and a histogram's axis
    /// label showing through it would say otherwise — and what a button that
    /// appears over the words it acts on has to do for the same reason.
    ///
    /// Scoped rather than a switch, because the interface is not built in the
    /// order it is stacked: the info panel floats over the content area but is
    /// drawn before the bars, so a button of its own that simply moved to the
    /// top layer would take the bars up there with it.
    pub fn over<T>(&mut self, draw: impl FnOnce(&mut Self) -> T) -> T {
        let was = self.current;
        self.current = LAYERS - 1;
        let drawn = draw(self);
        self.current = was;
        drawn
    }

    fn layer(&mut self) -> &mut Layer {
        &mut self.layers[self.current]
    }

    pub fn rect(&mut self, rect: Rect, color: Color) {
        self.rect_blended(rect, color, Blend::Over);
    }

    /// As [`UiFrame::rect`], but combined with what is under it by `blend`
    /// rather than simply covering it.
    pub fn rect_blended(&mut self, rect: Rect, color: Color, blend: Blend) {
        self.layer().shapes.push(Shape::Quad(QuadItem {
            rect,
            color,
            corner: 0.0,
            blend,
        }));
    }

    /// What a line `thickness` logical pixels thick is actually drawn: the
    /// nearest whole number of device pixels to it, given back in logical
    /// ones, and never fewer than one — a line thinner than a device pixel is
    /// not a fainter line but a line drawn at random.
    ///
    /// Anything laying one line against another needs this, since the answer
    /// is not what was asked for.
    pub fn line_width(&self, thickness: f32) -> f32 {
        if self.scale.is_finite() && self.scale > 0.0 {
            (thickness * self.scale).round().max(1.0) / self.scale
        } else {
            thickness
        }
    }

    /// Draws a line `thickness` logical pixels thick, on the device's own
    /// grid.
    ///
    /// A display need not have a whole number of device pixels to the logical
    /// one — 1.6 of them is an ordinary scale — so a line a logical pixel
    /// thick falls across two device pixels in whatever proportion its
    /// position happens to give, and the shader's half-pixel feather then
    /// takes a bite out of both. How much is left depends on where the line
    /// landed, which is why two rules drawn to the same width come out at two
    /// weights, and why the fainter of them reads as a mistake.
    ///
    /// Snapped, a line is a whole number of device pixels at a whole device
    /// pixel: the feather has nothing to work on and every line in the window
    /// drawn to one width is the same line. It keeps the weight it was asked
    /// for rather than being cut to a single device pixel, so that a display
    /// with two device pixels to the logical one does not draw the whole
    /// interface at half strength.
    ///
    /// Which way the line runs is taken from the rectangle: the long side is
    /// its length, and the short one is replaced by the snapped thickness.
    pub fn line(&mut self, rect: Rect, thickness: f32, color: Color) {
        if !(self.scale.is_finite() && self.scale > 0.0) {
            self.rect(rect, color);
            return;
        }
        let device = |value: f32| (value * self.scale).round() / self.scale;
        let thickness = self.line_width(thickness);
        let (x, y) = (device(rect.x), device(rect.y));
        let snapped = if rect.width >= rect.height {
            Rect::new(x, y, device(rect.right()) - x, thickness)
        } else {
            Rect::new(x, y, thickness, device(rect.bottom()) - y)
        };
        self.rect(snapped, color);
    }

    /// A rule one logical pixel thick: what parts a bar from the content, or
    /// one section of the info panel's column from the next.
    pub fn hairline(&mut self, rect: Rect, color: Color) {
        self.line(rect, 1.0, color);
    }

    pub fn rounded_rect(&mut self, rect: Rect, corner: f32, color: Color) {
        self.layer().shapes.push(Shape::Quad(QuadItem {
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
            self.layer().shapes.push(Shape::Poly(PolyItem {
                vertices,
                color,
                blend,
            }));
        }
    }

    /// Strokes the polyline `points` — in logical pixels — `width` wide.
    ///
    /// Each segment is a quad about its own centre line, with a square patch
    /// at every interior joint. Mitring would be the tidier construction, but
    /// a patch the width of the stroke fills the notch on the outside of a
    /// bend at any angle, and the one curve drawn with this turns over
    /// hundreds of short segments where the difference cannot be seen.
    pub fn polyline(&mut self, points: &[[f32; 2]], width: f32, color: Color, blend: Blend) {
        let half = width / 2.0;
        let mut vertices = Vec::with_capacity(points.len() * 12);
        for pair in points.windows(2) {
            let (from, to) = (pair[0], pair[1]);
            let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
            let length = dx.hypot(dy);
            // A repeated point has no direction to stand perpendicular to,
            // and a non-finite one would put a NaN vertex in the buffer.
            if !length.is_finite() || length <= f32::EPSILON {
                continue;
            }
            let (nx, ny) = (-dy / length * half, dx / length * half);
            let (a, b) = ([from[0] + nx, from[1] + ny], [from[0] - nx, from[1] - ny]);
            let (c, d) = ([to[0] - nx, to[1] - ny], [to[0] + nx, to[1] + ny]);
            vertices.extend_from_slice(&[a, b, c, a, c, d]);
        }
        // The joints, once the segments they sit between are known to exist.
        if !vertices.is_empty() {
            for point in &points[1..points.len().saturating_sub(1)] {
                let (left, right) = (point[0] - half, point[0] + half);
                let (top, bottom) = (point[1] - half, point[1] + half);
                vertices.extend_from_slice(&[
                    [left, top],
                    [left, bottom],
                    [right, bottom],
                    [left, top],
                    [right, bottom],
                    [right, top],
                ]);
            }
            self.layer().shapes.push(Shape::Poly(PolyItem {
                vertices,
                color,
                blend,
            }));
        }
    }

    /// A filled triangle, in logical pixels: what the arrowheads and
    /// chevrons an icon is drawn from are made of, since a rectangle cannot
    /// point anywhere.
    pub fn triangle(&mut self, vertices: [[f32; 2]; 3], color: Color) {
        self.layer().shapes.push(Shape::Poly(PolyItem {
            vertices: vertices.to_vec(),
            color,
            blend: Blend::Over,
        }));
    }

    /// Draws `text` with its top-left corner at `at`.
    pub fn text(&mut self, at: [f32; 2], size: f32, color: Color, text: impl Into<String>) {
        self.layer().texts.push(TextItem {
            text: text.into(),
            at,
            size,
            color,
            face: Face::Sans,
            weight: Weight::Regular,
            max_width: None,
            wrap: false,
            clip: None,
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
        self.clipped(
            at,
            size,
            color,
            Face::Sans,
            Weight::Regular,
            max_width,
            text,
        );
    }

    /// As [`UiFrame::text_clipped`], set bold.
    pub fn text_clipped_bold(
        &mut self,
        at: [f32; 2],
        size: f32,
        color: Color,
        max_width: f32,
        text: impl Into<String>,
    ) {
        self.clipped(at, size, color, Face::Sans, Weight::Bold, max_width, text);
    }

    /// As [`UiFrame::text_clipped`], set in the monospace face.
    pub fn text_clipped_mono(
        &mut self,
        at: [f32; 2],
        size: f32,
        color: Color,
        max_width: f32,
        text: impl Into<String>,
    ) {
        self.clipped(
            at,
            size,
            color,
            Face::Mono,
            Weight::Regular,
            max_width,
            text,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn clipped(
        &mut self,
        at: [f32; 2],
        size: f32,
        color: Color,
        face: Face,
        weight: Weight,
        max_width: f32,
        text: impl Into<String>,
    ) {
        self.layer().texts.push(TextItem {
            text: text.into(),
            at,
            size,
            color,
            face,
            weight,
            max_width: Some(max_width),
            wrap: false,
            clip: None,
        });
    }

    /// Draws `text` broken across lines at `width`, with its first line's
    /// top-left corner at `at`, showing only what falls inside `clip`.
    ///
    /// The clip is what makes a panel scrollable: the run is laid out at its
    /// true position, which may be above or below the panel it belongs to,
    /// and the glyphs are cut to the panel's edges — including partly, so a
    /// line half out of view is drawn half.
    pub fn text_wrapped(
        &mut self,
        at: [f32; 2],
        size: f32,
        color: Color,
        width: f32,
        clip: Rect,
        text: impl Into<String>,
    ) {
        self.layer().texts.push(TextItem {
            text: text.into(),
            at,
            size,
            color,
            face: Face::Sans,
            weight: Weight::Regular,
            max_width: Some(width),
            wrap: true,
            clip: Some(clip),
        });
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
        self.text.measure(text, size, Face::Sans, None)
    }

    /// As [`UiRenderer::measure`], for a run set in the monospace face.
    pub fn measure_mono(&mut self, text: &str, size: f32) -> [f32; 2] {
        self.text.measure(text, size, Face::Mono, None)
    }

    /// As [`UiRenderer::measure`], but for text broken across lines at
    /// `width`: how tall a paragraph will come out, for anything stacking one
    /// under another.
    pub fn measure_wrapped(&mut self, text: &str, size: f32, width: f32) -> [f32; 2] {
        self.text.measure(text, size, Face::Sans, Some(width))
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
        let shapes = frame.layers.each_ref().map(|layer| layer.shapes.as_slice());
        let texts = frame.layers.each_ref().map(|layer| layer.texts.as_slice());
        self.shapes.prepare(device, queue, shapes, physical, scale);
        self.text.prepare(device, queue, texts, physical, scale)
    }

    /// Each layer's shapes, then its words, then the layer above it: the one
    /// point in the frame where the two halves of the draw list interleave.
    pub fn render(&self, pass: &mut wgpu::RenderPass<'_>) -> Result<()> {
        for layer in 0..LAYERS {
            self.shapes.render(pass, layer);
            self.text.render(pass, layer)?;
        }
        Ok(())
    }

    pub fn trim(&mut self) {
        self.text.trim();
    }
}
