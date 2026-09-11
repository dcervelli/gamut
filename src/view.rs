//! Zoom, pan and fit state. Pure geometry, no GPU or windowing types.

use crate::render::{Placement, Upscale};

const ZOOM_STEP: f32 = 1.25;
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
    pub fn other(self) -> Fit {
        match self {
            Fit::Whole => Fit::Fill,
            Fit::Fill => Fit::Whole,
        }
    }

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

    /// The point the image is centered on, and that zoom works about.
    fn center(&self) -> [f32; 2] {
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
    upscale: Upscale,
}

impl View {
    pub fn new() -> Self {
        Self {
            fit: Some(Fit::Whole),
            zoom: 1.0,
            pan: [0.0, 0.0],
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
        let pan = Self::clamp_pan(self.pan, image, viewport.size(), zoom);
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

    fn zoom_by(&mut self, factor: f32, image: [f32; 2], viewport: Viewport) {
        // Materialize the current fit zoom before leaving fit mode, so zooming
        // continues from what is on screen rather than jumping.
        let current = self.zoom(image, viewport);
        self.zoom = (current * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        self.fit = None;
        self.pan = Self::clamp_pan(self.pan, image, viewport.size(), self.zoom);
    }

    pub fn zoom_in(&mut self, image: [f32; 2], viewport: Viewport) {
        self.zoom_by(ZOOM_STEP, image, viewport);
    }

    pub fn zoom_out(&mut self, image: [f32; 2], viewport: Viewport) {
        self.zoom_by(1.0 / ZOOM_STEP, image, viewport);
    }

    /// Zooms by `steps` of the keyboard's zoom increment, keeping whatever is
    /// under `anchor` — a point in window pixels — where it is. Fractional
    /// steps are what a trackpad sends, so this takes a float rather than a
    /// count of notches.
    ///
    /// The anchor cannot always be honored: an image smaller than the
    /// viewport stays centered on that axis, and one panned to its edge stops
    /// there. `clamp_pan` decides that, exactly as it does for a drag.
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

        // Where the anchor sits over the image, from the pan actually on
        // screen rather than the one held: they differ whenever the view is
        // against an edge, and zooming from the held one would jump.
        let pan = Self::clamp_pan(self.pan, image, viewport.size(), before);
        let center = viewport.center();
        let offset = [anchor[0] - center[0], anchor[1] - center[1]];
        let point = [pan[0] + offset[0] / before, pan[1] + offset[1] / before];

        self.zoom = after;
        self.fit = None;
        self.pan = Self::clamp_pan(
            [point[0] - offset[0] / after, point[1] - offset[1] / after],
            image,
            viewport.size(),
            after,
        );
    }

    /// Zooms to `zoom` about the center of the viewport, leaving fit mode.
    /// What was in the middle stays in the middle, which is what a zoom asked
    /// for by name — rather than at a point — means.
    pub fn set_zoom(&mut self, zoom: f32, image: [f32; 2], viewport: Viewport) {
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self.fit = None;
        self.pan = Self::clamp_pan(self.pan, image, viewport.size(), self.zoom);
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

    pub fn toggle_fit(&mut self) {
        self.set_fit(match self.fit {
            None => Fit::Whole,
            Some(fit) => fit.other(),
        });
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
    /// screen rather than the one held, as `zoom_steps_at` reads it: a view
    /// against an edge is at the edge, wherever it was asked to go.
    pub fn position(&self, image: [f32; 2], viewport: Viewport) -> Position {
        let zoom = self.zoom(image, viewport);
        let pan = Self::clamp_pan(self.pan, image, viewport.size(), zoom);
        Position {
            u: [pan[0] * zoom, pan[1] * zoom],
            v: zoom,
        }
    }

    /// The view at `position`: what is on screen part way through a move.
    /// Not in fit mode, whatever this view is in — a fit is a zoom the
    /// viewport decides, and a view on its way there is at some other one.
    /// The upscale filter comes along, being a preference rather than a
    /// place.
    pub fn at(&self, position: Position) -> View {
        View {
            fit: None,
            zoom: position.v,
            pan: [position.u[0] / position.v, position.u[1] / position.v],
            upscale: self.upscale,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMAGE: [f32; 2] = [900.0, 600.0];
    const WINDOW: Viewport = Viewport::whole([1200.0, 1200.0]);

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn opens_fitted_and_centered() {
        let view = View::new();
        assert_eq!(view.fit(), Some(Fit::Whole));

        // Width is the tighter constraint, so the image spans the window.
        let placement = view.placement(IMAGE, WINDOW);
        assert!(close(placement.zoom, 1200.0 / 900.0));
        assert!(close(placement.x, 0.0));
        assert!(close(placement.width, 1200.0));
        assert!(close(placement.height, 800.0));
        assert!(close(placement.y, 200.0));
    }

    /// The readout in the bar is this mapping run backwards from the pointer,
    /// so it has to invert `placement` exactly — including the half-pixel the
    /// centering leaves when the image does not fill the window.
    #[test]
    fn a_window_point_maps_back_to_the_image_pixel_under_it() {
        let view = View::new();
        let placement = view.placement(IMAGE, WINDOW);

        // The corners of the drawn image are the corners of the image.
        let top_left = placement.image_point([placement.x, placement.y]);
        assert!(close(top_left[0], 0.0) && close(top_left[1], 0.0));
        let bottom_right = placement.image_point([
            placement.x + placement.width,
            placement.y + placement.height,
        ]);
        assert!(close(bottom_right[0], IMAGE[0]) && close(bottom_right[1], IMAGE[1]));

        // And the middle of the window is the middle of a centered image.
        let center = placement.image_point([600.0, 600.0]);
        assert!(close(center[0], IMAGE[0] / 2.0) && close(center[1], IMAGE[1] / 2.0));
    }

    /// Zoomed in and panned, the point under the pointer is wherever the pan
    /// has put it, not where the fitted view had it.
    #[test]
    fn the_mapping_follows_zoom_and_pan() {
        // A viewport smaller than the image, so there is somewhere to pan to.
        let viewport = Viewport::whole([400.0, 300.0]);
        let mut view = View::new();
        view.set_zoom(1.0, IMAGE, viewport);
        view.pan_by(100.0, 50.0, IMAGE, viewport);

        let placement = view.placement(IMAGE, viewport);
        let point = placement.image_point([200.0, 150.0]);
        // 1:1, so the viewport center sits over the image center plus the pan.
        assert!(close(point[0], IMAGE[0] / 2.0 + 100.0));
        assert!(close(point[1], IMAGE[1] / 2.0 + 50.0));

        // Off the top-left of the image reads negative rather than clamping,
        // which is what lets the bar tell "on the image" from "beside it".
        let outside = placement.image_point([placement.x - 10.0, placement.y - 1.0]);
        assert!(outside[0] < 0.0 && outside[1] < 0.0);
    }

    #[test]
    fn a_viewport_holds_its_own_pixels_only() {
        let viewport = Viewport::new(50.0, 30.0, 100.0, 60.0);
        assert!(viewport.contains([50.0, 30.0]));
        assert!(viewport.contains([149.0, 89.0]));
        // Half-open: the far edge belongs to whatever is next to it.
        assert!(!viewport.contains([150.0, 60.0]));
        assert!(!viewport.contains([100.0, 90.0]));
        assert!(!viewport.contains([49.0, 60.0]));
    }

    #[test]
    fn fit_toggles_between_the_whole_image_and_a_filled_viewport() {
        let mut view = View::new();
        assert_eq!(view.fit(), Some(Fit::Whole));
        view.toggle_fit();
        assert_eq!(view.fit(), Some(Fit::Fill));
        view.toggle_fit();
        assert_eq!(view.fit(), Some(Fit::Whole));
    }

    /// A region fitted is a zoom of its own: the region spans the window the
    /// way the whole image would, its middle in the middle, and the view is
    /// no longer in fit mode.
    #[test]
    fn a_region_is_fitted_and_centered_like_an_image() {
        let mut view = View::new();
        // The middle third of the image.
        let region = [300.0, 200.0, 300.0, 200.0];
        view.fit_region(Fit::Whole, region, IMAGE, WINDOW);
        assert_eq!(view.fit(), None);
        assert!(close(view.zoom(IMAGE, WINDOW), 1200.0 / 300.0));
        let placement = view.placement(IMAGE, WINDOW);
        let corner = placement.screen_point([300.0, 200.0]);
        assert!(close(corner[0], 0.0), "{corner:?}");
        assert!(close(corner[1], 200.0), "{corner:?}");
        let middle = placement.image_point([600.0, 600.0]);
        assert!(
            close(middle[0], 450.0) && close(middle[1], 300.0),
            "{middle:?}"
        );

        // Filled, the region's short side spans the window instead.
        view.fit_region(Fit::Fill, region, IMAGE, WINDOW);
        assert!(close(view.zoom(IMAGE, WINDOW), 1200.0 / 200.0));

        // A region of a few pixels stops at the zoom limit.
        view.fit_region(Fit::Whole, [10.0, 10.0, 2.0, 2.0], IMAGE, WINDOW);
        assert!(close(view.zoom(IMAGE, WINDOW), MAX_ZOOM));

        // The view stays inside the image: a region in the corner is held
        // against the edge rather than centered off it.
        let viewport = Viewport::whole([400.0, 300.0]);
        view.fit_region(Fit::Whole, [0.0, 0.0, 100.0, 100.0], IMAGE, viewport);
        let placement = view.placement(IMAGE, viewport);
        assert!(placement.x >= -0.5 && placement.y >= -0.5, "{placement:?}");
    }

    /// The two fits are the two axes of the image, each taken once: 900x600
    /// in a square window is held by its width to be seen whole, and by its
    /// height to fill the window.
    #[test]
    fn the_two_fits_take_an_axis_each() {
        let mut view = View::new();
        assert!(close(view.zoom(IMAGE, WINDOW), 1200.0 / 900.0));
        view.toggle_fit();
        assert!(close(view.zoom(IMAGE, WINDOW), 1200.0 / 600.0));
    }

    /// Which axis each fit lands on, which is what the menu draws the fill
    /// with: never the same one, and both of them turning over together when
    /// the window becomes the other way round to the image.
    #[test]
    fn the_axis_a_fit_lands_on_follows_the_shape_of_the_window() {
        let wide = Viewport::whole([2400.0, 600.0]);
        for window in [WINDOW, wide] {
            assert_ne!(
                Fit::Whole.axis(IMAGE, window),
                Fit::Fill.axis(IMAGE, window)
            );
        }
        // 900x600 in a square window is wider than the room it has, so it is
        // held across the window to be seen whole and down it to fill it; in
        // a window wider still than the image, the other way about.
        assert_eq!(Fit::Whole.axis(IMAGE, WINDOW), Axis::Across);
        assert_eq!(Fit::Fill.axis(IMAGE, WINDOW), Axis::Down);
        assert_eq!(Fit::Whole.axis(IMAGE, wide), Axis::Down);
        assert_eq!(Fit::Fill.axis(IMAGE, wide), Axis::Across);
    }

    #[test]
    fn zooming_continues_from_what_is_on_screen() {
        let mut view = View::new();
        let fitted = view.zoom(IMAGE, WINDOW);
        view.zoom_in(IMAGE, WINDOW);
        assert_eq!(view.fit(), None);
        assert!(close(view.zoom(IMAGE, WINDOW), fitted * ZOOM_STEP));

        view.zoom_out(IMAGE, WINDOW);
        assert!(close(view.zoom(IMAGE, WINDOW), fitted));
    }

    #[test]
    fn actual_size_is_one_to_one() {
        let mut view = View::new();
        view.set_zoom(1.0, IMAGE, WINDOW);
        assert_eq!(view.fit(), None);
        let placement = view.placement(IMAGE, WINDOW);
        assert!(close(placement.zoom, 1.0));
        assert!(close(placement.width, IMAGE[0]));
        assert!(close(placement.height, IMAGE[1]));
    }

    #[test]
    fn zoom_is_clamped() {
        let mut view = View::new();
        for _ in 0..200 {
            view.zoom_in(IMAGE, WINDOW);
        }
        assert!(close(view.zoom(IMAGE, WINDOW), MAX_ZOOM));
        for _ in 0..400 {
            view.zoom_out(IMAGE, WINDOW);
        }
        assert!(close(view.zoom(IMAGE, WINDOW), MIN_ZOOM));
    }

    #[test]
    fn an_image_smaller_than_the_window_stays_centered() {
        let mut view = View::new();
        view.set_zoom(1.0, IMAGE, WINDOW);
        view.pan_by(500.0, 500.0, IMAGE, WINDOW);
        let placement = view.placement(IMAGE, WINDOW);
        assert!(close(placement.x, (WINDOW.width - IMAGE[0]) / 2.0));
        assert!(close(placement.y, (WINDOW.height - IMAGE[1]) / 2.0));
    }

    #[test]
    fn panning_stops_at_the_image_edge() {
        let mut view = View::new();
        view.set_zoom(1.0, IMAGE, WINDOW);
        // 4x zoom makes the image 3600x2400, larger than the window on both axes.
        view.zoom = 4.0;

        view.pan_by(100_000.0, 100_000.0, IMAGE, WINDOW);
        let placement = view.placement(IMAGE, WINDOW);
        // Panning right to the limit puts the image's right edge on the window's.
        assert!(close(placement.x + placement.width, WINDOW.width));
        assert!(close(placement.y + placement.height, WINDOW.height));

        view.pan_by(-100_000.0, -100_000.0, IMAGE, WINDOW);
        let placement = view.placement(IMAGE, WINDOW);
        assert!(close(placement.x, 0.0));
        assert!(close(placement.y, 0.0));
    }

    /// The far side in one press, and only on the axis asked for: the other
    /// keeps whatever pan it had.
    #[test]
    fn panning_to_the_edge_goes_as_far_as_there_is() {
        let mut view = View::new();
        view.set_zoom(1.0, IMAGE, WINDOW);
        // 4x zoom makes the image 3600x2400, larger than the window on both axes.
        view.zoom = 4.0;

        view.pan_to_edge([1.0, 0.0], IMAGE, WINDOW);
        let placement = view.placement(IMAGE, WINDOW);
        assert!(close(placement.x + placement.width, WINDOW.width));
        // Vertically untouched, so still centered.
        assert!(close(
            placement.y + placement.height / 2.0,
            WINDOW.height / 2.0
        ));

        view.pan_to_edge([0.0, -1.0], IMAGE, WINDOW);
        let placement = view.placement(IMAGE, WINDOW);
        assert!(close(placement.x + placement.width, WINDOW.width));
        assert!(close(placement.y, 0.0));
    }

    /// Centering on a point puts that point in the middle of the window,
    /// until the edge of the image gets there first.
    #[test]
    fn centering_on_a_point_goes_as_far_as_the_edge_lets_it() {
        let mut view = View::new();
        view.set_zoom(4.0, IMAGE, WINDOW);
        let middle = [WINDOW.width / 2.0, WINDOW.height / 2.0];

        // 4x zoom makes the image 3600x2400: a point well inside lands in
        // the middle of the window, at the same zoom.
        view.center_on([300.0, 200.0], IMAGE, WINDOW);
        let placement = view.placement(IMAGE, WINDOW);
        let shown = placement.image_point(middle);
        assert!(
            close(shown[0], 300.0) && close(shown[1], 200.0),
            "{shown:?}"
        );
        assert!(close(placement.zoom, 4.0));

        // A point by the corner stops with the image's edges at the
        // window's, as a drag there would.
        view.center_on([10.0, 590.0], IMAGE, WINDOW);
        let placement = view.placement(IMAGE, WINDOW);
        assert!(close(placement.x, 0.0), "{placement:?}");
        assert!(
            close(placement.y + placement.height, WINDOW.height),
            "{placement:?}"
        );

        // Fitted, there is nowhere to go: the image stays centered.
        let mut fitted = View::new();
        fitted.center_on([10.0, 10.0], IMAGE, WINDOW);
        let placement = fitted.placement(IMAGE, WINDOW);
        assert!(close(
            placement.x + placement.width / 2.0,
            WINDOW.width / 2.0
        ));
    }

    /// An image with nothing to pan stays where it is rather than being
    /// shoved against a side of the window.
    #[test]
    fn panning_to_the_edge_of_a_fitted_image_does_nothing() {
        let mut view = View::new();
        view.pan_to_edge([-1.0, 1.0], IMAGE, WINDOW);
        let placement = view.placement(IMAGE, WINDOW);
        assert!(close(
            placement.x + placement.width / 2.0,
            WINDOW.width / 2.0
        ));
        assert!(close(
            placement.y + placement.height / 2.0,
            WINDOW.height / 2.0
        ));
    }

    #[test]
    fn a_filled_viewport_still_pans_vertically() {
        let mut view = View::new();
        view.toggle_fit();
        assert_eq!(view.fit(), Some(Fit::Fill));
        // 900x600 filling a 1200x600 window is 1200x800: taller than the
        // window, so there is room to scroll down but not sideways.
        let window = Viewport::whole([1200.0, 600.0]);
        view.pan_by(400.0, 400.0, IMAGE, window);
        assert_eq!(view.fit(), Some(Fit::Fill));
        let placement = view.placement(IMAGE, window);
        assert!(close(placement.x, 0.0));
        assert!(close(placement.y + placement.height, window.height));
    }

    /// What a wheel zoom is for: the pixel under the pointer is still under it
    /// afterwards, so zooming in on a detail does not also require panning back
    /// to it.
    #[test]
    fn a_wheel_zoom_keeps_the_point_under_the_pointer() {
        let mut view = View::new();
        view.set_zoom(1.0, IMAGE, WINDOW);
        // 4x makes the image larger than the window, so there is pan to give.
        view.zoom = 4.0;

        let anchor = [300.0, 900.0];
        let under = |view: &View| {
            let placement = view.placement(IMAGE, WINDOW);
            [
                (anchor[0] - placement.x) / placement.zoom,
                (anchor[1] - placement.y) / placement.zoom,
            ]
        };

        let before = under(&view);
        view.zoom_steps_at(1.0, anchor, IMAGE, WINDOW);
        assert!(view.zoom(IMAGE, WINDOW) > 4.0);
        let after = under(&view);
        assert!(close(before[0], after[0]), "{before:?} -> {after:?}");
        assert!(close(before[1], after[1]), "{before:?} -> {after:?}");
    }

    /// The image sits in what the panels leave, not in the window, so
    /// everything measured against the viewport has to allow for where it
    /// begins — a wheel zoom's anchor included.
    #[test]
    fn an_offset_viewport_carries_the_image_and_the_anchor_with_it() {
        // Narrow enough that the fit is below 1:1, where placement is not
        // rounded to whole pixels and the centering can be checked exactly.
        let inset = Viewport::new(50.0, 30.0, 450.0, 1140.0);

        // Fitted on width, so it spans the viewport and is centered in it —
        // rather than spanning the window it is a hole in.
        let placement = View::new().placement(IMAGE, inset);
        assert!(close(placement.x, inset.x));
        assert!(close(placement.width, inset.width));
        assert!(close(
            placement.y + placement.height / 2.0,
            inset.y + inset.height / 2.0
        ));

        let mut view = View::new();
        view.set_zoom(1.0, IMAGE, inset);
        // 4x makes the image larger than the viewport, so there is pan to give.
        view.zoom = 4.0;

        let anchor = [300.0, 900.0];
        let under = |view: &View| {
            let placement = view.placement(IMAGE, inset);
            [
                (anchor[0] - placement.x) / placement.zoom,
                (anchor[1] - placement.y) / placement.zoom,
            ]
        };

        let before = under(&view);
        view.zoom_steps_at(1.0, anchor, IMAGE, inset);
        assert!(close(view.zoom(IMAGE, inset), 5.0));
        let after = under(&view);
        // Placement lands on whole pixels above 1:1, so the point can move by
        // half an output pixel at each of the two zooms and no further. Which
        // is the same half pixel the anchor would drift by in a window-sized
        // viewport: the offset itself adds nothing.
        let tolerance = 0.5 / 4.0 + 0.5 / 5.0;
        assert!(
            (before[0] - after[0]).abs() <= tolerance,
            "{before:?} -> {after:?}"
        );
        assert!(
            (before[1] - after[1]).abs() <= tolerance,
            "{before:?} -> {after:?}"
        );
    }

    /// A trackpad sends fractions of a notch rather than whole ones.
    #[test]
    fn a_wheel_zoom_takes_fractional_steps() {
        let mut view = View::new();
        view.set_zoom(1.0, IMAGE, WINDOW);
        view.zoom_steps_at(0.5, [600.0, 600.0], IMAGE, WINDOW);
        assert!(close(view.zoom(IMAGE, WINDOW), ZOOM_STEP.powf(0.5)));
    }

    #[test]
    fn a_wheel_zoom_stops_at_the_same_limits_as_the_keyboard() {
        let mut view = View::new();
        view.zoom_steps_at(-200.0, [0.0, 0.0], IMAGE, WINDOW);
        assert!(close(view.zoom(IMAGE, WINDOW), MIN_ZOOM));
        view.zoom_steps_at(400.0, [0.0, 0.0], IMAGE, WINDOW);
        assert!(close(view.zoom(IMAGE, WINDOW), MAX_ZOOM));
    }

    /// The filter is a preference about how to read images, not part of where
    /// this one is scrolled to, so stepping to the next file keeps it.
    #[test]
    fn the_upscale_filter_outlives_the_image() {
        let mut view = View::new();
        assert_eq!(view.upscale(), Upscale::Nearest);
        view.cycle_upscale();
        assert_eq!(view.upscale(), Upscale::Bicubic);

        view.zoom_in(IMAGE, WINDOW);
        view.reset();
        assert_eq!(view.fit(), Some(Fit::Whole));
        assert_eq!(view.upscale(), Upscale::Bicubic);
        assert_eq!(view.placement(IMAGE, WINDOW).upscale, Upscale::Bicubic);
    }

    /// Antialiased nearest resolves a texel edge that lands mid-pixel, which
    /// is right at 4.5:1 and wrong at 1:1 — there every texel edge would land
    /// mid-pixel, and the 100% view would come out uniformly soft.
    #[test]
    fn magnified_views_land_on_whole_pixels() {
        let mut view = View::new();
        // An odd window against an even image is what puts the center on a
        // half pixel.
        let window = Viewport::whole([1201.0, 1201.0]);
        view.set_zoom(1.0, IMAGE, window);
        let placement = view.placement(IMAGE, window);
        assert_eq!(placement.x, placement.x.round());
        assert_eq!(placement.y, placement.y.round());

        // Below 1:1 it is left alone.
        view.toggle_fit();
        let placement = view.placement([4000.0, 4000.0], window);
        assert!(placement.zoom < 1.0);
        assert!(close(placement.x, 0.0));
    }

    /// What a grab cursor is offered on: a fitted image has nowhere to go, and
    /// a filled viewport leaves only the axis that overflows.
    #[test]
    fn there_is_nothing_to_pan_while_the_whole_image_is_visible() {
        let mut view = View::new();
        assert!(!view.can_pan(IMAGE, WINDOW));

        view.set_zoom(1.0, IMAGE, WINDOW);
        assert!(!view.can_pan(IMAGE, WINDOW));
        view.zoom_in(IMAGE, WINDOW);
        view.zoom_in(IMAGE, WINDOW);
        assert!(view.can_pan(IMAGE, WINDOW));

        // 900x600 filling a 1200x600 window is 1200x800: taller than the
        // window, so the vertical axis has somewhere to go.
        view.reset();
        view.toggle_fit();
        assert!(view.can_pan(IMAGE, Viewport::whole([1200.0, 600.0])));
    }

    /// The straight line in space-scale coordinates is a straight line on
    /// screen for every point of the image, travelled at a steady rate: the
    /// point half way along the path is half way between where it began and
    /// where it ends. Interpolating pan and zoom separately fails this — the
    /// zoom runs ahead of the pan and the point swings out and back.
    #[test]
    fn a_pan_and_zoom_together_carry_every_point_in_a_straight_line() {
        // Zooms below 1:1, where placement is not rounded to whole pixels
        // and the check can be exact; an image large enough at both to have
        // somewhere to pan to.
        let image = [4000.0, 4000.0];
        let mut from = View::new();
        from.set_zoom(0.5, image, WINDOW);
        from.pan_by(150.0, -100.0, image, WINDOW);
        let mut to = View::new();
        to.set_zoom(0.9, image, WINDOW);
        to.pan_by(800.0, 350.0, image, WINDOW);
        let (a, b) = (from.position(image, WINDOW), to.position(image, WINDOW));
        assert!(a.v < b.v && a.u != b.u);

        let on_screen = |t: f32, point: [f32; 2]| {
            let placement = from.at(Position::between(a, b, t)).placement(image, WINDOW);
            [
                placement.x + point[0] * placement.zoom,
                placement.y + point[1] * placement.zoom,
            ]
        };
        for point in [
            [0.0, 0.0],
            [2000.0, 2000.0],
            [3500.0, 700.0],
            [4000.0, 4000.0],
        ] {
            let start = on_screen(0.0, point);
            let end = on_screen(1.0, point);
            for t in [0.25, 0.5, 0.75] {
                let along = on_screen(t, point);
                for axis in 0..2 {
                    let expected = start[axis] + (end[axis] - start[axis]) * t;
                    assert!(
                        close(along[axis], expected),
                        "{point:?} at {t}: {along:?}, expected {expected} on axis {axis}"
                    );
                }
            }
        }
    }

    /// The ends of a move are where a view can be, so the line between them
    /// is too: the pan limit is a straight line in these coordinates, and
    /// the clamp never has to move a point that is on its way.
    #[test]
    fn the_line_between_two_views_stays_within_the_image() {
        let image = [4000.0, 4000.0];
        let mut from = View::new();
        from.set_zoom(0.5, image, WINDOW);
        from.pan_to_edge([1.0, 1.0], image, WINDOW);
        let mut to = View::new();
        to.set_zoom(2.0, image, WINDOW);
        to.pan_to_edge([-1.0, -1.0], image, WINDOW);
        let (a, b) = (from.position(image, WINDOW), to.position(image, WINDOW));
        for t in [0.1, 0.3, 0.5, 0.7, 0.9] {
            let between = Position::between(a, b, t);
            let along = from.at(between);
            // What the clamp leaves is what was asked for.
            let held = along.position(image, WINDOW);
            assert!(close(held.u[0], between.u[0]) && close(held.u[1], between.u[1]));
        }
    }

    /// A wheel zoom keeps the point under the pointer at both ends, and so
    /// all the way along: the straight line keeps whatever its ends share.
    #[test]
    fn a_moving_wheel_zoom_keeps_its_anchor_throughout() {
        let image = [4000.0, 4000.0];
        let mut from = View::new();
        from.set_zoom(2.0, image, WINDOW);
        from.pan_by(300.0, -200.0, image, WINDOW);
        let mut to = from;
        let anchor = [900.0, 250.0];
        to.zoom_steps_at(4.0, anchor, image, WINDOW);
        let (a, b) = (from.position(image, WINDOW), to.position(image, WINDOW));

        let under = |t: f32| {
            from.at(Position::between(a, b, t))
                .placement(image, WINDOW)
                .image_point(anchor)
        };
        let start = under(0.0);
        for t in [0.2, 0.5, 0.8, 1.0] {
            let along = under(t);
            // Placement lands on whole pixels above 1:1, so the point can
            // drift by half an output pixel at either zoom and no further.
            let tolerance = 0.5 / a.v + 0.5 / b.v;
            assert!(
                (along[0] - start[0]).abs() <= tolerance,
                "{start:?} -> {along:?} at {t}"
            );
            assert!(
                (along[1] - start[1]).abs() <= tolerance,
                "{start:?} -> {along:?} at {t}"
            );
        }
    }

    /// A view on its way to a fit is not yet fitted, and the fit it reaches
    /// is the same place the fitted view is.
    #[test]
    fn a_view_at_the_end_of_its_line_is_where_the_settled_view_is() {
        let mut from = View::new();
        from.set_zoom(4.0, IMAGE, WINDOW);
        from.pan_by(500.0, 500.0, IMAGE, WINDOW);
        let to = View::new();
        assert_eq!(to.fit(), Some(Fit::Whole));
        let end = from.at(to.position(IMAGE, WINDOW));
        assert_eq!(end.fit(), None);
        let (arrived, settled) = (end.placement(IMAGE, WINDOW), to.placement(IMAGE, WINDOW));
        assert!(close(arrived.x, settled.x) && close(arrived.y, settled.y));
        assert!(close(arrived.zoom, settled.zoom));
    }

    #[test]
    fn reset_returns_to_the_opening_state() {
        let mut view = View::new();
        view.zoom_in(IMAGE, WINDOW);
        view.pan_by(300.0, 300.0, IMAGE, WINDOW);
        view.reset();
        assert_eq!(view.fit(), Some(Fit::Whole));
        let placement = view.placement(IMAGE, WINDOW);
        assert!(close(placement.x, 0.0));
    }
}
