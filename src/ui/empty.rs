//! What the content area shows with nothing open: the buttons that give
//! the window something.
//!
//! A program started with no path on its command line opens on this, and
//! comes back to it when everything it was handed failed to open. Three
//! buttons, one under the other in the middle of the area the picture
//! would fill: the desktop's file dialog for image files, the same dialog
//! for a folder, and a paste of the picture on the clipboard. Each wears
//! the key that does the same thing from anywhere, printed the way the
//! menus print theirs, and each is a press that goes through the same
//! [`Control`] its key does, so the two cannot drift.
//!
//! Two buttons for the dialog rather than one, because that is how every
//! desktop's dialog is built: it picks files, or it picks a folder, and a
//! program that wanted both would have to put up two. The paste button
//! stays where it is whether or not the clipboard holds a picture — dead,
//! and saying why — so that the empty window always offers the same three
//! things in the same places, where the strip's own paste button comes and
//! goes with the clipboard; the strip's is left out while this one is up,
//! one control being enough for one thing.

use egui::{Align, Button, Layout, RichText, WidgetInfo, WidgetType, vec2};

use super::chrome::Pass;
use super::control::Control;
use super::tooltip::Tip;
use super::{PADDING, Rect};

/// One of the three buttons: wide enough for the longest of their labels
/// with its key beside it, and taller than a button in a bar, since these
/// are the whole of what is on screen and are meant to be found.
const BUTTON: [f32; 2] = [220.0, 34.0];
/// The gap between one button and the next.
const GAP: f32 = 10.0;
/// The size the labels are set in: a step up from the bars' text, for
/// the same reason the buttons are larger.
const LABEL_SIZE: f32 = 15.0;

/// The height the column takes, for deciding whether there is room for it.
const COLUMN_HEIGHT: f32 = 3.0 * BUTTON[1] + 2.0 * GAP;

/// Where the column goes: centered on `content`, or `None` where the area
/// is too small to hold it — a window dragged down to nothing, where the
/// keys still work and the buttons would only be cut off.
pub fn panel(content: Rect) -> Option<Rect> {
    let width = BUTTON[0];
    if content.width < width + 2.0 * PADDING || content.height < COLUMN_HEIGHT + 2.0 * PADDING {
        return None;
    }
    Some(Rect::new(
        content.x + (content.width - width) / 2.0,
        content.y + (content.height - COLUMN_HEIGHT) / 2.0,
        width,
        COLUMN_HEIGHT,
    ))
}

/// Draws the three buttons, and reads what was pressed.
pub(super) fn show(pass: &mut Pass, ui: &mut egui::Ui, content: Rect) {
    let Some(panel) = panel(content) else {
        return;
    };
    let area = egui::Rect::from_min_size(
        egui::pos2(panel.x, panel.y),
        vec2(panel.width, panel.height),
    );
    egui::Area::new(egui::Id::new("empty"))
        .order(egui::Order::Middle)
        .fixed_pos(area.min)
        .interactable(true)
        .show(ui.ctx(), |ui| {
            ui.set_min_size(area.size());
            ui.set_max_size(area.size());
            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, GAP);
                let picking = pass.input.picking;
                button(pass, ui, Control::OpenFiles, "Open files\u{2026}", !picking);
                button(
                    pass,
                    ui,
                    Control::OpenFolder,
                    "Open folder\u{2026}",
                    !picking,
                );
                button(pass, ui, Control::Paste, "Paste", pass.panels.paste);
            });
        });
}

/// One button, with its key printed after the label where a key does the
/// same job, and the tooltip that names it — or, dead, says why.
fn button(pass: &mut Pass, ui: &mut egui::Ui, control: Control, label: &str, enabled: bool) {
    let mut button = Button::new(RichText::new(label).size(LABEL_SIZE));
    if let Some(key) = pass.namer.shortcut(control) {
        button = button.shortcut_text(key);
    }
    let response = ui
        .add_enabled_ui(enabled, |ui| ui.add_sized(BUTTON, button))
        .inner;
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
