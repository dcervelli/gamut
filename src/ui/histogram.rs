//! The floating histogram panel.

use crate::image::display::{AutoWindow, Colormap, Display, ToneMap};
use crate::image::stats::BINS;
use crate::render::{Blend, Color, Rect, TextMeasure, UiFrame};
use crate::theme::Theme;

use super::buttons::{ICON_SIDE, button_ink, centered_text, outline, text_top};
use super::chrome::BUTTON_SIZE;
use super::icon::{self, Mark};
use super::status::format_window;
use super::tooltip::{Opens, Tip, Tips};
use super::{
    BECOMES, Current, FrameInput, PADDING, PANEL_INSET, PANEL_RADIUS, PANEL_WIDTH, Panels,
    TEXT_SIZE, Widget,
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
const NUDGES: [(Widget, &[Mark]); 4] = [
    (Widget::WindowDown, icon::CHEVRONS_LEFT),
    (Widget::WindowNarrow, icon::CHEVRONS_RIGHT_LEFT),
    (Widget::WindowWiden, icon::CHEVRONS_LEFT_RIGHT),
    (Widget::WindowUp, icon::CHEVRONS_RIGHT),
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
/// Public because the pointer is tested against the whole panel from outside
/// the frame: what lands on it belongs to it, and must not reach the picture
/// it is floating over.
pub fn panel(content: Rect) -> Rect {
    // Rounded, so that the whole-pixel bin spacing starts on a pixel edge.
    Rect::new(
        (content.right() - HISTOGRAM_SIZE[0] - PADDING)
            .max(content.x + PADDING)
            .round(),
        (content.y + PADDING).round(),
        HISTOGRAM_SIZE[0],
        HISTOGRAM_SIZE[1],
    )
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
fn toolbar(gray: bool) -> &'static [Widget] {
    const GRAY: [Widget; 3] = [Widget::Luma, Widget::Log, Widget::Reset];
    const COLOR: [Widget; 4] = [Widget::Luma, Widget::Planes, Widget::Log, Widget::Reset];
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
fn row_label(widget: Widget, display: &Display) -> Option<(String, bool)> {
    Some(match widget {
        // The step's own worth, written as it acts: one source for the number
        // on the button and the number the press is worth.
        Widget::ExposureDown => (format!("{:+.2}", -EV_STEP), false),
        Widget::ExposureUp => (format!("{:+.2}", EV_STEP), false),
        Widget::Window(index) => (WINDOWS.get(index)?.0.to_string(), false),
        // Capitalized, where the bar sets the same word in the middle of a
        // line: a button wears a name, and a name starts with a capital.
        Widget::Curve(index) => {
            let curve = *ToneMap::ALL.get(index)?;
            (capitalized(curve.label()), display.tone_map == curve)
        }
        _ => return None,
    })
}

/// `label` with its first letter capitalized.
fn capitalized(label: &str) -> String {
    let mut letters = label.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().chain(letters).collect(),
        None => String::new(),
    }
}

/// Every button in that block, with where it goes. The one list the drawing,
/// the pointer and the tooltips all work from.
///
/// Every image has all of them: an exposure, a window and a curve are what
/// the display does to any file whatever, where the two controls the panel
/// leaves out are about what the file itself holds.
fn row_buttons(panel: Rect) -> impl Iterator<Item = (Widget, Rect)> {
    let rows = Rows::new(panel);
    let steps = [
        (Widget::ExposureDown, rows.step(false)),
        (Widget::ExposureUp, rows.step(true)),
    ];
    let windows = (0..WINDOWS.len()).map(move |index| {
        (
            Widget::Window(index),
            share(rows.window, WINDOWS.len(), index),
        )
    });
    let curves = (0..ToneMap::ALL.len()).map(move |index| {
        (
            Widget::Curve(index),
            share(rows.curve, ToneMap::ALL.len(), index),
        )
    });
    let nudges = NUDGES
        .iter()
        .enumerate()
        .map(move |(index, (widget, _))| (*widget, share(rows.nudges(), NUDGES.len(), index)));
    steps.into_iter().chain(nudges).chain(windows).chain(curves)
}

/// Which of the panel's own buttons a point lands on.
///
/// Two of them do not apply to every image, and `gray` decides both: a
/// single-channel image has no color planes to toggle, and a color image is
/// its own color, so the false colors are what the display leaves out for
/// it. Neither is drawn where it does not apply, and neither answers here.
pub fn widget_at(content: Rect, point: [f32; 2], gray: bool) -> Option<Widget> {
    let panel = panel(content);
    if !panel.contains(point) {
        return None;
    }
    for (index, widget) in toolbar(gray).iter().enumerate() {
        if toolbar_button(panel, gray, index).contains(point) {
            return Some(*widget);
        }
    }
    if let Some((widget, _)) = row_buttons(panel).find(|(_, rect)| rect.contains(point)) {
        return Some(widget);
    }
    if !gray {
        return None;
    }
    let bars = plot_area(panel, gray);
    (0..Colormap::ALL.len())
        .find(|&index| swatch_button(bars, index).contains(point))
        .map(Widget::Ramp)
}

/// Offers every button on the panel to the tooltips, each naming itself into
/// the plot beside it: a label about the plot should be read without looking
/// away from the plot, and the panel is wide enough to hold one.
///
/// The same rectangles [`widget_at`] answers the pointer with, so that what a
/// tooltip hangs from is what the pointer found.
pub(super) fn offer_tips(tips: &mut Tips, content: Rect, gray: bool) {
    let panel = panel(content);
    for (index, widget) in toolbar(gray).iter().enumerate() {
        tips.offer_toward(
            Tip::Widget(*widget),
            toolbar_button(panel, gray, index),
            Opens::Right,
        );
    }
    // Beside themselves rather than under themselves, like the toggles: these
    // are stacked in rows, and a label opening downwards would cover the row
    // the pointer is on its way to.
    for (widget, rect) in row_buttons(panel) {
        tips.offer_toward(Tip::Widget(widget), rect, Opens::Right);
    }
    if !gray {
        return;
    }
    let bars = plot_area(panel, gray);
    for index in 0..Colormap::ALL.len() {
        tips.offer_toward(
            Tip::Widget(Widget::Ramp(index)),
            swatch_button(bars, index),
            Opens::Right,
        );
    }
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
    let plot = plot_area(panel(content), current.image.is_gray());
    if let Some(bin) = hovered_bin(plot, cursor) {
        return Some(bin);
    }
    let at = pointer?;
    let sample = current.image.sample(at[0], at[1])?;
    current.stats.plot.bin_of(&current.image, &sample)
}

/// Draws the histogram in the top-right of `content`, the area the panels
/// leave free — above the information panel, the order the two toggles that
/// open them are stacked in.
///
/// Color images get four planes — red, green, blue and luminance — over the
/// range their color channels span; gray images keep the single luminance
/// plane over theirs.
pub(super) fn draw(
    frame: &mut UiFrame,
    text: &mut dyn TextMeasure,
    current: &Current,
    input: &FrameInput,
    panels: &Panels,
    content: Rect,
    theme: &Theme,
) {
    let panel = panel(content);
    frame.rounded_rect(panel, PANEL_RADIUS, theme.panel_background);

    // What applies to this image: the false colors are for a single channel
    // and the color planes are for three, and the panel leaves out whichever
    // the display would ignore rather than drawing it dead.
    let gray = current.image.is_gray();
    let bars = plot_area(panel, gray);
    frame.rounded_rect(
        bars.inset(-PLOT_INSET, -PLOT_INSET),
        PLOT_RADIUS,
        theme.plot_background,
    );

    // Luminance always goes down first, underneath the color planes: it
    // runs as tall as the tallest of them about as often as not, and painting
    // it on top swamps the color the panel exists to show.
    let plotted = &current.stats.plot;
    let luma: &[[u32; BINS]] = if panels.show_luma {
        std::slice::from_ref(&plotted.luma)
    } else {
        &[]
    };
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
    // Half of what every value comes out as: on a surface with room above
    // white and no curve on, the response runs past 1, and so does the
    // readout, since that is what the screen is showing.
    let headroom = input.headroom;
    let marked = marked(current, content, input.cursor, input.pointer);
    let across = marked.map(bin_across);
    let mut ends_fit = true;
    if let Some(across) = across {
        let value = transfer.to_linear(axis_min + across * span);
        let mapped = current.display.response(value, headroom).max(0.0);
        let readout = format!("{value:.4}  {BECOMES}  {mapped:.4}");
        let ends_width = [axis_min, axis_max].map(|end| {
            text.measure_text(&format!("{:.4}", transfer.to_linear(end)), label_size)[0]
        });
        let width = text.measure_text(&readout, label_size)[0];
        let (x, fits) = readout_placement(bars, width, ends_width);
        ends_fit = fits;
        frame.text([x, label_y], label_size, theme.text_primary, readout);
    }
    if ends_fit {
        // Pinned to the ends of the axis they name rather than set together
        // in the corner: each is the value of the plot directly below it.
        let high = format!("{:.4}", transfer.to_linear(axis_max));
        let high_width = text.measure_text(&high, label_size)[0];
        frame.text(
            [bars.x, label_y],
            label_size,
            theme.text_dim,
            format!("{:.4}", transfer.to_linear(axis_min)),
        );
        frame.text(
            [bars.right() - high_width, label_y],
            label_size,
            theme.text_dim,
            high,
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
    // One point per bin, at its center, with the ends carried out to the
    // edges of the plot so the shape fills its width.
    let curve = |counts: &[u32; BINS]| -> Vec<[f32; 2]> {
        counts
            .iter()
            .enumerate()
            .map(|(index, &count)| {
                let x = bars.x + bin_across(index) * bars.width;
                [x, bars.bottom() - height_of(count)]
            })
            .collect()
    };

    // Dimmed only when it is a backdrop; with the color planes off — or on
    // a gray image, which has none — it is the plot.
    let luma_ink = if color.is_empty() {
        theme.histogram_luma
    } else {
        theme.histogram_luma.with_alpha(HISTOGRAM_LUMA_UNDER)
    };
    for counts in luma {
        frame.area(&curve(counts), bars.bottom(), luma_ink, Blend::Over);
    }
    for (counts, color) in color.iter().zip(theme.histogram_planes) {
        frame.area(&curve(counts), bars.bottom(), color, Blend::Screen);
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
        frame.rect(
            on_device(
                Rect::new(
                    bars.x + across * bars.width - CURSOR_WIDTH / 2.0,
                    bars.y,
                    CURSOR_WIDTH,
                    bars.height,
                ),
                input.scale,
            ),
            theme.accent.with_alpha(CURSOR_ALPHA),
        );
    }

    controls(frame, text, current, input, panels, panel, bars, theme);

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
    // That is a job for a tick against the axis, not for a rule standing
    // through the plot in the ink the curve is drawn in.
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
            frame.rect(
                on_device(
                    Rect::new(
                        bars.x + position * bars.width - TICK_WIDTH / 2.0,
                        bars.bottom() - TICK_RISE,
                        TICK_WIDTH,
                        TICK_HEIGHT,
                    ),
                    input.scale,
                ),
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
        //
        // One cell per bin, over the bin's own middle, so a cell is the
        // color of the bar standing above it.
        // On the device's pixels, like every other mark here: a cell is
        // about one logical pixel wide, so unsnapped the band ripples at the
        // beat of the scale factor. See [`device`].
        let band = ramp(bars);
        let snap = |value: f32| device(value, input.scale);
        let (top, bottom) = (snap(band.y), snap(band.bottom()));
        let edge = |index: usize| snap(band.x + band.width * index as f32 / BINS as f32);

        // A cell above white — which only a surface with room above white
        // has, and only with no curve on — is drawn white, since the panel
        // cannot glow, with the accent along its top edge to say that the
        // screen does: the same ink as the tick that marks white on the
        // axis, and the run of it is how much of the axis is out past that.
        let channels = current.image.channels();
        let hair = 1.0 / input.scale;
        for index in 0..BINS {
            let (left, right) = (edge(index), edge(index + 1));
            let across = (index as f32 + 0.5) / BINS as f32;
            let value = transfer.to_linear(axis_min + across * span);
            frame.rect(
                Rect::new(left, top, right - left, bottom - top),
                Color::from_linear(current.display.shade(value, channels, headroom)),
            );
            if current.display.response(value, headroom) > ABOVE_WHITE {
                frame.rect(Rect::new(left, top, right - left, hair), theme.accent);
            }
        }
        // Outside the color rather than over it, so that the band keeps its
        // full depth. A window left of everything makes the whole ramp black,
        // and a black band on a dark panel is a gap in it without this. One
        // physical pixel, snapped like the band it rings.
        let (left, right) = (edge(0), edge(BINS));
        outline(
            frame,
            Rect::new(
                left - hair,
                top - hair,
                right - left + 2.0 * hair,
                bottom - top + 2.0 * hair,
            ),
            hair,
            theme.border,
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
            // doing nothing is the diagonal. Plotting the linear response
            // against an encoded axis would bend the curve by the transfer
            // function alone, and draw a shoulder into an image nobody had
            // touched.
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
            let scale = Scale::new(transfer.to_encoded(1.0), highest);
            if let Some(white) = scale.white {
                let y = device(bars.bottom() - white * bars.height, input.scale);
                frame.rect(
                    Rect::new(bars.x, y, bars.width, 1.0 / input.scale),
                    theme.text_dim,
                );
                let size = TEXT_SIZE * 0.75;
                let width = text.measure_text(WHITE_LABEL, size)[0];
                frame.text(
                    [bars.right() - width - 2.0, y - size * 1.3],
                    size,
                    theme.text_dim,
                    WHITE_LABEL,
                );
            }
            let curve: Vec<[f32; 2]> = responses
                .iter()
                .enumerate()
                .map(|(column, &response)| {
                    let across = column as f32 / columns as f32;
                    [
                        bars.x + across * bars.width,
                        bars.bottom() - scale.up(response) * bars.height,
                    ]
                })
                .collect();
            frame.polyline(&curve, CURVE_WIDTH, theme.accent, Blend::Over);
        }
    }
}

/// The strip of buttons down the left of the panel, and the row of false
/// colors under its ramp.
///
/// Drawn here rather than with the chrome's toggles because these belong to
/// the panel: they say what the plot beside them is showing and what the band
/// beneath them is painted with, and two of them are pictures of the very
/// thing they switch.
#[allow(clippy::too_many_arguments)]
fn controls(
    frame: &mut UiFrame,
    text: &mut dyn TextMeasure,
    current: &Current,
    input: &FrameInput,
    panels: &Panels,
    panel: Rect,
    bars: Rect,
    theme: &Theme,
) {
    let gray = current.image.is_gray();
    let hovered = |widget: Widget| panels.hover == Some(widget);

    for (slot, widget) in toolbar(gray).iter().enumerate() {
        let button = toolbar_button(panel, gray, slot);
        let active = match widget {
            Widget::Luma => panels.show_luma,
            Widget::Planes => panels.show_planes,
            Widget::Log => panels.log_counts,
            // The reset is never lit, where the two above it are: it does
            // something rather than being something, and a momentary button
            // holding a state is a button that has to explain itself.
            _ => false,
        };
        let (background, ink) = button_ink(active, hovered(*widget), theme);
        frame.rounded_rect(button, TOGGLE_RADIUS, background);

        let square = icon::fit(frame, button, ICON_SIDE);
        match widget {
            // The two plane toggles are drawn here rather than taken from
            // `ui::icon` because they are pictures of the planes themselves,
            // each in the color that plane is plotted in — which is not
            // something a mark drawn in one ink can be.
            //
            // Luminance is one plane, so it is one disc, in the neutral the
            // plot draws that plane in.
            Widget::Luma => {
                let place = icon::Placer::new(frame, square);
                frame.circle(
                    place.free(frame, [GRID_MIDDLE, GRID_MIDDLE]),
                    place.units(LUMA_DISC),
                    theme.histogram_luma,
                );
            }
            // And the color planes are three, so they are three smaller
            // discs, in their own colors: nothing else in the window is red,
            // green and blue together.
            Widget::Planes => {
                let place = icon::Placer::new(frame, square);
                for (turn, plane) in theme.histogram_planes.into_iter().enumerate() {
                    // Struck about the middle at a third of a turn each,
                    // starting at the top, so the three read as one mark
                    // rather than as a row.
                    let angle = (-90.0 + 120.0 * turn as f32).to_radians();
                    let at = [
                        GRID_MIDDLE + PLANE_ORBIT * angle.cos(),
                        GRID_MIDDLE + PLANE_ORBIT * angle.sin(),
                    ];
                    frame.circle(place.free(frame, at), place.units(PLANE_DISC), plane);
                }
            }
            // The count axis as a curve, which is what the switch puts it on.
            Widget::Log => icon::draw(frame, icon::SPLINE, square, ink, background),
            // Back to the start.
            _ => icon::draw(frame, icon::ROTATE_CCW, square, ink, background),
        }
    }

    rows(frame, text, current, panels, panel, theme);

    // The false colors, each showing itself, and only where the display
    // would act on the choice. The whole ramp rather than one color off it:
    // a map is a sequence, and a single swatch of viridis is a green
    // rectangle that could be anything.
    if !gray {
        return;
    }
    for (index, map) in Colormap::ALL.into_iter().enumerate() {
        let button = swatch_button(bars, index);
        let chosen = current.display.colormap == map;
        let (background, _) = button_ink(chosen, hovered(Widget::Ramp(index)), theme);
        frame.rounded_rect(button, SWATCH_RADIUS, background);

        // The gradient on the device's pixels, as the band above it is: a
        // swatch is the same row of one-pixel cells, over less room.
        let face = button.inset(SWATCH_INSET, SWATCH_INSET);
        let snap = |value: f32| device(value, input.scale);
        let (top, bottom) = (snap(face.y), snap(face.bottom()));
        let steps = (face.width * input.scale).max(1.0) as usize;
        for step in 0..steps {
            let edge = |step: usize| snap(face.x + face.width * step as f32 / steps as f32);
            let (left, right) = (edge(step), edge(step + 1));
            let t = (step as f32 + 0.5) / steps as f32;
            frame.rect(
                Rect::new(left, top, right - left, bottom - top),
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
fn rows(
    frame: &mut UiFrame,
    text: &mut dyn TextMeasure,
    current: &Current,
    panels: &Panels,
    panel: Rect,
    theme: &Theme,
) {
    let rows = Rows::new(panel);
    let display = &current.display;

    // The words down the left, set back the way a fact in a bar is set behind
    // the name it is about: the rows are read for their values, and these say
    // which value is which.
    for (label, at) in ["EV", "Window", "Curve"].into_iter().zip(rows.labels) {
        frame.text(
            [frame.snap(at.x), text_top(frame, text, at, ROW_TEXT)],
            ROW_TEXT,
            theme.text_dim,
            label,
        );
    }

    // The exposure between its two steps, and the window along its own line.
    // Both in the units the rest of the interface quotes them in: stops for
    // the one, and for the other the bounds the bottom bar writes out, in the
    // file's own counts where the file is counting things.
    //
    // The bounds alone, and not the name of the rule they came from: what a
    // window is for is the two numbers, the rule is a fact about how they
    // were arrived at, and the bar at the foot of the window already says it.
    let stops = rows.stops();
    frame.text(
        [frame.snap(stops.x), text_top(frame, text, stops, ROW_TEXT)],
        ROW_TEXT,
        theme.text_primary,
        format!("{:.2}", display.exposure_stops),
    );
    let window = rows.window_reading();
    frame.text_clipped(
        [
            frame.snap(window.x),
            text_top(frame, text, window, ROW_TEXT),
        ],
        ROW_TEXT,
        theme.text_primary,
        window.width,
        format_window(current),
    );

    for (widget, rect) in row_buttons(panel) {
        let worn = row_label(widget, display);
        let active = worn.as_ref().is_some_and(|(_, active)| *active);
        let (background, ink) = button_ink(active, panels.hover == Some(widget), theme);
        frame.rounded_rect(rect, TOGGLE_RADIUS, background);
        match &worn {
            Some((label, _)) => centered_text(frame, text, rect, ink, label, ROW_TEXT),
            // The nudges wear marks where the rest of the block wears words:
            // what they do to the two ends of the window is a shape, and four
            // of them spelled out would be a paragraph on a line that is
            // already a number.
            None => {
                if let Some((_, marks)) = NUDGES.iter().find(|(nudge, _)| *nudge == widget) {
                    icon::draw(
                        frame,
                        marks,
                        icon::fit(frame, rect, NUDGE_ICON),
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
    use crate::render::ui_tests::test_fonts;

    /// The pointer reads a bin of the plot and nothing outside it — not the
    /// panel around it, and not the line the axis labels are set on.
    #[test]
    fn only_the_plot_itself_answers_the_pointer() {
        let bars = bars(panel(Rect::new(0.0, 0.0, 800.0, 600.0)));

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
        let bars = bars(panel(Rect::new(0.0, 0.0, 800.0, 600.0)));
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

    fn middle(rect: Rect) -> [f32; 2] {
        [rect.x + rect.width / 2.0, rect.y + rect.height / 2.0]
    }

    /// The panel grew a strip of buttons and a row of ramps around the plot,
    /// and the plot itself did not move across: a bin is one logical pixel,
    /// which is what keeps the bars from landing astride a pixel boundary.
    #[test]
    fn the_plot_keeps_one_pixel_to_the_bin_whatever_grows_around_it() {
        let panel = panel(Rect::new(0.0, 0.0, 800.0, 600.0));
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
        let panel = panel(Rect::new(0.0, 0.0, 800.0, 600.0));
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

    /// Each of the panel's buttons answers inside its own bounds and nowhere
    /// else — not in the gaps between them, and not on the plot.
    #[test]
    fn the_panels_buttons_answer_within_their_own_bounds() {
        let content = Rect::new(0.0, 0.0, 800.0, 600.0);
        let panel = panel(content);

        for gray in [true, false] {
            for (index, widget) in toolbar(gray).iter().enumerate() {
                let at = middle(toolbar_button(panel, gray, index));
                assert_eq!(widget_at(content, at, gray), Some(*widget));
            }
            let plot = plot_area(panel, gray);
            assert_eq!(widget_at(content, middle(plot), gray), None, "the plot");
        }

        let bars = plot_area(panel, true);
        for index in 0..Colormap::ALL.len() {
            let at = middle(swatch_button(bars, index));
            assert_eq!(widget_at(content, at, true), Some(Widget::Ramp(index)));
        }
        assert_eq!(
            widget_at(content, [panel.x - 1.0, panel.y + 20.0], true),
            None,
            "off the panel altogether"
        );
        let gap = swatch_button(bars, 0).right() + RAMP_GAP / 2.0;
        let row = middle(swatch_button(bars, 0))[1];
        assert_eq!(widget_at(content, [gap, row], true), None, "between two");
    }

    /// A control the display would ignore is not on the panel at all, and the
    /// ones that remain close the gap up rather than leaving a hole where it
    /// would have been.
    #[test]
    fn a_control_that_could_do_nothing_is_not_there() {
        let content = Rect::new(0.0, 0.0, 800.0, 600.0);
        let panel = panel(content);

        assert_eq!(toolbar(true), [Widget::Luma, Widget::Log, Widget::Reset]);
        assert_eq!(
            toolbar(false),
            [Widget::Luma, Widget::Planes, Widget::Log, Widget::Reset],
            "three channels have planes to toggle"
        );
        // The reset moves up into the room the planes toggle is not taking.
        assert_eq!(
            toolbar_button(panel, true, 1).y,
            toolbar_button(panel, false, 1).y,
            "and the slot it leaves is filled, not left empty"
        );

        // Nothing answers where the row of false colors would be on an image
        // that has none: the plot is there instead.
        let swatch = middle(swatch_button(plot_area(panel, true), 2));
        assert_eq!(widget_at(content, swatch, true), Some(Widget::Ramp(2)));
        assert_eq!(
            widget_at(content, swatch, false),
            None,
            "three channels are their own color, and the false ones are not applied"
        );
    }

    /// The three rows sit under the band, inside the panel, and land in the
    /// same place whether or not the image has a row of false colors — the
    /// band and the swatches under it end on the same line, so the controls
    /// below them do not shift about from one file to the next.
    #[test]
    fn the_rows_sit_under_the_band_whatever_it_ends_in() {
        let panel = panel(Rect::new(0.0, 0.0, 800.0, 600.0));
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

    /// Every one of them answers the pointer inside its own bounds and
    /// nowhere else — not in the column of words beside them, and not in the
    /// gaps between one and the next.
    #[test]
    fn the_rows_answer_the_pointer_where_they_were_drawn() {
        let content = Rect::new(0.0, 0.0, 800.0, 600.0);
        let panel = panel(content);
        let rows = Rows::new(panel);

        // Every image has them, gray or not: what they set is what the
        // display does to any file whatever.
        for gray in [true, false] {
            for (widget, rect) in row_buttons(panel) {
                assert_eq!(widget_at(content, middle(rect), gray), Some(widget));
            }
            assert_eq!(
                widget_at(content, middle(rows.labels[0]), gray),
                None,
                "the word naming a row is not a button"
            );
            assert_eq!(
                widget_at(content, middle(rows.stops()), gray),
                None,
                "nor is the exposure the two steps after it act on"
            );
            assert_eq!(
                widget_at(content, middle(rows.window_reading()), gray),
                None,
                "nor the window the four nudges after it act on"
            );
            let gap = [
                share(rows.window, WINDOWS.len(), 0).right() + CELL_GAP / 2.0,
                middle(rows.window)[1],
            ];
            assert_eq!(widget_at(content, gap, gray), None, "between two windows");
        }
    }

    /// The exposure's two steps are worth what they say they are worth, and
    /// the windows and the curves cover everything there is to choose.
    #[test]
    fn the_rows_offer_every_choice_there_is() {
        let panel = panel(Rect::new(0.0, 0.0, 800.0, 600.0));
        let widgets: Vec<Widget> = row_buttons(panel).map(|(widget, _)| widget).collect();
        assert_eq!(widgets[0], Widget::ExposureDown);
        assert_eq!(widgets[1], Widget::ExposureUp);
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
                Widget::WindowDown,
                Widget::WindowNarrow,
                Widget::WindowWiden,
                Widget::WindowUp
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

    /// The least room a word may leave inside the cell it is set in, in
    /// logical pixels: enough that it is not set against the button's rounded
    /// corners, whichever of the labels it is.
    const ROOM: f32 = 5.0;

    /// Every word on the three rows fits the room laid out for it — the three
    /// naming the rows, and the label each button wears.
    ///
    /// The layout is in constants, the pointer having to be answered where
    /// there are no fonts to ask. This is what holds those constants to the
    /// face the panel is actually set in: too mean and a label is clipped or
    /// spills over its neighbor, and no measurement anywhere else would catch
    /// it.
    #[test]
    fn every_word_on_the_rows_fits_the_room_it_is_given() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let panel = panel(Rect::new(0.0, 0.0, 800.0, 600.0));
        let mut width = |label: &str| fonts.measure_text(label, ROW_TEXT)[0];

        for word in ["EV", "Window", "Curve"] {
            let room = ROW_LABEL - width(word);
            assert!(room >= 0.0, "\"{word}\" overruns its column by {room}");
        }
        // The two readings lead their rows and the buttons follow them, so
        // what is set aside for each has to hold what it can be asked to
        // say — for the window, a float raster's bounds written out in full.
        let rows = Rows::new(panel);
        for reading in [
            "0.000\u{2013}1.000",
            "-32768\u{2013}65535",
            "-431.000\u{2013}8848.000",
        ] {
            let room = rows.window_reading().width;
            assert!(
                width(reading) <= room,
                "\"{reading}\" needs more than {room}"
            );
        }
        for stops in ["0.00", "-16.00"] {
            let room = rows.stops().width;
            assert!(width(stops) <= room, "\"{stops}\" needs more than {room}");
        }

        let display = Display::default();
        for (widget, rect) in row_buttons(panel) {
            // A nudge wears a mark rather than a word, and a mark is fitted
            // to its button by `icon::fit` rather than measured.
            let Some((label, _)) = row_label(widget, &display) else {
                assert!(
                    NUDGES.iter().any(|(nudge, _)| *nudge == widget),
                    "{widget:?} wears neither a word nor a mark"
                );
                continue;
            };
            let room = rect.width - width(&label);
            assert!(room >= 2.0 * ROOM, "\"{label}\" leaves {room} in {rect:?}");
        }
    }

    /// The readout is set in the middle of the line, and the ends of the axis
    /// keep their corners until it actually reaches them — at which point
    /// both go, rather than one.
    #[test]
    fn the_axis_ends_give_the_line_up_to_the_readout_and_not_before() {
        let bars = bars(panel(Rect::new(0.0, 0.0, 800.0, 600.0)));
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
