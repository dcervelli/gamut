//! The histogram, at the head of the side panel: its geometry, the words on
//! it, and the header and rows it is laid out with. The plot is `plot`, the band and
//! its handles `track`, the buttons `controls` and the exposure's slider
//! `slider`, each a file beside this one.

mod controls;
mod plot;
mod slider;
mod track;

use controls::controls;
use plot::{bin_across, plot};
use slider::slider;
use track::track;

use egui::{Align2, Color32, FontId, Sense, WidgetInfo, WidgetType, pos2};

use crate::image::display::{AutoWindow, Colormap, Display, EV_STEP, ToneMap};
use crate::image::stats::BINS;
use crate::render::Color;

use super::Rect;

use super::chrome::{BUTTON_SIZE, ICON_SIDE, Pass};
use super::icon;
use super::outline;
use super::slider::{HANDLE_GRIP, HANDLE_WIDTH, Hand, handle};
use super::style::TOGGLE_RADIUS;
use super::tooltip::Tip;
use super::{BECOMES, Command, Control, Current, PANEL_INSET, PANEL_WIDTH, TEXT_SIZE};

/// What the three rows of settings under the band take off the panel's
/// height: each row and the gap above it, under the plot's own inset —
/// which the band hangs below rather than inside, so it is what is left
/// over between the two and belongs to whatever comes next.
///
/// Three rows, and every file's. The exposure: pushing a picture two stops
/// up is how you find out whether a shadow is empty or merely dark,
/// whatever the file. The window: a graded file's is 0..1 by rights, and
/// the handles move it off that on any file, so the row is where it is put
/// back as much as where a rule is chosen. The curve, because the exposure
/// is: a stop up puts the top of any file above white, and the curve is
/// what fits it back into a surface that stops there. One height for every
/// file, so the panel never jumps from one file to the next.
const ROWS_HEIGHT: f32 = PLOT_INSET + 3.0 * (ROW_GAP + ROW_HEIGHT);

/// The histogram at its own size: the side panel at its narrowest, and tall
/// enough for the line the readout is set on, a plot, the band of what the
/// display makes of its axis, the row of false colors a gray image has
/// under that, and the three rows of settings. What a window has to have
/// room for before the toggle is alive — see [`super::PANELS_ROOM`]. A
/// wider side panel widens it, by whole steps — see [`panel`].
pub const SIZE: [f32; 2] = [
    PANEL_WIDTH,
    2.0 * PANEL_INSET
        + LABEL_HEIGHT
        + PLOT_INSET
        + PLOT_HEIGHT
        + RAMP_GAP
        + SWATCH_HEIGHT
        + RAMP_GAP
        + RAMP_HEIGHT
        + ROWS_HEIGHT,
];

/// The room the strip of buttons down the left takes: a button's width and
/// the gap between it and the plot. Read by [`super::PANEL_WIDTH`], which is
/// this and the plot, so that a bin stays exactly one logical pixel wide.
pub(super) const TOOLBAR_WIDTH: f32 = BUTTON_SIZE + PANEL_INSET;
/// Between one button of that strip and the next. Tighter than the gap the
/// chrome's own strips keep, so that on a color image the four buttons
/// beside the plot leave a gap between the last of them and the one beside
/// the band — see [`marks_button`].
const TOOLBAR_GAP: f32 = 6.0;

/// The middle of the grid a mark is described on, and the two measures the
/// plane toggles are drawn from, in that grid's units: how far each color
/// disc is struck from the middle, and how big the discs are. The luminance
/// disc is one mark where the colors are three, so it is the larger.
const GRID_MIDDLE: f32 = 12.0;
const LUMA_DISC: f32 = 8.0;
const PLANE_ORBIT: f32 = 5.5;
const PLANE_DISC: f32 = 4.0;
/// The row of false colors under the ramp: how deep a swatch is, and the
/// corner it is drawn with. Deep enough to press and to read the map off,
/// shallow enough that the row reads as a legend under the band rather than
/// as a second band.
const SWATCH_HEIGHT: f32 = 16.0;
const SWATCH_RADIUS: f32 = 3.0;
/// What is left around a swatch's color inside its button, so that the
/// button's own lit background is what shows a chosen map.
const SWATCH_INSET: f32 = 3.0;

