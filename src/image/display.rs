//! How the numbers in an image become something you can look at: the window
//! applied before display, exposure, tone mapping and false colour.
//!
//! All of it is uniform state — changing any of it re-renders, it never
//! re-decodes or re-uploads.

use super::{DecodedImage, Sample, Stats, Transfer};

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
    /// `unit`, `minmax`, or `pct`, as the command line names them.
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.to_ascii_lowercase().as_str() {
            "unit" | "off" => AutoWindow::Off,
            "minmax" | "min-max" => AutoWindow::MinMax,
            "pct" | "percentile" => AutoWindow::Percentile,
            _ => return None,
        })
    }

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

    /// The curve itself, applied to one linear colour.
    ///
    /// The GPU runs this on every pixel of every frame as `tone_map` in
    /// `shaders/composite.wgsl`; this is the same arithmetic for the one pixel
    /// a readout has to describe. Keep the two in step — the point of a
    /// readout is that it agrees with the screen.
    pub fn apply(self, color: [f32; 3]) -> [f32; 3] {
        match self {
            ToneMap::Clip => color.map(|c| c.clamp(0.0, 1.0)),
            ToneMap::Reinhard => color.map(|c| {
                let c = c.max(0.0);
                c / (c + 1.0)
            }),
            ToneMap::Neutral => neutral(color.map(|c| c.max(0.0))),
        }
    }
}

/// Khronos PBR Neutral, the twin of `neutral()` in `shaders/composite.wgsl`.
fn neutral(color: [f32; 3]) -> [f32; 3] {
    const START_COMPRESSION: f32 = 0.8 - 0.04;
    const DESATURATION: f32 = 0.15;

    let darkest = color[0].min(color[1]).min(color[2]);
    let offset = if darkest < 0.08 {
        darkest - 6.25 * darkest * darkest
    } else {
        0.04
    };
    let color = color.map(|c| c - offset);

    let peak = color[0].max(color[1]).max(color[2]);
    if peak < START_COMPRESSION {
        return color;
    }

    let d = 1.0 - START_COMPRESSION;
    let new_peak = 1.0 - d * d / (peak + d - START_COMPRESSION);
    let color = color.map(|c| c * (new_peak / peak));

    // Highlights desaturate towards the peak rather than clipping a channel
    // at a time, which is what keeps the hue.
    let g = 1.0 - 1.0 / (DESATURATION * (peak - new_peak) + 1.0);
    color.map(|c| c + (new_peak - c) * g)
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

    /// The colour this map gives to a windowed value, in the linear working
    /// space. Out-of-window values take the end of the ramp, as they do on
    /// screen.
    ///
    /// The same fits `false_color` runs in `shaders/image.wgsl`, and the same
    /// linearisation after them — the polynomials produce sRGB-encoded
    /// values. Two copies of a table is a thing to keep an eye on; the
    /// alternative is a readout that names a colour the screen is not
    /// showing.
    pub fn color(self, value: f32) -> [f32; 3] {
        let t = value.clamp(0.0, 1.0);
        let encoded = match self {
            Colormap::Gray => [t; 3],
            Colormap::Viridis => ramp(&VIRIDIS, t),
            Colormap::Magma => ramp(&MAGMA, t),
            Colormap::Turbo => ramp(&TURBO, t),
        };
        encoded.map(|c| Transfer::Srgb.to_linear(c.clamp(0.0, 1.0)))
    }
}

/// A colormap as its coefficients: one RGB triple per power of the ramp
/// position, lowest first, evaluated by Horner's method.
fn ramp(coefficients: &[[f32; 3]], t: f32) -> [f32; 3] {
    let mut out = [0.0; 3];
    for triple in coefficients.iter().rev() {
        for (slot, coefficient) in out.iter_mut().zip(triple) {
            *slot = *slot * t + coefficient;
        }
    }
    out
}

// Written out to the digit as the shader has them, so that the two tables can
// be checked against each other by eye; f32 keeps rather fewer of them.
#[allow(clippy::excessive_precision)]
const VIRIDIS: [[f32; 3]; 7] = [
    [0.2777273, 0.00540734, 0.33409980],
    [0.10509304, 1.40461353, 1.38459016],
    [-0.33086183, 0.21484756, 0.09509516],
    [-4.63423050, -5.79910097, -19.33244096],
    [6.22826994, 14.17993337, 56.69055260],
    [4.77638500, -13.74514538, -65.35303263],
    [-5.43545586, 4.64585261, 26.31241433],
];

#[allow(clippy::excessive_precision)]
const MAGMA: [[f32; 3]; 7] = [
    [-0.00213649, -0.00074966, -0.00538613],
    [0.25166054, 0.67752324, 2.49402660],
    [8.35371728, -3.57771951, 0.31446790],
    [-27.66873309, 14.26473078, -13.64921319],
    [52.17613981, -27.94360607, 12.94416944],
    [-50.76852536, 29.04658282, 4.23415299],
    [18.65570507, -11.48977352, -5.60196151],
];

