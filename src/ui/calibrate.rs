//! The print size's calibration: a modal with a credit card's outline drawn
//! at the size the monitor is taken to be, a slider that grows and shrinks
//! it, the monitor's diagonal that size makes, and OK and Cancel. The button
//! beside the zoom menu's print size opens it; what it does is the
//! application's — see `app::calibration`.
//!
//! The card is drawn at the size it should be on the glass, in the
//! interface's points, so it does not follow the interface's scale: a card
//! is the size it is. Inside it, in two opposite corners, stand two rules
//! square to the card's edges — two inches by one, and forty millimeters by
//! twenty — for a ruler held to the glass where there is no card to hand,
//! and to check that the two axes come out alike. Its room is the size of the card at the slider's far
//! end, so that the dialog does not grow and shrink under the hand, and a
//! card larger than the window allows is cut at the room's edge.

use egui::{Label, RichText, Sense, vec2};

use super::chrome::Pass;
use super::control::{Command, Control};
use super::icon;
use super::rename::{self, Dialog, GAP};
use super::slider::{self, Line};
use super::style::MENU_PADDING;
use super::style::TOGGLE_RADIUS;
use super::tooltip::Tip;
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
/// The slider's row: how tall, and how much of it the readout and the
/// reset after the slider are given — the reset a square the row's height,
/// wearing the mark the histogram's reset wears.
const ROW: f32 = 24.0;
const READOUT: f32 = 100.0;
const RESET_ICON: f32 = 16.0;
/// How much of the window's height the rest of the dialog takes — its
/// title, the words, the slider, the buttons and the gaps between — which
/// the card's room is cut to fit beside.
const REST: f32 = 190.0;
/// How far the card's fill is faded toward the dialog's ground.
const FILL_ALPHA: u8 = 60;

/// What the dialog says above the card, and under that, either side of the
/// two arrow keys drawn as marks: the interface's face has no arrows of its
/// own.
const HOLD: &str = "Hold a credit card or ruler to the screen and adjust the slider until \
                    they match.";
const USE: &str = "Use";
const FINELY: &str = "to finely adjust.";
/// How large the arrows' marks are drawn, beside words of [`TEXT_SIZE`].
const ARROW: f32 = 14.0;

/// One of the rules inside the card: where its corner is, in millimeters
/// from the card's top left; how long its two legs are, signed the way each
/// runs from the corner, across and down; what one of its divisions is, in
/// millimeters, and how many make the middle mark and the long one; and
/// what it is labeled.
struct Rule {
    corner: [f32; 2],
    legs: [f32; 2],
    division: f32,
    middle: u32,
    long: u32,
    label: &'static str,
}

/// How far each rule's corner stands in from the card's edges.
const INSET: f32 = 5.0;
/// How long a rule's marks are, in millimeters: a division, the middle one
/// and the long one.
const MARKS: [f32; 3] = [1.4, 2.2, 3.0];
/// How far the label stands from the rule's corner, in millimeters into
/// its angle.
const LABEL_OFFSET: f32 = 5.0;

/// The two rules: two inches by one in eighths at the top left, and forty
/// millimeters by twenty at the bottom right.
const RULES: [Rule; 2] = [
    Rule {
        corner: [INSET, INSET],
        legs: [2.0 * 25.4, 25.4],
        division: 25.4 / 8.0,
        middle: 4,
        long: 8,
        label: "2\" \u{00d7} 1\"",
    },
    Rule {
        corner: [CARD[0] - INSET, CARD[1] - INSET],
        legs: [-40.0, -20.0],
        division: 1.0,
        middle: 5,
        long: 10,
        label: "40 \u{00d7} 20 mm",
    },
];
/// The slider's name, and what the card says in its middle.
const SIZE: &str = "Card size";
const CREDIT_CARD: &str = "Credit card";

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
            arrows(pass, ui);
            ui.add_space(GAP);
            card(pass, ui, input, [inside, room[1]]);
            ui.add_space(GAP);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(GAP, 0.0);
                let width = (inside - READOUT - ROW - 2.0 * GAP).max(0.0);
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
                ui.add_sized([READOUT, ROW], Label::new(diagonal(input.diagonal)));
                reset(pass, ui, input);
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
    format!("{inches:.1}\" diagonal")
}

