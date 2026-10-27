//! The print size's calibration: a modal with a credit card's outline drawn
//! at the size the monitor is taken to be, a slider that grows and shrinks
//! it, the monitor's diagonal that size makes, and OK and Cancel. The button
//! beside the zoom menu's print size opens it; what it does is the
//! application's — see `app::calibration`.
//!
//! The card is drawn at the size it should be on the glass, in the
//! interface's points, so it does not follow the interface's scale: a card
//! is the size it is. Its room is the size of the card at the slider's far
//! end, so that the dialog does not grow and shrink under the hand, and a
//! card larger than the window allows is cut at the room's edge.

use egui::{Label, RichText, Sense, vec2};

use super::chrome::Pass;
use super::control::{Command, Control};
use super::rename::{self, Dialog, GAP};
use super::slider::{self, Line};
use super::style::MENU_PADDING;
use super::{PADDING, Rect, TEXT_SIZE};

/// The modal's id in egui's memory.
pub fn id() -> egui::Id {
    egui::Id::new("calibrate")
}

/// A credit card — ISO/IEC 7810 ID-1 — in millimeters: its sides, and the
/// round of its corners.
const CARD: [f32; 2] = [85.60, 53.98];
const CARD_RADIUS: f32 = 3.18;

/// The dialog's least width, which the words above the card are set in.
const WIDTH: f32 = 400.0;
/// The slider's row: how tall, and how much of it the readout after the
/// slider is given.
const ROW: f32 = 24.0;
const READOUT: f32 = 110.0;
/// How much of the window's height the rest of the dialog takes — its
/// title, the words, the slider, the buttons and the gaps between — which
/// the card's room is cut to fit beside.
const REST: f32 = 190.0;
/// How far the card's fill is faded toward the dialog's ground.
const FILL_ALPHA: u8 = 60;

/// What the dialog says above the card.
const HOLD: &str = "Hold a credit card to the screen and move the slider until the outline \
                    matches it. The arrow keys move it in fine steps.";
/// The slider's name.
const SIZE: &str = "Card size";

/// What the dialog is drawn from, on each frame it is up.
#[derive(Clone, Debug, PartialEq)]
pub struct Input {
    /// The interface's points to a millimeter, across and down, at the
    /// size the slider says the monitor is.
    pub points_per_mm: [f32; 2],
    /// Where the slider stands, and the ends of its run, in stops of the
    /// card's size from the size the monitor says — or the guess, where it
    /// says none.
    pub stops: f32,
    pub low: f32,
    pub high: f32,
    pub step: f32,
    /// The monitor's diagonal in inches at that size.
    pub diagonal: f64,
}

pub(super) fn show(pass: &mut Pass, ui: &mut egui::Ui, input: &Input) {
    // The card's room is the card at the far end of the run.
    let largest = (input.high - input.stops).exp2();
    let room = [0, 1].map(|axis| CARD[axis] * input.points_per_mm[axis] * largest);
    let window = pass.input.logical;
    let room = [
        room[0].min(window[0] - 2.0 * (PADDING + MENU_PADDING)),
        room[1].min(window[1] - REST).max(0.0),
    ];
    rename::modal(
        pass,
        ui,
        Dialog {
            id: id(),
            title: &Control::Calibrate.label(),
            width: WIDTH.max(room[0] + 2.0 * MENU_PADDING),
            cancel: Control::CancelCalibrate,
        },
        // The keys are the application's — see `app::calibration`.
        |_, _| {},
        |pass, ui, inside| {
            ui.add(Label::new(RichText::new(HOLD).size(TEXT_SIZE)).wrap());
            ui.add_space(GAP);
            card(pass, ui, input, [inside, room[1]]);
            ui.add_space(GAP);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(GAP, 0.0);
                let width = (inside - READOUT - GAP).max(0.0);
                let (row, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::HOVER);
                let asked = slider::show(
                    pass,
                    ui,
                    Line {
                        name: SIZE,
                        value: input.stops,
                        low: input.low,
                        high: input.high,
                        step: input.step,
                        // The size the monitor says, from which the fill runs
                        // and where its mark stands: how far the card is from
                        // what the monitor says.
                        origin: 0.0,
                        tip: None,
                        live: true,
                        ground: pass.theme.menu_background,
                    },
                    Rect::new(row.left(), row.top(), row.width(), row.height()),
                );
                if let Some(asked) = asked {
                    pass.commands.push(Command::Calibration(asked));
                }
                ui.add(Label::new(diagonal(input.diagonal)));
            });
            ui.add_space(GAP);
            rename::ok_cancel(
                pass,
                ui,
                Control::CalibrateOk,
                Control::CancelCalibrate,
                true,
            );
        },
    );
}

/// What the readout after the slider says: the monitor's diagonal, which is
/// what a monitor is sold by and so a number the reader can check.
pub fn diagonal(inches: f64) -> String {
    format!("{inches:.1} in diagonal")
}

/// The card, filled and outlined, in the middle of `room`, and cut at its
/// edge.
fn card(pass: &Pass, ui: &mut egui::Ui, input: &Input, room: [f32; 2]) {
    let theme = pass.theme;
    let (room, _) = ui.allocate_exact_size(room.into(), Sense::HOVER);
    let size = vec2(
        CARD[0] * input.points_per_mm[0],
        CARD[1] * input.points_per_mm[1],
    );
    let radius = CARD_RADIUS * input.points_per_mm[0];
    let card = egui::Rect::from_center_size(room.center(), size);
    let painter = ui.painter().with_clip_rect(room);
    painter.rect_filled(card, radius, theme.text_dim.with_alpha(FILL_ALPHA));
    let line = pass.grid.line_width(1.0);
    painter.rect_stroke(
        card,
        radius,
        egui::Stroke::new(line, theme.text_primary),
        egui::StrokeKind::Inside,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_diagonal_is_written_to_a_tenth_of_an_inch() {
        assert_eq!(diagonal(15.302), "15.3 in diagonal");
        assert_eq!(diagonal(27.0), "27.0 in diagonal");
    }
}
