//! The help popup: every key the program answers, in the sections `--help`
//! lists them under, with what each does and when it does anything. `?`
//! and `/` open it, as does the button at the foot of the right strip; the
//! same again, `Esc`, or a click outside closes it.
//!
//! A popup like the chooser's — its open state is in egui's memory under
//! [`id`], so `Esc`, the click outside and the rule that one popup is open
//! at a time are all the toolkit's — with nothing of its own to hand back:
//! the table is read once from the application's [`Naming`] and laid out,
//! and nothing on it can be pressed.
//!
//! The table is three columns: the keys, what they do, and the condition
//! on which they do anything. A row whose condition does not hold right
//! now is dimmed whole, with the condition itself in the caution ink, so
//! that the keys that would do something are the ones that stand out.
//! The headings stay at the top while the rows scroll under them, on a
//! band of their own and parted from the rows by a hairline as the
//! information panel's column is from its header.
//!
//! [`Naming`]: super::control::Naming

use egui::{
    Frame, Label, LayerId, PopupAnchor, PopupCloseBehavior, PopupKind, RectAlign, RichText, pos2,
    vec2,
};

use super::chrome::Pass;
use super::control::{Command, Control};
use super::info::{HEADER_GAP, RULE_WIDTH, SCROLLBAR_GUTTER, SCROLLBAR_WIDTH, rule};
use super::style::{MENU_PADDING, MENU_RADIUS};
use super::{PADDING, Rect, TEXT_SIZE, fonts, info};

/// The popup's id in egui's memory: what the application opens, and what
/// it asks whether it is open.
pub fn id() -> egui::Id {
    egui::Id::new("help")
}

/// One heading of the table and the rows under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub title: &'static str,
    pub rows: Vec<Row>,
}

/// One line of the table: the keys, what they do, and on what condition
/// they do anything — `None` for a key that always does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub key: String,
    pub does: &'static str,
    pub when: Option<Condition>,
}

/// The condition a key waits on: what it is in words, and whether it holds
/// as the popup is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Condition {
    pub words: &'static str,
    pub met: bool,
}

/// The popup at most this wide and this tall.
const WIDTH_MAX: f32 = 720.0;
const HEIGHT_MAX: f32 = 640.0;
/// The least the popup will open at: the width of the two panels that
/// float over the picture, and the least height the information panel
/// takes — a title and a few stacked rows fit in that — so that the help
/// comes and goes with them, and its button is dead where theirs are.
const WIDTH_MIN: f32 = super::PANEL_WIDTH;
const HEIGHT_MIN: f32 = info::INFO_MIN_HEIGHT;
/// Inside this width the three columns of a row go one under the other
/// instead of side by side: the description's column would otherwise be
/// down to a few words a line, and narrower still it would be less than
/// nothing, which egui refuses to lay out. egui has no notion of a layout
/// that answers to its width; this is that notion, for this one table.
const STACK_BELOW: f32 = 520.0;
/// The hairline around the popup.
const HAIRLINE: f32 = 1.0;
/// The key column's width — room for `Ctrl+Shift+Arrows` in the monospace
/// face on one line — and the condition column's; what each does gets the
/// rest of the line.
const KEY_WIDTH: f32 = 150.0;
const WHEN_WIDTH: f32 = 170.0;
/// The gap between one column and the next, and between one row and the
/// next; the room a stacked row leaves under itself over that, its lines
/// being spaced as the columns' rows are and needing something more to
/// read as one row rather than three.
const COLUMN_GAP: f32 = 16.0;
const ROW_GAP: f32 = 4.0;
const STACK_GAP: f32 = 10.0;
/// The room above a section's title, parting it from the rows before it,
/// and between the title and its rows.
const SECTION_GAP: f32 = 14.0;
const TITLE_GAP: f32 = 6.0;
/// What the three columns are headed.
const HEADINGS: [&str; 3] = ["Key", "Action", "When"];

/// Where the popup goes: the middle of `content`, at most [`WIDTH_MAX`] by
/// [`HEIGHT_MAX`] and inside the padding everything floating over the image
/// keeps. `None` when the window is too small for it to be read, in which
/// case the popup stays off.
pub fn panel(content: Rect) -> Option<Rect> {
    let width = WIDTH_MAX.min(content.width - 2.0 * PADDING);
    let height = HEIGHT_MAX.min(content.height - 2.0 * PADDING);
    if width < WIDTH_MIN || height < HEIGHT_MIN {
        return None;
    }
    Some(Rect::new(
        (content.x + (content.width - width) / 2.0).round(),
        (content.y + (content.height - height) / 2.0).round(),
        width.round(),
        height.round(),
    ))
}

