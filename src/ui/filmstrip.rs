//! The file list: a strip of thumbnails down the left of the picture, one
//! row per file in the order the list is walked, broken into sections and
//! sorted by what its head asks for.
//!
//! The order is the list's own — `]` and `[` step through it, and the
//! counter in the bar counts along it — so what the strip shows is what
//! the application holds, not a view of it. The two choices at its head,
//! [`Section`] and [`Sort`], are made here rather than in the application
//! for the same reason `Copies` is: they are what the menus offer, and the
//! interface cannot reach into the application for its words.
//!
//! The panel is one of the chrome's, laid out by `Chrome` down the left
//! edge of the window under the top bar — the left strip and the bottom
//! bar start where it ends — and given exactly that width by
//! `Pass::file_list`, so that the picture is fitted into what it leaves
//! before anything is drawn. Its right edge is a [`grip`] that asks for a
//! wider or narrower slot, which the next frame is laid out at. Its head — the
//! two menus and the pair that go back and forward through the files
//! seen — stays put; the rows under it scroll, and only the rows on
//! screen are laid out, as the chooser's are.

use std::ops::Range;
use std::sync::Arc;

use egui::{Align, Layout, RectAlign, Sense, WidgetInfo, WidgetType, pos2, vec2};

use super::chrome::{BAR_HEIGHT, BAR_PADDING, BUTTON_GAP, Corners, Pass, STEP_SEAM};
use super::control::{Command, Control};
use super::style::{ACTIVE_BUTTON_WASH, SCROLLBAR_GUTTER, SCROLLBAR_WIDTH};
use super::{PADDING, TEXT_SIZE, fonts, icon, menu};

/// The square each thumbnail is fitted into, at its narrowest: the
/// thumbnail thread's smallest display copy, drawn at its own size. The
/// panel opens at this.
pub const SLOT_MIN: f32 = crate::thumbnailer::DISPLAY_SIDES[0] as f32;
/// The square at its widest.
pub const SLOT_MAX: f32 = 384.0;
/// The room around a slot, on every side.
pub const INSET: f32 = 6.0;
/// A section's row: one line of words.
pub const HEADER_HEIGHT: f32 = 22.0;
/// The head of the panel, where the buttons are: a bar's height, so that
/// it lines up with the bars.
pub const HEAD_HEIGHT: f32 = BAR_HEIGHT;
/// How far either side of the panel's right edge a drag takes hold of it.
const GRIP: f32 = 3.0;

/// A file's row, for a slot of `slot`: the slot and the room around it.
pub fn row_height(slot: f32) -> f32 {
    slot + 2.0 * INSET
}

/// What the panel takes off the picture, for a slot of `slot`: a row, and
/// the scrollbar's gutter beside it.
pub fn width(slot: f32) -> f32 {
    row_height(slot) + SCROLLBAR_GUTTER
}

/// The slot a panel `width` wide has, held between [`SLOT_MIN`] and
/// [`SLOT_MAX`] and put on a whole logical pixel: what a drag of the
/// panel's edge asks for.
pub fn slot_for(width: f32) -> f32 {
    (width - SCROLLBAR_GUTTER - 2.0 * INSET)
        .round()
        .clamp(SLOT_MIN, SLOT_MAX)
}

/// The scrollbar stands this far in from the panel's edge, so that the
/// bar and the hairline along the edge do not read as one thick rule;
/// what is left of the gutter parts it from the rows.
const SCROLLBAR_OUTER_MARGIN: f32 = 4.0;
/// The label over a thumbnail's corner is inset this far from the corner
/// of its backing, and the backing is this much of the bar's ground.
const LABEL_PAD: f32 = 3.0;
const LABEL_WASH: u8 = 200;
/// The hairline under the head, and around an empty slot.
const HAIRLINE: f32 = 1.0;

/// How the list is broken up before it is sorted: not at all, by the
/// directory a file is in, or by what kind of file it is. Sections stand
/// in ascending order of their label, and a section whose label is not
/// yet known — a type the header has not been read for — stands last.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Section {
    #[default]
    None,
    Path,
    Type,
}

impl Section {
    /// In the order the menu offers them.
    pub const ALL: [Section; 3] = [Section::None, Section::Path, Section::Type];

