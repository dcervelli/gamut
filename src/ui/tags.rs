//! The info panel's second tab: every tag `exiftool` reads from the file,
//! under a field that filters them, as a tree of the groups they are in.
//!
//! What the tab is drawn from is the application's, handed in as [`Input`]
//! on each frame the tab is on screen; what is pressed on it goes back as
//! commands. The rows are of two heights, a group's one line and a tag's
//! two, and where each starts down the list is worked out once when the
//! rows are built — see [`tops`] — so that a pass lays out only the rows in
//! view: a raw's maker notes run to hundreds.

use std::sync::Arc;

use egui::{Align, Frame, Key, Modifiers, Sense, TextEdit, WidgetInfo, WidgetType, pos2, vec2};

use super::chrome::{ICON_SIDE, Pass};
use super::control::{Command, Control};
use super::info::LABEL_SIZE;
use super::style::{SCROLLBAR_GUTTER, SCROLLBAR_WIDTH, TOGGLE_RADIUS};
use super::{HAIRLINE, TEXT_SIZE, fonts, icon};

/// What the field says while it is empty.
const HINT: &str = "Type to filter";
/// What stands in the tree's place when nothing fits the query, and when
/// the file has nothing to say at all.
const NOTHING: &str = "No matching tags.";
const EMPTY: &str = "No tags.";
/// What the tab says when the program is not there, line by line.
const NOT_FOUND: &str = "exiftool was not found.";
const REQUIRES: &str = "Raw data view requires exiftool to be installed on your system.";
#[cfg(not(target_os = "macos"))]
const INSTALL: &str =
    "Install your distribution's package: perl-image-exiftool, libimage-exiftool-perl or exiftool.";
#[cfg(target_os = "macos")]
const INSTALL: &str = "Install it with `brew install exiftool`, or the package from exiftool.org.";
const CONFIGURE: &str = "Or set `exiftool` in the configuration file to its path.";
/// And when it ran and refused the file, above the line it said why in.
const FAILED: &str = "exiftool could not read this file.";

/// The field's height, what its text is inset by, and the space under it.
const FIELD_HEIGHT: f32 = 26.0;
const FIELD_INSET: f32 = 8.0;
const FIELD_GAP: f32 = 6.0;
/// The line under the field saying how many tags there are, as tall as the
/// button at its end that folds or opens every group.
const COUNT_HEIGHT: f32 = 20.0;
/// The space between the count and that button.
const FOLD_GAP: f32 = 6.0;
const COUNT_GAP: f32 = 4.0;
/// A group's row, one line, and a tag's or a field's, two.
pub const GROUP_HEIGHT: f32 = 22.0;
pub const TAG_HEIGHT: f32 = 36.0;
/// The chevron beside a group's name, and the space after it.
const CHEVRON: f32 = 14.0;
const CHEVRON_GAP: f32 = 4.0;
/// How far each level of the tree is set in: as far as a group's name is
/// from its chevron, so that what is in a group starts under its name.
const INDENT: f32 = CHEVRON + CHEVRON_GAP;
/// The room kept at a row's ends.
const ROW_PADDING: f32 = 4.0;
/// What parts a tag's group from its name, where the row says both.
const SEPARATOR: &str = " \u{00b7} ";
/// The most of a line its raw value, or its group, is given.
const RAW_SHARE: f32 = 0.4;
/// How tall each button at the foot of the tab is while exiftool is not
/// found.
const BUTTON_HEIGHT: f32 = 26.0;
/// The space between the words and the buttons under them.
const BUTTONS_GAP: f32 = 12.0;
/// The space between the lines of what stands in the tree's place.
const STATE_GAP: f32 = 6.0;
/// The space between the mark beside [`NOTHING`] and the words.
const NOTHING_GAP: f32 = 6.0;

/// Which tab the info panel is on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tab {
    /// What the panel says of the file itself: the column of facts.
    #[default]
    Facts,
    /// Every tag exiftool reads.
    Tags,
}

