//! The file list: a strip of thumbnails down the left of the picture, one
//! row per file in the order the list is walked, sorted by what its head
//! asks for.
//!
//! The order is the list's own — `]` and `[` step through it, and the
//! counter in the bar counts along it — so what the strip shows is what
//! the application holds, not a view of it. The choices at its head,
//! [`Sort`] and [`Direction`], are made here rather than in the application
//! for the same reason `Copies` is: they are what the menus offer, and the
//! interface cannot reach into the application for its words.
//!
//! The panel is one of the chrome's, laid out by `Chrome` down the left
//! edge of the window under the top bar — the left strip and the bottom
//! bar start where it ends — and given exactly that width by
//! `Pass::file_list`, so that the picture is fitted into what it leaves
//! before anything is drawn. Its right edge is a [`grip`] that asks for a
//! wider or narrower slot, which the next frame is laid out at. Its head — the
//! menu of sorts and the pair that go back and forward through the files
//! seen — stays put; the rows under it scroll, and only the rows on
//! screen are laid out, as the chooser's are.

use std::ops::Range;
use std::sync::Arc;
use std::time::SystemTime;

use egui::{Align, Layout, RectAlign, Sense, WidgetInfo, WidgetType, pos2, vec2};

use super::chrome::{BAR_HEIGHT, BAR_PADDING, BUTTON_GAP, Corners, Pass, STEP_SEAM};
use super::control::{Command, Control};
use super::style::{ACTIVE_BUTTON_WASH, SCROLLBAR_GUTTER, SCROLLBAR_WIDTH};
use super::{PADDING, TEXT_SIZE, fonts, icon, menu};

/// The square each thumbnail is fitted into, at its narrowest: the
/// thumbnail thread's smallest display copy, drawn at its own size.
pub const SLOT_MIN: f32 = crate::thumbnailer::DISPLAY_SIDES[0] as f32;
/// The square the panel opens at: a little wider than the narrowest, so
/// that a name has room in the title.
pub const SLOT_DEFAULT: f32 = 148.0;
/// The square at its widest.
pub const SLOT_MAX: f32 = 384.0;
/// The room around a slot, on every side.
pub const INSET: f32 = 6.0;
/// A file's title, over its slot: one line of words.
pub const TITLE_HEIGHT: f32 = 22.0;
/// The head of the panel, where the buttons are: a bar's height, so that
/// it lines up with the bars.
pub const HEAD_HEIGHT: f32 = BAR_HEIGHT;
/// How far either side of the panel's right edge a drag takes hold of it.
const GRIP: f32 = 3.0;

/// A file's row, for a slot of `slot`: its title, and under it the slot
/// and the room around it.
pub fn row_height(slot: f32) -> f32 {
    TITLE_HEIGHT + slot + 2.0 * INSET
}

/// What the panel takes off the picture, for a slot of `slot`: a row, and
/// the scrollbar's gutter beside it.
pub fn width(slot: f32) -> f32 {
    slot + 2.0 * INSET + SCROLLBAR_GUTTER
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
/// The sorted value across a slot: its words a little smaller than the
/// title's and inset this far inside their backing across and down, the
/// backing's corners rounded this much and its foot this far up from the
/// slot's, and the backing this much of the bar's ground.
const VALUE_SIZE: f32 = TEXT_SIZE - 2.0;
const VALUE_PAD_X: f32 = 6.0;
const VALUE_PAD_Y: f32 = 3.0;
const VALUE_RADIUS: f32 = 3.0;
const VALUE_LIFT: f32 = 8.0;
const VALUE_WASH: u8 = 235;
/// However little room a name has, a cut in its middle keeps this many of
/// its last characters before its extension, where a numbered run of
/// files differs.
const TAIL_KEPT: usize = 4;
/// The hairline under the head, and around an empty slot.
const HAIRLINE: f32 = 1.0;

/// What the files are sorted by. A file whose key
/// is not yet known — a size or a type the header has not been read for —
/// sorts after every file whose key is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Sort {
    #[default]
    Name,
    Path,
    Type,
    Date,
    Size,
    Width,
    Height,
    Area,
}

