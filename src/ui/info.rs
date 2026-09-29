//! The info panel: what the file is, as a column of words down the right of
//! the content area.
//!
//! The only part of the interface with more to say than fits, so it is the
//! only part that scrolls: the column lives in a scroll area, under a header
//! that does not scroll, holding the one instruction the panel needs and the
//! button that copies the whole of it.
//!
//! It is also the only part of the interface that is read out rather than
//! merely read: a click on a field or on a heading puts it on the clipboard.
//! [`Contents`] is the words, and one index runs through it for the drawing
//! and the copying alike, so what is drawn lit and what lands on the
//! clipboard cannot come to disagree.

use std::time::SystemTime;

use egui::{
    Align, Label, Layout, RichText, Sense, StrokeKind, UiBuilder, Vec2, WidgetInfo, WidgetType,
    pos2, vec2,
};

use crate::clock;
use crate::image::AlphaMode;
use crate::image::decode::Rendering;
use crate::image::exif;
use crate::image::gain_map::Lift;
use crate::image::sequence::{Loops, Sequence};
use crate::render::Color;

use super::Rect;
use crate::theme::Theme;

use super::chrome::{ICON_SIDE, Pass, measure};
use super::control::Control;
use super::icon;
use super::style::{SCROLLBAR_GUTTER, SCROLLBAR_WIDTH, TOGGLE_RADIUS};
use super::tooltip::{Tip, Tooltip};
use super::{
    Current, PADDING, PANEL_INSET, PANEL_RADIUS, PANEL_WIDTH, RULE_WIDTH, TEXT_SIZE, rule,
};

/// Below this the panel would show its header, two facts and a scrollbar, so
/// it stays off instead. There is no matching minimum for the width: the
/// panel is [`PANEL_WIDTH`] wide or it is not on screen.
pub(super) const INFO_MIN_HEIGHT: f32 = 160.0;

/// The size a field's name is written at, against [`TEXT_SIZE`] for its
/// value: the values are what is being read, and the names only say which is
/// which.
const LABEL_SIZE: f32 = TEXT_SIZE * 0.85;

/// The space above a field's name. Enough that a name reads as belonging to
/// the value under it rather than to the one above, and no more: the column
/// is long, and every pixel spent parting two fields is a pixel of some
/// further field pushed off the bottom of the panel.
const FIELD_GAP: f32 = 6.0;
/// The space above a section's name, which has to part two sections more
/// plainly than a field parts two fields — near enough twice as plainly, with
/// a hairline drawn through the middle of it doing part of the parting.
const SECTION_GAP: f32 = 11.0;
/// The space between a field's name and its value. Less than nothing: a line
/// box carries its own leading above the glyphs, so the two lines are pulled
/// a pixel into one another's boxes without their ink coming any closer, and
/// a name and what it names read as one thing rather than as two.
const LABEL_GAP: f32 = -1.0;
/// The space between two lines of a section written as prose rather than as
/// fields, which are one account of one thing and are set as a paragraph is.
const LINE_GAP: f32 = 1.0;
/// The space between a heading's mark and its name.
const MARK_GAP: f32 = 6.0;
/// The space under the head of a section drawn with a mark: less than
/// [`FIELD_GAP`], the mark already making the head's line taller than the
/// words in it.
const HEAD_GAP: f32 = 3.0;
/// The space between a table's two columns.
const COLUMN_GAP: f32 = 8.0;
/// The most of the column's width a table's names are given, so that a long
/// name wraps rather than leaving its value no room.
const NAMES_SHARE: f32 = 0.45;

/// The fields of the section about the file on disk. Named here because the
/// section is drawn from them by name rather than as a column of fields, and
/// the names are what a copy of the section writes down beside each.
const NAME: &str = "Name";
const FOLDER: &str = "Folder";
const SIZE: &str = "Size";
const MODIFIED: &str = "Modified";
/// The fields of the section about the picture that stand at its head
/// rather than in its table.
const RESOLUTION: &str = "Resolution";
const READ_BY: &str = "Read by";

/// The words at the top of the panel. An instruction rather than a fact about
/// the file, so it is written small and dim: what it says is worth knowing
/// once and not worth re-reading every time the panel is opened.
const HINT: &str = "Click to copy section or item.";

/// The space under the header, with the hairline that parts it from the
/// column through the middle of it. The help popup keeps the same under its
/// headings.
pub(super) const HEADER_GAP: f32 = 11.0;

/// A copy button: how tall it is, what is kept clear inside it at either end,
/// and the space between its label and its mark.
const CHIP_HEIGHT: f32 = 20.0;
const CHIP_PADDING: f32 = 7.0;
const CHIP_GAP: f32 = 5.0;
/// The room set aside for the mark on a copy button, in the button's width
/// as well as in what [`icon::square`] sizes a square out of. A side
/// toggle's, so that the two marks are drawn at one size wherever they are
/// seen together.
const COPY_ICON: f32 = ICON_SIDE;

/// What the file itself says about the image, as opposed to what its pixels
/// do: read once when the image opens, since none of it changes while the
/// image is on screen.
#[derive(Clone, Debug, Default)]
pub struct FileFacts {
    /// The whole path as given, which is what the panel shows; the name on
    /// its own is [`Current::label`].
    pub path: String,
    /// `None` when the file could not be stat'd — it may have been replaced
    /// between being read and being asked about.
    pub bytes: Option<u64>,
    pub modified: Option<SystemTime>,
    /// The decoder that claimed the file, which is chosen by what its bytes
    /// say rather than by what its name does. `None` on the same terms as the
    /// two above.
    pub reader: Option<&'static str>,
}

/// What a row of the column is, which is what it is drawn as: the name of a
/// section, the name of a field, or what that field says.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Heading,
    Label,
    Value,
}

impl Kind {
    /// The size it is written at.
    fn size(self) -> f32 {
        match self {
            Kind::Heading | Kind::Value => TEXT_SIZE,
            Kind::Label => LABEL_SIZE,
        }
    }

    fn ink(self, theme: &Theme) -> Color {
        // The panel is the bars' own ground, so the words on it are the bars'
        // own ink; a heading is the one thing on it that is picked out.
        match self {
            Kind::Heading => theme.accent,
            Kind::Label => theme.text_dim,
            Kind::Value => theme.text_primary,
        }
    }
}

/// Something the panel can put on the clipboard: the whole of it, one
/// section of it, or one field.
///
/// The three run over different stretches of the same list, and say as much
/// of the table as the stretch is worth: three columns, two, or none — see
/// [`copied`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Copyable {
    /// Every field the panel says, which is the button at the top of it.
    All,
    /// One section, by its place down the column.
    Section(usize),
    /// One field, by its place in the column's fields — headings not counted,
    /// since a heading is not one of the rows that go on the clipboard.
    Fact(usize),
}

impl Copyable {
    /// What the button for it says, if anything.
    ///
    /// Only the one at the top, which is on screen whether or not anything is
    /// being pointed at and has to say what it would take. The two that
    /// appear under the pointer wear the mark alone: the header has already
    /// said that a click copies, and a button that repeated it would say the
    /// same three words over every field in the column.
    fn label(self) -> Option<&'static str> {
        match self {
            Copyable::All => Some("Copy All"),
            Copyable::Section(_) | Copyable::Fact(_) => None,
        }
    }
}

/// How a section is drawn. The copying is the same for every one of them —
/// a section is its fields, in order, whatever it looks like — and only the
/// drawing is told apart.
#[derive(Clone, Copy)]
enum Face {
    /// Each field's name over its value, under the section's name: every
    /// section the metadata reads out, and any that has no drawing of its own.
    Fields,
    /// The file on disk, as a few lines of prose under a heading with a mark:
    /// see [`file_section`].
    File,
    /// A section with a head of its own: some of its fields written on one
    /// line beside a mark, and the rest as a table under them — see
    /// [`headed_section`]. The picture, headed by its size and its format,
    /// and the camera, headed by its name.
    Headed {
        mark: &'static [icon::Mark],
        head: &'static [&'static str],
    },
}