impl Tab {
    /// What the tab says.
    pub fn label(self) -> &'static str {
        match self {
            Tab::Facts => "Curated",
            Tab::Tags => "Raw Data",
        }
    }
}

/// How the tags shown are copied: as exiftool writes them, as a table, or
/// as JSON or XML keeping each value's lists and structures.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Table {
    Text,
    Csv,
    Json,
    Xml,
}

impl Table {
    /// In the order the menu offers them.
    pub const ALL: [Table; 4] = [Table::Text, Table::Csv, Table::Json, Table::Xml];

    /// What its item says.
    pub fn label(self) -> &'static str {
        match self {
            Table::Text => "Plaintext",
            Table::Csv => "CSV",
            Table::Json => "JSON",
            Table::Xml => "XML",
        }
    }
}

/// What the button at the end of the count line does: open every group of
/// the tree, or fold them all shut.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fold {
    Open,
    Shut,
}

impl Fold {
    /// What it is called.
    pub fn label(self) -> &'static str {
        match self {
            Fold::Open => "Expand all",
            Fold::Shut => "Collapse all",
        }
    }
}

/// What a row of the tree is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// A group of tags: the kind of metadata, or where in it.
    Group,
    /// A tag.
    Tag,
    /// One piece of a tag holding a list or a structure, by its path.
    Field,
}

/// One row of the tree, as the frame draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub kind: Kind,
    /// How many levels in it is set.
    pub depth: u8,
    /// A group's name; a tag's title, exiftool's description of it or its
    /// name where it has none; or a field's path.
    pub name: String,
    /// What it says on its second line; a group says nothing there.
    pub value: String,
    /// The raw value, where it is something other than what is printed.
    pub raw: Option<String>,
    /// The group a tag is in, said before its name where there is no tree
    /// to say it: under a query, whose list is best first.
    pub group: Option<String>,
    /// The chars of the name and of the value the query was found at.
    pub name_lit: Vec<usize>,
    pub value_lit: Vec<usize>,
    /// Whether a group is folded shut, and how many tags it holds.
    pub collapsed: bool,
    pub count: usize,
}

impl Row {
    fn height(&self) -> f32 {
        match self.kind {
            Kind::Group => GROUP_HEIGHT,
            Kind::Tag | Kind::Field => TAG_HEIGHT,
        }
    }
}

/// Where each of `rows` starts down the list, and after them where the list
/// ends: what the rows in view are found by.
pub fn tops(rows: &[Row]) -> Vec<f32> {
    let mut tops = Vec::with_capacity(rows.len() + 1);
    let mut top = 0.0;
    tops.push(top);
    for row in rows {
        top += row.height();
        tops.push(top);
    }
    tops
}

/// Where the tags stand.
#[derive(Clone, Debug, PartialEq)]
pub enum State {
    /// exiftool is reading the file.
    Waiting,
    /// It is not there to run; `configured` is what it was looked for as.
    NotInstalled { configured: String },
    /// It ran and refused the file, saying this.
    Failed(String),
    /// Its tags are in.
    Ready,
}

/// What the tab is drawn from, on each frame it is on screen.
#[derive(Clone, Debug, PartialEq)]
pub struct Input {
    /// The file the tags are of, which the list's scroll is kept per.
    pub file: String,
    pub query: String,
    /// The version of exiftool that read them.
    pub version: Option<String>,
    pub state: State,
    /// The rows, and where each starts: see [`tops`].
    pub rows: Arc<[Row]>,
    pub tops: Arc<[f32]>,
    /// How many tags fit the query, of how many.
    pub shown: usize,
    pub total: usize,
    /// What the button at the end of the count line does — open every
    /// group while any is shut, and fold them all while none is — and
    /// `None` where there is no tree: under a query, or with no tags in.
    pub fold: Option<Fold>,
    /// The row of the tag last clicked, marked; and whether the list is to
    /// be scrolled to put it in view, which it is once, when a click on a
    /// query's list brings the tag up in the tree.
    pub marked: Option<usize>,
    pub reveal: bool,
}

