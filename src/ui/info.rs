//! The info panel: what the file is, as a column of words down the right of
//! the content area.
//!
//! The only part of the interface with more to say than fits, so it is the
//! only part that scrolls. The column is laid out in full every frame, from
//! the top of the content down, and what falls outside the panel is cut off
//! by the text layer rather than by anything here — which is what lets a line
//! be drawn half, sliding under the panel's edge as the wheel turns.

use std::time::SystemTime;

use crate::image::AlphaMode;
use crate::render::{Color, Rect, TextMeasure, UiFrame};
use crate::theme::Theme;

use super::histogram::HISTOGRAM_SIZE;
use super::{Current, PADDING, PANEL_INSET, PANEL_RADIUS, PANEL_WIDTH, TEXT_SIZE};

/// Below this the panel would show two facts and a scrollbar, so it stays off
/// instead. There is no matching minimum for the width: the panel is
/// [`PANEL_WIDTH`] wide or it is not on screen.
const INFO_MIN_HEIGHT: f32 = 120.0;

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

/// The hairline drawn across the column above every section but the first.
/// The same width as the hairline along a panel's edge, being the same kind
/// of thing.
const RULE_WIDTH: f32 = 1.0;

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

/// One run of text in the column, at its place down it. `y` is measured from
/// the top of the column, not of the window: the panel subtracts the scroll
/// from it, and the same list serves both the drawing and the question of how
/// far it may be scrolled.
struct Row {
    text: String,
    kind: Kind,
    y: f32,
    height: f32,
}

/// The column as it is built: rows so far, how tall they come to, and the
/// width they are broken at.
struct Column {
    rows: Vec<Row>,
    height: f32,
    width: f32,
}

