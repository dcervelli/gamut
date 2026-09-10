//! The floating histogram panel.

use egui::{Align2, Color32, FontId, Sense, Stroke, WidgetInfo, WidgetType, pos2, vec2};

use crate::image::display::{AutoWindow, Colormap, Display, ToneMap};
use crate::image::stats::BINS;
use crate::render::Color;

use super::Rect;
use crate::theme::Theme;

use super::chrome::{BUTTON_SIZE, ICON_SIDE, Pass};
use super::icon::{self, Mark};
use super::outline;
use super::status::format_window;
use super::tooltip::Tip;
use super::{
    BECOMES, Control, Current, PADDING, PANEL_INSET, PANEL_RADIUS, PANEL_WIDTH, TEXT_SIZE,
    capitalized,
};

/// The panel: as wide as anything else floating over the content area, and
/// tall enough for a plot with its axis label above it, the ramp of what the
/// display makes of that axis below, and under that the settings the plot is
/// drawing — the exposure, the window and the curve, in [`Rows`].
pub(super) const HISTOGRAM_SIZE: [f32; 2] = [
    PANEL_WIDTH,
    130.0 + RAMP_GAP + RAMP_HEIGHT + RAMP_GAP + SWATCH_HEIGHT + ROWS_HEIGHT,
];

/// The room the strip of buttons down the left takes: a button's width and
/// the gap between it and the plot. Read by [`super::PANEL_WIDTH`], which is
/// this and the plot, so that a bin stays exactly one logical pixel wide.
pub(super) const TOOLBAR_WIDTH: f32 = BUTTON_SIZE + PANEL_INSET;
/// Between one button of that strip and the next.
const TOOLBAR_GAP: f32 = 8.0;

/// The corner a toolbar button is drawn with, and what is left around its
/// icon. The same as the chrome's toggles, which are the same size — and the
/// inset is taken from the side those wear their marks at rather than set
/// beside it, so that a button in this strip and a button in the corner of
/// the window cannot end up carrying marks of two different weights.
const TOGGLE_RADIUS: f32 = 5.0;

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
/// The window's ticks: how far one stands, and how wide. Tall enough to be
/// found along the axis, short enough not to be read as a second plot.
const TICK_HEIGHT: f32 = 5.0;
const TICK_WIDTH: f32 = 1.5;
/// How far a tick rises above the baseline, the rest of it standing in the
/// margin below. Enough to join the axis rather than float under it.
const TICK_RISE: f32 = 1.0;

/// The rule that marks a value, and how wide it is. The accent, like the
/// curve and the window's ticks: everything the panel draws over the bins is
/// the interface talking about them rather than more measurement, and one ink
/// for all of it says so. Held back to translucent, which is what keeps it
/// under the curve it crosses in the reading as well as in the drawing.
const CURSOR_ALPHA: u8 = 190;
const CURSOR_WIDTH: f32 = 1.0;

/// The ramp under the plot: how deep the band of color is, and how far it
/// stands off the plot's ground. Deep enough to read a color off and no
/// deeper — it is a legend along the axis, not a second plot.
const RAMP_HEIGHT: f32 = 8.0;
const RAMP_GAP: f32 = 4.0;

/// A quarter of a stop: what one press of the exposure row is worth, and what
/// the keys that do the same job step by.
///
/// Held here, where the buttons that carry the number are drawn, and read by
/// the key table from here: the two are one step, so a press and a keystroke
/// move the exposure by the same amount and the label on the button cannot
/// come to disagree with what pressing it does.
pub const EV_STEP: f32 = 0.25;

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

/// The windows the panel offers outright, in the order they are set out: what
/// the image itself would open with, and then the three rules named.
///
/// `None` is the image's own, which only the image can answer — see
/// [`AutoWindow::default_for`]. None of them says which window is in force:
/// the reading on the line above them does that, and a press here sets a
/// window rather than switching one on.
pub const WINDOWS: [(&str, Option<AutoWindow>); 4] = [
    ("Auto", None),
    ("0\u{2013}1", Some(AutoWindow::Off)),
    ("Min/Max", Some(AutoWindow::MinMax)),
    ("99.8%", Some(AutoWindow::Percentile)),
];

/// The four nudges at the end of the window's reading, in the order they are
/// set out: along the axis one way, in about its own middle, out again, and
/// along it the other way. Each wears the two chevrons that say what its two
/// ends do.
///
/// They move the window that is there rather than putting it on one of the
/// rules below them, which is why they are on the line the window is read
/// out on and not in that row.
const NUDGES: [(Control, &[Mark]); 4] = [
    (Control::WindowDown, icon::CHEVRONS_LEFT),
    (Control::WindowNarrow, icon::CHEVRONS_RIGHT_LEFT),
    (Control::WindowWiden, icon::CHEVRONS_LEFT_RIGHT),
    (Control::WindowUp, icon::CHEVRONS_RIGHT),
];

/// The rows of controls under the band: how tall a row is, and the two gaps —
/// one between the three rows, and the tighter one between the window's own
/// reading and the buttons that set it, the two being one row in two parts.
const ROW_HEIGHT: f32 = 20.0;
const ROW_GAP: f32 = 8.0;
const SUB_GAP: f32 = 4.0;

/// The column of words down the left of those rows, and the gap between one
/// of them and what it names.
///
/// Wide enough for the longest of the three at [`ROW_TEXT`] and no wider:
/// what it does not take is what the buttons beside it have, and the row of
/// four windows is the narrowest cell on the panel.
/// `every_word_on_the_rows_fits_the_room_it_is_given` holds both ends of that
/// to the face the panel is actually set in.
const ROW_LABEL: f32 = 44.0;
const ROW_LABEL_GAP: f32 = 6.0;

/// How wide one of the exposure row's two steps is, and how much is set aside
/// in front of them for the exposure itself.
///
/// The reading leads and the buttons follow it, close enough to touch: what a
/// step does is change the number in front of it, and a number a button's
/// width away from the button that moves it is a number that has to be looked
/// for. The room the group does not take is left at the end of the line,
/// where it is margin. Both rows are set out this way — see
/// [`WINDOW_READING`] — so the two readings start on one line down the panel
/// and the controls that move them start on another.
const STEP_WIDTH: f32 = 54.0;
const STOPS_WIDTH: f32 = 36.0;

/// And how much is set aside for the window's own reading, in front of the
/// four nudges that move it. Enough for a float raster's bounds written out
/// in full — an elevation model in meters, say — which is the widest reading
/// this line can be asked to carry; the test below holds it to that.
const WINDOW_READING: f32 = 104.0;

/// What is left around one of the nudges' chevrons inside its button. The
/// same air a side-panel toggle leaves around its own mark, less the two
/// logical pixels this button is shorter.
const NUDGE_ICON: f32 = ROW_HEIGHT - 6.0;

/// Between one cell of a row and the next: the false colors under the band,
/// and the buttons of the rows below them.
const CELL_GAP: f32 = 4.0;

/// What the words on those rows are set at. The axis labels' size, this being
/// the same panel's second thoughts about the same measurement — and small
/// enough that the longest button label has room in the narrowest cell.
const ROW_TEXT: f32 = TEXT_SIZE * 0.85;

