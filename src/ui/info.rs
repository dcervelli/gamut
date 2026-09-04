//! The info panel: what the file is, as a column of words down the right of
//! the content area.
//!
//! The only part of the interface with more to say than fits, so it is the
//! only part that scrolls. The column is laid out in full every frame, from
//! the top of the content down, and what falls outside the panel is cut off
//! by the text layer rather than by anything here — which is what lets a line
//! be drawn half, sliding under the panel's edge as the wheel turns. Above it
//! is a header that does not scroll, holding the one instruction the panel
//! needs and the button that copies the whole of it.
//!
//! It is also the only part of the interface that is read out rather than
//! merely read: a click on a field or on a heading puts it on the clipboard.
//! So there are three lists here rather than one, each derived from the last
//! — [`Contents`], which is the words; [`Column`], which is where they go;
//! and [`blocks`], which is what can be pointed at — and one index runs
//! through all three, so what is under the pointer, what is drawn lit, and
//! what is copied cannot come to disagree.

use std::time::SystemTime;

use crate::clock;
use crate::image::AlphaMode;
use crate::render::{Color, Rect, TextMeasure, UiFrame};
use crate::theme::Theme;

use super::buttons::{ICON_SIDE, outline, text_top};
use super::histogram::HISTOGRAM_SIZE;
use super::icon;
use super::menu::CELL_RADIUS;
use super::{Current, PADDING, PANEL_INSET, PANEL_RADIUS, PANEL_WIDTH, Panels, TEXT_SIZE};

/// Below this the panel would show its header, two facts and a scrollbar, so
/// it stays off instead. There is no matching minimum for the width: the
/// panel is [`PANEL_WIDTH`] wide or it is not on screen.
const INFO_MIN_HEIGHT: f32 = 160.0;

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
/// as well as in what [`icon::fit`] sizes a square out of. A side toggle's,
/// so that the two marks are drawn at one size wherever they are seen
/// together.
const COPY_ICON: f32 = ICON_SIDE;