    /// The word the menu item wears.
    pub fn label(self) -> &'static str {
        match self {
            Section::None => "No sections",
            Section::Path => "By folder",
            Section::Type => "By type",
        }
    }

    /// What the item says when rested on.
    pub fn describe(self) -> &'static str {
        match self {
            Section::None => "One list, unbroken",
            Section::Path => "A section for each folder",
            Section::Type => "A section for each kind of file",
        }
    }

    /// Whether the breaking needs what a file's header says, which arrives
    /// after the list does.
    pub fn reads_facts(self) -> bool {
        matches!(self, Section::Type)
    }
}

/// What the files of a section are sorted by, ascending. A file whose key
/// is not yet known — a size or a type the header has not been read for —
/// sorts after every file whose key is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Sort {
    #[default]
    Name,
    Path,
    Type,
    Size,
    Width,
    Height,
    Area,
}

impl Sort {
    /// In the order the menu offers them.
    pub const ALL: [Sort; 7] = [
        Sort::Name,
        Sort::Path,
        Sort::Type,
        Sort::Size,
        Sort::Width,
        Sort::Height,
        Sort::Area,
    ];

    /// The word the menu item wears.
    pub fn label(self) -> &'static str {
        match self {
            Sort::Name => "Name",
            Sort::Path => "Path",
            Sort::Type => "Type",
            Sort::Size => "Size",
            Sort::Width => "Width",
            Sort::Height => "Height",
            Sort::Area => "Area",
        }
    }

    /// What the item says when rested on.
    pub fn describe(self) -> &'static str {
        match self {
            Sort::Name => "Sort by file name",
            Sort::Path => "Sort by the whole path",
            Sort::Type => "Sort by kind of file",
            Sort::Size => "Sort by size on disk",
            Sort::Width => "Sort by width in pixels",
            Sort::Height => "Sort by height in pixels",
            Sort::Area => "Sort by pixels in all",
        }
    }

    /// Whether the sort needs what a file's header says, which arrives
    /// after the list does.
    pub fn reads_facts(self) -> bool {
        !matches!(self, Sort::Name | Sort::Path)
    }
}

/// Which way the sort runs within a section: smallest, earliest or first
/// name at the top, or the other way. The sections themselves stand in
/// ascending order either way, and what is not yet known stands last
/// either way.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Direction {
    #[default]
    Ascending,
    Descending,
}

impl Direction {
    /// In the order the menu offers them.
    pub const ALL: [Direction; 2] = [Direction::Ascending, Direction::Descending];

    /// The word the menu item wears.
    pub fn label(self) -> &'static str {
        match self {
            Direction::Ascending => "Ascending",
            Direction::Descending => "Descending",
        }
    }

    /// What the item says when rested on.
    pub fn describe(self) -> &'static str {
        match self {
            Direction::Ascending => "Smallest, earliest or first name at the top",
            Direction::Descending => "Largest, latest or last name at the top",
        }
    }
}

/// How the list stands: its sections, the sort within each, and which way
/// that sort runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Order {
    pub section: Section,
    pub sort: Sort,
    pub direction: Direction,
}

impl Order {
    /// Whether the order can change as headers are read.
    pub fn reads_facts(self) -> bool {
        self.section.reads_facts() || self.sort.reads_facts()
    }
}

/// One row of the strip, as the frame draws it.
#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    /// A section's heading: what the files under it have in common.
    Header(String),
    /// A file.
    File {
        /// Its place in the list, counted from one, as the bar counts.
        index: usize,
        /// Its name, with nothing of the path it sits in.
        name: String,
        /// The whole path, for the tooltip: the name over the thumbnail is
        /// cut to the slot.
        path: String,
        /// Its thumbnail, once one has arrived and while the screen still
        /// holds it.
        thumb: Option<super::Thumb>,
    },
}

/// How tall a row is drawn, for a slot of `slot`.
pub fn height(row: &Row, slot: f32) -> f32 {
    match row {
        Row::Header(_) => HEADER_HEIGHT,
        Row::File { .. } => row_height(slot),
    }
}