/// One line of a copy of the tags: a tag, or one piece of a tag holding
/// more than one.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub group: String,
    pub subgroup: String,
    /// The tag's name, followed by the piece's path.
    pub name: String,
    pub id: String,
    pub description: String,
    pub value: String,
    pub raw: String,
}

/// The heads of a CSV copy.
const CSV_COLUMNS: &str = "Group,Subgroup,Tag,ID,Description,Value,Raw";
/// How wide the group and the name are set in a copy as text, as exiftool
/// sets them: `[IFD0]          Orientation                     : Rotate 180`.
const TEXT_GROUP: usize = 15;
const TEXT_NAME: usize = 32;

/// `lines` as a table under its heads. No line after the last, as the
/// Curated tab's copies have none.
pub fn csv(lines: &[Line]) -> String {
    std::iter::once(CSV_COLUMNS.to_string())
        .chain(lines.iter().map(|line| {
            [
                &line.group,
                &line.subgroup,
                &line.name,
                &line.id,
                &line.description,
                &line.value,
                &line.raw,
            ]
            .map(|field| super::info::quoted(field))
            .join(",")
        }))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `lines` laid out as exiftool prints them, the raw value after the
/// printed one in parentheses.
pub fn text(lines: &[Line]) -> String {
    lines
        .iter()
        .map(|line| {
            let raw = if line.raw.is_empty() {
                String::new()
            } else {
                format!(" ({})", line.raw)
            };
            format!(
                "{:<TEXT_GROUP$}{:<TEXT_NAME$}: {}{raw}",
                format!("[{}]", line.subgroup),
                line.name,
                line.value
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The field's id, which is how a click on it is told from the rest and how
/// `Esc` takes the keyboard back from it.
fn field_id() -> egui::Id {
    egui::Id::new("tags query")
}

/// Draws the tab's body, `width` wide: the field, the count under it, and
/// the tree — or what stands in its place.
pub(super) fn show(pass: &mut Pass, ui: &mut egui::Ui, input: &Input, width: f32) {
    // `Esc` while the field has the keyboard hands it back, rather than
    // reaching the window, which would read it as a key to act on.
    let focused = ui.memory(|memory| memory.has_focus(field_id()));
    if focused && ui.input_mut(|keys| keys.consume_key(Modifiers::NONE, Key::Escape)) {
        ui.memory_mut(|memory| memory.surrender_focus(field_id()));
    }
    field(pass, ui, input, width);
    ui.add_space(FIELD_GAP);
    count(pass, ui, input, width);
    ui.add_space(COUNT_GAP);
    let rest = ui.available_rect_before_wrap();
    match &input.state {
        State::Waiting => {
            let spinner = egui::Rect::from_center_size(rest.center(), egui::Vec2::splat(ICON_SIDE));
            ui.put(
                spinner,
                egui::Spinner::new()
                    .size(ICON_SIDE)
                    .color(pass.theme.text_dim),
            );
        }
        State::NotInstalled { configured } => {
            let first = if configured == "exiftool" {
                NOT_FOUND.to_string()
            } else {
                format!("{configured} was not found.")
            };
            let lines = [first.as_str(), REQUIRES, INSTALL, CONFIGURE];
            let below = said(pass, ui, rest, &lines, NOT_FOUND_BUTTONS_HEIGHT);
            not_found_buttons(pass, ui, rest, below);
        }
        State::Failed(why) => {
            said(pass, ui, rest, &[FAILED, why], 0.0);
        }
        State::Ready if input.rows.is_empty() && input.query.is_empty() => {
            said(pass, ui, rest, &[EMPTY], 0.0);
        }
        State::Ready if input.rows.is_empty() => nothing(pass, ui, width),
        State::Ready => tree(pass, ui, input, width),
    }
}

/// The field: a hand-drawn box holding a plain text edit, as the chooser's
/// is. It is not given the keyboard when the tab comes up — the panel is not
/// a popup, and a field holding the keyboard takes every key the window
/// answers — but by a click.
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
        .id(field_id())
        .return_key(None)
        .frame(Frame::NONE)
        .hint_text(HINT)
        .font(egui::FontId::proportional(TEXT_SIZE))
        .text_color(theme.text_primary.into())
        .desired_width(f32::INFINITY)
        .vertical_align(Align::Center)
        .margin(egui::Margin::ZERO);
    let response = ui.put(rect.shrink2(vec2(FIELD_INSET, 0.0)), edit);
    if response.changed() {
        pass.commands.push(Command::Filter(text));
    }
}

/// The line under the field: which exiftool read the tags, and how many
/// there are — or how many of them fit the query.
fn count(pass: &mut Pass, ui: &mut egui::Ui, input: &Input, width: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, COUNT_HEIGHT), Sense::HOVER);
    if input.state != State::Ready {
        return;
    }
    let room = match input.fold {
        Some(fold) => {
            let square = egui::Rect::from_min_size(
                pos2(rect.right() - COUNT_HEIGHT, rect.top()),
                egui::Vec2::splat(COUNT_HEIGHT),
            );
            fold_button(pass, ui, square, fold);
            width - COUNT_HEIGHT - FOLD_GAP
        }
        None => width,
    };
    let tags = |count: usize| {
        if count == 1 {
            "1 tag".to_string()
        } else {
            format!("{count} tags")
        }
    };
    let counted = if input.query.is_empty() {
        tags(input.total)
    } else {
        format!("{} of {}", input.shown, tags(input.total))
    };
    let words = match &input.version {
        Some(version) => format!("{counted} \u{00b7} exiftool {version}"),
        None => counted,
    };
    let ink: egui::Color32 = pass.theme.text_dim.into();
    let galley = super::chooser::lit(
        ui,
        &words,
        &[],
        egui::FontId::proportional(LABEL_SIZE),
        ink,
        ink,
        room.max(0.0),
    );
    let at = pos2(rect.left(), rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(at, galley, ink);
}

/// The button at the end of the count line, in `rect`: the pair of
/// chevrons pointing out to open every group, or in to fold them all.
fn fold_button(pass: &mut Pass, ui: &mut egui::Ui, rect: egui::Rect, fold: Fold) {
    let control = Control::TagsFold(fold);
    let response = ui.interact(rect, ui.id().with("tags fold"), Sense::CLICK);
    let (background, ink) = pass.button_ink(false, &response, true);
    ui.painter().rect_filled(rect, TOGGLE_RADIUS, background);
    let mark = match fold {
        Fold::Open => icon::CHEVRONS_UP_DOWN,
        Fold::Shut => icon::CHEVRONS_DOWN_UP,
    };
    icon::paint(
        ui.painter(),
        mark,
        icon::square(pass.grid, rect, CHEVRON),
        ink,
        background,
    );
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, control.label()));
    let response = pass.tooltip(response, super::tooltip::Tip::Control(control));
    if response.clicked() {
        pass.press(control);
    }
}

/// Lines of words, the first in the text's ink and the rest dim, centered
/// across `rect` and — with `after` more to go under them, the buttons that
/// follow them — the whole block centered down it: what stands in the
/// tree's place. Says where the last line ends.
fn said(pass: &Pass, ui: &mut egui::Ui, rect: egui::Rect, lines: &[&str], after: f32) -> f32 {
    let theme = pass.theme;
    let galleys: Vec<_> = lines
        .iter()
        .enumerate()
        .map(|(place, line)| {
            let ink: egui::Color32 = if place == 0 {
                theme.text_primary.into()
            } else {
                theme.text_dim.into()
            };
            let mut job = egui::text::LayoutJob::simple(
                (*line).to_string(),
                egui::FontId::proportional(LABEL_SIZE),
                ink,
                rect.width(),
            );
            job.halign = Align::Center;
            (ui.ctx().fonts_mut(|fonts| fonts.layout_job(job)), ink)
        })
        .collect();
    let words: f32 = galleys
        .iter()
        .map(|(galley, _)| galley.size().y)
        .sum::<f32>()
        + STATE_GAP * galleys.len().saturating_sub(1) as f32;
    let block = if after > 0.0 {
        words + BUTTONS_GAP + after
    } else {
        words
    };
    let mut top = (rect.center().y - block / 2.0).round().max(rect.top());
    for (galley, ink) in galleys {
        let height = galley.size().y;
        ui.painter().galley(pos2(rect.center().x, top), galley, ink);
        top += height + STATE_GAP;
    }
    top - STATE_GAP
}

/// How tall the buttons under the words are while exiftool is not found.
const NOT_FOUND_BUTTONS_HEIGHT: f32 = 3.0 * BUTTON_HEIGHT + 2.0 * STATE_GAP;

/// The buttons under the words while exiftool is not found, stacked from
/// `below`, where the words end: its website, the configuration file, and
/// a look for it again.
fn not_found_buttons(pass: &mut Pass, ui: &mut egui::Ui, rect: egui::Rect, below: f32) {
    let controls = [
        Control::VisitExiftool,
        Control::EditConfig,
        Control::TagsRefresh,
    ];
    let top = below + BUTTONS_GAP;
    let area = egui::Rect::from_min_max(
        pos2(rect.left(), top),
        pos2(rect.right(), top + NOT_FOUND_BUTTONS_HEIGHT),
    );
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(area)
            .layout(egui::Layout::top_down_justified(Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.y = STATE_GAP;
            for control in controls {
                let button =
                    egui::Button::new(egui::RichText::new(control.label()).size(TEXT_SIZE))
                        .sense(Sense::CLICK)
                        .min_size(vec2(0.0, BUTTON_HEIGHT));
                let response = ui.add(button);
                let response = pass.tooltip(response, super::tooltip::Tip::Control(control));
                if response.clicked() {
                    pass.press(control);
                }
            }
        },
    );
}

/// What the tree says when nothing fits the query: the chooser's mark and
/// [`NOTHING`] beside it, dim.
fn nothing(pass: &mut Pass, ui: &mut egui::Ui, width: f32) {
    let theme = pass.theme;
    let (rect, _) = ui.allocate_exact_size(vec2(width, FIELD_HEIGHT), Sense::HOVER);
    let ink: egui::Color32 = theme.text_dim.into();
    let galley = ui.ctx().fonts_mut(|fonts| {
        fonts.layout_no_wrap(
            NOTHING.to_owned(),
            egui::FontId::proportional(TEXT_SIZE),
            ink,
        )
    });
    let held = ICON_SIDE + NOTHING_GAP + galley.size().x;
    let x = rect.left() + ((rect.width() - held) / 2.0).round();
    let painter = ui.painter();
    let mark = egui::Rect::from_min_size(
        pos2(x, rect.center().y - ICON_SIDE / 2.0),
        vec2(ICON_SIDE, ICON_SIDE),
    );
    icon::paint(
        painter,
        icon::SEARCH_ALERT,
        icon::square(pass.grid, mark, ICON_SIDE),
        ink,
        theme.bar_background.into(),
    );
    let at = pos2(
        x + ICON_SIDE + NOTHING_GAP,
        rect.center().y - galley.size().y / 2.0,
    );
    painter.galley(at, galley, ink);
}

/// The tree, virtualized: only the rows in the viewport are laid out. Its
/// scroll is the file's own, as the Facts tab's column is.
fn tree(pass: &mut Pass, ui: &mut egui::Ui, input: &Input, width: f32) {
    let rows = &input.rows;
    let tops = &input.tops;
    let total = tops.last().copied().unwrap_or(0.0);
    ui.spacing_mut().scroll.bar_inner_margin = SCROLLBAR_GUTTER - SCROLLBAR_WIDTH;
    egui::ScrollArea::vertical()
        .id_salt(("tags", input.file.as_str()))
        .auto_shrink(false)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
        .show_viewport(ui, |ui, viewport| {
            ui.set_width(width - SCROLLBAR_GUTTER);
            ui.set_height(total);
            let left = ui.max_rect().left();
            let top = ui.max_rect().top();
            let across = ui.max_rect().width();
            // The last row starting above the viewport's top, through the
            // first starting below its bottom.
            let first = tops
                .partition_point(|&start| start <= viewport.min.y)
                .saturating_sub(1);
            let last = tops
                .partition_point(|&start| start < viewport.max.y)
                .min(rows.len());
            let rect_of = |index: usize| {
                egui::Rect::from_min_max(
                    pos2(left, top + tops[index]),
                    pos2(left + across, top + tops[index + 1]),
                )
            };
            for index in first..last {
                let marked = input.marked == Some(index);
                row(pass, ui, &rows[index], index, rect_of(index), marked);
            }
            // Put there, in the middle, rather than scrolled there: the tag
            // may be anywhere in hundreds of rows.
            if input.reveal
                && let Some(marked) = input.marked.filter(|&marked| marked < rows.len())
            {
                ui.scroll_to_rect_animation(
                    rect_of(marked),
                    Some(Align::Center),
                    egui::style::ScrollAnimation::none(),
                );
            }
        });
}

/// One row: a group's chevron, name and count, or a tag's name — after its
/// group, under a query — over its value. Washed under the pointer; a click folds a group or copies a
/// tag's value.
fn row(
    pass: &mut Pass,
    ui: &mut egui::Ui,
    item: &Row,
    index: usize,
    rect: egui::Rect,
    marked: bool,
) {
    let theme = pass.theme;
    let control = match item.kind {
        Kind::Group => Control::TagGroup(index),
        Kind::Tag | Kind::Field => Control::TagRow(index),
    };
    let response = ui.interact(rect, ui.id().with(("tag", index)), Sense::CLICK);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, control.label()));
    let painter = ui.painter_at(rect);
    if marked {
        painter.rect_filled(rect, TOGGLE_RADIUS, ui.visuals().selection.bg_fill);
    } else if response.hovered() {
        painter.rect_filled(rect, TOGGLE_RADIUS, theme.button_hover);
    }
    let dim: egui::Color32 = theme.text_dim.into();
    let hit: egui::Color32 = theme.hit.into();
    let left = rect.left() + ROW_PADDING + f32::from(item.depth) * INDENT;
    let right = rect.right() - ROW_PADDING;
    match item.kind {
        Kind::Group => {
            let mark = egui::Rect::from_min_size(
                pos2(left, rect.center().y - CHEVRON / 2.0),
                vec2(CHEVRON, CHEVRON),
            );
            let ground = if response.hovered() {
                theme.button_hover
            } else {
                theme.bar_background
            };
            icon::paint(
                &painter,
                if item.collapsed {
                    icon::CHEVRON_RIGHT
                } else {
                    icon::CHEVRON_DOWN
                },
                icon::square(pass.grid, mark, CHEVRON),
                dim,
                ground.into(),
            );
            let text = left + CHEVRON + CHEVRON_GAP;
            let bold = egui::FontId::new(LABEL_SIZE, egui::FontFamily::Name(fonts::BOLD.into()));
            let heading: egui::Color32 = theme.text_primary.into();
            let name = super::chooser::lit(
                ui,
                &item.name,
                &item.name_lit,
                bold,
                heading,
                hit,
                (right - text).max(0.0),
            );
            let after = text + name.size().x + CHEVRON_GAP;
            painter.galley(
                pos2(text, rect.center().y - name.size().y / 2.0),
                name,
                heading,
            );
            let count = super::chooser::lit(
                ui,
                &format!("({})", item.count),
                &[],
                egui::FontId::proportional(LABEL_SIZE),
                dim,
                dim,
                (right - after).max(0.0),
            );
            painter.galley(
                pos2(after, rect.center().y - count.size().y / 2.0),
                count,
                dim,
            );
        }
        Kind::Tag | Kind::Field => {
            let room = (right - left).max(0.0);
            let small = egui::FontId::proportional(LABEL_SIZE);
            let body = egui::FontId::proportional(TEXT_SIZE);
            // The ID at the end of the first line, and the name before it
            // in what is left.
            // The group first, where the row says it, and the name after
            // it in what is left.
            let group = item.group.as_ref().map(|group| {
                super::chooser::lit(
                    ui,
                    &format!("{group}{SEPARATOR}"),
                    &[],
                    small.clone(),
                    dim,
                    dim,
                    room * RAW_SHARE,
                )
            });
            let group_width = group.as_ref().map_or(0.0, |group| group.size().x);
            let name = super::chooser::lit(
                ui,
                &item.name,
                &item.name_lit,
                small,
                dim,
                hit,
                (room - group_width).max(0.0),
            );
            let line = name.size().y;
            let block = line + TEXT_SIZE + 4.0;
            let top = rect.center().y - block / 2.0;
            if let Some(group) = group {
                painter.galley(pos2(left, top), group, dim);
            }
            painter.galley(pos2(left + group_width, top), name, dim);
            // The value under them, cut to one line, and its raw value
            // after it in parentheses.
            let raw = item.raw.as_ref().map(|raw| {
                super::chooser::lit(
                    ui,
                    &format!(" ({raw})"),
                    &[],
                    body.clone(),
                    dim,
                    dim,
                    room * RAW_SHARE,
                )
            });
            let raw_width = raw.as_ref().map_or(0.0, |raw| raw.size().x);
            let ink: egui::Color32 = theme.text_primary.into();
            let value = super::chooser::lit(
                ui,
                &item.value,
                &item.value_lit,
                body,
                ink,
                hit,
                (room - raw_width).max(0.0),
            );
            let second = top + line + 2.0;
            let value_width = value.size().x;
            painter.galley(pos2(left, second), value, ink);
            if let Some(raw) = raw {
                painter.galley(pos2(left + value_width, second), raw, dim);
            }
        }
    }
    if response.clicked() {
        pass.press(control);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(kind: Kind) -> Row {
        Row {
            kind,
            depth: 0,
            name: String::new(),
            value: String::new(),
            raw: None,
            group: None,
            name_lit: Vec::new(),
            value_lit: Vec::new(),
            collapsed: false,
            count: 0,
        }
    }

    /// Each row starts where the last ended, a group one line down and a
    /// tag two, and the last top is where the list ends.
    #[test]
    fn the_tops_add_up_the_two_heights() {
        let rows = [row(Kind::Group), row(Kind::Tag), row(Kind::Field)];
        assert_eq!(
            tops(&rows),
            [
                0.0,
                GROUP_HEIGHT,
                GROUP_HEIGHT + TAG_HEIGHT,
                GROUP_HEIGHT + 2.0 * TAG_HEIGHT
            ]
        );
        assert_eq!(tops(&[]), [0.0]);
    }

    fn line(name: &str, value: &str, raw: &str) -> Line {
        Line {
            group: "EXIF".to_string(),
            subgroup: "IFD0".to_string(),
            name: name.to_string(),
            id: "274".to_string(),
            description: "Orientation".to_string(),
            value: value.to_string(),
            raw: raw.to_string(),
        }
    }

    /// A table under its heads, quoted where a field needs it, and the
    /// same as text, as exiftool writes it; neither ends in a new line.
    #[test]
    fn the_copies_are_a_table_and_exiftools_text() {
        let lines = [
            line("Orientation", "Rotate 180", "3"),
            line("Artist", "Doe, Jane", ""),
        ];
        assert_eq!(
            csv(&lines),
            "Group,Subgroup,Tag,ID,Description,Value,Raw\n\
             EXIF,IFD0,Orientation,274,Orientation,Rotate 180,3\n\
             EXIF,IFD0,Artist,274,Orientation,\"Doe, Jane\","
        );
        assert_eq!(
            text(&lines),
            "[IFD0]         Orientation                     : Rotate 180 (3)\n\
             [IFD0]         Artist                          : Doe, Jane"
        );
        assert_eq!(text(&[]), "");
    }
}
