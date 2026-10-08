//! The info panel: what the file is, as a column of words down the side
//! panel.
//!
//! The only part of the interface with more to say than fits, so it is the
//! only part that scrolls: the column lives in a scroll area, under a header
//! that does not scroll, holding the switch between the panel's two tabs and
//! the button that copies the whole of the one on screen. The second tab,
//! every tag `exiftool` reads, is [`super::tags`]'s.
//!
//! It is also the only part of the interface that is read out rather than
//! merely read: a click on a field or on a heading puts it on the clipboard.
//! [`Contents`] is the words, and one index runs through it for the drawing
//! and the copying alike, so what is drawn lit and what lands on the
//! clipboard cannot come to disagree.

use std::sync::Arc;
use std::time::SystemTime;

use egui::{
    Align, Label, Layout, RichText, Sense, StrokeKind, UiBuilder, Vec2, WidgetInfo, WidgetType,
    pos2, vec2,
};

use crate::clock;
use crate::image::AlphaMode;
use crate::image::auxiliary::{Auxiliary, Showing};
use crate::image::decode::Rendering;
use crate::image::depth::{Accuracy, Quantity};
use crate::image::exif::{self, Group, ShownRegion};
use crate::image::gain_map::Lift;
use crate::image::metadata_region::Placed;
use crate::image::orient;
use crate::image::sequence::{Loops, Sequence};
use crate::render::Color;

use super::Rect;
use crate::theme::Theme;

use super::chrome::{ICON_SIDE, Pass, measure};
use super::control::Control;
use super::icon;
use super::pixel;
use super::style::{SCROLLBAR_GUTTER, SCROLLBAR_WIDTH, TOGGLE_RADIUS};
use super::tags::Tab;
use super::tooltip::{Tip, Tooltip};
use super::{Current, PANEL_INSET, PANEL_RADIUS, RULE_WIDTH, TEXT_SIZE, rule};

/// The heading of the regions the metadata marks out on the picture.
const REGIONS: &str = "Regions";
/// The heading of the depth map the picture carries.
const DEPTH_MAP: &str = "Depth map";
/// The heading of the gain map the picture carries.
const GAIN_MAP: &str = "Gain map";
/// What the pill at the end of a heading says while its section is on
/// screen in the picture's place.
const SHOWING: &str = "Showing";

/// Below this the panel would show its header, two facts and a scrollbar, so
/// it stays off instead. The least width is the side panel's own — see
/// [`super::side::WIDTH_MIN`].
pub(super) const INFO_MIN_HEIGHT: f32 = 160.0;

/// The size a field's name is written at, against [`TEXT_SIZE`] for what it
/// says: the values are what is being read, and the names only say which is
/// which.
pub(super) const LABEL_SIZE: f32 = TEXT_SIZE * 0.85;

/// The space above a section's name, with a hairline drawn through the
/// middle of it doing part of the parting.
const SECTION_GAP: f32 = 11.0;
/// The space between two lines of a section written as prose rather than as
/// fields, which are one account of one thing and are set as a paragraph is.
const LINE_GAP: f32 = 1.0;
/// The space between a heading's mark and its name.
const MARK_GAP: f32 = 6.0;
/// The space under the head of a section drawn with a mark: small, the mark
/// already making the head's line taller than the words in it.
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
    /// The file on disk, as a few lines of prose under a heading with a mark:
    /// see [`file_section`].
    File,
    /// A section with a head of its own: some of its fields written on one
    /// line beside a mark, and the rest as a table under them — see
    /// [`headed_section`]. The picture, headed by its size and its format,
    /// the camera by its name, and so on; one with no head of its own, by
    /// its name.
    Headed {
        mark: &'static [icon::Mark],
        head: &'static [&'static str],
        /// A button after the head, with the mark it wears: the map of
        /// where the picture was taken, beside the coordinates.
        button: Option<(&'static [icon::Mark], Control)>,
    },
    /// The regions the metadata marks out on the picture, as a table of who
    /// or what each is and where, each drawn on the picture while the
    /// pointer is on its row — see [`regions_section`].
    Regions,
}

/// One section of the column: its name, how it is drawn, and its fields.
struct Section {
    name: &'static str,
    face: Face,
    facts: Vec<Fact>,
    /// For [`Face::Regions`], each region a fact is the row of, in the same
    /// order; empty for every other face.
    regions: Vec<ShownRegion>,
    /// Whether what the section describes is on screen, which a pill at the
    /// end of its heading says: the picture's, or the gain map's or the
    /// depth map's while it is shown in its place.
    showing: bool,
}

