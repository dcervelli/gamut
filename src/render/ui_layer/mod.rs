//! The interface layer: a draw list, and the text machinery behind it.
//!
//! The UI is deliberately not part of image rendering. It draws into its own
//! sRGB target, so widget code works in ordinary sRGB colors and logical
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

/// An sRGB color with straight alpha, the way UI code likes to think about
/// color. Conversion to whatever the pipeline needs happens on the way to
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

    /// The interface color that shows a linear working-space value: the
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
    /// Meant for opaque colors. Alpha still controls coverage, so a
    /// translucent shape screens proportionately less, but a color dimmed by
    /// its alpha rather than by its components will not read as intended.
    Screen,
}

/// The x axis of everything that lies with the window, which is every shape
/// in the interface but the strokes an icon is drawn from.
const ALONG_THE_WINDOW: [f32; 2] = [1.0, 0.0];

/// The square `radius` out from `centre` on all sides: the box a circle of
/// that radius is drawn in.
fn square(centre: [f32; 2], radius: f32) -> Rect {
    Rect::new(
        centre[0] - radius,
        centre[1] - radius,
        2.0 * radius,
        2.0 * radius,
    )
}

pub(crate) struct QuadItem {
    /// Centre, and half extent along the shape's own two axes, in logical
    /// pixels. A stroked shape's extent is its centre line: the band the
    /// shader lays down straddles it.
    centre: [f32; 2],
    half: [f32; 2],
    color: Color,
    corner: f32,
    /// The unit vector the shape's own x axis runs along. `[1.0, 0.0]` for
    /// everything that lies with the window, which is everything but the
    /// strokes an icon is drawn from.
    axis: [f32; 2],
    /// Zero for a filled shape; the width of the band, for a stroked one.
    stroke: f32,
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

    /// The one place a quad is added to the list. Everything else here is a
    /// choice of the four things one can be: where it is, how round, which
    /// way it faces, and whether it is filled or drawn as a band on its own
    /// outline.
    #[allow(clippy::too_many_arguments)]
    fn quad(
        &mut self,
        rect: Rect,
        color: Color,
        corner: f32,
        axis: [f32; 2],
        stroke: f32,
        blend: Blend,
    ) {
        self.layer().shapes.push(Shape::Quad(QuadItem {
            centre: [rect.x + rect.width / 2.0, rect.y + rect.height / 2.0],
            half: [rect.width / 2.0, rect.height / 2.0],
            color,
            corner,
            axis,
            stroke,
            blend,
        }));
    }

    pub fn rect(&mut self, rect: Rect, color: Color) {
        self.rect_blended(rect, color, Blend::Over);
    }