/// What the strip is drawn from, on each frame it is up.
#[derive(Clone, Debug, PartialEq)]
pub struct Input {
    /// The rows, in order. Shared rather than copied, as the chooser's
    /// are: a session of thousands of files is thousands of rows.
    pub rows: Arc<[Row]>,
    /// Where each row starts down the strip, and after the last where it
    /// ends: one entry more than there are rows. What says which rows are
    /// on screen without measuring every row above them.
    pub tops: Arc<[f32]>,
    /// The square each thumbnail is fitted into, which the tops were
    /// worked out for; the panel is [`width`] of it.
    pub slot: f32,
    /// Which row is the file on screen, if it is in the list.
    pub current: Option<usize>,
    /// The order in force, which the menus are lit against.
    pub order: Order,
    /// Whether there is a file seen before this one to go back to, and one
    /// seen after it to go forward to: what the pair at the head answers.
    pub back: bool,
    pub forward: bool,
    /// Whether the file on screen changed since the last frame, in which
    /// case the strip scrolls to keep its row in view. Not on every frame:
    /// a strip that scrolled to the file while the wheel was moving away
    /// from it would fight the hand.
    pub reveal: bool,
    /// The rows that were on screen at the last frame, so that a frame can
    /// say so only when it changes.
    pub visible: Range<usize>,
}

/// The rows any part of which lies between `top` and `bottom` down the
/// strip, `tops` being where each starts — see [`Input::tops`].
pub fn span(tops: &[f32], top: f32, bottom: f32) -> Range<usize> {
    let count = tops.len().saturating_sub(1);
    if count == 0 {
        return 0..0;
    }
    // The last row starting at or above `top` is the first on screen, and
    // the first starting at or below `bottom` is the last.
    let first = tops[..count].partition_point(|&start| start <= top).saturating_sub(1);
    let last = tops[..count].partition_point(|&start| start < bottom);
    first..last.max(first)
}

/// Lays the panel out in `ui`, which is the whole of it: the head, and the
/// rows under it. The head is a bar, and goes with the bars when the
/// interface is hidden; the rows stay.
pub(super) fn show(pass: &mut Pass, ui: &mut egui::Ui, input: &Input) {
    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
    if pass.panels.show_ui {
        head(pass, ui, input);
    }
    rows(pass, ui, input);
}

/// The head: the two menus, and the pair that go back and forward through
/// the files seen, on one row a bar high with a hairline under it.
fn head(pass: &mut Pass, ui: &mut egui::Ui, input: &Input) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, HEAD_HEIGHT), Sense::HOVER);
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(vec2(BAR_PADDING, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    row.spacing_mut().item_spacing = egui::Vec2::ZERO;
    let ctx = row.ctx().clone();
    // The menus first: what the list is broken into, then what each
    // piece is sorted by. Each lit while its menu is open, as the other
    // menu buttons are, and hung below.
    let sections = egui::Id::new("section menu");
    let open = egui::Popup::is_id_open(&ctx, sections);
    let button = pass.icon_button(
        &mut row,
        icon::ROWS_3,
        Control::Sections,
        open,
        true,
        Corners::All,
    );
    egui::Popup::menu(&button)
        .id(sections)
        .align(RectAlign::BOTTOM_START)
        .gap(PADDING)
        .show(|ui| menu::section_cells(pass, ui, input.order));
    row.add_space(BUTTON_GAP);
    let sorting = egui::Id::new("sort menu");
    let open = egui::Popup::is_id_open(&ctx, sorting);
    // The button wears the way the sort runs: bars growing down the mark
    // for ascending, shrinking for descending.
    let mark = match input.order.direction {
        Direction::Ascending => icon::ARROW_DOWN_NARROW_WIDE,
        Direction::Descending => icon::ARROW_DOWN_WIDE_NARROW,
    };
    let button = pass.icon_button(&mut row, mark, Control::Sorting, open, true, Corners::All);
    egui::Popup::menu(&button)
        .id(sorting)
        .align(RectAlign::BOTTOM_START)
        .gap(PADDING)
        .show(|ui| menu::sort_cells(pass, ui, input.order));
    row.add_space(BUTTON_GAP);
    // Then the pair, set against each other as the pair that steps
    // through the list is: the two ways of one thing.
    let back = pass.icon_button(
        &mut row,
        icon::ARROW_LEFT,
        Control::Back,
        false,
        input.back,
        Corners::Leading,
    );
    if back.clicked() {
        pass.press(Control::Back);
    }
    row.add_space(STEP_SEAM);
    let forward = pass.icon_button(
        &mut row,
        icon::ARROW_RIGHT,
        Control::Forward,
        false,
        input.forward,
        Corners::Trailing,
    );
    if forward.clicked() {
        pass.press(Control::Forward);
    }
    // The hairline along the foot of the head, on the device's grid so
    // that it is one pixel wide wherever it lands.
    let grid = pass.grid;
    let edge = grid.line_width(HAIRLINE);
    let line = egui::Rect::from_min_size(
        pos2(rect.min.x, grid.snap(rect.max.y - edge)),
        vec2(rect.width(), edge),
    );
    ui.painter().rect_filled(line, 0.0, pass.theme.border);
}

/// The grip along the panel's right edge, a little either side of it: a
/// drag of it asks for the slot that puts the edge under the pointer,
/// which the application holds between [`SLOT_MIN`] and [`SLOT_MAX`].
pub(super) fn grip(pass: &mut Pass, ui: &mut egui::Ui, slot: f32) {
    let Some(list) = pass.file_list else {
        return;
    };
    let rect = egui::Rect::from_x_y_ranges(
        list.right() - GRIP..=list.right() + GRIP,
        list.y_range(),
    );
    let response = ui.interact(rect, egui::Id::new("filmstrip grip"), Sense::DRAG);
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    if response.dragged()
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let asked = slot_for(pointer.x - list.left());
        if asked != slot {
            pass.commands.push(Command::FilmstripSlot(asked));
        }
    }
}