impl Sort {
    /// In the order the menu offers them.
    pub const ALL: [Sort; 8] = [
        Sort::Name,
        Sort::Path,
        Sort::Type,
        Sort::Date,
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
            Sort::Date => "Date",
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
            Sort::Date => "Sort by when the file was last changed",
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

/// Which way the sort runs: smallest, earliest or first name at the top,
/// or the other way. What is not yet known stands last either way.
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

/// How the list stands: what it is sorted by, and which way that sort runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Order {
    pub sort: Sort,
    pub direction: Direction,
}

impl Order {
    /// Whether the order can change as headers are read.
    pub fn reads_facts(self) -> bool {
        self.sort.reads_facts()
    }
}

/// One file's row of the strip, as the frame draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// Its place in the list, counted from one, as the bar counts.
    pub index: usize,
    /// Its name, with nothing of the path it sits in.
    pub name: String,
    /// The whole path, for the tooltip and a sort by path.
    pub path: String,
    /// What kind of file it is, its size in pixels, its size on disk and
    /// when it was last written, for the tooltip and the sorted value,
    /// once its header has been read.
    pub format: Option<&'static str>,
    pub size: Option<(u32, u32)>,
    pub bytes: Option<u64>,
    pub modified: Option<SystemTime>,
    /// Its thumbnail, once one has arrived and while the screen still
    /// holds it.
    pub thumb: Option<super::Thumb>,
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

/// The head: the menu of sorts, and the pair that go back and forward
/// through the files seen, on one row a bar high with a hairline under it.
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
    let sorting = egui::Id::new("sort menu");
    let open = egui::Popup::is_id_open(&ctx, sorting);
    // The menu first, lit while it is open, as the other menu buttons are,
    // and hung below. The button wears the way the sort runs: bars growing down the mark
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
    // The panel's right edge, past the scrollbar: where a row's tooltip
    // hangs, clear of the strip.
    let edge = ui.max_rect().right();
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
                file(pass, ui, input, row, row_rect(row), edge);
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

/// A file's row: its title — its place in the list and its name — and
/// under it its thumbnail in its slot, with what the list is sorted by
/// centered over the slot's foot. Washed in the accent when it is the
/// file on screen, its title set bold, lit under the pointer, and a press
/// on it shows the file. Rested on, it says the name in full, with what is
/// known of it under it, beside the panel's `edge` and level with the
/// slot's top: the name in the title is cut in its middle to the row.
fn file(pass: &mut Pass, ui: &mut egui::Ui, input: &Input, row: usize, rect: egui::Rect, edge: f32) {
    let Row {
        index,
        ref name,
        ref path,
        format,
        size,
        bytes,
        modified,
        thumb,
    } = input.rows[row];
    let known = Known {
        format,
        size,
        bytes,
        modified,
    };
    let theme = pass.theme;
    let control = Control::Thumb(row);
    let response = ui.interact(rect, ui.id().with(("filmstrip row", row)), Sense::CLICK);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, control.label()));
    let painter = ui.painter_at(rect);
    let current = input.current == Some(row);
    let hovered = response.hovered();
    let title = egui::Rect::from_min_size(rect.min, vec2(rect.width(), TITLE_HEIGHT));
    if current {
        painter.rect_filled(rect, 0.0, theme.accent.with_alpha(ACTIVE_BUTTON_WASH));
    } else if hovered {
        painter.rect_filled(rect, 0.0, theme.button_hover);
    } else {
        painter.rect_filled(title, 0.0, theme.button_idle);
    }

    // The slot, and the picture fitted and centered in it: the copy that
    // covers the slot's device pixels, drawn no larger than itself.
    let slot = egui::Rect::from_min_size(
        pos2(rect.left() + INSET, title.bottom() + INSET),
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
    let beside = egui::Rect::from_min_max(pos2(edge, slot.top()), pos2(edge, slot.bottom()));
    let response = pass.caption(
        response,
        Some(beside),
        vec![name.to_string()],
        known.about(path),
    );

    // What the list is sorted by, where that is not the name itself: a
    // label centered across the foot of the slot, a little way up it, on
    // a rounded wash of the bar's ground just wide enough for its words,
    // so that the picture shows round it.
    if let Some(value) = known.sorted_by(input.order.sort, path) {
        let ink: egui::Color32 = theme.text_primary.into();
        let font = egui::FontId::proportional(VALUE_SIZE);
        let room = (slot.width() - 2.0 * VALUE_PAD_X).max(0.0);
        let value = match input.order.sort {
            // A folder keeps its last part, the one its files are in.
            Sort::Path => {
                let last = value.rsplit('/').next().unwrap_or_default();
                cut_middle(ui, "", &value, last.chars().count() + 1, &font, room)
            }
            _ => value,
        };
        let galley = super::chooser::lit(ui, &value, &[], font, ink, ink, room);
        let size = galley.size() + vec2(2.0 * VALUE_PAD_X, 2.0 * VALUE_PAD_Y);
        // On whole logical pixels, so that the words sit as crisply as the
        // title's do.
        let backing = egui::Rect::from_min_size(
            pos2(slot.center().x - size.x / 2.0, slot.bottom() - VALUE_LIFT - size.y).round(),
            size,
        );
        painter.rect_filled(
            backing,
            VALUE_RADIUS,
            theme.bar_background.with_alpha(VALUE_WASH),
        );
        painter.galley(backing.min + vec2(VALUE_PAD_X, VALUE_PAD_Y), galley, ink);
    }

    // The title: the index, and the name cut in its middle to what is
    // left, in line with the slot under it.
    let (ink, family): (egui::Color32, _) = if current {
        (theme.text_bright.into(), egui::FontFamily::Name(fonts::BOLD.into()))
    } else if hovered {
        (theme.text_bright.into(), egui::FontFamily::Proportional)
    } else {
        (theme.text_primary.into(), egui::FontFamily::Proportional)
    };
    let font = egui::FontId::new(TEXT_SIZE, family);
    let room = (title.width() - 2.0 * INSET).max(0.0);
    let text = titled(ui, index, name, &font, room);
    let galley = super::chooser::lit(ui, &text, &[], font, ink, ink, room);
    painter.galley(
        pos2(title.left() + INSET, title.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );

    if response.clicked() {
        pass.press(control);
    }
}

/// What is known of a file from its header and the file system, as far as
/// they have been read.
#[derive(Clone, Copy)]
struct Known {
    format: Option<&'static str>,
    size: Option<(u32, u32)>,
    bytes: Option<u64>,
    modified: Option<SystemTime>,
}

impl Known {
    /// What its tooltip says under its name: the folder it is in; what
    /// kind of file it is, its size in pixels and its size on disk, a
    /// middot between each; and when it was last written, on this
    /// machine's clock.
    fn about(self, path: &str) -> Vec<String> {
        let folder = folder(path);
        let facts: Vec<String> = self
            .format
            .map(str::to_string)
            .into_iter()
            .chain(self.size.map(dimensions))
            .chain(self.bytes.map(super::info::round_bytes))
            .collect();
        let facts = (!facts.is_empty()).then(|| facts.join(" \u{b7} "));
        folder
            .into_iter()
            .chain(facts)
            .chain(self.modified.map(local_time))
            .collect()
    }

    /// What the list is sorted by, as the row shows it, for the file at
    /// `path`: `None` for a sort by name, which the title already says,
    /// and for a value not known yet. A sort by path shows the folder, the
    /// name being in the title.
    fn sorted_by(self, sort: Sort, path: &str) -> Option<String> {
        match sort {
            Sort::Name => None,
            Sort::Path => folder(path),
            Sort::Type => self.format.map(str::to_string),
            Sort::Date => self.modified.map(local_time),
            Sort::Size => self.bytes.map(super::info::round_bytes),
            Sort::Width | Sort::Height => self.size.map(dimensions),
            Sort::Area => self
                .size
                .map(|(width, height)| super::info::round_pixels(u64::from(width) * u64::from(height))),
        }
    }
}

/// The folder a file is in, as its path names it: `None` for a path that
/// names none.
fn folder(path: &str) -> Option<String> {
    std::path::Path::new(path)
        .parent()
        .map(|parent| parent.display().to_string())
        .filter(|folder| !folder.is_empty())
}

/// A size in pixels, as the bar writes one.
fn dimensions((width, height): (u32, u32)) -> String {
    format!("{width} \u{00d7} {height}")
}

/// When a file was last written, on this machine's clock: the list is read
/// here, beside the files, where the info panel's UTC is bound for
/// wherever its reader is.
fn local_time(time: SystemTime) -> String {
    let at = crate::clock::local(time);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        at.year, at.month, at.day, at.hour, at.minute
    )
}

