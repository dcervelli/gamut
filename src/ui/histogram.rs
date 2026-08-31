//! The floating histogram panel.

use crate::image::stats::BINS;
use crate::render::{Blend, Rect, UiFrame};
use crate::theme::Theme;

use super::{Current, PADDING, PANEL_INSET, PANEL_RADIUS, PANEL_WIDTH, TEXT_SIZE};

/// The panel: as wide as anything else floating over the content area, and
/// tall enough for a plot with its axis label above it.
pub(super) const HISTOGRAM_SIZE: [f32; 2] = [PANEL_WIDTH, 130.0];

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
/// What the window's markers drop to beside that curve. They place its two
/// ends, and are drawn in the same ink for that reason, but the curve is the
/// thing being read.
const MARKER_ALPHA: u8 = 110;

/// Draws the histogram in the top-right of `content`, the area the panels
/// leave free — above the information panel, the order the two toggles that
/// open them are stacked in.
///
/// Colour images get four planes — red, green, blue and luminance — over the
/// range their colour channels span; grey images keep the single luminance
/// plane over theirs.
pub(super) fn draw(frame: &mut UiFrame, current: &Current, content: Rect, theme: &Theme) {
    // Rounded, so that the whole-pixel bin spacing starts on a pixel edge.
    let panel = Rect::new(
        (content.right() - HISTOGRAM_SIZE[0] - PADDING)
            .max(content.x + PADDING)
            .round(),
        (content.y + PADDING).round(),
        HISTOGRAM_SIZE[0],
        HISTOGRAM_SIZE[1],
    );
    frame.rounded_rect(panel, PANEL_RADIUS, theme.panel_background);

    let plot = panel.inset(PANEL_INSET, PANEL_INSET);
    let label_height = TEXT_SIZE * 1.4;
    // The bins are one logical pixel each, so the ground under them is the
    // full width of the plot and the room around it is drawn outside that.
    let bars = Rect::new(
        plot.x,
        plot.y + label_height + PLOT_INSET,
        plot.width,
        plot.height - label_height - PLOT_INSET,
    );
    frame.rounded_rect(
        bars.inset(-PLOT_INSET, -PLOT_INSET),
        PLOT_RADIUS,
        theme.plot_background,
    );

    // Luminance always goes down first, underneath the colour planes: it
    // runs as tall as the tallest of them about as often as not, and painting
    // it on top swamps the colour the panel exists to show.
    let plotted = &current.stats.plot;
    let luma = &plotted.luma;
    let colour: &[[u32; BINS]] = plotted.colour.as_ref().map_or(&[], |planes| planes);
    let (axis_min, axis_max) = (plotted.min, plotted.max);

    // The axis is in the file's own encoding; the label is not, since the
    // numbers everything else quotes are the decoded ones.
    let transfer = current.image.color.transfer;
    frame.text(
        [plot.x, plot.y],
        TEXT_SIZE * 0.85,
        theme.text_dim,
        format!(
            "{:.4}  \u{2013}  {:.4}",
            transfer.to_linear(axis_min),
            transfer.to_linear(axis_max)
        ),
    );

    // One peak across every plane, so their heights stay comparable.
    let peak = colour
        .iter()
        .flatten()
        .chain(luma)
        .copied()
        .max()
        .unwrap_or(1)
        .max(1) as f32;
    let bin_width = bars.width / BINS as f32;
    // Strictly linear in the counts, the way a photo editor plots it: the
    // height of a bin is its share of the fullest one. A single dominating
    // bin — a nodata background, say — will flatten the rest, which is a
    // measurement-data problem to solve separately.
    let height_of = |count: u32| (count as f32 / peak) * bars.height;
    // One point per bin, at its centre, with the ends carried out to the
    // edges of the plot so the shape fills its width.
    let curve = |counts: &[u32; BINS]| -> Vec<[f32; 2]> {
        counts
            .iter()
            .enumerate()
            .map(|(index, &count)| {
                let x = match index {
                    0 => bars.x,
                    last if last == BINS - 1 => bars.right(),
                    _ => bars.x + (index as f32 + 0.5) * bin_width,
                };
                [x, bars.bottom() - height_of(count)]
            })
            .collect()
    };

    // Dimmed only when it is a backdrop; on a grey image it is the plot.
    let luma_ink = if colour.is_empty() {
        theme.histogram_luma
    } else {
        theme.histogram_luma.with_alpha(HISTOGRAM_LUMA_UNDER)
    };
    frame.area(&curve(luma), bars.bottom(), luma_ink, Blend::Over);
    for (counts, color) in colour.iter().zip(theme.histogram_planes) {
        frame.area(&curve(counts), bars.bottom(), color, Blend::Screen);
    }

    // What the display is doing to the values underneath, drawn over them.
    //
    // The markers place the two ends of the window: the values that come out
    // black and white, exposure included, rather than the window's own
    // bounds — exposure lives in the gain, so it moves white without moving
    // `high`, and a marker taken from the bounds would sit where nothing is
    // happening. The curve between them is the rest of the answer, and the
    // only part that can show a tone curve at all: a shoulder is a shape, not
    // a threshold, and there is no line that means "reinhard".
    let span = axis_max - axis_min;
    if span > 0.0 {
        let (black, white) = current.display.displayed_bounds();
        for value in [black, white] {
            let encoded = transfer.to_encoded(value);
            // `clamp` passes a NaN straight through, so a non-finite window
            // would put a NaN rectangle into the vertex buffer. Skip it: the
            // marker for a degenerate window is simply not drawn.
            let position = ((encoded - axis_min) / span).clamp(0.0, 1.0);
            if !position.is_finite() {
                continue;
            }
            frame.rect(
                Rect::new(
                    bars.x + position * bars.width - 0.5,
                    bars.y,
                    1.5,
                    bars.height,
                ),
                theme.accent.with_alpha(MARKER_ALPHA),
            );
        }

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
            let curve: Vec<[f32; 2]> = (0..=columns)
                .map(|column| {
                    let across = column as f32 / columns as f32;
                    // Decoded to run the transform on, then encoded again to
                    // be drawn: both axes are in the file's own units, so a
                    // display doing nothing is the diagonal. Plotting the
                    // linear response against an encoded axis would bend the
                    // curve by the transfer function alone, and draw a
                    // shoulder into an image nobody had touched.
                    let value = transfer.to_linear(axis_min + across * span);
                    let response = current.display.response(value).clamp(0.0, 1.0);
                    let response = transfer.to_encoded(response).clamp(0.0, 1.0);
                    [
                        bars.x + across * bars.width,
                        bars.bottom() - response * bars.height,
                    ]
                })
                .collect();
            frame.polyline(&curve, CURVE_WIDTH, theme.accent, Blend::Over);
        }
    }
}
