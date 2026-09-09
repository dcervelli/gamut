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
use crate::render::Color;

use super::Rect;
use crate::theme::Theme;

use super::chrome::{ICON_SIDE, Pass, measure};
use super::control::Control;
use super::histogram::{self, HISTOGRAM_SIZE};
use super::icon;
use super::style::TOGGLE_RADIUS;
use super::tooltip::Tip;
use super::{Current, PADDING, PANEL_INSET, PANEL_RADIUS, PANEL_WIDTH, TEXT_SIZE};

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
/// a hairline drawn through the middle of it doing the parting that the space
/// used to have to do alone.
const SECTION_GAP: f32 = 11.0;
/// The space between a field's name and its value. Less than nothing: a line
/// box carries its own leading above the glyphs, so the two lines are pulled
/// a pixel into one another's boxes without their ink coming any closer, and
/// a name and what it names read as one thing rather than as two.
const LABEL_GAP: f32 = -1.0;

/// The hairline drawn across the column above every section but the first,
/// and under the header. The same width as the hairline along a panel's edge,
/// being the same kind of thing.
const RULE_WIDTH: f32 = 1.0;

/// The words at the top of the panel. An instruction rather than a fact about
/// the file, so it is written small and dim: what it says is worth knowing
/// once and not worth re-reading every time the panel is opened.
const HINT: &str = "Click to copy section or item.";

/// The space under the header, with the hairline that parts it from the
/// column through the middle of it.
const HEADER_GAP: f32 = 11.0;

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

/// The scrollbar down the panel's inner edge, and the room kept clear for it
/// whether or not there is anything to scroll — text that reflowed the moment
/// the bar appeared would be text that reflowed as it was being read.
const SCROLLBAR_WIDTH: f32 = 3.0;
const SCROLLBAR_GUTTER: f32 = SCROLLBAR_WIDTH + 7.0;

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
    sections: Vec<(&'static str, Vec<Fact>)>,
}

impl Contents {
    /// Every field in the column, in reading order, with the section it
    /// stands under. This is the order [`Copyable::Fact`] counts in.
    fn facts(&self) -> impl Iterator<Item = (&'static str, &Fact)> {
        self.sections
            .iter()
            .flat_map(|(name, facts)| facts.iter().map(move |fact| (*name, fact)))
    }
}

/// Where the panel goes: down the right of `content`, starting under the
/// histogram when that is showing as well and at the top of the content when
/// it is not — the order the two toggles are stacked in.
///
/// `None` when the window has no room for a column worth reading, which is
/// also what keeps the panel off screen rather than shrunk to nothing.
///
/// It is one width or it is not there: the column is read at the same measure
/// whatever the window is doing. In a window narrow enough for the panel and
/// the minimap to want the same strip the panel takes it, being drawn after —
/// it is on screen because it was asked for, and the minimap is a guide to a
/// picture the panel is already covering.
pub fn panel(content: Rect, show_histogram: bool) -> Option<Rect> {
    let width = PANEL_WIDTH;
    if width + 2.0 * PADDING > content.width {
        return None;
    }
    // The histogram takes the top of the column's strip; the panel starts
    // below it rather than being drawn over it. Only where the histogram is
    // on screen, which asks the window as well as the toggle: a window with
    // no room for the plot is not one the column has to start below.
    let taken = if show_histogram && histogram::panel(content).is_some() {
        HISTOGRAM_SIZE[1] + PADDING
    } else {
        0.0
    };
    let height = content.height - 2.0 * PADDING - taken;
    if height < INFO_MIN_HEIGHT {
        return None;
    }
    Some(Rect::new(
        (content.right() - width - PADDING).round(),
        (content.y + PADDING + taken).round(),
        width,
        height,
    ))
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
        icon::square(icon::Grid::new(ui.pixels_per_point()), mark, COPY_ICON),
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
    let response = pass.tooltip(response, Tip::Control(control), true);
    if response.clicked() {
        pass.press(control);
    }
}

/// A hairline across the column, on the device's grid.
fn rule(pass: &Pass, ui: &mut egui::Ui, width: f32) {
    let grid = icon::Grid::new(ui.pixels_per_point());
    let edge = grid.line_width(RULE_WIDTH);
    let (rect, _) = ui.allocate_exact_size(vec2(width, edge), Sense::HOVER);
    ui.painter().rect_filled(
        egui::Rect::from_min_size(rect.min, vec2(width, edge)),
        0.0,
        pass.theme.border,
    );
}

