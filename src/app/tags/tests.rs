use super::*;
use crate::fuzzy::Plain;

fn stamp(len: u64) -> Signature {
    Signature::of_length(len)
}

fn text(value: &str) -> Value {
    Value::Text(value.to_string())
}

fn tag(family0: &str, family1: &str, name: &str, printed: Value) -> Tag {
    Tag {
        family0: family0.to_string(),
        family1: family1.to_string(),
        name: name.to_string(),
        id: None,
        table: None,
        lang: None,
        desc: name.to_string(),
        printed,
        raw: None,
    }
}

fn report() -> Report {
    Report {
        version: "13.55".to_string(),
        tags: vec![
            Tag {
                desc: "File Size".to_string(),
                ..tag("File", "System", "FileSize", text("1540 bytes"))
            },
            tag("File", "File", "FileType", text("image/jpeg")),
            Tag {
                raw: Some(text("3")),
                ..tag("EXIF", "IFD0", "Orientation", text("Rotate 180"))
            },
            tag("EXIF", "ExifIFD", "UserComment", text("made by hand")),
            tag(
                "XMP",
                "XMP-dc",
                "Subject",
                Value::List(vec![text("red"), text("green")]),
            ),
            tag(
                "XMP",
                "XMP-apdi",
                "Calibration",
                Value::Struct(vec![
                    (
                        "IntrinsicMatrix".to_string(),
                        Value::List(vec![text("1"), text("2"), text("3")]),
                    ),
                    ("Name".to_string(), text("wide")),
                ]),
            ),
            tag("Composite", "Composite", "ImageSize", text("32x24")),
        ],
    }
}

fn tags() -> Tags {
    Tags::with(Box::new(Plain))
}

/// A file's tags, asked for and answered.
fn answered(tags: &mut Tags, path: &Path, stamp: Signature) {
    let request = tags.want(path, stamp).expect("asked");
    assert!(tags.take(Delivered {
        asked: request.asked,
        path: path.to_path_buf(),
        stamp,
        outcome: Ok(report()),
    }));
}

fn names(input: &Input) -> Vec<String> {
    input
        .rows
        .iter()
        .map(|row| format!("{}{}", "  ".repeat(row.depth.into()), row.name))
        .collect()
}

/// A file kept is not run again; the same file rewritten is; and a file
/// whose run is on its way is not asked for twice.
#[test]
fn a_run_is_asked_for_once_per_file_as_it_stands() {
    let mut tags = tags();
    let path = Path::new("/a.jpg");
    let first = stamp(1);
    let request = tags.want(path, first).expect("nothing kept yet");
    assert!(tags.want(path, first).is_none(), "already on its way");
    assert_eq!(tags.input("a.jpg", "exiftool").state, State::Waiting);
    tags.take(Delivered {
        asked: request.asked,
        path: path.to_path_buf(),
        stamp: first,
        outcome: Ok(report()),
    });
    assert!(tags.want(path, first).is_none(), "kept");
    assert_eq!(tags.input("a.jpg", "exiftool").state, State::Ready);
    assert!(tags.want(path, stamp(2)).is_some(), "rewritten since");
}

/// An answer to a run asked before the last is kept, but ends no wait, and
/// does not replace a later answer for the same file.
#[test]
fn a_stale_answer_is_kept_but_ends_no_wait() {
    let mut tags = tags();
    let (a, b) = (Path::new("/a.jpg"), Path::new("/b.jpg"));
    let stamp = stamp(1);
    let for_a = tags.want(a, stamp).expect("asked");
    let for_b = tags.want(b, stamp).expect("asked");
    assert!(!tags.take(Delivered {
        asked: for_a.asked,
        path: a.to_path_buf(),
        stamp,
        outcome: Ok(report()),
    }));
    assert_eq!(tags.input("b.jpg", "exiftool").state, State::Waiting);
    assert!(tags.want(b, stamp).is_none(), "b's run is still waited for");
    assert!(tags.want(a, stamp).is_none(), "a's answer was kept");
    let _ = for_b;
}

