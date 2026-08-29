//! Zoom, pan and fit state. Pure geometry, no GPU or windowing types.

const ZOOM_STEP: f32 = 1.25;
const MIN_ZOOM: f32 = 0.02;
const MAX_ZOOM: f32 = 64.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fit {
    /// Whole image visible.
    Whole,
    /// Image width fills the window; height may overflow.
    Width,
    /// Image height fills the window; width may overflow.
    Height,
}

impl Fit {
    pub fn label(self) -> &'static str {
        match self {
            Fit::Whole => "fit",
            Fit::Width => "fit width",
            Fit::Height => "fit height",
        }
    }

    fn next(self) -> Fit {
        match self {
            Fit::Whole => Fit::Width,
            Fit::Width => Fit::Height,
            Fit::Height => Fit::Whole,
        }
    }
}

/// How the image is resampled when it is shown larger than life.
///
/// Minification has one right answer — average what the pixel covers — but
/// magnification is a judgement about what the image is for, so it is the
/// user's to make. Neither is a smoothing filter in the ordinary sense: both
/// leave a texel centre exactly as it was found.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Upscale {
    /// Nearest neighbour, ramped across the single output pixel that straddles
    /// a texel edge. Shows the pixel grid a measurement image is read on, and
    /// unlike plain nearest it does not double columns unevenly at a zoom that
    /// is not a whole number.
    #[default]
    Nearest,
    /// Catmull-Rom. Smooth, noticeably sharper than bilinear, and worth having
    /// when the subject is a photograph rather than a grid of measurements.
    Bicubic,
}

impl Upscale {
    pub fn label(self) -> &'static str {
        match self {
            Upscale::Nearest => "nearest",
            Upscale::Bicubic => "bicubic",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.to_ascii_lowercase().as_str() {
            "nearest" | "point" | "pixel" => Upscale::Nearest,
            "bicubic" | "cubic" | "catmull" | "catmull-rom" => Upscale::Bicubic,
            _ => return None,
        })
    }

    fn next(self) -> Self {
        match self {
            Upscale::Nearest => Upscale::Bicubic,
            Upscale::Bicubic => Upscale::Nearest,
        }
    }

    /// Matches the `filter` codes in shaders/image.wgsl, where 0 is the area
    /// filter minification uses.
    pub fn index(self) -> u32 {
        match self {
            Upscale::Nearest => 1,
            Upscale::Bicubic => 2,
        }
    }
}

/// Where the image sits in the window, in physical pixels, origin top-left.
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub zoom: f32,
    pub upscale: Upscale,
}

pub struct View {
    /// `None` once the user has zoomed manually.
    fit: Option<Fit>,
    /// Only consulted when `fit` is `None`.
    zoom: f32,
    /// The image-space point, relative to the image centre, shown at the
    /// centre of the window.
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

