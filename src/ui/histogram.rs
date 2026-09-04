//! The floating histogram panel.

use crate::image::display::Colormap;
use crate::image::stats::BINS;
use crate::render::{Blend, Color, Rect, TextMeasure, UiFrame};
use crate::theme::Theme;

use super::buttons::{ICON_SIDE, button_ink, outline};
use super::chrome::BUTTON_SIZE;
use super::icon;
use super::{
    BECOMES, Current, FrameInput, PADDING, PANEL_INSET, PANEL_RADIUS, PANEL_WIDTH, Panels,
    TEXT_SIZE, Widget,
};

/// The panel: as wide as anything else floating over the content area, and
/// tall enough for a plot with its axis label above it and the ramp of what
/// the display makes of that axis below.
pub(super) const HISTOGRAM_SIZE: [f32; 2] = [
    PANEL_WIDTH,
    130.0 + RAMP_GAP + RAMP_HEIGHT + RAMP_GAP + SWATCH_HEIGHT,
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
/// plane toggles are drawn from, in that grid's units: how far each colour
/// disc is struck from the middle, and how big the discs are. The luminance
/// disc is one mark where the colours are three, so it is the larger.
const GRID_MIDDLE: f32 = 12.0;
const LUMA_DISC: f32 = 8.0;
const PLANE_ORBIT: f32 = 5.5;
const PLANE_DISC: f32 = 4.0;
/// The row of false colours under the ramp: how deep a swatch is, and the
/// corner it is drawn with. Deep enough to press and to read the map off,
/// shallow enough that the row reads as a legend under the band rather than
/// as a second band.
const SWATCH_HEIGHT: f32 = 16.0;
const SWATCH_RADIUS: f32 = 3.0;
/// What is left around a swatch's colour inside its button, so that the
/// button's own lit background is what shows a chosen map.
const SWATCH_INSET: f32 = 3.0;

/// The corner radius of the plot's own ground inside the panel. Smaller than
/// the panel's, the way an inner corner always is.
const PLOT_RADIUS: f32 = 3.0;
/// The room left around that ground, so the plot reads as set into the panel
/// rather than as a hole cut in it.
const PLOT_INSET: f32 = 4.0;

/// What the luminance plane drops to once colour planes are drawn over it.
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

/// The ramp under the plot: how deep the band of colour is, and how far it
/// stands off the plot's ground. Deep enough to read a colour off and no
/// deeper — it is a legend along the axis, not a second plot.
const RAMP_HEIGHT: f32 = 8.0;
const RAMP_GAP: f32 = 4.0;

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
        plot.height - label_height - PLOT_INSET - RAMP_GAP - RAMP_HEIGHT,
    )
}

/// The same, on an image the false colours apply to: the row of them takes
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
/// with one channel has no colour planes to toggle, and the two that remain
/// close the gap up. Hiding rather than dimming is the panel's rule for both
/// of these — see [`swatch_button`] for the other one.
fn toolbar(gray: bool) -> &'static [Widget] {
    const GRAY: [Widget; 3] = [Widget::Luma, Widget::Log, Widget::Reset];
    const COLOUR: [Widget; 4] = [Widget::Luma, Widget::Planes, Widget::Log, Widget::Reset];
    if gray { &GRAY } else { &COLOUR }
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

/// One of the false-colour swatches under the ramp, by its place in
/// [`Colormap::ALL`]. They divide the plot's width between them, so each sits
/// under the stretch of band it would colour.
///
/// Only on an image they apply to. The display ignores the false colour on a
/// three-channel image — its channels are colours already — so on one of
/// those the row is not there at all, and the plot has the room instead.
fn swatch_button(bars: Rect, index: usize) -> Rect {
    let count = Colormap::ALL.len() as f32;
    let width = (bars.width - (count - 1.0) * RAMP_GAP) / count;
    Rect::new(
        bars.x + index as f32 * (width + RAMP_GAP),
        ramp(bars).bottom() + RAMP_GAP,
        width,
        SWATCH_HEIGHT,
    )
}

/// Which of the panel's own buttons a point lands on.
///
/// Two of them do not apply to every image, and `gray` decides both: a
/// single-channel image has no colour planes to toggle, and a colour image is
/// its own colour, so the false colours are what the display leaves out for
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
    if !gray {
        return None;
    }
    let bars = plot_area(panel, gray);
    (0..Colormap::ALL.len())
        .find(|&index| swatch_button(bars, index).contains(point))
        .map(Widget::Ramp)
}

/// The band of colour under the plot, aligned with the bins so that a cell of
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
/// shape fills the plot's width; the rest stand at their centres.
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
/// centre and a mark comes out as the shape it is.
fn device(value: f32, scale: f32) -> f32 {
    (value * scale).round() / scale
}

