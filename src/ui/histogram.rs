//! The floating histogram panel.

use egui::{
    Align2, Color32, CursorIcon, FontId, Sense, Stroke, WidgetInfo, WidgetType, pos2, vec2,
};

use crate::image::Referred;
use crate::image::display::{AutoWindow, Colormap, Display, ToneMap};
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
    capitalized,
};

/// Which of the rows of settings under the band a file gets.
///
/// The exposure is every file's: pushing a picture two stops up is how you
/// find out whether a shadow is empty or merely dark, whatever the file. The
/// other two are offered where they answer a question the file raises and
/// left off where they do not, since a row of buttons that would only make
/// a picture worse is a row the panel has to explain.
///
/// Settled from the file alone rather than from what has been done to it,
/// so that the panel is one height for the whole of a file's stay on
/// screen: the information column starts under it, and a column that
/// jumped every time a handle was dragged would be a column no one could
/// read while dragging.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Offered {
    /// The three windows named outright. Linear data has no white of its
    /// own, so where the useful range is has to be found in the pixels, and
    /// these are the three rules for finding it. A graded file's window is
    /// 0..1 and nothing else, and offering it two ways to be wrong is not
    /// a kindness.
    pub window: bool,
    /// The curves. A curve exists to fit values above white into a surface
    /// that stops there, so it is offered where the file has any — graded
    /// light with headroom in it, or measured light with outliers past the
    /// window — and not under an SDR photograph, which never does.
    pub curve: bool,
}

impl Offered {
    /// Every row there is: what the tallest panel holds, and what the window
    /// has to have room for.
    pub const ALL: Offered = Offered {
        window: true,
        curve: true,
    };

    /// What `current` gets, or every row where there is no file yet: the
    /// panel is not drawn without one, and the conservative answer is the
    /// one the window's own floor is measured against.
    pub fn of(current: Option<&Current>) -> Self {
        current.map_or(Self::ALL, Self::for_file)
    }

    pub fn for_file(current: &Current) -> Self {
        Self {
            window: current.image.referred == Referred::Scene,
            curve: Display::opens_above_white(&current.image, &current.stats),
        }
    }

    /// What the rows take off the panel's height: each row and the gap
    /// above it, under the plot's own inset — which the band hangs below
    /// rather than inside, so it is what is left over between the two and
    /// belongs to whatever comes next.
    const fn rows_height(self) -> f32 {
        let mut height = PLOT_INSET + ROW_GAP + ROW_HEIGHT;
        if self.window {
            height += ROW_GAP + ROW_HEIGHT;
        }
        if self.curve {
            height += ROW_GAP + ROW_HEIGHT;
        }
        height
    }
}

/// The panel: as wide as anything else floating over the content area, and
/// tall enough for the line the readout is set on, a plot, the band of what
/// the display makes of its axis, the row of false colors a gray image has
/// under that, and the rows of settings this file is offered — see
/// [`Offered`].
pub const fn size(offered: Offered) -> [f32; 2] {
    [
        PANEL_WIDTH,
        2.0 * PANEL_INSET
            + LABEL_HEIGHT
            + PLOT_INSET
            + PLOT_HEIGHT
            + RAMP_GAP
            + SWATCH_HEIGHT
            + RAMP_GAP
            + RAMP_HEIGHT
            + offered.rows_height(),
    ]
}

/// The panel with every row on it, which is what a window has to have room
/// for before either toggle is lit — see [`super::PANELS_ROOM`].
pub(super) const TALLEST: [f32; 2] = size(Offered::ALL);

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

/// A quarter of a stop: what one press of the exposure row is worth, and what
/// the keys that do the same job step by.
///
/// Held here, where the buttons that carry the number are drawn, and read by
/// the key table from here: the two are one step, so a press and a keystroke
/// move the exposure by the same amount and the label on the button cannot
/// come to disagree with what pressing it does.
pub const EV_STEP: f32 = 0.25;