    pub fn mode_label(&self) -> &'static str {
        match self.fit {
            Some(f) => f.label(),
            None => "free",
        }
    }

    fn fit_zoom(fit: Fit, image: [f32; 2], window: [f32; 2]) -> f32 {
        let sx = window[0] / image[0];
        let sy = window[1] / image[1];
        match fit {
            Fit::Whole => sx.min(sy),
            Fit::Width => sx,
            Fit::Height => sy,
        }
    }

    pub fn zoom(&self, image: [f32; 2], window: [f32; 2]) -> f32 {
        match self.fit {
            Some(fit) => Self::fit_zoom(fit, image, window).clamp(MIN_ZOOM, MAX_ZOOM),
            None => self.zoom,
        }
    }

    /// Keeps the image from being dragged away from the window: when it is
    /// larger than the window you can pan up to its edges and no further, and
    /// when it is smaller it stays centred on that axis.
    fn clamp_pan(pan: [f32; 2], image: [f32; 2], window: [f32; 2], zoom: f32) -> [f32; 2] {
        let limit = |image_extent: f32, window_extent: f32| {
            (image_extent / 2.0 - window_extent / (2.0 * zoom)).max(0.0)
        };
        let lx = limit(image[0], window[0]);
        let ly = limit(image[1], window[1]);
        [pan[0].clamp(-lx, lx), pan[1].clamp(-ly, ly)]
    }

    pub fn placement(&self, image: [f32; 2], window: [f32; 2]) -> Placement {
        let zoom = self.zoom(image, window);
        let pan = Self::clamp_pan(self.pan, image, window, zoom);
        let width = image[0] * zoom;
        let height = image[1] * zoom;
        let x = window[0] / 2.0 - pan[0] * zoom - width / 2.0;
        let y = window[1] / 2.0 - pan[1] * zoom - height / 2.0;

        // At and above 1:1 the pixel grid is the whole point, so the image goes
        // on whole pixels. Centring an odd difference otherwise leaves the
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

    fn zoom_by(&mut self, factor: f32, image: [f32; 2], window: [f32; 2]) {
        // Materialise the current fit zoom before leaving fit mode, so zooming
        // continues from what is on screen rather than jumping.
        let current = self.zoom(image, window);
        self.zoom = (current * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        self.fit = None;
        self.pan = Self::clamp_pan(self.pan, image, window, self.zoom);
    }

    pub fn zoom_in(&mut self, image: [f32; 2], window: [f32; 2]) {
        self.zoom_by(ZOOM_STEP, image, window);
    }

    pub fn zoom_out(&mut self, image: [f32; 2], window: [f32; 2]) {
        self.zoom_by(1.0 / ZOOM_STEP, image, window);
    }

    /// Zooms by `steps` of the keyboard's zoom increment, keeping whatever is
    /// under `anchor` — a point in window pixels — where it is. Fractional
    /// steps are what a trackpad sends, so this takes a float rather than a
    /// count of notches.
    ///
    /// The anchor cannot always be honoured: an image smaller than the window
    /// stays centred on that axis, and one panned to its edge stops there.
    /// `clamp_pan` decides that, exactly as it does for a drag.
    pub fn zoom_steps_at(
        &mut self,
        steps: f32,
        anchor: [f32; 2],
        image: [f32; 2],
        window: [f32; 2],
    ) {
        let before = self.zoom(image, window);
        let after = (before * ZOOM_STEP.powf(steps)).clamp(MIN_ZOOM, MAX_ZOOM);
        if after == before {
            return;
        }

        // Where the anchor sits over the image, from the pan actually on
        // screen rather than the one held: they differ whenever the view is
        // against an edge, and zooming from the held one would jump.
        let pan = Self::clamp_pan(self.pan, image, window, before);
        let offset = [anchor[0] - window[0] / 2.0, anchor[1] - window[1] / 2.0];
        let point = [pan[0] + offset[0] / before, pan[1] + offset[1] / before];

        self.zoom = after;
        self.fit = None;
        self.pan = Self::clamp_pan(
            [point[0] - offset[0] / after, point[1] - offset[1] / after],
            image,
            window,
            after,
        );
    }

    pub fn actual_size(&mut self, image: [f32; 2], window: [f32; 2]) {
        self.zoom = 1.0;
        self.fit = None;
        self.pan = Self::clamp_pan(self.pan, image, window, self.zoom);
    }

    /// `dx`/`dy` are in window pixels: positive moves the viewport right/down
    /// over the image.
    pub fn pan_by(&mut self, dx: f32, dy: f32, image: [f32; 2], window: [f32; 2]) {
        let zoom = self.zoom(image, window);
        let panned = [self.pan[0] + dx / zoom, self.pan[1] + dy / zoom];
        self.pan = Self::clamp_pan(panned, image, window, zoom);
    }

    pub fn cycle_fit(&mut self) {
        self.fit = Some(match self.fit {
            None => Fit::Whole,
            Some(fit) => fit.next(),
        });
        self.pan = [0.0, 0.0];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMAGE: [f32; 2] = [900.0, 600.0];
    const WINDOW: [f32; 2] = [1200.0, 1200.0];

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn opens_fitted_and_centred() {
        let view = View::new();
        assert_eq!(view.mode_label(), "fit");

        // Width is the tighter constraint, so the image spans the window.
        let placement = view.placement(IMAGE, WINDOW);
        assert!(close(placement.zoom, 1200.0 / 900.0));
        assert!(close(placement.x, 0.0));
        assert!(close(placement.width, 1200.0));
        assert!(close(placement.height, 800.0));
        assert!(close(placement.y, 200.0));
    }

    #[test]
    fn fit_cycles_whole_width_height() {
        let mut view = View::new();
        assert_eq!(view.mode_label(), "fit");
        view.cycle_fit();
        assert_eq!(view.mode_label(), "fit width");
        view.cycle_fit();
        assert_eq!(view.mode_label(), "fit height");
        view.cycle_fit();
        assert_eq!(view.mode_label(), "fit");
    }

    #[test]
    fn fit_width_and_height_use_one_axis_each() {
        let mut view = View::new();
        view.cycle_fit();
        assert!(close(view.zoom(IMAGE, WINDOW), 1200.0 / 900.0));
        view.cycle_fit();
        assert!(close(view.zoom(IMAGE, WINDOW), 1200.0 / 600.0));
    }

    #[test]
    fn zooming_continues_from_what_is_on_screen() {
        let mut view = View::new();
        let fitted = view.zoom(IMAGE, WINDOW);
        view.zoom_in(IMAGE, WINDOW);
        assert_eq!(view.mode_label(), "free");
        assert!(close(view.zoom(IMAGE, WINDOW), fitted * ZOOM_STEP));

        view.zoom_out(IMAGE, WINDOW);
        assert!(close(view.zoom(IMAGE, WINDOW), fitted));
    }

    #[test]
    fn actual_size_is_one_to_one() {
        let mut view = View::new();
        view.actual_size(IMAGE, WINDOW);
        assert_eq!(view.mode_label(), "free");
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
    fn an_image_smaller_than_the_window_stays_centred() {
        let mut view = View::new();
        view.actual_size(IMAGE, WINDOW);
        view.pan_by(500.0, 500.0, IMAGE, WINDOW);
        let placement = view.placement(IMAGE, WINDOW);
        assert!(close(placement.x, (WINDOW[0] - IMAGE[0]) / 2.0));
        assert!(close(placement.y, (WINDOW[1] - IMAGE[1]) / 2.0));
    }

    #[test]
    fn panning_stops_at_the_image_edge() {
        let mut view = View::new();
        view.actual_size(IMAGE, WINDOW);
        // 4x zoom makes the image 3600x2400, larger than the window on both axes.
        view.zoom = 4.0;

        view.pan_by(100_000.0, 100_000.0, IMAGE, WINDOW);
        let placement = view.placement(IMAGE, WINDOW);
        // Panning right to the limit puts the image's right edge on the window's.
        assert!(close(placement.x + placement.width, WINDOW[0]));
        assert!(close(placement.y + placement.height, WINDOW[1]));

        view.pan_by(-100_000.0, -100_000.0, IMAGE, WINDOW);
        let placement = view.placement(IMAGE, WINDOW);
        assert!(close(placement.x, 0.0));
        assert!(close(placement.y, 0.0));
    }

    #[test]
    fn fit_width_still_pans_vertically() {
        let mut view = View::new();
        view.cycle_fit();
        assert_eq!(view.mode_label(), "fit width");
        // 900x600 at fit-width in a 1200x600 window is 1200x800: taller than the
        // window, so there is room to scroll down but not sideways.
        let window = [1200.0, 600.0];
        view.pan_by(400.0, 400.0, IMAGE, window);
        assert_eq!(view.mode_label(), "fit width");
        let placement = view.placement(IMAGE, window);
        assert!(close(placement.x, 0.0));
        assert!(close(placement.y + placement.height, window[1]));
    }

    /// What a wheel zoom is for: the pixel under the pointer is still under it
    /// afterwards, so zooming in on a detail does not also require panning back
    /// to it.
    #[test]
    fn a_wheel_zoom_keeps_the_point_under_the_pointer() {
        let mut view = View::new();
        view.actual_size(IMAGE, WINDOW);
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

    /// A trackpad sends fractions of a notch rather than whole ones.
    #[test]
    fn a_wheel_zoom_takes_fractional_steps() {
        let mut view = View::new();
        view.actual_size(IMAGE, WINDOW);
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
        assert_eq!(view.mode_label(), "fit");
        assert_eq!(view.upscale(), Upscale::Bicubic);
        assert_eq!(view.placement(IMAGE, WINDOW).upscale, Upscale::Bicubic);
    }

    /// Antialiased nearest resolves a texel edge that lands mid-pixel, which
    /// is right at 4.5:1 and wrong at 1:1 — there every texel edge would land
    /// mid-pixel, and the 100% view would come out uniformly soft.
    #[test]
    fn magnified_views_land_on_whole_pixels() {
        let mut view = View::new();
        // An odd window against an even image is what puts the centre on a
        // half pixel.
        let window = [1201.0, 1201.0];
        view.actual_size(IMAGE, window);
        let placement = view.placement(IMAGE, window);
        assert_eq!(placement.x, placement.x.round());
        assert_eq!(placement.y, placement.y.round());

        // Below 1:1 it is left alone.
        view.cycle_fit();
        view.cycle_fit();
        view.cycle_fit();
        let placement = view.placement([4000.0, 4000.0], window);
        assert!(placement.zoom < 1.0);
        assert!(close(placement.x, 0.0));
    }

    #[test]
    fn reset_returns_to_the_opening_state() {
        let mut view = View::new();
        view.zoom_in(IMAGE, WINDOW);
        view.pan_by(300.0, 300.0, IMAGE, WINDOW);
        view.reset();
        assert_eq!(view.mode_label(), "fit");
        let placement = view.placement(IMAGE, WINDOW);
        assert!(close(placement.x, 0.0));
    }
}