/// One mark moved onto that grid, kept at least a whole pixel so that
/// something thinner than one is still drawn rather than rounded away.
///
/// Not what the ramp's cells use: they tile, so what matters there is that
/// each shares an edge exactly with its neighbour, and a floor under their
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
/// Colour images get four planes — red, green, blue and luminance — over the
/// range their colour channels span; grey images keep the single luminance
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

    // What applies to this image: the false colours are for a single channel
    // and the colour planes are for three, and the panel leaves out whichever
    // the display would ignore rather than drawing it dead.
    let gray = current.image.is_gray();
    let bars = plot_area(panel, gray);
    frame.rounded_rect(
        bars.inset(-PLOT_INSET, -PLOT_INSET),
        PLOT_RADIUS,
        theme.plot_background,
    );

    // Luminance always goes down first, underneath the colour planes: it
    // runs as tall as the tallest of them about as often as not, and painting
    // it on top swamps the colour the panel exists to show.
    let plotted = &current.stats.plot;
    let luma: &[[u32; BINS]] = if panels.show_luma {
        std::slice::from_ref(&plotted.luma)
    } else {
        &[]
    };
    let colour: &[[u32; BINS]] = match plotted.colour.as_ref() {
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
    // quantised file does not comb, and the response up, so that a display
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
    let peak = colour
        .iter()
        .chain(luma)
        .flatten()
        .copied()
        .max()
        .unwrap_or(1);
    let height_of = |count: u32| bar_fraction(count, peak, panels.log_counts) * bars.height;
    // One point per bin, at its centre, with the ends carried out to the
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

    // Dimmed only when it is a backdrop; with the colour planes off — or on
    // a grey image, which has none — it is the plot.
    let luma_ink = if colour.is_empty() {
        theme.histogram_luma
    } else {
        theme.histogram_luma.with_alpha(HISTOGRAM_LUMA_UNDER)
    };
    for counts in luma {
        frame.area(&curve(counts), bars.bottom(), luma_ink, Blend::Over);
    }
    for (counts, color) in colour.iter().zip(theme.histogram_planes) {
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

    controls(frame, current, input, panels, panel, bars, theme);

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
        // along the foot of the plot: the bin above a cell, and the colour it
        // comes out as under it.
        //
        // The curve says how much and this says what of, which are different
        // questions on a false-coloured image — a curve cannot draw viridis —
        // and the same question answered twice on a grey one, where the band
        // is the tone curve as a wedge and the curve is it as a shape. It is
        // where clipping stops being an inference: everything left of the
        // window comes out black and everything right of it comes out at the
        // top of the ramp, so the two flat runs at the ends are the range the
        // display is throwing away, drawn at the width they occupy.
        //
        // One cell per bin, over the bin's own middle, so a cell is the
        // colour of the bar standing above it.
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
        // Outside the colour rather than over it, so that the band keeps its
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
/// colours under its ramp.
///
/// Drawn here rather than with the chrome's toggles because these belong to
/// the panel: they say what the plot beside them is showing and what the band
/// beneath them is painted with, and two of them are pictures of the very
/// thing they switch.
fn controls(
    frame: &mut UiFrame,
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
            // each in the colour that plane is plotted in — which is not
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
            // And the colour planes are three, so they are three smaller
            // discs, in their own colours: nothing else in the window is red,
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

    // The false colours, each showing itself, and only where the display
    // would act on the choice. The whole ramp rather than one colour off it:
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

#[cfg(test)]
mod tests {
    use super::*;

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
            // And it ends above the band of colour, which is what the room
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

    /// The false colours take their room off the plot rather than off the
    /// panel, so that the information panel below does not shift about from
    /// one file to the next.
    #[test]
    fn the_panel_is_one_height_with_the_false_colours_and_without() {
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

        // Nothing answers where the row of false colours would be on an image
        // that has none: the plot is there instead.
        let swatch = middle(swatch_button(plot_area(panel, true), 2));
        assert_eq!(widget_at(content, swatch, true), Some(Widget::Ramp(2)));
        assert_eq!(
            widget_at(content, swatch, false),
            None,
            "three channels are their own colour, and the false ones are not applied"
        );
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
        let bars = bars(panel(Rect::new(0.0, 0.0, 800.0, 600.0)));
        let centred = |width: f32| bars.x + (bars.width - width) / 2.0;

        let (x, fits) = readout_placement(bars, 80.0, [40.0, 40.0]);
        assert_eq!(x, centred(80.0), "centred on the plot, not on the panel");
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