impl Section {
    /// A section drawn as `face`, with no regions.
    fn new(name: &'static str, face: Face, facts: Vec<Fact>) -> Section {
        Section {
            name,
            face,
            facts,
            regions: Vec::new(),
            showing: false,
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

/// The width of the button for `copies`: its mark, whatever label goes
/// before it, and what is kept clear around them.
fn chip_width(ui: &egui::Ui, label: Option<&str>) -> f32 {
    let label = label.map_or(0.0, |label| {
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
fn chip(pass: &Pass, ui: &egui::Ui, rect: egui::Rect, label: Option<&str>, hover: bool) {
    chip_in(pass, ui, rect, label, hover, true);
}

/// [`chip`], drawn dead where it is not `enabled`: its words and mark faded
/// as a dead button's are, and never lit.
fn chip_in(
    pass: &Pass,
    ui: &egui::Ui,
    rect: egui::Rect,
    label: Option<&str>,
    hover: bool,
    enabled: bool,
) {
    let theme = pass.theme;
    let (edge, ink): (egui::Color32, egui::Color32) = if !enabled {
        (
            theme.border.into(),
            theme
                .text_dim
                .with_alpha(super::style::DEAD_BUTTON_INK)
                .into(),
        )
    } else if hover {
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
    let label = label.map(|label| {
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
    copy_button(
        pass,
        ui,
        Copyable::All.label(),
        Control::Facts(Copyable::All),
    );
}

/// A copy button at the head of the panel, saying `label` and pressing
/// `control`: Copy All on the Facts tab, and the Tags tab's two.
fn copy_button(pass: &mut Pass, ui: &mut egui::Ui, label: Option<&str>, control: Control) {
    let width = chip_width(ui, label);
    let (rect, response) = ui.allocate_exact_size(vec2(width, CHIP_HEIGHT), Sense::CLICK);
    chip(pass, ui, rect, label, response.hovered());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, control.label()));
    let response = pass.tooltip(response, Tip::Control(control));
    if response.clicked() {
        pass.press(control);
    }
}

/// The Raw Data tab's copy button: a chip like Copy All, which opens the
/// menu of the forms the tags shown can be copied in.
fn copy_menu(pass: &mut Pass, ui: &mut egui::Ui) {
    let control = Control::TagsCopyMenu;
    let label = Some("Copy");
    let width = chip_width(ui, label);
    let (rect, response) = ui.allocate_exact_size(vec2(width, CHIP_HEIGHT), Sense::CLICK);
    let id = egui::Id::new(("menu", control.label()));
    let open = egui::Popup::is_id_open(ui.ctx(), id);
    // Dead while there is nothing to copy: exiftool not there, or not done.
    let enabled = pass
        .input
        .tags
        .as_ref()
        .is_some_and(|tags| tags.state == super::tags::State::Ready);
    chip_in(pass, ui, rect, label, open || response.hovered(), enabled);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, control.label()));
    let response = pass.tooltip(response, Tip::Control(control));
    if enabled {
        egui::Popup::menu(&response)
            .id(id)
            .align(egui::RectAlign::BOTTOM_END)
            .gap(super::MENU_OFFSET)
            .show(|ui| super::menu::tags_copy_items(pass, ui));
    }
}

/// The switch between the two tabs, at the head of the panel: each tab's
/// name, the one on screen washed and in the text's ink, the other dim.
fn tabs(pass: &mut Pass, ui: &mut egui::Ui) {
    let theme = pass.theme;
    for tab in [Tab::Facts, Tab::Tags] {
        let control = Control::InfoTab(tab);
        let live = pass.panels.info_tab == tab;
        let galley = ui.ctx().fonts_mut(|fonts| {
            fonts.layout_no_wrap(
                tab.label().to_string(),
                egui::FontId::proportional(LABEL_SIZE),
                egui::Color32::PLACEHOLDER,
            )
        });
        let width = (galley.size().x + 2.0 * CHIP_PADDING).round();
        let (rect, response) = ui.allocate_exact_size(vec2(width, CHIP_HEIGHT), Sense::CLICK);
        if live {
            ui.painter()
                .rect_filled(rect, TOGGLE_RADIUS, theme.button_hover);
        }
        let ink: egui::Color32 = if live || response.hovered() {
            theme.text_primary.into()
        } else {
            theme.text_dim.into()
        };
        ui.painter().galley(
            pos2(
                rect.min.x + CHIP_PADDING,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            ink,
        );
        response
            .widget_info(|| WidgetInfo::selected(WidgetType::Button, true, live, control.label()));
        let response = pass.tooltip(response, Tip::Control(control));
        if response.clicked() {
            pass.press(control);
        }
    }
}

/// Draws the panel down the whole of the side panel, `panel`: the header,
/// and under it the column of everything the file has to say, scrolled by
/// egui and read out by a click.
pub(super) fn show(pass: &mut Pass, ui: &mut egui::Ui, current: &Current, panel: Rect) {
    let content = pass.content;
    let area = egui::Rect::from(panel);
    // What the regions are drawn on while the pointer is on their rows: the
    // layer the picture's own marks are painted on, under every panel, this
    // one included. Nothing is drawn over a stand-in for another file.
    let picture = ui
        .painter()
        .with_clip_rect(if pass.input.standin.is_none() {
            content.into()
        } else {
            egui::Rect::NOTHING
        });
    let inside = area.shrink(PANEL_INSET);
    ui.scope_builder(UiBuilder::new().max_rect(inside), |ui| {
        let inside = inside.size();
        ui.set_min_size(inside);
        ui.set_max_size(inside);
        ui.spacing_mut().item_spacing = Vec2::ZERO;

        // The header: the two tabs at the left, and at the right
        // what copies the one on screen.
        ui.horizontal(|ui| {
            tabs(pass, ui);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                match pass.panels.info_tab {
                    Tab::Facts => copy_all(pass, ui),
                    Tab::Tags => copy_menu(pass, ui),
                }
            });
        });
        // A hairline between the header and the column, which
        // says that the column runs on under the header rather
        // than stopping short of it.
        ui.add_space((HEADER_GAP - RULE_WIDTH) / 2.0);
        rule(pass, ui, inside.x - SCROLLBAR_GUTTER);
        ui.add_space((HEADER_GAP - RULE_WIDTH) / 2.0);

        let input = pass.input;
        match (pass.panels.info_tab, &input.tags) {
            (Tab::Tags, Some(tags)) => {
                super::tags::show(pass, ui, tags, inside.x);
            }
            _ => column(pass, ui, current, &picture),
        }
    });
}

/// The column, in a scroll area with the bar down the panel's inner edge and
/// a gutter kept clear for it whether or not there is anything to scroll.
///
/// A different picture is a different column of words about it, and it is
/// read from the top: the scroll area is the file's own, so stepping to
/// another file starts at the top of its column rather than however far
/// down the last one had been read.
fn column(pass: &mut Pass, ui: &mut egui::Ui, current: &Current, picture: &egui::Painter) {
    let contents = cached(ui, current);
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
                    Face::File => file_section(pass, ui, current, section, index, width),
                    Face::Headed { mark, head, button } => {
                        headed_section(pass, ui, place, section, index, width, mark, head, button)
                    }
                    Face::Regions => {
                        regions_section(pass, ui, current, picture, place, section, index, width)
                    }
                }
                index += section.facts.len();
            }
        });
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
    line(
        pass,
        ui,
        width,
        Some(icon::FILE),
        theme.accent,
        None,
        name,
        None,
        0.0,
    );
    if let Some((index, folder)) = field(FOLDER) {
        ui.add_space(HEAD_GAP);
        block(pass, ui, Copyable::Fact(index), width, |ui| {
            let text = RichText::new(&folder.value)
                .size(TEXT_SIZE)
                .color(theme.text_primary);
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
            None,
            0.0,
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
    button: Option<(&[icon::Mark], Control)>,
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
        marked_heading(pass, ui, place, section, width, mark);
    } else {
        // The pill stands at the column's end, level with the mark, in
        // room the head is kept out of.
        let left = ui.cursor().min.x;
        let pill = section.showing.then(|| showing_width(ui));
        let at = line(
            pass,
            ui,
            width,
            Some(mark),
            theme.accent,
            None,
            pieces,
            button,
            pill.map_or(0.0, |pill| pill + CHIP_GAP),
        );
        if let (Some(pill), Some(at)) = (pill, at) {
            let rect = egui::Rect::from_min_size(
                pos2(
                    left + width - pill,
                    (at.center().y - CHIP_HEIGHT / 2.0).round(),
                ),
                vec2(pill, CHIP_HEIGHT),
            );
            paint_showing(ui, rect, theme, pass.grid);
        }
    }
    let rows = (first..)
        .zip(&section.facts)
        .filter(|(_, fact)| !head.contains(&fact.name.as_str()));
    if rows.clone().next().is_some() {
        ui.add_space(HEAD_GAP);
        table(pass, ui, rows, width);
    }
}

/// `section`'s name after `mark`, in the headings' ink, copying the section.
/// Whether the pointer is on it.
fn marked_heading(
    pass: &mut Pass,
    ui: &mut egui::Ui,
    place: usize,
    section: &Section,
    width: f32,
    mark: &[icon::Mark],
) -> bool {
    let theme = pass.theme;
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
                theme.bar_background.into(),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if section.showing {
                    showing_pill(ui, theme, grid);
                    ui.add_space(CHIP_GAP);
                }
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    // A heading is the one thing on the panel picked out in
                    // the accent; everything else wears the bars' own inks.
                    ui.add(
                        Label::new(
                            RichText::new(section.name)
                                .size(TEXT_SIZE)
                                .color(theme.accent),
                        )
                        .wrap(),
                    );
                });
            });
        });
    })
}