/// Draws the popup, if it is open.
pub(super) fn show(pass: &mut Pass, ui: &mut egui::Ui, content: Rect) {
    let Some(panel) = panel(content) else {
        return;
    };
    let theme = pass.theme;
    let frame = Frame::NONE
        .fill(theme.menu_background.into())
        .stroke(egui::Stroke::new(HAIRLINE, theme.border))
        .corner_radius(MENU_RADIUS)
        .inner_margin(MENU_PADDING);
    let inside = vec2(
        panel.width - 2.0 * MENU_PADDING,
        panel.height - 2.0 * MENU_PADDING,
    );
    let sections = pass.namer.help();
    let width = table_width(panel.width);
    egui::Popup::new(
        id(),
        ui.ctx().clone(),
        PopupAnchor::Position(pos2(panel.x, panel.y)),
        LayerId::background(),
    )
    .open_memory(None)
    .kind(PopupKind::Popup)
    .close_behavior(close_behavior(pass, Control::Help))
    .align(RectAlign::BOTTOM_START)
    .align_alternatives(&[])
    .gap(0.0)
    .width(panel.width)
    .frame(frame)
    .show(|ui| {
        ui.set_min_size(inside);
        ui.set_max_size(inside);
        ui.spacing_mut().item_spacing = vec2(COLUMN_GAP, 0.0);
        // The headings stay put above the rows, which scroll under them;
        // stacked, there are no columns for them to head. They sit on a
        // band of their own, parted from the rows by the information
        // panel's hairline under its header, which says the same: the
        // table runs on under here.
        if !stacked(width) {
            // The band the headings sit on runs edge to edge of the popup
            // and down to the hairline, inside the popup's own stroke;
            // painted first, so that the headings and the hairline go
            // over it.
            let band = ui.painter().add(egui::Shape::Noop);
            let frame = ui.max_rect().expand(MENU_PADDING).shrink(HAIRLINE);
            headings(pass, ui, width);
            ui.add_space((HEADER_GAP - RULE_WIDTH) / 2.0);
            rule(pass, ui, width);
            let rect = egui::Rect::from_min_max(frame.min, pos2(frame.max.x, ui.cursor().min.y));
            let corners = egui::CornerRadius {
                nw: (MENU_RADIUS - HAIRLINE) as u8,
                ne: (MENU_RADIUS - HAIRLINE) as u8,
                sw: 0,
                se: 0,
            };
            ui.painter()
                .set(band, egui::Shape::rect_filled(rect, corners, theme.heading));
            ui.add_space((HEADER_GAP - RULE_WIDTH) / 2.0);
        }
        // The bar down the popup's inner edge, in a gutter kept clear for
        // it as the information panel keeps one: the headings are laid out
        // to the width the rows have, and the rows' width must not depend
        // on whether the bar is showing.
        ui.spacing_mut().scroll.bar_inner_margin = SCROLLBAR_GUTTER - SCROLLBAR_WIDTH;
        egui::ScrollArea::vertical()
            .id_salt("help table")
            .auto_shrink(false)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .show(ui, |ui| table(pass, ui, &sections, width));
    });
}

/// How wide the table is in a popup `panel_width` wide: inside the
/// popup's padding, and short of the scrollbar's gutter.
fn table_width(panel_width: f32) -> f32 {
    panel_width - 2.0 * MENU_PADDING - SCROLLBAR_GUTTER
}

/// How a popup the application opens and closes takes a click: closing on
/// one outside itself, as a menu does — except on the frame its own button
/// was pressed, when the click is left alone. The button is outside the
/// popup, so egui would close it on that click, and the press the button
/// hands back, done after the frame, would then find it closed and open it
/// again. The press is the toggle; the popup only has to not get there
/// first.
pub(super) fn close_behavior(pass: &Pass, button: Control) -> PopupCloseBehavior {
    if pass.commands.contains(&Command::Press(button)) {
        PopupCloseBehavior::IgnoreClicks
    } else {
        PopupCloseBehavior::CloseOnClickOutside
    }
}

/// Whether a table `width` wide has its rows' three parts side by side or
/// one under the other — see [`STACK_BELOW`].
pub(super) fn stacked(width: f32) -> bool {
    width < STACK_BELOW
}

/// How wide the action's column is in a table `width` wide: what the two
/// fixed columns and the gaps between the three leave.
fn does_width(width: f32) -> f32 {
    width - KEY_WIDTH - WHEN_WIDTH - 2.0 * COLUMN_GAP
}

/// The three headings, over the columns they head.
fn headings(pass: &Pass, ui: &mut egui::Ui, width: f32) {
    let theme = pass.theme;
    ui.horizontal_top(|ui| {
        for (heading, column) in
            HEADINGS
                .into_iter()
                .zip([KEY_WIDTH, does_width(width), WHEN_WIDTH])
        {
            cell(
                ui,
                column,
                RichText::new(heading)
                    .family(egui::FontFamily::Name(fonts::BOLD.into()))
                    .color(theme.text_primary),
            );
        }
    });
}