/// The rows, virtualized: only the rows in the viewport are laid out, and
/// which rows those are goes back as a command when it changes, so that
/// their thumbnails can be asked for first.
fn rows(pass: &mut Pass, ui: &mut egui::Ui, input: &Input) {
    let count = input.rows.len();
    let total = input.tops.last().copied().unwrap_or(0.0);
    ui.spacing_mut().scroll.bar_outer_margin = SCROLLBAR_OUTER_MARGIN;
    ui.spacing_mut().scroll.bar_inner_margin =
        SCROLLBAR_GUTTER - SCROLLBAR_WIDTH - SCROLLBAR_OUTER_MARGIN;
    let scroll = egui::ScrollArea::vertical()
        .id_salt("filmstrip rows")
        .auto_shrink(false)
        .show_viewport(ui, |ui, viewport| {
            ui.set_height(total.max(viewport.height()));
            // The rows stop short of the scrollbar, which the area lays
            // beside its content rather than over it.
            let width = ui.max_rect().width();
            let left = ui.max_rect().left();
            let top = ui.max_rect().top();
            let row_rect = |row: usize| {
                egui::Rect::from_min_size(
                    pos2(left, top + input.tops[row]),
                    vec2(width, input.tops[row + 1] - input.tops[row]),
                )
            };
            let range = span(&input.tops, viewport.min.y, viewport.max.y);
            for row in range.clone() {
                match &input.rows[row] {
                    Row::Header(label) => header(pass, ui, label, row_rect(row)),
                    Row::File {
                        index,
                        name,
                        path,
                        thumb,
                    } => {
                        file(pass, ui, input, row, row_rect(row), *index, name, path, *thumb);
                    }
                }
            }
            // Put there, not scrolled there: the file on screen may have
            // been stepped to from anywhere in the list, and a glide
            // across the whole list is a wait.
            if input.reveal
                && let Some(current) = input.current
                && current < count
            {
                ui.scroll_to_rect_animation(
                    row_rect(current),
                    None,
                    egui::style::ScrollAnimation::none(),
                );
            }
            range
        });
    if scroll.inner != input.visible {
        pass.commands.push(Command::FilmstripVisible(scroll.inner));
    }
}

/// A section's heading: its label, set bold in the dim ink, cut to the
/// row, and said in full when rested on. Nothing to press.
fn header(pass: &mut Pass, ui: &mut egui::Ui, label: &str, rect: egui::Rect) {
    let response = ui.interact(rect, ui.id().with(("filmstrip heading", label)), Sense::HOVER);
    pass.caption(response, vec![label.to_string()], Vec::new());
    let dim: egui::Color32 = pass.theme.text_dim.into();
    let bold = egui::FontId::new(TEXT_SIZE, egui::FontFamily::Name(fonts::BOLD.into()));
    let room = (rect.width() - 2.0 * INSET).max(0.0);
    let galley = super::chooser::lit(ui, label, &[], bold, dim, dim, room);
    let painter = ui.painter_at(rect);
    painter.galley(
        pos2(rect.left() + INSET, rect.center().y - galley.size().y / 2.0),
        galley,
        dim,
    );
}

