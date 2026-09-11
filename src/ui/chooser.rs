//! The file chooser: a popup over the top of the picture with a field to
//! type in and, under it, the files of the session that fit what was typed,
//! each with its thumbnail. `Ctrl+P` opens it; `Enter` or a click opens the
//! file under the cursor and closes it; `Esc` or a click outside closes it.
//!
//! A fifth popup, and egui's like the four menus: its open state is in
//! egui's memory under [`id`], so `Esc`, a click outside, and the rule that
//! one popup is open at a time all come from the toolkit, and the
//! application opens and closes it exactly as it closes a menu. What is
//! not egui's is everything about the list — the query, the rows, which
//! row the cursor is on — which is the application's, handed in as
//! [`Input`] each frame it is open and handed back as commands.
//!
//! While the field has the keyboard every key goes to egui and none to the
//! window, so the keys the chooser answers — the arrows, `Enter`, `Ctrl+P`
//! again — are read here, at the top of the pass, before the field can see
//! them, and come back as commands like a press would.

use std::ops::Range;
use std::sync::Arc;

use egui::{
    Align, Frame, Key, LayerId, Modifiers, PopupAnchor, PopupCloseBehavior, PopupKind, RectAlign,
    Sense, TextEdit, WidgetInfo, WidgetType, load::SizedTexture, pos2, vec2,
};

use super::chrome::Pass;
use super::control::{Command, Control};
use super::icon;
use super::style::{MENU_PADDING, MENU_RADIUS, TOGGLE_RADIUS};
use super::{PADDING, Rect, TEXT_SIZE, fonts};

/// The popup's id in egui's memory: what the application opens, and what
/// it asks whether it is open.
pub fn id() -> egui::Id {
    egui::Id::new("chooser")
}

/// The popup at most this wide, and this share of the content area tall.
const WIDTH_MAX: f32 = 720.0;
const HEIGHT_SHARE: f32 = 0.6;
/// Below this width the rows could not carry a name and a thumbnail, and
/// the popup stays off.
const WIDTH_MIN: f32 = 240.0;
/// One row of the list, and the field above it.
pub const ROW_HEIGHT: f32 = 56.0;
const FIELD_HEIGHT: f32 = 32.0;
/// The space between the field and the list.
const FIELD_GAP: f32 = 8.0;
/// The thumbnail's slot in a row, and what it is inset from the row's edge.
const THUMB_SLOT: [f32; 2] = [72.0, 48.0];
const THUMB_INSET: f32 = 4.0;
/// The gap between the thumbnail and the words, and between the two runs
/// of words.
const ROW_GAP: f32 = 10.0;
/// The columns down the right of a row: what kind of file, its place in
/// the list, and its size.
const KIND_WIDTH: f32 = 110.0;
const INDEX_WIDTH: f32 = 44.0;
const DIMENSIONS_WIDTH: f32 = 90.0;
/// The room a row keeps at its right edge, past the last column.
const ROW_PADDING: f32 = 8.0;
/// The bar at the left edge of the row of the file already on screen.
const CURRENT_MARK: f32 = 2.0;
/// The field's text is inset this far from its edge.
const FIELD_INSET: f32 = 8.0;
/// The hairline around the field, and around a thumbnail's slot.
const HAIRLINE: f32 = 1.0;
/// What the field says while it is empty.
const HINT: &str = "Type to filter";

/// One row of the list, as the frame draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// The file's name, set bold.
    pub name: String,
    /// The directory it is in, relative to the one every file in the
    /// session shares; empty where that is the directory itself.
    pub dir: String,
    /// What kind of file: the extension, and the frames or pages it holds
    /// once its header has been read.
    pub kind: String,
    /// Its place in the list, counted from one, as the bar counts.
    pub index: usize,
    /// Its size in pixels, once known.
    pub dimensions: Option<(u32, u32)>,
    /// Its thumbnail, once one has arrived and while the screen still
    /// holds it.
    pub thumb: Option<SizedTexture>,
    /// The chars of `dir/name` — `name` alone where `dir` is empty — the
    /// query was found at, for lighting them.
    pub positions: Vec<usize>,
}