/// One section of the column: its name, how it is drawn, and its fields.
struct Section {
    name: &'static str,
    face: Face,
    facts: Vec<Fact>,
}

impl Section {
    /// A section drawn as a column of fields.
    fn fields(name: &'static str, facts: Vec<Fact>) -> Section {
        Section {
            name,
            face: Face::Fields,
            facts,
        }
    }
}

/// One field as the clipboard takes it. Which section it stands under is
/// [`Contents`]'s to know, that being the same for every field in one.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Fact {
    name: String,
    value: String,
}

/// What the panel has to say, before any of it is measured: the sections in
/// the order they are read, each with the fields that came to something.
///
/// Held apart from the layout because a copy wants the words without the
/// geometry and needs no fonts to get them, and because the drawing and the
/// hit testing then count the same list rather than two that could come to
/// disagree about which field is which.
struct Contents {
    sections: Vec<Section>,
}

impl Contents {
    /// Every field in the column, in reading order, with the section it
    /// stands under. This is the order [`Copyable::Fact`] counts in.
    fn facts(&self) -> impl Iterator<Item = (&'static str, &Fact)> {
        self.sections
            .iter()
            .flat_map(|section| section.facts.iter().map(move |fact| (section.name, fact)))
    }
}

/// Where the panel goes: down the right of `content`, starting under the
/// histogram when that is showing as well — `above`, which is where it is —
/// and at the top of the content when it is not, the order the two toggles
/// are stacked in.
///
/// `None` when the window has no room for a column worth reading, which is
/// also what keeps the panel off screen rather than shrunk to nothing.
///
/// It is one width or it is not there: the column is read at the same measure
/// whatever the window is doing. In a window narrow enough for the panel and
/// the minimap to want the same strip the panel takes it, being drawn after —
/// it is on screen because it was asked for, and the minimap is a guide to a
/// picture the panel is already covering.
pub fn panel(content: Rect, above: Option<Rect>) -> Option<Rect> {
    // The histogram takes the top of the column's strip; the panel starts
    // below it rather than being drawn over it. Only where the histogram is
    // on screen, which the caller settles from the window as well as the
    // toggle: a window with no room for the plot is not one the column has
    // to start below. It arrives as a rectangle rather than as a constant
    // because it may not be there at all.
    let taken = above.map_or(0.0, |histogram| histogram.height + PADDING);
    let below = Rect::new(
        content.x,
        content.y + taken,
        content.width,
        content.height - taken,
    );
    super::panel::fit(
        below,
        [PANEL_WIDTH, f32::INFINITY],
        [PANEL_WIDTH, INFO_MIN_HEIGHT],
        super::panel::Place::TopRight,
    )
}

/// The width of the button for `copies`: its mark, whatever label goes
/// before it, and what is kept clear around them.
fn chip_width(ui: &egui::Ui, copies: Copyable) -> f32 {
    let label = copies.label().map_or(0.0, |label| {
        measure(ui, label) * LABEL_SIZE / TEXT_SIZE + CHIP_GAP
    });
    (2.0 * CHIP_PADDING + label + COPY_ICON).round()
}

/// Paints a copy button in `rect`: its label, and after it the mark for the
/// two sheets a copy makes of one.
///
/// On opaque ground rather than the panel's own, because the one in the
/// column is drawn over the words it would copy and they must not show
/// through it; and outlined, because ground the color of the panel it sits
/// on would otherwise leave it no edge.
fn chip(pass: &Pass, ui: &egui::Ui, rect: egui::Rect, copies: Copyable, hover: bool) {
    let theme = pass.theme;
    let (edge, ink): (egui::Color32, egui::Color32) = if hover {
        (theme.accent.into(), theme.text_primary.into())
    } else {
        (theme.border.into(), theme.text_dim.into())
    };
    let ground: egui::Color32 = theme.bar_background.into();
    let painter = ui.painter();
    painter.rect_filled(rect, TOGGLE_RADIUS, ground);
    painter.rect_stroke(
        rect,
        TOGGLE_RADIUS,
        egui::Stroke::new(RULE_WIDTH, edge),
        StrokeKind::Inside,
    );

    // Centered as one, so that a button wearing only the mark has it in the
    // middle rather than pushed to the end a label would have started at.
    let font = egui::FontId::proportional(LABEL_SIZE);
    let label = copies.label().map(|label| {
        let galley = ui
            .ctx()
            .fonts_mut(|fonts| fonts.layout_no_wrap(label.to_string(), font.clone(), ink));
        (galley.size().x, galley)
    });
    let held = label.as_ref().map_or(0.0, |(width, _)| width + CHIP_GAP) + COPY_ICON;
    let x = rect.min.x + ((rect.width() - held) / 2.0).round();
    if let Some((_, galley)) = &label {
        let at = pos2(x, rect.center().y - galley.size().y / 2.0);
        painter.galley(at, galley.clone(), ink);
    }
    // The chip's own ground, which is opaque, is what the sheet in front is
    // knocked out of: the mark has to read as one sheet over another rather
    // than as a lattice, and a wash would show the sheet behind through it.
    let mark = egui::Rect::from_min_size(
        pos2(
            x + label.as_ref().map_or(0.0, |(width, _)| width + CHIP_GAP),
            rect.min.y,
        ),
        vec2(COPY_ICON, rect.height()),
    );
    icon::paint(
        painter,
        icon::COPY,
        icon::square(pass.grid, mark, COPY_ICON),
        ink,
        ground,
    );
}

/// The one copy button that is always on screen, at the head of the panel:
/// it says what it would take, since the header has not yet said that a
/// click copies.
fn copy_all(pass: &mut Pass, ui: &mut egui::Ui) {
    let width = chip_width(ui, Copyable::All);
    let (rect, response) = ui.allocate_exact_size(vec2(width, CHIP_HEIGHT), Sense::CLICK);
    chip(pass, ui, rect, Copyable::All, response.hovered());
    let control = Control::Facts(Copyable::All);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, control.label()));
    let response = pass.tooltip(response, Tip::Control(control));
    if response.clicked() {
        pass.press(control);
    }
}

/// Draws the panel: the header, and under it the column of everything the
/// file has to say, scrolled by egui and read out by a click.
pub(super) fn show(pass: &mut Pass, ui: &mut egui::Ui) {
    let Some(current) = pass.current else {
        return;
    };
    let content = pass.content;
    let Some(panel) = panel(content, super::histogram_shown(content, pass.panels)) else {
        return;
    };
    let theme = pass.theme;
    if pass.input.waiting {
        super::panel::waiting(ui.ctx(), "info", None, panel, theme);
        return;
    }
    let area = egui::Rect::from_min_size(pos2(panel.x, panel.y), vec2(panel.width, panel.height));
    super::panel::area("info", panel, egui::Order::Middle).show(ui.ctx(), |ui| {
        egui::Frame::NONE
            .fill(theme.panel_background.into())
            .corner_radius(PANEL_RADIUS)
            .inner_margin(egui::Margin::same(PANEL_INSET as i8))
            .show(ui, |ui| {
                let inside = area.size() - Vec2::splat(2.0 * PANEL_INSET);
                ui.set_min_size(inside);
                ui.set_max_size(inside);
                ui.spacing_mut().item_spacing = Vec2::ZERO;

                // The header: the button first, and the hint takes what
                // is left, wrapping into it. The button has a size it
                // must be to be pressed and the hint is words, which set
                // on two lines as readily as on one.
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        copy_all(pass, ui);
                        ui.add_space(CHIP_GAP);
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            ui.add(
                                Label::new(
                                    RichText::new(HINT).size(LABEL_SIZE).color(theme.text_dim),
                                )
                                .wrap(),
                            );
                        });
                    });
                });
                // A hairline between the header and the column, which
                // says that the column runs on under the header rather
                // than stopping short of it.
                ui.add_space((HEADER_GAP - RULE_WIDTH) / 2.0);
                rule(pass, ui, inside.x - SCROLLBAR_GUTTER);
                ui.add_space((HEADER_GAP - RULE_WIDTH) / 2.0);

                column(pass, ui, current);
            });
    });
}