/// A file's row: its thumbnail in its slot, and over the slot's top-left
/// corner its place in the list and its name. Washed in the accent when it
/// is the file on screen, lit under the pointer, and a press on it shows
/// the file. Rested on, it says the name in full, with the whole path
/// under it: the name over the thumbnail is cut to the slot.
#[allow(clippy::too_many_arguments, reason = "one row, its parts by name")]
fn file(
    pass: &mut Pass,
    ui: &mut egui::Ui,
    input: &Input,
    row: usize,
    rect: egui::Rect,
    index: usize,
    name: &str,
    path: &str,
    thumb: Option<super::Thumb>,
) {
    let theme = pass.theme;
    let control = Control::Thumb(row);
    let response = ui.interact(rect, ui.id().with(("filmstrip row", row)), Sense::CLICK);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, control.label()));
    let response = pass.caption(response, vec![name.to_string()], vec![path.to_string()]);
    let painter = ui.painter_at(rect);
    if input.current == Some(row) {
        painter.rect_filled(rect, 0.0, theme.accent.with_alpha(ACTIVE_BUTTON_WASH));
    } else if response.hovered() {
        painter.rect_filled(rect, 0.0, theme.button_hover);
    }

    // The slot, and the picture fitted and centered in it: the copy that
    // covers the slot's device pixels, drawn no larger than itself.
    let slot = egui::Rect::from_min_size(
        pos2(rect.left() + INSET, rect.top() + INSET),
        vec2(input.slot, input.slot),
    );
    match thumb {
        Some(thumb) => {
            let texture = thumb.for_side(input.slot * ui.ctx().pixels_per_point());
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
                pass.grid,
                super::Rect::new(slot.left(), slot.top(), slot.width(), slot.height()),
                HAIRLINE,
                theme.border.into(),
            );
        }
    }

    // The label over the corner: the index and the name, on a wash of
    // the bar's ground so that they read over any picture, and cut to
    // the slot.
    let bright: egui::Color32 = theme.text_bright.into();
    let body = egui::FontId::proportional(TEXT_SIZE);
    let room = (input.slot - 2.0 * LABEL_PAD).max(0.0);
    let galley = super::chooser::lit(
        ui,
        &format!("{index}  {name}"),
        &[],
        body,
        bright,
        bright,
        room,
    );
    let backing = egui::Rect::from_min_size(
        slot.min,
        galley.size() + vec2(2.0 * LABEL_PAD, 2.0 * LABEL_PAD),
    );
    painter.rect_filled(backing, 0.0, theme.bar_background.with_alpha(LABEL_WASH));
    painter.galley(backing.min + vec2(LABEL_PAD, LABEL_PAD), galley, bright);

    if response.clicked() {
        pass.press(control);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rows on screen are the ones any part of which is in the
    /// viewport: the one the top edge cuts through, every one wholly in
    /// it, and the one the bottom edge cuts through.
    #[test]
    fn the_span_is_every_row_the_viewport_touches() {
        let tops = [0.0, 22.0, 162.0, 302.0, 442.0];
        assert_eq!(span(&tops, 0.0, 100.0), 0..2);
        assert_eq!(span(&tops, 22.0, 162.0), 1..2, "edges on the boundary count once");
        assert_eq!(span(&tops, 30.0, 310.0), 1..4);
        assert_eq!(span(&tops, 500.0, 600.0), 3..4, "past the end, the last row");
        assert_eq!(span(&[0.0], 0.0, 100.0), 0..0, "no rows at all");
        assert_eq!(span(&[], 0.0, 100.0), 0..0);
    }

    /// A panel's width comes back as the slot it was made from, and a
    /// width past either end as that end's slot, on a whole pixel.
    #[test]
    fn a_width_is_the_slot_it_was_made_from_held_to_the_range() {
        for slot in [SLOT_MIN, 200.0, SLOT_MAX] {
            assert_eq!(slot_for(width(slot)), slot);
        }
        assert_eq!(slot_for(width(200.0) + 0.4), 200.0);
        assert_eq!(slot_for(0.0), SLOT_MIN);
        assert_eq!(slot_for(10_000.0), SLOT_MAX);
    }
}