/// The line above the plot: where the readout goes while the pointer is
/// reading something, and the two ends of the axis where they are worth
/// writing. A whole number of pixels, as everything the panel's height is
/// summed from is, so that the panel ends on one and the column under it
/// starts on one.
const LABEL_HEIGHT: f32 = 18.0;
/// How tall the plot's ground stands on an image with a row of false colors
/// under the band. The row takes its room off the plot rather than off the
/// panel — see [`plot_area`] — so a color image's plot is this and the row
/// besides.
const PLOT_HEIGHT: f32 = 88.0;

/// The corner radius of the plot's own ground inside the panel. Smaller than
/// the panel's, the way an inner corner always is.
const PLOT_RADIUS: f32 = 3.0;
/// The room left around that ground, so the plot reads as set into the panel
/// rather than as a hole cut in it.
const PLOT_INSET: f32 = 4.0;

/// What the luminance plane drops to once color planes are drawn over it.
const HISTOGRAM_LUMA_UNDER: u8 = 110;

/// The response curve's stroke, in logical pixels.
const CURVE_WIDTH: f32 = 1.5;

/// The rule that marks a value, and how wide it is. The accent, like the
/// curve and the handles on the band: everything the panel draws over the
/// bins is the interface talking about them rather than more measurement,
/// and one ink for all of it says so. Held back to translucent, which is
/// what keeps it under the curve it crosses in the reading as well as in the
/// drawing.
const CURSOR_ALPHA: u8 = 190;
const CURSOR_WIDTH: f32 = 1.0;

/// The share of the picture the display is clipping, written in the two top
/// corners of the plot: what it is set at, how far in from the corner, and
/// what is left around it inside the ground it is backed with — the plot's
/// own, laid over whatever bar has climbed into the corner, so the number
/// stays a number on a picture that has piled up at one end.
const CLIP_TEXT: f32 = TEXT_SIZE * 0.75;
const CLIP_INSET: f32 = 3.0;
const CLIP_PAD: f32 = 2.0;
const CLIP_BACKING_ALPHA: u8 = 215;

/// The band under the plot: how deep the color is, and how far it stands
/// off the plot's ground. Deep enough to read a color off and to take hold
/// of, and no deeper — it is a legend along the axis, not a second plot.
const RAMP_HEIGHT: f32 = 10.0;
const RAMP_GAP: f32 = 4.0;

/// How far the band's two handles stand above and below it — see
/// [`slider::handle`](super::slider::handle), which draws them.
const HANDLE_REACH: f32 = 3.0;

/// How far the exposure's slider runs each way, in stops. Not the whole of
/// what the exposure can be — the keys and `--exposure` go on to the
/// model's own limit — but the run a hand wants at a quarter of a stop to
/// the step: six stops each way is a shadow lifted out of black or a
/// highlight brought back from four stops over, and across the slider a
/// step is still a few pixels wide. Past the end the handle stands hollow,
/// as the band's do, and the number beside it says where the exposure is.
const SLIDER_STOPS: f32 = 6.0;
/// An exposure in stops, written the way the interface counts them.
///
/// The step is a quarter, so the numbers the interface actually reaches are
/// quarters, and a decimal cannot write one: a tenth of a stop rounds ¼ to
/// `0.2` and ¾ to `0.8`, which are two different-looking readings of one
/// even pair of steps. The fractions say it exactly and in fewer characters,
/// and they are the units a photographer already counts exposure in.
///
/// Signed unless it is nothing, since what a reader wants from it is which
/// way the picture has been pushed. Anything that is not a whole quarter can
/// only have come from `--exposure`, and falls back to the decimal it was
/// asked for in.
pub fn stops_label(stops: f32) -> String {
    if stops == 0.0 {
        return "0".to_string();
    }
    let quarters = (stops.abs() / EV_STEP).round();
    if (quarters * EV_STEP - stops.abs()).abs() > 1e-4 {
        return format!("{stops:+.2}");
    }
    let sign = if stops < 0.0 { '-' } else { '+' };
    let whole = (quarters as u32) / 4;
    let fraction = match (quarters as u32) % 4 {
        1 => "\u{00bc}",
        2 => "\u{00bd}",
        3 => "\u{00be}",
        _ => "",
    };
    // The whole number is left out where there is none, so a quarter of a
    // stop reads `+¼` rather than `+0¼`.
    let whole = match whole {
        0 if !fraction.is_empty() => String::new(),
        whole => whole.to_string(),
    };
    format!("{sign}{whole}{fraction}")
}

