//! The File section: the picture as the file stores it — no lift, and in
//! its own primaries — at the foot of the column, where everything above it
//! starts from.

use super::plot::{bars, corners};
use super::*;

/// What the section says with the pointer on its plot at `bin`: the value
/// the bin stands for, in the units the file counts in.
pub(super) fn bin_mark(current: &Current, bin: usize) -> Mark {
    Mark {
        bin: Some(bin),
        words: axis_words(current, bin_value(&current.stored.plot, bin)),
    }
}

/// What the section says with the pointer on the picture's pixel `(x, y)`:
/// the pixel as the file stores it, in the units it counts in.
pub(super) fn pixel_mark(current: &Current, x: u32, y: u32) -> Option<Mark> {
    let sample = current.sample_as_stored(x, y)?;
    let encoded = Plot::value_of(&current.image, &sample)?;
    Some(Mark {
        bin: current.stored.plot.bin_of(&current.image, &sample),
        words: axis_words(current, encoded),
    })
}

/// The File section, laid out at `rect`: the plot of the picture as stored,
/// with the ends of its axis in its corners where they are worth writing.
pub(super) fn show(
    pass: &mut Pass,
    ui: &mut egui::Ui,
    current: &Current,
    rect: Rect,
    mark: Option<&Mark>,
) {
    let plot_rect = Section::File.plot(rect, current.image.is_gray());
    let plotted = &current.stored.plot;
    bars(
        pass,
        ui,
        plotted,
        plot_rect,
        false,
        mark.and_then(|mark| mark.bin),
    );
    if let Some([low, high]) = axis_ends(current, plotted) {
        let dim = Color32::from(pass.theme.text_dim);
        corners(ui, plot_rect, Some((low, dim)), Some((high, dim)));
    }
    section::header(
        pass,
        ui,
        Section::File,
        rect,
        mark.map(|mark| mark.words.as_str()),
    );
}