/// The button after the readout that puts the slider back on the size the
/// monitor says: dead while it is there.
fn reset(pass: &mut Pass, ui: &mut egui::Ui, input: &Input) {
    let control = Control::CalibrateReset;
    let live = input.stops != 0.0;
    let sense = if live { Sense::CLICK } else { Sense::HOVER };
    let (rect, response) = ui.allocate_exact_size(vec2(ROW, ROW), sense);
    let (background, ink) = pass.button_ink(false, &response, live);
    ui.painter().rect_filled(rect, TOGGLE_RADIUS, background);
    icon::paint(
        ui.painter(),
        icon::ROTATE_CCW,
        icon::square(pass.grid, rect, RESET_ICON),
        ink,
        background,
    );
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, live, control.label()));
    let response = pass.tooltip(response, Tip::Control(control));
    if response.clicked() {
        pass.press(control);
    }
}

/// The line under the words: "Use ← → to finely adjust.", the arrows drawn
/// as the marks the buttons wear.
fn arrows(pass: &Pass, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 0.0);
        ui.add(Label::new(RichText::new(USE).size(TEXT_SIZE)));
        for marks in [icon::ARROW_LEFT, icon::ARROW_RIGHT] {
            let (rect, _) = ui.allocate_exact_size(vec2(ARROW, ARROW), Sense::HOVER);
            let ground = pass.theme.menu_background;
            icon::paint(
                ui.painter(),
                marks,
                icon::square(pass.grid, rect, ARROW),
                pass.theme.text_primary.into(),
                ground.into(),
            );
        }
        ui.add(Label::new(RichText::new(FINELY).size(TEXT_SIZE)));
    });
}

/// The card, filled and outlined, in the middle of `room`, and cut at its
/// edge, with its two rules.
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
    painter.text(
        card.center(),
        egui::Align2::CENTER_CENTER,
        CREDIT_CARD,
        egui::FontId::proportional(TEXT_SIZE),
        theme.text_dim.into(),
    );

    // Every line of a rule is put on the device's grid, as any rule is: a
    // tick a pixel wide either side of a pixel is two gray pixels, and the
    // half a device pixel it moves is under a tenth of a millimeter.
    let grid = pass.grid;
    let crisp = |at: f32| grid.stroke_center_in_device(at * grid.scale(), line);
    let ppm = input.points_per_mm;
    let at = |mm: [f32; 2]| [card.min.x + mm[0] * ppm[0], card.min.y + mm[1] * ppm[1]];
    let stroke = egui::Stroke::new(line, theme.text_primary);
    for rule in &RULES {
        let sign = rule.legs.map(f32::signum);
        let corner = at(rule.corner);
        let ends = at([rule.corner[0] + rule.legs[0], rule.corner[1] + rule.legs[1]]);
        let [x, y] = [crisp(corner[0]), crisp(corner[1])];
        // The two legs, and their marks standing into the rule's angle.
        painter.line_segment([egui::pos2(x, y), egui::pos2(ends[0], y)], stroke);
        painter.line_segment([egui::pos2(x, y), egui::pos2(x, ends[1])], stroke);
        for axis in 0..2 {
            let count = (rule.legs[axis].abs() / rule.division).round() as u32;
            for index in 0..=count {
                let length = if index % rule.long == 0 {
                    MARKS[2]
                } else if index % rule.middle == 0 {
                    MARKS[1]
                } else {
                    MARKS[0]
                };
                let along = rule.corner[axis] + sign[axis] * rule.division * index as f32;
                let across = 1 - axis;
                let mut from = rule.corner;
                from[axis] = along;
                let mut to = from;
                to[across] += sign[across] * length;
                let (from, to) = (at(from), at(to));
                let (from, to) = if axis == 0 {
                    let x = crisp(from[0]);
                    ([x, y], [x, to[1]])
                } else {
                    let y = crisp(from[1]);
                    ([x, y], [to[0], y])
                };
                painter.line_segment([from.into(), to.into()], stroke);
            }
        }
        // The label in the angle, set from the corner as the legs run.
        let anchor = at([
            rule.corner[0] + sign[0] * LABEL_OFFSET,
            rule.corner[1] + sign[1] * LABEL_OFFSET,
        ]);
        let align = if sign[0] > 0.0 {
            egui::Align2::LEFT_TOP
        } else {
            egui::Align2::RIGHT_BOTTOM
        };
        painter.text(
            anchor.into(),
            align,
            rule.label,
            egui::FontId::proportional(TEXT_SIZE),
            theme.text_primary.into(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_diagonal_is_written_to_a_tenth_of_an_inch() {
        assert_eq!(diagonal(15.302), "15.3\" diagonal");
        assert_eq!(diagonal(27.0), "27.0\" diagonal");
    }
}
