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

/// Where the image sits in the window, in physical pixels, origin top-left.
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub zoom: f32,
}

pub struct View {
    /// `None` once the user has zoomed manually.
    fit: Option<Fit>,
    /// Only consulted when `fit` is `None`.
    zoom: f32,
    /// The image-space point, relative to the image centre, shown at the
    /// centre of the window.
    pan: [f32; 2],
}

impl View {
    pub fn new() -> Self {
        Self {
            fit: Some(Fit::Whole),
            zoom: 1.0,
            pan: [0.0, 0.0],
        }
    }

    /// Back to the state a freshly opened image gets.
    pub fn reset(&mut self) {
        *self = Self::new();
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
        Placement {
            x: window[0] / 2.0 - pan[0] * zoom - width / 2.0,
            y: window[1] / 2.0 - pan[1] * zoom - height / 2.0,
            width,
            height,
            zoom,
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