/// The pill at the end of a heading whose section is on screen: an eye and
/// the word, in the accent, round at both ends so it reads as a state
/// rather than as a button to press.
fn showing_pill(ui: &mut egui::Ui, theme: &Theme, grid: icon::Grid) {
    let (rect, _) = ui.allocate_exact_size(vec2(showing_width(ui), CHIP_HEIGHT), Sense::HOVER);
    paint_showing(ui, rect, theme, grid);
}

/// The words the pill says, laid out to be drawn.
fn showing_galley(ui: &egui::Ui) -> Arc<egui::Galley> {
    let font = egui::FontId::proportional(LABEL_SIZE);
    ui.ctx().fonts_mut(|fonts| {
        fonts.layout_no_wrap(SHOWING.to_string(), font, egui::Color32::PLACEHOLDER)
    })
}

/// How wide the pill is.
fn showing_width(ui: &egui::Ui) -> f32 {
    (2.0 * CHIP_PADDING + COPY_ICON + CHIP_GAP + showing_galley(ui).size().x).round()
}

/// The pill drawn in `rect`, wherever the heading has put it.
fn paint_showing(ui: &egui::Ui, rect: egui::Rect, theme: &Theme, grid: icon::Grid) {
    let galley = showing_galley(ui);
    let ink: egui::Color32 = theme.accent.into();
    let ground: egui::Color32 = theme.bar_background.into();
    let painter = ui.painter();
    painter.rect_stroke(
        rect,
        CHIP_HEIGHT / 2.0,
        egui::Stroke::new(RULE_WIDTH, ink),
        StrokeKind::Inside,
    );
    let mark = egui::Rect::from_min_size(
        pos2(rect.min.x + CHIP_PADDING, rect.min.y),
        vec2(COPY_ICON, rect.height()),
    );
    icon::paint(
        painter,
        icon::EYE,
        icon::square(grid, mark, COPY_ICON),
        ink,
        ground,
    );
    let at = pos2(
        mark.max.x + CHIP_GAP,
        rect.center().y - galley.size().y / 2.0,
    );
    painter.galley(at, galley, ink);
}