/// The windows the panel offers outright, in the order they are set out,
/// each in the words for what it does rather than the rule's own name: the
/// values taken as they are, the whole of what the image holds, and that
/// range with its outliers trimmed off.
///
/// None of them says which window is in force: the handles on the band do
/// that, and a press here puts the window on a rule rather than switching
/// one on. There is no fourth button for "the image's own", because the
/// file's own is always one of these — *As stored* on a graded file and on
/// scene light, whose meter is on the exposure, *Trimmed* on a measurement
/// — and the reset button puts it back.
pub const WINDOWS: [(&str, AutoWindow); 3] = [
    ("As stored", AutoWindow::Off),
    ("Full range", AutoWindow::MinMax),
    ("Trimmed", AutoWindow::Percentile),
];

/// The rows of controls under the band: how tall a row is, and the gap
/// between one and the next.
const ROW_HEIGHT: f32 = 20.0;
const ROW_GAP: f32 = 8.0;

/// The column of words down the left of those rows, and the gap between one
/// of them and what it names.
///
/// Wide enough for the longest of the three at [`ROW_TEXT`] and no wider:
/// what it does not take is what the buttons beside it have, and the row of
/// three windows is the narrowest cell on the panel.
const ROW_LABEL: f32 = 58.0;
const ROW_LABEL_GAP: f32 = 6.0;

/// How much of the exposure's row is set aside at its end for the reading:
/// the slider runs up to it, and it is set flush with the right edge of the
/// rows below, where their last button ends. Wide enough for the stops the
/// slider reaches and for the two-decimal reading a `--exposure` off the
/// quarters falls back to.
const STOPS_WIDTH: f32 = 40.0;

/// Between one cell of a row and the next: the false colors under the band,
/// and the buttons of the rows below them.
const CELL_GAP: f32 = 4.0;

/// What the words on those rows are set at. The axis labels' size, this being
/// the same panel's second thoughts about the same measurement — and small
/// enough that the longest button label has room in the narrowest cell.
const ROW_TEXT: f32 = TEXT_SIZE * 0.85;

/// The least room left between the pointer's readout and the axis ends it is
/// set between, before they give way to it.
const LABEL_GAP: f32 = 8.0;

/// The word set beside the line that marks white, when white is not the top
/// of the plot.
const WHITE_LABEL: &str = "white";

/// How far past white a value has to reach before it is marked as beyond it:
/// a hair, so that the window's own top does not count.
const ABOVE_WHITE: f32 = 1.0 + 1e-3;

/// Where the pointer's readout goes on the label line — the middle of it —
/// and whether the two ends of the axis still fit either side of it.
///
/// The ends give way rather than the other way about: they are two constants
/// of the image, and the readout is what the pointer was moved there to read.
/// They are only ever in the way on a file whose numbers are wide enough to
/// fill the line between them, and both go together, one end dropped on its
/// own being a line that reads as lopsided rather than as full.
fn readout_placement(bars: Rect, width: f32, ends: [f32; 2]) -> (f32, bool) {
    let x = bars.x + (bars.width - width) / 2.0;
    let fits = x - LABEL_GAP >= bars.x + ends[0] && x + width + LABEL_GAP <= bars.right() - ends[1];
    (x, fits)
}

/// The histogram's own rectangle at the head of the side panel `side`, on a
/// display of `scale` device pixels to the logical one.
///
/// [`SIZE`]'s height, and as wide as the plot comes out with the buttons
/// beside it: the widest plot that gives every bin the same whole number of
/// device pixels, and never narrower than a logical pixel to each. Stepped
/// rather than stretched to the panel: a plot whose bins came out of
/// different widths would be drawing a comb that is not in the picture. So
/// it grows in jumps as the panel widens, and stands in the middle of the
/// panel between them.
///
/// Public because the pointer is tested against it from outside the frame
/// as well: see [`marked`].
pub fn panel(side: Rect, scale: f32) -> Rect {
    let bins = BINS as f32;
    let room = side.width - TOOLBAR_WIDTH - 2.0 * PANEL_INSET;
    let steps = (room * scale / bins).floor();
    let plot = (steps * bins / scale).max(bins);
    let width = TOOLBAR_WIDTH + plot + 2.0 * PANEL_INSET;
    Rect::new(
        side.x + ((side.width - width) / 2.0).max(0.0).round(),
        side.y,
        width,
        SIZE[1],
    )
}