/// A program not there is said, and asked again next time: it may have
/// been installed since.
#[test]
fn not_installed_is_said_and_asked_again() {
    let mut tags = tags();
    let path = Path::new("/a.jpg");
    let stamp = stamp(1);
    let request = tags.want(path, stamp).expect("asked");
    tags.take(Delivered {
        asked: request.asked,
        path: path.to_path_buf(),
        stamp,
        outcome: Err(Failure::NotInstalled),
    });
    assert_eq!(
        tags.input("a.jpg", "exiftool").state,
        State::NotInstalled {
            configured: "exiftool".to_string()
        }
    );
    assert!(tags.want(path, stamp).is_some());
}

/// The tree is grouped by kind, then by where; a kind with one group of its
/// own name has no level between; a list of text is one row, and a
/// structure's pieces follow it.
#[test]
fn the_tree_is_grouped_twice() {
    let mut tags = tags();
    answered(&mut tags, Path::new("/a.jpg"), stamp(1));
    tags.fold(Fold::Open);
    let input = tags.input("a.jpg", "exiftool");
    assert_eq!(
        names(&input),
        [
            "File",
            "  System",
            "    File Size",
            "  File",
            "    FileType",
            "EXIF",
            "  IFD0",
            "    Orientation",
            "  ExifIFD",
            "    UserComment",
            "XMP",
            "  XMP-dc",
            "    Subject",
            "  XMP-apdi",
            "    Calibration",
            "      IntrinsicMatrix",
            "      Name",
            "Composite",
            "  ImageSize",
        ]
    );
    assert_eq!((input.shown, input.total), (7, 7));
    assert_eq!(input.version.as_deref(), Some("13.55"));
    assert_eq!(input.rows[0].count, 2);
    assert_eq!(input.rows[12].value, "[red, green]");
    assert_eq!(input.rows[14].value, "2 fields");
    assert_eq!(input.rows[15].value, "[1, 2, 3]");
    assert!(input.rows.iter().all(|row| row.group.is_none()));
    // A tag is headed by exiftool's words for it, and a field by its path.
    assert_eq!(input.rows[2].name, "File Size");
    assert_eq!(input.rows[15].name, "IntrinsicMatrix");
    assert_eq!(input.tops.len(), input.rows.len() + 1);
}

fn ranked_names(tags: &mut Tags, query: &str) -> Vec<String> {
    tags.set_query(query.to_string());
    tags.input("a.jpg", "exiftool")
        .rows
        .iter()
        .filter(|row| row.kind == Kind::Tag)
        .map(|row| format!("{}:{}", row.group.as_deref().unwrap_or("-"), row.name))
        .collect()
}

/// Under a query the tags are a list, best first, each saying its group: a
/// name holding the query ahead of a value holding it, whatever the file's
/// order, and scattered hits only where nothing holds it outright.
#[test]
fn a_query_ranks_names_over_values_over_scattered_hits() {
    let mut tags = tags();
    answered(&mut tags, Path::new("/a.jpg"), stamp(1));
    assert_eq!(
        ranked_names(&mut tags, "image"),
        ["Composite:ImageSize", "File:FileType"]
    );
    let input = tags.input("a.jpg", "exiftool");
    assert_eq!(input.rows[0].name_lit, [0, 1, 2, 3, 4]);
    assert_eq!(input.rows[1].value_lit, [0, 1, 2, 3, 4]);
    assert_eq!((input.shown, input.total), (2, 7));

    // The words may be anywhere, in any order.
    assert_eq!(ranked_names(&mut tags, "rot ifd0"), ["IFD0:Orientation"]);
    // Nothing holds `fsz`: the matcher's scattered hit stands.
    assert_eq!(ranked_names(&mut tags, "fsz"), ["System:File Size"]);
    assert!(ranked_names(&mut tags, "qqq").is_empty());
}