/// The regions the metadata marks out, under their heading: a table of who
/// or what each is and where, its top left corner and its size in the
/// picture as shown — a circle's the square around it, a point's no size.
/// Each row copies itself, and while the pointer is on one its region is
/// drawn on the picture with its subject over it; while it is on the
/// heading, every one of them is. A row copies as its five cells in CSV,
/// and the heading as the table.
///
/// The subject takes what the four numbers leave, which are set right, as
/// numbers in a column are, each column as wide as its widest.
#[allow(clippy::too_many_arguments)]
fn regions_section(
    pass: &mut Pass,
    ui: &mut egui::Ui,
    current: &Current,
    picture: &egui::Painter,
    place: usize,
    section: &Section,
    first: usize,
    width: f32,
) {
    let theme = pass.theme;
    if marked_heading(pass, ui, place, section, width, icon::SQUARE_DASHED) {
        mark_regions(pass, picture, current, &section.regions);
    }
    let cells: Vec<[String; 4]> = section.regions.iter().map(region_cells).collect();
    let font = egui::FontId::proportional(TEXT_SIZE);
    let measured = |text: &str| {
        ui.ctx().fonts_mut(|fonts| {
            fonts
                .layout_no_wrap(text.to_string(), font.clone(), egui::Color32::PLACEHOLDER)
                .size()
                .x
        })
    };
    let columns: [f32; 4] = std::array::from_fn(|column| {
        cells
            .iter()
            .map(|row| measured(&row[column]))
            .chain([measured(REGION_COLUMNS[column + 1])])
            .fold(0.0, f32::max)
            .ceil()
    });
    let subject = (width - columns.iter().sum::<f32>() - 4.0 * COLUMN_GAP).max(0.0);
    // One row of the table, the subject first and the numbers after it,
    // each set right in its column.
    let row = |ui: &mut egui::Ui, first: RichText, rest: [RichText; 4]| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = COLUMN_GAP;
            let response = ui
                .scope(|ui| {
                    ui.set_min_width(subject);
                    ui.set_max_width(subject);
                    ui.add(Label::new(first).wrap())
                })
                .inner;
            for (text, column) in rest.into_iter().zip(columns) {
                ui.allocate_ui_with_layout(vec2(column, 0.0), Layout::top_down(Align::Max), |ui| {
                    ui.set_min_width(column);
                    ui.label(text);
                });
            }
            response
        })
        .inner
    };
    ui.add_space(HEAD_GAP);
    let heading = |text: &str| RichText::new(text).size(LABEL_SIZE).color(theme.text_dim);
    row(
        ui,
        heading(REGION_COLUMNS[0]),
        std::array::from_fn(|column| heading(REGION_COLUMNS[column + 1])),
    );
    let value = |text: &str| {
        RichText::new(text)
            .size(TEXT_SIZE)
            .color(theme.text_primary)
    };
    for ((index, region), cells) in (first..).zip(&section.regions).zip(&cells) {
        ui.add_space(LINE_GAP);
        let pointed = block(pass, ui, Copyable::Fact(index), width, |ui| {
            let response = row(
                ui,
                value(&region.subject),
                std::array::from_fn(|column| value(&cells[column])),
            );
            // What kind of region it is, and what is written about it,
            // where that says more than the subject does.
            if region.about != region.subject {
                said(theme, response, region.about.clone());
            }
        });
        if pointed {
            mark_regions(pass, picture, current, std::slice::from_ref(region));
        }
    }
}

