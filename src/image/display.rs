//! How the numbers in an image become something you can look at: the window
//! applied before display, exposure, tone mapping and false color.
//!
//! All of it is uniform state — changing any of it re-renders, it never
//! re-decodes or re-uploads.

use super::{Channels, DecodedImage, Referred, Sample, Stats, Transfer};

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

    /// The window an image opens with, which is one rule: the image's own
    /// [`Referred`]. Something already graded has a white of its own and 0..1
    /// is exactly right; linear sensor counts have no white, and showing them
    /// unwindowed is how you get a black rectangle.
    ///
    /// Held apart from [`Display::for_image_with`] because it is also what
    /// the panel's `Auto` button puts back — that button is this rule, where
    /// the three beside it are the windows named outright.
    pub fn default_for(image: &DecodedImage) -> Self {
        match image.referred {
            Referred::Display => AutoWindow::Off,
            Referred::Scene => AutoWindow::Percentile,
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
///
/// A curve is something added: it exists to fit values above white into a
/// surface that stops there. So there are the two curves, and `None` — which
/// is not a third curve but the absence of one, and means whatever the
/// surface makes of the highlights on its own: an SDR surface clamps them at
/// white, and an HDR surface shows them at the brightness they were graded
/// to. Which of the two is [`Headroom`]'s to say, and the surface's; the
/// choice of curve is the viewer's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToneMap {
    /// No curve. Clipping on an SDR surface, which is correct for measurement
    /// work where clipping is a thing to be seen; the highlights as they are
    /// on an HDR one, which is the whole point of asking for one.
    None,
    Reinhard,
    /// Khronos PBR Neutral: keeps hue and saturation far better than a
    /// Reinhard curve, and rolls off highlights without the ACES color cast.
    Neutral,
}

impl ToneMap {
    /// Every curve there is, in the order the key cycles them and the order
    /// the histogram panel's own row of them is drawn in.
    pub const ALL: [ToneMap; 3] = [ToneMap::None, ToneMap::Reinhard, ToneMap::Neutral];

    pub fn label(self) -> &'static str {
        match self {
            ToneMap::None => "none",
            ToneMap::Reinhard => "reinhard",
            ToneMap::Neutral => "neutral",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.to_ascii_lowercase().as_str() {
            "none" => ToneMap::None,
            "reinhard" => ToneMap::Reinhard,
            "neutral" => ToneMap::Neutral,
            _ => return None,
        })
    }

    fn next(self) -> Self {
        match self {
            ToneMap::None => ToneMap::Reinhard,
            ToneMap::Reinhard => ToneMap::Neutral,
            ToneMap::Neutral => ToneMap::None,
        }
    }

    /// The curve a picture gets when nothing has asked for one: none at all
    /// where the surface has room for the highlights, and otherwise a
    /// roll-off where there are highlights above white to roll off — and
    /// none, again, where there are not, since a curve on a picture that
    /// never reaches white is a bend in it for no reason.
    ///
    /// `above_white` is the picture as the display has it: not what kind of
    /// file it is, but whether anything in it comes out past white once the
    /// window is on it. See [`Display::exceeds_white`].
    ///
    /// Held apart from [`Display::for_image_with`] because the surface can
    /// change under a picture — it is settled after the first file is
    /// decoded, and it is switched — so the answer has to be asked for again
    /// whenever it does.
    pub fn default_for(headroom: Headroom, above_white: bool) -> Self {
        match (headroom, above_white) {
            (Headroom::Above, _) | (Headroom::None, false) => ToneMap::None,
            (Headroom::None, true) => ToneMap::Neutral,
        }
    }

    /// The curve itself, applied to one linear color, on a surface with
    /// `headroom`.
    ///
    /// The GPU runs this on every pixel of every frame as `tone_map` in
    /// `shaders/composite.wgsl`, with `shader_codes::tone_map` choosing the
    /// arm the way the match below does; this is the same arithmetic for the
    /// one pixel a readout has to describe. Keep the two in step — the point
    /// of a readout is that it agrees with the screen.
    pub fn apply(self, color: [f32; 3], headroom: Headroom) -> [f32; 3] {
        match (self, headroom) {
            // The hardware clamps on the way into an SDR surface.
            (ToneMap::None, Headroom::None) => color.map(|c| c.clamp(0.0, 1.0)),
            // The negatives go, as they do under every other curve here:
            // undershoot from a bicubic lobe is not light.
            (ToneMap::None, Headroom::Above) => color.map(|c| c.max(0.0)),
            (ToneMap::Reinhard, _) => color.map(|c| {
                let c = c.max(0.0);
                c / (c + 1.0)
            }),
            (ToneMap::Neutral, _) => neutral(color.map(|c| c.max(0.0))),
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

/// False color for single-channel images. Ignored for color images.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Colormap {
    Gray,
    Viridis,
    Magma,
    Turbo,
}

impl Colormap {
    /// Every map, in the order the key cycles them — which is the order the
    /// buttons under the histogram's ramp are laid out in, so that the two
    /// ways of choosing one agree about what comes after what.
    pub const ALL: [Colormap; 4] = [
        Colormap::Gray,
        Colormap::Viridis,
        Colormap::Magma,
        Colormap::Turbo,
    ];

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

    /// The color this map gives to a windowed value, in the linear working
    /// space. Out-of-window values take the end of the ramp, as they do on
    /// screen.
    ///
    /// The same fits `false_color` runs in `shaders/image.wgsl`, and the same
    /// linearization after them — the polynomials produce sRGB-encoded
    /// values. Two copies of a table is a thing to keep an eye on; the
    /// alternative is a readout that names a color the screen is not
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

// Viridis and magma are Matt Zucker's polynomial fits to matplotlib's
// colormaps, from https://www.shadertoy.com/view/WlfXRN, dedicated to the
// public domain under CC0; the colormap data he fitted was CC0 as well. What
// is borrowed is the fit, not the colormap, which is why the license recorded
// in `REUSE.toml` is the fit's.
//
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

/// Turbo's fit is Google's own rather than a third party's: the colormap is
/// Anton Mikhailov's and the approximation Ruofei Du's, published together at
/// <https://gist.github.com/mikhailov-work/0d177465a8151eb6ede1768d51d476c7>
/// under Apache-2.0. `REUSE.toml` records it.
///
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

/// Whether the surface being drawn to has room above SDR white.
///
/// It is what decides what becomes of values over 1.0 when no curve is on,
/// so it belongs to the output rather than to the image or to the user, and
/// is passed to everything here that has to say what the screen shows rather
/// than kept in [`Display`]. `render` resolves it from the surface it managed
/// to get; this layer only has to know which of the two it is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Headroom {
    /// An SDR surface: everything above 1.0 is clamped at white on the way in.
    #[default]
    None,
    /// An HDR surface: the highlights go out at the brightness they were
    /// graded to, and tone mapping is something the viewer asks for rather
    /// than something the output imposes.
    Above,
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

/// How far past white a value has to reach to count as above it: a hair, so
/// that the top of the window itself does not.
const ABOVE_WHITE: f32 = 1.0 + 1e-3;

/// A quarter of a stop: what one press of the histogram panel's exposure
/// buttons is worth, what the keys that do the same job step by, and what
/// the white handle's drag is snapped to on a photograph.
///
/// One step for all of them, so that a press, a keystroke and a drag cannot
/// be worth different amounts, and the label on the button cannot come to
/// disagree with what pressing it does. Held here rather than with the
/// buttons because the model snaps to it too — see [`Display::put_white`].
pub const EV_STEP: f32 = 0.25;

/// The furthest the exposure goes either way, in stops.
const EXPOSURE_LIMIT: f32 = 16.0;

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
            tone_map: ToneMap::None,
            colormap: Colormap::Gray,
        }
    }
}

