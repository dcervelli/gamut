//! The plot itself: the bins as columns, the pointer's rule, the shares
//! clipped at either end, the band along the foot and the response curve
//! over the lot — and the arithmetic under it: where a bin stands, how tall
//! its bar is, and what a stretch of a column comes out as with the planes
//! standing over it.

use egui::{Align2, Color32, FontId, Stroke, pos2, vec2};

use super::*;

/// The planes a column of the plot is drawn from: which of them stand this
/// high, as a set. What the color of a stretch of the column is a function of.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Cover {
    pub luma: bool,
    pub planes: [bool; 3],
}

/// What a stretch of a column comes out as, with `cover` standing over it:
/// the plot's ground, the luminance plane laid over that, and the color
/// planes screened over the lot.
///
/// egui has one blend, so the screening is done here, per stretch of
/// column: the planes are the primaries on a near-black ground, so two of
/// them give the secondary between and all three give white, which is the
/// reading a channel histogram is looked at for.
pub(super) fn screened(theme: &Theme, luma_ink: Color, cover: Cover) -> Color32 {
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

/// How the response curve is scaled up the plot: in the file's own encoding,
/// as the bins across are, from 0 at the axis to `ceiling` at the top.
///
/// The ceiling is white — encoded, so that a display doing nothing draws the
/// diagonal — except where the curve runs past it, which it does on a surface
/// with room above white and no curve on: the top of the plot is then
/// wherever the response gets to, and white is a line drawn across it.
/// `white` is where that line goes, as a fraction of the plot's height, and
/// `None` where white is the top and the line would be the plot's own edge.
pub(super) struct Scale {
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

/// Where a bin's bar is drawn across the plot, from 0 at the left edge to 1
/// at the right. The two end bins are carried out to the edges so that the
/// shape fills the plot's width; the rest stand at their centers.
pub(super) fn bin_across(index: usize) -> f32 {
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
pub(super) fn bar_fraction(count: u32, peak: u32, log: bool) -> f32 {
    let (count, peak) = (count as f32, peak.max(1) as f32);
    if log {
        count.ln_1p() / peak.ln_1p()
    } else {
        count / peak
    }
}

/// The plot itself: the bins, the pointer's rule, the shares clipped at
/// either end, the band along the foot and — where the display is doing
/// anything — the response curve over the lot.
pub(super) fn plot(pass: &Pass, ui: &egui::Ui, current: &Current, panel: Rect, content: Rect) {
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
    let grid = pass.grid;
    let snap = |value: f32| grid.snap(value);
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
            area(grid.rect(Rect::new(
                bars.x + across * bars.width - CURSOR_WIDTH / 2.0,
                bars.y,
                CURSOR_WIDTH,
                bars.height,
            ))),
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
    // set those ends are drawn over it, in [`track()`].
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
        if current
            .display
            .response(value, channels.is_gray(), headroom)
            > ABOVE_WHITE
        {
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
        grid,
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
    // a threshold, and there is no line that means "rolled off". The handles
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
            let response = current
                .display
                .response(value, channels.is_gray(), headroom)
                .max(0.0);
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
        let y = grid.snap(bars.bottom() - white * bars.height);
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
