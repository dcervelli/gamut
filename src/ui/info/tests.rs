use super::*;
use crate::ui::histogram;
use std::time::Duration;

use crate::image::exif::{Entry, Exif, Section};
use crate::image::sequence::Sequence;
use crate::image::{AlphaMode, Channels, ColorSpace, DecodedImage, Samples};

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
    Current {
        file: FileFacts {
            path: "/home/reader/pictures/kingfisher.png".into(),
            bytes: Some(1_258_291),
            modified: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_756_632_722)),
            reader: Some("png"),
        },
        exif: photograph(),
        ..Current::of(image, "kingfisher.png")
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
                group: Group::Camera,
                entries: vec![
                    entry("Camera", "Apple iPhone 16 Pro"),
                    entry("Exposure", "1/50 s \u{00b7} f/1.78 \u{00b7} ISO 200"),
                ],
            },
            Section {
                group: Group::About,
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
    // What somebody wrote about the picture comes before how it is
    // stored, and what took it after.
    assert!(index("File") < index("About"));
    assert!(index("About") < index("Field 0"));
    assert!(index("Field 23") < index("Image"));
}

/// A picture carrying a depth map has it described in a section of its
/// own after the picture's and before the camera's, whichever of the two
/// is on screen; one without has no such section.
#[test]
fn a_depth_map_is_described_after_the_image() {
    use crate::image::depth::{Accuracy, DepthMap, Quantity, Scale, Unit, Vendor};
    let mut current = current();
    assert!(!written(&current).iter().any(|row| row == DEPTH_MAP));
    let map = |scale| DepthMap {
        width: 2,
        height: 3,
        samples: Samples::U8 {
            channels: Channels::Gray,
            data: vec![0; 6],
        },
        scale,
    };
    let carrying = |current: &mut Current, map: DepthMap| {
        let mut image = (*current.image).clone();
        image.depth = Some(std::sync::Arc::new(map));
        current.image = std::sync::Arc::new(image);
    };
    let section = |current: &Current| {
        let rows = written(current);
        let at = rows
            .iter()
            .position(|row| row == DEPTH_MAP)
            .expect("a section");
        assert!(rows.iter().position(|row| row == "Image") < Some(at));
        assert!(rows.iter().position(|row| row == "Camera") > Some(at));
        let end = rows.iter().position(|row| row == "Camera").unwrap();
        rows[at + 1..end].to_vec()
    };
    let pairs = |pairs: &[(&str, &str)]| -> Vec<String> {
        pairs
            .iter()
            .flat_map(|(name, value)| [name.to_string(), value.to_string()])
            .collect()
    };

    // Codes with nothing said about them.
    carrying(&mut current, map(None));
    assert_eq!(
        section(&current),
        pairs(&[
            ("Resolution", "2 \u{00d7} 3"),
            ("Samples", "8-bit gray"),
            ("Encoding", "unknown"),
        ])
    );

    // An iPhone's disparity, estimated in scale: the range runs from
    // near to far whichever code stands for which.
    carrying(
        &mut current,
        map(Some(Scale {
            codes: Some([0.0, 255.0]),
            values: [0.25, 2.0],
            quantity: Quantity::Inverse,
            unit: Unit::Meters,
            accuracy: Accuracy::Relative,
            vendor: Vendor::Apple,
        })),
    );
    assert_eq!(
        section(&current),
        pairs(&[
            ("Resolution", "2 \u{00d7} 3"),
            ("Samples", "8-bit gray"),
            ("Described by", "Apple"),
            ("Encoding", "inverse distance"),
            ("Range", "\u{2248}0.50 m to \u{2248}4.00 m"),
            ("Accuracy", "relative"),
        ])
    );

    // Shown in the picture's place, the map is still the picture's, and
    // its heading says it is on screen rather than a row of the image's.
    let showing = |current: &Current| {
        contents(current)
            .sections
            .iter()
            .find(|section| section.name == DEPTH_MAP)
            .expect("a section")
            .showing
    };
    assert!(!showing(&current));
    let (picture, _) = current.picture();
    let face = crate::ui::Face::new(picture.depth.as_ref().unwrap().image());
    current.show(
        crate::image::auxiliary::Showing::Auxiliary(crate::image::auxiliary::Auxiliary::Depth),
        |_| Some(face),
    );
    assert!(section(&current).contains(&"Apple".to_string()));
    assert!(showing(&current));
    assert!(!written(&current).iter().any(|row| row == "Showing"));
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
    // A table of its own, a region beside each row, named by who is
    // in it and saying what kind of region it is in its tooltip.
    let contents = contents(&current);
    let section = contents
        .sections
        .iter()
        .find(|section| section.name == REGIONS)
        .expect("a regions section");
    assert!(matches!(section.face, Face::Regions));
    assert_eq!(section.regions.len(), section.facts.len());
    assert_eq!(section.regions[0].subject, "Jane Doe");
    assert_eq!(section.regions[0].about, "Face \u{00b7} Jane Doe");
    assert_eq!(
        section.regions[0].placed.map(Placed::bounds),
        Some([0.0, 0.0, 2.0, 2.0])
    );
    // A row copies as its cells, and the section as the table.
    let at = contents
        .facts()
        .position(|(name, _)| name == REGIONS)
        .expect("a region's row");
    assert_eq!(copied(&current, Copyable::Fact(at)), "Jane Doe,0,0,2,2");
    let place = contents
        .sections
        .iter()
        .position(|section| section.name == REGIONS)
        .expect("a regions section");
    assert_eq!(
        copied(&current, Copyable::Section(place)),
        "Subject,X,Y,W,H\nJane Doe,0,0,2,2"
    );
    let written_now = written(&current);
    let index = |text: &str| written_now.iter().position(|row| row == text);
    assert!(index("Camera") < index("Regions"));
    assert_eq!(
        written_now[index("Face").expect("the region is written") + 1],
        "Jane Doe,0,0,2,2"
    );
    current.turn = current.turn.clockwise();
    assert!(
        written(&current)
            .iter()
            .any(|row| row == "Jane Doe,3,0,2,2"),
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
    assert!(!written.contains(&"About".to_string()), "{written:?}");
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