/// What the chooser is drawn from, on each frame it is open.
#[derive(Clone, Debug, PartialEq)]
pub struct Input {
    pub query: String,
    /// The rows that fit the query, best first. Shared rather than copied:
    /// a session of thousands of files is thousands of rows, and a frame
    /// should not copy them to draw a few.
    pub rows: Arc<[Row]>,
    /// Which row the cursor is on: what `Enter` opens.
    pub cursor: usize,
    /// Which row is the file already on screen, if it is in the list.
    pub current: Option<usize>,
    /// Whether the session spans more than one directory, which is when a
    /// row says which one it is in.
    pub several_dirs: bool,
    /// Whether this is the first frame the popup is up. The chord that
    /// opened it went to the window's key table and to egui alike — egui
    /// is handed every key, and only refuses the window the ones its
    /// widgets want — so on this frame it is still in egui's input, and
    /// reading it here would close what it had just opened.
    pub opened: bool,
    /// Whether the cursor was moved by a key since the last frame, in which
    /// case the list scrolls to keep it in view. Not on every frame: a
    /// list that scrolled to the cursor while the wheel was moving it away
    /// would fight the hand.
    pub reveal: bool,
    /// The rows that were on screen at the last frame, so that a frame can
    /// say so only when it changes.
    pub visible: Range<usize>,
}

/// A move of the cursor asked for by a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Up,
    Down,
    /// A page of `rows` rows, which is what the list had room for when the
    /// key was pressed.
    Page {
        down: bool,
        rows: usize,
    },
    First,
    Last,
}

/// Where the popup goes: across the top of `content`, centered, at most
/// [`WIDTH_MAX`] wide and [`HEIGHT_SHARE`] of the content tall. `None`
/// when the window has no room for the field and three rows, or is too
/// narrow for a row to say anything, in which case the chooser stays off.
pub fn panel(content: Rect) -> Option<Rect> {
    let width = WIDTH_MAX.min(content.width - 2.0 * PADDING);
    let height = (HEIGHT_SHARE * content.height).round();
    let least = 2.0 * MENU_PADDING + FIELD_HEIGHT + FIELD_GAP + 3.0 * ROW_HEIGHT;
    if width < WIDTH_MIN || height < least || height > content.height - 2.0 * PADDING {
        return None;
    }
    Some(Rect::new(
        (content.x + (content.width - width) / 2.0).round(),
        (content.y + PADDING).round(),
        width.round(),
        height,
    ))
}

/// Draws the popup, if it is open, and reads what was pressed in it.
pub(super) fn show(pass: &mut Pass, ui: &mut egui::Ui, input: &Input, content: Rect) {
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
    egui::Popup::new(
        id(),
        ui.ctx().clone(),
        PopupAnchor::Position(pos2(panel.x, panel.y)),
        LayerId::background(),
    )
    .open_memory(None)
    .kind(PopupKind::Popup)
    .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
    .align(RectAlign::BOTTOM_START)
    .align_alternatives(&[])
    .gap(0.0)
    .width(panel.width)
    .frame(frame)
    .show(|ui| {
        ui.set_min_size(inside);
        ui.set_max_size(inside);
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        let list_height = inside.y - FIELD_HEIGHT - FIELD_GAP;
        keys(pass, ui, input, list_height);
        field(pass, ui, input, inside.x);
        ui.add_space(FIELD_GAP);
        rows(pass, ui, input, inside.x, list_height);
    });
}

/// The keys the chooser answers, taken out of the input before the field
/// can see them. `Esc` is not among them: closing the popup on it is
/// egui's, and the field leaves it alone. `Ctrl+P` is consumed on the
/// opening frame too, so that it does not reach the field as text, but is
/// not acted on — see [`Input::opened`].
fn keys(pass: &mut Pass, ui: &mut egui::Ui, input: &Input, list_height: f32) {
    let page = (list_height / ROW_HEIGHT).floor().max(1.0) as usize;
    let pressed = ui.input_mut(|keys| {
        let mut pressed = Vec::new();
        if keys.consume_key(Modifiers::COMMAND, Key::P) && !input.opened {
            pressed.push(Command::Press(Control::Chooser));
        }
        if keys.consume_key(Modifiers::NONE, Key::Enter) && !input.rows.is_empty() {
            pressed.push(Command::Press(Control::Choose(input.cursor)));
        }
        for (key, step) in [
            (Key::ArrowUp, Step::Up),
            (Key::ArrowDown, Step::Down),
            (
                Key::PageUp,
                Step::Page {
                    down: false,
                    rows: page,
                },
            ),
            (
                Key::PageDown,
                Step::Page {
                    down: true,
                    rows: page,
                },
            ),
            (Key::Home, Step::First),
            (Key::End, Step::Last),
        ] {
            if keys.consume_key(Modifiers::NONE, key) {
                pressed.push(Command::Cursor(step));
            }
        }
        pressed
    });
    pass.commands.extend(pressed);
}