impl Column {
    fn add(&mut self, text: &mut dyn TextMeasure, kind: Kind, body: String) {
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
        });
    }

    /// A field: its name, and under it what it says. A field with nothing to
    /// say is left out altogether — a name with a blank under it says less
    /// than nothing.
    fn field(&mut self, text: &mut dyn TextMeasure, name: &str, value: &str) {
        if value.is_empty() {
            return;
        }
        self.add(text, Kind::Label, name.to_string());
        self.add(text, Kind::Value, value.to_string());
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

/// How far the column may be scrolled before its last line is at the bottom
/// of the panel: zero when it all fits, or when there is no panel.
///
/// The application clamps the scroll against this as the wheel turns, so that
/// a spin past the end does not leave the panel having to be wound back
/// through nothing.
pub fn max_scroll(text: &mut dyn TextMeasure, current: &Current, panel: Rect) -> f32 {
    let view = panel.inset(PANEL_INSET, PANEL_INSET);
    (column(text, current, view.width).height - view.height).max(0.0)
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
    let view = panel.inset(PANEL_INSET, PANEL_INSET);
    let column = column(text, current, view.width);
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

/// Draws the panel, with the column scrolled by `scroll` logical pixels.
pub(super) fn draw(
    frame: &mut UiFrame,
    text: &mut dyn TextMeasure,
    current: &Current,
    scroll: f32,
    content: Rect,
    show_histogram: bool,
    theme: &Theme,
) {
    let Some(panel) = panel(content, show_histogram) else {
        return;
    };
    frame.rounded_rect(panel, PANEL_RADIUS, theme.panel_background);

    let view = panel.inset(PANEL_INSET, PANEL_INSET);
    let column = column(text, current, view.width);
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
            frame.rect(rule, theme.border);
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
}

/// The whole column, laid out into a panel `width` wide.
///
/// Sections, nearest first: the file on disk, then the picture in it, then
/// whatever its metadata has to say — the camera, the place, the ground, the
/// words, and last the fields nothing above spoke for. A section with nothing
/// in it is not named, an empty heading being a question about where the rest
/// of it went; only the first two are certain, since a file always has a size
/// and a picture always has a size and a colour space.
fn column(text: &mut dyn TextMeasure, current: &Current, width: f32) -> Column {
    let mut column = Column {
        rows: Vec::new(),
        height: 0.0,
        width: (width - SCROLLBAR_GUTTER).max(1.0),
    };

    for (heading, facts) in [
        ("File", file_facts(current)),
        ("Image", image_facts(current)),
    ] {
        column.add(text, Kind::Heading, heading.to_string());
        for (name, value) in facts {
            column.field(text, name, &value);
        }
    }

    for section in &current.exif.sections {
        column.add(text, Kind::Heading, section.name.to_string());
        for entry in &section.entries {
            column.field(text, &entry.name, &entry.value);
        }
    }
    column
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
        // The range the file declared, as against the one its pixels turned
        // out to occupy: the histogram shows the second, and only the file
        // can say the first.
        (
            "Declared range",
            image
                .value_range
                .map(|(low, high)| format!("{low} \u{2013} {high}"))
                .unwrap_or_default(),
        ),
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

/// When the file was last written, as UTC.
///
/// UTC rather than local time because the standard library knows nothing of
/// time zones, and a wrong local time is worse than a right one in a zone the
/// reader has to convert from — the label says which it is.
fn format_time(time: SystemTime) -> String {
    let seconds = match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(since) => since.as_secs() as i64,
        // Before 1970, which a file can be: the error carries how far before.
        Err(before) => -(before.duration().as_secs() as i64),
    };
    let days = seconds.div_euclid(86_400);
    let time_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        time_of_day / 3600,
        (time_of_day / 60) % 60,
        time_of_day % 60
    )
}

/// The civil date `days` after 1970-01-01, by Howard Hinnant's algorithm:
/// the calendar is shifted to start in March so that the leap day falls at
/// the end of the year and the month lengths make a repeating pattern.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use crate::image::display::{Display, Startup};
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

    fn written(current: &Current, width: f32) -> Vec<String> {
        column(&mut Monospace, current, width)
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
        let view = panel.inset(PANEL_INSET, PANEL_INSET);
        let height = column(&mut Monospace, &current, view.width).height;

        assert!(height > view.height, "a photograph's metadata overflows");
        assert_eq!(
            max_scroll(&mut Monospace, &current, panel),
            height - view.height
        );

        // Rows are stacked in the order they were added, each below the last
        // by exactly the gap its kind asks for — a value included, which is
        // pulled a pixel up into its label's line box rather than set below
        // it, that pixel being leading and not ink.
        let rows = column(&mut Monospace, &current, view.width).rows;
        for pair in rows.windows(2) {
            let gap = pair[1].kind.style().1;
            assert_eq!(pair[1].y, pair[0].y + pair[0].height + gap);
            assert!(pair[0].y < pair[1].y, "rows go down the column");
        }
        assert_eq!(rows[0].y, 0.0, "the column starts at the top of the panel");

        // Nowhere to scroll to when the panel is taller than its column.
        let roomy = Rect::new(0.0, 0.0, PANEL_WIDTH, height + 2.0 * PANEL_INSET);
        assert_eq!(max_scroll(&mut Monospace, &current, roomy), 0.0);
    }

    /// What a drag of the scrollbar is multiplied by: dragging the thumb the
    /// length of its track scrolls the column from its first line to its
    /// last, however much longer than the track the column is.
    #[test]
    fn dragging_the_thumb_across_its_track_scrolls_the_whole_column() {
        let current = current();
        let panel = panel(CONTENT, false).expect("room in a 900x640 content area");
        let view = panel.inset(PANEL_INSET, PANEL_INSET);
        let height = column(&mut Monospace, &current, view.width).height;

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
        let roomy = Rect::new(0.0, 0.0, PANEL_WIDTH, height + 2.0 * PANEL_INSET);
        assert_eq!(scroll_per_drag(&mut Monospace, &current, roomy), 1.0);
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