/// A region's four numbers as the table writes them: its top left corner
/// and its size, a point's size left blank and a region with no place all
/// four.
fn region_cells(region: &ShownRegion) -> [String; 4] {
    match region.placed {
        Some(placed @ Placed::Point { .. }) => {
            let [x, y, _, _] = placed.bounds();
            [x.to_string(), y.to_string(), String::new(), String::new()]
        }
        Some(placed) => placed.bounds().map(|value| value.to_string()),
        None => Default::default(),
    }
}

/// A region's row as it is copied: the table's five cells as a line of
/// CSV, the subject quoted where it has to be.
fn region_row(region: &ShownRegion) -> String {
    let [x, y, width, height] = region_cells(region);
    [quoted(&region.subject), x, y, width, height].join(",")
}

/// The heads of the regions table's columns.
const REGION_COLUMNS: [&str; 5] = ["Subject", "X", "Y", "W", "H"];

/// The weight of a region's outline drawn on the picture: the marked
/// region's, being the same kind of thing.
const REGION_OUTLINE: f32 = 1.5;
/// The dashes a region's outline is drawn in, and the gaps between them, in
/// logical pixels: dashed as the heading's mark is, and so told apart from
/// the region marked out by hand, which is drawn solid.
const REGION_DASH: f32 = 6.0;
const REGION_GAP: f32 = 4.0;
/// The radius of the ring a region that is only a point is drawn as.
const POINT_RING: f32 = 6.0;
/// The room around a region's subject, inside the pill it is written on,
/// and the space between the pill and the region.
const PILL_INSET: [f32; 2] = [6.0, 2.0];
const PILL_GAP: f32 = 4.0;

/// Draws `regions` on the picture with `painter`, where each is in the
/// picture as it is placed now, each with its subject written over it.
fn mark_regions(pass: &Pass, painter: &egui::Painter, current: &Current, regions: &[ShownRegion]) {
    let theme = pass.theme;
    let grid = pass.grid;
    let scale = pass.input.scale;
    let placement = pass.view.placement(current.size(), pass.input.viewport);
    let screen = |point: [f64; 2]| {
        let [x, y] = placement.screen_point([point[0] as f32, point[1] as f32]);
        pos2(x / scale, y / scale)
    };
    let accent: egui::Color32 = theme.accent.into();
    let stroke = egui::Stroke::new(grid.line_width(REGION_OUTLINE), accent);
    for region in regions {
        let Some(placed) = region.placed else {
            continue;
        };
        // Where the subject is written: over the region's top left corner,
        // or over a point's ring.
        let corner = match placed {
            Placed::Rectangle { corner, size } => {
                let start = screen(corner);
                let end = screen([corner[0] + size[0], corner[1] + size[1]]);
                let [left, top, right, bottom] = [
                    grid.snap(start.x),
                    grid.snap(start.y),
                    grid.snap(end.x),
                    grid.snap(end.y),
                ];
                let points = [
                    pos2(left, top),
                    pos2(right, top),
                    pos2(right, bottom),
                    pos2(left, bottom),
                    pos2(left, top),
                ];
                painter.extend(egui::Shape::dashed_line(
                    &points,
                    stroke,
                    REGION_DASH,
                    REGION_GAP,
                ));
                pos2(left, top)
            }
            Placed::Circle { center, diameter } => {
                let at = screen(center);
                let radius = (diameter as f32 * placement.zoom / scale / 2.0).max(1.0);
                // Enough links that each is a few pixels long whatever the
                // circle's size, so the dashes follow the curve.
                let steps = ((std::f32::consts::TAU * radius / 3.0).ceil() as usize).max(12);
                let points: Vec<egui::Pos2> = (0..=steps)
                    .map(|step| {
                        let angle = std::f32::consts::TAU * step as f32 / steps as f32;
                        at + radius * vec2(angle.cos(), angle.sin())
                    })
                    .collect();
                painter.extend(egui::Shape::dashed_line(
                    &points,
                    stroke,
                    REGION_DASH,
                    REGION_GAP,
                ));
                at - vec2(radius, radius)
            }
            Placed::Point { center } => {
                let at = screen(center);
                painter.circle_stroke(at, POINT_RING, stroke);
                painter.circle_filled(at, stroke.width, accent);
                at - vec2(POINT_RING, POINT_RING)
            }
        };
        let galley = painter.layout_no_wrap(
            region.subject.clone(),
            egui::FontId::proportional(LABEL_SIZE),
            egui::Color32::PLACEHOLDER,
        );
        let size = galley.size() + 2.0 * Vec2::from(PILL_INSET);
        let pill = egui::Rect::from_min_size(pos2(corner.x, corner.y - PILL_GAP - size.y), size);
        painter.rect_filled(pill, PANEL_RADIUS, theme.menu_background);
        painter.rect_stroke(
            pill,
            PANEL_RADIUS,
            egui::Stroke::new(RULE_WIDTH, theme.border),
            StrokeKind::Inside,
        );
        painter.galley(
            pill.min + Vec2::from(PILL_INSET),
            galley,
            theme.text_primary.into(),
        );
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
/// `between` set between each two, broken to `width` less `reserve` where
/// they do not fit; and after them `button`, wearing its mark, where there
/// is one. Where the mark went, for whatever stands level with it.
///
/// Each piece is pointed at and copied on its own, and says in its tooltip
/// what it stands for where it is written short. The copy button goes where
/// every other one in the column does, at the column's end, level with the
/// piece pointed at.
#[allow(clippy::too_many_arguments)]
fn line(
    pass: &mut Pass,
    ui: &mut egui::Ui,
    width: f32,
    mark: Option<&[icon::Mark]>,
    ink: Color,
    between: Option<&str>,
    pieces: impl IntoIterator<Item = Piece>,
    button: Option<(&[icon::Mark], Control)>,
    reserve: f32,
) -> Option<egui::Rect> {
    let grid = pass.grid;
    let ground = pass.theme.bar_background;
    let left = ui.cursor().min.x;
    let mut marked = None;
    ui.horizontal_wrapped(|ui| {
        ui.set_width(width - reserve);
        let space = measure(ui, " ");
        ui.spacing_mut().item_spacing.x = space;
        let text = |text: &str| RichText::new(text).size(TEXT_SIZE).color(ink);
        if let Some(mark) = mark {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(COPY_ICON), Sense::HOVER);
            marked = Some(rect);
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
        if let Some((mark, control)) = button {
            ui.add_space(MARK_GAP - space);
            let (rect, response) = ui.allocate_exact_size(Vec2::splat(CHIP_HEIGHT), Sense::CLICK);
            let (background, ink) = pass.button_ink(false, &response, true);
            ui.painter().rect_filled(rect, TOGGLE_RADIUS, background);
            icon::paint(
                ui.painter(),
                mark,
                icon::square(grid, rect, COPY_ICON),
                ink,
                background,
            );
            response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, control.label()));
            let response = pass.tooltip(response, Tip::Control(control));
            if response.clicked() {
                pass.press(control);
            }
        }
    });
    marked
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

/// A stretch of the column that can be pointed at: what a click on it
/// copies. While the pointer is on it, the button saying so appears over the
/// words rather than beside them — the column is as wide as the panel lets
/// it be and there is no margin to stand one in — nudged back inside the
/// panel where centering it on the stretch would hang it over an edge.
///
/// Whether the pointer is on it.
fn block(
    pass: &mut Pass,
    ui: &mut egui::Ui,
    copies: Copyable,
    width: f32,
    add: impl FnOnce(&mut egui::Ui),
) -> bool {
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
    pointed
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
        let chip_width = chip_width(ui, copies.label());
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
            copies.label(),
            true,
        );
    }
    if response.clicked() {
        pass.press(control);
    }
}