/// The scrollbar down the panel's inner edge, and the room kept clear for it
/// whether or not there is anything to scroll — text that reflowed the moment
/// the bar appeared would be text that reflowed as it was being read.
const SCROLLBAR_WIDTH: f32 = 3.0;
const SCROLLBAR_GUTTER: f32 = SCROLLBAR_WIDTH + 7.0;
/// The least of the track the thumb may take, so that a very long column
/// still has something to grab hold of with the eye.
const THUMB_MIN: f32 = 24.0;

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
    /// The size it is written at, and the space above it. A section is set
    /// off from the one before it, a name sits close to the value under it.
    fn style(self) -> (f32, f32) {
        match self {
            Kind::Heading => (TEXT_SIZE, SECTION_GAP),
            Kind::Label => (LABEL_SIZE, FIELD_GAP),
            Kind::Value => (TEXT_SIZE, LABEL_GAP),
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

/// One run of text in the column, at its place down it, and what clicking it
/// copies. `y` is measured from the top of the column, not of the window: the
/// panel subtracts the scroll from it, and the same list serves the drawing,
/// the hit testing, and the question of how far it may be scrolled.
struct Row {
    text: String,
    kind: Kind,
    y: f32,
    height: f32,
    copies: Copyable,
}

/// The column as it is built: rows so far, how tall they come to, and the
/// width they are broken at.
struct Column {
    rows: Vec<Row>,
    height: f32,
    width: f32,
}

impl Column {
    fn add(&mut self, text: &mut dyn TextMeasure, kind: Kind, body: String, copies: Copyable) {
        let (size, gap) = kind.style();
        let height = text.measure_wrapped(&body, size, self.width)[1];
        let y = if self.rows.is_empty() {
            0.0
        } else {
            self.height + gap
        };
        self.height = y + height;
        self.rows.push(Row {
            text: body,
            kind,
            y,
            height,
            copies,
        });
    }
}

/// The stretches of the column that can be pointed at, in the order they run
/// down it: what each copies, and where it starts and ends measured from the
/// top of the column.
///
/// A field's name and the value under it are one stretch, being one thing to
/// point at and one row to copy; a heading is one on its own, and does not
/// take in the fields standing under it — a click on a field would otherwise
/// have two answers.
fn blocks(column: &Column) -> Vec<(Copyable, f32, f32)> {
    let mut blocks: Vec<(Copyable, f32, f32)> = Vec::new();
    for row in &column.rows {
        match blocks.last_mut() {
            Some((copies, _, bottom)) if *copies == row.copies => *bottom = row.y + row.height,
            _ => blocks.push((row.copies, row.y, row.y + row.height)),
        }
    }
    blocks
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
    // below it rather than being drawn over it.
    let taken = if show_histogram {
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

/// The panel's header: the hint, and the button that copies the whole column
/// right-justified beside it.
///
/// The button is placed first and the hint takes what is left, wrapping into
/// it. The button has a size it must be to be pressed and the hint is words,
/// which set on two lines as readily as on one — so where the two will not
/// share a line it is the words that give.
struct Header {
    /// The strip both of them take at the top of the panel.
    strip: Rect,
    hint: Rect,
    button: Rect,
}

fn header(text: &mut dyn TextMeasure, view: Rect) -> Header {
    let width = chip_width(text, Copyable::All).min(view.width);
    let left = view.right() - width;
    let hint_width = (left - view.x - CHIP_GAP).max(1.0);
    let hint_height = text.measure_wrapped(HINT, LABEL_SIZE, hint_width)[1];
    let height = hint_height.max(CHIP_HEIGHT);
    let centred = |own: f32| (view.y + (height - own) / 2.0).round();
    Header {
        strip: Rect::new(view.x, view.y, view.width, height),
        hint: Rect::new(view.x, centred(hint_height), hint_width, hint_height),
        button: Rect::new(left, centred(CHIP_HEIGHT), width, CHIP_HEIGHT),
    }
}

/// The panel's two parts: the header, which stays where it is, and the strip
/// the column scrolls in under it.
///
/// The header does not scroll — it is the panel's own furniture, and a button
/// that wandered off the top of the panel could not be pressed twice running
/// — so everything about the column is measured, clipped and hit-tested
/// against the second of these rather than against the whole panel.
fn parts(text: &mut dyn TextMeasure, panel: Rect) -> (Header, Rect) {
    let view = panel.inset(PANEL_INSET, PANEL_INSET);
    let header = header(text, view);
    let top = header.strip.bottom() + HEADER_GAP;
    let column = Rect::new(view.x, top, view.width, (view.bottom() - top).max(0.0));
    (header, column)
}

/// How far the column may be scrolled before its last line is at the bottom
/// of the panel: zero when it all fits, or when there is no panel.
///
/// The application clamps the scroll against this as the wheel turns, so that
/// a spin past the end does not leave the panel having to be wound back
/// through nothing.
pub fn max_scroll(text: &mut dyn TextMeasure, current: &Current, panel: Rect) -> f32 {
    let (_, view) = parts(text, panel);
    (column(text, &contents(current), view.width).height - view.height).max(0.0)
}

/// What clicking at `point` would copy, `point` being in the logical pixels
/// the interface is laid out in and the panel being scrolled by `scroll`.
///
/// `None` where the pointer is on the panel but on none of it that copies:
/// the hint, the scrollbar's gutter, or the space under the last line.
pub fn copyable_at(
    text: &mut dyn TextMeasure,
    current: &Current,
    panel: Rect,
    scroll: f32,
    point: [f32; 2],
) -> Option<Copyable> {
    let (header, view) = parts(text, panel);
    if header.button.contains(point) {
        return Some(Copyable::All);
    }
    // The gutter is the scrollbar's, and a press there is a press on the
    // scrollbar however far down the column it happens to fall.
    let clip = Rect::new(view.x, view.y, view.width - SCROLLBAR_GUTTER, view.height);
    if !clip.contains(point) {
        return None;
    }
    let column = column(text, &contents(current), view.width);
    let scroll = scroll.clamp(0.0, (column.height - view.height).max(0.0));
    let at = point[1] - view.y + scroll;
    blocks(&column)
        .into_iter()
        .find(|(_, top, bottom)| at >= *top && at < *bottom)
        .map(|(copies, ..)| copies)
}

/// How far the column moves for each logical pixel a drag travels, the drag
/// being of the scrollbar's thumb rather than of the words: a thumb that
/// crosses its track in a short distance carries a long column past in that
/// same distance, so the words run faster than the pointer.
///
/// One where there is nothing to scroll and so no thumb to measure: the
/// scroll is clamped against the overflow in any case, so a column that all
/// fits stays where it is whatever the drag is multiplied by.
pub fn scroll_per_drag(text: &mut dyn TextMeasure, current: &Current, panel: Rect) -> f32 {
    let (_, view) = parts(text, panel);
    let column = column(text, &contents(current), view.width);
    let overflow = (column.height - view.height).max(0.0);
    let travel = thumb(view.height, column.height).1;
    if overflow <= 0.0 || travel <= 0.0 {
        return 1.0;
    }
    overflow / travel
}

/// The hairline parting a section from the one before it, given where that
/// section's heading landed: across the column, in the middle of the space
/// above the heading, so that the gap reads as belonging to neither section
/// more than the other.
///
/// `None` for a line that would fall outside the panel. A rule is one pixel
/// and cannot be drawn half, so unlike the words — which slide under the
/// panel's edge — it is either on screen or it is not.
fn rule(y: f32, clip: Rect) -> Option<Rect> {
    let rule = Rect::new(
        clip.x,
        (y - SECTION_GAP / 2.0).round(),
        clip.width,
        RULE_WIDTH,
    );
    (rule.y >= clip.y && rule.bottom() <= clip.bottom()).then_some(rule)
}

/// The scrollbar's thumb: how tall it is, and how far down the track it
/// travels between the top of the column and the end of it. Long columns
/// stop shortening it at [`THUMB_MIN`], which is why the two are not simply
/// proportional to what the panel shows.
fn thumb(view_height: f32, column_height: f32) -> (f32, f32) {
    let height = (view_height * (view_height / column_height)).max(THUMB_MIN);
    (height, (view_height - height).max(0.0))
}

/// How wide the button for `copies` comes out: its mark, whatever label goes
/// before it, and what is kept clear around them.
fn chip_width(text: &mut dyn TextMeasure, copies: Copyable) -> f32 {
    let label = copies.label().map_or(0.0, |label| {
        text.measure_text(label, LABEL_SIZE)[0] + CHIP_GAP
    });
    (2.0 * CHIP_PADDING + label + COPY_ICON).round()
}

/// A copy button: its label, and after it the mark for the two sheets a copy
/// makes of one.
///
/// On opaque ground rather than the panel's own, because the one in the
/// column is drawn over the words it would copy and they must not show
/// through it; and outlined, because ground the colour of the panel it sits
/// on would otherwise leave it no edge.
fn chip(
    frame: &mut UiFrame,
    text: &mut dyn TextMeasure,
    rect: Rect,
    copies: Copyable,
    hover: bool,
    theme: &Theme,
) {
    let (edge, ink) = if hover {
        (theme.accent, theme.text_primary)
    } else {
        (theme.border, theme.text_dim)
    };
    frame.rounded_rect(rect, CELL_RADIUS, theme.bar_background);
    outline(frame, rect, RULE_WIDTH, edge);

    // Centred as one, so that a button wearing only the mark has it in the
    // middle rather than pushed to the end a label would have started at.
    let label = copies
        .label()
        .map(|label| (label, text.measure_text(label, LABEL_SIZE)[0]));
    let held = label.map_or(0.0, |(_, width)| width + CHIP_GAP) + COPY_ICON;
    let x = rect.x + ((rect.width - held) / 2.0).round();
    if let Some((label, _)) = label {
        frame.text(
            [x, text_top(frame, text, rect, LABEL_SIZE)],
            LABEL_SIZE,
            ink,
            label,
        );
    }
    // The chip's own ground, which is opaque, is what the sheet in front is
    // knocked out of: the mark has to read as one sheet over another rather
    // than as a lattice, and a wash would show the sheet behind through it.
    let held = Rect::new(
        x + label.map_or(0.0, |(_, width)| width + CHIP_GAP),
        rect.y,
        COPY_ICON,
        rect.height,
    );
    icon::draw(
        frame,
        icon::COPY,
        icon::fit(frame, held, COPY_ICON),
        ink,
        theme.bar_background,
    );
}

/// Draws the panel: the column scrolled by [`Panels::info_scroll`], with a
/// copy button on whatever [`Panels::info_hover`] says the pointer is over.
pub(super) fn draw(
    frame: &mut UiFrame,
    text: &mut dyn TextMeasure,
    current: &Current,
    panels: &Panels,
    content: Rect,
    theme: &Theme,
) {
    let (scroll, hover) = (panels.info_scroll, panels.info_hover);
    let Some(panel) = panel(content, panels.show_histogram) else {
        return;
    };
    frame.rounded_rect(panel, PANEL_RADIUS, theme.panel_background);

    let (header, view) = parts(text, panel);
    frame.text_wrapped(
        [header.hint.x, header.hint.y],
        LABEL_SIZE,
        theme.text_dim,
        header.hint.width,
        header.hint,
        HINT.to_string(),
    );
    chip(
        frame,
        text,
        header.button,
        Copyable::All,
        hover == Some(Copyable::All),
        theme,
    );
    // A hairline between the header and the column, which says that the
    // column runs on under the header rather than stopping short of it.
    frame.hairline(
        Rect::new(
            view.x,
            (view.y - HEADER_GAP / 2.0).round(),
            view.width - SCROLLBAR_GUTTER,
            RULE_WIDTH,
        ),
        theme.border,
    );

    let column = column(text, &contents(current), view.width);
    // Clamped here as well as where the wheel sets it: the window can be
    // resized under a scrolled panel, and the frame drawn from a scroll that
    // is now past the end should still show the end.
    let overflow = (column.height - view.height).max(0.0);
    let scroll = scroll.clamp(0.0, overflow);

    // Everything is clipped to the column rather than to the panel, so a line
    // slides under the inset edge instead of touching the rounded corner.
    let clip = Rect::new(view.x, view.y, view.width - SCROLLBAR_GUTTER, view.height);
    for (index, row) in column.rows.iter().enumerate() {
        let y = view.y + row.y - scroll;
        // The first heading opens the column and has nothing above it to be
        // parted from; every one after it is a section starting, which is
        // what a reader scrolling this column is looking for.
        if row.kind == Kind::Heading
            && index > 0
            && let Some(rule) = rule(y, clip)
        {
            frame.hairline(rule, theme.border);
        }
        // Cheap enough to hand every row to the text layer, which clips them,
        // but a long column would then be reshaped in full every frame.
        if y + row.height < view.y || y > view.bottom() {
            continue;
        }
        frame.text_wrapped(
            [view.x, y],
            row.kind.style().0,
            row.kind.ink(theme),
            clip.width,
            clip,
            row.text.clone(),
        );
    }

    if overflow > 0.0 {
        let track = Rect::new(
            view.right() - SCROLLBAR_WIDTH,
            view.y,
            SCROLLBAR_WIDTH,
            view.height,
        );
        frame.rounded_rect(track, SCROLLBAR_WIDTH / 2.0, theme.border);
        let (height, travel) = thumb(view.height, column.height);
        frame.rounded_rect(
            Rect::new(
                track.x,
                view.y + travel * (scroll / overflow),
                SCROLLBAR_WIDTH,
                height,
            ),
            SCROLLBAR_WIDTH / 2.0,
            theme.accent,
        );
    }

    // The button saying what the stretch under the pointer would copy, over
    // the words rather than beside them: the column is as wide as the panel
    // lets it be and there is no margin to stand one in. It costs nothing to
    // read past, being on screen only while it is being pointed at.
    //
    // On the layer above, so that it covers those words instead of being
    // covered by them, and dropped rather than drawn half where the stretch
    // it belongs to is at the edge of the panel.
    if view.height >= CHIP_HEIGHT
        && let Some(copies) = hover.filter(|copies| *copies != Copyable::All)
        && let Some((_, top, bottom)) = blocks(&column).into_iter().find(|(c, ..)| *c == copies)
    {
        let width = chip_width(text, copies);
        let middle = view.y + (top + bottom) / 2.0 - scroll;
        // Centred on what it would copy, but nudged back inside the panel
        // where that would hang it over an edge — which the first heading,
        // being a single line at the very top of the column, otherwise does.
        // A button belongs to the thing it is beside, and half a button at
        // the edge of a panel belongs to nothing.
        let y = (middle - CHIP_HEIGHT / 2.0)
            .round()
            .clamp(view.y, view.bottom() - CHIP_HEIGHT);
        let rect = Rect::new(clip.right() - width, y, width, CHIP_HEIGHT);
        frame.over(|frame| chip(frame, text, rect, copies, true, theme));
    }
}

/// The whole column, laid out into a panel `width` wide.
///
/// Nothing is decided here beyond where the words go: which sections there
/// are and what stands under them is [`contents`]'s, and the two agree about
/// what a [`Copyable::Fact`] index counts because the counting happens once,
/// here, over the list that one built.
fn column(text: &mut dyn TextMeasure, contents: &Contents, width: f32) -> Column {
    let mut column = Column {
        rows: Vec::new(),
        height: 0.0,
        width: (width - SCROLLBAR_GUTTER).max(1.0),
    };

    let mut index = 0;
    for (section, (name, facts)) in contents.sections.iter().enumerate() {
        column.add(
            text,
            Kind::Heading,
            name.to_string(),
            Copyable::Section(section),
        );
        for fact in facts {
            let copies = Copyable::Fact(index);
            column.add(text, Kind::Label, fact.name.clone(), copies);
            column.add(text, Kind::Value, fact.value.clone(), copies);
            index += 1;
        }
    }
    column
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
/// picture always has a colour space.
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
        ("Colour space", image.color.label()),
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
    use crate::ui::Monospace;

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
                        entry(
                            "Exposure",
                            "1/50 s   \u{00b7}   f/1.78   \u{00b7}   ISO 200",
                        ),
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

    /// A panel with room for a column `height` tall and the header over it,
    /// so that there is nothing left to scroll.
    fn roomy(height: f32) -> Rect {
        let inset = 2.0 * PANEL_INSET;
        let panel = Rect::new(0.0, 0.0, PANEL_WIDTH, height + inset);
        let taken = parts(&mut Monospace, panel).1.y - panel.y;
        Rect::new(0.0, 0.0, PANEL_WIDTH, height + taken + PANEL_INSET)
    }

    fn written(current: &Current, width: f32) -> Vec<String> {
        column(&mut Monospace, &contents(current), width)
            .rows
            .into_iter()
            .map(|row| row.text)
            .collect()
    }

    /// Every fact the panel exists to show, written out rather than merely
    /// headed: what the file is, then what the picture in it is, then the
    /// metadata's own groups after both.
    #[test]
    fn the_column_says_what_the_file_is() {
        let written = written(&current(), 300.0);
        for expected in [
            "kingfisher.png",
            "/home/reader/pictures/kingfisher.png",
            "png",
            "1.26 MB (1,258,291 bytes)",
            "2025-08-31 09:32:02 UTC",
            "4 \u{00d7} 5",
            "8-bit rgb",
            "BT.709 / sRGB",
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
        let written = written(&current, 300.0);
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
        let written = written(&current, 300.0);
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

    /// What the pointer is over is what a click there would copy: the button
    /// at the top the whole panel, a heading its section, and a name or the
    /// value under it the one field the two of them are.
    #[test]
    fn what_the_pointer_is_over_is_what_a_click_would_copy() {
        let current = current();
        let panel = panel(CONTENT, false).expect("room in a 900x640 content area");
        let (header, view) = parts(&mut Monospace, panel);
        let at = |x: f32, y: f32| copyable_at(&mut Monospace, &current, panel, 0.0, [x, y]);

        assert_eq!(
            at(header.button.x + 1.0, header.button.y + 1.0),
            Some(Copyable::All)
        );
        // The hint beside it is an instruction rather than a fact, and copies
        // nothing.
        assert_eq!(at(header.hint.x + 1.0, header.hint.y + 1.0), None);

        let column = column(&mut Monospace, &contents(&current), view.width);
        let blocks = blocks(&column);
        assert_eq!(blocks[0].0, Copyable::Section(0), "the first heading");
        assert_eq!(blocks[1].0, Copyable::Fact(0), "the field under it");

        for (copies, top, bottom) in blocks {
            let middle = view.y + (top + bottom) / 2.0;
            if middle > view.bottom() {
                break;
            }
            assert_eq!(at(view.x + 1.0, middle), Some(copies));
            // The gutter is the scrollbar's, however far down the column it
            // falls: a press there takes hold of the thumb.
            assert_eq!(at(view.right() - 1.0, middle), None);
        }

        // Above the column is the header, and below the last line is nothing.
        assert_eq!(at(view.x + 1.0, view.y - 1.0), None);
        assert_eq!(at(view.x + 1.0, view.bottom() + 1.0), None);
    }

    /// The hairline that parts two sections sits in the space above the
    /// heading, belonging to neither section, and is dropped rather than
    /// drawn half when that space falls off the end of the panel.
    #[test]
    fn a_section_is_parted_from_the_one_before_it_by_a_line() {
        let clip = Rect::new(10.0, 100.0, 200.0, 300.0);
        let heading = 240.0;
        let line = rule(heading, clip).expect("a heading in the middle of the panel");
        assert!(line.bottom() <= heading && line.y >= heading - SECTION_GAP);
        assert_eq!(line.x, clip.x, "across the whole column");
        assert_eq!(line.width, clip.width);
        assert_eq!(line.height, RULE_WIDTH, "a hairline, not a band");

        // A heading scrolled to the very top of the panel has taken its gap
        // off the top with it, and there is nothing left to draw a line in.
        // The same at the bottom — though a heading only just past the end
        // still has its gap on screen, and is announced by the line before
        // the words themselves come up.
        assert_eq!(rule(clip.y, clip), None);
        assert!(rule(clip.bottom() + 1.0, clip).is_some());
        assert_eq!(rule(clip.bottom() + SECTION_GAP, clip), None);
    }

    /// What the wheel is clamped against: enough to bring the last line up to
    /// the bottom of the panel, and not a pixel more.
    #[test]
    fn the_column_scrolls_exactly_as_far_as_it_overflows() {
        let current = current();
        let panel = panel(CONTENT, false).expect("room in a 900x640 content area");
        let (_, view) = parts(&mut Monospace, panel);
        let height = column(&mut Monospace, &contents(&current), view.width).height;

        assert!(height > view.height, "a photograph's metadata overflows");
        assert_eq!(
            max_scroll(&mut Monospace, &current, panel),
            height - view.height
        );

        // Rows are stacked in the order they were added, each below the last
        // by exactly the gap its kind asks for — a value included, which is
        // pulled a pixel up into its label's line box rather than set below
        // it, that pixel being leading and not ink.
        let rows = column(&mut Monospace, &contents(&current), view.width).rows;
        for pair in rows.windows(2) {
            let gap = pair[1].kind.style().1;
            assert_eq!(pair[1].y, pair[0].y + pair[0].height + gap);
            assert!(pair[0].y < pair[1].y, "rows go down the column");
        }
        assert_eq!(rows[0].y, 0.0, "the column starts at the top of the panel");

        // Nowhere to scroll to when the panel is taller than its column.
        assert_eq!(max_scroll(&mut Monospace, &current, roomy(height)), 0.0);
    }

    /// What a drag of the scrollbar is multiplied by: dragging the thumb the
    /// length of its track scrolls the column from its first line to its
    /// last, however much longer than the track the column is.
    #[test]
    fn dragging_the_thumb_across_its_track_scrolls_the_whole_column() {
        let current = current();
        let panel = panel(CONTENT, false).expect("room in a 900x640 content area");
        let (_, view) = parts(&mut Monospace, panel);
        let height = column(&mut Monospace, &contents(&current), view.width).height;

        let per_pixel = scroll_per_drag(&mut Monospace, &current, panel);
        let travel = thumb(view.height, height).1;
        assert!(travel > 0.0, "a thumb with somewhere to go");
        assert!(
            (per_pixel * travel - max_scroll(&mut Monospace, &current, panel)).abs() < 0.01,
            "{per_pixel} per pixel over {travel} does not cross the column"
        );
        assert!(
            per_pixel > 1.0,
            "a column longer than its panel outruns the pointer"
        );

        // Nothing to scroll: the drag is left as it comes, and the clamp on
        // the scroll is what keeps the column still.
        assert_eq!(
            scroll_per_drag(&mut Monospace, &current, roomy(height)),
            1.0
        );
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
        // Room for the panel, but not once the histogram has had its corner.
        let squeezed = Rect::new(0.0, 0.0, 900.0, 200.0);
        assert!(panel(squeezed, false).is_some());
        assert_eq!(panel(squeezed, true), None);
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
