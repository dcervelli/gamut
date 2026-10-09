//! The histogram, down the side panel: the picture as a pipeline, one
//! section to a stage, in the order the data flows read from the screen
//! back — what goes out to the screen at the top, the display that makes it
//! under that, the gain map that lifts the picture where it has one, and
//! the file as it stores it at the foot. Each section is a plot of the
//! picture at its stage and a header naming the stage, and the pixel under
//! the pointer is traced down the column, a rule on each plot and its value
//! at that stage in each header.
//!
//! This file is the column: the sections in their order, what the panel is
//! laid out in, the strip of buttons beside the head plot, and the trace.
//! `section` is a section's height and header, `plot` the plot every
//! section draws, `output`, `display`, `gain_map` and `file` the four
//! stages, the band and its handles `track` and the exposure's slider
//! `slider`, each a file beside this one.

mod display;
mod file;
mod gain_map;
mod output;
mod plot;
mod section;
mod slider;
mod track;

use plot::{HISTOGRAM_LUMA, HISTOGRAM_PLANES, bin_across};

use egui::{Color32, FontId, Sense, WidgetInfo, WidgetType, pos2};

use crate::image::display::{AutoWindow, EV_STEP};
use crate::image::stats::{BINS, Plot};
use crate::render::Color;

use super::Rect;

use super::chrome::{BUTTON_SIZE, ICON_SIDE, Pass};
use super::icon;
use super::style::TOGGLE_RADIUS;
use super::tooltip::Tip;
use super::{BECOMES, Control, Current, PANEL_INSET, PANEL_WIDTH, TEXT_SIZE};

pub use section::Section;

/// The least room the panel is useful in: the head section, which is what
/// goes out to the screen, and the panel's insets around it. What a window
/// has to have room for before the toggle is alive — see
/// [`super::PANELS_ROOM`]. The rest of the column scrolls under it.
pub const MIN_HEIGHT: f32 = 2.0 * PANEL_INSET + Section::Output.height();

/// The room the strip of buttons down the left takes: a button's width and
/// the gap between it and the plot. Read by [`super::PANEL_WIDTH`], which is
/// this and the plot, so that a bin stays exactly one point wide.
pub(super) const TOOLBAR_WIDTH: f32 = BUTTON_SIZE + PANEL_INSET;
/// Between one button of that strip and the next. Tighter than the gap the
/// chrome's own strips keep, so that on a color image the four buttons
/// beside the head plot leave a gap between the last of them and the one
/// beside its band — see [`output::marks_button`].
const TOOLBAR_GAP: f32 = 6.0;

/// The middle of the grid a mark is described on, and the two measures the
/// plane toggles are drawn from, in that grid's units: how far each color
/// disc is struck from the middle, and how big the discs are. The luminance
/// disc is one mark where the colors are three, so it is the larger.
const GRID_MIDDLE: f32 = 12.0;
const LUMA_DISC: f32 = 8.0;
const PLANE_ORBIT: f32 = 5.5;
const PLANE_DISC: f32 = 4.0;

/// The line a section opens with: its name, and what the pointer is
/// reading at its stage. A whole number of pixels, as everything a
/// section's height is summed from is, so that each section starts on one.
const LABEL_HEIGHT: f32 = 18.0;
/// How tall the plot's ground stands in the two sections whose plots are
/// read closely — what goes out, and what the display makes of it — on an
/// image with a row of false colors under the band. The row takes its room
/// off the plot rather than off the section — see [`display::plot_area`] —
/// so a color image's plot is this and the row besides, and the head
/// section's plot is that height for every file, which is what the strip
/// of four buttons beside it needs.
const PLOT_HEIGHT: f32 = 88.0;
/// And in the two whose plots are context — the gain map's lift and the
/// file as stored — which are read for their shape against the plots
/// above them, and so can be shorter.
const SHORT_PLOT_HEIGHT: f32 = 56.0;

/// The corner radius of the plot's own ground inside the panel. Smaller than
/// the panel's, the way an inner corner always is.
const PLOT_RADIUS: f32 = 3.0;
/// The room left around that ground, so the plot reads as set into the panel
/// rather than as a hole cut in it.
const PLOT_INSET: f32 = 4.0;
/// Between one section and the next.
const SECTION_GAP: f32 = 8.0;

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