/// Everything the panel has to say, in the order it says it: the file on
/// disk, then what somebody wrote about the picture in it, then the picture
/// itself and the gain map and depth map it carries, then what else its metadata has to
/// say — the camera, the exposure, the place, the ground — and last the
/// regions it marks out on the picture.
///
/// A field the file would not give up is left out, a name with a blank under
/// it saying less than nothing; a section left with nothing in it goes too,
/// an empty heading being a question about where the rest of it went. In
/// practice the first and the picture's always stand, since a file always
/// has a size and a picture always has a color space.
fn contents(current: &Current) -> Contents {
    let exif = &current.exif;
    // What somebody wrote says what the picture is, so it comes before
    // how the picture is stored; the rest of the metadata, after.
    let (about, captured): (Vec<_>, Vec<_>) = exif
        .sections
        .iter()
        .partition(|section| section.group == Group::About);
    let mut sections = vec![Section::new(
        "File",
        Face::File,
        fields(file_facts(current)),
    )];
    sections.extend(
        about
            .into_iter()
            .map(|section| metadata_section(exif, section)),
    );
    // The pill goes on whichever of the two is on screen.
    sections.push(Section {
        showing: current.showing == Showing::Picture,
        ..Section::new(
            "Image",
            Face::Headed {
                mark: icon::IMAGE,
                head: &[RESOLUTION, READ_BY],
                button: None,
            },
            fields(image_facts(current)),
        )
    });
    sections.push(Section {
        showing: current.showing == Showing::Auxiliary(Auxiliary::GainMap),
        ..Section::new(
            GAIN_MAP,
            Face::Headed {
                mark: icon::SUN,
                head: &[],
                button: None,
            },
            fields(gain_map_facts(current)),
        )
    });
    sections.push(Section {
        showing: current.showing == Showing::Auxiliary(Auxiliary::Depth),
        ..Section::new(
            DEPTH_MAP,
            Face::Headed {
                mark: icon::AXIS_3D,
                head: &[],
                button: None,
            },
            fields(depth_facts(current)),
        )
    });
    sections.extend(
        captured
            .into_iter()
            .map(|section| metadata_section(exif, section)),
    );
    // The regions are written out here rather than with the rest, since
    // where each is depends on the turn in force.
    // Each is copied as its row of the table, which is never empty, so the
    // facts and the regions stay in step.
    let regions = exif.regions(current.pixels(), current.turn);
    let facts = fields(
        regions
            .iter()
            .map(|region| (region.entry.name.as_str(), region_row(region))),
    );
    sections.push(Section {
        regions,
        ..Section::new(REGIONS, Face::Regions, facts)
    });
    sections.retain(|section| !section.facts.is_empty());
    Contents { sections }
}