/// The column, in a scroll area with the bar down the panel's inner edge and
/// a gutter kept clear for it whether or not there is anything to scroll.
///
/// A different picture is a different column of words about it, and it is
/// read from the top: the scroll area is the file's own, so stepping to
/// another file starts at the top of its column rather than however far
/// down the last one had been read.
fn column(pass: &mut Pass, ui: &mut egui::Ui, current: &Current) {
    let contents = contents(current);
    ui.spacing_mut().scroll.bar_inner_margin = SCROLLBAR_GUTTER - SCROLLBAR_WIDTH;
    egui::ScrollArea::vertical()
        .id_salt(("info column", current.file.path.as_str()))
        .auto_shrink(false)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            let width = ui.available_width();
            let mut index = 0;
            for (place, section) in contents.sections.iter().enumerate() {
                // The first heading opens the column and has nothing above
                // it to be parted from; every one after it is a section
                // starting, parted from the last by a hairline through the
                // middle of the space above it, so the gap reads as
                // belonging to neither section more than the other.
                if place > 0 {
                    ui.add_space((SECTION_GAP - RULE_WIDTH) / 2.0);
                    rule(pass, ui, width);
                    ui.add_space((SECTION_GAP - RULE_WIDTH) / 2.0);
                }
                match section.face {
                    Face::Fields => fields_section(pass, ui, place, section, index, width),
                    Face::File => file_section(pass, ui, current, section, index, width),
                    Face::Headed { mark, head } => {
                        headed_section(pass, ui, place, section, index, width, mark, head)
                    }
                }
                index += section.facts.len();
            }
        });
}

/// A section as a column of fields under its name. `place` is where the
/// section is down the column and `first` where its first field is in the
/// column's fields, which is what a click on either copies.
fn fields_section(
    pass: &mut Pass,
    ui: &mut egui::Ui,
    place: usize,
    section: &Section,
    first: usize,
    width: f32,
) {
    block(pass, ui, Copyable::Section(place), width, |ui| {
        words(ui, Kind::Heading, section.name, pass.theme);
    });
    for (index, fact) in (first..).zip(&section.facts) {
        ui.add_space(FIELD_GAP);
        // A field's name and the value under it are one block, being one
        // thing to point at and one row to copy.
        block(pass, ui, Copyable::Fact(index), width, |ui| {
            words(ui, Kind::Label, &fact.name, pass.theme);
            ui.add_space(LABEL_GAP);
            words(ui, Kind::Value, &fact.value, pass.theme);
        });
    }
}

/// The file on disk, written as it would be said rather than as a table:
/// its name at the head, the folder it is in under that, and then how big
/// it is and how long ago it was last written.
///
/// Every piece is still a field, and a click on one copies that field as a
/// click on a field anywhere else does. The size and the date are written
/// roundly, as they are read at a glance, and each is exact in its tooltip
/// and on the clipboard: "2 weeks ago" is no use pasted anywhere. With the
/// file's name standing at the head, there is no heading left to copy the
/// section by; Copy All still has it.
fn file_section(
    pass: &mut Pass,
    ui: &mut egui::Ui,
    current: &Current,
    section: &Section,
    first: usize,
    width: f32,
) {
    let theme = pass.theme;
    let field = |name: &str| find(section, first, name);
    // The name cut in its middle to the room beside the mark, keeping its
    // extension: a name cut at its end loses what kind of file it is.
    let name = field(NAME).map(|(index, name)| {
        let font = egui::FontId::proportional(TEXT_SIZE);
        let room = width - COPY_ICON - MARK_GAP;
        let shown = super::filmstrip::cut_name(ui, "", &name.value, &font, room);
        let whole = (shown != name.value).then(|| name.value.clone());
        Piece {
            index,
            shown,
            exact: whole,
        }
    });
    line(pass, ui, width, Some(icon::FILE), theme.accent, None, name);
    if let Some((index, folder)) = field(FOLDER) {
        ui.add_space(HEAD_GAP);
        block(pass, ui, Copyable::Fact(index), width, |ui| {
            // At the size of the rest, in the names' ink: where the file
            // is matters less than what it is.
            let text = RichText::new(&folder.value)
                .size(TEXT_SIZE)
                .color(theme.text_dim);
            ui.add(Label::new(text).wrap());
        });
    }
    let file = &current.file;
    let size = field(SIZE)
        .zip(file.bytes)
        .map(|((index, _), bytes)| Piece {
            index,
            shown: round_bytes(bytes),
            exact: Some(exact_bytes(bytes)),
        });
    let modified = field(MODIFIED)
        .zip(file.modified)
        .map(|((index, _), time)| Piece {
            index,
            shown: ago(time, SystemTime::now()),
            exact: Some(format_time(time)),
        });
    if size.is_some() || modified.is_some() {
        ui.add_space(LINE_GAP);
        line(
            pass,
            ui,
            width,
            None,
            theme.text_primary,
            Some("\u{00b7}"),
            size.into_iter().chain(modified),
        );
    }
}

/// A section headed by the fields named in `head`, written on one line after
/// `mark` in the headings' ink, each copying itself, and under them the rest
/// of its fields as a table of two columns, each row copying its value.
///
/// A section with none of the fields its head is made of is headed by its
/// own name instead, which copies the section, as a column of fields is.
#[allow(clippy::too_many_arguments)]
fn headed_section(
    pass: &mut Pass,
    ui: &mut egui::Ui,
    place: usize,
    section: &Section,
    first: usize,
    width: f32,
    mark: &[icon::Mark],
    head: &[&str],
) {
    let theme = pass.theme;
    let pieces: Vec<Piece> = head
        .iter()
        .filter_map(|name| find(section, first, name))
        .map(|(index, fact)| Piece {
            index,
            shown: fact.value.clone(),
            exact: None,
        })
        .collect();
    if pieces.is_empty() {
        let grid = pass.grid;
        block(pass, ui, Copyable::Section(place), width, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = MARK_GAP;
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(COPY_ICON), Sense::HOVER);
                icon::paint(
                    ui.painter(),
                    mark,
                    icon::square(grid, rect, COPY_ICON),
                    theme.accent.into(),
                    theme.panel_background.into(),
                );
                words(ui, Kind::Heading, section.name, theme);
            });
        });
    } else {
        line(pass, ui, width, Some(mark), theme.accent, None, pieces);
    }
    let rows = (first..)
        .zip(&section.facts)
        .filter(|(_, fact)| !head.contains(&fact.name.as_str()));
    if rows.clone().next().is_some() {
        ui.add_space(HEAD_GAP);
        table(pass, ui, rows, width);
    }
}

/// The field of `section` called `name`, with its place in the column's
/// fields, `first` being where the section's first one is.
fn find<'a>(section: &'a Section, first: usize, name: &str) -> Option<(usize, &'a Fact)> {
    (first..)
        .zip(&section.facts)
        .find(|(_, fact)| fact.name == name)
}

