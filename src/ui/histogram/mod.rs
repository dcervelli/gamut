//! The floating histogram panel: its geometry, the words on it, and the
//! header and rows it is laid out with. The plot is `plot`, the band and
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

use egui::{Align2, Color32, FontId, Sense, WidgetInfo, WidgetType, pos2, vec2};

use crate::image::display::{AutoWindow, Colormap, Display, EV_STEP, ToneMap};
use crate::image::stats::BINS;
use crate::render::Color;

use super::Rect;
use crate::theme::Theme;

use super::chrome::{BUTTON_SIZE, ICON_SIDE, Pass};
use super::icon;
use super::outline;
use super::tooltip::Tip;
use super::{
    BECOMES, Command, Control, Current, PADDING, PANEL_INSET, PANEL_RADIUS, PANEL_WIDTH, TEXT_SIZE,
};

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
/// file, so the information column under the panel never jumps.
const ROWS_HEIGHT: f32 = PLOT_INSET + 3.0 * (ROW_GAP + ROW_HEIGHT);

/// The panel: as wide as anything else floating over the content area, and
/// tall enough for the line the readout is set on, a plot, the band of what
/// the display makes of its axis, the row of false colors a gray image has
/// under that, and the three rows of settings. What a window has to have
/// room for before the toggle is lit — see [`super::PANELS_ROOM`].
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

/// The two handles on the band, which are the window: how wide the mark
/// is, how far it stands above and below the band, and how wide the room
/// around it that takes the pointer is — wider than the mark, since a mark
/// five pixels wide is not something a hand lands on.
const HANDLE_WIDTH: f32 = 5.0;
const HANDLE_REACH: f32 = 3.0;
const HANDLE_GRIP: f32 = 14.0;
/// The corner a handle is drawn with, and the hairline around it in the
/// panel's own ground, which is what parts it from a band the same color.
const HANDLE_RADIUS: f32 = 1.5;
const HANDLE_RING: f32 = 1.0;

/// How far the exposure's slider runs each way, in stops. Not the whole of
/// what the exposure can be — the keys and `--exposure` go on to the
/// model's own limit — but the run a hand wants at a quarter of a stop to
/// the step: six stops each way is a shadow lifted out of black or a
/// highlight brought back from four stops over, and across the slider a
/// step is still a few pixels wide. Past the end the handle stands hollow,
/// as the band's do, and the number beside it says where the exposure is.
const SLIDER_STOPS: f32 = 6.0;
/// The slider's track: how thick the groove is, and how tall the mark at
/// nothing stands, either side of it.
const SLIDER_TRACK: f32 = 2.0;
const SLIDER_TICK: f32 = 4.0;
/// How far the groove is faded toward the panel: the dim ink, held back so
/// that a line two pixels thick reads as a groove under the handle rather
/// than as a rule across the row, while a button's own ground, which is
/// what the rows below are drawn in, comes out too faint at that width to
/// be seen at all.
const SLIDER_GROOVE_ALPHA: u8 = 90;
/// How tall the handle on it stands, the band's handles' width wide.
const SLIDER_HANDLE: f32 = 12.0;

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

