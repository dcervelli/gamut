//! How the numbers in an image become something you can look at: the window
//! applied before display, exposure, tone mapping and false colour.
//!
//! All of it is uniform state — changing any of it re-renders, it never
//! re-decodes or re-uploads.

use super::{DecodedImage, Stats, Transfer};

/// How the display window is chosen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AutoWindow {
    /// Take the values at face value: 0..1 is the visible range.
    Off,
    /// Stretch the full observed range to 0..1.
    MinMax,
    /// Stretch the central 99.8% of the range, ignoring outliers. Usually
    /// what you want for sensor data with hot pixels.
    Percentile,
    /// Left where the user put it.
    Manual,
}

impl AutoWindow {
    pub fn label(self) -> &'static str {
        match self {
            AutoWindow::Off => "unit",
            AutoWindow::MinMax => "min/max",
            AutoWindow::Percentile => "99.8%",
            AutoWindow::Manual => "manual",
        }
    }

    fn next(self) -> Self {
        match self {
            AutoWindow::Off => AutoWindow::MinMax,
            AutoWindow::MinMax => AutoWindow::Percentile,
            // Cycling out of a hand-set window returns to the automatic ones.
            AutoWindow::Percentile | AutoWindow::Manual => AutoWindow::Off,
        }
    }
}

/// What to do with values that are still above 1.0 once windowed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToneMap {
    /// Clip. Correct for measurement work, where you want to see clipping.
    Clip,
    Reinhard,
    /// Khronos PBR Neutral: keeps hue and saturation far better than a
    /// Reinhard curve, and rolls off highlights without the ACES colour cast.
    Neutral,
}

impl ToneMap {
    pub fn label(self) -> &'static str {
        match self {
            ToneMap::Clip => "clip",
            ToneMap::Reinhard => "reinhard",
            ToneMap::Neutral => "neutral",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.to_ascii_lowercase().as_str() {
            "clip" | "none" => ToneMap::Clip,
            "reinhard" => ToneMap::Reinhard,
            "neutral" => ToneMap::Neutral,
            _ => return None,
        })
    }

    fn next(self) -> Self {
        match self {
            ToneMap::Clip => ToneMap::Reinhard,
            ToneMap::Reinhard => ToneMap::Neutral,
            ToneMap::Neutral => ToneMap::Clip,
        }
    }

    pub fn index(self) -> u32 {
        match self {
            ToneMap::Clip => 0,
            ToneMap::Reinhard => 1,
            ToneMap::Neutral => 2,
        }
    }
}

/// False colour for single-channel images. Ignored for colour images.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Colormap {
    Gray,
    Viridis,
    Magma,
    Turbo,
}

impl Colormap {
    pub fn label(self) -> &'static str {
        match self {
            Colormap::Gray => "gray",
            Colormap::Viridis => "viridis",
            Colormap::Magma => "magma",
            Colormap::Turbo => "turbo",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.to_ascii_lowercase().as_str() {
            "gray" | "grey" | "none" => Colormap::Gray,
            "viridis" => Colormap::Viridis,
            "magma" => Colormap::Magma,
            "turbo" => Colormap::Turbo,
            _ => return None,
        })
    }

    fn next(self) -> Self {
        match self {
            Colormap::Gray => Colormap::Viridis,
            Colormap::Viridis => Colormap::Magma,
            Colormap::Magma => Colormap::Turbo,
            Colormap::Turbo => Colormap::Gray,
        }
    }

    pub fn index(self) -> u32 {
        match self {
            Colormap::Gray => 0,
            Colormap::Viridis => 1,
            Colormap::Magma => 2,
            Colormap::Turbo => 3,
        }
    }
}

/// Display state requested on the command line, applied on top of whatever
/// each image's own defaults work out to.
#[derive(Clone, Copy, Default, Debug)]
pub struct Startup {
    pub colormap: Option<Colormap>,
    pub tone_map: Option<ToneMap>,
    pub auto: Option<AutoWindow>,
    pub exposure_stops: Option<f32>,
}

#[derive(Clone, Debug)]
pub struct Display {
    /// Linear working-space values mapped to 0 and 1 respectively.
    pub low: f32,
    pub high: f32,
    pub auto: AutoWindow,
    pub exposure_stops: f32,
    pub tone_map: ToneMap,
    pub colormap: Colormap,
}

impl Default for Display {
    /// Used only when there is no image on screen.
    fn default() -> Self {
        Self {
            low: 0.0,
            high: 1.0,
            auto: AutoWindow::Off,
            exposure_stops: 0.0,
            tone_map: ToneMap::Clip,
            colormap: Colormap::Gray,
        }
    }
}