/// A field written as part of a line rather than under its name: what the
/// line shows of it, and what its tooltip says where that is more.
struct Piece {
    index: usize,
    shown: String,
    exact: Option<String>,
}

/// One line of `pieces` in `ink`, after `mark` where there is one and with
/// `between` set between each two, broken to `width` where they do not fit.
///
/// Each piece is pointed at and copied on its own, and says in its tooltip
/// what it stands for where it is written short. The copy button goes where
/// every other one in the column does, at the column's end, level with the
/// piece pointed at.
fn line(
    pass: &mut Pass,
    ui: &mut egui::Ui,
    width: f32,
    mark: Option<&[icon::Mark]>,
    ink: Color,
    between: Option<&str>,
    pieces: impl IntoIterator<Item = Piece>,
) {
    let grid = pass.grid;
    let ground = pass.theme.panel_background;
    let left = ui.cursor().min.x;
    ui.horizontal_wrapped(|ui| {
        ui.set_width(width);
        let space = measure(ui, " ");
        ui.spacing_mut().item_spacing.x = space;
        let text = |text: &str| RichText::new(text).size(TEXT_SIZE).color(ink);
        if let Some(mark) = mark {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(COPY_ICON), Sense::HOVER);
            icon::paint(
                ui.painter(),
                mark,
                icon::square(grid, rect, COPY_ICON),
                ink.into(),
                ground.into(),
            );
            ui.add_space(MARK_GAP - space);
        }
        for (count, piece) in pieces.into_iter().enumerate() {
            if count > 0
                && let Some(between) = between
            {
                ui.label(text(between));
            }
            let response = ui.add(Label::new(text(&piece.shown)).wrap().sense(Sense::CLICK));
            let response = match piece.exact {
                Some(exact) => said(pass.theme, response, exact),
                None => response,
            };
            let across = egui::Rect::from_x_y_ranges(left..=left + width, response.rect.y_range());
            let pointed = response.contains_pointer();
            offer(
                pass,
                ui,
                Copyable::Fact(piece.index),
                &response,
                across,
                pointed,
            );
        }
    });
}

/// `rows` as a table of two columns: each field's name, dim, in a column as
/// wide as the widest of them needs — up to [`NAMES_SHARE`] of `width` — and
/// its value beside it, broken to what is left. A row is one block, copying
/// its value.
///
/// A value [`short`] has a shorter way of saying is written that way, and
/// says the whole of it in its tooltip; the copy is the whole of it too.
fn table<'a>(
    pass: &mut Pass,
    ui: &mut egui::Ui,
    rows: impl Iterator<Item = (usize, &'a Fact)> + Clone,
    width: f32,
) {
    let theme = pass.theme;
    let font = egui::FontId::proportional(TEXT_SIZE);
    let names = rows
        .clone()
        .map(|(_, fact)| {
            ui.ctx().fonts_mut(|fonts| {
                fonts
                    .layout_no_wrap(fact.name.clone(), font.clone(), egui::Color32::PLACEHOLDER)
                    .size()
                    .x
            })
        })
        .fold(0.0, f32::max)
        .min(width * NAMES_SHARE)
        .ceil();
    for (count, (index, fact)) in rows.enumerate() {
        if count > 0 {
            ui.add_space(LINE_GAP);
        }
        block(pass, ui, Copyable::Fact(index), width, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = COLUMN_GAP;
                ui.scope(|ui| {
                    ui.set_min_width(names);
                    ui.set_max_width(names);
                    let text = RichText::new(&fact.name)
                        .size(TEXT_SIZE)
                        .color(theme.text_dim);
                    ui.add(Label::new(text).wrap());
                });
                let shown = short(fact);
                let text = RichText::new(shown.as_deref().unwrap_or(&fact.value))
                    .size(TEXT_SIZE)
                    .color(theme.text_primary);
                let response = ui.add(Label::new(text).wrap());
                if shown.is_some() {
                    said(theme, response, fact.value.clone());
                }
            });
        });
    }
}

/// A shorter way of saying `fact`'s value, where the table has one: when a
/// photograph was taken, as how long ago — "3 weeks ago" — the date itself
/// being what the tooltip and the clipboard are for.
fn short(fact: &Fact) -> Option<String> {
    if fact.name != exif::TAKEN {
        return None;
    }
    let taken = clock::parse(&fact.value)?;
    Some(ago(taken, SystemTime::now()))
}

/// `response` with a tooltip of the one line `words`: a fact written short,
/// said in full. No key does what a click on it does, so there is nothing for
/// [`Pass::tooltip`]'s keymap to add, and nothing here for it to compose.
fn said(theme: &Theme, response: egui::Response, words: String) -> egui::Response {
    let theme = *theme;
    let tooltip = Tooltip {
        title: vec![words],
        hints: Vec::new(),
    };
    response.on_hover_ui(move |ui| super::tooltip::show(ui, &tooltip, &theme))
}

/// How long before `now` the file was written, as it is said: "2 weeks ago".
/// A time after `now` — a clock set wrong somewhere — is said as now rather
/// than as a time to come, which would read as a mistake of this panel's.
fn ago(time: SystemTime, now: SystemTime) -> String {
    let since = now.duration_since(time).unwrap_or_default();
    timeago::Formatter::new().convert(since)
}

/// A file's size exactly: `1,258,291 bytes`.
fn exact_bytes(bytes: u64) -> String {
    match bytes {
        1 => "1 byte".to_string(),
        _ => format!("{} bytes", grouped(bytes)),
    }
}

/// One run of words in the column, broken to its width.
fn words(ui: &mut egui::Ui, kind: Kind, text: &str, theme: &Theme) {
    ui.add(Label::new(RichText::new(text).size(kind.size()).color(kind.ink(theme))).wrap());
}

/// A stretch of the column that can be pointed at: what a click on it
/// copies. While the pointer is on it, the button saying so appears over the
/// words rather than beside them — the column is as wide as the panel lets
/// it be and there is no margin to stand one in — nudged back inside the
/// panel where centering it on the stretch would hang it over an edge.
fn block(
    pass: &mut Pass,
    ui: &mut egui::Ui,
    copies: Copyable,
    width: f32,
    add: impl FnOnce(&mut egui::Ui),
) {
    let response = ui
        .scope_builder(UiBuilder::new().sense(Sense::CLICK), |ui| {
            ui.set_width(width);
            add(ui);
        })
        .response;
    // The pointer is on the words inside rather than on the block, as egui
    // counts hovering, so it is asked where the pointer is instead.
    let pointed = ui.rect_contains_pointer(response.rect);
    offer(pass, ui, copies, &response, response.rect, pointed);
}

/// What makes something laid out in the column a thing that copies: its
/// name for a screen reader, the button over the end of `across` while it is
/// `pointed` at, and the press when it is clicked.
fn offer(
    pass: &mut Pass,
    ui: &egui::Ui,
    copies: Copyable,
    response: &egui::Response,
    across: egui::Rect,
    pointed: bool,
) {
    let control = Control::Facts(copies);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, control.label()));
    if pointed && ui.clip_rect().height() >= CHIP_HEIGHT {
        let chip_width = chip_width(ui, copies);
        let clip = ui.clip_rect();
        let y = (across.center().y - CHIP_HEIGHT / 2.0)
            .round()
            .clamp(clip.min.y, clip.max.y - CHIP_HEIGHT);
        chip(
            pass,
            ui,
            egui::Rect::from_min_size(
                pos2(across.max.x - chip_width, y),
                vec2(chip_width, CHIP_HEIGHT),
            ),
            copies,
            true,
        );
    }
    if response.clicked() {
        pass.press(control);
    }
}

