//! The Image section: the picture as the file stores it — no lift, and in
//! its own primaries — at the foot of the column, where everything above it
//! starts from. Named for the picture rather than for the file, since the
//! gain map above it is the file's too.
//!
//! The pixel under the pointer is marked here a channel at a time: a rule
//! on each plane, and a line under the header with each channel's value in
//! the units the file keeps it in, as the bottom bar's Decimal readout
//! writes them.

use egui::{Align2, FontId};

use super::plot::{bars, corners};
use super::*;
use crate::image::Samples;
use crate::ui::pixel::component;

/// The line under the header that the pixel's values are written on: as
/// tall as a line of the words it is set in, and a cell to a channel.
pub(super) const VALUES_HEIGHT: f32 = 16.0;
/// How big the dot before each value is, and the gap between it and the
/// number.
const DOT_RADIUS: f32 = 3.5;
const DOT_GAP: f32 = 4.0;

/// The cells of a pixel's values: red, green, blue and luminance, in that
/// order, each always where it is, so that the numbers do not move about as
/// the pointer crosses the picture.
const CELLS: usize = 4;

/// The line the pixel's values are written on, in the section laid out at
/// `rect`.
pub(super) fn values_line(rect: Rect) -> Rect {
    Rect::new(
        rect.x,
        rect.y + LABEL_HEIGHT + HEAD_GAP,
        rect.width,
        VALUES_HEIGHT,
    )
}

/// Whether the file's samples are floats, which are written as numbers
/// rather than as counts.
fn float(current: &Current) -> bool {
    matches!(current.image.samples, Samples::F32 { .. })
}

/// A value on the plot's axis — which is the file's samples as fractions of
/// full scale — in the units the file keeps them in: counts for integer
/// samples, the number itself for floats, as [`component`] writes them.
fn stored_words(current: &Current, axis: f32) -> String {
    component(axis * current.image.samples.full_scale(), float(current))
}

/// What the section says with the pointer on its plot at `bin`: the value
/// the bin stands for, in the units the file keeps its samples in.
pub(super) fn bin_mark(current: &Current, bin: usize) -> Mark {
    Mark {
        bin: Some(bin),
        words: stored_words(current, bin_value(&current.stored.plot, bin)),
        ..Mark::default()
    }
}

/// What the section says with the pointer on the picture's pixel `(x, y)`:
/// the pixel as the file stores it, a channel at a time — red, green and
/// blue as the file holds them, and the luminance worked out from them as
/// the plot's luminance plane is, put back on the file's curve and scale —
/// each with the bin of its own plane. A gray file has the one channel.
pub(super) fn pixel_mark(current: &Current, x: u32, y: u32) -> Option<Mark> {
    let sample = current.sample_as_stored(x, y)?;
    let image = &current.image;
    let plot = &current.stored.plot;
    let scale = 1.0 / image.samples.full_scale();
    let mut channels = Vec::with_capacity(CELLS);
    if !image.is_gray() {
        for (plane, stored) in Plane::COLOR.into_iter().zip(sample.stored()) {
            channels.push(Channel {
                plane,
                bin: plot.bin_at(stored * scale),
                words: component(*stored, float(current)),
            });
        }
    }
    let luma = Plot::value_of(image, &sample)?;
    channels.push(Channel {
        plane: Plane::Luma,
        bin: plot.bin_at(luma),
        words: stored_words(current, luma),
    });
    Some(Mark {
        channels,
        ..Mark::default()
    })
}

/// The Image section, laid out at `rect`: the header, the line of the
/// pixel's values under it, and the plot of the picture as stored, with a
/// rule in each plane's color for the pixel's channels and the ends of its
/// axis in its corners where they are worth writing.
pub(super) fn show(
    pass: &mut Pass,
    ui: &mut egui::Ui,
    current: &Current,
    rect: Rect,
    mark: Option<&Mark>,
) {
    let plot_rect = Section::Image.plot(rect, current.image.is_gray());
    let plotted = &current.stored.plot;
    // A rule for each channel whose plane is on screen, the luminance last
    // so that it stands over the colors where they meet; and the accent's
    // one rule where the pointer is on the plot instead.
    let rules: Vec<(usize, Color32)> = match mark {
        Some(mark) if !mark.channels.is_empty() => mark
            .channels
            .iter()
            .filter(|channel| channel.plane.shown(pass.panels))
            .filter_map(|channel| Some((channel.bin?, channel.plane.ink())))
            .collect(),
        _ => accent_rule(pass, mark),
    };
    bars(pass, ui, plotted, plot_rect, false, &rules);
    // The ends in the same units as the values: counts, or the number.
    let ends = axis_ends(current, plotted)
        .map(|_| [plotted.min, plotted.max].map(|end| stored_words(current, end)));
    if let Some([low, high]) = ends {
        let dim = Color32::from(pass.theme.text_dim);
        corners(ui, plot_rect, Some((low, dim)), Some((high, dim)));
    }
    section::header(pass, ui, Section::Image, rect, None);
    if let Some(mark) = mark {
        values(pass, ui, values_line(rect), mark);
    }
}

/// The line of the pixel's values: a cell to each channel, a dot in the
/// plane's ink and the number after it; or, with the pointer on the plot,
/// the value of the bin under it, at the head of the line.
fn values(pass: &Pass, ui: &egui::Ui, line: Rect, mark: &Mark) {
    let painter = ui.painter();
    let font = FontId::proportional(ROW_TEXT);
    let ink: Color32 = pass.theme.text_primary.into();
    let middle = line.y + line.height / 2.0;
    if mark.channels.is_empty() {
        painter.text(
            pos2(pass.grid.snap(line.x), middle),
            Align2::LEFT_CENTER,
            &mark.words,
            font,
            ink,
        );
        return;
    }
    for (index, channel) in mark.channels.iter().enumerate() {
        let cell = share(line, CELLS, index);
        painter.circle_filled(
            pos2(cell.x + DOT_RADIUS, middle),
            DOT_RADIUS,
            channel.plane.ink(),
        );
        painter.text(
            pos2(pass.grid.snap(cell.x + 2.0 * DOT_RADIUS + DOT_GAP), middle),
            Align2::LEFT_CENTER,
            &channel.words,
            font.clone(),
            ink,
        );
    }
}