/// The panel's own rectangle inside `content`: the top right corner, inside
/// the padding everything floating over the image keeps.
///
/// `None` where the content area is too small to take it, as the information
/// panel's [`info::panel`](super::info::panel) is `None` in a window too
/// short for it. The panel is one size for a file — the plot's bins are a
/// logical pixel each and the rows under it are set to what they say, so
/// there is nothing here to give — and a panel drawn larger than the area
/// it floats over would cover the picture it is about and run off the
/// window besides.
///
/// Public because the pointer is tested against the whole panel from outside
/// the frame: what lands on it belongs to it, and must not reach the picture
/// it is floating over.
pub fn panel(content: Rect) -> Option<Rect> {
    let size = SIZE;
    if size[0] + 2.0 * PADDING > content.width || size[1] + 2.0 * PADDING > content.height {
        return None;
    }
    // Rounded, so that the whole-pixel bin spacing starts on a pixel edge.
    Some(Rect::new(
        (content.right() - size[0] - PADDING).round(),
        (content.y + PADDING).round(),
        size[0],
        size[1],
    ))
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
    content: Rect,
    cursor: Option<[f32; 2]>,
    pointer: Option<[u32; 2]>,
) -> Option<usize> {
    let panel = panel(content)?;
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

/// A handle on a line — the band's two, and the exposure's — drawn as the
/// same kind of thing, a value on a line: `mark`, already on the device's
/// grid, in the accent every mark on the plot wears, or the primary ink
/// while the hand is `on` it, ringed in the panel's ground so that it
/// stays a shape against a band that has come round to the same color.
/// Hollow where `t`, its place along the line, is out past either end, so
/// that it can be taken hold of and brought back without claiming a
/// boundary that is not there.
fn handle(painter: &egui::Painter, theme: &Theme, grid: icon::Grid, mark: Rect, on: bool, t: f32) {
    let ring = mark.inset(-HANDLE_RING, -HANDLE_RING);
    painter.rect_filled(
        area(ring),
        HANDLE_RADIUS + HANDLE_RING,
        theme.panel_background,
    );
    let ink: Color32 = if on { theme.text_primary } else { theme.accent }.into();
    if (0.0..=1.0).contains(&t) {
        painter.rect_filled(area(mark), HANDLE_RADIUS, ink);
    } else {
        outline(painter, grid, mark, grid.line_width(1.0), ink);
    }
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
            let held = controls(pass, ui, current, panel);
            header(pass, ui, current, panel, held);
        });
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
    let response = ui.allocate_rect(area(rect), Sense::CLICK);
    let (background, ink) = pass.button_ink(active, &response, enabled);
    ui.painter().rect_filled(area(rect), radius, background);
    response
        .widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, active, control.label()));
    let response = pass.tooltip(response, Tip::Control(control), enabled);
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
    let grid = icon::Grid::new(ui.pixels_per_point());

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
            area(rect).center(),
            Align2::CENTER_CENTER,
            label,
            font.clone(),
            ink,
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::image::display::{Headroom, Startup};
    use crate::image::exif::Exif;
    use crate::image::sequence::Sequence;
    use crate::image::{AlphaMode, Channels, ColorSpace, DecodedImage, Samples, Stats};
    use crate::ui::FileFacts;

    /// A content area with room for everything.
    fn content() -> Rect {
        Rect::new(0.0, 0.0, 800.0, 600.0)
    }

    fn full_panel() -> Rect {
        panel(content()).expect("room")
    }

    /// `image`, on screen.
    fn shown(image: DecodedImage) -> Current {
        let stats = Stats::scan(&image);
        Current {
            display: Display::for_image_with(&image, &stats, Startup::default(), Headroom::None),
            image: Arc::new(image),
            stats,
            label: "file".into(),
            file: FileFacts {
                path: "file".into(),
                bytes: None,
                modified: None,
                reader: None,
            },
            exif: Exif::default(),
            stored: None,
            sequence: Sequence::Still,
            page: 0,
            lift: None,
        }
    }

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

    /// A share of the picture is written so that a small one is still
    /// something and a large one is not four digits: nothing at all for
    /// none, a floor under the least that is not none, tenths while they
    /// tell the reader something and whole percents once they do not.
    #[test]
    fn a_clipped_share_is_written_to_be_read_at_a_glance() {
        assert_eq!(share_words(0.0), None);
        assert_eq!(share_words(-1.0), None);
        assert_eq!(share_words(f32::NAN), None);
        assert_eq!(share_words(0.0001).as_deref(), Some("<0.1%"));
        assert_eq!(share_words(0.001).as_deref(), Some("0.1%"));
        assert_eq!(share_words(0.0234).as_deref(), Some("2.3%"));
        assert_eq!(share_words(0.1).as_deref(), Some("10%"));
        assert_eq!(share_words(0.5).as_deref(), Some("50%"));
        assert_eq!(share_words(1.0).as_deref(), Some("100%"));
    }

    /// The ends of an axis and the value under the pointer are written with
    /// no more digits than they need: `1`, not `1.0000`.
    #[test]
    fn a_decimal_is_written_without_the_zeros_it_does_not_need() {
        assert_eq!(trimmed("1.000".into()), "1");
        assert_eq!(trimmed("0.500".into()), "0.5");
        assert_eq!(trimmed("0.059".into()), "0.059");
        assert_eq!(trimmed("0.000".into()), "0");
        assert_eq!(trimmed("-0.000".into()), "0");
        assert_eq!(trimmed("12".into()), "12");

        let graded = shown(DecodedImage::new(
            2,
            1,
            Samples::U8 {
                channels: Channels::Gray,
                data: vec![0, 255],
            },
            ColorSpace::SRGB,
            AlphaMode::Opaque,
        ));
        assert_eq!(axis_words(&graded, 1.0), "1");
        assert_eq!(axis_words(&graded, 0.0), "0");

        // Linear integer data reads back as the counts it was stored in.
        let counts = shown(DecodedImage::new(
            2,
            1,
            Samples::U16 {
                channels: Channels::Gray,
                data: vec![0, 4095],
            },
            ColorSpace::LINEAR_BT709,
            AlphaMode::Opaque,
        ));
        assert_eq!(axis_words(&counts, 4095.0 / 65535.0), "4095");
    }

    /// The pointer reads a bin of the plot and nothing outside it — not the
    /// panel around it, and not the line the axis labels are set on.
    #[test]
    fn only_the_plot_itself_answers_the_pointer() {
        let bars = bars(full_panel());

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
        let bars = bars(full_panel());
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
        let room = [SIZE[0] + 2.0 * PADDING, SIZE[1] + 2.0 * PADDING];
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

    /// A bin is one logical pixel, which is what keeps the bars from landing
    /// astride a pixel boundary, whatever stands beside the plot. Nor does
    /// the plot move down for the rows under it, which is what the panel
    /// grows by.
    #[test]
    fn the_plot_keeps_one_pixel_to_the_bin_whatever_grows_around_it() {
        let panel = full_panel();
        for gray in [true, false] {
            let bars = plot_area(panel, gray);
            assert_eq!(bars.width, BINS as f32, "gray {gray}");

            // Everything the panel holds is inside it, and clear of the
            // plot.
            let last = toolbar(gray).len() - 1;
            let button = toolbar_button(panel, gray, last);
            assert!(button.right() <= bars.x, "the strip clears the plot");
            assert!(toolbar_button(panel, gray, 0).y >= panel.y);
            assert!(button.bottom() <= panel.bottom(), "{button:?}");
            // And it ends above the band of color, which is what the
            // room under the plot is for: a button beside the ramp would
            // read as belonging to it rather than to the plot it acts on.
            assert!(
                button.bottom() <= ramp(bars).y,
                "gray {gray}: {button:?} against the band at {:?}",
                ramp(bars)
            );
            assert!(ramp(bars).y >= bars.bottom(), "the band is under the plot");
            assert!(ramp(bars).bottom() <= panel.bottom());
        }
    }

    /// The button that marks the clipped pixels stands beside the band, in
    /// the strip's own column and centered on the band, in the room between
    /// the stack above and the rows below — clear of both, whatever the
    /// stack holds, and clear of the black handle's grip beside it.
    #[test]
    fn the_marks_button_stands_beside_the_band() {
        let panel = full_panel();
        let button = marks_button(panel);
        let band = ramp(bars(panel));

        assert_eq!(button.x, toolbar_button(panel, false, 0).x, "in the strip");
        assert_eq!(
            button.y + button.height / 2.0,
            band.y + band.height / 2.0,
            "centered on the band"
        );
        for gray in [true, false] {
            let last = toolbar_button(panel, gray, toolbar(gray).len() - 1);
            assert!(
                button.y >= last.bottom() + TOOLBAR_GAP,
                "gray {gray}: {button:?} against the stack ending at {last:?}"
            );
        }
        assert!(button.bottom() <= Rows::new(panel).exposure.y, "{button:?}");
        assert!(
            button.right() <= grip(band, band.x).x,
            "{button:?} against the handle at {:?}",
            grip(band, band.x)
        );
    }

    /// The false colors take their room off the plot rather than off the
    /// panel, so that the information panel below does not shift about from
    /// one file to the next.
    #[test]
    fn the_panel_is_one_height_with_the_false_colors_and_without() {
        let panel = full_panel();
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

    /// The handles stand on the band and reach past it, and the room that
    /// takes the pointer is wider than the mark: a mark five pixels wide is
    /// not something a hand lands on.
    #[test]
    fn a_handle_is_easier_to_take_hold_of_than_it_is_wide() {
        let band = ramp(bars(full_panel()));
        let at = band.x + 100.0;
        let grip = grip(band, at);
        assert!(grip.width > HANDLE_WIDTH);
        assert_eq!(
            grip.x + grip.width / 2.0,
            at,
            "centered on the value it marks"
        );
        assert!(grip.y < band.y && grip.bottom() > band.bottom());
        // Above the band it stays clear of the plot's ground, and below it
        // clear of whatever comes next: the false colors, or the rows.
        let plot = bars(full_panel());
        assert!(
            grip.y >= plot.bottom() + PLOT_INSET,
            "{grip:?} over {plot:?}"
        );
        assert!(grip.bottom() <= swatch_button(plot, 0).y);
        assert!(grip.bottom() <= Rows::new(full_panel()).exposure.y);
    }

    /// A control the display would ignore is not on the panel at all, and the
    /// ones that remain close the gap up rather than leaving a hole where it
    /// would have been.
    #[test]
    fn a_control_that_could_do_nothing_is_not_there() {
        let panel = full_panel();

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

    /// The rows sit under the band, inside the panel, and land in the same
    /// place whether or not the image has a row of false colors — the band
    /// and the swatches under it end on the same line, so the controls
    /// below them do not shift about from one file to the next. The last
    /// of the three rows ends the panel.
    #[test]
    fn the_rows_sit_under_the_band_whatever_it_ends_in() {
        let panel = full_panel();
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
        // The last row is the last thing on the panel, and what it
        // leaves under it is the panel's own inset and nothing more.
        assert_eq!(rows.bottom(), inside.bottom());
        let labels: Vec<&str> = rows.labels().map(|(word, _)| word).collect();
        assert_eq!(labels, ["Exposure", "Window", "Curve"]);
    }

    /// The windows and the curves cover everything there is to choose, and
    /// the exposure's row is a slider up to its reading, which ends where
    /// the rows' last buttons do.
    #[test]
    fn the_rows_offer_every_choice_there_is() {
        let panel = full_panel();
        let widgets: Vec<Control> = row_buttons(panel).map(|(widget, _)| widget).collect();
        assert_eq!(widgets.len(), WINDOWS.len() + ToneMap::ALL.len());

        let rows = Rows::new(panel);
        let inside = panel.inset(PANEL_INSET, PANEL_INSET);
        assert_eq!(rows.slider().x, rows.exposure.x);
        assert!(rows.slider().right() < rows.stops().x);
        assert_eq!(rows.stops().right(), inside.right());
        assert!(
            rows.slider().width - HANDLE_GRIP > 3.0 * 2.0 * SLIDER_STOPS / EV_STEP,
            "a quarter stop is wider than a few pixels"
        );

        // The three rules, and no fourth for the image's own, which is
        // always one of these.
        let named: Vec<AutoWindow> = WINDOWS.iter().map(|(_, window)| *window).collect();
        assert_eq!(
            named,
            [AutoWindow::Off, AutoWindow::MinMax, AutoWindow::Percentile]
        );
        assert_eq!(ToneMap::ALL.len(), 2, "one button to a choice");
    }

    /// The readout is set in the middle of the line, and the ends of the axis
    /// keep their corners until it actually reaches them — at which point
    /// both go, rather than one.
    #[test]
    fn the_axis_ends_give_the_line_up_to_the_readout_and_not_before() {
        let bars = bars(full_panel());
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