/// What [`contents`] is made from, as far as the frame can tell it apart:
/// the picture itself, the file it is called, the turn and the image of
/// the file shown, the page, the rendering and the lift's weight. The
/// same key means the same words.
struct Key {
    image: usize,
    path: String,
    label: String,
    turn: orient::Turn,
    showing: Showing,
    page: usize,
    rendering: Rendering,
    lift: Option<u32>,
}

impl Key {
    fn of(current: &Current) -> Self {
        Self {
            image: Arc::as_ptr(&current.image) as *const u8 as usize,
            path: current.file.path.clone(),
            label: current.label.clone(),
            turn: current.turn,
            showing: current.showing,
            page: current.page,
            rendering: current.rendering,
            lift: current.lift.as_ref().map(|lift| lift.weight().to_bits()),
        }
    }

    /// Whether `current` would make this same key, read without making one.
    fn is(&self, current: &Current) -> bool {
        self.image == Arc::as_ptr(&current.image) as *const u8 as usize
            && self.turn == current.turn
            && self.showing == current.showing
            && self.page == current.page
            && self.rendering == current.rendering
            && self.lift == current.lift.as_ref().map(|lift| lift.weight().to_bits())
            && self.path == current.file.path
            && self.label == current.label
    }
}

/// The contents kept between frames, in egui's memory under the column's
/// id: made again only when the [`Key`] has moved.
#[derive(Clone)]
struct Cached {
    key: Arc<Key>,
    contents: Arc<Contents>,
}

/// The panel's contents for `current`, made once per picture rather than
/// once per frame: they are a few dozen formatted facts and the regions
/// placed under the turn, none of which changes while the picture sits
/// there, and the panel is drawn on every frame it is up.
fn cached(ui: &egui::Ui, current: &Current) -> Arc<Contents> {
    let id = egui::Id::new("info contents");
    let held = ui.data(|data| data.get_temp::<Cached>(id));
    if let Some(held) = &held
        && held.key.is(current)
    {
        return Arc::clone(&held.contents);
    }
    let made = Cached {
        key: Arc::new(Key::of(current)),
        contents: Arc::new(contents(current)),
    };
    ui.data_mut(|data| data.insert_temp(id, made.clone()));
    made.contents
}