/// The shader writes this one as two dot products per channel; it is the same
/// degree-five polynomial, transposed to a triple per power.
#[allow(clippy::excessive_precision)]
const TURBO: [[f32; 3]; 6] = [
    [0.13572138, 0.09140261, 0.10667330],
    [4.61539260, 2.19418839, 12.64194608],
    [-42.66032258, 4.84296658, -60.58204836],
    [132.13108234, -14.18503333, 110.36276771],
    [-152.94239396, 4.27729857, -89.90310912],
    [59.28637943, 2.82956604, 27.34824973],
];

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
    /// counts have not, and showing them unwindowed is how you get a black
    /// rectangle.
    ///
    /// PQ and HLG belong with the graded ones. They are absolute curves —
    /// 1.0 is reference white and the headroom above it is where the
    /// highlights were put on purpose — so stretching their observed range
    /// into 0..1 would undo exactly the grading they exist to carry, and
    /// would do it differently for every frame of a sequence.
    pub fn for_image_with(image: &DecodedImage, stats: &Stats, startup: Startup) -> Self {
        let display_referred = matches!(
            image.color.transfer,
            Transfer::Srgb | Transfer::Gamma(_) | Transfer::Pq | Transfer::Hlg
        );
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

    /// What this display state makes of one pixel: the number it becomes and
    /// the colour it comes out as. The readout in the bottom bar is this run
    /// for whichever pixel the pointer is over.
    pub fn map(&self, sample: &Sample) -> Mapped {
        let (offset, gain) = self.transform();
        let mut values = [0.0; 3];
        for (slot, value) in values.iter_mut().zip(sample.color()) {
            *slot = (value - offset) * gain;
        }
        let count = sample.channels.color_count();

        // The order the pipeline uses: window, then false colour for a single
        // channel, then the tone curve over whatever that produced.
        let color = match (sample.channels.is_gray(), self.colormap) {
            (true, Colormap::Gray) => [values[0]; 3],
            (true, colormap) => colormap.color(values[0]),
            (false, _) => values,
        };

        Mapped {
            values,
            count,
            color: self.tone_map.apply(color),
            alpha: sample.alpha,
        }
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

    /// The two values on the image's own scale that come out as displayed 0
    /// and 1: the window as the shader actually applies it.
    ///
    /// Not `(low, high)`. Exposure is folded into the gain rather than into
    /// the bounds, so a stop of it halves the distance to white while leaving
    /// both where they were. The histogram's markers are drawn from this, and
    /// a marker taken from the bounds would name a value the shader is not
    /// clipping at.
    pub fn displayed_bounds(&self) -> (f32, f32) {
        let (offset, gain) = self.transform();
        (offset, offset + 1.0 / gain)
    }

    /// What the screen makes of one value on the image's own linear scale:
    /// the window, the exposure and the tone curve, as a number from 0 to 1.
    ///
    /// The neutral axis of the pipeline — a grey fed through it — which is
    /// what the histogram draws as its response curve. [`Display::map`] is
    /// the same arithmetic for a whole pixel, where the false colour and a
    /// tone curve's cross-channel terms also come in; a curve for those would
    /// be three curves, and the panel is asking a one-dimensional question.
    pub fn response(&self, value: f32) -> f32 {
        let (offset, gain) = self.transform();
        self.tone_map.apply([(value - offset) * gain; 3])[0]
    }
}

/// One pixel as the display transform leaves it.
#[derive(Clone, Copy, Debug)]
pub struct Mapped {
    values: [f32; 3],
    count: usize,
    /// The colour on screen, in the linear BT.709 the compositor works in.
    ///
    /// Not simply [`Mapped::values`] repeated: a false colour is three
    /// components where the value is one, and the tone curve has moved both
    /// by the time they reach the surface.
    pub color: [f32; 3],
    /// Coverage, carried through from the sample. Nothing above windows it.
    pub alpha: f32,
}

impl Mapped {
    /// `(value - low) * gain` per colour channel, before the tone curve: a
    /// highlight over the window reads as the number it is rather than as the
    /// 1.0 it is about to be clipped to, which is the whole use of a readout
    /// on measurement work.
    pub fn values(&self) -> &[f32] {
        &self.values[..self.count]
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

    /// PQ carries its grading in absolute luminance, so the window has to
    /// stay at reference white and let the tone map deal with the headroom
    /// above it. Stretching a PQ frame's observed range into 0..1 is how a
    /// dim night shot comes out looking like noon.
    #[test]
    fn absolute_hdr_curves_open_at_reference_white_rather_than_stretched() {
        for transfer in [Transfer::Pq, Transfer::Hlg] {
            // A frame whose highlights reach far above reference white, and
            // whose darkest sample is nowhere near zero.
            let frame = gray(vec![30_000, 45_000, 60_000], transfer);
            let stats = Stats::scan(&frame);
            let display = Display::for_image_with(&frame, &stats, Startup::default());

            assert!(stats.max > 1.0, "{transfer:?} should exceed SDR white");
            assert_eq!(display.auto, AutoWindow::Off, "{transfer:?}");
            assert_eq!((display.low, display.high), (0.0, 1.0), "{transfer:?}");
            assert_eq!(display.tone_map, ToneMap::Neutral, "{transfer:?}");
        }
    }

    #[test]
    fn a_declared_value_range_beats_scanning() {
        let mut image = gray(vec![0, 1000, 4095], Transfer::Linear);
        image.value_range = Some((0.1, 0.2));
        let display = Display::for_image_with(&image, &Stats::scan(&image), Startup::default());
        assert_eq!(display.auto, AutoWindow::Manual);
        assert_eq!((display.low, display.high), (0.1, 0.2));
    }

    /// The number a readout shows is the window's own scale: whatever was set
    /// as `low` reads 0 and whatever was set as `high` reads 1, which is what
    /// lets someone check a pixel against the bounds the bar is naming.
    #[test]
    fn mapping_a_pixel_puts_the_window_ends_at_zero_and_one() {
        let image = gray(vec![0, u16::MAX / 2, u16::MAX], Transfer::Linear);
        let display = Display {
            low: 0.0,
            high: 0.5,
            ..Default::default()
        };

        let low = display.map(&image.sample(0, 0).expect("inside"));
        assert!(low.values()[0].abs() < 1e-6);

        let middle = display.map(&image.sample(1, 0).expect("inside"));
        assert!(
            (middle.values()[0] - 1.0).abs() < 1e-3,
            "{:?}",
            middle.values()
        );
    }

    /// The two halves of a readout answer different questions, so they are
    /// allowed to disagree: the value says how far above the window the pixel
    /// is, and the colour says what the screen did about it.
    #[test]
    fn a_clipped_highlight_reads_as_the_number_it_is_and_shows_as_white() {
        let image = DecodedImage {
            width: 1,
            height: 1,
            samples: Samples::F32 {
                channels: Channels::Rgb,
                data: vec![4.0, 4.0, 4.0],
            },
            color: ColorSpace::LINEAR_BT709,
            alpha: AlphaMode::Opaque,
            value_range: None,
            nodata: None,
        };
        let sample = image.sample(0, 0).expect("inside");

        let clipped = Display::default().map(&sample);
        assert_eq!(clipped.values(), [4.0, 4.0, 4.0]);
        assert_eq!(clipped.color, [1.0, 1.0, 1.0]);

        // A curve keeps it below white instead, and says so in the swatch
        // while leaving the measurement alone.
        let rolled = Display {
            tone_map: ToneMap::Neutral,
            ..Default::default()
        }
        .map(&sample);
        assert_eq!(rolled.values(), [4.0, 4.0, 4.0]);
        assert!(
            rolled.color[0] < 1.0 && rolled.color[0] > 0.8,
            "{:?}",
            rolled.color
        );
    }

    /// A false-coloured pixel has one value and three components of colour,
    /// and only the swatch can say what the second is.
    #[test]
    fn false_colour_leaves_the_value_alone_and_changes_the_colour() {
        let image = gray(vec![0, u16::MAX], Transfer::Linear);
        let mut display = Display::default();
        let dark = image.sample(0, 0).expect("inside");

        assert_eq!(display.map(&dark).color, [0.0; 3], "grey stays grey");

        display.colormap = Colormap::Viridis;
        let mapped = display.map(&dark);
        assert_eq!(mapped.values(), [0.0], "the measurement is untouched");
        assert!(
            mapped.color[2] > mapped.color[1],
            "the bottom of viridis is purple"
        );
    }

    /// The fits are transcribed from `shaders/image.wgsl`, where a mistyped
    /// coefficient would be invisible; against matplotlib's own colours they
    /// are not. Loose, because a seven-term fit is an approximation of a
    /// 256-entry table, but nowhere near loose enough to hide a typo.
    #[test]
    fn the_colormaps_land_on_the_colours_they_are_named_after() {
        let encoded = |map: Colormap, t: f32| {
            map.color(t)
                .map(|channel| Transfer::Srgb.to_encoded(channel))
        };
        let close =
            |got: [f32; 3], want: [f32; 3]| got.iter().zip(want).all(|(a, b)| (a - b).abs() < 0.06);

        // matplotlib: viridis runs #440154 -> #21918c -> #fde725.
        assert!(close(
            encoded(Colormap::Viridis, 0.0),
            [0.267, 0.005, 0.329]
        ));
        assert!(close(
            encoded(Colormap::Viridis, 0.5),
            [0.129, 0.569, 0.549]
        ));
        assert!(close(
            encoded(Colormap::Viridis, 1.0),
            [0.993, 0.906, 0.144]
        ));

        // magma runs #000004 -> #b5367a -> #fcfdbf.
        assert!(close(encoded(Colormap::Magma, 0.0), [0.001, 0.000, 0.014]));
        assert!(close(encoded(Colormap::Magma, 0.5), [0.716, 0.215, 0.475]));
        assert!(close(encoded(Colormap::Magma, 1.0), [0.987, 0.991, 0.749]));

        // turbo runs dark blue -> green -> dark red.
        let middle = encoded(Colormap::Turbo, 0.5);
        assert!(middle[1] > middle[0] && middle[1] > middle[2], "{middle:?}");
        let top = encoded(Colormap::Turbo, 1.0);
        assert!(top[0] > 0.4 && top[2] < 0.2, "{top:?}");

        // Past either end of the window the ramp stops rather than running on
        // into whatever the polynomial does out there.
        assert_eq!(Colormap::Viridis.color(-3.0), Colormap::Viridis.color(0.0));
        assert_eq!(Colormap::Viridis.color(9.0), Colormap::Viridis.color(1.0));
    }

    /// Mirrors of shader code are worth pinning to their anchors: clip is a
    /// clamp, Reinhard sends infinity to one, and the neutral curve leaves
    /// ordinary values where they are before rolling off the top.
    #[test]
    fn the_tone_curves_match_what_the_compositor_does() {
        assert_eq!(ToneMap::Clip.apply([-1.0, 0.5, 2.0]), [0.0, 0.5, 1.0]);

        let reinhard = ToneMap::Reinhard.apply([-1.0, 1.0, 3.0]);
        assert_eq!(reinhard[0], 0.0);
        assert!((reinhard[1] - 0.5).abs() < 1e-6);
        assert!((reinhard[2] - 0.75).abs() < 1e-6);

        let neutral = ToneMap::Neutral.apply([0.2, 0.4, 0.6]);
        for (got, want) in neutral.iter().zip([0.2, 0.4, 0.6]) {
            assert!((got - want).abs() < 0.05, "{neutral:?}");
        }
        // Everything above the shoulder stays inside the display's range.
        for value in [1.0, 4.0, 100.0] {
            let peak = ToneMap::Neutral.apply([value; 3])[0];
            assert!((0.8..=1.0).contains(&peak), "{value} -> {peak}");
        }
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

    /// Exposure moves where white falls without moving the window, so the
    /// markers that say where the window lands have to be taken from the
    /// transform rather than from `low` and `high`.
    #[test]
    fn the_displayed_bounds_follow_exposure() {
        let mut display = Display {
            low: 0.25,
            high: 0.75,
            ..Default::default()
        };
        assert_eq!(display.displayed_bounds(), (0.25, 0.75));

        display.adjust_exposure(1.0);
        let (black, white) = display.displayed_bounds();
        assert_eq!(black, 0.25, "black stays at the foot of the window");
        assert!((white - 0.5).abs() < 1e-6, "a stop halves the way to white");

        // Whatever the state, they are the transform run backwards.
        display.adjust_exposure(-3.0);
        let (offset, gain) = display.transform();
        let (black, white) = display.displayed_bounds();
        assert!(((black - offset) * gain).abs() < 1e-6);
        assert!(((white - offset) * gain - 1.0).abs() < 1e-6);
    }

    /// The curve the histogram draws has to be the pipeline, not a sketch of
    /// it: the window's foot comes out black, its head comes out white under
    /// a clip, and a tone curve bends the top down instead without ever
    /// letting it back below what came before.
    #[test]
    fn the_response_is_the_whole_pipeline_run_on_one_value() {
        let mut display = Display {
            low: 0.25,
            high: 0.75,
            ..Default::default()
        };
        let (black, white) = display.displayed_bounds();
        assert!(display.response(black).abs() < 1e-6);
        assert!((display.response(white) - 1.0).abs() < 1e-6);
        // Clipping is flat on both sides of the window, which is the corner
        // the curve is drawn to show.
        assert_eq!(display.response(0.0), 0.0);
        assert_eq!(display.response(4.0), 1.0);

        display.tone_map = ToneMap::Neutral;
        assert!(display.response(white) < 1.0, "the shoulder rolls off");
        let mut previous = f32::NEG_INFINITY;
        for step in 0..64 {
            let response = display.response(step as f32 / 16.0);
            assert!(response >= previous, "step {step}: {response} < {previous}");
            assert!((0.0..=1.0).contains(&response), "step {step}: {response}");
            previous = response;
        }
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
