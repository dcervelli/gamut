//! The side panel: the column down the right of the picture that the
//! histogram and the information are shown in, one at a time.
//!
//! Part of the chrome, as the file list is on the other side, rather than
//! floating over the picture: the picture is fitted into what it leaves, so
//! that nothing is drawn over the image to say something about it. The
//! histogram's and the information's buttons in the right strip choose which
//! of the two it holds, and the one already shown takes it down again.

use super::Rect;
use super::chrome::Pass;
use super::control::Control;

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

/// How wide the panel is: the histogram at its own size, a point to a bin
/// with its buttons beside the plot, which is the one thing on either tab
/// that cannot give. The information's column is read at any width.
pub const WIDTH: f32 = super::PANEL_WIDTH;

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
