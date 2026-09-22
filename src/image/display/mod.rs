//! How the numbers in an image become something you can look at: the window
//! applied before display, exposure, tone mapping and false color.
//!
//! All of it is uniform state — changing any of it re-renders, it never
//! re-decodes or re-uploads. [`Display`] is the state; the vocabularies it
//! is set in — the window rules, the curves, the ramps — are each a file
//! beside it.

mod auto;
mod colormap;
mod tone_map;

pub use auto::AutoWindow;
pub use colormap::Colormap;
pub use tone_map::{Headroom, ToneMap};

use super::{Channels, DecodedImage, Referred, Sample, Stats, Transfer};

/// Whether a step from `from` to `to` is a step at all, against a window
/// `width` wide: a press against the plot's end asks for the end, and the
/// end can stand a rounding error off where the handle already is, so a
/// move of a hair is no move. A real step is a twentieth of the window.
fn moved(from: f32, to: f32, width: f32) -> bool {
    (to - from).abs() > 1e-4 * width.abs()
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

/// A quarter of a stop: what the histogram panel's exposure slider snaps
/// to, and what the keys that do the same job step by.
///
/// One step for both, so that a keystroke and a drag cannot be worth
/// different amounts, and the reading beside the slider is always one the
/// keys could have reached. Held here beside [`EXPOSURE_LIMIT`] because the
/// exposure is the model's, whatever sets it.
pub const EV_STEP: f32 = 0.25;

/// The furthest the exposure goes either way, in stops.
const EXPOSURE_LIMIT: f32 = 16.0;

/// Where a meter puts the key of a scene: the reflectance of a gray card,
/// and the one number the people who make scene-linear files agree on — a
/// render pipeline keeps its mid-gray here, and a light probe's ground
/// lands near it once exposed. Not tuned; the convention.
const MIDDLE_GRAY: f32 = 0.18;

#[derive(Clone, Debug)]
pub struct Display {
    /// The window in the file's own linear units: the value mapped to 0 and
    /// the value mapped to 1 *before exposure*. Not the black and white
    /// points — those are [`Display::displayed_bounds`], which folds the
    /// exposure in.
    pub window_low: f32,
    pub window_high: f32,
    pub auto: AutoWindow,
    pub exposure_stops: f32,
    pub tone_map: ToneMap,
    pub colormap: Colormap,
}

impl Default for Display {
    /// Used only when there is no image on screen.
    fn default() -> Self {
        Self {
            window_low: 0.0,
            window_high: 1.0,
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
    /// Scene light — a Radiance picture, an EXR — is the third case, and it
    /// is metered. Its numbers are real, a renderer's units or cd/m², and
    /// spread over more stops than a surface has; no window fits them, and
    /// the one a percentile finds is sized for the light sources, which
    /// leaves everything else black. A meter exposes for the bulk of the
    /// light instead: the key of the scene, [`Stats::key`], is put at
    /// [`MIDDLE_GRAY`], and what that leaves above white is the curve's.
    /// The window stays 0..1 in the file's own units and the meter's
    /// decision goes on the exposure, in stops, where the slider shows it
    /// and `d`/`f` nudge it — the same two dials the panel has for every
    /// file, and a metered file opens with one of them already turned. A
    /// window rule asked for on the command line takes the meter's place:
    /// it has already put the scene's own range at 0..1, and an exposure
    /// pushed on top of that would open the picture stops too bright.
    ///
    /// The tone curve follows from the window and the exposure rather than
    /// from the file: a curve exists to fit values above white into a
    /// surface that stops there, so it is wanted when they leave something
    /// above white and the surface has no room for it — the highlights of a
    /// graded HDR picture, or of a metered scene, on an SDR surface — and
    /// not otherwise. The startup exposure is applied after that decision
    /// is made: it is a setting like any other, and what the file opens
    /// with is a fact about the file.
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
            window_low: 0.0,
            window_high: 1.0,
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
        if image.referred == Referred::Scene && display.auto == AutoWindow::Off {
            display.exposure_stops = Self::metered(stats);
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

    /// The exposure a meter would give the scene, in stops: what puts its
    /// key at [`MIDDLE_GRAY`]. Zero where nothing was lit, and never past
    /// the slider's ends.
    fn metered(stats: &Stats) -> f32 {
        match stats.key {
            Some(key) if key > 0.0 => {
                let stops = (MIDDLE_GRAY / key).log2();
                stops.clamp(-EXPOSURE_LIMIT, EXPOSURE_LIMIT)
            }
            _ => 0.0,
        }
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
        self.window_low == 0.0
            && self.window_high == 1.0
            && self.exposure_stops == 0.0
            && self.tone_map == ToneMap::None
    }

    /// Puts the two values that come out black and white where they are
    /// told to, the exposure staying what it is: the levels track's handles,
    /// each of which is dragged to the value it should stand at.
    ///
    /// The inverse of [`Display::displayed_bounds`]. Exposure lives in the
    /// gain, so the bound that comes out white is `window_low` plus the window's
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
        self.window_low = black;
        self.window_high = black + (white - black) * self.exposure_stops.exp2();
        self.auto = AutoWindow::Manual;
    }

    fn apply_auto(&mut self, stats: &Stats) {
        let (low, high) = match self.auto {
            AutoWindow::Off => (0.0, 1.0),
            AutoWindow::MinMax => (stats.min, stats.max),
            AutoWindow::Percentile => (stats.percentile(0.001), stats.percentile(0.999)),
            AutoWindow::Manual => return,
        };
        self.window_low = low;
        // A degenerate window would divide by zero in the shader.
        self.window_high = if high > low { high } else { low + 1.0 };
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

    /// Puts `curve` on the picture, unless a false color is on it — the
    /// curve is held at a clip there, see [`Display::curve_on`], and a curve
    /// changed under a ramp would have the picture change when the ramp came
    /// off, from a press made long before. `gray` is whether the image has
    /// one channel, which is what a false color is a reading of. Says
    /// whether anything changed. The one place the refusal is made: the key
    /// that cycles the curves and the button that names one both come here.
    pub fn set_tone_map(&mut self, curve: ToneMap, gray: bool) -> bool {
        if self.false_colored(gray) {
            return false;
        }
        let changed = self.tone_map != curve;
        self.tone_map = curve;
        changed
    }

    /// Puts `map` on the picture, unless the image is not `gray`: a false
    /// color is a reading of one channel, and means nothing on three. Says
    /// whether anything changed. As with [`Display::set_tone_map`], the key
    /// and the button both come here, so they cannot drift.
    pub fn set_colormap(&mut self, map: Colormap, gray: bool) -> bool {
        if !gray {
            return false;
        }
        let changed = self.colormap != map;
        self.colormap = map;
        changed
    }

    /// The next curve along, under [`Display::set_tone_map`]'s rule.
    pub fn cycle_tone_map(&mut self, gray: bool) -> bool {
        self.set_tone_map(self.tone_map.next(), gray)
    }

    /// The next map along, under [`Display::set_colormap`]'s rule.
    pub fn cycle_colormap(&mut self, gray: bool) -> bool {
        self.set_colormap(self.colormap.next(), gray)
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

    /// Puts the value that comes out white where it is told to, the value
    /// that comes out black staying put and the exposure staying what it
    /// is: the white handle on the histogram's band, the twin of
    /// [`Display::put_black`].
    ///
    /// The window's top on every file, a graded one included. Exposure and
    /// the window's top are two dials for one effect — a stop up is white
    /// moved to half its value with black held — and they are kept honestly
    /// two, the window in the file's own units and the exposure in stops,
    /// so that a handle means the same thing on every file: a hand on
    /// either handle is a hand-set window.
    ///
    /// Refused where `white` is not above black, or is not a number: that is
    /// not a white point.
    pub fn put_white(&mut self, white: f32) {
        let (black, _) = self.displayed_bounds();
        self.set_displayed_bounds(black, white);
    }

    /// The black handle stepped by `fraction` of the window's width along
    /// the plot, and no lower than `floor`, the bottom of the plot: what a
    /// press of the black point's key does.
    ///
    /// Along the plot, which is on the file's own curve, `transfer`, rather
    /// than in linear light: a step is the same distance on the band every
    /// time, as a drag's is. In linear light a twentieth of a 0..1 window
    /// on an sRGB file is a quarter of the band on the first press and a
    /// tenth on the next, the handle bounding out of the shadows and then
    /// slowing, since the curve gives the shadows most of the band.
    ///
    /// The handle stops at the floor because the band does, and a key that
    /// went on past it would be a lift — nothing coming out black — which
    /// is grading and not a place to look. Whether anything moved, since a
    /// press against the floor is a press that did nothing.
    pub fn step_black(&mut self, fraction: f32, transfer: Transfer, floor: f32) -> bool {
        let (black, white) = self.displayed_bounds();
        let (black, white) = (transfer.to_encoded(black), transfer.to_encoded(white));
        // A window already under the floor is not pushed up to it by a
        // press downward: the floor stops a step, it does not make one.
        let asked = (black + (white - black) * fraction).max(floor.min(black));
        if !asked.is_finite() || !moved(black, asked, white - black) {
            return false;
        }
        self.put_black(transfer.to_linear(asked));
        true
    }

    /// The white handle stepped by `fraction` of the window's width along
    /// the plot, and no higher than `ceiling`, the top of the plot: what a
    /// press of the white point's key does, the twin of
    /// [`Display::step_black`] with the ceiling for the floor. A window
    /// already over the ceiling is not pulled down to it by a press upward.
    /// Whether anything moved.
    pub fn step_white(&mut self, fraction: f32, transfer: Transfer, ceiling: f32) -> bool {
        let (black, white) = self.displayed_bounds();
        let (black, white) = (transfer.to_encoded(black), transfer.to_encoded(white));
        let asked = (white + (white - black) * fraction).min(ceiling.max(white));
        if !asked.is_finite() || !moved(white, asked, white - black) {
            return false;
        }
        self.put_white(transfer.to_linear(asked));
        true
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

    /// The curve the picture goes out through, and the room it goes out
    /// into: the chosen one on the surface as it is — or, over a false
    /// color, a plain clip whatever the surface.
    ///
    /// False color is already display-referred: a tone curve on top of a
    /// colormap would distort the mapping the viewer is reading values off,
    /// and headroom above the top of the ramp is a color the ramp does not
    /// have. The one place the choice is made: `render/composite.rs` asks
    /// this for the arm the shader runs, and [`Display::map`] runs the same
    /// arm on the CPU for the readouts, so neither can stop describing the
    /// screen the other draws.
    pub fn curve_on(&self, gray: bool, headroom: Headroom) -> (ToneMap, Headroom) {
        if self.false_colored(gray) {
            (ToneMap::None, Headroom::None)
        } else {
            (self.tone_map, headroom)
        }
    }

    /// [`Display::curve_on`] applied to one color.
    fn curve(&self, gray: bool, headroom: Headroom, color: [f32; 3]) -> [f32; 3] {
        let (tone_map, headroom) = self.curve_on(gray, headroom);
        tone_map.apply(color, headroom)
    }

    /// `(offset, gain)` such that `(value - offset) * gain` is the displayed
    /// 0..1 value, exposure included.
    pub fn transform(&self) -> (f32, f32) {
        let span = self.window_high - self.window_low;
        let gain = if span.abs() > f32::EPSILON {
            self.exposure_stops.exp2() / span
        } else {
            self.exposure_stops.exp2()
        };
        (self.window_low, gain)
    }

    /// The two values on the image's own scale that come out as displayed 0
    /// and 1: the window as the shader actually applies it.
    ///
    /// Not `(window_low, window_high)`. Exposure is folded into the gain rather than into
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

    /// `(value - window_low) * gain`: one value through the window with its
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
    /// `(value - window_low) * gain` per color channel, before the tone curve: a
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

    /// Linear float gray as a measurement: numbers that need not be light,
    /// windowed to what they hold.
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
            referred: Referred::Measured,
            exposure: None,
            nodata: None,
            gain_map: None,
        }
    }

    /// The same numbers as scene light, which is what a Radiance picture or
    /// an EXR comes out as: in the file's own scale, with no white stated.
    fn scene_gray(data: Vec<f32>) -> DecodedImage {
        let mut image = float_gray(data);
        image.referred = Referred::Scene;
        image
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
            exposure: None,
            nodata: None,
            gain_map: None,
        }
    }

    /// The distinction the whole default rests on. A photograph has already
    /// been graded, so touching its window would be second-guessing whoever
    /// made it; sensor counts have not, and showing them raw is a black frame.
    #[test]
    fn display_referred_images_are_left_alone_and_measurements_are_stretched() {
        let photographic = gray(vec![0, 1000, 4095], Transfer::Srgb);
        let display = Display::for_image_with(
            &photographic,
            &Stats::scan(&photographic),
            Startup::default(),
            Headroom::None,
        );
        assert_eq!(display.auto, AutoWindow::Off);
        assert_eq!((display.window_low, display.window_high), (0.0, 1.0));

        let measurement = gray(vec![0, 1000, 4095], Transfer::Linear);
        let display = Display::for_image_with(
            &measurement,
            &Stats::scan(&measurement),
            Startup::default(),
            Headroom::None,
        );
        assert_eq!(display.auto, AutoWindow::Percentile);
        assert!(
            display.window_high < 0.1,
            "12-bit data windowed to its own range"
        );
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
            assert_eq!(
                (display.window_low, display.window_high),
                (0.0, 1.0),
                "{transfer:?}"
            );
            assert_eq!(display.tone_map, ToneMap::Neutral, "{transfer:?}");
        }
    }

    /// What the histogram's ramp is painted with. The ends are the reason it
    /// is worth drawing: past either edge of the window the screen has one
    /// color and no more, and the band shows how much of the axis that is.
    #[test]
    fn a_value_is_shaded_the_way_the_screen_shows_it() {
        let mut display = Display::default();
        (display.window_low, display.window_high) = (0.25, 0.75);

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
            tone_map: ToneMap::Neutral,
            ..Default::default()
        };
        (display.window_low, display.window_high) = (0.0, 2.0);

        for headroom in [Headroom::None, Headroom::Above] {
            let shaded = display.shade(1.0, Channels::Gray, headroom);
            assert_eq!(shaded, Colormap::Magma.color(0.5), "{headroom:?}");
            let curved = ToneMap::Neutral.apply(Colormap::Magma.color(0.5), headroom);
            assert_ne!(shaded, curved, "{headroom:?}: the curve is held off");
        }
        // Three colors are colors already, and the curve is on them.
        assert_eq!(
            display.shade(1.0, Channels::Rgb, Headroom::None),
            ToneMap::Neutral.apply([0.5; 3], Headroom::None)
        );
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
        assert_eq!((display.window_low, display.window_high), (0.0, 1.0));
        assert_eq!(display.tone_map, ToneMap::Neutral);
    }

    /// The number a readout shows is the window's own scale: whatever was set
    /// as `window_low` reads 0 and whatever was set as `window_high` reads 1, which is what
    /// lets someone check a pixel against the bounds the bar is naming.
    #[test]
    fn mapping_a_pixel_puts_the_window_ends_at_zero_and_one() {
        let image = gray(vec![0, u16::MAX / 2, u16::MAX], Transfer::Linear);
        let display = Display {
            window_low: 0.0,
            window_high: 0.5,
            ..Default::default()
        };

        let low = display.map(&image.sample(0, 0, None).expect("inside"), Headroom::None);
        assert!(low.values()[0].abs() < 1e-6);

        let middle = display.map(&image.sample(1, 0, None).expect("inside"), Headroom::None);
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
            exposure: None,
            nodata: None,
            gain_map: None,
        };
        let sample = image.sample(0, 0, None).expect("inside");

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
        let dark = image.sample(0, 0, None).expect("inside");

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

    #[test]
    fn the_transform_maps_the_window_onto_zero_to_one() {
        let display = Display {
            window_low: 0.25,
            window_high: 0.75,
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
    /// transform rather than from `window_low` and `window_high`.
    #[test]
    fn the_displayed_bounds_follow_exposure() {
        let mut display = Display {
            window_low: 0.25,
            window_high: 0.75,
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
            window_low: 0.25,
            window_high: 0.75,
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
            window_low: 0.5,
            window_high: 0.5,
            ..Default::default()
        };
        let (_, gain) = display.transform();
        assert!(gain.is_finite());
    }

    #[test]
    fn a_step_of_black_moves_that_end_alone_and_stops_at_the_floor() {
        let mut display = Display {
            window_low: 0.2,
            window_high: 0.6,
            ..Default::default()
        };
        assert!(display.step_black(0.5, Transfer::Linear, 0.0));
        assert!((display.window_low - 0.4).abs() < 1e-6);
        assert!((display.window_high - 0.6).abs() < 1e-6);
        assert_eq!(display.auto, AutoWindow::Manual);
        // Down by more than there is room for stops at the floor, and a
        // press against the floor moves nothing.
        assert!(display.step_black(-5.0, Transfer::Linear, 0.1));
        assert!((display.window_low - 0.1).abs() < 1e-6);
        assert!(!display.step_black(-0.05, Transfer::Linear, 0.1));
        assert!((display.window_low - 0.1).abs() < 1e-6);
        // A window that starts under the floor is not lifted to it.
        assert!(!display.step_black(-0.05, Transfer::Linear, 0.5));
        assert!((display.window_low - 0.1).abs() < 1e-6);
    }

    /// A step is a twentieth of the band as drawn, which on a curved file
    /// is a twentieth of the window on that curve: the first press from 0
    /// on an sRGB file lands black at a twentieth of the plot, a value in
    /// the deep shadows, not at a twentieth of the light.
    #[test]
    fn a_step_is_along_the_plot_and_not_through_the_light() {
        let mut display = Display {
            window_low: 0.0,
            window_high: 1.0,
            ..Default::default()
        };
        assert!(display.step_black(0.05, Transfer::Srgb, 0.0));
        assert!((Transfer::Srgb.to_encoded(display.window_low) - 0.05).abs() < 1e-5);
        assert!(display.window_low < 0.005, "{}", display.window_low);
        // And the second press is the same distance along the band again.
        assert!(display.step_black(0.05, Transfer::Srgb, 0.0));
        assert!((Transfer::Srgb.to_encoded(display.window_low) - 0.0975).abs() < 1e-5);
        assert!((display.window_high - 1.0).abs() < 1e-6);
    }

    /// A step of white moves that end alone and stops at the ceiling, the
    /// twin of the black handle's test; and a window already over the
    /// ceiling is not pulled down to it by a press upward.
    #[test]
    fn a_step_of_white_moves_that_end_alone_and_stops_at_the_ceiling() {
        let mut display = Display {
            window_low: 0.2,
            window_high: 0.6,
            ..Default::default()
        };
        let linear = Transfer::Linear;
        assert!(display.step_white(0.5, linear, 1.0));
        assert!((display.window_low - 0.2).abs() < 1e-6);
        assert!((display.window_high - 0.8).abs() < 1e-6);
        assert_eq!(display.exposure_stops, 0.0);
        assert_eq!(display.auto, AutoWindow::Manual);
        assert!(display.step_white(5.0, linear, 1.0));
        assert!((display.window_high - 1.0).abs() < 1e-6);
        assert!(!display.step_white(0.05, linear, 1.0));
        assert!(!display.step_white(0.05, linear, 0.5));
        assert!((display.window_high - 1.0).abs() < 1e-6);
    }

    /// On a graded file too: its window opens at 0..1, and the white key
    /// steps the top of it along the plot, as the black key steps the
    /// bottom, with the exposure left as it was. A graded file's plot is
    /// 0..1, so the top cannot be stepped past 1.
    #[test]
    fn a_step_of_white_is_the_windows_top_on_graded_light_too() {
        let mut display = Display::default();
        let srgb = Transfer::Srgb;
        assert!(display.step_white(-0.05, srgb, 1.0));
        assert_eq!(display.exposure_stops, 0.0);
        assert_eq!(display.window_low, 0.0);
        assert_eq!(display.auto, AutoWindow::Manual);
        let (_, white) = display.displayed_bounds();
        assert!((srgb.to_encoded(white) - 0.95).abs() < 1e-5, "{white}");
        // Along the plot: the second press is the same distance on the band.
        assert!(display.step_white(-0.05, srgb, 1.0));
        let (_, white) = display.displayed_bounds();
        assert!((srgb.to_encoded(white) - 0.9025).abs() < 1e-5, "{white}");
        // And back up as far as the plot's top, no further.
        assert!(display.step_white(5.0, srgb, 1.0));
        assert!(
            (display.window_high - 1.0).abs() < 1e-5,
            "{}",
            display.window_high
        );
        assert!(!display.step_white(0.05, srgb, 1.0));
        assert_eq!(display.exposure_stops, 0.0);
    }

    #[test]
    fn cycling_out_of_a_hand_set_window_returns_to_automatic() {
        let image = gray(vec![0, 1000, 4095], Transfer::Linear);
        let stats = Stats::scan(&image);
        let mut display =
            Display::for_image_with(&image, &stats, Startup::default(), Headroom::None);

        display.step_black(0.05, Transfer::Linear, 0.0);
        assert_eq!(display.auto, AutoWindow::Manual);
        display.cycle_auto(&stats);
        assert_eq!(display.auto, AutoWindow::Off);
        display.cycle_auto(&stats);
        assert_eq!(display.auto, AutoWindow::MinMax);
    }

    fn opened(image: &DecodedImage, headroom: Headroom) -> Display {
        Display::for_image_with(image, &Stats::scan(image), Startup::default(), headroom)
    }

    /// The curve is for highlights the window and the exposure leave above
    /// white on a surface that stops there. A graded HDR picture has them; an
    /// ordinary one does not; a measurement — however wide its numbers — is
    /// windowed to what it holds first, which leaves nothing above white for
    /// a curve to act on and would make a curve a bend in the data for no
    /// reason; and a metered scene has whatever the meter left above white,
    /// which is the curve's to roll off.
    #[test]
    fn a_curve_is_added_only_where_the_window_leaves_highlights_above_white() {
        let photograph = gray(vec![0, 4095], Transfer::Srgb);
        assert_eq!(opened(&photograph, Headroom::None).tone_map, ToneMap::None);

        let pq = gray(vec![30_000, 60_000], Transfer::Pq);
        assert_eq!(opened(&pq, Headroom::None).tone_map, ToneMap::Neutral);

        // Sensor counts: linear float that reaches well past 1.0, and is
        // windowed to its own range rather than curved.
        let measurement = float_gray(vec![0.0, 100.0, 4000.0, 4095.0]);
        let display = opened(&measurement, Headroom::None);
        assert_eq!(display.auto, AutoWindow::Percentile);
        assert_eq!(display.tone_map, ToneMap::None);

        // A render: a dim room and one light, in whatever units. Exposed for
        // the room, which leaves the light above white for the curve.
        let mut room = vec![0.02; 15];
        room.push(50.0);
        let render = scene_gray(room);
        let display = opened(&render, Headroom::None);
        assert_eq!(display.auto, AutoWindow::Off);
        assert_eq!(display.tone_map, ToneMap::Neutral);
        assert_eq!(opened(&render, Headroom::Above).tone_map, ToneMap::None);

        // The same render with its light no brighter than its key has
        // nothing above white once metered, and no curve for no reason.
        let flat = scene_gray(vec![0.02; 16]);
        assert_eq!(opened(&flat, Headroom::None).tone_map, ToneMap::None);
    }

    /// Scene light opens metered: the window left at 0..1 of the file's own
    /// units, and the exposure turned to put the key of the scene at middle
    /// gray — whatever the scale the file was made in, so a render in a
    /// renderer's units and the same render in cd/m² open looking the same.
    /// The panel then has the decision on its exposure slider, in stops,
    /// where a hand can move it on from.
    #[test]
    fn scene_light_opens_with_its_key_at_middle_gray() {
        // A flat scene: every pixel is the key.
        let key = 0.03;
        let scene = scene_gray(vec![key; 8]);
        let stats = Stats::scan(&scene);
        let display = Display::for_image_with(&scene, &stats, Startup::default(), Headroom::None);
        assert_eq!(display.auto, AutoWindow::Off);
        assert_eq!((display.window_low, display.window_high), (0.0, 1.0));
        let shown = display
            .map(&scene.sample(0, 0, None).unwrap(), Headroom::None)
            .values()[0];
        assert!((shown - MIDDLE_GRAY).abs() < 1e-3, "key shown at {shown}");

        // The same scene a thousand times brighter opens the same.
        let bright = scene_gray(vec![key * 1000.0; 8]);
        let stats = Stats::scan(&bright);
        let brighter = Display::for_image_with(&bright, &stats, Startup::default(), Headroom::None);
        let shown = brighter
            .map(&bright.sample(0, 0, None).unwrap(), Headroom::None)
            .values()[0];
        assert!((shown - MIDDLE_GRAY).abs() < 1e-3, "key shown at {shown}");
        assert!((brighter.exposure_stops - (display.exposure_stops - 1000f32.log2())).abs() < 1e-3);

        // What the command line says still wins, as it does for every file.
        let asked = Display::for_image_with(
            &scene,
            &Stats::scan(&scene),
            Startup {
                exposure_stops: Some(1.0),
                ..Startup::default()
            },
            Headroom::None,
        );
        assert_eq!(asked.exposure_stops, 1.0);

        // And a reset puts the meter's reading back, not zero.
        let mut moved = display.clone();
        moved.adjust_exposure(3.0);
        moved.reset(&Stats::scan(&scene), &scene, Headroom::None);
        assert_eq!(moved.exposure_stops, display.exposure_stops);

        // A window rule asked for at startup has put the scene's range at
        // 0..1 already; the meter would only push it past white.
        let windowed = Display::for_image_with(
            &scene,
            &Stats::scan(&scene),
            Startup {
                auto: Some(AutoWindow::Percentile),
                ..Startup::default()
            },
            Headroom::None,
        );
        assert_eq!(windowed.auto, AutoWindow::Percentile);
        assert_eq!(windowed.exposure_stops, 0.0);
    }

    /// A scene with nothing lit, or one that would want more than the slider
    /// has, opens at the slider's ends rather than off them.
    #[test]
    fn the_meter_stays_on_the_slider() {
        let dark = scene_gray(vec![0.0; 4]);
        assert_eq!(opened(&dark, Headroom::None).exposure_stops, 0.0);

        let faint = scene_gray(vec![1e-9; 4]);
        assert_eq!(
            opened(&faint, Headroom::None).exposure_stops,
            EXPOSURE_LIMIT
        );

        let blinding = scene_gray(vec![1e9; 4]);
        assert_eq!(
            opened(&blinding, Headroom::None).exposure_stops,
            -EXPOSURE_LIMIT
        );
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
            stats.max > display.window_high,
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
            tone_map: ToneMap::Neutral,
            ..Default::default()
        };
        let rolled = ToneMap::Neutral.apply([4.0; 3], Headroom::Above)[0];
        assert!(rolled < 1.0, "{rolled}");
        assert!(display.false_colored(true));
        assert!(!display.false_colored(false));
        assert_eq!(display.response(4.0, true, Headroom::Above), 1.0);
        assert_eq!(display.response(4.0, false, Headroom::Above), rolled);

        let gray = Display {
            colormap: Colormap::Gray,
            ..display
        };
        assert!(!gray.false_colored(true));
        assert_eq!(gray.response(4.0, true, Headroom::Above), rolled);
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
        assert_eq!(display.window_low, 0.2);
        assert_eq!(display.window_high, 0.6);

        // With a stop of exposure on, the same request lands the same two
        // values at black and white — which means a wider window under the
        // gain, since the gain is what carries the exposure.
        display.exposure_stops = 1.0;
        display.set_displayed_bounds(0.2, 0.6);
        let (black, white) = display.displayed_bounds();
        assert!((black - 0.2).abs() < 1e-6 && (white - 0.6).abs() < 1e-6);
        assert!(
            (display.window_high - 1.0).abs() < 1e-6,
            "{}",
            display.window_high
        );

        // Refused where the two are not a window, and left as they were.
        display.set_displayed_bounds(0.6, 0.6);
        display.set_displayed_bounds(0.7, 0.6);
        display.set_displayed_bounds(f32::NAN, 0.6);
        display.set_displayed_bounds(0.2, f32::INFINITY);
        assert!((display.window_high - 1.0).abs() < 1e-6);
        assert_eq!(display.window_low, 0.2);
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
                window_high: 0.5,
                ..identity.clone()
            }
            .is_identity()
        );
        assert!(
            !Display {
                window_low: 0.1,
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

    /// Each handle moves its own end of the window, whatever the file, and
    /// the exposure is left alone by both: a hand-set white under a stop of
    /// exposure is that white, with the stop still on top of it.
    #[test]
    fn the_white_handle_is_the_windows_top_whatever_the_file() {
        let mut display = Display::default();
        display.put_black(0.1);
        assert_eq!(display.displayed_bounds(), (0.1, 1.0));
        display.put_white(0.55);
        assert_eq!(display.exposure_stops, 0.0);
        assert_eq!(display.auto, AutoWindow::Manual);
        let (black, white) = display.displayed_bounds();
        assert!(
            (black - 0.1).abs() < 1e-6 && (white - 0.55).abs() < 1e-6,
            "{black} {white}"
        );

        // Under a stop of exposure the request lands where it asked, and
        // the window's top is worked back under the gain to put it there.
        let mut display = Display {
            exposure_stops: 1.0,
            ..Display::default()
        };
        display.put_white(0.25);
        assert_eq!(display.exposure_stops, 1.0);
        let (black, white) = display.displayed_bounds();
        assert!(black == 0.0 && (white - 0.25).abs() < 1e-6, "{white}");
        assert!((display.window_high - 0.5).abs() < 1e-6);
        assert_eq!(display.auto, AutoWindow::Manual);

        // Refused where it is not a white point.
        display.put_black(0.1);
        display.put_white(0.05);
        display.put_white(f32::NAN);
        let (black, white) = display.displayed_bounds();
        assert!((black - 0.1).abs() < 1e-6 && (white - 0.25).abs() < 1e-6);
    }

    /// A false color is refused on a color image, by the key's cycle and by
    /// the button's choice alike; and under a false color the curve is held
    /// where it is, the key and the button refused the same way.
    #[test]
    fn a_ramp_reaches_only_a_gray_image_and_holds_the_curve() {
        let mut display = Display::default();
        assert!(!display.set_colormap(Colormap::Viridis, false));
        assert!(!display.cycle_colormap(false));
        assert_eq!(display.colormap, Colormap::Gray);
        assert!(display.set_tone_map(ToneMap::Neutral, false));
        assert!(display.cycle_tone_map(false));
        assert_eq!(display.tone_map, ToneMap::None);

        assert!(display.cycle_colormap(true));
        assert_eq!(display.colormap, Colormap::Viridis);
        assert!(!display.set_tone_map(ToneMap::Neutral, true));
        assert!(!display.cycle_tone_map(true));
        assert_eq!(display.tone_map, ToneMap::None, "held at the clip");
        assert!(display.set_colormap(Colormap::Gray, true));
        assert!(display.set_tone_map(ToneMap::Neutral, true));
    }
}
