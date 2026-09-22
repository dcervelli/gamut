//! What the content area shows with nothing open: the buttons that give
//! the window something.
//!
//! A program started with no path on its command line opens on this, and
//! comes back to it when everything it was handed failed to open, or the
//! last file was deleted. Three buttons, one under the other in the middle
//! of the area the picture would fill: the desktop's file dialog for image
//! files, the same dialog for a folder, and a paste of the picture on the
//! clipboard. Each wears a mark at its head, the key that does the same
//! thing from anywhere at its foot — printed the way the menus print
//! theirs — and each is a press that goes through the same [`Control`] its
//! key does, so the two cannot drift.
//!
//! Two buttons for the dialog rather than one, because that is how every
//! desktop's dialog is built: it picks files, or it picks a folder, and a
//! program that wanted both would have to put up two. The paste button
//! stays where it is whether or not the clipboard holds a picture — dead,
//! and saying why — so that the empty window always offers the same three
//! things in the same places, where the strip's own paste button comes and
//! goes with the clipboard; the strip's is left out while this one is up,
//! one control being enough for one thing.
//!
//! Drawn by hand, as the toggles in the strips are, rather than as egui's
//! own button: a mark wants placing on the device's grid to come out
//! sharp, and the label and the key each want an ink of their own.

use egui::{Align, Layout, Sense, WidgetInfo, WidgetType, pos2, vec2};

use super::chrome::{ICON_SIDE, Pass};
use super::control::Control;
use super::icon::{self, Mark};
use super::style::TOGGLE_RADIUS;
use super::tooltip::Tip;
use super::{Rect, panel};

/// One of the three buttons: wide enough for the longest of their labels
/// with its mark before it and its key after it, and taller than a button
/// in a bar, since these are the whole of what is on screen and are meant
/// to be found.
const BUTTON: [f32; 2] = [236.0, 36.0];
/// The gap between one button and the next.
const GAP: f32 = 10.0;
/// The room inside a button's ends: what the mark stands in from the left
/// and the key from the right.
const INSET: f32 = 10.0;
/// The room set aside for the mark, and the gap between it and the label.
const MARK: f32 = ICON_SIDE + 2.0;
const MARK_GAP: f32 = 10.0;
/// The size the labels are set in: a step up from the bars' text, for
/// the same reason the buttons are larger.
const LABEL_SIZE: f32 = 15.0;

/// The height the column takes, for deciding whether there is room for it.
const COLUMN_HEIGHT: f32 = 3.0 * BUTTON[1] + 2.0 * GAP;

/// Where the column goes: centered on `content`, or `None` where the area
/// is too small to hold it — a window dragged down to nothing, where the
/// keys still work and the buttons would only be cut off.
pub fn panel(content: Rect) -> Option<Rect> {
    let size = [BUTTON[0], COLUMN_HEIGHT];
    panel::fit(content, size, size, panel::Place::Center)
}

/// Draws the three buttons, and reads what was pressed.
pub(super) fn show(pass: &mut Pass, ui: &mut egui::Ui) {
    let content = pass.content;
    let Some(panel) = panel(content) else {
        return;
    };
    let area = egui::Rect::from_min_size(
        egui::pos2(panel.x, panel.y),
        vec2(panel.width, panel.height),
    );
    panel::area("empty", panel, egui::Order::Middle).show(ui.ctx(), |ui| {
        ui.set_min_size(area.size());
        ui.set_max_size(area.size());
        ui.with_layout(Layout::top_down(Align::Center), |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, GAP);
            let picking = pass.input.picking;
            button(
                pass,
                ui,
                Control::OpenFiles,
                icon::FILE_IMAGE,
                "Open files\u{2026}",
                !picking,
            );
            button(
                pass,
                ui,
                Control::OpenFolder,
                icon::FOLDER,
                "Open folder\u{2026}",
                !picking,
            );
            button(
                pass,
                ui,
                Control::Paste,
                icon::CLIPBOARD,
                "Paste",
                pass.panels.paste,
            );
        });
    });
}