/// The field: a hand-drawn box, since egui's own ground for a field is the
/// theme's hairline color and would not read as one, holding a plain text
/// edit. It keeps the keyboard for as long as the popup is up: on the first
/// frame nothing has it, and after a click on the popup's own frame nothing
/// has it again, and either way it is asked for here.
fn field(pass: &mut Pass, ui: &mut egui::Ui, input: &Input, width: f32) {
    let theme = pass.theme;
    let (rect, _) = ui.allocate_exact_size(vec2(width, FIELD_HEIGHT), Sense::HOVER);
    let painter = ui.painter();
    painter.rect_filled(rect, TOGGLE_RADIUS, theme.bar_background);
    painter.rect_stroke(
        rect,
        TOGGLE_RADIUS,
        egui::Stroke::new(HAIRLINE, theme.border),
        egui::StrokeKind::Inside,
    );
    let mut text = input.query.clone();
    let edit = TextEdit::singleline(&mut text)
        .id(id().with("query"))
        .return_key(None)
        .frame(Frame::NONE)
        .hint_text(HINT)
        .font(egui::FontId::proportional(TEXT_SIZE))
        .text_color(theme.text_primary.into())
        .desired_width(f32::INFINITY)
        .vertical_align(Align::Center)
        .margin(egui::Margin::ZERO);
    let inner = rect.shrink2(vec2(FIELD_INSET, 0.0));
    let response = ui.put(inner, edit);
    if response.changed() {
        pass.commands.push(Command::Query(text));
    }
    if ui.memory(|memory| memory.focused().is_none()) {
        response.request_focus();
    }
}

/// The list, virtualized: only the rows in the viewport are laid out, and
/// which rows those are goes back as a command when it changes, so that
/// their thumbnails can be asked for first.
fn rows(pass: &mut Pass, ui: &mut egui::Ui, input: &Input, width: f32, height: f32) {
    let count = input.rows.len();
    let scroll = egui::ScrollArea::vertical()
        .id_salt("chooser rows")
        .auto_shrink(false)
        .max_height(height)
        .show_viewport(ui, |ui, viewport| {
            ui.set_width(width);
            ui.set_height((ROW_HEIGHT * count as f32).max(height));
            // The rows stop short of the scrollbar, which the area lays
            // beside its content rather than over it.
            let width = ui.max_rect().width();
            let left = ui.max_rect().left();
            let top = ui.max_rect().top();
            let first = ((viewport.min.y / ROW_HEIGHT).floor().max(0.0) as usize).min(count);
            let last = ((viewport.max.y / ROW_HEIGHT).ceil().max(0.0) as usize + 1).min(count);
            let row_rect = |index: usize| {
                egui::Rect::from_min_size(
                    pos2(left, top + index as f32 * ROW_HEIGHT),
                    vec2(width, ROW_HEIGHT),
                )
            };
            for index in first..last {
                row(pass, ui, input, index, row_rect(index));
            }
            if input.reveal && input.cursor < count {
                ui.scroll_to_rect(row_rect(input.cursor), None);
            }
            first..last
        });
    if scroll.inner != input.visible {
        pass.commands.push(Command::Visible(scroll.inner));
    }
}

