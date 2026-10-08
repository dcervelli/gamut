//! The side panel: the column down the right of the picture that the
//! histogram and the information are shown in, one at a time.
//!
//! Part of the chrome, as the file list is on the other side, rather than
//! floating over the picture: the picture is fitted into what it leaves, so
//! that nothing is drawn over the image to say something about it. The
//! histogram's and the information's buttons in the right strip choose which
//! of the two it holds, and the one already shown takes it down again.

use egui::Sense;

use super::Rect;
use super::chrome::Pass;
use super::control::{Command, Control};

/// What the side panel holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Histogram,
    Info,
}

impl Side {
    /// The button that chooses it.
    pub fn control(self) -> Control {
        match self {
            Side::Histogram => Control::Histogram,
            Side::Info => Control::Info,
        }
    }
}

/// The panel at its narrowest: the histogram at its own width, a logical
/// pixel to a bin with its buttons beside the plot, which is the one thing
/// on either tab that cannot give. The information's column is read at any
/// width, and narrower than this it would be a few words a line.
pub const WIDTH_MIN: f32 = super::PANEL_WIDTH;
/// The width it opens at: the narrowest, which is the most it leaves the
/// picture.
pub const WIDTH_DEFAULT: f32 = WIDTH_MIN;
/// The panel at its widest: room for the plot at two device pixels to a bin
/// on a display of one, the widest the plot is drawn at — see
/// [`super::histogram::panel`] — and about as long a line as the column is
/// read at comfortably. Past that, a wider panel would be taking the
/// picture's room to give the words more of a line than they can use.
pub const WIDTH_MAX: f32 = super::PANEL_WIDTH + crate::image::stats::BINS as f32;
/// How far either side of the panel's left edge a drag takes hold of it.
const GRIP: f32 = 3.0;

/// The width a panel whose edge is dragged to `width` has, held between
/// [`WIDTH_MIN`] and [`WIDTH_MAX`] and put on a whole logical pixel.
pub fn width_for(width: f32) -> f32 {
    width.round().clamp(WIDTH_MIN, WIDTH_MAX)
}

/// The side panel as the application hands it to a frame, on every frame it
/// is on screen.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Shown {
    /// What it holds.
    pub side: Side,
    /// How wide it is asked to be, which the window may leave it less of.
    pub width: f32,
}

/// The grip along the panel's left edge, a little either side of it: a drag
/// of it asks for the width that puts the edge under the pointer, which the
/// application holds between [`WIDTH_MIN`] and [`WIDTH_MAX`].
pub(super) fn grip(pass: &mut Pass, ui: &mut egui::Ui, width: f32) {
    let Some(panel) = pass.side_panel else {
        return;
    };
    let rect =
        egui::Rect::from_x_y_ranges(panel.left() - GRIP..=panel.left() + GRIP, panel.y_range());
    let response = ui.interact(rect, egui::Id::new("side grip"), Sense::DRAG);
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    if response.dragged()
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let asked = width_for(panel.right() - pointer.x);
        if asked != width {
            pass.commands.push(Command::SideWidth(asked));
        }
    }
}

/// What the panel holds, laid out in `panel`: the histogram at its head, or
/// the information down the whole of it. Nothing while there is no picture
/// to say anything about, and a spinner in place of what it will say while
/// another file is read slowly enough to say so.
pub(super) fn show(pass: &mut Pass, ui: &mut egui::Ui, side: Side, panel: Rect) {
    let Some(current) = pass.current else {
        return;
    };
    if pass.input.waiting {
        let name = match side {
            Side::Histogram => Some("Histogram panel"),
            Side::Info => None,
        };
        super::panel::spinner(ui, panel, name, pass.theme);
        return;
    }
    match side {
        Side::Histogram => super::histogram::show(pass, ui, current, panel),
        Side::Info => super::info::show(pass, ui, current, panel),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A drag asks for a whole logical pixel, and never for less than the
    /// histogram needs or more than it can use.
    #[test]
    fn a_dragged_width_is_held_between_the_least_and_the_most() {
        assert_eq!(width_for(0.0), WIDTH_MIN);
        assert_eq!(width_for(10_000.0), WIDTH_MAX);
        assert_eq!(width_for(WIDTH_MIN + 40.4), WIDTH_MIN + 40.0);
        const { assert!(WIDTH_MIN <= WIDTH_DEFAULT && WIDTH_DEFAULT <= WIDTH_MAX) };
    }
}