/// The tree starts with every group shut; one opened stays open for the
/// session, a query lists the tags without their groups, and the button
/// opens every group while any is shut and folds them all while none is.
#[test]
fn the_tree_starts_shut_and_folds_all_at_once() {
    let mut tags = tags();
    answered(&mut tags, Path::new("/a.jpg"), stamp(1));
    let input = tags.input("a.jpg", "exiftool");
    assert_eq!(names(&input), ["File", "EXIF", "XMP", "Composite"]);
    assert!(input.rows.iter().all(|row| row.collapsed));
    assert_eq!(input.fold, Some(Fold::Open));

    tags.toggle_group(0);
    let input = tags.input("a.jpg", "exiftool");
    assert_eq!(names(&input)[..4], ["File", "  System", "  File", "EXIF"]);

    assert_eq!(ranked_names(&mut tags, "jpeg"), ["File:FileType"]);
    assert_eq!(
        tags.input("a.jpg", "exiftool").fold,
        None,
        "no tree to fold"
    );
    tags.set_query(String::new());
    assert!(
        !tags.input("a.jpg", "exiftool").rows[0].collapsed,
        "open for the session"
    );

    tags.fold(Fold::Open);
    let input = tags.input("a.jpg", "exiftool");
    assert_eq!(input.rows.len(), 19);
    assert_eq!(input.fold, Some(Fold::Shut));
    tags.toggle_group(1);
    assert_eq!(tags.input("a.jpg", "exiftool").fold, Some(Fold::Open));
    tags.fold(Fold::Shut);
    assert_eq!(tags.input("a.jpg", "exiftool").rows.len(), 4);
}

/// A click copies a tag's value, a list of text as it is shown, a
/// structure a line per piece, one piece, and nothing for a group.
#[test]
fn a_row_copies_its_value() {
    let mut tags = tags();
    answered(&mut tags, Path::new("/a.jpg"), stamp(1));
    tags.fold(Fold::Open);
    let input = tags.input("a.jpg", "exiftool");
    let at = |name: &str| {
        input
            .rows
            .iter()
            .position(|row| row.name == name)
            .expect("a row")
    };
    assert_eq!(
        tags.value_at(at("Orientation")).as_deref(),
        Some("Rotate 180")
    );
    assert_eq!(
        tags.value_at(at("Subject")).as_deref(),
        Some("[red, green]")
    );
    assert_eq!(
        tags.value_at(at("Calibration")).as_deref(),
        Some("IntrinsicMatrix: [1, 2, 3]\nName: wide")
    );
    assert_eq!(
        tags.value_at(at("IntrinsicMatrix")).as_deref(),
        Some("[1, 2, 3]")
    );
    assert_eq!(tags.value_at(0), None);
}

/// The copies are of the tags that fit, folded or not, a structure's
/// pieces a line each.
#[test]
fn a_copy_is_of_what_fits() {
    let mut tags = tags();
    assert_eq!(tags.copied(Table::Csv), None);
    answered(&mut tags, Path::new("/a.jpg"), stamp(1));
    let _ = tags.input("a.jpg", "exiftool");
    tags.toggle_group(0);
    let (all, count) = tags.copied(Table::Csv).expect("tags in");
    assert_eq!(count, 7);
    assert_eq!(all.lines().count(), 1 + 8);
    assert!(all.contains("XMP,XMP-dc,Subject,,Subject,\"[red, green]\","));
    assert!(all.contains("XMP,XMP-apdi,Calibration.IntrinsicMatrix,,Calibration,\"[1, 2, 3]\","));
    assert!(all.contains("EXIF,IFD0,Orientation,,Orientation,Rotate 180,3"));

    tags.set_query("180".to_string());
    let (text, count) = tags.copied(Table::Text).expect("tags in");
    assert_eq!(count, 1);
    assert_eq!(
        text,
        "[IFD0]         Orientation                     : Rotate 180 (3)"
    );
}