/// Everything the panel has to say, in the order it says it: the file on
/// disk, then the picture in it, then whatever its metadata has to say — the
/// camera, the place, the ground, the words, and last the fields nothing
/// above spoke for.
///
/// A field the file would not give up is left out, a name with a blank under
/// it saying less than nothing; a section left with nothing in it goes too,
/// an empty heading being a question about where the rest of it went. In
/// practice the first two always stand, since a file always has a size and a
/// picture always has a color space.
fn contents(current: &Current) -> Contents {
    let mut sections = vec![
        Section {
            name: "File",
            face: Face::File,
            facts: fields(file_facts(current)),
        },
        Section {
            name: "Image",
            face: Face::Headed {
                mark: icon::IMAGE,
                head: &[RESOLUTION, READ_BY],
            },
            facts: fields(image_facts(current)),
        },
    ];
    let exif = &current.exif;
    let first = sections.len();
    for section in &exif.sections {
        let entries = section
            .entries
            .iter()
            .map(|entry| (entry.name.as_str(), entry.value.clone()));
        // The camera's own section is headed by the camera's name; every
        // other the metadata reads out is a column of fields.
        let face = match section.name {
            "Camera" => Face::Headed {
                mark: icon::CAMERA,
                head: &[exif::CAMERA],
            },
            _ => Face::Fields,
        };
        sections.push(Section {
            name: section.name,
            face,
            facts: fields(entries),
        });
    }
    // The regions are written out here rather than with the rest, since
    // where each is depends on the turn in force.
    let regions = exif.regions(current.pixels(), current.turn);
    let entries = regions
        .iter()
        .map(|entry| (entry.name.as_str(), entry.value.clone()));
    sections.insert(
        first + exif.regions_at(),
        Section::fields(exif::REGIONS, fields(entries)),
    );
    sections.retain(|section| !section.facts.is_empty());
    Contents { sections }
}

/// A section's fields, with the ones that came to nothing dropped.
fn fields<'a>(from: impl IntoIterator<Item = (&'a str, String)>) -> Vec<Fact> {
    from.into_iter()
        .filter(|(_, value)| !value.is_empty())
        .map(|(name, value)| Fact {
            name: name.to_string(),
            value,
        })
        .collect()
}

/// What `copies` puts on the clipboard, which is as much of a table as the
/// thing clicked actually is.
///
/// The whole panel is a table of three columns, a row of it having to say
/// which section it came from to be worth anything beside a row from another.
/// A section is a table of two: every row of it came from the section that
/// was clicked, and repeating that down the column would be saying once per
/// row what the click already said. One field is not a table at all — it is
/// the value, as it is written, with none of the quoting a table needs, since
/// a path or a coordinate is wanted where it is pasted rather than wanted
/// back in a spreadsheet.
pub fn copied(current: &Current, copies: Copyable) -> String {
    let contents = contents(current);
    match copies {
        Copyable::All => joined(
            contents
                .facts()
                .map(|(section, fact)| format!("{},{}", quoted(section), row(fact))),
        ),
        Copyable::Section(index) => match contents.sections.get(index) {
            Some(section) => joined(section.facts.iter().map(row)),
            None => String::new(),
        },
        Copyable::Fact(index) => contents
            .facts()
            .nth(index)
            .map(|(_, fact)| fact.value.clone())
            .unwrap_or_default(),
    }
}

/// One field as the two columns it is: its name, and what it says.
fn row(fact: &Fact) -> String {
    format!("{},{}", quoted(&fact.name), quoted(&fact.value))
}

/// Rows as one body of text. No line after the last: a single row copied on
/// its own should paste as a word rather than as a word and a new line.
fn joined(rows: impl Iterator<Item = String>) -> String {
    rows.collect::<Vec<_>>().join("\n")
}

/// One value of a row, quoted where CSV asks for it: anything holding a
/// comma, a quote or a line break is wrapped in quotes with its own quotes
/// doubled. Not a formality here — a coordinate is two numbers with a comma
/// between them, and so is half of what a raster says about its ground.
///
/// Only where there are columns to keep apart. A field copied on its own goes
/// as it is written: there is nothing for it to run into.
fn quoted(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// What the panel says about the file as a file: what it is called and where
/// it lives, how big it is, and when it was last written. Nothing here is
/// about the picture, nor about how it was read.
fn file_facts(current: &Current) -> Vec<(&'static str, String)> {
    let file = &current.file;
    // The folder as the path was given: a file named on its own on the
    // command line has none to say, and the line goes.
    let folder = std::path::Path::new(&file.path)
        .parent()
        .map(|folder| folder.display().to_string())
        .unwrap_or_default();
    vec![
        (NAME, current.label.clone()),
        (FOLDER, folder),
        (SIZE, file.bytes.map(format_bytes).unwrap_or_default()),
        (MODIFIED, file.modified.map(format_time).unwrap_or_default()),
    ]
}

/// And what it says about the picture: how large it is, what each pixel holds,
/// and what those numbers are meant as light. The bars say some of this too,
/// but they say it in passing and drop it when the window narrows; this is
/// where it is written out and stays written.
fn image_facts(current: &Current) -> Vec<(&'static str, String)> {
    let image = &current.image;
    let mut facts = vec![
        // The decoder that claimed the file, by what its bytes say: the
        // first thing to know about why the rest reads as it does.
        // Upper case, as a format's name is written: `PNG`, `JPEG XL`.
        (
            READ_BY,
            current.file.reader.unwrap_or_default().to_uppercase(),
        ),
        // Which of a raw's two pictures the rest of these are about, where
        // it is the camera's rather than the one developed here.
        (
            "Rendering",
            match current.rendering {
                Rendering::Developed => String::new(),
                Rendering::CameraJpeg => "camera JPEG".to_string(),
            },
        ),
        (
            RESOLUTION,
            format!("{} \u{00d7} {}", image.width, image.height),
        ),
        (
            "Samples",
            format!(
                "{} {}",
                image.samples.component_name(),
                image.channels().label()
            ),
        ),
        // What the device could not keep of what the file holds, and why:
        // nothing for the usual picture, which the device holds as it is.
        (
            "Precision",
            current
                .reduced
                .map(|reduced| format!("half float: {}", reduced.reason()))
                .unwrap_or_default(),
        ),
        ("Color space", image.color.label()),
        // Only where there is an alpha channel to have been multiplied
        // through or not: "opaque" under an image the line above already
        // called rgb is a word about nothing.
        (
            "Alpha",
            match image.alpha {
                AlphaMode::Opaque => String::new(),
                AlphaMode::Straight => "straight".to_string(),
                AlphaMode::Premultiplied => "premultiplied".to_string(),
            },
        ),
        // What the numbers mean once they are light: graded, so that 1.0 is
        // white and the picture opens as it is; scene light, so that it
        // opens metered; or measured, so that it opens windowed to what it
        // holds. The one fact the opening view is decided by, and the one a
        // reader asking why a file opened dark or stretched is looking for.
        ("Referred to", image.referred.label().to_string()),
    ];
    // The gain map, where there is one: whose description of it the file
    // gives, the map itself, how far above the base it can lift the picture,
    // and how much of that this display's room is showing.
    let map = image.gain_map.as_deref();
    facts.extend([
        (
            "Gain map",
            map.map(|map| match map.lift {
                Lift::Iso(_) => "ISO 21496-1".to_string(),
                Lift::Apple { .. } => "Apple".to_string(),
            })
            .unwrap_or_default(),
        ),
        (
            "Gain map size",
            map.map(|map| {
                let kind = match map.channels {
                    1 => "luminance",
                    _ => "RGB",
                };
                format!("{} \u{00d7} {}, {kind}", map.width, map.height)
            })
            .unwrap_or_default(),
        ),
        (
            "HDR headroom",
            map.map(|map| format!("{:.1} stops above SDR white", map.stops()))
                .unwrap_or_default(),
        ),
        (
            "Gain applied",
            map.map(|_| applied(current.lift.as_ref().map_or(0.0, |lift| lift.weight())))
                .unwrap_or_default(),
        ),
    ]);
    facts.extend([
        // The multiplier a Radiance picture says has already been applied
        // to it, which is why it is graded rather than metered; nothing for
        // a file with no place to say it.
        (
            "Exposure applied",
            image
                .exposure
                .map(|exposure| format!("×{exposure}"))
                .unwrap_or_default(),
        ),
        // What else the file holds, for the two kinds that hold more than
        // one picture; nothing for the usual kind.
        ("Holds", holds(current)),
    ]);
    facts
}

/// How much of a gain map's lift is on screen, at `weight`: all of it, a
/// share, or none — which is what a display with no room above white gets.
fn applied(weight: f32) -> String {
    if weight >= 1.0 {
        "all".to_string()
    } else if weight <= 0.0 {
        "none: the display has no room above white".to_string()
    } else {
        format!(
            "{:.0}%, as much as the display has room for",
            weight * 100.0
        )
    }
}

/// The frames or pages a file holds, in a phrase: how many, and for an
/// animation how it loops. Empty for a still.
fn holds(current: &Current) -> String {
    match current.sequence {
        Sequence::Still => String::new(),
        Sequence::Animation { count, loops } => {
            let loops = match loops {
                Loops::Forever => "looping for ever".to_string(),
                Loops::Times(times) if times.get() == 1 => "played once".to_string(),
                Loops::Times(times) => format!("played {times} times"),
            };
            format!("{count} frames, {loops}")
        }
        Sequence::Pages { count, .. } => {
            format!("{count} pages, of which this is page {}", current.page + 1)
        }
    }
}

/// A file's size in the unit that reads best, and the exact count after it:
/// the round number is what a size is compared by, the exact one what it is
/// checked by.
fn format_bytes(bytes: u64) -> String {
    if bytes < 1000 {
        return format!("{bytes} bytes");
    }
    format!("{} ({} bytes)", round_bytes(bytes), grouped(bytes))
}

/// A file's size in the unit that reads best, and nothing more: what the
/// file list says, where there is no room for the exact count.
pub(super) fn round_bytes(bytes: u64) -> String {
    rounded(bytes, "bytes", &["kB", "MB", "GB", "TB", "PB"])
}

/// A count of pixels in the unit that reads best: `2.07 MP`.
pub(super) fn round_pixels(pixels: u64) -> String {
    rounded(pixels, "pixels", &["kP", "MP", "GP"])
}

/// `count` to three significant figures in the largest of `units` — each a
/// thousand times the one before, the first a thousand ones — that keeps it
/// at one or more; under a thousand, the count itself in `ones`.
fn rounded(count: u64, ones: &str, units: &[&str]) -> String {
    if count < 1000 {
        return format!("{count} {ones}");
    }
    let mut value = count as f64 / 1000.0;
    let mut unit = 0;
    // 999.5 rather than 1000: the rounding below would carry it to "1000 MB",
    // which is a size nobody writes.
    while value >= 999.5 && unit + 1 < units.len() {
        value /= 1000.0;
        unit += 1;
    }
    // Three significant figures, which is as much as a size is ever read to.
    let rounded = if value < 10.0 {
        format!("{value:.2}")
    } else if value < 100.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.0}")
    };
    format!("{rounded} {}", units[unit])
}