    /// As [`UiFrame::rect`], but combined with what is under it by `blend`
    /// rather than simply covering it.
    pub fn rect_blended(&mut self, rect: Rect, color: Color, blend: Blend) {
        self.quad(rect, color, 0.0, ALONG_THE_WINDOW, 0.0, blend);
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

    /// One coordinate moved onto the device's own pixel grid, in logical
    /// pixels.
    ///
    /// A display need not have a whole number of device pixels to the logical
    /// one, so a coordinate that is round in the units the interface is
    /// written in is very often not round in the units it is drawn in — and
    /// every edge the shader lays down is feathered over a device pixel,
    /// which is what turns that difference into a blur.
    pub fn snap(&self, value: f32) -> f32 {
        if self.scale.is_finite() && self.scale > 0.0 {
            (value * self.scale).round() / self.scale
        } else {
            value
        }
    }

    /// `rect` with all four of its edges on the device grid, and never
    /// narrower or shallower than a single device pixel — something rounded
    /// away to nothing is not a fainter shape but no shape.
    pub fn snap_rect(&self, rect: Rect) -> Rect {
        let (x, y) = (self.snap(rect.x), self.snap(rect.y));
        let least = self.line_width(0.0);
        Rect::new(
            x,
            y,
            (self.snap(rect.right()) - x).max(least),
            (self.snap(rect.bottom()) - y).max(least),
        )
    }

    /// The largest whole number of `step`s that fits inside `value`, and
    /// never fewer than one.
    ///
    /// What sizes the square an icon is drawn in. An icon is described on a
    /// grid of its own and scaled into that square, so whether the grid
    /// divides into whole device pixels decides whether the cells of a
    /// lattice come out equal — snapping each line on its own cannot fix an
    /// interval that is five and a half pixels wide, it can only round the
    /// lines to either side of it in different directions.
    ///
    /// Down rather than to the nearest, so that a caller which set aside
    /// `value` for the square has set aside enough: rounding up could return
    /// half a step more than the room it was given.
    pub fn snap_within(&self, value: f32, step: f32) -> f32 {
        if !(self.scale.is_finite() && self.scale > 0.0) || step <= 0.0 {
            return value;
        }
        let step = step * self.scale;
        ((value * self.scale / step).floor().max(1.0) * step) / self.scale
    }

    /// `pixels` device pixels, in the logical ones the interface is written
    /// in: for a measure that is fixed in the device's own units, the way the
    /// quantum an icon's square is sized in is.
    pub fn device_pixels(&self, pixels: f32) -> f32 {
        if self.scale.is_finite() && self.scale > 0.0 {
            pixels / self.scale
        } else {
            pixels
        }
    }

    /// Where a stroke `thickness` wide, wanted at `at` device pixels, has to
    /// be centred for both of its edges to land on device pixel boundaries:
    /// the middle of a pixel when it is an odd number of them across, and the
    /// seam between two when it is even. The answer is in logical pixels, as
    /// everything a frame holds is.
    ///
    /// The rule every icon is drawn by, and the counterpart of
    /// [`UiFrame::line`] for anything given as a centre line rather than as
    /// the box it fills. `thickness` is what [`UiFrame::line_width`] gave, so
    /// that it is a whole number of device pixels to begin with; a stroke
    /// centred anywhere else is spread over one pixel more than it needs, and
    /// the feather then draws it at two weights depending on where it fell.
    ///
    /// `at` is in device pixels rather than logical ones because a caller
    /// that can say exactly where a mark goes in the device's own units would
    /// lose that exactness by converting first: a whole number of device
    /// pixels is a repeating fraction of a logical one at any scale that is
    /// not itself whole, and multiplying it back lands a hair either side of
    /// the half pixel the stroke was to be centred on. Which side is
    /// arbitrary, and a row of marks landing on different sides is a row with
    /// uneven gaps — which is what a lattice must not have.
    pub fn stroke_centre_in_device(&self, at: f32, thickness: f32) -> f32 {
        if !(self.scale.is_finite() && self.scale > 0.0) {
            return at;
        }
        let phase = if (thickness * self.scale).round() as i32 % 2 == 0 {
            0.0
        } else {
            0.5
        };
        ((at - phase).round() + phase) / self.scale
    }

    /// `value` in device pixels, rounded to a whole one: the inverse of
    /// [`UiFrame::device_pixels`], for a measure already known to be on the
    /// device's grid and wanted back in its own units without the error that
    /// a round trip through logical pixels leaves behind.
    pub fn to_device(&self, value: f32) -> f32 {
        if self.scale.is_finite() && self.scale > 0.0 {
            (value * self.scale).round()
        } else {
            value
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
        self.quad(rect, color, corner, ALONG_THE_WINDOW, 0.0, Blend::Over);
    }

    /// A stroke `width` wide from `from` to `to`, with a round cap at each
    /// end.
    ///
    /// One instance whichever way it runs: a box with no thickness, drawn as
    /// a band on its own outline, comes out a stadium about the line between
    /// the two points. So a stroke at an angle is anti-aliased by the same
    /// half pixel of feathering as the edge of a panel, rather than by
    /// nothing at all the way a triangulated one would be.
    ///
    /// Nothing here is snapped. Where a stroke has to be sharp — an icon —
    /// the caller puts its ends on the device grid first, with
    /// [`UiFrame::stroke_centre_in_device`].
    pub fn stroke(&mut self, from: [f32; 2], to: [f32; 2], width: f32, color: Color) {
        let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
        let length = dx.hypot(dy);
        // A stroke between one point and itself is a dot, and has no
        // direction to be turned by.
        let axis = if length > f32::EPSILON {
            [dx / length, dy / length]
        } else {
            ALONG_THE_WINDOW
        };
        let centre = [(from[0] + to[0]) / 2.0, (from[1] + to[1]) / 2.0];
        self.quad(
            Rect::new(centre[0] - length / 2.0, centre[1], length, 0.0),
            color,
            0.0,
            axis,
            width,
            Blend::Over,
        );
    }

    /// A band `width` wide laid along the outline of `rect`, rounded by
    /// `corner`: a rectangle drawn rather than filled.
    ///
    /// `rect` is the centre line, the way an SVG rectangle's is, so the band
    /// reaches half its width either side of it.
    pub fn stroke_rect(&mut self, rect: Rect, corner: f32, width: f32, color: Color) {
        self.quad(rect, color, corner, ALONG_THE_WINDOW, width, Blend::Over);
    }

    /// A filled circle.
    pub fn circle(&mut self, centre: [f32; 2], radius: f32, color: Color) {
        self.rounded_rect(square(centre, radius), radius, color);
    }

    /// A circle drawn rather than filled: [`UiFrame::stroke_rect`] on a
    /// square rounded as far as it will go.
    pub fn stroke_circle(&mut self, centre: [f32; 2], radius: f32, width: f32, color: Color) {
        self.stroke_rect(square(centre, radius), radius, width, color);
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

/// The fonts are here, so this is what answers for them.
impl crate::render::TextMeasure for UiRenderer {
    fn measure_text(&mut self, text: &str, size: f32) -> [f32; 2] {
        self.text.measure(text, size, Face::Sans, None)
    }

    fn measure_mono(&mut self, text: &str, size: f32) -> [f32; 2] {
        self.text.measure(text, size, Face::Mono, None)
    }

    fn measure_wrapped(&mut self, text: &str, size: f32, width: f32) -> [f32; 2] {
        self.text.measure(text, size, Face::Sans, Some(width))
    }

    fn cap_centre(&mut self, size: f32) -> f32 {
        self.text.cap_centre(size, Face::Sans)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The scales a display actually asks for, whole and fractional.
    const SCALES: [f32; 5] = [1.0, 1.25, 1.5, 1.75, 2.0];

    /// A stroke centred where `stroke_centre_in_device` puts it has both edges
    /// on a device pixel boundary — which is the whole reason the method
    /// exists, and is not what rounding to a whole logical pixel gives.
    #[test]
    fn a_stroke_centre_puts_both_edges_on_the_grid() {
        for scale in SCALES {
            let frame = UiFrame::new(scale);
            for thickness in [1.0, 1.5, 2.0, 3.0] {
                let width = frame.line_width(thickness);
                for step in 0..40 {
                    let asked = step as f32 * 0.37;
                    let centre = frame.stroke_centre_in_device(asked * scale, width);
                    for edge in [centre - width / 2.0, centre + width / 2.0] {
                        let device = edge * scale;
                        assert!(
                            (device - device.round()).abs() < 1e-3,
                            "scale {scale}, width {width}: an edge at {device}"
                        );
                    }
                    // And it is the nearest such place, not just some place.
                    assert!(
                        (centre - asked).abs() <= 0.5 / scale + 1e-3,
                        "scale {scale}: {asked} moved to {centre}"
                    );
                }
            }
        }
    }

    /// A line thinner than a device pixel is not a fainter line but a line
    /// drawn at random, so a snapped rectangle keeps a pixel either way.
    #[test]
    fn a_snapped_rectangle_never_vanishes() {
        for scale in SCALES {
            let frame = UiFrame::new(scale);
            let thin = frame.snap_rect(Rect::new(3.2, 4.9, 0.01, 0.01));
            assert!(thin.width * scale >= 1.0 - 1e-3, "{thin:?} at {scale}");
            assert!(thin.height * scale >= 1.0 - 1e-3, "{thin:?} at {scale}");
        }
    }

    #[test]
    fn snapping_within_a_step_lands_on_a_whole_multiple_that_fits() {
        for scale in SCALES {
            let frame = UiFrame::new(scale);
            let step = frame.device_pixels(8.0);
            for asked in [1.0, 7.0, 13.4, 22.0] {
                let side = frame.snap_within(asked, step);
                let steps = side / step;
                assert!(
                    (steps - steps.round()).abs() < 1e-3,
                    "scale {scale}: {asked} snapped to {side}, which is {steps} steps"
                );
                // Never more room than it was given, unless one whole step is
                // already more than that.
                assert!(
                    side <= asked + 1e-3 || steps <= 1.0 + 1e-3,
                    "scale {scale}: {asked} snapped up to {side}"
                );
                assert!(side > 0.0);
            }
        }
    }

    /// Nothing here may divide by a scale the window has not reported yet.
    #[test]
    fn an_impossible_scale_leaves_every_measure_alone() {
        for scale in [0.0, f32::NAN, f32::INFINITY] {
            let frame = UiFrame::new(scale);
            assert_eq!(frame.snap(3.7), 3.7);
            assert_eq!(frame.stroke_centre_in_device(3.7, 1.0), 3.7);
            assert_eq!(frame.snap_within(3.7, 1.0), 3.7);
            assert_eq!(frame.device_pixels(3.7), 3.7);
            assert_eq!(frame.to_device(3.7), 3.7);
            assert_eq!(frame.line_width(2.0), 2.0);
        }
    }
}