/// The ground the bins stand on inside that panel, with the label line
/// above it. The bins are one logical pixel each, so this is the full width
/// of the plot and the room around it is drawn outside it.
///
/// Set out from the top of the panel down, and so the same whatever rows
/// the file has under it: the rows are what the panel grows by, and the
/// plot does not move for them.
fn bars(panel: Rect) -> Rect {
    let inside = panel.inset(PANEL_INSET, PANEL_INSET);
    Rect::new(
        inside.x + TOOLBAR_WIDTH,
        inside.y + LABEL_HEIGHT + PLOT_INSET,
        inside.width - TOOLBAR_WIDTH,
        PLOT_HEIGHT + RAMP_GAP + SWATCH_HEIGHT,
    )
}

/// The same, on an image the false colors apply to: the row of them takes
/// its room off the bottom of the plot.
///
/// The panel is one height either way, so the information panel below it does
/// not shift about from a gray file to a color one. What changes is how much
/// of that height is plot, which nothing reads absolutely — the bars are
/// drawn as fractions of whatever they are given.
fn plot_area(panel: Rect, gray: bool) -> Rect {
    let bars = bars(panel);
    if !gray {
        return bars;
    }
    Rect::new(bars.x, bars.y, bars.width, PLOT_HEIGHT)
}

/// The buttons down the left that act on the plot, in the order they are
/// stacked beside it. The one beside the band is not among them — see
/// [`marks_button`].
///
/// A control that could not act is left out rather than drawn dead: an image
/// with one channel has no color planes to toggle, and the two that remain
/// close the gap up. Hiding rather than dimming is the panel's rule for both
/// of these — see [`swatch_button`] for the other one.
fn toolbar(gray: bool) -> &'static [Control] {
    const GRAY: [Control; 3] = [Control::Luma, Control::Log, Control::Reset];
    const COLOR: [Control; 4] = [Control::Luma, Control::Planes, Control::Log, Control::Reset];
    if gray { &GRAY } else { &COLOR }
}

/// One of those buttons by its place in the column. Aligned with the top of
/// the plot's ground rather than with the panel, since what they act on is
/// the plot.
fn toolbar_button(panel: Rect, gray: bool, index: usize) -> Rect {
    Rect::new(
        panel.x + PANEL_INSET,
        plot_area(panel, gray).y - PLOT_INSET + index as f32 * (BUTTON_SIZE + TOOLBAR_GAP),
        BUTTON_SIZE,
        BUTTON_SIZE,
    )
}

/// The button that marks the clipped pixels on the picture: at the foot of
/// the strip, beside the band, and centered on it. Beside the band rather
/// than in the stack above, because what it paints is the band's two ends —
/// the pixels the window has taken to black and to white — and not anything
/// about the plot; the stack is the plot's. It sits in the strip's one
/// stretch of room that is not the stack's: on a color image the stack
/// ends a gap above it, and the row of settings under the band begins a
/// hair below it.
fn marks_button(panel: Rect) -> Rect {
    let band = ramp(bars(panel));
    Rect::new(
        panel.x + PANEL_INSET,
        band.y + (band.height - BUTTON_SIZE) / 2.0,
        BUTTON_SIZE,
        BUTTON_SIZE,
    )
}

/// One of the false-color swatches under the ramp, by its place in
/// [`Colormap::ALL`]. They divide the plot's width between them, so each sits
/// under the stretch of band it would color.
///
/// Only on an image they apply to. The display ignores the false color on a
/// three-channel image — its channels are colors already — so on one of
/// those the row is not there at all, and the plot has the room instead.
fn swatch_button(bars: Rect, index: usize) -> Rect {
    let row = Rect::new(
        bars.x,
        ramp(bars).bottom() + RAMP_GAP,
        bars.width,
        SWATCH_HEIGHT,
    );
    share(row, Colormap::ALL.len(), index)
}