/// Draws the panel: the header, and under it the column of everything the
/// file has to say, scrolled by egui and read out by a click.
pub(super) fn show(pass: &mut Pass, ui: &mut egui::Ui, current: &Current, content: Rect) {
    let Some(panel) = panel(content, pass.panels.show_histogram) else {
        return;
    };
    let theme = pass.theme;
    let area = egui::Rect::from_min_size(pos2(panel.x, panel.y), vec2(panel.width, panel.height));
    egui::Area::new(egui::Id::new("info"))
        .order(egui::Order::Middle)
        .fixed_pos(area.min)
        .interactable(true)
        .show(ui.ctx(), |ui| {
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
            for (section, (name, facts)) in contents.sections.iter().enumerate() {
                // The first heading opens the column and has nothing above
                // it to be parted from; every one after it is a section
                // starting, parted from the last by a hairline through the
                // middle of the space above it, so the gap reads as
                // belonging to neither section more than the other.
                if section > 0 {
                    ui.add_space((SECTION_GAP - RULE_WIDTH) / 2.0);
                    rule(pass, ui, width);
                    ui.add_space((SECTION_GAP - RULE_WIDTH) / 2.0);
                }
                block(pass, ui, Copyable::Section(section), width, |ui| {
                    words(ui, Kind::Heading, name, pass.theme);
                });
                for fact in facts {
                    ui.add_space(FIELD_GAP);
                    // A field's name and the value under it are one block,
                    // being one thing to point at and one row to copy.
                    block(pass, ui, Copyable::Fact(index), width, |ui| {
                        words(ui, Kind::Label, &fact.name, pass.theme);
                        ui.add_space(LABEL_GAP);
                        words(ui, Kind::Value, &fact.value, pass.theme);
                    });
                    index += 1;
                }
            }
        });
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
    let control = Control::Facts(copies);
    let response = ui
        .scope_builder(UiBuilder::new().sense(Sense::CLICK), |ui| {
            ui.set_width(width);
            add(ui);
        })
        .response;
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, control.label()));
    let rect = response.rect;
    if ui.rect_contains_pointer(rect) && ui.clip_rect().height() >= CHIP_HEIGHT {
        let chip_width = chip_width(ui, copies);
        let clip = ui.clip_rect();
        let y = (rect.center().y - CHIP_HEIGHT / 2.0)
            .round()
            .clamp(clip.min.y, clip.max.y - CHIP_HEIGHT);
        chip(
            pass,
            ui,
            egui::Rect::from_min_size(
                pos2(rect.max.x - chip_width, y),
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
        ("File", fields(file_facts(current))),
        ("Image", fields(image_facts(current))),
    ];
    for section in &current.exif.sections {
        let entries = section
            .entries
            .iter()
            .map(|entry| (entry.name.as_str(), entry.value.clone()));
        sections.push((section.name, fields(entries)));
    }
    sections.retain(|(_, facts)| !facts.is_empty());
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
            Some((_, facts)) => joined(facts.iter().map(row)),
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
/// it lives, which decoder turned out to own it, then how big it is and when
/// it was last written. Nothing here is about the picture.
fn file_facts(current: &Current) -> Vec<(&'static str, String)> {
    let file = &current.file;
    vec![
        ("Name", current.label.clone()),
        ("Path", file.path.clone()),
        ("Read by", file.reader.unwrap_or_default().to_string()),
        ("Size", file.bytes.map(format_bytes).unwrap_or_default()),
        (
            "Modified",
            file.modified.map(format_time).unwrap_or_default(),
        ),
    ]
}

/// And what it says about the picture: how large it is, what each pixel holds,
/// and what those numbers are meant as light. The bars say some of this too,
/// but they say it in passing and drop it when the window narrows; this is
/// where it is written out and stays written.
fn image_facts(current: &Current) -> Vec<(&'static str, String)> {
    let image = &current.image;
    vec![
        (
            "Resolution",
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
        // What we asked the GPU for is not always what it had: a 16-bit image
        // on a device without the format lands somewhere wider or narrower,
        // and the difference belongs beside what the file holds.
        ("Stored as", current.stored.clone().unwrap_or_default()),
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
        // white and the picture opens as it is, or measured, so that it opens
        // windowed to what it holds. The one fact the opening view is decided
        // by, and the one a reader asking why a file opened dark or stretched
        // is looking for.
        ("Referred to", image.referred.label().to_string()),
    ]
}

/// A file's size in the unit that reads best, and the exact count after it:
/// the round number is what a size is compared by, the exact one what it is
/// checked by.
fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["kB", "MB", "GB", "TB", "PB"];
    if bytes < 1000 {
        return format!("{bytes} bytes");
    }
    let mut value = bytes as f64 / 1000.0;
    let mut unit = 0;
    // 999.5 rather than 1000: the rounding below would carry it to "1000 MB",
    // which is a size nobody writes.
    while value >= 999.5 && unit + 1 < UNITS.len() {
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
    format!("{rounded} {} ({} bytes)", UNITS[unit], grouped(bytes))
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
fn format_time(time: SystemTime) -> String {
    let at = clock::utc(time);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
        at.year, at.month, at.day, at.hour, at.minute, at.second
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use crate::image::display::{Display, Headroom, Startup};
    use crate::image::exif::{Entry, Exif, Section};
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
            display: Display::for_image_with(&image, &stats, Startup::default(), Headroom::None),
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
            stored: None,
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
        }
    }

    /// Every row of the column, in reading order: a section's name, then
    /// each of its fields as its name and its value.
    fn written(current: &Current) -> Vec<String> {
        contents(current)
            .sections
            .iter()
            .flat_map(|(name, facts)| {
                std::iter::once(name.to_string()).chain(
                    facts
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
            "/home/reader/pictures/kingfisher.png",
            "png",
            "1.26 MB (1,258,291 bytes)",
            "2025-08-31 09:32:02 UTC",
            "4 \u{00d7} 5",
            "8-bit rgb",
            "BT.709/sRGB",
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
        assert!(index("Path") < index("/home/reader/pictures/kingfisher.png"));
        assert!(index("File") < index("Image"));
        assert!(index("Image") < index("Resolution"));
        // The picture's size is a fact about the picture, not about the file
        // it arrived in, and is read under the heading that says so.
        assert!(index("Size") < index("Image"));
        assert!(index("Resolution") < index("Camera"));
        assert!(index("Camera") < index("Capture metadata"));
        assert!(index("Capture metadata") < index("Field 0"));
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
            "Stored as",
        ] {
            assert!(!written.iter().any(|row| row == absent), "{written:?}");
        }
        assert!(written.iter().any(|row| row == "kingfisher.png"));
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
            copied(&current, Copyable::Section(0)).lines().next(),
            Some("Name,kingfisher.png")
        );
        assert_eq!(copied(&current, Copyable::Fact(0)), "kingfisher.png");

        // A size holds commas, which the two table shapes quote and the
        // field on its own does not: there is nothing there to run into.
        assert_eq!(all[3], "File,Size,\"1.26 MB (1,258,291 bytes)\"");
        assert_eq!(
            copied(&current, Copyable::Fact(3)),
            "1.26 MB (1,258,291 bytes)"
        );

        // And the three agree about which field is which, however much of it
        // each of them says: a section's rows are the panel's with the
        // column naming the section taken off the front, and a field's is
        // the last column of its own row.
        let contents = contents(&current);
        let mut index = 0;
        for (section, (name, facts)) in contents.sections.iter().enumerate() {
            let rows: Vec<String> = copied(&current, Copyable::Section(section))
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
        let with = panel(CONTENT, true).expect("room");
        let without = panel(CONTENT, false).expect("room");
        // The same width as the histogram, and the same width whether or not
        // the column it holds is long enough to need a scrollbar.
        assert_eq!(with.width, HISTOGRAM_SIZE[0]);
        assert_eq!(with.width, without.width);
        assert_eq!(with.right(), without.right());
        // The histogram has the top of the strip; the column starts below it
        // and the two end together.
        assert_eq!(with.y, without.y + HISTOGRAM_SIZE[1] + PADDING);
        assert_eq!(with.bottom(), without.bottom());
        // Top right of the content area, when the histogram is not there.
        assert_eq!(without.right(), CONTENT.right() - PADDING);
        assert_eq!(without.y, CONTENT.y + PADDING);

        // It shows wherever it fits with its margins — a window that holds
        // the panel and not much else still holds the panel — and nowhere
        // narrower or shorter than that.
        let snug = Rect::new(0.0, 0.0, PANEL_WIDTH + 2.0 * PADDING, 640.0);
        assert!(panel(snug, false).is_some(), "{snug:?}");
        assert_eq!(
            panel(
                Rect::new(0.0, 0.0, PANEL_WIDTH + 2.0 * PADDING - 1.0, 640.0),
                false
            ),
            None
        );
        assert_eq!(panel(Rect::new(0.0, 0.0, 900.0, 140.0), false), None);

        // Room for the column, but not once the histogram has had the top of
        // the strip: tall enough for the plot and a hundred pixels more.
        let squeezed = Rect::new(0.0, 0.0, 900.0, HISTOGRAM_SIZE[1] + 2.0 * PADDING + 100.0);
        assert!(histogram::panel(squeezed).is_some(), "the plot fits");
        assert!(panel(squeezed, false).is_some());
        assert_eq!(panel(squeezed, true), None);

        // And a window too short for the plot takes nothing off the column
        // for it. The toggle is on, but there is no plot on screen for the
        // column to start below — and its own toggle is dead as well.
        let short = Rect::new(0.0, 0.0, 900.0, 200.0);
        assert!(histogram::panel(short).is_none(), "the plot does not fit");
        assert_eq!(panel(short, true), panel(short, false));
        assert!(panel(short, true).is_some());
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
