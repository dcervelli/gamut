//! The Display section: what the display starts from — the picture lifted
//! where it has a gain map, in the working space's primaries — and what it
//! does to it, drawn over that as the response curve and set under it: the
//! band and its handles, the row of false colors a gray image has, and the
//! rows of the exposure, the window and the curve.

use egui::{Align2, FontId, Stroke, pos2};

use super::plot::{Scale, band, bars, corners};
use super::slider::slider;
use super::track::track;
use super::*;
use crate::image::display::{Colormap, Display, Headroom, ToneMap};

/// The row of false colors under the band: how deep a swatch is, and the
/// corner it is drawn with. Deep enough to press and to read the map off,
/// shallow enough that the row reads as a legend under the band rather than
/// as a second band.
pub(super) const SWATCH_HEIGHT: f32 = 16.0;
const SWATCH_RADIUS: f32 = 3.0;
/// What is left around a swatch's color inside its button, so that the
/// button's own lit background is what shows a chosen map.
const SWATCH_INSET: f32 = 3.0;

/// The rows of controls under the band: how tall a row is, and the gap
/// between one and the next.
const ROW_HEIGHT: f32 = 20.0;
const ROW_GAP: f32 = 8.0;

/// What the three rows of settings under the band take off the section's
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
/// file, so the column never jumps from one file to the next.
pub(super) const ROWS_HEIGHT: f32 = PLOT_INSET + 3.0 * (ROW_GAP + ROW_HEIGHT);

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

/// The plot and the row of false colors under its band, together: the room
/// the section gives them, the same for every file.
fn full_bars(rect: Rect) -> Rect {
    let inside = rect.inset(PANEL_INSET, 0.0);
    Rect::new(
        inside.x + TOOLBAR_WIDTH,
        rect.y + LABEL_HEIGHT + PLOT_INSET,
        inside.width - TOOLBAR_WIDTH,
        PLOT_HEIGHT + RAMP_GAP + SWATCH_HEIGHT,
    )
}

/// The plot's ground in the section laid out at `rect`, on an image the
/// false colors apply to or not: the row of them takes its room off the
/// bottom of the plot, and the band moves up with it.
///
/// The section is one height either way, so the sections below it do not
/// shift about from a gray file to a color one. What changes is how much
/// of that height is plot, which nothing reads absolutely — the bars are
/// drawn as fractions of whatever they are given.
pub(super) fn plot_area(rect: Rect, gray: bool) -> Rect {
    let bars = full_bars(rect);
    if !gray {
        return bars;
    }
    Rect::new(bars.x, bars.y, bars.width, PLOT_HEIGHT)
}