/// A click on a query's hit copies it, and then puts the query away and
/// shows the tag in the tree: its two groups opened, and no other, the tag
/// marked, and the list told once to bring it into view.
#[test]
fn a_hit_chosen_is_shown_in_the_tree() {
    let mut tags = tags();
    answered(&mut tags, Path::new("/a.jpg"), stamp(1));
    // One group open already, which is left as it is.
    let _ = tags.input("a.jpg", "exiftool");
    tags.toggle_group(3);
    assert_eq!(ranked_names(&mut tags, "180"), ["IFD0:Orientation"]);
    assert_eq!(tags.value_at(0).as_deref(), Some("Rotate 180"));
    tags.choose(0);

    let input = tags.input("a.jpg", "exiftool");
    assert!(input.query.is_empty());
    assert_eq!(
        names(&input),
        [
            "File",
            "EXIF",
            "  IFD0",
            "    Orientation",
            "  ExifIFD",
            "XMP",
            "Composite",
            "  ImageSize",
        ]
    );
    assert_eq!(input.marked, Some(3));
    assert!(input.reveal);
    let again = tags.input("a.jpg", "exiftool");
    assert_eq!(again.marked, Some(3), "still marked");
    assert!(!again.reveal, "brought into view once");

    // A click in the tree marks, and moves nothing.
    tags.choose(7);
    let input = tags.input("a.jpg", "exiftool");
    assert_eq!(input.marked, Some(7));
    assert!(!input.reveal);
    assert_eq!(input.rows.len(), 8);
}

/// JSON and XML keep each value's shape, leave out what is not there, and
/// escape what their syntax needs.
#[test]
fn json_and_xml_keep_the_values_shape() {
    let mut tags = tags();
    let path = Path::new("/a.jpg");
    let request = tags.want(path, stamp(1)).expect("asked");
    let mut report = report();
    report.tags[3].printed = text("say \"hi\" & <go>");
    tags.take(Delivered {
        asked: request.asked,
        path: path.to_path_buf(),
        stamp: stamp(1),
        outcome: Ok(report),
    });
    tags.set_query("image".to_string());
    let (json, count) = tags.copied(Table::Json).expect("tags in");
    assert_eq!(count, 2);
    assert_eq!(
        json,
        "[\n  {\"Group\": \"Composite\", \"Subgroup\": \"Composite\", \"Tag\": \"ImageSize\", \
         \"Description\": \"ImageSize\", \"Value\": \"32x24\"},\n  \
         {\"Group\": \"File\", \"Subgroup\": \"File\", \"Tag\": \"FileType\", \
         \"Description\": \"FileType\", \"Value\": \"image/jpeg\"}\n]"
    );

    tags.set_query("calib".to_string());
    let (json, _) = tags.copied(Table::Json).expect("tags in");
    assert!(
        json.contains(
            "\"Value\": {\"IntrinsicMatrix\": [\"1\", \"2\", \"3\"], \"Name\": \"wide\"}"
        ),
        "{json}"
    );
    let (xml, _) = tags.copied(Table::Xml).expect("tags in");
    assert_eq!(
        xml,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <tags exiftool=\"13.55\">\n  \
         <tag group=\"XMP\" subgroup=\"XMP-apdi\" name=\"Calibration\" description=\"Calibration\">\n    \
         <value>\n      \
         <field name=\"IntrinsicMatrix\">\n        \
         <item>1</item>\n        <item>2</item>\n        <item>3</item>\n      \
         </field>\n      \
         <field name=\"Name\">wide</field>\n    \
         </value>\n  \
         </tag>\n\
         </tags>"
    );

    tags.set_query("hand".to_string());
    tags.set_query("say".to_string());
    let (json, _) = tags.copied(Table::Json).expect("tags in");
    assert!(json.contains(r#""Value": "say \"hi\" & <go>""#), "{json}");
    let (xml, _) = tags.copied(Table::Xml).expect("tags in");
    assert!(
        xml.contains("<value>say &quot;hi&quot; &amp; &lt;go&gt;</value>"),
        "{xml}"
    );
    let (text, _) = tags.copied(Table::Text).expect("tags in");
    assert!(text.starts_with("[ExifIFD]"), "{text}");
}