impl Display {
    /// Sensible starting point for this particular image.
    ///
    /// The distinction that matters is display-referred versus scene-referred.
    /// A JPEG or an sRGB PNG has already been graded by whoever produced it,
    /// so 0..1 is exactly right and touching it would be wrong. Linear sensor
    /// counts and HDR frames have not, and showing them unwindowed is how you
    /// get a black rectangle.
    pub fn for_image_with(image: &DecodedImage, stats: &Stats, startup: Startup) -> Self {
        let display_referred = matches!(image.color.transfer, Transfer::Srgb | Transfer::Gamma(_));
        let auto = if display_referred {
            AutoWindow::Off
        } else {
            AutoWindow::Percentile
        };

        let mut display = Self {
            low: 0.0,
            high: 1.0,
            auto,
            exposure_stops: 0.0,
            tone_map: if image.is_high_dynamic_range() {
                ToneMap::Neutral
            } else {
                ToneMap::Clip
            },
            colormap: Colormap::Gray,
        };

        // A file that states its own range is more trustworthy than a scan of
        // the pixels, so it wins over the automatic modes.
        match image.value_range {
            Some((low, high)) if high > low && !display_referred => {
                display.low = low;
                display.high = high;
                display.auto = AutoWindow::Manual;
            }
            _ => display.apply_auto(stats),
        }

        if let Some(auto) = startup.auto {
            display.auto = auto;
            display.apply_auto(stats);
        }
        if let Some(colormap) = startup.colormap {
            display.colormap = colormap;
        }
        if let Some(tone_map) = startup.tone_map {
            display.tone_map = tone_map;
        }
        if let Some(stops) = startup.exposure_stops {
            display.exposure_stops = stops;
        }
        display
    }

    fn apply_auto(&mut self, stats: &Stats) {
        let (low, high) = match self.auto {
            AutoWindow::Off => (0.0, 1.0),
            AutoWindow::MinMax => (stats.min, stats.max),
            AutoWindow::Percentile => (stats.percentile(0.001), stats.percentile(0.999)),
            AutoWindow::Manual => return,
        };
        self.low = low;
        // A degenerate window would divide by zero in the shader.
        self.high = if high > low { high } else { low + 1.0 };
    }

    /// Re-derives the window from a fresh scan of the same image's pixels,
    /// for when the file has changed underneath a view the user has already
    /// set up. A hand-set window is left exactly where they put it.
    pub fn refresh_auto(&mut self, stats: &Stats) {
        self.apply_auto(stats);
    }

    pub fn cycle_auto(&mut self, stats: &Stats) {
        self.auto = self.auto.next();
        self.apply_auto(stats);
    }

    pub fn cycle_tone_map(&mut self) {
        self.tone_map = self.tone_map.next();
    }

    pub fn cycle_colormap(&mut self) {
        self.colormap = self.colormap.next();
    }

    pub fn adjust_exposure(&mut self, stops: f32) {
        self.exposure_stops = (self.exposure_stops + stops).clamp(-16.0, 16.0);
    }

    /// Widens or narrows the window about its own centre, the "level" half of
    /// a window/level control.
    pub fn adjust_contrast(&mut self, factor: f32) {
        let centre = (self.low + self.high) / 2.0;
        let half = (self.high - self.low) / 2.0 * factor;
        if half.is_finite() && half > 0.0 {
            self.low = centre - half;
            self.high = centre + half;
            self.auto = AutoWindow::Manual;
        }
    }

    /// Slides the window without changing its width, the "window" half.
    pub fn shift_window(&mut self, fraction: f32) {
        let offset = (self.high - self.low) * fraction;
        self.low += offset;
        self.high += offset;
        self.auto = AutoWindow::Manual;
    }

    pub fn reset(&mut self, stats: &Stats, image: &DecodedImage, startup: Startup) {
        *self = Self::for_image_with(image, stats, startup);
    }