/// A file's title, `index` and `name`, with the name cut in its middle to
/// fit `room` in `font` — `example-long-…-0.png` — keeping its extension
/// and a few characters before it.
fn titled(ui: &egui::Ui, index: usize, name: &str, font: &egui::FontId, room: f32) -> String {
    let tail_min = match name.rfind('.') {
        Some(dot) if dot > 0 => name[dot..].chars().count() + TAIL_KEPT,
        _ => TAIL_KEPT,
    };
    cut_middle(ui, &format!("{index}  "), name, tail_min, font, room)
}

/// `prefix` and then `text`, with `text` cut in its middle by
/// [`middle_cut`] to fit `room` in `font`, keeping at least its last
/// `tail_min` characters where they fit. Text that cannot be measured a
/// character at a time, or with no room for any of its start, is left
/// whole, for the layout to cut at its end.
fn cut_middle(
    ui: &egui::Ui,
    prefix: &str,
    text: &str,
    tail_min: usize,
    font: &egui::FontId,
    room: f32,
) -> String {
    let whole = format!("{prefix}{text}");
    let widths = |text: &str| -> Vec<f32> {
        let galley = ui.ctx().fonts_mut(|fonts| {
            fonts.layout_no_wrap(text.to_string(), font.clone(), egui::Color32::PLACEHOLDER)
        });
        galley
            .rows
            .iter()
            .flat_map(|row| row.glyphs.iter().map(|glyph| glyph.advance_width))
            .collect()
    };
    let prefix_width: f32 = widths(prefix).iter().sum();
    let ellipsis: f32 = widths(ELLIPSIS).iter().sum();
    let text_widths = widths(text);
    let chars: Vec<char> = text.chars().collect();
    if text_widths.len() != chars.len() {
        return whole;
    }
    match middle_cut(&text_widths, ellipsis, tail_min, room - prefix_width) {
        None | Some((0, _)) => whole,
        Some((head, tail)) => {
            let head: String = chars[..head].iter().collect();
            let tail: String = chars[chars.len() - tail..].iter().collect();
            format!("{prefix}{head}{ELLIPSIS}{tail}")
        }
    }
}