/// What is written in the two top corners of a plot — the share of the
/// picture clipped at either end, or the ends of the axis — what it is set
/// at, how far in from the corner, and what is left around it inside the
/// ground it is backed with: the plot's own, laid over whatever bar has
/// climbed into the corner, so the number stays a number on a picture that
/// has piled up at one end.
const CLIP_TEXT: f32 = TEXT_SIZE * 0.75;
const CLIP_INSET: f32 = 3.0;
const CLIP_PAD: f32 = 2.0;
const CLIP_BACKING_ALPHA: u8 = 215;

/// The band under a plot: how deep the color is, and how far it stands off
/// the plot's ground. Deep enough to read a color off and to take hold of,
/// and no deeper — it is a legend along the axis, not a second plot.
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

/// Between one cell of a row and the next: the false colors under the band,
/// and the buttons of the rows below them.
const CELL_GAP: f32 = 4.0;

/// What the words on the panel are set at: the headers and the rows. Small
/// enough that the longest button label has room in the narrowest cell.
const ROW_TEXT: f32 = TEXT_SIZE * 0.85;

/// The word set beside the line that marks white, when white is not the top
/// of the plot.
const WHITE_LABEL: &str = "white";

/// How far past white a value has to reach before it is marked as beyond it:
/// a hair, so that the window's own top does not count.
const ABOVE_WHITE: f32 = 1.0 + 1e-3;

/// The whole of the side panel `side`: the column the sections are laid
/// out down, which scrolls where the side is shorter than they are.
pub fn panel(side: Rect) -> Rect {
    Rect::new(side.x, side.y, PANEL_WIDTH, side.height)
}

/// The buttons down the left that act on every plot, in the order they are
/// stacked beside the head one. The one beside its band is not among them —
/// see [`output::marks_button`].
///
/// A control that could not act is left out rather than drawn dead: an image
/// with one channel has no color planes to toggle, and the two that remain
/// close the gap up. Hiding rather than dimming is the panel's rule for both
/// of these — see [`display::swatch_button`] for the other one.
fn toolbar(gray: bool) -> &'static [Control] {
    const GRAY: [Control; 3] = [Control::Luma, Control::Log, Control::Reset];
    const COLOR: [Control; 4] = [Control::Luma, Control::Planes, Control::Log, Control::Reset];
    if gray { &GRAY } else { &COLOR }
}

/// One of the buttons down the left of a section whose plot is `bars`, by
/// its place in the column: aligned with the top of the plot's ground
/// rather than with the section, since what they act on is the plot.
fn toolbar_button(bars: Rect, index: usize) -> Rect {
    Rect::new(
        bars.x - TOOLBAR_WIDTH,
        bars.y - PLOT_INSET + index as f32 * (BUTTON_SIZE + TOOLBAR_GAP),
        BUTTON_SIZE,
        BUTTON_SIZE,
    )
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

/// The band of color under a plot, aligned with the bins so that a cell of
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
        x - super::slider::HANDLE_GRIP / 2.0,
        band.y - HANDLE_REACH,
        super::slider::HANDLE_GRIP,
        band.height + 2.0 * HANDLE_REACH,
    )
}

/// Which bin the pointer is over, or `None` when it is not over the plot.
fn hovered_bin(bars: Rect, cursor: Option<[f32; 2]>) -> Option<usize> {
    let cursor = cursor.filter(|point| bars.contains(*point))?;
    let bin = (cursor[0] - bars.x) / bars.width * BINS as f32;
    Some((bin as usize).min(BINS - 1))
}

/// The value at the middle of `bin` on `plot`'s axis.
fn bin_value(plot: &Plot, bin: usize) -> f32 {
    plot.min + bin_across(bin) * (plot.max - plot.min)
}

/// What one section marks: the bin its rule stands on, where the value is
/// on its plot's axis, and what the value is at that stage, for its
/// header.
#[derive(Clone, Debug, Default, PartialEq)]
struct Mark {
    pub bin: Option<usize>,
    pub words: String,
}

/// What every section marks, one for each stage: the pixel under the
/// pointer followed down the column, or the bin under the pointer on the
/// one plot it is over. A section that marks nothing is `None`.
#[derive(Clone, Debug, Default)]
struct Traced {
    pub output: Option<Mark>,
    pub display: Option<Mark>,
    pub gain_map: Option<Mark>,
    pub file: Option<Mark>,
}