/// One of `count` cells dividing `row` between them, with [`CELL_GAP`]
/// between neighbors. What every row of buttons on this panel is set out
/// with, so that the false colors under the band and the windows under those
/// line up down the panel rather than each being spaced its own way.
fn share(row: Rect, count: usize, index: usize) -> Rect {
    let count = count as f32;
    let width = (row.width - (count - 1.0) * CELL_GAP) / count;
    Rect::new(
        row.x + index as f32 * (width + CELL_GAP),
        row.y,
        width,
        row.height,
    )
}

/// The block of controls under the band: the lines it is set on, and the
/// column of words down its left.
///
/// One rectangle for each, worked out once and read by everything — what is
/// drawn, what the pointer finds, and what names itself — so that a button
/// cannot be pressed anywhere but where it was drawn.
///
/// It starts below whichever of the two things the band ends in: the ramp
/// alone on a color image, and the ramp with the row of false colors under it
/// on a gray one. Those end on the same line — the room the swatches take is
/// taken off the plot rather than off the panel, see [`plot_area`] — so this
/// is one block either way, and the rows do not shift about from one file to
/// the next: the three rows are every file's.
#[derive(Clone, Copy)]
struct Rows {
    /// The exposure's line, and where its word goes: the slider along it
    /// and its reading at the end.
    exposure: Rect,
    exposure_label: Rect,
    /// The three windows.
    window: (Rect, Rect),
    /// And the two choices for the curve.
    curve: (Rect, Rect),
}

impl Rows {
    fn new(panel: Rect) -> Self {
        let inside = panel.inset(PANEL_INSET, PANEL_INSET);
        let left = inside.x + ROW_LABEL + ROW_LABEL_GAP;
        let line = |y: f32| {
            (
                Rect::new(left, y, inside.right() - left, ROW_HEIGHT),
                Rect::new(inside.x, y, ROW_LABEL, ROW_HEIGHT),
            )
        };

        let mut y = ramp(bars(panel)).bottom() + ROW_GAP;
        let (exposure, exposure_label) = line(y);
        let mut next = || {
            y += ROW_HEIGHT + ROW_GAP;
            line(y)
        };
        let window = next();
        let curve = next();
        Self {
            exposure,
            exposure_label,
            window,
            curve,
        }
    }

    /// The exposure's reading, at the end of its row.
    fn stops(&self) -> Rect {
        Rect::new(
            self.exposure.right() - STOPS_WIDTH,
            self.exposure.y,
            STOPS_WIDTH,
            self.exposure.height,
        )
    }

    /// The slider, from the row's head to the reading: the room that takes
    /// the pointer, the whole height of the row. The track itself is drawn
    /// inside it, in from each end by half a grip, so that the handle at
    /// either end still stands within the row.
    fn slider(&self) -> Rect {
        Rect::new(
            self.exposure.x,
            self.exposure.y,
            self.stops().x - CELL_GAP - self.exposure.x,
            self.exposure.height,
        )
    }

    /// Each row's word and where it goes, in the order they are stacked.
    fn labels(&self) -> impl Iterator<Item = (&'static str, Rect)> {
        [
            ("Exposure", self.exposure_label),
            ("Window", self.window.1),
            ("Curve", self.curve.1),
        ]
        .into_iter()
    }

    /// The bottom of the lowest row: where the block, and the panel, end.
    #[cfg(test)]
    fn bottom(&self) -> f32 {
        [self.exposure, self.window.0, self.curve.0]
            .into_iter()
            .map(|row| row.bottom())
            .fold(0.0, f32::max)
    }
}

/// What one of the rows' buttons wears, and whether it is lit.
///
/// Beside the geometry rather than inside the drawing so that the test can
/// ask for the same words the frame is set with: a label the layout was not
/// measured against is a label that can outgrow its button.
fn row_label(widget: Control, display: &Display) -> Option<(String, bool)> {
    Some(match widget {
        Control::Window(index) => (WINDOWS.get(index)?.0.to_string(), false),
        // Named for what becomes of the light above white, since that is
        // what the choice is: the bar says the same in the middle of a line.
        Control::Curve(index) => {
            let curve = *ToneMap::ALL.get(index)?;
            let label = match curve {
                ToneMap::None => "Clip",
                ToneMap::Neutral => "Roll off",
            };
            (label.to_string(), display.tone_map() == curve)
        }
        _ => return None,
    })
}