/// What the block of them takes off the panel's height: the rows, the gaps
/// between them, the gap that parts the block from the band above it, and the
/// plot's own inset — which the band hangs below rather than inside, so it is
/// what is left over between the two and it belongs to whatever comes next.
/// With it counted here the last row ends the panel's own inset above its
/// edge, the way everything else on the panel starts one below its top.
const ROWS_HEIGHT: f32 = PLOT_INSET
    + ROW_GAP
    + ROW_HEIGHT
    + ROW_GAP
    + ROW_HEIGHT
    + SUB_GAP
    + ROW_HEIGHT
    + ROW_GAP
    + ROW_HEIGHT;

/// The least room left between the pointer's readout and the axis ends it is
/// set between, before they give way to it.
const LABEL_GAP: f32 = 8.0;

/// The word set beside the line that marks white, when white is not the top
/// of the plot.
const WHITE_LABEL: &str = "white";

/// How far past white a value has to reach before it is marked as beyond it:
/// a hair, so that the window's own top does not count.
const ABOVE_WHITE: f32 = 1.0 + 1e-3;

/// How the response curve is scaled up the plot: in the file's own encoding,
/// as the bins across are, from 0 at the axis to `ceiling` at the top.
///
/// The ceiling is white — encoded, so that a display doing nothing draws the
/// diagonal — except where the curve runs past it, which it does on a surface
/// with room above white and no curve on: the top of the plot is then
/// wherever the response gets to, and white is a line drawn across it.
/// `white` is where that line goes, as a fraction of the plot's height, and
/// `None` where white is the top and the line would be the plot's own edge.
struct Scale {
    ceiling: f32,
    white: Option<f32>,
}

impl Scale {
    fn new(encoded_white: f32, highest: f32) -> Self {
        let ceiling = highest.max(encoded_white).max(f32::MIN_POSITIVE);
        let white = encoded_white / ceiling;
        Self {
            ceiling,
            white: (white < 1.0 - 1e-3).then_some(white),
        }
    }

    /// An encoded response as a fraction of the plot's height.
    fn up(&self, encoded: f32) -> f32 {
        (encoded / self.ceiling).clamp(0.0, 1.0)
    }
}

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

/// The panel's own rectangle inside `content`: the top right corner, inside
/// the padding everything floating over the image keeps.
///
/// `None` where the content area is too small to take it, as the information
/// panel's [`info::panel`](super::info::panel) is `None` in a window too
/// short for it. The panel is one fixed size — the plot's bins are a logical
/// pixel each and the rows under it are set to what they say, so there is
/// nothing here to give — and a panel drawn larger than the area it floats
/// over would cover the picture it is about and run off the window besides.
///
/// Public because the pointer is tested against the whole panel from outside
/// the frame: what lands on it belongs to it, and must not reach the picture
/// it is floating over.
pub fn panel(content: Rect) -> Option<Rect> {
    if HISTOGRAM_SIZE[0] + 2.0 * PADDING > content.width
        || HISTOGRAM_SIZE[1] + 2.0 * PADDING > content.height
    {
        return None;
    }
    // Rounded, so that the whole-pixel bin spacing starts on a pixel edge.
    Some(Rect::new(
        (content.right() - HISTOGRAM_SIZE[0] - PADDING).round(),
        (content.y + PADDING).round(),
        HISTOGRAM_SIZE[0],
        HISTOGRAM_SIZE[1],
    ))
}

/// The ground the bins stand on inside that panel, with the axis label's line
/// above it. The bins are one logical pixel each, so this is the full width of
/// the plot and the room around it is drawn outside it.
fn bars(panel: Rect) -> Rect {
    let plot = panel.inset(PANEL_INSET, PANEL_INSET);
    let label_height = TEXT_SIZE * 1.4;
    Rect::new(
        plot.x + TOOLBAR_WIDTH,
        plot.y + label_height + PLOT_INSET,
        plot.width - TOOLBAR_WIDTH,
        plot.height - label_height - PLOT_INSET - RAMP_GAP - RAMP_HEIGHT - ROWS_HEIGHT,
    )
}

/// The same, on an image the false colors apply to: the row of them takes
/// its room off the bottom of the plot.
///
/// The panel is one height either way, so the information panel below it does
/// not shift about from file to file. What changes is how much of that height
/// is plot, which nothing reads absolutely — the bars are drawn as fractions
/// of whatever they are given.
fn plot_area(panel: Rect, gray: bool) -> Rect {
    let bars = bars(panel);
    if !gray {
        return bars;
    }
    Rect::new(
        bars.x,
        bars.y,
        bars.width,
        bars.height - RAMP_GAP - SWATCH_HEIGHT,
    )
}

/// The buttons down the left, in the order they are stacked.
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
/// the next.
#[derive(Clone, Copy)]
struct Rows {
    /// Where each row's own word goes, in the order they are stacked.
    labels: [Rect; 3],
    /// The exposure's line: its two steps at the ends and its reading
    /// between them.
    exposure: Rect,
    /// What the window is now, written along the line its label is on.
    reading: Rect,
    /// The four windows on offer, and the three curves below them.
    window: Rect,
    curve: Rect,
}

impl Rows {
    fn new(panel: Rect) -> Self {
        let inside = panel.inset(PANEL_INSET, PANEL_INSET);
        let left = inside.x + ROW_LABEL + ROW_LABEL_GAP;
        let line = |y: f32, height: f32| Rect::new(left, y, inside.right() - left, height);
        let label = |y: f32, height: f32| Rect::new(inside.x, y, ROW_LABEL, height);

        let mut y = ramp(bars(panel)).bottom() + ROW_GAP;
        let (exposure, ev) = (line(y, ROW_HEIGHT), label(y, ROW_HEIGHT));
        y += ROW_HEIGHT + ROW_GAP;
        let (reading, window_label) = (line(y, ROW_HEIGHT), label(y, ROW_HEIGHT));
        y += ROW_HEIGHT + SUB_GAP;
        let window = line(y, ROW_HEIGHT);
        y += ROW_HEIGHT + ROW_GAP;
        let (curve, curve_label) = (line(y, ROW_HEIGHT), label(y, ROW_HEIGHT));

        Self {
            labels: [ev, window_label, curve_label],
            exposure,
            reading,
            window,
            curve,
        }
    }

    /// The exposure itself, at the head of its row: the number the two steps
    /// beside it act on.
    fn stops(&self) -> Rect {
        Rect::new(
            self.exposure.x,
            self.exposure.y,
            STOPS_WIDTH,
            self.exposure.height,
        )
    }

    /// One of those two steps, following it: down first, then up.
    fn step(&self, up: bool) -> Rect {
        let x = self.stops().right() + CELL_GAP + if up { STEP_WIDTH + CELL_GAP } else { 0.0 };
        Rect::new(x, self.exposure.y, STEP_WIDTH, self.exposure.height)
    }

    /// The same on the window's line: the room its bounds are written in, and
    /// after it the four nudges that move them.
    ///
    /// The nudges are square, and as deep as the line, so that a chevron is
    /// drawn on the grid it was described on rather than in a box of some
    /// other shape.
    fn window_reading(&self) -> Rect {
        Rect::new(
            self.reading.x,
            self.reading.y,
            WINDOW_READING,
            self.reading.height,
        )
    }

    fn nudges(&self) -> Rect {
        let side = self.reading.height;
        let count = NUDGES.len() as f32;
        Rect::new(
            self.window_reading().right() + CELL_GAP,
            self.reading.y,
            count * side + (count - 1.0) * CELL_GAP,
            self.reading.height,
        )
    }
}