impl Traced {
    /// The mark for `section`.
    pub fn of(&self, section: Section) -> Option<&Mark> {
        match section {
            Section::Output => self.output.as_ref(),
            Section::Display => self.display.as_ref(),
            Section::GainMap => self.gain_map.as_ref(),
            Section::File => self.file.as_ref(),
        }
    }

    fn slot(&mut self, section: Section) -> &mut Option<Mark> {
        match section {
            Section::Output => &mut self.output,
            Section::Display => &mut self.display,
            Section::GainMap => &mut self.gain_map,
            Section::File => &mut self.file,
        }
    }
}

/// What each section's plot is of, for the trace: the plots it reads its
/// bins off, worked out once a pass.
struct Plots<'a> {
    pub output: &'a output::Binned,
    pub lift: Option<&'a Plot>,
}

/// What each section marks: the pixel under the pointer traced down the
/// column, or the bin under the pointer on the plot it is over.
///
/// A pointer on one section's plot is reading that plot's axis, and is on
/// no pixel of the picture besides, so only that section marks anything,
/// and what it says is the value its bin stands for. A pointer on the
/// picture marks every section at once, each with the pixel as its own
/// stage has it: as the file stores it, how far the gain map lifts it, what
/// the display starts from and makes of it, and what goes out. The rules
/// down the column are then one pixel's path from the file to the screen.
///
/// One bin a section, and so one rule a section: the panel draws bars, and
/// a marker on it can only honestly point at one of them. A value off a
/// plot's axis is still written, and marks no bar.
fn marked(
    current: &Current,
    layout: &[(Section, Rect)],
    cursor: Option<[f32; 2]>,
    pointer: Option<[u32; 2]>,
    headroom: crate::image::display::Headroom,
    plots: &Plots,
) -> Traced {
    let mut traced = Traced::default();
    for &(section, rect) in layout {
        let bars = section.plot(rect, current.image.is_gray());
        if let Some(bin) = hovered_bin(bars, cursor) {
            *traced.slot(section) = Some(match section {
                Section::Output => output::bin_mark(plots.output, bin),
                Section::Display => display::bin_mark(current, headroom, bin),
                Section::GainMap => gain_map::bin_mark(plots.lift, bin),
                Section::File => file::bin_mark(current, bin),
            });
            return traced;
        }
    }
    let Some([x, y]) = pointer else {
        return traced;
    };
    for &(section, _) in layout {
        *traced.slot(section) = match section {
            Section::Output => output::pixel_mark(current, headroom, plots.output, x, y),
            Section::Display => display::pixel_mark(current, headroom, x, y),
            Section::GainMap => gain_map::pixel_mark(current, plots.lift, x, y),
            Section::File => file::pixel_mark(current, x, y),
        };
    }
    traced
}

/// How wide `text` comes out at `size`.
fn width_of(ui: &egui::Ui, text: &str, size: f32) -> f32 {
    crate::ui::text_width(ui, text, FontId::proportional(size))
}

/// A value on the plot of the image as it is shown or stored, written in
/// the units the file counts in.
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