/// Every button in that block, with where it goes. The one list the drawing,
/// the pointer and the tooltips all work from.
fn row_buttons(panel: Rect) -> impl Iterator<Item = (Control, Rect)> {
    let rows = Rows::new(panel);
    let (row, _) = rows.window;
    let windows = (0..WINDOWS.len())
        .map(move |index| (Control::Window(index), share(row, WINDOWS.len(), index)));
    let (row, _) = rows.curve;
    let curves = (0..ToneMap::ALL.len())
        .map(move |index| (Control::Curve(index), share(row, ToneMap::ALL.len(), index)));
    windows.chain(curves)
}

/// The band of color under the plot, aligned with the bins so that a cell of
/// it sits under the bar it belongs to. Below the plot's ground, with room
/// above it for the handles to stand up into.
fn ramp(bars: Rect) -> Rect {
    Rect::new(
        bars.x,
        bars.bottom() + PLOT_INSET + RAMP_GAP,
        bars.width,
        RAMP_HEIGHT,
    )
}

/// The room around a handle at `x` along the band that takes the pointer:
/// wider than the mark it is drawn as, and as tall as the mark stands.
fn grip(band: Rect, x: f32) -> Rect {
    Rect::new(
        x - HANDLE_GRIP / 2.0,
        band.y - HANDLE_REACH,
        HANDLE_GRIP,
        band.height + 2.0 * HANDLE_REACH,
    )
}

/// Which bin the pointer is over, or `None` when it is not over the plot.
fn hovered_bin(bars: Rect, cursor: Option<[f32; 2]>) -> Option<usize> {
    let cursor = cursor.filter(|point| bars.contains(*point))?;
    let bin = (cursor[0] - bars.x) / bars.width * BINS as f32;
    Some((bin as usize).min(BINS - 1))
}

/// Which bin the panel is marking: the one under the pointer while it is over
/// the plot, and otherwise the one that counted the pixel it is over on the
/// picture. `None` when it is over neither.
///
/// The plot comes first: a pointer on the plot is reading the axis, and is
/// on no pixel of the picture besides.
///
/// One bin either way, and so one rule either way: the panel draws bars, and
/// a marker on it can only honestly point at one of them. What the bin is
/// worth is only written out for the first of the two — see [`header`] —
/// since the pixel's own exact numbers are the bottom bar's to report: that
/// readout is about a pixel, and this one is about a bar.
///
/// Public because the pointer is tested against the plot from outside the
/// frame as well: a mark that follows the pointer has to be able to say when
/// the frame it was drawn in has gone out of date.
pub fn marked(
    current: &Current,
    panel: Rect,
    cursor: Option<[f32; 2]>,
    pointer: Option<[u32; 2]>,
) -> Option<usize> {
    let plot = plot_area(panel, current.image.is_gray());
    if let Some(bin) = hovered_bin(plot, cursor) {
        return Some(bin);
    }
    let at = pointer?;
    let sample = current
        .image
        .sample(at[0], at[1], current.lift.as_deref())?;
    current.stats.plot.bin_of(&current.image, &sample)
}

/// An egui rectangle for one of ours.
/// How wide `text` comes out at `size`.
fn width_of(ui: &egui::Ui, text: &str, size: f32) -> f32 {
    crate::ui::text_width(ui, text, FontId::proportional(size))
}

/// A value on the plot's axis, written in the units the file counts in.
///
/// Linear integer data reads back as counts — an elevation model in meters,
/// a sensor's twelve bits — which is what measurement work wants; anything
/// else is the decoded value, to three places with the trailing zeros taken
/// off, so that the ends of a 0..1 axis read `0` and `1` rather than as
/// four decimals of nothing.
fn axis_words(current: &Current, encoded: f32) -> String {
    let transfer = current.image.color.transfer;
    let scale = if transfer.is_linear() {
        current.image.samples.full_scale()
    } else {
        1.0
    };
    let value = transfer.to_linear(encoded) * scale;
    if scale > 1.0 {
        return format!("{value:.0}");
    }
    trimmed(format!("{value:.3}"))
}

/// A decimal with the zeros it does not need taken off the end: `0.500` is
/// `0.5`, and `1.000` is `1`.
fn trimmed(decimal: String) -> String {
    if !decimal.contains('.') {
        return decimal;
    }
    let trimmed = decimal.trim_end_matches('0').trim_end_matches('.');
    if trimmed == "-0" { "0" } else { trimmed }.to_string()
}