const ELLIPSIS: &str = "…";

/// How many characters of a name, `widths` wide one by one, to keep from
/// its start and from its end, with an ellipsis `ellipsis` wide between
/// them, to fit `room`; `None` when the whole name fits. The end is kept
/// first: its last `tail_min` characters where they fit in what is left
/// once the ellipsis is set, and otherwise half of it. The start takes
/// what the end leaves, and whatever the start cannot use goes back to
/// the end.
pub fn middle_cut(
    widths: &[f32],
    ellipsis: f32,
    tail_min: usize,
    room: f32,
) -> Option<(usize, usize)> {
    if widths.iter().sum::<f32>() <= room {
        return None;
    }
    let left = room - ellipsis;
    let n = widths.len();
    let (mut tail, mut tail_width) = (0, 0.0);
    while tail < n {
        let next = tail_width + widths[n - 1 - tail];
        if next > left || (tail >= tail_min && next > left / 2.0) {
            break;
        }
        tail += 1;
        tail_width = next;
    }
    let (mut head, mut head_width) = (0, 0.0);
    while head + tail < n && head_width + widths[head] + tail_width <= left {
        head_width += widths[head];
        head += 1;
    }
    while head + tail < n && head_width + tail_width + widths[n - 1 - tail] <= left {
        tail_width += widths[n - 1 - tail];
        tail += 1;
    }
    Some((head, tail))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rows on screen are the ones any part of which is in the
    /// viewport: the one the top edge cuts through, every one wholly in
    /// it, and the one the bottom edge cuts through.
    /// The tooltip says what is known: the folder always, the kind and
    /// sizes as far as the header has been read, and the date where there
    /// is one; and the row shows the value it is sorted by, but not a name,
    /// and of a path only the folder.
    #[test]
    fn a_file_says_what_is_known_of_it() {
        let when = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_756_632_722);
        let everything = Known {
            format: Some("PNG"),
            size: Some((1920, 1080)),
            bytes: Some(1_258_291),
            modified: Some(when),
        };
        let about = everything.about("a/b.png");
        assert_eq!(about[..2], ["a", "PNG \u{b7} 1920 \u{d7} 1080 \u{b7} 1.26 MB"]);
        assert_eq!(about[2], local_time(when));
        let nothing = Known {
            format: None,
            size: None,
            bytes: None,
            modified: None,
        };
        assert_eq!(nothing.about("b.png"), Vec::<String>::new(), "no folder, nothing read");
        let sized = Known {
            size: Some((640, 480)),
            ..nothing
        };
        assert_eq!(sized.about("a/b.png"), ["a", "640 \u{d7} 480"]);

        let sorted = |known: Known, sort| known.sorted_by(sort, "photos/2024/b.png");
        assert_eq!(sorted(everything, Sort::Name), None);
        assert_eq!(sorted(everything, Sort::Path).as_deref(), Some("photos/2024"));
        assert_eq!(nothing.sorted_by(Sort::Path, "b.png"), None, "no folder");
        assert_eq!(sorted(everything, Sort::Size).as_deref(), Some("1.26 MB"));
        assert_eq!(sorted(everything, Sort::Area).as_deref(), Some("2.07 MP"));
        assert_eq!(sorted(everything, Sort::Width).as_deref(), Some("1920 \u{d7} 1080"));
        assert_eq!(sorted(nothing, Sort::Date), None, "not known yet");
    }

    #[test]
    fn a_long_name_is_cut_in_its_middle_keeping_its_end() {
        let ten = [1.0; 10];
        assert_eq!(middle_cut(&ten, 1.0, 4, 10.0), None, "a name that fits is whole");
        assert_eq!(middle_cut(&ten, 1.0, 2, 7.0), Some((3, 3)), "halves, the end first");
        assert_eq!(middle_cut(&ten, 1.0, 5, 7.0), Some((1, 5)), "the end's minimum");
        assert_eq!(middle_cut(&ten, 1.0, 8, 7.0), Some((0, 6)), "the end, all there is room for");
        assert_eq!(middle_cut(&ten, 1.0, 4, 0.5), Some((0, 0)), "no room at all");
        // A wide character at the start the head cannot take goes to the end.
        let wide = [4.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
        assert_eq!(middle_cut(&wide, 1.0, 1, 6.0), Some((0, 5)));
    }

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