/// The table: each section's title and rows. In columns, the key and the
/// condition are a fixed width and what a key does wraps in what is left;
/// stacked, each part has the whole width. A row whose condition does not
/// hold is written in the dim ink throughout, its condition in the caution
/// ink: what the row says is true, and what it needs is what is missing.
fn table(pass: &Pass, ui: &mut egui::Ui, sections: &[Section], width: f32) {
    let theme = pass.theme;
    let stacked = stacked(width);
    let does_width = does_width(width);
    ui.spacing_mut().item_spacing = vec2(COLUMN_GAP, ROW_GAP);
    for (index, section) in sections.iter().enumerate() {
        if index > 0 {
            ui.add_space(SECTION_GAP);
        }
        ui.label(
            RichText::new(section.title)
                .family(egui::FontFamily::Name(fonts::BOLD.into()))
                .size(TEXT_SIZE)
                .color(theme.text_primary),
        );
        ui.add_space(TITLE_GAP);
        for row in &section.rows {
            let met = row.when.is_none_or(|when| when.met);
            let ink = if met {
                theme.text_primary
            } else {
                theme.text_dim
            };
            let key = RichText::new(&row.key).monospace().color(ink);
            let does = RichText::new(row.does).color(ink);
            let when = row.when.map(|when| {
                RichText::new(when.words).color(if when.met {
                    theme.text_dim
                } else {
                    theme.caution
                })
            });
            if stacked {
                cell(ui, width, key);
                cell(ui, width, does);
                if let Some(when) = when {
                    cell(ui, width, when);
                }
                ui.add_space(STACK_GAP);
            } else {
                ui.horizontal_top(|ui| {
                    cell(ui, KEY_WIDTH, key);
                    cell(ui, does_width, does);
                    cell(ui, WHEN_WIDTH, when.unwrap_or_else(|| RichText::new("")));
                });
            }
        }
    }
}

/// One cell of a row: `text` wrapped in a column `width` wide, so that the
/// column after it starts where it should whatever this one says.
fn cell(ui: &mut egui::Ui, width: f32, text: RichText) {
    ui.scope(|ui| {
        ui.set_min_width(width);
        ui.set_max_width(width);
        ui.add(Label::new(text.size(TEXT_SIZE)).wrap());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A window with room to spare gets the popup at its full size, in the
    /// middle of the content area.
    #[test]
    fn the_popup_is_centered_and_capped() {
        let content = Rect::new(0.0, 30.0, 1600.0, 1000.0);
        let panel = panel(content).expect("room for the popup");
        assert_eq!([panel.width, panel.height], [WIDTH_MAX, HEIGHT_MAX]);
        assert_eq!(panel.x + panel.width / 2.0, 800.0);
        assert_eq!(panel.y + panel.height / 2.0, 530.0);
    }

    /// A smaller window gets a smaller popup, inside the padding; one too
    /// small for a row to be read gets none.
    #[test]
    fn the_popup_shrinks_to_the_window_and_then_goes() {
        let content = Rect::new(0.0, 0.0, 500.0, 400.0);
        let smaller = panel(content).expect("room for a smaller popup");
        assert_eq!(smaller.width, 500.0 - 2.0 * PADDING);
        assert_eq!(smaller.height, 400.0 - 2.0 * PADDING);
        assert!(panel(Rect::new(0.0, 0.0, 200.0, 400.0)).is_none());
        assert!(panel(Rect::new(0.0, 0.0, 800.0, 150.0)).is_none());
    }

    /// The popup opens in exactly the content area the information panel
    /// opens in, and not in one a pixel narrower or shorter: the two
    /// buttons go dead together.
    #[test]
    fn the_popup_needs_what_the_information_panel_needs() {
        let fits = |width: f32, height: f32| {
            let content = Rect::new(0.0, 0.0, width, height);
            assert_eq!(
                panel(content).is_some(),
                info::panel(content, None).is_some(),
                "{width} by {height}"
            );
            panel(content).is_some()
        };
        let least = [WIDTH_MIN + 2.0 * PADDING, HEIGHT_MIN + 2.0 * PADDING];
        assert!(fits(least[0], least[1]));
        assert!(!fits(least[0] - 1.0, least[1]));
        assert!(!fits(least[0], least[1] - 1.0));
    }

    /// The columns are only laid out where the description's has room for
    /// a few words a line, so no width the popup opens at asks egui for a
    /// column less than nothing wide. The narrowest popup there is stacks.
    #[test]
    fn the_columns_never_come_to_less_than_nothing() {
        let least_does = does_width(STACK_BELOW);
        assert!(least_does >= 120.0, "{least_does} for a sentence");
        assert!(stacked(table_width(WIDTH_MIN)));
        assert!(!stacked(table_width(WIDTH_MAX)));
    }
}