/// One row: the thumbnail in its slot, the name and its directory after
/// it, and the columns of facts down the right. Washed under the cursor,
/// lit under the pointer, and marked at its left edge when it is the file
/// already on screen.
fn row(pass: &mut Pass, ui: &mut egui::Ui, input: &Input, index: usize, rect: egui::Rect) {
    let theme = pass.theme;
    let item = &input.rows[index];
    let control = Control::Choose(index);
    let response = ui.interact(rect, ui.id().with(("chooser row", index)), Sense::CLICK);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, control.label()));
    let painter = ui.painter_at(rect);
    if index == input.cursor {
        painter.rect_filled(rect, TOGGLE_RADIUS, ui.visuals().selection.bg_fill);
    } else if response.hovered() {
        painter.rect_filled(rect, TOGGLE_RADIUS, theme.button_hover);
    }
    let grid = icon::Grid::new(ui.pixels_per_point());
    if input.current == Some(index) {
        let mark = egui::Rect::from_min_size(
            pos2(grid.snap(rect.left()), grid.snap(rect.top() + THUMB_INSET)),
            vec2(
                grid.line_width(CURRENT_MARK),
                grid.snap(rect.bottom() - THUMB_INSET) - grid.snap(rect.top() + THUMB_INSET),
            ),
        );
        painter.rect_filled(mark, 0.0, theme.accent);
    }

    // The thumbnail's slot, and the picture fitted and centered in it.
    let slot = egui::Rect::from_min_size(
        pos2(
            rect.left() + THUMB_INSET + CURRENT_MARK,
            rect.top() + (ROW_HEIGHT - THUMB_SLOT[1]) / 2.0,
        ),
        vec2(THUMB_SLOT[0], THUMB_SLOT[1]),
    );
    match item.thumb {
        Some(texture) => {
            let size = texture.size;
            let scale = (slot.width() / size.x).min(slot.height() / size.y).min(1.0);
            let fitted = vec2((size.x * scale).round(), (size.y * scale).round());
            let at = egui::Rect::from_center_size(slot.center(), fitted);
            painter.image(
                texture.id,
                at,
                egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        None => {
            painter.rect_filled(slot, 0.0, theme.button_idle);
            super::outline(
                &painter,
                grid,
                Rect::new(slot.left(), slot.top(), slot.width(), slot.height()),
                HAIRLINE,
                theme.border.into(),
            );
        }
    }

    // The columns down the right, right-aligned each in its own width.
    let dim: egui::Color32 = theme.text_dim.into();
    let body = egui::FontId::proportional(TEXT_SIZE);
    let mut right = rect.right() - ROW_PADDING;
    for (text, column) in [
        (
            item.dimensions
                .map(|(width, height)| format!("{width}\u{00d7}{height}")),
            DIMENSIONS_WIDTH,
        ),
        (Some(item.index.to_string()), INDEX_WIDTH),
        (Some(item.kind.clone()), KIND_WIDTH),
    ] {
        if let Some(text) = text {
            let galley = ui
                .ctx()
                .fonts_mut(|fonts| fonts.layout_no_wrap(text, body.clone(), dim));
            let at = pos2(
                right - galley.size().x,
                rect.center().y - galley.size().y / 2.0,
            );
            painter.galley(at, galley, dim);
        }
        right -= column;
    }

    // The name in bold, the directory dim after it, each with the chars
    // the query was found at picked out in the accent. Laid out in their
    // own inks, and cut to the room the columns leave.
    let bold = egui::FontId::new(TEXT_SIZE, egui::FontFamily::Name(fonts::BOLD.into()));
    let dir_chars = if item.dir.is_empty() {
        0
    } else {
        item.dir.chars().count() + 1
    };
    let (in_dir, in_name): (Vec<usize>, Vec<usize>) =
        item.positions.iter().partition(|&&at| at < dir_chars);
    let in_name: Vec<usize> = in_name.into_iter().map(|at| at - dir_chars).collect();
    let left = slot.right() + ROW_GAP;
    let room = (right - left).max(0.0);
    let accent: egui::Color32 = theme.accent.into();
    let bright: egui::Color32 = theme.text_bright.into();
    let name = lit(ui, &item.name, &in_name, bold, bright, accent, room);
    let at = pos2(left, rect.center().y - name.size().y / 2.0);
    painter.galley(at, name.clone(), bright);
    if input.several_dirs && !item.dir.is_empty() {
        let after = left + name.size().x + ROW_GAP;
        let room = (right - after).max(0.0);
        if room > 0.0 {
            let dir = lit(ui, &item.dir, &in_dir, body, dim, accent, room);
            let at = pos2(after, rect.center().y - dir.size().y / 2.0);
            painter.galley(at, dir, dim);
        }
    }

    if response.clicked() {
        pass.press(control);
    }
}

/// `text` laid out in `ink`, with the chars at `positions` in `lit`
/// instead, truncated to `room`.
fn lit(
    ui: &egui::Ui,
    text: &str,
    positions: &[usize],
    font: egui::FontId,
    ink: egui::Color32,
    lit: egui::Color32,
    room: f32,
) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = room;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let mut lit_at = positions.iter().copied().peekable();
    for (index, (offset, c)) in text.char_indices().enumerate() {
        let is_lit = lit_at.peek() == Some(&index);
        if is_lit {
            lit_at.next();
        }
        job.append(
            &text[offset..offset + c.len_utf8()],
            0.0,
            egui::TextFormat {
                font_id: font.clone(),
                color: if is_lit { lit } else { ink },
                ..Default::default()
            },
        );
    }
    ui.ctx().fonts_mut(|fonts| fonts.layout_job(job))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The popup is centered across the top of the content area, capped in
    /// width, and gone from a window too small to hold a few rows of it.
    #[test]
    fn the_panel_is_centered_capped_and_absent_when_small() {
        let content = Rect::new(30.0, 30.0, 940.0, 640.0);
        let wide = panel(content).expect("room for it");
        assert_eq!(wide.width, WIDTH_MAX);
        assert_eq!(wide.height, (HEIGHT_SHARE * 640.0).round());
        assert_eq!(wide.y, 30.0 + PADDING);
        assert!((wide.x + wide.width / 2.0 - (30.0 + 470.0)).abs() <= 0.5);

        let narrow = panel(Rect::new(0.0, 0.0, 400.0, 640.0)).expect("room for it");
        assert_eq!(narrow.width, 400.0 - 2.0 * PADDING);

        assert_eq!(panel(Rect::new(0.0, 0.0, 200.0, 640.0)), None);
        assert_eq!(panel(Rect::new(0.0, 0.0, 940.0, 200.0)), None);
    }
}