/// `1258291` as `1,258,291`: a byte count is read by its digits, and eight of
/// them in a row cannot be.
fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// When the file was last written. UTC, and the label says so, since a time
/// read off this panel — or copied out of it — is bound for wherever the
/// reader is; see [`crate::clock`], which also has the local clock a pasted
/// picture is named from.
pub(super) fn format_time(time: SystemTime) -> String {
    let at = clock::utc(time);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
        at.year, at.month, at.day, at.hour, at.minute, at.second
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::histogram;
    use std::time::Duration;

    use crate::image::display::{Display, Startup};
    use crate::image::exif::{Entry, Exif, Section};
    use crate::image::sequence::Sequence;
    use crate::image::{AlphaMode, Channels, ColorSpace, DecodedImage, Samples, Stats};

    /// A window's worth of content area, for the panel to be placed in.
    const CONTENT: Rect = Rect {
        x: 50.0,
        y: 30.0,
        width: 900.0,
        height: 640.0,
    };

    fn current() -> Current {
        let image = DecodedImage::new(
            4,
            5,
            Samples::U8 {
                channels: Channels::Rgb,
                data: vec![0u8; 4 * 5 * 3],
            },
            ColorSpace::SRGB,
            AlphaMode::Opaque,
        );
        let stats = Stats::scan(&image);
        Current {
            display: Display::for_image_with(&image, &stats, Startup::default()),
            image: std::sync::Arc::new(image),
            stats,
            label: "kingfisher.png".into(),
            file: FileFacts {
                path: "/home/reader/pictures/kingfisher.png".into(),
                bytes: Some(1_258_291),
                modified: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_756_632_722)),
                reader: Some("png"),
            },
            exif: photograph(),
            reduced: None,
            sequence: Sequence::Still,
            page: 0,
            lift: None,
            turn: crate::image::orient::Turn::NONE,
            rendering: crate::image::decode::Rendering::Developed,
            camera_jpeg: crate::image::decode::CameraJpeg::Unavailable,
        }
    }

    /// What a photograph's metadata comes to the panel as: the groups it was
    /// read into, the last of them long enough to have to be scrolled.
    fn photograph() -> Exif {
        let entry = |name: &str, value: &str| Entry {
            name: name.to_string(),
            value: value.to_string(),
        };
        Exif {
            sections: vec![
                Section {
                    name: "Camera",
                    entries: vec![
                        entry("Camera", "Apple iPhone 16 Pro"),
                        entry("Exposure", "1/50 s \u{00b7} f/1.78 \u{00b7} ISO 200"),
                    ],
                },
                Section {
                    name: "Capture metadata",
                    entries: (0..24)
                        .map(|index| entry(&format!("Field {index}"), &format!("value {index}")))
                        .collect(),
                },
            ],
            ..Exif::default()
        }
    }

    /// Every row of the column, in reading order: a section's name, then
    /// each of its fields as its name and its value.
    fn written(current: &Current) -> Vec<String> {
        contents(current)
            .sections
            .iter()
            .flat_map(|section| {
                std::iter::once(section.name.to_string()).chain(
                    section
                        .facts
                        .iter()
                        .flat_map(|fact| [fact.name.clone(), fact.value.clone()]),
                )
            })
            .collect()
    }

    /// Every fact the panel exists to show, written out rather than merely
    /// headed: what the file is, then what the picture in it is, then the
    /// metadata's own groups after both.
    #[test]
    fn the_column_says_what_the_file_is() {
        let written = written(&current());
        for expected in [
            "kingfisher.png",
            "/home/reader/pictures",
            "PNG",
            "1.26 MB (1,258,291 bytes)",
            "2025-08-31 09:32:02 UTC",
            "4 \u{00d7} 5",
            "8-bit RGB",
            "sRGB",
            "Apple iPhone 16 Pro",
            "value 23",
        ] {
            assert!(
                written.iter().any(|row| row == expected),
                "{expected:?} is missing from {written:?}"
            );
        }
        // Each fact is named, and the name comes before its value; each
        // section is headed, and the sections come in the order they are read.
        let index = |text: &str| written.iter().position(|row| row == text);
        assert!(index("Folder") < index("/home/reader/pictures"));
        assert!(index("File") < index("Image"));
        // How the file was read is said of the picture it was read into.
        assert!(index("Image") < index("Read by"));
        assert!(index("Image") < index("Resolution"));
        // The picture's size is a fact about the picture, not about the file
        // it arrived in, and is read under the heading that says so.
        assert!(index("Size") < index("Image"));
        assert!(index("Resolution") < index("Camera"));
        assert!(index("Camera") < index("Capture metadata"));
        assert!(index("Capture metadata") < index("Field 0"));
    }

    /// A file whose XMP marks regions out on the picture has them under a
    /// heading of their own after the summaries, each where it is in the
    /// picture as it is turned now.
    #[test]
    fn the_regions_are_where_the_turn_puts_them() {
        use crate::image::metadata_region::{MetadataRegion, Shape, Units};
        let mut current = current();
        current.exif.regions = vec![MetadataRegion {
            label: "Face".into(),
            name: Some("Jane Doe".into()),
            details: Vec::new(),
            shape: Some(Shape::Rectangle {
                center: [0.25, 0.2],
                size: [0.5, 0.4],
            }),
            units: Units::Shares,
        }];
        let written_now = written(&current);
        let index = |text: &str| written_now.iter().position(|row| row == text);
        assert!(index("Camera") < index("Regions"));
        assert!(index("Regions") < index("Capture metadata"));
        assert_eq!(
            written_now[index("Face").expect("the region is written") + 1],
            "Jane Doe \u{00b7} 2 \u{00d7} 2 at 0, 0"
        );
        current.turn = current.turn.clockwise();
        assert!(
            written(&current)
                .iter()
                .any(|row| row == "Jane Doe \u{00b7} 2 \u{00d7} 2 at 3, 0"),
            "{:?}",
            written(&current)
        );
    }

    /// A file named on its own, with no folder in the path it was given,
    /// has no folder line rather than an empty one.
    #[test]
    fn a_file_named_alone_has_no_folder() {
        let mut current = current();
        current.file.path = "kingfisher.png".into();
        let written = written(&current);
        assert!(!written.iter().any(|row| row == "Folder"), "{written:?}");
    }

    /// A file that carries no metadata still has a file and a picture to
    /// describe, and is not given empty headings to explain the rest.
    #[test]
    fn a_file_with_no_metadata_is_all_file_and_no_headings_for_the_rest() {
        let mut current = current();
        current.exif = Exif::default();
        let written = written(&current);
        assert!(written.contains(&"File".to_string()), "{written:?}");
        assert!(written.contains(&"Image".to_string()), "{written:?}");
        assert!(!written.contains(&"Camera".to_string()), "{written:?}");
        assert!(
            !written.contains(&"Capture metadata".to_string()),
            "{written:?}"
        );
    }

    /// A fact the file will not give up is left out altogether: a name with a
    /// blank under it says less than nothing.
    #[test]
    fn a_fact_the_file_will_not_give_up_is_left_out() {
        let mut current = current();
        current.file.bytes = None;
        current.file.modified = None;
        current.file.reader = None;
        let written = written(&current);
        for absent in [
            "Size",
            "Modified",
            "Read by",
            "Alpha",
            "Declared range",
            "Precision",
        ] {
            assert!(!written.iter().any(|row| row == absent), "{written:?}");
        }
        assert!(written.iter().any(|row| row == "kingfisher.png"));
    }

    /// The camera's section is headed by the camera's name; the sections
    /// after it are columns of fields.
    #[test]
    fn the_camera_is_headed_by_its_name() {
        let contents = contents(&current());
        let face = |name: &str| {
            let section = contents
                .sections
                .iter()
                .find(|section| section.name == name);
            section.expect(name).face
        };
        assert!(matches!(
            face("Camera"),
            Face::Headed { head, .. } if head == [exif::CAMERA]
        ));
        assert!(matches!(face("Capture metadata"), Face::Fields));
    }

    /// When a photograph was taken is said as how long ago, where the date
    /// can be read; anything else is written as it is.
    #[test]
    fn the_time_taken_is_said_as_how_long_ago() {
        let fact = |name: &str, value: &str| Fact {
            name: name.to_string(),
            value: value.to_string(),
        };
        let long_ago = short(&fact(exif::TAKEN, "2001-01-01 12:00:00 +00:00"));
        assert!(
            long_ago
                .as_deref()
                .is_some_and(|said| said.ends_with("years ago")),
            "{long_ago:?}"
        );
        assert_eq!(short(&fact(exif::TAKEN, "sometime in spring")), None);
        assert_eq!(short(&fact(exif::LENS, "2001-01-01 12:00:00 +00:00")), None);
    }

    /// A picture with a gain map says what the map is and how much of its
    /// lift is on screen; one without says nothing about a map.
    #[test]
    fn a_gain_map_is_described_under_the_image() {
        use crate::image::gain_map::{GainMap, Lift};
        let mut current = current();
        assert!(!written(&current).iter().any(|row| row == "Gain map"));
        let map = GainMap {
            width: 2,
            height: 3,
            channels: 1,
            data: vec![0; 6],
            lift: Lift::Apple { headroom: 8.0 },
        };
        let mut image = (*current.image).clone();
        image.gain_map = Some(std::sync::Arc::new(map.clone()));
        current.image = std::sync::Arc::new(image);
        let value = |current: &Current, name: &str| {
            let rows = written(current);
            let at = rows.iter().position(|row| row == name).expect(name);
            rows[at + 1].clone()
        };
        assert_eq!(value(&current, "Gain map"), "Apple");
        assert_eq!(value(&current, "Gain map size"), "2 \u{00d7} 3, luminance");
        assert_eq!(value(&current, "HDR headroom"), "3.0 stops above SDR white");
        assert_eq!(
            value(&current, "Gain applied"),
            "none: the display has no room above white"
        );
        current.lift = Some(std::sync::Arc::new(map.table(0.5)));
        assert_eq!(
            value(&current, "Gain applied"),
            "50%, as much as the display has room for"
        );
        current.lift = Some(std::sync::Arc::new(map.table(1.0)));
        assert_eq!(value(&current, "Gain applied"), "all");
    }

    /// Precision lost on the way to the device is said, with why; a picture
    /// that lost none says nothing about it.
    #[test]
    fn precision_is_mentioned_only_where_it_was_lost() {
        let mut current = current();
        assert!(!written(&current).iter().any(|row| row == "Precision"));
        current.reduced = Some(crate::render::Reduced::NoNorm16);
        let rows = written(&current);
        let at = rows
            .iter()
            .position(|row| row == "Precision")
            .expect("a row");
        assert_eq!(
            rows[at + 1],
            "half float: this GPU has no 16-bit integer textures"
        );
    }

    /// A file of frames or pages says how many it holds, and a still says
    /// nothing about it: a line saying "one" would be a line about nothing.
    #[test]
    fn a_file_of_several_pictures_says_how_many() {
        let mut current = current();
        assert!(!written(&current).iter().any(|row| row == "Holds"));

        current.sequence = Sequence::Animation {
            count: 24,
            loops: Loops::Forever,
        };
        let rows = written(&current);
        let holds = rows.iter().position(|row| row == "Holds").expect("a line");
        assert_eq!(rows[holds + 1], "24 frames, looping for ever");

        current.sequence = Sequence::Animation {
            count: 3,
            loops: Loops::Times(std::num::NonZeroU32::new(2).unwrap()),
        };
        assert!(
            written(&current)
                .iter()
                .any(|row| row == "3 frames, played 2 times")
        );

        current.sequence = Sequence::Pages {
            count: 5,
            default: 0,
        };
        current.page = 1;
        assert!(
            written(&current)
                .iter()
                .any(|row| row == "5 pages, of which this is page 2")
        );
    }

    /// What the panel puts on the clipboard is as much of a table as the
    /// thing clicked is: three columns for the lot, two for a section, and
    /// for one field the value on its own.
    #[test]
    fn a_copy_says_as_much_of_the_table_as_was_clicked() {
        let current = current();
        let all: Vec<String> = copied(&current, Copyable::All)
            .lines()
            .map(str::to_string)
            .collect();
        assert_eq!(all[0], "File,Name,kingfisher.png");
        assert_eq!(
            copied(&current, Copyable::Section(0)).lines().nth(1),
            Some("Folder,/home/reader/pictures")
        );
        assert_eq!(copied(&current, Copyable::Fact(0)), "kingfisher.png");

        // A size holds commas, which the two table shapes quote and the
        // field on its own does not: there is nothing there to run into.
        assert_eq!(all[2], "File,Size,\"1.26 MB (1,258,291 bytes)\"");
        assert_eq!(
            copied(&current, Copyable::Fact(2)),
            "1.26 MB (1,258,291 bytes)"
        );

        // And the three agree about which field is which, however much of it
        // each of them says: a section's rows are the panel's with the
        // column naming the section taken off the front, and a field's is
        // the last column of its own row.
        let contents = contents(&current);
        let mut index = 0;
        for (place, section) in contents.sections.iter().enumerate() {
            let (name, facts) = (section.name, &section.facts);
            let rows: Vec<String> = copied(&current, Copyable::Section(place))
                .lines()
                .map(str::to_string)
                .collect();
            assert_eq!(rows.len(), facts.len(), "section {name}");
            for (row, fact) in rows.iter().zip(facts) {
                assert_eq!(all[index], format!("{},{row}", quoted(name)));
                assert_eq!(copied(&current, Copyable::Fact(index)), fact.value);
                index += 1;
            }
        }
        assert_eq!(index, all.len(), "every row belongs to a section");

        // Nothing rather than something wrong for an index off the end,
        // which a click cannot produce but a stale hover could.
        assert_eq!(copied(&current, Copyable::Fact(9_999)), "");
        assert_eq!(copied(&current, Copyable::Section(9_999)), "");
    }

    #[test]
    fn a_value_that_would_break_a_row_is_quoted() {
        assert_eq!(quoted("plain"), "plain");
        assert_eq!(
            quoted("44.68202\u{00b0} S, 169.16196\u{00b0} E"),
            "\"44.68202\u{00b0} S, 169.16196\u{00b0} E\""
        );
        assert_eq!(quoted("a \"quoted\" word"), "\"a \"\"quoted\"\" word\"");
        assert_eq!(quoted("two\nlines"), "\"two\nlines\"");
    }

    /// The panel keeps out of the way of the two widgets it shares the
    /// content area with, and stays off screen where it cannot.
    #[test]
    fn the_panel_gives_way_to_the_histogram_and_to_a_small_window() {
        let tallest = histogram::SIZE;
        let histogram = histogram::panel(CONTENT);
        let with = panel(CONTENT, histogram).expect("room");
        let without = panel(CONTENT, None).expect("room");
        // The same width as the histogram, and the same width whether or not
        // the column it holds is long enough to need a scrollbar.
        assert_eq!(with.width, tallest[0]);
        assert_eq!(with.width, without.width);
        assert_eq!(with.right(), without.right());
        // The histogram has the top of the strip; the column starts below it
        // and the two end together.
        assert_eq!(with.y, without.y + tallest[1] + PADDING);
        assert_eq!(with.bottom(), without.bottom());
        // Top right of the content area, when the histogram is not there.
        assert_eq!(without.right(), CONTENT.right() - PADDING);
        assert_eq!(without.y, CONTENT.y + PADDING);

        // It shows wherever it fits with its margins — a window that holds
        // the panel and not much else still holds the panel — and nowhere
        // narrower or shorter than that.
        let snug = Rect::new(0.0, 0.0, PANEL_WIDTH + 2.0 * PADDING, 640.0);
        assert!(panel(snug, None).is_some(), "{snug:?}");
        assert_eq!(
            panel(
                Rect::new(0.0, 0.0, PANEL_WIDTH + 2.0 * PADDING - 1.0, 640.0),
                None
            ),
            None
        );
        assert_eq!(panel(Rect::new(0.0, 0.0, 900.0, 140.0), None), None);

        // Room for the column, but not once the histogram has had the top of
        // the strip: the tallest panel, the gap under it, and a pixel short
        // of a column below that.
        let squeezed = Rect::new(
            0.0,
            0.0,
            900.0,
            tallest[1] + 3.0 * PADDING + INFO_MIN_HEIGHT - 1.0,
        );
        let plot = histogram::panel(squeezed);
        assert!(plot.is_some(), "the plot fits");
        assert!(panel(squeezed, None).is_some());
        assert_eq!(panel(squeezed, plot), None);

        // And a window too short for the plot takes nothing off the column
        // for it. The toggle is on, but there is no plot on screen for the
        // column to start below — and its own toggle is dead as well.
        let short = Rect::new(0.0, 0.0, 900.0, 200.0);
        assert!(histogram::panel(short).is_none(), "the plot does not fit");
        assert!(panel(short, None).is_some());
    }

    #[test]
    fn a_size_is_given_roundly_and_then_exactly() {
        assert_eq!(format_bytes(0), "0 bytes");
        assert_eq!(format_bytes(999), "999 bytes");
        assert_eq!(format_bytes(1000), "1.00 kB (1,000 bytes)");
        assert_eq!(format_bytes(1_258_291), "1.26 MB (1,258,291 bytes)");
        assert_eq!(format_bytes(45_600_000), "45.6 MB (45,600,000 bytes)");
        assert_eq!(format_bytes(999_500_000), "1.00 GB (999,500,000 bytes)");
    }

    /// The date is said as how long ago it was, and a date after now as now.
    #[test]
    fn a_date_is_said_as_how_long_ago_it_was() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_756_632_722);
        let before = |seconds: u64| ago(now - Duration::from_secs(seconds), now);
        assert_eq!(before(14 * 24 * 60 * 60), "2 weeks ago");
        assert_eq!(before(3 * 60 * 60), "3 hours ago");
        assert_eq!(ago(now + Duration::from_secs(60), now), "now");
        assert_eq!(exact_bytes(1), "1 byte");
        assert_eq!(exact_bytes(1_258_291), "1,258,291 bytes");
    }

    #[test]
    fn digits_are_grouped_in_threes_from_the_right() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_000), "1,000");
        assert_eq!(grouped(1_234_567_890), "1,234,567,890");
    }

    /// The dates a calendar is most often got wrong on: a leap day, the day
    /// after one, the turn of a century that is not a leap year, and the turn
    /// of one that is.
    #[test]
    fn the_calendar_holds_across_leap_years_and_centuries() {
        let at = |seconds: i64| {
            let epoch = SystemTime::UNIX_EPOCH;
            let offset = Duration::from_secs(seconds.unsigned_abs());
            format_time(if seconds >= 0 {
                epoch + offset
            } else {
                epoch - offset
            })
        };
        assert_eq!(at(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(at(951_782_400), "2000-02-29 00:00:00 UTC");
        assert_eq!(at(951_868_800), "2000-03-01 00:00:00 UTC");
        assert_eq!(at(4_107_542_400), "2100-03-01 00:00:00 UTC");
        assert_eq!(at(1_756_632_722), "2025-08-31 09:32:02 UTC");
        // A file older than the epoch still reads as a date, not as 1970.
        assert_eq!(at(-1), "1969-12-31 23:59:59 UTC");
    }
}
