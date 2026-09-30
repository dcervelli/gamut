//! Zoom, pan and fit state. Pure geometry, no GPU or windowing types.

use crate::render::{Placement, Upscale};

/// How much one step of the zoom — a notch of the wheel, a key — multiplies
/// it by.
pub const ZOOM_STEP: f32 = 1.25;
const MIN_ZOOM: f32 = 0.02;
const MAX_ZOOM: f32 = 64.0;

/// What the image is measured against when it is fitted: the viewport takes
/// the image in, or the image covers the viewport.
///
/// The two ways round rather than one per axis. Fitting the width and fitting
/// the height are the same pair seen from the other side: whichever of them
/// is the smaller scale shows the whole image, which is [`Fit::Whole`]
/// already, and only the larger one — the image's short side against the
/// viewport, the long side running off the ends — says anything the other
/// two do not.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fit {
    /// Whole image visible; the viewport may have room to spare on one axis.
    Whole,
    /// Viewport filled; the image overflows it on one axis.
    Fill,
}

impl Fit {
    /// Which axis this fit is measured on: the pair of the viewport's edges
    /// the image lands exactly against, the other pair being the one it
    /// falls short of or runs past.
    ///
    /// The two fits always take an axis each — the whole image is held by
    /// the axis with the least room, a filled viewport by the one with the
    /// most — so this is what says which way round the image is, and it is
    /// the image's shape against the viewport's that decides.
    pub fn axis(self, image: [f32; 2], viewport: Viewport) -> Axis {
        let [width, height] = viewport.size();
        let (sx, sy) = (width / image[0], height / image[1]);
        let across = match self {
            Fit::Whole => sx <= sy,
            Fit::Fill => sx >= sy,
        };
        if across { Axis::Across } else { Axis::Down }
    }
}

/// The way an image is held against the viewport: across it or down it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    Across,
    Down,
}

/// The part of the window the image is drawn in, in physical pixels, origin
/// top-left.
///
/// The interface is opaque, so the image is fitted to and panned within what
/// the panels leave in the middle rather than within the whole window. When
/// they are hidden that is the window again, which is all it takes for a
/// fitted view to re-fit the moment the interface comes and goes.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Viewport {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// The whole of a window of `size`, which is what the image gets when the
    /// interface is not on screen.
    pub const fn whole(size: [f32; 2]) -> Self {
        Self::new(0.0, 0.0, size[0], size[1])
    }

    fn size(&self) -> [f32; 2] {
        [self.width, self.height]
    }

    /// The point the image is centered on, and that a zoom works about when
    /// the pointer is not over the picture.
    pub fn center(&self) -> [f32; 2] {
        [self.x + self.width / 2.0, self.y + self.height / 2.0]
    }

    /// Whether `point`, in physical window pixels, is inside. Half-open, so
    /// the panels and the image never both claim the same pixel.
    pub fn contains(&self, point: [f32; 2]) -> bool {
        point[0] >= self.x
            && point[0] < self.x + self.width
            && point[1] >= self.y
            && point[1] < self.y + self.height
    }
}

/// The view in space-scale coordinates: where its center is in the diagram
/// Furnas and Bederson draw in *Space-Scale Diagrams: Understanding
/// Multiscale Interfaces* (CHI '95), which stacks every magnification of the
/// image up a scale axis and shows a view as a window of fixed size moved
/// about in it.
///
/// `v` is the zoom, and `u` is the pan scaled by it: the image's center in
/// screen pixels from the viewport's, the other way about. The point of the
/// coordinates is that a straight line through them is the path a pan and a
/// zoom together should take: a point of the image lands on screen at
/// `x·v − u`, linear in both, so while `u` and `v` move at a steady rate so
/// does every point on screen. Interpolate pan and zoom on their own and the
/// place being zoomed towards swings away first — the zoom carries it off
/// faster than the pan can bring it back — then returns. That is the joint
/// pan-zoom problem of the paper's fourth page, and this is its answer.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Position {
    pub u: [f32; 2],
    pub v: f32,
}