/// What one of the rows' buttons wears, and whether it is lit. `None` for a
/// nudge, which wears a mark rather than a word.
///
/// Beside the geometry rather than inside the drawing so that the test can
/// ask for the same words the frame is set with: a label the layout was not
/// measured against is a label that can outgrow its button.
fn row_label(widget: Control, display: &Display) -> Option<(String, bool)> {
    Some(match widget {
        // The step's own worth, written as it acts: one source for the number
        // on the button and the number the press is worth.
        Control::ExposureDown => (stops_label(-EV_STEP), false),
        Control::ExposureUp => (stops_label(EV_STEP), false),
        Control::Window(index) => (WINDOWS.get(index)?.0.to_string(), false),
        // Capitalized, where the bar sets the same word in the middle of a
        // line: a button wears a name, and a name starts with a capital.
        Control::Curve(index) => {
            let curve = *ToneMap::ALL.get(index)?;
            (capitalized(curve.label()), display.tone_map == curve)
        }
        _ => return None,
    })
}

/// Every button in that block, with where it goes. The one list the drawing,
/// the pointer and the tooltips all work from.
///
/// Every image has all of them: an exposure, a window and a curve are what
/// the display does to any file whatever, where the two controls the panel
/// leaves out are about what the file itself holds.
fn row_buttons(panel: Rect) -> impl Iterator<Item = (Control, Rect)> {
    let rows = Rows::new(panel);
    let steps = [
        (Control::ExposureDown, rows.step(false)),
        (Control::ExposureUp, rows.step(true)),
    ];
    let windows = (0..WINDOWS.len()).map(move |index| {
        (
            Control::Window(index),
            share(rows.window, WINDOWS.len(), index),
        )
    });
    let curves = (0..ToneMap::ALL.len()).map(move |index| {
        (
            Control::Curve(index),
            share(rows.curve, ToneMap::ALL.len(), index),
        )
    });
    let nudges = NUDGES
        .iter()
        .enumerate()
        .map(move |(index, (widget, _))| (*widget, share(rows.nudges(), NUDGES.len(), index)));
    steps.into_iter().chain(nudges).chain(windows).chain(curves)
}

/// The band of color under the plot, aligned with the bins so that a cell of
/// it sits under the bar it belongs to. Below the plot's ground, and so below
/// the ticks that stand in the ground's lower margin.
fn ramp(bars: Rect) -> Rect {
    Rect::new(
        bars.x,
        bars.bottom() + PLOT_INSET + RAMP_GAP,
        bars.width,
        RAMP_HEIGHT,
    )
}

/// Where a bin's bar is drawn across the plot, from 0 at the left edge to 1
/// at the right. The two end bins are carried out to the edges so that the
/// shape fills the plot's width; the rest stand at their centers.
fn bin_across(index: usize) -> f32 {
    match index {
        0 => 0.0,
        last if last == BINS - 1 => 1.0,
        _ => (index as f32 + 0.5) / BINS as f32,
    }
}

/// How tall a bin's bar stands, from 0 on the axis to 1 at the top of the
/// plot, against the fullest bin drawn beside it.
///
/// Linear is what a photograph wants and what a photo editor draws: the
/// height of a bin is its share of the fullest one, and the shape read off
/// the plot is the distribution itself. It is the wrong plot for measurement
/// data, where one bin often holds most of the image — a masked sea, the
/// surround of a scan — and flattens everything the rest of the range is
/// doing into the axis. A value the file declares as nodata is already
/// thrown out by [`crate::image::Stats::scan`], so the bin that does this is
/// a background the file says nothing about. Logarithmic is that same plot
/// with the tall bin cut down to where the short ones can be seen beside it.
///
/// `ln(1 + n)` rather than `ln(n)`: an empty bin stays flat on the axis,
/// where a floored logarithm would lift it off and draw a count that is not
/// there, and the fullest bin still reaches the top either way. What is lost
/// is that two bars can no longer be compared by their heights — which is the
/// switch's whole point, and why it is a switch and not the plot.
fn bar_fraction(count: u32, peak: u32, log: bool) -> f32 {
    let (count, peak) = (count as f32, peak.max(1) as f32);
    if log {
        count.ln_1p() / peak.ln_1p()
    } else {
        count / peak
    }
}

/// `value` moved onto the device's own pixel grid.
///
/// The interface is laid out in logical pixels, which is right for a panel
/// and the words on it. The marks on this plot are the exception: they are a
/// pixel or two wide, and shapes are drawn with a pixel of feathering at
/// their edges, so an edge landing mid-pixel makes a mark that is mostly
/// edge. On its own that is a soft line; in a row of them it is a ripple at
/// the beat of the scale factor, which on a 1.6 display is every fifth pixel.
/// Snapped, the feather resolves to fully in or fully out at each pixel
/// center and a mark comes out as the shape it is.
fn device(value: f32, scale: f32) -> f32 {
    (value * scale).round() / scale
}

/// One mark moved onto that grid, kept at least a whole pixel so that
/// something thinner than one is still drawn rather than rounded away.
///
/// Not what the ramp's cells use: they tile, so what matters there is that
/// each shares an edge exactly with its neighbor, and a floor under their
/// width would make them overlap and run past the end of the band.
fn on_device(rect: Rect, scale: f32) -> Rect {
    let (x, y) = (device(rect.x, scale), device(rect.y, scale));
    Rect::new(
        x,
        y,
        (device(rect.right(), scale) - x).max(1.0 / scale),
        (device(rect.bottom(), scale) - y).max(1.0 / scale),
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
/// The plot comes first because the panel floats over the picture, so a
/// pointer on the panel is on the axis and not on the pixel behind it.
///
/// One bin either way, and so one rule and one reading either way: the panel
/// draws bars, and a marker on it can only honestly point at one of them. The
/// pixel's own exact numbers are the bottom bar's to report — that readout is
/// about a pixel, and this one is about a bar.
///
/// Public because the pointer is tested against the plot from outside the
/// frame as well: a mark that follows the pointer has to be able to say when
/// the frame it was drawn in has gone out of date.
pub fn marked(
    current: &Current,
    content: Rect,
    cursor: Option<[f32; 2]>,
    pointer: Option<[u32; 2]>,
) -> Option<usize> {
    let plot = plot_area(panel(content)?, current.image.is_gray());
    if let Some(bin) = hovered_bin(plot, cursor) {
        return Some(bin);
    }
    let at = pointer?;
    let sample = current.image.sample(at[0], at[1])?;
    current.stats.plot.bin_of(&current.image, &sample)
}

/// An egui rectangle for one of ours.
fn area(rect: Rect) -> egui::Rect {
    egui::Rect::from_min_size(pos2(rect.x, rect.y), vec2(rect.width, rect.height))
}

/// How wide `text` comes out at `size`.
fn width_of(ui: &egui::Ui, text: &str, size: f32) -> f32 {
    ui.ctx().fonts_mut(|fonts| {
        fonts
            .layout_no_wrap(text.to_string(), FontId::proportional(size), Color32::WHITE)
            .size()
            .x
    })
}

/// The planes a column of the plot is drawn from: which of them stand this
/// high, as a set. What the color of a stretch of the column is a function of.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Cover {
    luma: bool,
    planes: [bool; 3],
}

/// What a stretch of a column comes out as, with `cover` standing over it:
/// the plot's ground, the luminance plane laid over that, and the color
/// planes screened over the lot.
///
/// The display list drew the planes as translucent shapes screened over one
/// another on the GPU; egui has one blend, so the screening is done here, per
/// stretch of column, which comes to the same picture — the planes are the
/// primaries on a near-black ground, so two of them give the secondary
/// between and all three give white, which is the reading a channel
/// histogram is looked at for.
fn screened(theme: &Theme, luma_ink: Color, cover: Cover) -> Color32 {
    let over = |ground: [f32; 3], ink: Color| -> [f32; 3] {
        let alpha = ink.a as f32 / 255.0;
        let ink = [ink.r, ink.g, ink.b].map(|channel| channel as f32 / 255.0);
        [0, 1, 2].map(|c| ground[c] * (1.0 - alpha) + ink[c] * alpha)
    };
    let screen = |ground: [f32; 3], ink: Color| -> [f32; 3] {
        let ink = [ink.r, ink.g, ink.b].map(|channel| channel as f32 / 255.0);
        [0, 1, 2].map(|c| 1.0 - (1.0 - ground[c]) * (1.0 - ink[c]))
    };
    let ground = theme.plot_background;
    let mut color = [ground.r, ground.g, ground.b].map(|channel| channel as f32 / 255.0);
    if cover.luma {
        color = over(color, luma_ink);
    }
    for (plane, ink) in cover.planes.into_iter().zip(theme.histogram_planes) {
        if plane {
            color = screen(color, ink);
        }
    }
    let [r, g, b] = color.map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8);
    Color32::from_rgb(r, g, b)
}