/// A share of the picture as the corner of the plot writes it, or `None`
/// for none at all: the corner is empty where nothing is clipped, which is
/// most of the time, and an empty corner is what makes a number in it
/// something to look at.
///
/// A tenth of a percent where the share is small enough for the tenths to
/// mean something, whole percents where they no longer do, and a floor
/// under the smallest share that is not nothing — a handful of pixels in a
/// picture is not `0.0%`, which would say there were none.
fn share_words(share: f32) -> Option<String> {
    // A share that is not a number is not a share: written so that one
    // fails the test rather than passing it.
    if share.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
        return None;
    }
    let percent = share * 100.0;
    Some(if percent < 0.05 {
        "<0.1%".to_string()
    } else if percent < 10.0 {
        format!("{percent:.1}%")
    } else {
        format!("{percent:.0}%")
    })
}

/// Draws the histogram at the head of the side panel `side`.
///
/// Color images get four planes — red, green, blue and luminance — over the
/// range their color channels span; gray images keep the single luminance
/// plane over theirs.
pub(super) fn show(pass: &mut Pass, ui: &mut egui::Ui, current: &Current, side: Rect) {
    let panel = panel(side, pass.input.scale);
    let body = ui.allocate_rect(egui::Rect::from(panel), Sense::CLICK | Sense::DRAG);
    body.widget_info(|| WidgetInfo::labeled(WidgetType::Other, true, "Histogram panel"));
    plot(pass, ui, current, panel);
    let held = controls(pass, ui, current, panel);
    header(pass, ui, current, panel, held);
}

/// The line above the plot: what the pointer is reading, in the middle, and
/// the two ends of the axis at its ends where they are worth writing.
///
/// The middle is the words the hand on the band asked for, where it is on
/// one — `held`, which [`track()`] hands back — and otherwise the bin under
/// the pointer while the pointer is over the plot: the value the bins under
/// the rule were counted at, and what the display makes of that value,
/// which is the height of the response curve where the rule crosses it and
/// the one number a curve on its own cannot be read off by eye. Neither
/// will measure against the plot underneath with a ruler, because both of
/// the plot's axes are spaced in the file's own encoding — the bins across,
/// so that a quantized file does not comb, and the response up, so that a
/// display doing nothing is the diagonal. The positions are the file's units
/// and the numbers are the ones every other readout quotes; a curve that is
/// straight and a value that is comparable cannot both be had, and the shape
/// is what the plot is for.
///
/// The ends are written only where the axis is not the one a graded file
/// always has. A graded file's plot runs from black to white, which every
/// histogram of such a file does and no one needs told; a linear file's
/// runs over whatever was measured, and what that was is the first thing to
/// know about it.
fn header(pass: &Pass, ui: &egui::Ui, current: &Current, panel: Rect, held: Option<String>) {
    let theme = pass.theme;
    let input = pass.input;
    let painter = ui.painter();
    let bars = plot_area(panel, current.image.is_gray());
    let plotted = &current.stats.plot;
    let (axis_min, axis_max) = (plotted.min, plotted.max);
    let span = axis_max - axis_min;
    let transfer = current.image.color.transfer;
    let label_y = panel.y + PANEL_INSET;
    let font = FontId::proportional(ROW_TEXT);

    let nominal = axis_min == 0.0 && (transfer.to_linear(axis_max) - 1.0).abs() < 1e-6;
    let ends =
        (!nominal && span > 0.0).then(|| [axis_min, axis_max].map(|end| axis_words(current, end)));

    let readout = held.or_else(|| {
        let bin = hovered_bin(bars, input.cursor)?;
        let value = transfer.to_linear(axis_min + bin_across(bin) * span);
        let mapped = current
            .display
            .response(value, current.image.is_gray(), input.headroom)
            .max(0.0);
        Some(format!(
            "{}  {BECOMES}  {}",
            axis_words(current, axis_min + bin_across(bin) * span),
            trimmed(format!("{mapped:.3}"))
        ))
    });

    let mut ends_fit = true;
    if let Some(readout) = readout {
        let ends_width = ends.as_ref().map_or([0.0; 2], |ends| {
            ends.each_ref().map(|end| width_of(ui, end, ROW_TEXT))
        });
        let width = width_of(ui, &readout, ROW_TEXT);
        let (x, fits) = readout_placement(bars, width, ends_width);
        ends_fit = fits;
        painter.text(
            pos2(x, label_y),
            Align2::LEFT_TOP,
            readout,
            font.clone(),
            theme.text_primary.into(),
        );
    }
    if let Some([low, high]) = ends.filter(|_| ends_fit) {
        // Pinned to the ends of the axis they name rather than set together
        // in the corner: each is the value of the plot directly below it.
        painter.text(
            pos2(bars.x, label_y),
            Align2::LEFT_TOP,
            low,
            font.clone(),
            theme.text_dim.into(),
        );
        painter.text(
            pos2(bars.right(), label_y),
            Align2::RIGHT_TOP,
            high,
            font,
            theme.text_dim.into(),
        );
    }
}

