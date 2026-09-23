//! The exposure's slider under the band.

use super::*;
use crate::ui::slider::{self as line, Line};

/// The exposure's slider: the interface's one slider (see `ui::slider`),
/// from nothing each way, its fill running from the mark at nothing so
/// that the push reads as a length and a direction before the number is
/// read. Snapped to the quarter stops the keys count in, so that the
/// reading beside it is always one the keys could have reached, and asked
/// for through [`Command::Exposure`].
///
/// The run is [`SLIDER_STOPS`] each way. An exposure past that, from the
/// keys or `--exposure`, stands hollow at the end, as a band handle out past
/// the plot does.
pub(super) fn slider(pass: &mut Pass, ui: &mut egui::Ui, exposure: f32, room: Rect) {
    let ground = pass.theme.panel_background;
    let asked = line::show(
        pass,
        ui,
        Line {
            name: "Exposure",
            value: exposure,
            low: -SLIDER_STOPS,
            high: SLIDER_STOPS,
            step: EV_STEP,
            origin: 0.0,
            tip: Some(Tip::Exposure),
            live: true,
            ground,
        },
        room,
    );
    if let Some(asked) = asked {
        pass.commands.push(Command::Exposure(asked));
    }
}