/// The two ends of a plot of the image as stored or shown, as
/// [`axis_words`] writes them, where they are worth writing: not on the
/// axis a graded file always has. A graded file's plot runs from black to
/// white, which every histogram of such a file does and no one needs told;
/// a linear file's runs over whatever was measured, and what that was is
/// the first thing to know about it.
fn axis_ends(current: &Current, plot: &Plot) -> Option<[String; 2]> {
    let transfer = current.image.color.transfer;
    let nominal = plot.min == 0.0 && (transfer.to_linear(plot.max) - 1.0).abs() < 1e-6;
    (!nominal && plot.max > plot.min)
        .then(|| [plot.min, plot.max].map(|end| axis_words(current, end)))
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

/// Draws the histogram down the side panel `side`: the sections in their
/// order, in a column that scrolls where the side is shorter than they are.
///
/// Every section is laid out at its own height whatever the room, so that
/// nothing is shrunk or dropped on a short window; the head section is what
/// the panel needs room for at least — see [`MIN_HEIGHT`] — and the rest is
/// scrolled to. The scroll is egui's, and is the wheel's alone: a drag in
/// the column is a drag of whatever is under it — a handle, the band, the
/// exposure's slider — and never moves the column.
pub(super) fn show(pass: &mut Pass, ui: &mut egui::Ui, current: &Current, side: Rect) {
    let panel = panel(side);
    let body = ui.interact(
        egui::Rect::from(panel),
        ui.id().with("histogram panel"),
        Sense::CLICK | Sense::DRAG,
    );
    body.widget_info(|| WidgetInfo::labeled(WidgetType::Other, true, "Histogram panel"));
    let height = section::height(current);
    egui::ScrollArea::vertical()
        .id_salt("histogram column")
        .auto_shrink(false)
        .scroll_source(egui::scroll_area::ScrollSource {
            drag: egui::scroll_area::DragScroll::Never,
            ..Default::default()
        })
        .show(ui, |ui| {
            let (column, _) =
                ui.allocate_exact_size(egui::vec2(PANEL_WIDTH, height), Sense::hover());
            let column = Rect::new(column.min.x, column.min.y, PANEL_WIDTH, height);
            let layout = section::layout(column, current);
            // A pointer over a section scrolled out of the panel is not
            // over it.
            let visible = ui.clip_rect();
            let cursor = pass
                .input
                .cursor
                .filter(|&[x, y]| visible.contains(pos2(x, y)));
            let binned = output::binned(ui, current, pass.input);
            let lift = gain_map::lift_plot(ui, current);
            let plots = Plots {
                output: &binned,
                lift: lift.as_deref(),
            };
            let traced = marked(
                current,
                &layout,
                cursor,
                pass.input.pointer,
                pass.input.headroom,
                &plots,
            );
            for (section, rect) in layout {
                let mark = traced.of(section);
                match section {
                    Section::Output => output::show(pass, ui, current, rect, &binned, mark),
                    Section::Display => display::show(pass, ui, current, rect, mark),
                    Section::GainMap => gain_map::show(pass, ui, current, rect, &plots, mark),
                    Section::File => file::show(pass, ui, current, rect, mark),
                }
            }
        });
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

/// The strip of buttons down the left of the plot `bars`: the luminance
/// and the color planes, the logarithmic counts and the reset. Beside the
/// head plot, since they act on every plot down the column and the head is
/// where they are always in view.
///
/// Drawn here rather than with the chrome's toggles because these belong to
/// the panel: they say what the plots beside them are showing, and two of
/// them are pictures of the very thing they switch.
fn toolbar_buttons(pass: &mut Pass, ui: &mut egui::Ui, gray: bool, bars: Rect) {
    let panels = pass.panels;
    for (slot, widget) in toolbar(gray).iter().enumerate() {
        let rect = toolbar_button(bars, slot);
        // The reset is never lit, where the toggles above it are: it does
        // something rather than being something, and a momentary button
        // holding a state is a button that has to explain itself.
        let active = panels.lit(*widget);
        let (_, background, ink) = button(pass, ui, rect, *widget, active, true, TOGGLE_RADIUS);
        let grid = pass.grid;
        let square = icon::square(grid, egui::Rect::from(rect), ICON_SIDE);
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
                painter.circle_filled(pos2(at[0], at[1]), place.units(LUMA_DISC), HISTOGRAM_LUMA);
            }
            // And the color planes are three, so they are three smaller
            // discs, in their own colors: nothing else in the window is red,
            // green and blue together.
            Control::Planes => {
                let place = icon::Placer::new(
                    grid,
                    Rect::new(square.min.x, square.min.y, square.width(), square.height()),
                );
                for (turn, plane) in HISTOGRAM_PLANES.into_iter().enumerate() {
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
}

/// A button with a mark from `ui::icon` on it, beside a section's plot or
/// band.
fn icon_button(
    pass: &mut Pass,
    ui: &mut egui::Ui,
    rect: Rect,
    control: Control,
    active: bool,
    mark: &[icon::Mark],
) {
    let (_, background, ink) = button(pass, ui, rect, control, active, true, TOGGLE_RADIUS);
    let square = icon::square(pass.grid, egui::Rect::from(rect), ICON_SIDE);
    icon::paint(ui.painter(), mark, square, ink, background);
}

/// The color of the band a section draws under its plot, at a point along
/// it, and whether the screen goes past white there.
type Shade = (Color, bool);

#[cfg(test)]
mod tests;
