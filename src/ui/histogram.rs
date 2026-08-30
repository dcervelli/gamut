//! The floating histogram panel.

use crate::image::stats::BINS;
use crate::render::{Blend, Rect, UiFrame};
use crate::theme::Theme;

use super::{Current, PADDING, TEXT_SIZE};

/// The gap between the histogram panel's edge and its plot.
const HISTOGRAM_INSET: f32 = 10.0;

/// Wide enough that a bin is exactly one logical pixel, which is what keeps
/// the bars evenly spaced instead of some of them landing astride a pixel
/// boundary and coming out fatter than their neighbours.
const HISTOGRAM_SIZE: [f32; 2] = [BINS as f32 + 2.0 * HISTOGRAM_INSET, 130.0];

/// What the luminance plane drops to once colour planes are drawn over it.
const HISTOGRAM_LUMA_UNDER: u8 = 110;

/// Draws the histogram in the bottom-right of `content`, the area the panels
/// leave free.
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
        (content.bottom() - HISTOGRAM_SIZE[1] - PADDING)
            .max(content.y + PADDING)
            .round(),
        HISTOGRAM_SIZE[0],
        HISTOGRAM_SIZE[1],
    );
    frame.rounded_rect(panel, 6.0, theme.panel_background);

    let plot = panel.inset(HISTOGRAM_INSET, HISTOGRAM_INSET);
    let label_height = TEXT_SIZE * 1.4;
    let bars = Rect::new(
        plot.x,
        plot.y + label_height,
        plot.width,
        plot.height - label_height,
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
        theme.panel_text,
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

    // Where the display window sits within the plotted range.
    let span = axis_max - axis_min;
    if span > 0.0 {
        for value in [current.display.low, current.display.high] {
            let encoded = transfer.to_encoded(value);
            let position = ((encoded - axis_min) / span).clamp(0.0, 1.0);
            frame.rect(
                Rect::new(
                    bars.x + position * bars.width - 0.5,
                    bars.y,
                    1.5,
                    bars.height,
                ),
                theme.accent,
            );
        }
    }
}