/// One of the false-color swatches under the band of the plot `bars`, by
/// its place in [`Colormap::ALL`]. They divide the plot's width between
/// them, so each sits under the stretch of band it would color.
///
/// Only on an image they apply to. The display ignores the false color on a
/// three-channel image — its channels are colors already — so on one of
/// those the row is not there at all, and the plot has the room instead.
pub(super) fn swatch_button(bars: Rect, index: usize) -> Rect {
    let row = Rect::new(
        bars.x,
        ramp(bars).bottom() + RAMP_GAP,
        bars.width,
        SWATCH_HEIGHT,
    );
    share(row, Colormap::ALL.len(), index)
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
/// taken off the plot rather than off the section, see [`plot_area`] — so
/// this is one block either way, and the rows do not shift about from one
/// file to the next: the three rows are every file's.
#[derive(Clone, Copy)]
pub(super) struct Rows {
    /// The exposure's line, and where its word goes: the slider along it
    /// and its reading at the end.
    pub exposure: Rect,
    exposure_label: Rect,
    /// The three windows.
    pub window: (Rect, Rect),
    /// And the two choices for the curve.
    pub curve: (Rect, Rect),
}

impl Rows {
    /// The rows of the section laid out at `rect`.
    pub fn new(rect: Rect) -> Self {
        let inside = rect.inset(PANEL_INSET, 0.0);
        let left = inside.x + ROW_LABEL + ROW_LABEL_GAP;
        let line = |y: f32| {
            (
                Rect::new(left, y, inside.right() - left, ROW_HEIGHT),
                Rect::new(inside.x, y, ROW_LABEL, ROW_HEIGHT),
            )
        };

        let mut y = ramp(full_bars(rect)).bottom() + ROW_GAP;
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
    pub fn stops(&self) -> Rect {
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
    pub fn slider(&self) -> Rect {
        Rect::new(
            self.exposure.x,
            self.exposure.y,
            self.stops().x - CELL_GAP - self.exposure.x,
            self.exposure.height,
        )
    }

    /// Each row's word and where it goes, in the order they are stacked.
    pub fn labels(&self) -> impl Iterator<Item = (&'static str, Rect)> {
        [
            ("Exposure", self.exposure_label),
            ("Window", self.window.1),
            ("Curve", self.curve.1),
        ]
        .into_iter()
    }

    /// The bottom of the lowest row: where the block, and the section, end.
    #[cfg(test)]
    pub fn bottom(&self) -> f32 {
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
pub(super) fn row_buttons(rect: Rect) -> impl Iterator<Item = (Control, Rect)> {
    let rows = Rows::new(rect);
    let (row, _) = rows.window;
    let windows = (0..WINDOWS.len())
        .map(move |index| (Control::Window(index), share(row, WINDOWS.len(), index)));
    let (row, _) = rows.curve;
    let curves = (0..ToneMap::ALL.len())
        .map(move |index| (Control::Curve(index), share(row, ToneMap::ALL.len(), index)));
    windows.chain(curves)
}

/// What the display makes of the value `encoded` stands at on the plot of
/// the picture as it starts from it: the value, in the file's units, and
/// what the display turns it into, which is the height of the response
/// curve where the rule crosses it and the one number a curve on its own
/// cannot be read off by eye.
///
/// Neither will measure against the plot underneath with a ruler, because
/// both of the plot's axes are spaced in the file's own encoding — the bins
/// across, so that a quantized file does not comb, and the response up, so
/// that a display doing nothing is the diagonal. The positions are the
/// file's units and the numbers are the ones every other readout quotes; a
/// curve that is straight and a value that is comparable cannot both be
/// had, and the shape is what the plot is for.
fn words(current: &Current, headroom: Headroom, encoded: f32) -> String {
    let value = current.image.color.transfer.to_linear(encoded);
    let mapped = current
        .display
        .response(value, current.image.is_gray(), headroom)
        .max(0.0);
    format!(
        "{}  {BECOMES}  {}",
        axis_words(current, encoded),
        trimmed(format!("{mapped:.3}"))
    )
}

/// What the section says with the pointer on its plot at `bin`.
pub(super) fn bin_mark(current: &Current, headroom: Headroom, bin: usize) -> Mark {
    Mark {
        bin: Some(bin),
        words: words(current, headroom, bin_value(&current.stats.plot, bin)),
    }
}

/// What the section says with the pointer on the picture's pixel `(x, y)`:
/// the pixel as the display starts from it, which is lifted and in the
/// working space, and what the display makes of it.
pub(super) fn pixel_mark(current: &Current, headroom: Headroom, x: u32, y: u32) -> Option<Mark> {
    let sample = current.sample(x, y)?;
    let encoded = Plot::value_of(&current.image, &sample)?;
    Some(Mark {
        bin: current.stats.plot.bin_of(&current.image, &sample),
        words: words(current, headroom, encoded),
    })
}

/// The Display section, laid out at `rect`: the picture as the display
/// starts from it, faded to a backdrop for the response curve over it; the
/// band of what each value comes out as, with the window's two handles on
/// it; the row of false colors on a gray image; and the rows under them.
pub(super) fn show(
    pass: &mut Pass,
    ui: &mut egui::Ui,
    current: &Current,
    rect: Rect,
    mark: Option<&Mark>,
) {
    let gray = current.image.is_gray();
    let plot_rect = plot_area(rect, gray);
    let plotted = &current.stats.plot;
    bars(
        pass,
        ui,
        plotted,
        plot_rect,
        true,
        mark.and_then(|mark| mark.bin),
    );
    let dim = Color32::from(pass.theme.text_dim);
    if let Some([low, high]) = axis_ends(current, plotted) {
        corners(ui, plot_rect, Some((low, dim)), Some((high, dim)));
    }

    let span = plotted.max - plotted.min;
    if span > 0.0 {
        // What the display turns each value into, in a band along the foot
        // of the plot: the value above a cell, and the color it comes out
        // as under it.
        //
        // The curve says how much and this says what of, which are
        // different questions on a false-colored image — a curve cannot
        // draw viridis — and the same question answered twice on a gray
        // one, where the band is the tone curve as a wedge and the curve is
        // it as a shape. Everything left of the window comes out black and
        // everything right of it at the top of the ramp, so the two flat
        // runs at the ends are the range the display is throwing away,
        // drawn at the width they occupy. The handles that set those ends
        // are drawn over it, in [`track()`].
        let transfer = current.image.color.transfer;
        let headroom = pass.input.headroom;
        let channels = current.image.channels();
        let display = &current.display;
        band(pass, ui, plot_rect, |t| {
            let value = transfer.to_linear(plotted.min + t * span);
            (
                Color::from_linear(display.shade(value, channels, headroom)),
                display.response(value, gray, headroom) > ABOVE_WHITE,
            )
        });
        curve(pass, ui, current, plot_rect);
    }

    let held = track(pass, ui, current, plot_rect);
    if gray {
        swatches(pass, ui, current, plot_rect);
    }
    rows(pass, ui, current, rect);
    let readout = held.or_else(|| mark.map(|mark| mark.words.clone()));
    section::header(pass, ui, Section::Display, rect, readout.as_deref());
}

/// What the display does to every value on the plot, drawn over it.
///
/// Always, where it used to be drawn only where the display was doing
/// anything: this section is the display, and its curve is what the
/// section is read for — the diagonal on a display doing nothing says so as
/// plainly as a bend says what it is doing.
///
/// The curve is the whole of what the display does, and the only part of
/// the panel that can show a tone map at all: a shoulder is a shape, not a
/// threshold, and there is no line that means "rolled off". The handles on
/// the band place the two values that come out black and white, exposure
/// included, which a curve meeting its floor tangentially cannot be read
/// for by eye.
fn curve(pass: &Pass, ui: &egui::Ui, current: &Current, bars: Rect) {
    let theme = pass.theme;
    let headroom = pass.input.headroom;
    let plotted = &current.stats.plot;
    let span = plotted.max - plotted.min;
    let transfer = current.image.color.transfer;
    let gray = current.image.is_gray();
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
    // A vertex to the device pixel, so that a steep toe is drawn as finely
    // as the screen can show it.
    let columns = pass.grid.columns(bars.x, bars.width).len().max(1);
    // Decoded to run the transform on, then encoded again to be drawn: both
    // axes are in the file's own units, so a display doing nothing would be
    // the diagonal.
    let responses: Vec<f32> = (0..=columns)
        .map(|column| {
            let across = column as f32 / columns as f32;
            let value = transfer.to_linear(plotted.min + across * span);
            let response = current.display.response(value, gray, headroom).max(0.0);
            transfer.to_encoded(response).max(0.0)
        })
        .collect();
    // The plot's height is white, unless the response runs past it — a
    // surface with room above white, and no curve on — in which case the
    // top is wherever the response gets to and white is a line across it.
    let highest = responses.iter().copied().fold(0.0, f32::max);
    let scale = Scale::new(transfer.to_encoded(1.0), highest);
    if let Some(white) = scale.white {
        plot::white_line(pass, ui, bars, white);
    }
    let points: Vec<egui::Pos2> = responses
        .iter()
        .enumerate()
        .map(|(column, &response)| {
            let across = column as f32 / columns as f32;
            pos2(
                bars.x + across * bars.width,
                bars.bottom() - scale.up(response) * bars.height,
            )
        })
        .collect();
    ui.painter().add(egui::Shape::line(
        points,
        Stroke::new(CURVE_WIDTH, theme.accent),
    ));
}

/// The false colors, each showing itself, under the band of the plot
/// `bars`, and only where the display would act on the choice. The whole
/// ramp rather than one color off it: a map is a sequence, and a single
/// swatch of viridis is a green rectangle that could be anything.
fn swatches(pass: &mut Pass, ui: &mut egui::Ui, current: &Current, bars: Rect) {
    for (index, map) in Colormap::ALL.into_iter().enumerate() {
        let rect = swatch_button(bars, index);
        let chosen = current.display.colormap() == map;
        button(
            pass,
            ui,
            rect,
            Control::Ramp(index),
            chosen,
            true,
            SWATCH_RADIUS,
        );

        // The gradient on the device's pixels, as the band above it is: a
        // swatch is the same row of one-pixel cells, over less room.
        let face = rect.inset(SWATCH_INSET, SWATCH_INSET);
        let grid = pass.grid;
        let snap = |value: f32| grid.snap(value);
        let (top, bottom) = (snap(face.y), snap(face.bottom()));
        for column in grid.columns(face.x, face.width) {
            ui.painter().rect_filled(
                egui::Rect::from_min_max(pos2(column.left, top), pos2(column.right, bottom)),
                0.0,
                Color::from_linear(map.color(column.t_center())),
            );
        }
    }
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
fn rows(pass: &mut Pass, ui: &mut egui::Ui, current: &Current, rect: Rect) {
    let theme = pass.theme;
    let rows = Rows::new(rect);
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
    // its ramp whatever curve is on — the row stays, since the section's
    // height is the file's, and says why when rested on.
    let false_colored = display.false_colored(current.image.is_gray());
    for (widget, rect) in row_buttons(rect) {
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