impl Position {
    /// The point `t` of the way along the straight line from `from` to `to`,
    /// `t` running from zero to one. Not clamped: it is the caller's easing
    /// that says how fast the line is travelled.
    pub fn between(from: Position, to: Position, t: f32) -> Position {
        let lerp = |a: f32, b: f32| a + (b - a) * t;
        Position {
            u: [lerp(from.u[0], to.u[0]), lerp(from.u[1], to.u[1])],
            v: lerp(from.v, to.v),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct View {
    /// `None` once the user has zoomed manually.
    fit: Option<Fit>,
    /// Only consulted when `fit` is `None`.
    zoom: f32,
    /// The image-space point, relative to the image center, shown at the
    /// center of the window.
    pan: [f32; 2],
    /// On its way somewhere — a view [`View::at`] makes, part way along a
    /// move — and so shown where the line puts it, rather than held within
    /// the limit a settled view is. The limit is not a straight line in
    /// space-scale coordinates — it bends where the image stops overflowing
    /// the viewport — so the line between two settled views can cross it:
    /// zooming out of a fit toward a detail, the image has to overflow one
    /// edge while still short of the other for a few frames. Held to the
    /// limit there, the move would pin the picture against the edge and then
    /// let it catch up, which is a bend.
    moving: bool,
    upscale: Upscale,
}

impl View {
    pub fn new() -> Self {
        Self {
            fit: Some(Fit::Whole),
            zoom: 1.0,
            pan: [0.0, 0.0],
            moving: false,
            upscale: Upscale::default(),
        }
    }

    /// Back to the state a freshly opened image gets. The upscale filter is a
    /// standing preference rather than part of the view, so it survives.
    pub fn reset(&mut self) {
        *self = Self {
            upscale: self.upscale,
            ..Self::new()
        };
    }

    pub fn upscale(&self) -> Upscale {
        self.upscale
    }

    pub fn set_upscale(&mut self, upscale: Upscale) {
        self.upscale = upscale;
    }

    pub fn cycle_upscale(&mut self) {
        self.upscale = self.upscale.next();
    }

    /// The picture under the view has been turned a quarter: the point that
    /// was at the center of the window is turned with it, so that the same
    /// detail stays there. The pan is measured from the picture's center,
    /// which a turn leaves where it is.
    pub fn turn(&mut self, clockwise: bool) {
        let [x, y] = self.pan;
        self.pan = match clockwise {
            true => [-y, x],
            false => [y, -x],
        };
    }

    /// The picture under the view has been swapped for another rendering of
    /// the same scene, `to` pixels across and down where it was `from`: a
    /// raw's developed frame for the camera's JPEG of it, which may be the
    /// same size give or take a margin or a quarter of it. The detail at the
    /// center of the window stays there, and the picture stays the size it
    /// was on screen, so the swap is made in place and the two can be
    /// compared by flicking between them. A fitted view is fitted again,
    /// which comes to the same thing.
    pub fn rescale(&mut self, from: [f32; 2], to: [f32; 2]) {
        if from.iter().chain(&to).any(|side| *side <= 0.0) {
            return;
        }
        let ratio = [to[0] / from[0], to[1] / from[1]];
        self.pan = [self.pan[0] * ratio[0], self.pan[1] * ratio[1]];
        self.zoom = (self.zoom / ratio[0]).clamp(MIN_ZOOM, MAX_ZOOM);
    }

    fn fit_zoom(fit: Fit, image: [f32; 2], viewport: [f32; 2]) -> f32 {
        let sx = viewport[0] / image[0];
        let sy = viewport[1] / image[1];
        match fit {
            Fit::Whole => sx.min(sy),
            Fit::Fill => sx.max(sy),
        }
    }

    pub fn zoom(&self, image: [f32; 2], viewport: Viewport) -> f32 {
        match self.fit {
            Some(fit) => Self::fit_zoom(fit, image, viewport.size()).clamp(MIN_ZOOM, MAX_ZOOM),
            None => self.zoom,
        }
    }

    /// How far the pan may go from the center on each axis before the image's
    /// edge reaches the viewport's, and so where panning that way stops. Zero
    /// on an axis the image does not overflow, which is the axis it stays
    /// centered on.
    fn pan_limit(image: [f32; 2], viewport: [f32; 2], zoom: f32) -> [f32; 2] {
        let limit = |image_extent: f32, viewport_extent: f32| {
            (image_extent / 2.0 - viewport_extent / (2.0 * zoom)).max(0.0)
        };
        [limit(image[0], viewport[0]), limit(image[1], viewport[1])]
    }

    /// Keeps the image from being dragged away from the viewport: when it is
    /// larger than the viewport you can pan up to its edges and no further,
    /// and when it is smaller it stays centered on that axis.
    fn clamp_pan(pan: [f32; 2], image: [f32; 2], viewport: [f32; 2], zoom: f32) -> [f32; 2] {
        let [lx, ly] = Self::pan_limit(image, viewport, zoom);
        [pan[0].clamp(-lx, lx), pan[1].clamp(-ly, ly)]
    }

    /// The pan on screen: the one held, within the limit — unless the view is
    /// on its way somewhere, when it is wherever the line has it.
    fn shown_pan(&self, image: [f32; 2], viewport: [f32; 2], zoom: f32) -> [f32; 2] {
        if self.moving {
            self.pan
        } else {
            Self::clamp_pan(self.pan, image, viewport, zoom)
        }
    }

    /// Whether the view has anywhere to pan to. False when the whole image is
    /// on screen — at `Fit::Whole`, or for anything smaller than the viewport
    /// — which is the one state where a drag can do nothing at all.
    pub fn can_pan(&self, image: [f32; 2], viewport: Viewport) -> bool {
        let zoom = self.zoom(image, viewport);
        // Half a pixel of overflow is not worth offering to drag.
        image[0] * zoom > viewport.width + 0.5 || image[1] * zoom > viewport.height + 0.5
    }

    pub fn placement(&self, image: [f32; 2], viewport: Viewport) -> Placement {
        let zoom = self.zoom(image, viewport);
        let pan = self.shown_pan(image, viewport.size(), zoom);
        let width = image[0] * zoom;
        let height = image[1] * zoom;
        let center = viewport.center();
        let x = center[0] - pan[0] * zoom - width / 2.0;
        let y = center[1] - pan[1] * zoom - height / 2.0;

        // At and above 1:1 the pixel grid is the whole point, so the image goes
        // on whole pixels. Centering an odd difference otherwise leaves the
        // quad half a pixel off the grid, which puts every texel edge through
        // the middle of a pixel and costs the 100% view its crispness. Below
        // 1:1 there is no grid to line up with, and rounding would make the
        // image twitch as the zoom changed.
        let (x, y) = if zoom >= 1.0 {
            (x.round(), y.round())
        } else {
            (x, y)
        };

        Placement {
            x,
            y,
            width,
            height,
            zoom,
            upscale: self.upscale,
        }
    }

    /// One step in, about `anchor` as [`View::zoom_steps_at`] takes it.
    pub fn zoom_in(&mut self, anchor: [f32; 2], image: [f32; 2], viewport: Viewport) {
        self.zoom_steps_at(1.0, anchor, image, viewport);
    }

    /// One step out, about `anchor` as [`View::zoom_steps_at`] takes it.
    pub fn zoom_out(&mut self, anchor: [f32; 2], image: [f32; 2], viewport: Viewport) {
        self.zoom_steps_at(-1.0, anchor, image, viewport);
    }

    /// Zooms by `steps` of the zoom increment — a key's step, or a notch of
    /// the wheel — keeping whatever is under `anchor`, a point in window
    /// pixels, where it is. Fractional steps are what a trackpad sends, so
    /// this takes a float rather than a count of notches. Zooming continues
    /// from what is on screen, so the first step out of a fit is one step
    /// from the fitted zoom rather than a jump.
    ///
    /// A step that reaches nothing — the zoom already at its limit — is not
    /// a zoom, and leaves a fit in force rather than materializing it.
    pub fn zoom_steps_at(
        &mut self,
        steps: f32,
        anchor: [f32; 2],
        image: [f32; 2],
        viewport: Viewport,
    ) {
        let before = self.zoom(image, viewport);
        let after = (before * ZOOM_STEP.powf(steps)).clamp(MIN_ZOOM, MAX_ZOOM);
        if after == before {
            return;
        }
        self.set_zoom_at(after, anchor, image, viewport);
    }

    /// Zooms to `zoom`, leaving fit mode, keeping whatever is under `anchor`
    /// — a point in window pixels — where it is: the pointer, for a zoom
    /// asked for with it over the picture, or the viewport's center.
    ///
    /// Asked for the zoom the view is already at, it puts what is under the
    /// anchor in the middle instead: a double-click at actual size on a
    /// detail goes to the detail, rather than doing nothing.
    ///
    /// The anchor cannot always be honored: an image smaller than the
    /// viewport stays centered on that axis, and one panned to its edge stops
    /// there. `clamp_pan` decides that, exactly as it does for a drag.
    pub fn set_zoom_at(
        &mut self,
        zoom: f32,
        anchor: [f32; 2],
        image: [f32; 2],
        viewport: Viewport,
    ) {
        let before = self.zoom(image, viewport);
        let after = zoom.clamp(MIN_ZOOM, MAX_ZOOM);

        // Where the anchor sits over the image, from the pan actually on
        // screen rather than the one held: they differ whenever the view is
        // against an edge, and zooming from the held one would jump.
        let pan = Self::clamp_pan(self.pan, image, viewport.size(), before);
        let center = viewport.center();
        let offset = [anchor[0] - center[0], anchor[1] - center[1]];
        let point = [pan[0] + offset[0] / before, pan[1] + offset[1] / before];

        let pan = if after == before {
            point
        } else {
            [point[0] - offset[0] / after, point[1] - offset[1] / after]
        };
        self.zoom = after;
        self.fit = None;
        self.pan = Self::clamp_pan(pan, image, viewport.size(), after);
    }

    /// Which fit the view is in, and `None` once it has been zoomed by hand.
    /// What tells a menu of zooms which of its choices is the one in force.
    pub fn fit(&self) -> Option<Fit> {
        self.fit
    }

    /// The image goes back to being fitted, centered: a fit with the view left
    /// panned off to one side would show a corner of an image it has just
    /// been asked to fit.
    pub fn set_fit(&mut self, fit: Fit) {
        self.fit = Some(fit);
        self.pan = [0.0, 0.0];
    }

    /// `dx`/`dy` are in physical pixels: positive moves the viewport
    /// right/down over the image.
    pub fn pan_by(&mut self, dx: f32, dy: f32, image: [f32; 2], viewport: Viewport) {
        let zoom = self.zoom(image, viewport);
        let panned = [self.pan[0] + dx / zoom, self.pan[1] + dy / zoom];
        self.pan = Self::clamp_pan(panned, image, viewport.size(), zoom);
    }

    /// Pans as far as the view goes, in the direction given as a sign per
    /// axis: to the image's edge on an axis there is more of than fits, and
    /// nowhere at all on one the image is centered on. An axis whose sign is
    /// zero keeps the pan it had.
    pub fn pan_to_edge(&mut self, direction: [f32; 2], image: [f32; 2], viewport: Viewport) {
        let zoom = self.zoom(image, viewport);
        let limit = Self::pan_limit(image, viewport.size(), zoom);
        for axis in 0..2 {
            if direction[axis] != 0.0 {
                self.pan[axis] = direction[axis].signum() * limit[axis];
            }
        }
        // The other axis may be holding a pan from before a zoom that has
        // since left it out of range, as `pan_by` would find it.
        self.pan = Self::clamp_pan(self.pan, image, viewport.size(), zoom);
    }

    /// Puts `point`, in image pixels, at the center of the viewport, at the
    /// zoom the view has. As near as the pan goes, that is: a point close to
    /// an edge stops with the edge at the viewport's, as a drag there would,
    /// and one on an axis the image does not overflow leaves that axis
    /// centered as it was.
    pub fn center_on(&mut self, point: [f32; 2], image: [f32; 2], viewport: Viewport) {
        let zoom = self.zoom(image, viewport);
        let center = [point[0] - image[0] / 2.0, point[1] - image[1] / 2.0];
        self.pan = Self::clamp_pan(center, image, viewport.size(), zoom);
    }

    /// What `Space` does: the whole image, the window filled, then actual
    /// size, and round again. Actual size is a zoom like any other, so a
    /// view zoomed by hand — to 100% or anything else — is at the same
    /// point of the round, and the next press shows the whole image.
    pub fn cycle_fit(&mut self, anchor: [f32; 2], image: [f32; 2], viewport: Viewport) {
        match self.fit {
            None => self.set_fit(Fit::Whole),
            Some(Fit::Whole) => self.set_fit(Fit::Fill),
            Some(Fit::Fill) => self.set_zoom_at(1.0, anchor, image, viewport),
        }
    }

    /// `zoom` about `anchor`, or the whole image from a view already at it:
    /// a quick look at the pixels and straight back out again. At `zoom` is
    /// at it by hand — a fit that happens to come out at the same scale is
    /// still a fit, and goes to `zoom` like any other.
    pub fn toggle_zoom(
        &mut self,
        zoom: f32,
        anchor: [f32; 2],
        image: [f32; 2],
        viewport: Viewport,
    ) {
        if self.fit.is_none() && self.zoom == zoom.clamp(MIN_ZOOM, MAX_ZOOM) {
            self.set_fit(Fit::Whole);
        } else {
            self.set_zoom_at(zoom, anchor, image, viewport);
        }
    }

    /// Fits `region` — `[x, y, width, height]` in image pixels — to the
    /// viewport as `fit` says, and centers it. Not a fit the view keeps: a
    /// fit is a zoom the viewport decides for the whole image, and this is
    /// a zoom chosen for part of it, so it is held as a zoom asked for by
    /// name is and does not follow the window. The zoom stops at the same
    /// limits as any other, so a region of a few pixels is centered rather
    /// than blown up past them.
    pub fn fit_region(&mut self, fit: Fit, region: [f32; 4], image: [f32; 2], viewport: Viewport) {
        let [x, y, width, height] = region;
        self.zoom = Self::fit_zoom(fit, [width.max(1.0), height.max(1.0)], viewport.size())
            .clamp(MIN_ZOOM, MAX_ZOOM);
        self.fit = None;
        let center = [
            x + width / 2.0 - image[0] / 2.0,
            y + height / 2.0 - image[1] / 2.0,
        ];
        self.pan = Self::clamp_pan(center, image, viewport.size(), self.zoom);
    }

    /// Where the view is in space-scale coordinates. From the pan actually on
    /// screen rather than the one held, as `set_zoom_at` reads it: a view
    /// against an edge is at the edge, wherever it was asked to go — and a
    /// view on its way somewhere is where the line has it.
    pub fn position(&self, image: [f32; 2], viewport: Viewport) -> Position {
        let zoom = self.zoom(image, viewport);
        let pan = self.shown_pan(image, viewport.size(), zoom);
        Position {
            u: [pan[0] * zoom, pan[1] * zoom],
            v: zoom,
        }
    }

    /// The view at `position`: what is on screen part way through a move.
    /// Not in fit mode, whatever this view is in — a fit is a zoom the
    /// viewport decides, and a view on its way there is at some other one —
    /// and not held within the pan limit, which the line may cross on its
    /// way from one settled view to another. The upscale filter comes
    /// along, being a preference rather than a place.
    pub fn at(&self, position: Position) -> View {
        View {
            fit: None,
            zoom: position.v,
            pan: [position.u[0] / position.v, position.u[1] / position.v],
            moving: true,
            upscale: self.upscale,
        }
    }
}

#[cfg(test)]
mod tests;