/// One button: its mark at the head, its label after that, and the key
/// that does the same job at the foot where a key does, in the ink the
/// menus print theirs in. The tooltip names it — or, dead, says why.
fn button(
    pass: &mut Pass,
    ui: &mut egui::Ui,
    control: Control,
    marks: &[Mark],
    label: &str,
    enabled: bool,
) {
    // A dead button senses nothing but the pointer resting on it, as a
    // dead toggle does: the press is refused, and the label says why.
    let sense = if enabled { Sense::CLICK } else { Sense::HOVER };
    let (rect, response) = ui.allocate_exact_size(vec2(BUTTON[0], BUTTON[1]), sense);
    let (background, ink) = pass.button_ink(false, &response, enabled);
    ui.painter().rect_filled(rect, TOGGLE_RADIUS, background);

    let grid = pass.grid;
    let mark = egui::Rect::from_center_size(
        pos2(rect.min.x + INSET + MARK / 2.0, rect.center().y),
        vec2(MARK, MARK),
    );
    icon::paint(
        ui.painter(),
        marks,
        icon::square(grid, mark, ICON_SIDE),
        ink,
        background,
    );

    // No ink of its own for either galley: each is drawn in an ink handed
    // to the painter below, and a color set here would be baked in.
    let label = ui.ctx().fonts_mut(|fonts| {
        fonts.layout_no_wrap(
            label.to_string(),
            egui::FontId::proportional(LABEL_SIZE),
            egui::Color32::PLACEHOLDER,
        )
    });
    ui.painter().galley(
        pos2(
            mark.max.x + MARK_GAP,
            rect.center().y - label.size().y / 2.0,
        ),
        label,
        ink,
    );
    if let Some(key) = pass.namer.shortcut(control) {
        let font = egui::TextStyle::Button.resolve(ui.style());
        let key = ui
            .ctx()
            .fonts_mut(|fonts| fonts.layout_no_wrap(key, font, egui::Color32::PLACEHOLDER));
        // The key is the quieter of the two, as it is beside a menu item:
        // the dim ink while the button is live, the button's own while
        // it is dead, there being nothing quieter than that.
        let key_ink = match enabled {
            true => pass.theme.text_dim.into(),
            false => ink,
        };
        ui.painter().galley(
            pos2(
                rect.max.x - INSET - key.size().x,
                rect.center().y - key.size().y / 2.0,
            ),
            key,
            key_ink,
        );
    }

    // Named by the control rather than by the words on it, which trail
    // off: what the accessibility tree and the tests reach it by.
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, control.label()));
    let response = pass.tooltip(response, Tip::Control(control), enabled);
    if response.clicked() {
        pass.press(control);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::PADDING;

    /// The column sits in the middle of the area, and is not drawn at all
    /// where the area cannot hold it.
    #[test]
    fn the_column_is_centered_or_absent() {
        let content = Rect::new(30.0, 30.0, 940.0, 640.0);
        let panel = panel(content).expect("room for it");
        assert_eq!(panel.width, BUTTON[0]);
        assert_eq!(panel.height, COLUMN_HEIGHT);
        assert!((panel.x + panel.width / 2.0 - (content.x + content.width / 2.0)).abs() < 0.01);
        assert!((panel.y + panel.height / 2.0 - (content.y + content.height / 2.0)).abs() < 0.01);

        assert!(panel_fits(
            BUTTON[0] + 2.0 * PADDING,
            COLUMN_HEIGHT + 2.0 * PADDING
        ));
        assert!(!panel_fits(BUTTON[0] + 2.0 * PADDING - 1.0, 1000.0));
        assert!(!panel_fits(1000.0, COLUMN_HEIGHT + 2.0 * PADDING - 1.0));
    }

    fn panel_fits(width: f32, height: f32) -> bool {
        panel(Rect::new(0.0, 0.0, width, height)).is_some()
    }
}