/// Draws the histogram in the top-right of `content`, the area the panels
/// leave free — above the information panel, the order the two toggles that
/// open them are stacked in.
///
/// Color images get four planes — red, green, blue and luminance — over the
/// range their color channels span; gray images keep the single luminance
/// plane over theirs.
///
/// The panel is opaque to the pointer: what lands on it belongs to it rather
/// than to the picture it is floating over.
pub(super) fn show(pass: &mut Pass, ui: &mut egui::Ui, current: &Current, content: Rect) {
    let Some(panel) = panel(content) else {
        return;
    };
    let theme = pass.theme;
    egui::Area::new(egui::Id::new("histogram"))
        .order(egui::Order::Middle)
        .fixed_pos(area(panel).min)
        .interactable(true)
        .show(ui.ctx(), |ui| {
            let (_, body) = ui.allocate_exact_size(area(panel).size(), Sense::CLICK | Sense::DRAG);
            body.widget_info(|| WidgetInfo::labeled(WidgetType::Other, true, "Histogram panel"));
            ui.painter()
                .rect_filled(area(panel), PANEL_RADIUS, theme.panel_background);
            plot(pass, ui, current, panel, content);
            controls(pass, ui, current, panel);
        });
}

/// The plot itself: the bins, the pointer's rule, the window's ticks, the
/// band along the foot and the response curve over the lot.
fn plot(pass: &Pass, ui: &egui::Ui, current: &Current, panel: Rect, content: Rect) {
    let theme = pass.theme;
    let panels = pass.panels;
    let input = pass.input;
    let painter = ui.painter();
    let scale = input.scale;

    // What applies to this image: the false colors are for a single channel
    // and the color planes are for three, and the panel leaves out whichever
    // the display would ignore rather than drawing it dead.
    let gray = current.image.is_gray();
    let bars = plot_area(panel, gray);
    painter.rect_filled(
        area(bars.inset(-PLOT_INSET, -PLOT_INSET)),
        PLOT_RADIUS,
        theme.plot_background,
    );

    // Luminance always goes down first, underneath the color planes: it
    // runs as tall as the tallest of them about as often as not, and painting
    // it on top swamps the color the panel exists to show.
    let plotted = &current.stats.plot;
    let luma: Option<&[u32; BINS]> = panels.show_luma.then_some(&plotted.luma);
    let color: &[[u32; BINS]] = match plotted.color.as_ref() {
        Some(planes) if panels.show_planes => planes,
        _ => &[],
    };
    let (axis_min, axis_max) = (plotted.min, plotted.max);
    let span = axis_max - axis_min;

    // The axis is in the file's own encoding; the labels are not, since the
    // numbers everything else quotes are the decoded ones.
    let transfer = current.image.color.transfer;
    let label_size = TEXT_SIZE * 0.85;
    let label_y = panel.y + PANEL_INSET;
    let font = FontId::proportional(label_size);

    // The pointer's readout, in the middle of the line the two ends of the
    // axis are pinned to. Two numbers, because the panel draws two things and
    // a column of it belongs to both: the value the bins under the rule were
    // counted at, and what the display makes of that value — the height of
    // the response curve where the rule crosses it, which is the one number a
    // curve on its own cannot be read off by eye.
    //
    // Both of them decoded, as the ends of the axis are. Neither will measure
    // against the plot underneath with a ruler, because both of the plot's
    // axes are spaced in the file's own encoding — the bins across, so that a
    // quantized file does not comb, and the response up, so that a display
    // doing nothing is the diagonal. The positions are the file's units and
    // the numbers are the ones every other readout quotes; a curve that is
    // straight and a value that is comparable cannot both be had, and the
    // shape is what the plot is for.
    let headroom = input.headroom;
    let marked = marked(current, content, input.cursor, input.pointer);
    let across = marked.map(bin_across);
    let mut ends_fit = true;
    if let Some(across) = across {
        let value = transfer.to_linear(axis_min + across * span);
        let mapped = current.display.response(value, headroom).max(0.0);
        let readout = format!("{value:.4}  {BECOMES}  {mapped:.4}");
        let ends_width = [axis_min, axis_max]
            .map(|end| width_of(ui, &format!("{:.4}", transfer.to_linear(end)), label_size));
        let width = width_of(ui, &readout, label_size);
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
    if ends_fit {
        // Pinned to the ends of the axis they name rather than set together
        // in the corner: each is the value of the plot directly below it.
        painter.text(
            pos2(bars.x, label_y),
            Align2::LEFT_TOP,
            format!("{:.4}", transfer.to_linear(axis_min)),
            font.clone(),
            theme.text_dim.into(),
        );
        painter.text(
            pos2(bars.right(), label_y),
            Align2::RIGHT_TOP,
            format!("{:.4}", transfer.to_linear(axis_max)),
            font.clone(),
            theme.text_dim.into(),
        );
    }

    // One peak across every plane on screen, so their heights stay
    // comparable — and only across those, so that a plane left on its own
    // fills the plot rather than keeping the room a hidden one wanted.
    let peak = color
        .iter()
        .chain(luma)
        .flatten()
        .copied()
        .max()
        .unwrap_or(1);
    let height_of = |count: u32| bar_fraction(count, peak, panels.log_counts) * bars.height;

    // Dimmed only when it is a backdrop; with the color planes off — or on
    // a gray image, which has none — it is the plot.
    let luma_ink = if color.is_empty() {
        theme.histogram_luma
    } else {
        theme.histogram_luma.with_alpha(HISTOGRAM_LUMA_UNDER)
    };

    // One column to a bin — the bins are a logical pixel each, which is what
    // the panel's width was fixed for — cut into stretches by the heights of
    // the planes standing in it, each stretch filled with what the planes
    // over it come to. On the device's grid, like every other mark here.
    let snap = |value: f32| device(value, scale);
    let edge = |index: usize| snap(bars.x + bars.width * index as f32 / BINS as f32);
    for bin in 0..BINS {
        let (left, right) = (edge(bin), edge(bin + 1));
        let mut heights: Vec<(f32, usize)> = Vec::with_capacity(4);
        if let Some(counts) = luma {
            heights.push((height_of(counts[bin]), 3));
        }
        for (plane, counts) in color.iter().enumerate() {
            heights.push((height_of(counts[bin]), plane));
        }
        heights.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut cover = Cover {
            luma: luma.is_some(),
            planes: [!color.is_empty(); 3],
        };
        let mut from = 0.0;
        for (height, plane) in heights {
            let (bottom, top) = (snap(bars.bottom() - from), snap(bars.bottom() - height));
            if top < bottom {
                painter.rect_filled(
                    egui::Rect::from_min_max(pos2(left, top), pos2(right, bottom)),
                    0.0,
                    screened(theme, luma_ink, cover),
                );
            }
            from = height;
            match plane {
                3 => cover.luma = false,
                plane => cover.planes[plane] = false,
            }
        }
    }

    // The pointer's rule: over the bins it is picking one of, and under the
    // response curve, since where that curve runs at this value is half of
    // what the readout above says and the line must not cover it.
    //
    // Full height, where the window's own marks are ticks against the axis.
    // A rule standing through the plot is what a pointer wants and what a
    // permanent annotation does not: this one is only there while it is being
    // aimed, and it has to be followed up from the axis to the curve.
    if let Some(across) = across {
        painter.rect_filled(
            area(on_device(
                Rect::new(
                    bars.x + across * bars.width - CURSOR_WIDTH / 2.0,
                    bars.y,
                    CURSOR_WIDTH,
                    bars.height,
                ),
                scale,
            )),
            0.0,
            theme.accent.with_alpha(CURSOR_ALPHA),
        );
    }

    // What the display is doing to the values underneath, drawn over them.
    //
    // The curve is the whole of it, and the only part that can show a tone
    // map at all: a shoulder is a shape, not a threshold, and there is no
    // line that means "reinhard". The ticks under it place the two ends of
    // the window — the values that come out black and white, exposure
    // included, rather than the window's own bounds, exposure living in the
    // gain rather than in them.
    //
    // Ticks rather than the full-height rules they used to be. The curve
    // draws both of those points already, leaving the floor at one and, under
    // a clip, turning its corner at the other; what the ticks add is where
    // exactly, since a curve meeting a floor tangentially cannot be read
    // along the axis by eye, and where the window's top is under a tone map,
    // which nothing on the curve marks because the curve never reaches it.
    if span > 0.0 {
        let (black, white) = current.display.displayed_bounds();
        for value in [black, white] {
            let position = (transfer.to_encoded(value) - axis_min) / span;
            // Dropped rather than pinned to the edge when the window ends
            // beyond what is plotted, which a few stops of exposure is enough
            // to do: a tick held at the edge reads as a boundary that is
            // there, and the curve running on past it says otherwise. A NaN
            // fails this test as well, so a degenerate window draws nothing.
            if !(0.0..=1.0).contains(&position) {
                continue;
            }
            painter.rect_filled(
                area(on_device(
                    Rect::new(
                        bars.x + position * bars.width - TICK_WIDTH / 2.0,
                        bars.bottom() - TICK_RISE,
                        TICK_WIDTH,
                        TICK_HEIGHT,
                    ),
                    scale,
                )),
                0.0,
                theme.accent,
            );
        }

        // And what the display turns each of those values into, in a band
        // along the foot of the plot: the bin above a cell, and the color it
        // comes out as under it.
        //
        // The curve says how much and this says what of, which are different
        // questions on a false-colored image — a curve cannot draw viridis —
        // and the same question answered twice on a gray one, where the band
        // is the tone curve as a wedge and the curve is it as a shape. It is
        // where clipping stops being an inference: everything left of the
        // window comes out black and everything right of it comes out at the
        // top of the ramp, so the two flat runs at the ends are the range the
        // display is throwing away, drawn at the width they occupy.
        let band = ramp(bars);
        let (top, bottom) = (snap(band.y), snap(band.bottom()));
        // A cell above white — which only a surface with room above white
        // has, and only with no curve on — is drawn white, since the panel
        // cannot glow, with the accent along its top edge to say that the
        // screen does: the same ink as the tick that marks white on the
        // axis, and the run of it is how much of the axis is out past that.
        let channels = current.image.channels();
        let hair = 1.0 / scale;
        for index in 0..BINS {
            let (left, right) = (edge(index), edge(index + 1));
            let across = (index as f32 + 0.5) / BINS as f32;
            let value = transfer.to_linear(axis_min + across * span);
            painter.rect_filled(
                egui::Rect::from_min_max(pos2(left, top), pos2(right, bottom)),
                0.0,
                Color::from_linear(current.display.shade(value, channels, headroom)),
            );
            if current.display.response(value, headroom) > ABOVE_WHITE {
                painter.rect_filled(
                    egui::Rect::from_min_max(pos2(left, top), pos2(right, top + hair)),
                    0.0,
                    theme.accent,
                );
            }
        }
        // Outside the color rather than over it, so that the band keeps its
        // full depth. A window left of everything makes the whole ramp black,
        // and a black band on a dark panel is a gap in it without this. One
        // physical pixel, snapped like the band it rings.
        let (left, right) = (edge(0), edge(BINS));
        outline(
            painter,
            icon::Grid::new(scale),
            Rect::new(
                left - hair,
                top - hair,
                right - left + 2.0 * hair,
                bottom - top + 2.0 * hair,
            ),
            hair,
            theme.border.into(),
        );

        // Sampled per column rather than per bin: the response is a
        // continuous function of the value, and stepping it where the
        // transform does not step would draw a stair that is not there.
        //
        // The same one check for every column, the arithmetic being the same
        // for all of them: a window left non-finite would otherwise put NaN
        // vertices in the buffer, which no clamp downstream can undo.
        let (offset, gain) = current.display.transform();
        if offset.is_finite() && gain.is_finite() {
            let columns = bars.width.max(1.0) as usize;
            // Decoded to run the transform on, then encoded again to be
            // drawn: both axes are in the file's own units, so a display
            // doing nothing is the diagonal.
            let responses: Vec<f32> = (0..=columns)
                .map(|column| {
                    let across = column as f32 / columns as f32;
                    let value = transfer.to_linear(axis_min + across * span);
                    let response = current.display.response(value, headroom).max(0.0);
                    transfer.to_encoded(response).max(0.0)
                })
                .collect();
            // The plot's height is white, unless the response runs past it —
            // a surface with room above white, and no curve on — in which
            // case the top is wherever the response gets to and white is a
            // line across the plot, so that the room above it can be seen as
            // the room it is rather than as a clip that is not happening.
            let highest = responses.iter().copied().fold(0.0, f32::max);
            let plot_scale = Scale::new(transfer.to_encoded(1.0), highest);
            if let Some(white) = plot_scale.white {
                let y = device(bars.bottom() - white * bars.height, scale);
                painter.rect_filled(
                    egui::Rect::from_min_size(pos2(bars.x, y), vec2(bars.width, 1.0 / scale)),
                    0.0,
                    theme.text_dim,
                );
                let size = TEXT_SIZE * 0.75;
                painter.text(
                    pos2(bars.right() - 2.0, y - size * 0.3),
                    Align2::RIGHT_BOTTOM,
                    WHITE_LABEL,
                    FontId::proportional(size),
                    theme.text_dim.into(),
                );
            }
            let curve: Vec<egui::Pos2> = responses
                .iter()
                .enumerate()
                .map(|(column, &response)| {
                    let across = column as f32 / columns as f32;
                    pos2(
                        bars.x + across * bars.width,
                        bars.bottom() - plot_scale.up(response) * bars.height,
                    )
                })
                .collect();
            painter.add(egui::Shape::line(
                curve,
                Stroke::new(CURVE_WIDTH, theme.accent),
            ));
        }
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
    radius: f32,
) -> (egui::Response, Color32, Color32) {
    let response = ui.allocate_rect(area(rect), Sense::CLICK);
    let (background, ink) = pass.button_ink(active, &response, true);
    ui.painter().rect_filled(area(rect), radius, background);
    response
        .widget_info(|| WidgetInfo::selected(WidgetType::Button, true, active, control.label()));
    let response = pass.tooltip(response, Tip::Control(control), true);
    if response.clicked() {
        pass.press(control);
    }
    (response, background, ink)
}

/// The strip of buttons down the left of the panel, the rows under the band,
/// and the row of false colors under its ramp.
///
/// Drawn here rather than with the chrome's toggles because these belong to
/// the panel: they say what the plot beside them is showing and what the band
/// beneath them is painted with, and two of them are pictures of the very
/// thing they switch.
fn controls(pass: &mut Pass, ui: &mut egui::Ui, current: &Current, panel: Rect) {
    let theme = pass.theme;
    let panels = pass.panels;
    let scale = pass.input.scale;
    let gray = current.image.is_gray();
    let bars = plot_area(panel, gray);

    for (slot, widget) in toolbar(gray).iter().enumerate() {
        let rect = toolbar_button(panel, gray, slot);
        let active = match widget {
            Control::Luma => panels.show_luma,
            Control::Planes => panels.show_planes,
            Control::Log => panels.log_counts,
            // The reset is never lit, where the two above it are: it does
            // something rather than being something, and a momentary button
            // holding a state is a button that has to explain itself.
            _ => false,
        };
        let (_, background, ink) = button(pass, ui, rect, *widget, active, TOGGLE_RADIUS);
        let grid = icon::Grid::new(ui.pixels_per_point());
        let square = icon::square(grid, area(rect), ICON_SIDE);
        let painter = ui.painter();
        match widget {
            // The two plane toggles are drawn here rather than taken from
            // `ui::icon` because they are pictures of the planes themselves,
            // each in the color that plane is plotted in — which is not
            // something a mark drawn in one ink can be.
            //
            // Luminance is one plane, so it is one disc, in the neutral the
            // plot draws that plane in.
            Control::Luma => {
                let place = icon::Placer::new(
                    grid,
                    Rect::new(square.min.x, square.min.y, square.width(), square.height()),
                );
                let at = place.free([GRID_MIDDLE, GRID_MIDDLE]);
                painter.circle_filled(
                    pos2(at[0], at[1]),
                    place.units(LUMA_DISC),
                    theme.histogram_luma,
                );
            }
            // And the color planes are three, so they are three smaller
            // discs, in their own colors: nothing else in the window is red,
            // green and blue together.
            Control::Planes => {
                let place = icon::Placer::new(
                    grid,
                    Rect::new(square.min.x, square.min.y, square.width(), square.height()),
                );
                for (turn, plane) in theme.histogram_planes.into_iter().enumerate() {
                    // Struck about the middle at a third of a turn each,
                    // starting at the top, so the three read as one mark
                    // rather than as a row.
                    let angle = (-90.0 + 120.0 * turn as f32).to_radians();
                    let at = place.free([
                        GRID_MIDDLE + PLANE_ORBIT * angle.cos(),
                        GRID_MIDDLE + PLANE_ORBIT * angle.sin(),
                    ]);
                    painter.circle_filled(pos2(at[0], at[1]), place.units(PLANE_DISC), plane);
                }
            }
            // The count axis as a curve, which is what the switch puts it on.
            Control::Log => icon::paint(painter, icon::SPLINE, square, ink, background),
            // Back to the start.
            _ => icon::paint(painter, icon::ROTATE_CCW, square, ink, background),
        }
    }

    rows(pass, ui, current, panel);

    // The false colors, each showing itself, and only where the display
    // would act on the choice. The whole ramp rather than one color off it:
    // a map is a sequence, and a single swatch of viridis is a green
    // rectangle that could be anything.
    if !gray {
        return;
    }
    for (index, map) in Colormap::ALL.into_iter().enumerate() {
        let rect = swatch_button(bars, index);
        let chosen = current.display.colormap == map;
        button(pass, ui, rect, Control::Ramp(index), chosen, SWATCH_RADIUS);

        // The gradient on the device's pixels, as the band above it is: a
        // swatch is the same row of one-pixel cells, over less room.
        let face = rect.inset(SWATCH_INSET, SWATCH_INSET);
        let snap = |value: f32| device(value, scale);
        let (top, bottom) = (snap(face.y), snap(face.bottom()));
        let steps = (face.width * scale).max(1.0) as usize;
        for step in 0..steps {
            let edge = |step: usize| snap(face.x + face.width * step as f32 / steps as f32);
            let (left, right) = (edge(step), edge(step + 1));
            let t = (step as f32 + 0.5) / steps as f32;
            ui.painter().rect_filled(
                egui::Rect::from_min_max(pos2(left, top), pos2(right, bottom)),
                0.0,
                Color::from_linear(map.color(t)),
            );
        }
    }
}

/// The three rows under the band: the exposure, the window, and the curve.
///
/// What the plot draws, said in words and set: the ticks along the axis are
/// the window, the curve over the bins is the curve, and the gain that moves
/// them both is the exposure. A reading and the buttons that change it,
/// together, so that a number on this panel is never one you have to go
/// somewhere else to act on.
///
/// The two rows that set a state light the one that is in force; the windows
/// do not. A window is set from the pixels and then moved by hand — a drag on
/// the picture, a key, the exposure under it — and a button lit for "min/max"
/// on a window that has since been shifted would be claiming something that
/// stopped being true. The line above them is what says where the window is,
/// and it says it in numbers.
fn rows(pass: &mut Pass, ui: &mut egui::Ui, current: &Current, panel: Rect) {
    let theme = pass.theme;
    let rows = Rows::new(panel);
    let display = &current.display;
    let font = FontId::proportional(ROW_TEXT);
    let grid = icon::Grid::new(ui.pixels_per_point());

    // The words down the left, set back the way a fact in a bar is set behind
    // the name it is about: the rows are read for their values, and these say
    // which value is which.
    for (label, at) in ["EV", "Window", "Curve"].into_iter().zip(rows.labels) {
        ui.painter().text(
            pos2(grid.snap(at.x), at.y + at.height / 2.0),
            Align2::LEFT_CENTER,
            label,
            font.clone(),
            theme.text_dim.into(),
        );
    }

    // The exposure between its two steps, and the window along its own line.
    // Both in the units the rest of the interface quotes them in: stops
    // counted in quarters, as the bar and the keys count them, and the window
    // in the file's own counts where the file is counting things.
    let stops = rows.stops();
    ui.painter().text(
        pos2(grid.snap(stops.x), stops.y + stops.height / 2.0),
        Align2::LEFT_CENTER,
        stops_label(display.exposure_stops),
        font.clone(),
        theme.text_primary.into(),
    );
    let window = rows.window_reading();
    ui.painter().with_clip_rect(area(window)).text(
        pos2(grid.snap(window.x), window.y + window.height / 2.0),
        Align2::LEFT_CENTER,
        format_window(current),
        font.clone(),
        theme.text_primary.into(),
    );

    for (widget, rect) in row_buttons(panel) {
        let worn = row_label(widget, display);
        let active = worn.as_ref().is_some_and(|(_, active)| *active);
        let (_, background, ink) = button(pass, ui, rect, widget, active, TOGGLE_RADIUS);
        match &worn {
            Some((label, _)) => {
                ui.painter().text(
                    area(rect).center(),
                    Align2::CENTER_CENTER,
                    label,
                    font.clone(),
                    ink,
                );
            }
            // The nudges wear marks where the rest of the block wears words:
            // what they do to the two ends of the window is a shape, and four
            // of them spelled out would be a paragraph on a line that is
            // already a number.
            None => {
                if let Some((_, marks)) = NUDGES.iter().find(|(nudge, _)| *nudge == widget) {
                    icon::paint(
                        ui.painter(),
                        marks,
                        icon::square(grid, area(rect), NUDGE_ICON),
                        ink,
                        background,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exposure is stepped in quarter stops, so it is written in
    /// quarters: a tenth of a stop cannot say what one press is worth, and
    /// would read one step as `0.2` and the next but one as `0.8`. Only a
    /// value from `--exposure` can be anything else, and it is written back
    /// as it was asked for.
    #[test]
    fn an_exposure_is_written_in_the_quarters_it_is_stepped_in() {
        assert_eq!(stops_label(0.0), "0");
        assert_eq!(stops_label(EV_STEP), "+\u{00bc}");
        assert_eq!(stops_label(-EV_STEP), "-\u{00bc}");
        assert_eq!(stops_label(0.5), "+\u{00bd}");
        assert_eq!(stops_label(-0.75), "-\u{00be}");
        assert_eq!(stops_label(1.0), "+1");
        assert_eq!(stops_label(-1.25), "-1\u{00bc}");
        assert_eq!(
            stops_label(-16.0),
            "-16",
            "the far end of what a key reaches"
        );
        assert_eq!(stops_label(0.1), "+0.10");
    }

    /// The pointer reads a bin of the plot and nothing outside it — not the
    /// panel around it, and not the line the axis labels are set on.
    #[test]
    fn only_the_plot_itself_answers_the_pointer() {
        let bars = bars(panel(Rect::new(0.0, 0.0, 800.0, 600.0)).expect("room"));

        assert_eq!(hovered_bin(bars, None), None, "no pointer, no mark");
        assert_eq!(hovered_bin(bars, Some([bars.x - 1.0, bars.y + 1.0])), None);
        assert_eq!(
            hovered_bin(bars, Some([bars.x + 1.0, bars.y - 1.0])),
            None,
            "the labels' line is above the plot, not part of it"
        );
        assert_eq!(
            hovered_bin(bars, Some([bars.right(), bars.y + 1.0])),
            None,
            "half-open at the far edge, as every other hit test here is"
        );
        assert_eq!(hovered_bin(bars, Some([bars.x + 1.0, bars.bottom()])), None);
    }

    /// And it reads the bin the bar under it was drawn from, so that the rule
    /// and the bar it stands on cannot part company.
    #[test]
    fn the_pointer_marks_the_bar_it_is_over() {
        let bars = bars(panel(Rect::new(0.0, 0.0, 800.0, 600.0)).expect("room"));
        let at = |x: f32| hovered_bin(bars, Some([bars.x + x, bars.y + 1.0]));

        assert_eq!(at(0.0), Some(0), "the first pixel of the plot is bin zero");
        assert_eq!(
            at(bars.width - 0.5),
            Some(BINS - 1),
            "and the last of it is the last bin"
        );
        assert_eq!(at(3.1), at(3.9), "one pixel of pointer, one bin");
        assert_ne!(at(3.1), at(4.1), "the next pixel is the next bin");

        // Every bin the pointer can name has a bar drawn inside the plot for
        // it to stand on, the two ends included.
        for bin in [0, 1, BINS / 2, BINS - 2, BINS - 1] {
            let across = bin_across(bin);
            assert!((0.0..=1.0).contains(&across), "bin {bin} at {across}");
        }
        assert_eq!(bin_across(0), 0.0);
        assert_eq!(bin_across(BINS - 1), 1.0);
    }

    /// The panel is one fixed size, so a content area smaller than it in
    /// either direction gets no panel at all rather than one hanging off the
    /// window over the picture it is about.
    #[test]
    fn a_content_area_too_small_gets_no_panel() {
        let room = [
            HISTOGRAM_SIZE[0] + 2.0 * PADDING,
            HISTOGRAM_SIZE[1] + 2.0 * PADDING,
        ];
        assert!(panel(Rect::new(0.0, 0.0, room[0], room[1])).is_some());
        assert!(panel(Rect::new(0.0, 0.0, room[0] - 1.0, room[1])).is_none());
        assert!(panel(Rect::new(0.0, 0.0, room[0], room[1] - 1.0)).is_none());
    }

    /// Where it does fit it sits in the top right of the content area, its
    /// own padding in from both edges.
    #[test]
    fn the_panel_sits_in_the_corner_with_its_padding_around_it() {
        let content = Rect::new(10.0, 20.0, 800.0, 600.0);
        let panel = panel(content).expect("room");
        assert_eq!(panel.right(), content.right() - PADDING);
        assert_eq!(panel.y, content.y + PADDING);
    }

    /// The panel grew a strip of buttons and a row of ramps around the plot,
    /// and the plot itself did not move across: a bin is one logical pixel,
    /// which is what keeps the bars from landing astride a pixel boundary.
    #[test]
    fn the_plot_keeps_one_pixel_to_the_bin_whatever_grows_around_it() {
        let panel = panel(Rect::new(0.0, 0.0, 800.0, 600.0)).expect("room");
        for gray in [true, false] {
            let bars = plot_area(panel, gray);
            assert_eq!(bars.width, BINS as f32, "gray {gray}");

            // Everything the panel holds is inside it, and clear of the plot.
            let last = toolbar(gray).len() - 1;
            let button = toolbar_button(panel, gray, last);
            assert!(button.right() <= bars.x, "the strip clears the plot");
            assert!(toolbar_button(panel, gray, 0).y >= panel.y);
            assert!(button.bottom() <= panel.bottom(), "{button:?}");
            // And it ends above the band of color, which is what the room
            // under the plot is for: a button beside the ramp would read as
            // belonging to it rather than to the plot it acts on.
            assert!(
                button.bottom() <= ramp(bars).y,
                "gray {gray}: {button:?} against the band at {:?}",
                ramp(bars)
            );
            assert!(ramp(bars).y >= bars.bottom(), "the band is under the plot");
            assert!(ramp(bars).bottom() <= panel.bottom());
        }
    }

    /// The false colors take their room off the plot rather than off the
    /// panel, so that the information panel below does not shift about from
    /// one file to the next.
    #[test]
    fn the_panel_is_one_height_with_the_false_colors_and_without() {
        let panel = panel(Rect::new(0.0, 0.0, 800.0, 600.0)).expect("room");
        let (with, without) = (plot_area(panel, true), plot_area(panel, false));
        assert!(with.height < without.height, "{with:?} {without:?}");
        assert_eq!(with.y, without.y, "both start under the same label");

        let last = swatch_button(with, Colormap::ALL.len() - 1);
        assert!(
            last.y >= ramp(with).bottom(),
            "the ramps sit under the band"
        );
        assert!(last.bottom() <= panel.bottom(), "{last:?} in {panel:?}");
        assert!(last.right() <= with.right() + 0.01, "{last:?}");
        assert!(
            ramp(without).bottom() <= panel.bottom(),
            "and without them the band still clears the panel's edge"
        );
    }

    /// A control the display would ignore is not on the panel at all, and the
    /// ones that remain close the gap up rather than leaving a hole where it
    /// would have been.
    #[test]
    fn a_control_that_could_do_nothing_is_not_there() {
        let panel = panel(Rect::new(0.0, 0.0, 800.0, 600.0)).expect("room");

        assert_eq!(toolbar(true), [Control::Luma, Control::Log, Control::Reset]);
        assert_eq!(
            toolbar(false),
            [Control::Luma, Control::Planes, Control::Log, Control::Reset],
            "three channels have planes to toggle"
        );
        // The reset moves up into the room the planes toggle is not taking.
        assert_eq!(
            toolbar_button(panel, true, 1).y,
            toolbar_button(panel, false, 1).y,
            "and the slot it leaves is filled, not left empty"
        );

        // The row of false colors takes its room off the plot on an image
        // that has them, under the band, and the plot is taller without.
        let swatch = swatch_button(plot_area(panel, true), 2);
        assert!(swatch.y >= ramp(plot_area(panel, true)).bottom());
        assert!(plot_area(panel, false).height > plot_area(panel, true).height);
    }

    /// The three rows sit under the band, inside the panel, and land in the
    /// same place whether or not the image has a row of false colors — the
    /// band and the swatches under it end on the same line, so the controls
    /// below them do not shift about from one file to the next.
    #[test]
    fn the_rows_sit_under_the_band_whatever_it_ends_in() {
        let panel = panel(Rect::new(0.0, 0.0, 800.0, 600.0)).expect("room");
        let rows = Rows::new(panel);
        let inside = panel.inset(PANEL_INSET, PANEL_INSET);

        let lowest = swatch_button(plot_area(panel, true), 0).bottom();
        assert_eq!(
            lowest,
            ramp(bars(panel)).bottom(),
            "the false colors end where the band alone would have"
        );
        for (widget, rect) in row_buttons(panel) {
            assert!(rect.y >= lowest, "{widget:?} clears the band: {rect:?}");
            assert!(rect.bottom() <= inside.bottom(), "{widget:?} {rect:?}");
            assert!(rect.x >= inside.x + ROW_LABEL, "{widget:?} clears its word");
            assert!(rect.right() <= inside.right() + 0.01, "{widget:?} {rect:?}");
        }
        // The last row is the last thing on the panel, and what it leaves
        // under it is the panel's own inset and nothing more.
        assert_eq!(rows.curve.bottom(), inside.bottom());
        assert_eq!(rows.labels[2].bottom(), inside.bottom());
    }

    /// The exposure's two steps are worth what they say they are worth, and
    /// the windows and the curves cover everything there is to choose.
    #[test]
    fn the_rows_offer_every_choice_there_is() {
        let panel = panel(Rect::new(0.0, 0.0, 800.0, 600.0)).expect("room");
        let widgets: Vec<Control> = row_buttons(panel).map(|(widget, _)| widget).collect();
        assert_eq!(widgets[0], Control::ExposureDown);
        assert_eq!(widgets[1], Control::ExposureUp);
        assert_eq!(
            widgets.len(),
            2 + NUDGES.len() + WINDOWS.len() + ToneMap::ALL.len()
        );

        // The four things a hand can do to a window, and no two of them the
        // same thing: along the axis either way, and about its own middle
        // either way.
        assert_eq!(
            NUDGES.map(|(widget, _)| widget),
            [
                Control::WindowDown,
                Control::WindowNarrow,
                Control::WindowWiden,
                Control::WindowUp
            ]
        );

        // The three the row names outright, and the fourth that only the
        // image can answer.
        let named: Vec<Option<AutoWindow>> = WINDOWS.iter().map(|(_, window)| *window).collect();
        assert_eq!(
            named,
            [
                None,
                Some(AutoWindow::Off),
                Some(AutoWindow::MinMax),
                Some(AutoWindow::Percentile)
            ]
        );
        assert_eq!(ToneMap::ALL.len(), 3, "one button to a curve");
    }

    /// Either way of scaling the plot draws an empty bin flat on the axis and
    /// the fullest one at the top of it: what changes is only what the bins
    /// between them do with the room.
    #[test]
    fn both_count_axes_run_from_the_axis_to_the_top_of_the_plot() {
        for log in [false, true] {
            assert_eq!(bar_fraction(0, 1000, log), 0.0, "an empty bin is flat");
            assert_eq!(bar_fraction(1000, 1000, log), 1.0, "the peak fills it");
            // A plot of nothing at all, which a blank image gives: no bar is
            // drawn off the top of it.
            assert_eq!(bar_fraction(0, 0, log), 0.0);
        }
    }

    /// And the logarithm lifts the short bars towards the tall one, which is
    /// the whole reason to reach for it: a bin holding a thousandth of what
    /// the fullest one holds is a hair off the axis linearly, and half the
    /// height of the plot once the axis is logarithmic.
    #[test]
    fn the_logarithm_lifts_the_bins_a_dominating_one_flattens() {
        let (linear, log) = (
            bar_fraction(1_000, 1_000_000, false),
            bar_fraction(1_000, 1_000_000, true),
        );
        assert!(linear < 0.01, "{linear}");
        assert!((0.4..0.6).contains(&log), "{log}");

        // Monotone either way: a fuller bin is never drawn shorter than an
        // emptier one, which is what keeps the shape on the plot readable as
        // the distribution however it is scaled.
        for log in [false, true] {
            let heights: Vec<f32> = [0, 1, 2, 10, 500, 999, 1000]
                .into_iter()
                .map(|count| bar_fraction(count, 1000, log))
                .collect();
            assert!(
                heights.windows(2).all(|pair| pair[0] < pair[1]),
                "log {log}: {heights:?}"
            );
        }
    }

    /// The readout is set in the middle of the line, and the ends of the axis
    /// keep their corners until it actually reaches them — at which point
    /// both go, rather than one.
    #[test]
    fn the_axis_ends_give_the_line_up_to_the_readout_and_not_before() {
        let bars = bars(panel(Rect::new(0.0, 0.0, 800.0, 600.0)).expect("room"));
        let centered = |width: f32| bars.x + (bars.width - width) / 2.0;

        let (x, fits) = readout_placement(bars, 80.0, [40.0, 40.0]);
        assert_eq!(x, centered(80.0), "centered on the plot, not on the panel");
        assert!(fits, "80 in the middle and 40 either side of 256 is room");

        let room = (bars.width - 80.0) / 2.0 - LABEL_GAP;
        assert!(
            readout_placement(bars, 80.0, [room, room]).1,
            "exactly room"
        );
        assert!(
            !readout_placement(bars, 80.0, [room + 0.5, room]).1,
            "and a hair less is not — the left end alone decides for both"
        );
        assert!(!readout_placement(bars, 80.0, [room, room + 0.5]).1);
        assert!(
            !readout_placement(bars, bars.width + 1.0, [0.0, 0.0]).1,
            "a readout wider than the plot leaves no line to share"
        );
    }
}