impl Display {
    /// Sensible starting point for this particular image.
    ///
    /// One rule, and it is the image's [`Referred`]. A JPEG, an sRGB PNG, a
    /// PQ frame or a photograph with its gain map applied has already been
    /// graded by whoever produced it: 1.0 is white, so 0..1 is exactly right
    /// and touching it would be wrong — and for the HDR ones, stretching the
    /// observed range into 0..1 would undo exactly the grading they exist to
    /// carry, differently for every frame of a sequence. Linear sensor counts
    /// have no white, and showing them unwindowed is how you get a black
    /// rectangle, so they are windowed to what they hold.
    ///
    /// The tone curve follows from the window rather than from the file: a
    /// curve exists to fit values above white into a surface that stops
    /// there, so it is wanted when the window leaves something above white
    /// and the surface has no room for it — the highlights of a graded HDR
    /// picture on an SDR surface — and not otherwise. The startup exposure is
    /// applied after that decision is made: it is a setting like any other,
    /// and what the file opens with is a fact about the file.
    ///
    /// `headroom` is the surface's half of that decision. The surface can
    /// change under a picture, and [`Display::adopt`] asks again when it does.
    pub fn for_image_with(
        image: &DecodedImage,
        stats: &Stats,
        startup: Startup,
        headroom: Headroom,
    ) -> Self {
        let auto = AutoWindow::default_for(image);

        let mut display = Self {
            low: 0.0,
            high: 1.0,
            auto,
            exposure_stops: 0.0,
            tone_map: ToneMap::None,
            colormap: Colormap::Gray,
        };
        display.apply_auto(stats);
        if let Some(auto) = startup.auto {
            display.auto = auto;
            display.apply_auto(stats);
        }
        display.adopt(headroom, stats);

        // The false color is a reading of one channel, and a color image's
        // three are colors already: the display ignores it there, and so
        // must the state, or the bar names a map that does nothing.
        if let Some(colormap) = startup.colormap.filter(|_| image.is_gray()) {
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

    /// Takes the tone curve the surface wants for what is on screen: none
    /// where it has room for the highlights, a roll-off where it does not and
    /// there are highlights to roll off. For when the surface has changed
    /// under the picture — it is settled after the first file is decoded, and
    /// it is switched.
    ///
    /// Not what was asked for with `t`: the switch chooses the curve the
    /// surface wants, and `t` changes it afterwards. A curve given on the
    /// command line is a choice rather than a default, and the caller keeps
    /// that one.
    pub fn adopt(&mut self, headroom: Headroom, stats: &Stats) {
        self.tone_map = ToneMap::default_for(headroom, self.exceeds_white(stats));
    }

    /// Whether the picture, as the window and the exposure have it, reaches
    /// past white: whether there is anything for a tone curve to act on, or
    /// for an SDR surface to clip.
    ///
    /// Measured at the value the window was set to put at white, where it was
    /// set from the pixels, and at the brightest pixel otherwise. A
    /// percentile window puts its percentile at white and leaves the outliers
    /// above it by design — that is what the percentile is for — so on that
    /// window the brightest pixel says nothing, and only exposure can carry
    /// the picture past white.
    pub fn exceeds_white(&self, stats: &Stats) -> bool {
        let top = match self.auto {
            AutoWindow::Percentile => stats.percentile(0.999),
            AutoWindow::Off | AutoWindow::MinMax | AutoWindow::Manual => stats.max,
        };
        self.windowed(top) > ABOVE_WHITE
    }

    /// Whether the picture reaches past white on the window it opens with,
    /// before anything has been asked of it: whether the file, shown as
    /// [`Display::for_image_with`] would first show it, has highlights for a
    /// curve to act on.
    ///
    /// A fact about the file rather than about the display in force, which
    /// is what the histogram panel asks when it decides whether to offer the
    /// curves at all: an SDR photograph never reaches white on its own, and
    /// a row of curves under one is a row of things to do about a problem it
    /// does not have. A picture pushed past white by hand can still have a
    /// curve put on it with the key.
    ///
    /// Measured at the brightest pixel, where [`Display::exceeds_white`]
    /// measures a percentile window at its percentile. That one asks whether
    /// the picture is being clipped, and a percentile window leaves its
    /// outliers above white on purpose; this asks whether there is anything
    /// up there at all, and the outliers are exactly that.
    pub fn opens_above_white(image: &DecodedImage, stats: &Stats) -> bool {
        let mut display = Self {
            auto: AutoWindow::default_for(image),
            ..Self::default()
        };
        display.apply_auto(stats);
        display.windowed(stats.max) > ABOVE_WHITE
    }

    /// Whether the values above white are being clipped, on a `gray` or a
    /// color image going out with `headroom`: no curve to fold them into a
    /// surface that stops at white, or a false color, which clips whatever
    /// the curve since a ramp has no color past its end for headroom to
    /// show as. The same question the bottom bar answers with its `clipped`,
    /// the histogram's right corner is a share of, and the shader's mark on
    /// a white pixel waits on.
    pub fn clips_white(&self, gray: bool, headroom: Headroom) -> bool {
        self.false_colored(gray) || (self.tone_map == ToneMap::None && headroom == Headroom::None)
    }

    /// Whether a false color is on the picture: a colormap other than the
    /// gray one, on a `gray` image, which is the only kind it is a reading
    /// of. The one test behind everything that changes under a false color
    /// — the curve held at a clip, the bar naming the ramp instead of the
    /// curve, the histogram's row of curves going dead — so that they cannot
    /// come to disagree about when.
    pub fn false_colored(&self, gray: bool) -> bool {
        gray && self.colormap != Colormap::Gray
    }

    /// Whether the display is doing nothing at all: the window is 0..1, the
    /// exposure is nothing and no curve is on, so every value comes out as
    /// itself. What the histogram asks before it draws the response curve,
    /// which on such a display is the diagonal, and says nothing the axis
    /// under it does not.
    pub fn is_identity(&self) -> bool {
        self.low == 0.0
            && self.high == 1.0
            && self.exposure_stops == 0.0
            && self.tone_map == ToneMap::None
    }

    /// Puts the two values that come out black and white where they are
    /// told to, the exposure staying what it is: the levels track's handles,
    /// each of which is dragged to the value it should stand at.
    ///
    /// The inverse of [`Display::displayed_bounds`]. Exposure lives in the
    /// gain, so the bound that comes out white is `low` plus the window's
    /// width scaled down by the exposure; putting white at `white` means
    /// setting the width so that the scaling lands there. A hand on the
    /// bounds is a hand-set window, whatever rule it was on before.
    ///
    /// Refused where the two are not a window: white at or below black
    /// would divide the shader by zero or turn the picture inside out, and
    /// a bound that is not a number is not a bound.
    pub fn set_displayed_bounds(&mut self, black: f32, white: f32) {
        if !(black.is_finite() && white.is_finite() && white > black) {
            return;
        }
        self.low = black;
        self.high = black + (white - black) * self.exposure_stops.exp2();
        self.auto = AutoWindow::Manual;
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
        self.set_auto(self.auto.next(), stats);
    }

    /// Puts the window on one of the automatic rules and works it out from
    /// the pixels, which is what the panel's row of them does: they name a
    /// window outright where the key steps through them in turn.
    pub fn set_auto(&mut self, auto: AutoWindow, stats: &Stats) {
        self.auto = auto;
        self.apply_auto(stats);
    }

    pub fn cycle_tone_map(&mut self) {
        self.tone_map = self.tone_map.next();
    }

    pub fn cycle_colormap(&mut self) {
        self.colormap = self.colormap.next();
    }

    pub fn adjust_exposure(&mut self, stops: f32) {
        self.set_exposure(self.exposure_stops + stops);
    }

    /// The exposure set outright, as the slider sets it: held to the same
    /// limit a step is, and left alone if asked for nothing.
    pub fn set_exposure(&mut self, stops: f32) {
        if stops.is_finite() {
            self.exposure_stops = stops.clamp(-EXPOSURE_LIMIT, EXPOSURE_LIMIT);
        }
    }

    /// Puts the value that comes out black where it is told to, the value
    /// that comes out white staying put: the black handle on the histogram's
    /// band.
    pub fn put_black(&mut self, black: f32) {
        let (_, white) = self.displayed_bounds();
        self.set_displayed_bounds(black, white);
    }

    /// Puts the value that comes out white where it is told to: the white
    /// handle on the histogram's band. Which of the two things that can put
    /// it there moves is the file's to say.
    ///
    /// A graded file's window is 0..1 and nothing else — that is what graded
    /// means — so on one of those the handle is the exposure: the stops that
    /// land white on `white` over the window as it is, snapped to the
    /// quarter stops the buttons and the keys count in, so that the handle
    /// reaches exactly the numbers they do and the exposure row reads as it
    /// would after so many presses. Measured light has no white of its own,
    /// and there the handle is the window's top, as the black handle is its
    /// bottom, with the exposure left as the push on top of it.
    ///
    /// Refused where `white` is not above black, or is not a number: that is
    /// not a white point.
    pub fn put_white(&mut self, white: f32, referred: Referred) {
        let (black, _) = self.displayed_bounds();
        match referred {
            Referred::Scene => self.set_displayed_bounds(black, white),
            Referred::Display => {
                let span = self.high - self.low;
                if !(white.is_finite() && white > black && span > 0.0) {
                    return;
                }
                let stops = (span / (white - black)).log2();
                self.exposure_stops =
                    ((stops / EV_STEP).round() * EV_STEP).clamp(-EXPOSURE_LIMIT, EXPOSURE_LIMIT);
            }
        }
    }

    /// Widens or narrows the window about its own center, the "level" half of
    /// a window/level control.
    pub fn adjust_contrast(&mut self, factor: f32) {
        let center = (self.low + self.high) / 2.0;
        let half = (self.high - self.low) / 2.0 * factor;
        if half.is_finite() && half > 0.0 {
            self.low = center - half;
            self.high = center + half;
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

    /// Puts the rendering back to what this image would open with if nothing
    /// had been asked for: the window, the exposure and the tone curve as
    /// [`Display::for_image_with`] chooses them for the file itself.
    ///
    /// Not back to what was asked for on the command line. An exposure given
    /// there is a setting like any other, and a reset that answered to it
    /// would do nothing at all for whoever had passed one — which is the one
    /// person who has most reason to press it.
    ///
    /// The false color is left where it is, being the one thing here that is
    /// not a rendering decision: it says which of the file's numbers you are
    /// trying to read, and a reset that threw that away would take the answer
    /// with it. There is a key and a row of buttons for changing it.
    pub fn reset(&mut self, stats: &Stats, image: &DecodedImage, headroom: Headroom) {
        let colormap = self.colormap;
        *self = Self::for_image_with(image, stats, Startup::default(), headroom);
        self.colormap = colormap;
    }

    /// What this display state makes of one pixel on a surface with
    /// `headroom`: the number it becomes and the color it comes out as. The
    /// readout in the bottom bar is this run for whichever pixel the pointer
    /// is over.
    pub fn map(&self, sample: &Sample, headroom: Headroom) -> Mapped {
        let (offset, gain) = self.transform();
        let mut values = [0.0; 3];
        for (slot, value) in values.iter_mut().zip(sample.color()) {
            *slot = (value - offset) * gain;
        }
        let count = sample.channels.color_count();

        // The order the pipeline uses: window, then false color for a single
        // channel, then the tone curve over whatever that produced.
        let color = match (sample.channels.is_gray(), self.colormap) {
            (true, Colormap::Gray) => [values[0]; 3],
            (true, colormap) => colormap.color(values[0]),
            (false, _) => values,
        };

        Mapped {
            values,
            count,
            color: self.curve(sample.channels.is_gray(), headroom, color),
            alpha: sample.alpha,
        }
    }

    /// The tone curve as the compositor runs it over a color: the chosen one
    /// — or, over a false color, a plain clip whatever the surface.
    ///
    /// False color is already display-referred, so `composite.rs` holds the
    /// curve at a clip over it: a tone curve on top of a colormap would
    /// distort the mapping the viewer is reading values off, and headroom
    /// above the top of the ramp is a color the ramp does not have. Every
    /// readout has to make the same choice, or it stops describing the screen
    /// it is meant to be describing.
    fn curve(&self, gray: bool, headroom: Headroom, color: [f32; 3]) -> [f32; 3] {
        if self.false_colored(gray) {
            ToneMap::None.apply(color, Headroom::None)
        } else {
            self.tone_map.apply(color, headroom)
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
    /// the window, the exposure and the tone curve, as a number from 0 — and
    /// past 1 on a surface with room above white and no curve on, since that
    /// is what such a surface shows.
    ///
    /// The neutral axis of the pipeline — a gray fed through it — which is
    /// what the histogram draws as its response curve. [`Display::map`] is
    /// the same arithmetic for a whole pixel, where the false color and a
    /// tone curve's cross-channel terms also come in; a curve for those would
    /// be three curves, and the panel is asking a one-dimensional question.
    /// It still needs to know whether the image is `gray`, since under a
    /// false color the curve is the clip, whatever was chosen.
    pub fn response(&self, value: f32, gray: bool, headroom: Headroom) -> f32 {
        self.curve(gray, headroom, [self.windowed(value); 3])[0]
    }

    /// And the color it comes out as: the window, the false color and the
    /// tone curve, in the order [`Display::map`] runs them.
    ///
    /// The same arithmetic as `map`, asked about a value rather than about a
    /// pixel, which is what lets the histogram paint the display's own output
    /// under the values it is plotting instead of a second drawing of it.
    ///
    /// It needs the channels because the false color is a reading of one:
    /// it is what a gray image is looked at through, and a color image's
    /// three are colors already. Everything below the window comes back
    /// black, and everything above it as the top of the ramp, or as white —
    /// or, on a surface with room above white and no curve on, brighter than
    /// white — because that is what the screen does with it.
    pub fn shade(&self, value: f32, channels: Channels, headroom: Headroom) -> [f32; 3] {
        let windowed = self.windowed(value);
        let color = match (channels.is_gray(), self.colormap) {
            (true, Colormap::Gray) | (false, _) => [windowed; 3],
            (true, colormap) => colormap.color(windowed),
        };
        self.curve(channels.is_gray(), headroom, color)
    }

    /// `(value - low) * gain`: one value through the window with its
    /// exposure, which is where everything the display does begins.
    fn windowed(&self, value: f32) -> f32 {
        let (offset, gain) = self.transform();
        (value - offset) * gain
    }
}

/// One pixel as the display transform leaves it.
#[derive(Clone, Copy, Debug)]
pub struct Mapped {
    values: [f32; 3],
    count: usize,
    /// The color on screen, in the linear BT.709 the compositor works in.
    ///
    /// Not simply [`Mapped::values`] repeated: a false color is three
    /// components where the value is one, and the tone curve has moved both
    /// by the time they reach the surface. Above 1.0 only on a surface with
    /// room above white and no curve on, which is the one case where the
    /// screen is.
    pub color: [f32; 3],
    /// Coverage, carried through from the sample. Nothing above windows it.
    pub alpha: f32,
}

impl Mapped {
    /// `(value - low) * gain` per color channel, before the tone curve: a
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
    use crate::image::{AlphaMode, Channels, ColorSpace, Primaries, Referred, Samples};

    /// Linear float gray, which is what every HDR path here comes out as:
    /// 1.0 is SDR white and anything above it is the headroom.
    fn float_gray(data: Vec<f32>) -> DecodedImage {
        DecodedImage {
            width: data.len() as u32,
            height: 1,
            samples: Samples::F32 {
                channels: Channels::Gray,
                data,
            },
            color: ColorSpace::LINEAR_BT709,
            alpha: AlphaMode::Opaque,
            referred: Referred::Scene,
            nodata: None,
        }
    }

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
            referred: Referred::of(transfer),
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
            Headroom::None,
        );
        assert_eq!(display.auto, AutoWindow::Off);
        assert_eq!((display.low, display.high), (0.0, 1.0));

        let measurement = gray(vec![0, 1000, 4095], Transfer::Linear);
        let display = Display::for_image_with(
            &measurement,
            &Stats::scan(&measurement),
            Startup::default(),
            Headroom::None,
        );
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
            let display =
                Display::for_image_with(&frame, &stats, Startup::default(), Headroom::None);

            assert!(stats.max > 1.0, "{transfer:?} should exceed SDR white");
            assert_eq!(display.auto, AutoWindow::Off, "{transfer:?}");
            assert_eq!((display.low, display.high), (0.0, 1.0), "{transfer:?}");
            assert_eq!(display.tone_map, ToneMap::Neutral, "{transfer:?}");
        }
    }

    /// What the histogram's ramp is painted with. The ends are the reason it
    /// is worth drawing: past either edge of the window the screen has one
    /// color and no more, and the band shows how much of the axis that is.
    #[test]
    fn a_value_is_shaded_the_way_the_screen_shows_it() {
        let mut display = Display::default();
        (display.low, display.high) = (0.25, 0.75);

        let gray = |v: f32| display.shade(v, Channels::Rgb, Headroom::None);
        assert_eq!(gray(0.25), [0.0; 3], "the window's floor comes out black");
        assert_eq!(gray(0.5), [0.5; 3]);
        assert_eq!(gray(0.75), [1.0; 3], "and its ceiling comes out white");
        assert_eq!(gray(0.0), [0.0; 3], "everything below it, clipped to one");
        assert_eq!(gray(1.0), [1.0; 3], "and everything above it, to the other");

        // A color image is three colors already, so the false color is not
        // for it however it is set.
        display.colormap = Colormap::Viridis;
        assert_eq!(display.shade(0.5, Channels::Rgb, Headroom::None), [0.5; 3]);
        assert_eq!(display.shade(0.5, Channels::Rgba, Headroom::None), [0.5; 3]);

        // On a gray one it is, and it clips to the ends of its own ramp
        // rather than to black and white.
        let mapped = |v: f32| display.shade(v, Channels::Gray, Headroom::None);
        assert_eq!(mapped(0.5), Colormap::Viridis.color(0.5));
        assert_eq!(mapped(-1.0), Colormap::Viridis.color(0.0));
        assert_eq!(mapped(9.0), Colormap::Viridis.color(1.0));
        assert_ne!(mapped(0.5), [0.5; 3], "viridis is not gray at mid ramp");
    }

    /// And no curve bends a false color, on either surface: the ramp is read
    /// off the windowed value and clipped at its ends, as the compositor
    /// holds it — a curve over the ramp would pick a different color off it,
    /// not merely a dimmer one, and the readouts have to agree with the
    /// screen about which.
    #[test]
    fn a_false_color_is_read_off_the_ramp_and_no_curve_bends_it() {
        let mut display = Display {
            colormap: Colormap::Magma,
            tone_map: ToneMap::Reinhard,
            ..Default::default()
        };
        (display.low, display.high) = (0.0, 2.0);

        for headroom in [Headroom::None, Headroom::Above] {
            let shaded = display.shade(1.0, Channels::Gray, headroom);
            assert_eq!(shaded, Colormap::Magma.color(0.5), "{headroom:?}");
            let curved = ToneMap::Reinhard.apply(Colormap::Magma.color(0.5), headroom);
            assert_ne!(shaded, curved, "{headroom:?}: the curve is held off");
        }
        // Three colors are colors already, and the curve is on them.
        assert_eq!(
            display.shade(1.0, Channels::Rgb, Headroom::None),
            ToneMap::Reinhard.apply([0.5; 3], Headroom::None)
        );
    }

    /// The key and the row of buttons offer the same maps in the same order.
    #[test]
    fn cycling_the_false_color_walks_the_row_of_them() {
        let mut map = Colormap::ALL[0];
        for expected in Colormap::ALL.into_iter().skip(1) {
            map = map.next();
            assert_eq!(map, expected);
        }
        assert_eq!(map.next(), Colormap::ALL[0], "and round again");
    }

    /// A photograph with its gain map applied is linear float — the signature
    /// of sensor data, which the opening window stretches — but it was graded
    /// before the map lifted its highlights, and its decoder says so. What it
    /// says wins over what the samples look like: the window stays at white,
    /// and the highlights above it get a curve rather than a stretch.
    #[test]
    fn a_decoder_that_calls_linear_light_graded_is_believed() {
        let mut image = float_gray(vec![0.0, 0.5, 1.0, 3.9]);
        image.referred = Referred::Display;
        let stats = Stats::scan(&image);
        let display = Display::for_image_with(&image, &stats, Startup::default(), Headroom::None);
        assert_eq!(display.auto, AutoWindow::Off);
        assert_eq!((display.low, display.high), (0.0, 1.0));
        assert_eq!(display.tone_map, ToneMap::Neutral);
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

        let low = display.map(&image.sample(0, 0).expect("inside"), Headroom::None);
        assert!(low.values()[0].abs() < 1e-6);

        let middle = display.map(&image.sample(1, 0).expect("inside"), Headroom::None);
        assert!(
            (middle.values()[0] - 1.0).abs() < 1e-3,
            "{:?}",
            middle.values()
        );
    }

    /// The two halves of a readout answer different questions, so they are
    /// allowed to disagree: the value says how far above the window the pixel
    /// is, and the color says what the screen did about it.
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
            referred: Referred::Scene,
            nodata: None,
        };
        let sample = image.sample(0, 0).expect("inside");

        let clipped = Display::default().map(&sample, Headroom::None);
        assert_eq!(clipped.values(), [4.0, 4.0, 4.0]);
        assert_eq!(clipped.color, [1.0, 1.0, 1.0]);

        // A curve keeps it below white instead, and says so in the swatch
        // while leaving the measurement alone.
        let rolled = Display {
            tone_map: ToneMap::Neutral,
            ..Default::default()
        }
        .map(&sample, Headroom::None);
        assert_eq!(rolled.values(), [4.0, 4.0, 4.0]);
        assert!(
            rolled.color[0] < 1.0 && rolled.color[0] > 0.8,
            "{:?}",
            rolled.color
        );
    }

    /// A false-colored pixel has one value and three components of color,
    /// and only the swatch can say what the second is.
    #[test]
    fn false_color_leaves_the_value_alone_and_changes_the_color() {
        let image = gray(vec![0, u16::MAX], Transfer::Linear);
        let mut display = Display::default();
        let dark = image.sample(0, 0).expect("inside");

        assert_eq!(
            display.map(&dark, Headroom::None).color,
            [0.0; 3],
            "gray stays gray"
        );

        display.colormap = Colormap::Viridis;
        let mapped = display.map(&dark, Headroom::None);
        assert_eq!(mapped.values(), [0.0], "the measurement is untouched");
        assert!(
            mapped.color[2] > mapped.color[1],
            "the bottom of viridis is purple"
        );
    }

    /// The fits are transcribed from `shaders/image.wgsl`, where a mistyped
    /// coefficient would be invisible; against matplotlib's own colors they
    /// are not. Loose, because a seven-term fit is an approximation of a
    /// 256-entry table, but nowhere near loose enough to hide a typo.
    #[test]
    fn the_colormaps_land_on_the_colors_they_are_named_after() {
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
        assert_eq!(
            ToneMap::None.apply([-1.0, 0.5, 2.0], Headroom::None),
            [0.0, 0.5, 1.0]
        );

        let reinhard = ToneMap::Reinhard.apply([-1.0, 1.0, 3.0], Headroom::None);
        assert_eq!(reinhard[0], 0.0);
        assert!((reinhard[1] - 0.5).abs() < 1e-6);
        assert!((reinhard[2] - 0.75).abs() < 1e-6);

        let neutral = ToneMap::Neutral.apply([0.2, 0.4, 0.6], Headroom::None);
        for (got, want) in neutral.iter().zip([0.2, 0.4, 0.6]) {
            assert!((got - want).abs() < 0.05, "{neutral:?}");
        }
        // Everything above the shoulder stays inside the display's range.
        for value in [1.0, 4.0, 100.0] {
            let peak = ToneMap::Neutral.apply([value; 3], Headroom::None)[0];
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
        assert!(display.response(black, false, Headroom::None).abs() < 1e-6);
        assert!((display.response(white, false, Headroom::None) - 1.0).abs() < 1e-6);
        // Clipping is flat on both sides of the window, which is the corner
        // the curve is drawn to show.
        assert_eq!(display.response(0.0, false, Headroom::None), 0.0);
        assert_eq!(display.response(4.0, false, Headroom::None), 1.0);

        display.tone_map = ToneMap::Neutral;
        assert!(
            display.response(white, false, Headroom::None) < 1.0,
            "the shoulder rolls off"
        );
        let mut previous = f32::NEG_INFINITY;
        for step in 0..64 {
            let response = display.response(step as f32 / 16.0, false, Headroom::None);
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
    fn contrast_keeps_the_center_and_marks_the_window_manual() {
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
        let mut display =
            Display::for_image_with(&image, &stats, Startup::default(), Headroom::None);

        display.adjust_contrast(0.5);
        assert_eq!(display.auto, AutoWindow::Manual);
        display.cycle_auto(&stats);
        assert_eq!(display.auto, AutoWindow::Off);
        display.cycle_auto(&stats);
        assert_eq!(display.auto, AutoWindow::MinMax);
    }

    fn opened(image: &DecodedImage, headroom: Headroom) -> Display {
        Display::for_image_with(image, &Stats::scan(image), Startup::default(), headroom)
    }

    /// The curve is for highlights the window leaves above white on a surface
    /// that stops there. A graded HDR picture has them; an ordinary one does
    /// not; and a measurement — however wide its numbers — is windowed to
    /// what it holds first, which leaves nothing above white for a curve to
    /// act on and would make a curve a bend in the data for no reason.
    #[test]
    fn a_curve_is_added_only_where_the_window_leaves_highlights_above_white() {
        let photograph = gray(vec![0, 4095], Transfer::Srgb);
        assert_eq!(opened(&photograph, Headroom::None).tone_map, ToneMap::None);

        let pq = gray(vec![30_000, 60_000], Transfer::Pq);
        assert_eq!(opened(&pq, Headroom::None).tone_map, ToneMap::Neutral);

        // Sensor counts, and a render: linear float that reaches well past
        // 1.0, and is windowed to its own range rather than curved.
        let measurement = float_gray(vec![0.0, 100.0, 4000.0, 4095.0]);
        let display = opened(&measurement, Headroom::None);
        assert_eq!(display.auto, AutoWindow::Percentile);
        assert_eq!(display.tone_map, ToneMap::None);
    }

    /// The whole point of asking for an HDR surface is the room above SDR
    /// white, so a curve that squeezes the highlights back into 0..1 before
    /// they get there would undo it. The tone map is what the SDR path needs,
    /// not what the content is.
    #[test]
    fn a_surface_with_headroom_starts_with_no_curve_at_all() {
        for above_white in [true, false] {
            assert_eq!(
                ToneMap::default_for(Headroom::Above, above_white),
                ToneMap::None,
                "a surface with headroom takes the pixels as they are"
            );
        }
        assert_eq!(ToneMap::default_for(Headroom::None, true), ToneMap::Neutral);
        assert_eq!(ToneMap::default_for(Headroom::None, false), ToneMap::None);

        let pq = gray(vec![30_000, 60_000], Transfer::Pq);
        assert_eq!(opened(&pq, Headroom::Above).tone_map, ToneMap::None);
    }

    /// The switch chooses the curve the surface wants for what is on screen,
    /// and asks about the picture as it is now rather than as it opened.
    #[test]
    fn adopting_a_surface_re_derives_the_curve_from_what_is_on_screen() {
        let pq = gray(vec![30_000, 60_000], Transfer::Pq);
        let stats = Stats::scan(&pq);
        let mut display = opened(&pq, Headroom::None);
        assert_eq!(display.tone_map, ToneMap::Neutral);

        display.adopt(Headroom::Above, &stats);
        assert_eq!(display.tone_map, ToneMap::None);
        display.adopt(Headroom::None, &stats);
        assert_eq!(display.tone_map, ToneMap::Neutral);

        // A window that brings the highlights under white leaves nothing for
        // a curve to do, on either surface.
        display.auto = AutoWindow::MinMax;
        display.refresh_auto(&stats);
        display.adopt(Headroom::None, &stats);
        assert_eq!(display.tone_map, ToneMap::None);
    }

    /// Whether anything comes out past white is a question about the window
    /// and the exposure, not about the file: an ordinary photograph pushed a
    /// stop up has highlights to clip, and a percentile window's outliers are
    /// what the percentile left out rather than headroom.
    #[test]
    fn exceeding_white_is_measured_where_the_window_put_it() {
        let photograph = gray(vec![0, 32_768, u16::MAX], Transfer::Srgb);
        let stats = Stats::scan(&photograph);
        let mut display = opened(&photograph, Headroom::None);
        assert!(!display.exceeds_white(&stats));
        display.adjust_exposure(1.0);
        assert!(display.exceeds_white(&stats));

        let pq = gray(vec![30_000, 60_000], Transfer::Pq);
        assert!(opened(&pq, Headroom::None).exceeds_white(&Stats::scan(&pq)));

        // A thousand ordinary samples and one hot pixel: the percentile
        // window ignores the hot pixel, and so does this.
        let mut counts: Vec<u16> = (0..1000).map(|count| count * 4).collect();
        counts.push(u16::MAX);
        let measurement = gray(counts, Transfer::Linear);
        let stats = Stats::scan(&measurement);
        let mut display = opened(&measurement, Headroom::None);
        assert_eq!(display.auto, AutoWindow::Percentile);
        assert!(
            stats.max > display.high,
            "the hot pixel is above the window"
        );
        assert!(!display.exceeds_white(&stats));
        // Until exposure carries the window's own top past white.
        display.adjust_exposure(0.5);
        assert!(display.exceeds_white(&stats));
    }

    /// The exposure asked for on the command line is a setting, applied
    /// after the file has decided what it opens with: a photograph opened a
    /// stop up clips rather than quietly acquiring a curve it was not asked
    /// for.
    #[test]
    fn a_startup_exposure_does_not_earn_a_curve() {
        let photograph = gray(vec![0, 4095], Transfer::Srgb);
        let display = Display::for_image_with(
            &photograph,
            &Stats::scan(&photograph),
            Startup {
                exposure_stops: Some(2.0),
                ..Startup::default()
            },
            Headroom::None,
        );
        assert_eq!(display.tone_map, ToneMap::None);
        assert_eq!(display.exposure_stops, 2.0);
    }

    /// A false color is a reading of one channel, and a color image's three
    /// are colors already: the flag reaches a gray image and not a color
    /// one, so that the state never names a map the screen is not applying.
    #[test]
    fn a_startup_colormap_reaches_only_a_gray_image() {
        let startup = Startup {
            colormap: Some(Colormap::Viridis),
            ..Startup::default()
        };
        let gray = gray(vec![0, 4095], Transfer::Srgb);
        let display = Display::for_image_with(&gray, &Stats::scan(&gray), startup, Headroom::None);
        assert_eq!(display.colormap, Colormap::Viridis);

        let color = DecodedImage::new(
            1,
            1,
            Samples::F32 {
                channels: Channels::Rgb,
                data: vec![0.5, 0.5, 0.5],
            },
            ColorSpace::LINEAR_BT709,
            AlphaMode::Opaque,
        );
        let display =
            Display::for_image_with(&color, &Stats::scan(&color), startup, Headroom::None);
        assert_eq!(display.colormap, Colormap::Gray);
    }

    /// No curve means whatever the surface does: an SDR surface clamps at
    /// white, and an HDR one passes the highlights through and clips nothing
    /// but the light that is not there — the shader's arms 0 and 3.
    #[test]
    fn no_curve_is_a_clip_on_sdr_and_a_pass_through_on_hdr() {
        let color = [-0.25, 0.5, 6.31];
        assert_eq!(ToneMap::None.apply(color, Headroom::None), [0.0, 0.5, 1.0]);
        assert_eq!(
            ToneMap::None.apply(color, Headroom::Above),
            [0.0, 0.5, 6.31]
        );
        // The curves are the curves whatever the surface.
        for map in [ToneMap::Reinhard, ToneMap::Neutral] {
            assert_eq!(
                map.apply(color, Headroom::None),
                map.apply(color, Headroom::Above)
            );
        }
    }

    /// And so does a readout of a value: on a surface with room above white
    /// the response runs past 1, which is what the histogram draws, and a
    /// false color is clipped there as everywhere, since the ramp has no
    /// color for what is past its end.
    #[test]
    fn the_response_and_the_shade_run_past_white_only_where_the_surface_does() {
        let display = Display::default();
        assert_eq!(display.response(4.0, false, Headroom::None), 1.0);
        assert_eq!(display.response(4.0, false, Headroom::Above), 4.0);
        assert_eq!(display.shade(4.0, Channels::Rgb, Headroom::Above), [4.0; 3]);

        let false_color = Display {
            colormap: Colormap::Viridis,
            ..Default::default()
        };
        assert_eq!(
            false_color.shade(4.0, Channels::Gray, Headroom::Above),
            Colormap::Viridis.color(1.0)
        );
    }

    /// Under a false color the curve that was chosen is not the curve that
    /// runs: the ramp clips at its end whatever the curve, and the response
    /// says so, since the histogram draws it and a curve on the plot that
    /// the picture is not under would be a curve for nothing. A color image
    /// has no false color, so its curve runs as chosen.
    #[test]
    fn the_response_is_the_clip_under_a_false_color() {
        let display = Display {
            colormap: Colormap::Viridis,
            tone_map: ToneMap::Reinhard,
            ..Default::default()
        };
        assert!(display.false_colored(true));
        assert!(!display.false_colored(false));
        assert_eq!(display.response(4.0, true, Headroom::Above), 1.0);
        assert_eq!(display.response(4.0, false, Headroom::Above), 0.8);

        let gray = Display {
            colormap: Colormap::Gray,
            ..display
        };
        assert!(!gray.false_colored(true));
        assert_eq!(gray.response(4.0, true, Headroom::Above), 0.8);
    }

    /// The handles put the values that come out black and white where they
    /// are dragged to, and they stay put across the exposure: a stop on top
    /// of a hand-set white point is a stop, not a different white point.
    #[test]
    fn the_displayed_bounds_go_where_they_are_put_whatever_the_exposure() {
        let mut display = Display::default();
        display.set_displayed_bounds(0.2, 0.6);
        assert_eq!(display.displayed_bounds(), (0.2, 0.6));
        assert_eq!(
            display.auto,
            AutoWindow::Manual,
            "a hand on the bounds is a hand-set window"
        );
        assert_eq!(display.low, 0.2);
        assert_eq!(display.high, 0.6);

        // With a stop of exposure on, the same request lands the same two
        // values at black and white — which means a wider window under the
        // gain, since the gain is what carries the exposure.
        display.exposure_stops = 1.0;
        display.set_displayed_bounds(0.2, 0.6);
        let (black, white) = display.displayed_bounds();
        assert!((black - 0.2).abs() < 1e-6 && (white - 0.6).abs() < 1e-6);
        assert!((display.high - 1.0).abs() < 1e-6, "{}", display.high);

        // Refused where the two are not a window, and left as they were.
        display.set_displayed_bounds(0.6, 0.6);
        display.set_displayed_bounds(0.7, 0.6);
        display.set_displayed_bounds(f32::NAN, 0.6);
        display.set_displayed_bounds(0.2, f32::INFINITY);
        assert!((display.high - 1.0).abs() < 1e-6);
        assert_eq!(display.low, 0.2);
    }

    /// The display is the identity exactly when nothing has been asked of
    /// it, and any one thing asked of it is enough to make it not.
    #[test]
    fn the_identity_is_the_display_with_nothing_asked_of_it() {
        let identity = Display::default();
        assert!(identity.is_identity());
        assert!(
            !Display {
                exposure_stops: 0.25,
                ..identity.clone()
            }
            .is_identity()
        );
        assert!(
            !Display {
                tone_map: ToneMap::Neutral,
                ..identity.clone()
            }
            .is_identity()
        );
        assert!(
            !Display {
                high: 0.5,
                ..identity.clone()
            }
            .is_identity()
        );
        assert!(
            !Display {
                low: 0.1,
                ..identity.clone()
            }
            .is_identity()
        );
        // The false color is not a change to the values, so it is not one
        // to the response.
        assert!(
            Display {
                colormap: Colormap::Turbo,
                ..identity
            }
            .is_identity()
        );
    }

    /// The black handle moves the black point alone, and the white handle
    /// is the exposure on a graded file and the white point on a measured
    /// one: two dials for one effect on a photograph would be one too many,
    /// and on linear data the two genuinely differ.
    #[test]
    fn the_white_handle_is_the_exposure_on_a_photograph_and_the_window_on_data() {
        let mut display = Display::default();
        display.put_black(0.1);
        assert_eq!(display.displayed_bounds(), (0.1, 1.0));
        assert_eq!(display.exposure_stops, 0.0);

        // A photograph: white to the middle of the range above black is a
        // stop, the window staying where it was.
        display.put_white(0.55, Referred::Display);
        assert_eq!(display.exposure_stops, 1.0);
        assert_eq!((display.low, display.high), (0.1, 1.0));
        let (black, white) = display.displayed_bounds();
        assert!(
            (black - 0.1).abs() < 1e-6 && (white - 0.55).abs() < 1e-6,
            "{white}"
        );
        // Snapped to the quarter stops the buttons count in, so the row of
        // them reads as it would after so many presses.
        display.put_white(0.6, Referred::Display);
        assert_eq!(display.exposure_stops, 0.75);
        // And held to the limit the keys stop at.
        display.put_white(0.1 + 1e-7, Referred::Display);
        assert_eq!(display.exposure_stops, EXPOSURE_LIMIT);
        // Refused where it is not a white point.
        display.put_white(0.05, Referred::Display);
        display.put_white(f32::NAN, Referred::Display);
        assert_eq!(display.exposure_stops, EXPOSURE_LIMIT);

        // Measured light: the same request moves the window's top, and the
        // exposure it carries stays as the push on top of it.
        let mut display = Display {
            exposure_stops: 1.0,
            ..Display::default()
        };
        display.put_white(0.25, Referred::Scene);
        assert_eq!(display.exposure_stops, 1.0);
        let (black, white) = display.displayed_bounds();
        assert!(black == 0.0 && (white - 0.25).abs() < 1e-6, "{white}");
        assert_eq!(display.auto, AutoWindow::Manual);
    }

    /// Whether a file has highlights above white is asked of it as it
    /// opens: an SDR photograph has none, a graded HDR one has, and linear
    /// data has whatever its percentile window leaves out.
    #[test]
    fn a_file_opens_above_white_or_it_does_not() {
        let sdr = gray(vec![0, 30000, 65535], Transfer::Srgb);
        assert!(!Display::opens_above_white(&sdr, &Stats::scan(&sdr)));

        // Graded light with headroom in it — what a PQ frame or a gain-map
        // JPEG decodes to — opens at 0..1 and reaches past the 1.
        let mut hdr = float_gray(vec![0.0, 0.5, 4.0]);
        hdr.referred = Referred::Display;
        assert!(Display::opens_above_white(&hdr, &Stats::scan(&hdr)));

        // Linear data with a lone hot pixel: the percentile window leaves it
        // out, and it stands above white.
        let mut samples = vec![0.5; 2000];
        samples[7] = 100.0;
        let scene = float_gray(samples);
        assert!(Display::opens_above_white(&scene, &Stats::scan(&scene)));
        let flat = float_gray(vec![0.5; 2000]);
        assert!(!Display::opens_above_white(&flat, &Stats::scan(&flat)));
    }

    /// Every curve has to be reachable from every other one, or a viewer on
    /// an HDR surface who presses `t` to see the SDR rendering has no way
    /// back to the one the surface was asked for.
    #[test]
    fn cycling_the_tone_map_returns_to_where_it_started() {
        let mut map = ToneMap::None;
        let mut seen = vec![map];
        for _ in 0..2 {
            map = map.next();
            assert!(!seen.contains(&map), "{map:?} came round twice");
            seen.push(map);
        }
        assert_eq!(map.next(), ToneMap::None);
    }
}