    /// `(offset, gain)` such that `(value - offset) * gain` is the displayed
    /// 0..1 value, exposure included.
    pub fn transform(&self) -> (f32, f32) {
        let span = self.high - self.low;
        let gain = if span.abs() > f32::EPSILON {
            self.exposure_stops.exp2() / span
        } else {
            self.exposure_stops.exp2()
        };
        (self.low, gain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::{AlphaMode, Channels, ColorSpace, Primaries, Samples};

    fn gray(data: Vec<u16>, transfer: Transfer) -> DecodedImage {
        DecodedImage {
            width: data.len() as u32,
            height: 1,
            samples: Samples::U16 {
                channels: Channels::Gray,
                data,
            },
            color: ColorSpace {
                transfer,
                primaries: Primaries::Bt709,
            },
            alpha: AlphaMode::Opaque,
            value_range: None,
            nodata: None,
        }
    }

    /// The distinction the whole default rests on. A photograph has already
    /// been graded, so touching its window would be second-guessing whoever
    /// made it; sensor counts have not, and showing them raw is a black frame.
    #[test]
    fn display_referred_images_are_left_alone_and_scene_referred_are_stretched() {
        let photographic = gray(vec![0, 1000, 4095], Transfer::Srgb);
        let display = Display::for_image_with(
            &photographic,
            &Stats::scan(&photographic),
            Startup::default(),
        );
        assert_eq!(display.auto, AutoWindow::Off);
        assert_eq!((display.low, display.high), (0.0, 1.0));

        let measurement = gray(vec![0, 1000, 4095], Transfer::Linear);
        let display =
            Display::for_image_with(&measurement, &Stats::scan(&measurement), Startup::default());
        assert_eq!(display.auto, AutoWindow::Percentile);
        assert!(display.high < 0.1, "12-bit data windowed to its own range");
    }

    #[test]
    fn a_declared_value_range_beats_scanning() {
        let mut image = gray(vec![0, 1000, 4095], Transfer::Linear);
        image.value_range = Some((0.1, 0.2));
        let display = Display::for_image_with(&image, &Stats::scan(&image), Startup::default());
        assert_eq!(display.auto, AutoWindow::Manual);
        assert_eq!((display.low, display.high), (0.1, 0.2));
    }

    #[test]
    fn the_transform_maps_the_window_onto_zero_to_one() {
        let display = Display {
            low: 0.25,
            high: 0.75,
            ..Default::default()
        };
        let (offset, gain) = display.transform();
        assert!(((0.25 - offset) * gain).abs() < 1e-6);
        assert!(((0.75 - offset) * gain - 1.0).abs() < 1e-6);
    }

    #[test]
    fn exposure_is_measured_in_stops() {
        let mut display = Display::default();
        let (_, base) = display.transform();
        display.adjust_exposure(1.0);
        let (_, brighter) = display.transform();
        assert!((brighter / base - 2.0).abs() < 1e-5);

        display.adjust_exposure(-2.0);
        let (_, dimmer) = display.transform();
        assert!((dimmer / base - 0.5).abs() < 1e-5);
    }

    #[test]
    fn a_degenerate_window_still_yields_a_finite_gain() {
        let display = Display {
            low: 0.5,
            high: 0.5,
            ..Default::default()
        };
        let (_, gain) = display.transform();
        assert!(gain.is_finite());
    }

    #[test]
    fn contrast_keeps_the_centre_and_marks_the_window_manual() {
        let mut display = Display {
            low: 0.0,
            high: 1.0,
            ..Default::default()
        };
        display.adjust_contrast(0.5);
        assert_eq!((display.low, display.high), (0.25, 0.75));
        assert_eq!(display.auto, AutoWindow::Manual);
    }

    #[test]
    fn shifting_the_window_keeps_its_width() {
        let mut display = Display {
            low: 0.2,
            high: 0.6,
            ..Default::default()
        };
        display.shift_window(0.5);
        assert!((display.high - display.low - 0.4).abs() < 1e-6);
        assert!((display.low - 0.4).abs() < 1e-6);
    }

    #[test]
    fn cycling_out_of_a_hand_set_window_returns_to_automatic() {
        let image = gray(vec![0, 1000, 4095], Transfer::Linear);
        let stats = Stats::scan(&image);
        let mut display = Display::for_image_with(&image, &stats, Startup::default());

        display.adjust_contrast(0.5);
        assert_eq!(display.auto, AutoWindow::Manual);
        display.cycle_auto(&stats);
        assert_eq!(display.auto, AutoWindow::Off);
        display.cycle_auto(&stats);
        assert_eq!(display.auto, AutoWindow::MinMax);
    }

    #[test]
    fn hdr_content_gets_a_tone_curve_and_ordinary_content_does_not() {
        let sdr = gray(vec![0, 4095], Transfer::Srgb);
        assert_eq!(
            Display::for_image_with(&sdr, &Stats::scan(&sdr), Startup::default()).tone_map,
            ToneMap::Clip
        );

        let hdr = DecodedImage {
            width: 2,
            height: 1,
            samples: Samples::F32 {
                channels: Channels::Gray,
                data: vec![0.5, 8.0],
            },
            color: ColorSpace::LINEAR_BT709,
            alpha: AlphaMode::Opaque,
            value_range: None,
            nodata: None,
        };
        assert_eq!(
            Display::for_image_with(&hdr, &Stats::scan(&hdr), Startup::default()).tone_map,
            ToneMap::Neutral
        );
    }
}