/// A button on the panel at `rect`: its ground and ink by whether it is
/// `active` and whether the pointer is on it, its tooltip, and what it
/// asked for. The caller paints the mark or the word on it.
fn button(
    pass: &mut Pass,
    ui: &mut egui::Ui,
    rect: Rect,
    control: Control,
    active: bool,
    enabled: bool,
    radius: f32,
) -> (egui::Response, Color32, Color32) {
    let response = ui.allocate_rect(egui::Rect::from(rect), Sense::CLICK);
    let (background, ink) = pass.button_ink(active, &response, enabled);
    ui.painter()
        .rect_filled(egui::Rect::from(rect), radius, background);
    response
        .widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, active, control.label()));
    let response = pass.tooltip(response, Tip::Control(control));
    if enabled && response.clicked() {
        pass.press(control);
    }
    (response, background, ink)
}

/// The rows under the band: the exposure, the window and the curve, for
/// every file.
///
/// What the plot draws, said in words and set: the handles on the band are
/// the window, the curve over the bins is the curve, and the gain that moves
/// them both is the exposure. A reading and the control that changes it,
/// together, so that a number on this panel is never one you have to go
/// somewhere else to act on: the exposure is a slider with its number at
/// the end, in the same quarter stops the keys count in.
///
/// The curves light the one that is in force; the windows do not. A window
/// is set from the pixels and then moved by hand — a handle, a key, the
/// exposure under it — and a button lit for "full range" on a window that
/// has since been shifted would be claiming something that stopped being
/// true. The handles are what say where the window is.
fn rows(pass: &mut Pass, ui: &mut egui::Ui, current: &Current, panel: Rect) {
    let theme = pass.theme;
    let rows = Rows::new(panel);
    let display = &current.display;
    let font = FontId::proportional(ROW_TEXT);
    let grid = pass.grid;

    // The words down the left, set back the way a fact in a bar is set behind
    // the name it is about: the rows are read for their values, and these say
    // which value is which.
    for (label, at) in rows.labels() {
        ui.painter().text(
            pos2(grid.snap(at.x), at.y + at.height / 2.0),
            Align2::LEFT_CENTER,
            label,
            font.clone(),
            theme.text_dim.into(),
        );
    }

    slider(pass, ui, display.exposure_stops(), rows.slider());

    // Its reading at the end of the row, in the units the rest of the
    // interface quotes it in: stops counted in quarters, as the bar and the
    // keys count them.
    let stops = rows.stops();
    ui.painter().text(
        pos2(grid.snap(stops.right()), stops.y + stops.height / 2.0),
        Align2::RIGHT_CENTER,
        stops_label(display.exposure_stops()),
        font.clone(),
        theme.text_primary.into(),
    );

    // The curves are dead under a false color, which clips at the top of
    // its ramp whatever curve is on — the row stays, since the panel's
    // height is the file's, and says why when rested on.
    let false_colored = display.false_colored(current.image.is_gray());
    for (widget, rect) in row_buttons(panel) {
        let Some((label, active)) = row_label(widget, display) else {
            continue;
        };
        let enabled = !(false_colored && matches!(widget, Control::Curve(_)));
        let (_, _, ink) = button(pass, ui, rect, widget, active, enabled, TOGGLE_RADIUS);
        ui.painter().text(
            egui::Rect::from(rect).center(),
            Align2::CENTER_CENTER,
            label,
            font.clone(),
            ink,
        );
    }
}

#[cfg(test)]
mod tests;