/// One section of the file's metadata, headed as its group is.
fn metadata_section(exif: &exif::Exif, section: &exif::Section) -> Section {
    let entries = section
        .entries
        .iter()
        .map(|entry| (entry.name.as_str(), entry.value.clone()));
    let (mark, head, button): (_, &'static [&'static str], _) = match section.group {
        // The mark the panel's own button wears.
        Group::About => (icon::INFO, &[exif::TITLE], None),
        Group::Camera => (icon::CAMERA, &[exif::CAMERA], None),
        Group::Exposure => (icon::APERTURE, &[], None),
        // Only where the file gave numbers a map can take.
        Group::Location => (
            icon::MAP_PIN,
            &[exif::LATITUDE, exif::LONGITUDE],
            exif.position.map(|_| (icon::MAP, Control::OpenMap)),
        ),
        Group::Georeference => (icon::MAP, &[], None),
    };
    Section::new(
        section.group.name(),
        Face::Headed { mark, head, button },
        fields(entries),
    )
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
        // The regions are a table already, and copy as one: its heads,
        // then each row as it is copied on its own.
        Copyable::Section(index) => match contents.sections.get(index) {
            Some(section) if matches!(section.face, Face::Regions) => joined(
                std::iter::once(REGION_COLUMNS.join(","))
                    .chain(section.facts.iter().map(|fact| fact.value.clone())),
            ),
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
pub(super) fn quoted(value: &str) -> String {
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
///
/// Always the picture's, whichever of the file's images is on screen: a
/// gain map or a depth map shown in its place is described in a section of
/// its own.
fn image_facts(current: &Current) -> Vec<(&'static str, String)> {
    let face = current.picture_face();
    let image = &face.image;
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
        // What the file asks to be done to its pixels to stand them up,
        // the turn the resolution above is after; and for a TIFF, how they are packed.
        (
            "Orientation",
            current
                .exif
                .orientation
                .and_then(orient::words)
                .unwrap_or_default()
                .to_string(),
        ),
        (
            "Compression",
            current.exif.compression.clone().unwrap_or_default(),
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
        ("Precision", precision(face)),
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

/// What the panel says about the gain map the picture carries, whichever of
/// the two is on screen, in a photographer's terms: its resolution and
/// samples, the precision it lost on its way to the device while it is
/// shown, whose description of it the file gives, how far above SDR white
/// the picture can go, how far the map raises the picture where it raises
/// it most, and how much of that this display's room is showing. Nothing
/// where the picture carries no map.
fn gain_map_facts(current: &Current) -> Vec<(&'static str, String)> {
    let face = current.picture_face();
    let Some(map) = face.image.gain_map.as_deref() else {
        return Vec::new();
    };
    let stops = map.stops();
    vec![
        (RESOLUTION, format!("{} \u{00d7} {}", map.width, map.height)),
        (
            "Samples",
            match map.channels {
                1 => "8-bit luminance",
                _ => "8-bit RGB",
            }
            .to_string(),
        ),
        // Only while the map is on screen, as the depth map's.
        (
            "Precision",
            if current.showing == Showing::Auxiliary(Auxiliary::GainMap) {
                precision(&current.shown)
            } else {
                String::new()
            },
        ),
        (
            "Described by",
            match map.lift {
                Lift::Iso(_) => "ISO 21496-1",
                Lift::Apple { .. } => "Apple",
            }
            .to_string(),
        ),
        ("HDR headroom", format!("{stops:.1} stops above SDR white")),
        ("Lift", map.lift_range().map(lift).unwrap_or_default()),
        (
            "Applied",
            applied(face.lift.as_ref().map_or(0.0, |lift| lift.weight()), stops),
        ),
    ]
}

/// The least and the most a gain map raises the picture by, in stops: how
/// far its brightest highlights go up, from where it leaves the picture as
/// it is. A map that darkens somewhere says so with a sign.
fn lift([low, high]: [f32; 2]) -> String {
    // Within a twentieth of a stop of nothing is nothing, at the tenths the
    // range is written in; adding zero makes a negative zero positive.
    let low = if low.abs() < 0.05 { 0.0 } else { low + 0.0 };
    if low >= 0.0 {
        format!("up to {high:.1} stops")
    } else {
        format!("{low:+.1} to {high:+.1} stops").replace('-', "\u{2212}")
    }
}

/// What the panel says about the depth map the picture carries, whichever
/// of the two is on screen: its resolution and samples, the precision it
/// lost on its way to the device while it is shown, whose words say what its
/// codes stand for, whether they stand for the distance or its inverse, the
/// distances the codes run between, and how far those can be believed.
/// Nothing where the picture carries no map.
///
/// A map whose file does not say what its codes stand for is still a map,
/// its encoding unknown: the readout shows its codes.
fn depth_facts(current: &Current) -> Vec<(&'static str, String)> {
    let (picture, _) = current.picture();
    let Some(map) = picture.depth.as_deref() else {
        return Vec::new();
    };
    let mut facts = vec![
        (RESOLUTION, format!("{} \u{00d7} {}", map.width, map.height)),
        (
            "Samples",
            format!(
                "{} {}",
                map.samples.component_name(),
                map.samples.channels().label()
            ),
        ),
        // Only while the map is on screen: until then it has not been on
        // its way to the device at all.
        (
            "Precision",
            if current.showing == Showing::Auxiliary(Auxiliary::Depth) {
                precision(&current.shown)
            } else {
                String::new()
            },
        ),
    ];
    let Some(scale) = map.scale else {
        facts.push(("Encoding", "unknown".to_string()));
        return facts;
    };
    let [near, far] = scale.range(&map.samples);
    let near = near.map(pixel::written).unwrap_or_default();
    let far = far.map_or_else(|| "infinity".to_string(), pixel::written);
    facts.extend([
        ("Described by", scale.vendor.name().to_string()),
        (
            "Encoding",
            match scale.quantity {
                Quantity::Distance => "distance",
                Quantity::Inverse => "inverse distance",
            }
            .to_string(),
        ),
        ("Range", format!("{near} to {far}")),
        (
            "Accuracy",
            match scale.accuracy {
                Accuracy::Absolute => "absolute",
                Accuracy::Relative => "relative",
            }
            .to_string(),
        ),
    ]);
    facts
}

/// What an image on screen could not keep of what it holds on its way to
/// the device, and why: nothing for the usual image, which the device
/// holds as it is.
fn precision(face: &crate::ui::Face) -> String {
    face.reduced
        .map(|reduced| format!("half float: {}", reduced.reason()))
        .unwrap_or_default()
}

/// How much of a gain map's lift of `stops` is on screen, at `weight`, in
/// stops: all of it, a share, or none — which is what a display with no
/// room above white gets.
fn applied(weight: f32, stops: f32) -> String {
    if weight >= 1.0 {
        format!("all {stops:.1} stops")
    } else if weight <= 0.0 {
        "none: the display has no room above white".to_string()
    } else {
        format!(
            "{:.1} of {stops:.1} stops, as much as the display has room for",
            weight * stops
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
mod tests;