/// How far the exposure's own number has to be dragged for one step of it.
/// A drag is the same quarter stops a press is, so many as the hand has
/// covered — see [`rows`] — and this is how much hand each is worth.
const EXPOSURE_DRAG: f32 = 10.0;

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
/// one on. Only a scene-referred file has the row — see [`Offered`] — and
/// such a file's own window is the trimmed one, so there is no fourth
/// button for "the image's own": it would be the third one twice.
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

/// How wide one of the exposure row's two steps is, and how much is set aside
/// in front of them for the exposure itself.
///
/// The reading leads and the buttons follow it, close enough to touch: what a
/// step does is change the number in front of it, and a number a button's
/// width away from the button that moves it is a number that has to be looked
/// for. The room the group does not take is left at the end of the line,
/// where it is margin.
const STEP_WIDTH: f32 = 54.0;
const STOPS_WIDTH: f32 = 36.0;

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
/// short for it. The panel is one size for a file — the plot's bins are a
/// logical pixel each and the rows under it are set to what they say, so
/// there is nothing here to give — and a panel drawn larger than the area
/// it floats over would cover the picture it is about and run off the
/// window besides.
///
/// Public because the pointer is tested against the whole panel from outside
/// the frame: what lands on it belongs to it, and must not reach the picture
/// it is floating over.
pub fn panel(content: Rect, offered: Offered) -> Option<Rect> {
    let size = size(offered);
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
/// the next. What does change from file to file is which rows there are,
/// which is [`Offered`]'s to say; a row not offered is `None` here, and the
/// rows below it close up.
#[derive(Clone, Copy)]
struct Rows {
    /// The exposure's line, and where its word goes: its reading at the
    /// head and its two steps after it.
    exposure: Rect,
    exposure_label: Rect,
    /// The three windows on offer, where they are offered.
    window: Option<(Rect, Rect)>,
    /// And the three curves.
    curve: Option<(Rect, Rect)>,
}

impl Rows {
    fn new(panel: Rect, offered: Offered) -> Self {
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
        let window = offered.window.then(&mut next);
        let curve = offered.curve.then(&mut next);
        Self {
            exposure,
            exposure_label,
            window,
            curve,
        }
    }

    /// The exposure itself, at the head of its row: the number the two steps
    /// beside it act on, and which a drag along it moves by the same steps.
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

    /// Each row's word and where it goes, in the order they are stacked.
    fn labels(&self) -> impl Iterator<Item = (&'static str, Rect)> {
        [
            Some(("Exposure", self.exposure_label)),
            self.window.map(|(_, label)| ("Window", label)),
            self.curve.map(|(_, label)| ("Highlights", label)),
        ]
        .into_iter()
        .flatten()
    }

    /// The bottom of the lowest row: where the block, and the panel, end.
    #[cfg(test)]
    fn bottom(&self) -> f32 {
        [
            Some(self.exposure),
            self.window.map(|(row, _)| row),
            self.curve.map(|(row, _)| row),
        ]
        .into_iter()
        .flatten()
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
fn row_buttons(panel: Rect, offered: Offered) -> impl Iterator<Item = (Control, Rect)> {
    let rows = Rows::new(panel, offered);
    let steps = [
        (Control::ExposureDown, rows.step(false)),
        (Control::ExposureUp, rows.step(true)),
    ];
    let windows = rows.window.into_iter().flat_map(|(row, _)| {
        (0..WINDOWS.len())
            .map(move |index| (Control::Window(index), share(row, WINDOWS.len(), index)))
    });
    let curves = rows.curve.into_iter().flat_map(|(row, _)| {
        (0..ToneMap::ALL.len())
            .map(move |index| (Control::Curve(index), share(row, ToneMap::ALL.len(), index)))
    });
    steps.into_iter().chain(windows).chain(curves)
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
    let panel = panel(content, Offered::for_file(current))?;
    let plot = plot_area(panel, current.image.is_gray());
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
    let offered = Offered::for_file(current);
    let Some(panel) = panel(content, offered) else {
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
            let held = controls(pass, ui, current, panel, offered);
            header(pass, ui, current, panel, held);
        });
}

/// The plot itself: the bins, the pointer's rule, the shares clipped at
/// either end, the band along the foot and — where the display is doing
/// anything — the response curve over the lot.
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
    let transfer = current.image.color.transfer;
    let headroom = input.headroom;

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
    // Full height, where the window's own marks are handles on the band. A
    // rule standing through the plot is what a pointer wants and what a
    // permanent annotation does not: this one is only there while it is
    // being aimed, and it has to be followed up from the axis to the curve.
    let marked = marked(current, content, input.cursor, input.pointer);
    if let Some(across) = marked.map(bin_across) {
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

    if span <= 0.0 {
        return;
    }

    // How much of the picture the window is throwing away, in the corner
    // it is being thrown out of: the share of the pixels at or below what
    // comes out black in the left corner, and at or above what comes out
    // white in the right — where the surface is actually clipping them,
    // rather than showing them or rolling them off. Only where there is a
    // share to write, so that a corner with a number in it is news.
    //
    // This is the question the panel is most often opened to answer, and a
    // spike against the edge of the plot cannot answer it: the spike says
    // there is clipping and the number says how much.
    let (black, white) = current.display.displayed_bounds();
    let [below, above] = current
        .stats
        .plot
        .clipped(transfer.to_encoded(black), transfer.to_encoded(white));
    let above = if current.display.clips_white(gray, headroom) {
        above
    } else {
        0.0
    };
    let clip_font = FontId::proportional(CLIP_TEXT);
    for (share, right) in [(below, false), (above, true)] {
        let Some(words) = share_words(share) else {
            continue;
        };
        let width = width_of(ui, &words, CLIP_TEXT);
        let x = if right {
            bars.right() - CLIP_INSET - width
        } else {
            bars.x + CLIP_INSET
        };
        let text = Rect::new(x, bars.y + CLIP_INSET, width, CLIP_TEXT);
        painter.rect_filled(
            area(text.inset(-CLIP_PAD, -CLIP_PAD)),
            CLIP_PAD,
            theme.plot_background.with_alpha(CLIP_BACKING_ALPHA),
        );
        painter.text(
            pos2(text.x, text.y),
            Align2::LEFT_TOP,
            words,
            clip_font.clone(),
            theme.accent.into(),
        );
    }

    // What the display turns each value into, in a band along the foot of
    // the plot: the bin above a cell, and the color it comes out as under it.
    //
    // The curve says how much and this says what of, which are different
    // questions on a false-colored image — a curve cannot draw viridis —
    // and the same question answered twice on a gray one, where the band is
    // the tone curve as a wedge and the curve is it as a shape. It is where
    // clipping stops being an inference: everything left of the window
    // comes out black and everything right of it comes out at the top of
    // the ramp, so the two flat runs at the ends are the range the display
    // is throwing away, drawn at the width they occupy. The handles that
    // set those ends are drawn over it, in [`track`].
    let band = ramp(bars);
    let (top, bottom) = (snap(band.y), snap(band.bottom()));
    // A cell above white — which only a surface with room above white has,
    // and only with no curve on — is drawn white, since the panel cannot
    // glow, with the accent along its top edge to say that the screen does:
    // the same ink as the handle that marks white on the band, and the run
    // of it is how much of the axis is out past that.
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

    // What the display is doing to the values underneath, drawn over them —
    // and only where it is doing something. On a display with nothing asked
    // of it the curve is the diagonal from corner to corner, which says
    // nothing the axis under it does not, and a line across every plot is a
    // line no one looks at; drawn only when it bends, or leans, it is news.
    //
    // The curve is the whole of what the display does, and the only part of
    // the panel that can show a tone map at all: a shoulder is a shape, not
    // a threshold, and there is no line that means "reinhard". The handles
    // on the band place the two values that come out black and white,
    // exposure included, which a curve meeting its floor tangentially
    // cannot be read for by eye.
    if current.display.is_identity() {
        return;
    }
    // Sampled per column rather than per bin: the response is a continuous
    // function of the value, and stepping it where the transform does not
    // step would draw a stair that is not there.
    //
    // The same one check for every column, the arithmetic being the same
    // for all of them: a window left non-finite would otherwise put NaN
    // vertices in the buffer, which no clamp downstream can undo.
    let (offset, gain) = current.display.transform();
    if !(offset.is_finite() && gain.is_finite()) {
        return;
    }
    let columns = bars.width.max(1.0) as usize;
    // Decoded to run the transform on, then encoded again to be drawn: both
    // axes are in the file's own units, so a display doing nothing would be
    // the diagonal.
    let responses: Vec<f32> = (0..=columns)
        .map(|column| {
            let across = column as f32 / columns as f32;
            let value = transfer.to_linear(axis_min + across * span);
            let response = current.display.response(value, headroom).max(0.0);
            transfer.to_encoded(response).max(0.0)
        })
        .collect();
    // The plot's height is white, unless the response runs past it — a
    // surface with room above white, and no curve on — in which case the
    // top is wherever the response gets to and white is a line across the
    // plot, so that the room above it can be seen as the room it is rather
    // than as a clip that is not happening.
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

/// The line above the plot: what the pointer is reading, in the middle, and
/// the two ends of the axis at its ends where they are worth writing.
///
/// The middle is the words the hand on the band asked for, where it is on
/// one — `held`, which [`track`] hands back — and otherwise the bin under
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
/// always has. A photograph's plot runs from black to white, which every
/// histogram of a photograph does and no one needs told; a linear file's
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
        let mapped = current.display.response(value, input.headroom).max(0.0);
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

/// The strip of buttons down the left of the panel, the handles on the
/// band, the rows under it, and the row of false colors under its ramp.
/// Hands back what the hand on the band wants written above the plot.
///
/// Drawn here rather than with the chrome's toggles because these belong to
/// the panel: they say what the plot beside them is showing and what the band
/// beneath them is painted with, and two of them are pictures of the very
/// thing they switch.
fn controls(
    pass: &mut Pass,
    ui: &mut egui::Ui,
    current: &Current,
    panel: Rect,
    offered: Offered,
) -> Option<String> {
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

    let held = track(pass, ui, current, bars);
    rows(pass, ui, current, panel, offered);

    // The false colors, each showing itself, and only where the display
    // would act on the choice. The whole ramp rather than one color off it:
    // a map is a sequence, and a single swatch of viridis is a green
    // rectangle that could be anything.
    if !gray {
        return held;
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
    held
}

/// The band under the plot as the levels track it is: a handle at the value
/// that comes out black and another at the value that comes out white, each
/// dragged to where it should stand, and the stretch of band between them,
/// which drags the window along the axis without changing its width. What
/// every levels tool in every editor looks like, so that no one has to be
/// told what the two handles do.
///
/// Hands back what to write above the plot while the hand is on it: the
/// value under the handle, or the two the band runs between. Not a tooltip,
/// which the toolkit takes down for the length of a drag, and a drag is
/// exactly when the number is wanted.
///
/// The handles set the values that come out black and white — exposure
/// included, since those are the two ends of the band's black run and its
/// white run — and [`Display::set_displayed_bounds`] works the window back
/// from them. A handle is dragged to the pointer rather than by it, so a
/// drag has no memory to lose: wherever the pointer is along the axis is
/// where the handle goes, and a hand that runs off the end of the band puts
/// the handle at the end.
///
/// The window can end past what is plotted, which a few stops of exposure
/// is enough to do; such a handle is drawn hollow at the edge it went out
/// of, so that it can be taken hold of and brought back, and so that it
/// does not claim a boundary the curve running on past it says is not
/// there.
fn track(pass: &mut Pass, ui: &mut egui::Ui, current: &Current, bars: Rect) -> Option<String> {
    let theme = pass.theme;
    let scale = pass.input.scale;
    let band = ramp(bars);
    let plotted = &current.stats.plot;
    let (axis_min, axis_max) = (plotted.min, plotted.max);
    let span = axis_max - axis_min;
    if span <= 0.0 {
        return None;
    }
    let transfer = current.image.color.transfer;
    let display = &current.display;
    let (black, white) = display.displayed_bounds();
    if !(black.is_finite() && white.is_finite()) {
        return None;
    }

    // Along the band from 0 at its left end to 1 at its right, which is the
    // plot's own axis: encoded, so that a handle stands under the bin its
    // value was counted in.
    let along = |value: f32| (transfer.to_encoded(value) - axis_min) / span;
    let at = |t: f32| bars.x + t.clamp(0.0, 1.0) * bars.width;
    let value_at = |x: f32| {
        let t = ((x - bars.x) / bars.width).clamp(0.0, 1.0);
        transfer.to_linear(axis_min + t * span)
    };
    // The least a window can be: a bin, so that the two handles cannot be
    // dragged through each other into a window the shader would divide by.
    let least = span / BINS as f32;
    let (black_t, white_t) = (along(black), along(white));

    let id = ui.id().with("levels");
    // The stretch between the handles first and the handles after, so that
    // where they overlap it is the handle that is under the pointer.
    let between = ui.interact(area(band), id.with("window"), Sense::DRAG);
    let black_handle = ui.interact(area(grip(band, at(black_t))), id.with("black"), Sense::DRAG);
    let white_handle = ui.interact(area(grip(band, at(white_t))), id.with("white"), Sense::DRAG);
    between.widget_info(|| WidgetInfo::labeled(WidgetType::Other, true, "Window"));
    black_handle.widget_info(|| WidgetInfo::labeled(WidgetType::Other, true, "Black point"));
    white_handle.widget_info(|| WidgetInfo::labeled(WidgetType::Other, true, "White point"));

    let mut said = None;
    if black_handle.dragged()
        && let Some(pointer) = black_handle.interact_pointer_pos()
    {
        let ceiling = transfer.to_linear(transfer.to_encoded(white) - least);
        let black = value_at(pointer.x).min(ceiling);
        pass.commands.push(Command::Levels { black, white });
    } else if white_handle.dragged()
        && let Some(pointer) = white_handle.interact_pointer_pos()
    {
        let floor = transfer.to_linear(transfer.to_encoded(black) + least);
        let white = value_at(pointer.x).max(floor);
        pass.commands.push(Command::Levels { black, white });
    } else if between.dragged() {
        // Along the axis by what the hand moved, in the axis's own units,
        // both ends together: the width of the window is the handles'
        // business, and the band's is where it is.
        let moved = between.drag_delta().x / bars.width * span;
        if moved != 0.0 {
            let slid = |value: f32| transfer.to_linear(transfer.to_encoded(value) + moved);
            pass.commands.push(Command::Levels {
                black: slid(black),
                white: slid(white),
            });
        }
    }
    let on_black = black_handle.hovered() || black_handle.dragged();
    let on_white = white_handle.hovered() || white_handle.dragged();
    let on_band = between.hovered() || between.dragged();
    if on_black {
        said = Some(format!(
            "Black at {}",
            axis_words(current, transfer.to_encoded(black))
        ));
    } else if on_white {
        said = Some(format!(
            "White at {}",
            axis_words(current, transfer.to_encoded(white))
        ));
    } else if on_band {
        said = Some(format!(
            "{} \u{2013} {}",
            axis_words(current, transfer.to_encoded(black)),
            axis_words(current, transfer.to_encoded(white))
        ));
    }
    if on_black || on_white {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    } else if between.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    } else if on_band {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
    }
    pass.tooltip(between, Tip::Window, true);
    pass.tooltip(black_handle, Tip::BlackPoint, true);
    pass.tooltip(white_handle, Tip::WhitePoint, true);

    // The handles themselves, over the band and standing up past it, in the
    // accent every mark on the plot wears, ringed in the panel's ground so
    // that one stays a shape against a band that has come round to the same
    // color. Snapped to the device's grid, as the ticks they replace were.
    let painter = ui.painter();
    let grid = icon::Grid::new(scale);
    for (t, on) in [(black_t, on_black), (white_t, on_white)] {
        let mark = on_device(
            Rect::new(
                at(t) - HANDLE_WIDTH / 2.0,
                band.y - HANDLE_REACH,
                HANDLE_WIDTH,
                band.height + 2.0 * HANDLE_REACH,
            ),
            scale,
        );
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
    said
}

/// The rows under the band: the exposure, and — where the file is offered
/// them — the window and the curve.
///
/// What the plot draws, said in words and set: the handles on the band are
/// the window, the curve over the bins is the curve, and the gain that moves
/// them both is the exposure. A reading and the buttons that change it,
/// together, so that a number on this panel is never one you have to go
/// somewhere else to act on — and the exposure's own number is a thing to
/// drag as well as to read, by the same quarter stops the buttons beside it
/// press.
///
/// The curves light the one that is in force; the windows do not. A window
/// is set from the pixels and then moved by hand — a handle, a key, the
/// exposure under it — and a button lit for "full range" on a window that
/// has since been shifted would be claiming something that stopped being
/// true. The handles are what say where the window is.
fn rows(pass: &mut Pass, ui: &mut egui::Ui, current: &Current, panel: Rect, offered: Offered) {
    let theme = pass.theme;
    let rows = Rows::new(panel, offered);
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

    // The exposure at the head of its row, in the units the rest of the
    // interface quotes it in: stops counted in quarters, as the bar and the
    // keys count them. A drag along it is those same quarters, one for every
    // [`EXPOSURE_DRAG`] the hand has covered, with what is left over carried
    // to the next frame so that a slow hand still gets there — pressed
    // through the same two controls the buttons press, so that a drag and a
    // keystroke cannot be worth different amounts.
    let stops = rows.stops();
    let dragged = ui.interact(area(stops), ui.id().with("exposure"), Sense::DRAG);
    dragged.widget_info(|| WidgetInfo::labeled(WidgetType::Other, true, "Exposure"));
    let carry = dragged.id.with("carry");
    if dragged.dragged() {
        let carried =
            ui.data(|data| data.get_temp::<f32>(carry).unwrap_or(0.0)) + dragged.drag_delta().x;
        let steps = (carried / EXPOSURE_DRAG).trunc();
        ui.data_mut(|data| data.insert_temp(carry, carried - steps * EXPOSURE_DRAG));
        let step = if steps > 0.0 {
            Control::ExposureUp
        } else {
            Control::ExposureDown
        };
        for _ in 0..steps.abs() as usize {
            pass.press(step);
        }
    } else if dragged.drag_stopped() {
        ui.data_mut(|data| data.remove_temp::<f32>(carry));
    }
    if dragged.hovered() || dragged.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
    pass.tooltip(dragged, Tip::Exposure, true);
    ui.painter().text(
        pos2(grid.snap(stops.x), stops.y + stops.height / 2.0),
        Align2::LEFT_CENTER,
        stops_label(display.exposure_stops),
        font.clone(),
        theme.text_primary.into(),
    );

    for (widget, rect) in row_buttons(panel, offered) {
        let Some((label, active)) = row_label(widget, display) else {
            continue;
        };
        let (_, _, ink) = button(pass, ui, rect, widget, active, TOGGLE_RADIUS);
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

    /// Every combination of rows a file can be offered.
    const EVERY_OFFER: [Offered; 4] = [
        Offered::ALL,
        Offered {
            window: false,
            curve: false,
        },
        Offered {
            window: true,
            curve: false,
        },
        Offered {
            window: false,
            curve: true,
        },
    ];

    /// A content area with room for everything.
    fn content() -> Rect {
        Rect::new(0.0, 0.0, 800.0, 600.0)
    }

    fn full_panel() -> Rect {
        panel(content(), Offered::ALL).expect("room")
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

        let photograph = shown(DecodedImage::new(
            2,
            1,
            Samples::U8 {
                channels: Channels::Gray,
                data: vec![0, 255],
            },
            ColorSpace::SRGB,
            AlphaMode::Opaque,
        ));
        assert_eq!(axis_words(&photograph, 1.0), "1");
        assert_eq!(axis_words(&photograph, 0.0), "0");

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

    /// The rows a file is offered are the ones that answer a question it
    /// raises: an SDR photograph gets the exposure alone, linear data gets
    /// the windows, and anything with highlights above white gets the
    /// curves as well.
    #[test]
    fn a_file_is_offered_the_rows_it_has_a_use_for() {
        let photograph = shown(DecodedImage::new(
            2,
            1,
            Samples::U8 {
                channels: Channels::Rgb,
                data: vec![0, 0, 0, 255, 255, 255],
            },
            ColorSpace::SRGB,
            AlphaMode::Opaque,
        ));
        assert_eq!(
            Offered::for_file(&photograph),
            Offered {
                window: false,
                curve: false
            }
        );

        // Linear samples with one far above the rest: the trimmed window
        // leaves it out, above white.
        let mut samples = vec![0.5; 2000];
        samples[3] = 50.0;
        let render = shown(DecodedImage::new(
            2000,
            1,
            Samples::F32 {
                channels: Channels::Gray,
                data: samples,
            },
            ColorSpace::LINEAR_BT709,
            AlphaMode::Opaque,
        ));
        assert_eq!(Offered::for_file(&render), Offered::ALL);

        // Linear samples with nothing above the window: spread evenly, so
        // that the trimmed window's top is the brightest of them.
        let flat = shown(DecodedImage::new(
            1000,
            1,
            Samples::F32 {
                channels: Channels::Gray,
                data: (0..1000).map(|i| i as f32 / 999.0).collect(),
            },
            ColorSpace::LINEAR_BT709,
            AlphaMode::Opaque,
        ));
        assert_eq!(
            Offered::for_file(&flat),
            Offered {
                window: true,
                curve: false
            }
        );

        // Nothing on screen: every row, which is the panel the window has to
        // have room for.
        assert_eq!(Offered::of(None), Offered::ALL);
        assert_eq!(Offered::of(Some(&flat)), Offered::for_file(&flat));
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

    /// The panel is one fixed size for a file, so a content area smaller
    /// than it in either direction gets no panel at all rather than one
    /// hanging off the window over the picture it is about. A file with
    /// fewer rows wants less room, and gets its panel in a window the
    /// tallest would not.
    #[test]
    fn a_content_area_too_small_gets_no_panel() {
        for offered in EVERY_OFFER {
            let size = size(offered);
            let room = [size[0] + 2.0 * PADDING, size[1] + 2.0 * PADDING];
            assert!(panel(Rect::new(0.0, 0.0, room[0], room[1]), offered).is_some());
            assert!(panel(Rect::new(0.0, 0.0, room[0] - 1.0, room[1]), offered).is_none());
            assert!(panel(Rect::new(0.0, 0.0, room[0], room[1] - 1.0), offered).is_none());
        }
        let least = size(Offered {
            window: false,
            curve: false,
        });
        assert!(least[1] < TALLEST[1]);
        assert_eq!(size(Offered::ALL), TALLEST);
        let short = Rect::new(0.0, 0.0, 800.0, least[1] + 2.0 * PADDING);
        assert!(panel(short, Offered::ALL).is_none());
        assert!(
            panel(
                short,
                Offered {
                    window: false,
                    curve: false
                }
            )
            .is_some()
        );
    }

    /// Where it does fit it sits in the top right of the content area, its
    /// own padding in from both edges.
    #[test]
    fn the_panel_sits_in_the_corner_with_its_padding_around_it() {
        let content = Rect::new(10.0, 20.0, 800.0, 600.0);
        let panel = panel(content, Offered::ALL).expect("room");
        assert_eq!(panel.right(), content.right() - PADDING);
        assert_eq!(panel.y, content.y + PADDING);
    }

    /// The panel grew a strip of buttons and a row of ramps around the plot,
    /// and the plot itself did not move across: a bin is one logical pixel,
    /// which is what keeps the bars from landing astride a pixel boundary.
    /// Nor does it move down for the rows under it, which is what the panel
    /// grows by.
    #[test]
    fn the_plot_keeps_one_pixel_to_the_bin_whatever_grows_around_it() {
        for offered in EVERY_OFFER {
            let panel = panel(content(), offered).expect("room");
            for gray in [true, false] {
                let bars = plot_area(panel, gray);
                assert_eq!(bars.width, BINS as f32, "gray {gray}");
                assert_eq!(bars.y, plot_area(full_panel(), gray).y, "{offered:?}");

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
        assert!(grip.bottom() <= Rows::new(full_panel(), Offered::ALL).exposure.y);
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
    /// below them do not shift about from one file to the next. Whatever
    /// rows a file is offered, the last of them ends the panel.
    #[test]
    fn the_rows_sit_under_the_band_whatever_it_ends_in() {
        for offered in EVERY_OFFER {
            let panel = panel(content(), offered).expect("room");
            let rows = Rows::new(panel, offered);
            let inside = panel.inset(PANEL_INSET, PANEL_INSET);

            let lowest = swatch_button(plot_area(panel, true), 0).bottom();
            assert_eq!(
                lowest,
                ramp(bars(panel)).bottom(),
                "the false colors end where the band alone would have"
            );
            for (widget, rect) in row_buttons(panel, offered) {
                assert!(rect.y >= lowest, "{widget:?} clears the band: {rect:?}");
                assert!(rect.bottom() <= inside.bottom(), "{widget:?} {rect:?}");
                assert!(rect.x >= inside.x + ROW_LABEL, "{widget:?} clears its word");
                assert!(rect.right() <= inside.right() + 0.01, "{widget:?} {rect:?}");
            }
            // The last row is the last thing on the panel, and what it
            // leaves under it is the panel's own inset and nothing more.
            assert_eq!(rows.bottom(), inside.bottom(), "{offered:?}");
            let labels: Vec<&str> = rows.labels().map(|(word, _)| word).collect();
            let mut expected = vec!["Exposure"];
            if offered.window {
                expected.push("Window");
            }
            if offered.curve {
                expected.push("Highlights");
            }
            assert_eq!(labels, expected);
        }
    }

    /// The exposure's two steps are worth what they say they are worth, and
    /// the windows and the curves cover everything there is to choose —
    /// where they are offered at all.
    #[test]
    fn the_rows_offer_every_choice_there_is() {
        for offered in EVERY_OFFER {
            let panel = panel(content(), offered).expect("room");
            let widgets: Vec<Control> = row_buttons(panel, offered)
                .map(|(widget, _)| widget)
                .collect();
            assert_eq!(widgets[0], Control::ExposureDown);
            assert_eq!(widgets[1], Control::ExposureUp);
            let windows = if offered.window { WINDOWS.len() } else { 0 };
            let curves = if offered.curve { ToneMap::ALL.len() } else { 0 };
            assert_eq!(widgets.len(), 2 + windows + curves, "{offered:?}");
        }

        // The three rules, and no fourth for the image's own: the row is
        // only offered to a file whose own is the third.
        let named: Vec<AutoWindow> = WINDOWS.iter().map(|(_, window)| *window).collect();
        assert_eq!(
            named,
            [AutoWindow::Off, AutoWindow::MinMax, AutoWindow::Percentile]
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
