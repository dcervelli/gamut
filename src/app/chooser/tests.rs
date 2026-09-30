use super::*;
use crate::fuzzy::Plain;

fn paths(names: &[&str]) -> Vec<PathBuf> {
    names.iter().map(PathBuf::from).collect()
}

/// An empty query is the list as it stands, with nothing lit; a query
/// drops what it does not fit; and equal fits keep their order, so the
/// list's own order is what the user sees among them.
#[test]
fn ranking_keeps_order_among_equals_and_drops_misses() {
    let names = ["b.png", "a.png", "c.jpg", "ab.png"];
    assert_eq!(
        rank(&Plain, "", &names),
        vec![(0, vec![]), (1, vec![]), (2, vec![]), (3, vec![])]
    );
    // `png` spans three chars in each, so all three tie and stay put.
    assert_eq!(
        rank(&Plain, "png", &names),
        vec![(0, vec![2, 3, 4]), (1, vec![2, 3, 4]), (3, vec![3, 4, 5])]
    );
    // A tighter fit outranks a looser one whatever the order.
    let names = ["a-b.png", "ab.png"];
    assert_eq!(rank(&Plain, "ab", &names)[0].0, 1);
    assert!(rank(&Plain, "zzz", &names).is_empty());
}

/// The positions are char indices of `dir/name`, and the row splits
/// them at the separator by the directory's char count, so a hit in a
/// name with a multi-byte char before it still lands on the right char.
#[test]
fn positions_are_char_indices_across_the_separator() {
    let common = PathBuf::from("root");
    let (dir, name) = relative(Path::new("root/über/Ünïcode.png"), &common);
    assert_eq!((dir.as_str(), name.as_str()), ("über", "Ünïcode.png"));
    let candidate = candidate(&dir, &name, None);
    let (_, positions) = Plain.fuzzy_indices(&candidate, "ün").expect("found");
    // The first `ü` is in the directory; `n` is the third char of the
    // name, past the directory's four chars and the separator.
    assert_eq!(positions, vec![0, 6]);
    let dir_chars = dir.chars().count() + 1;
    assert_eq!(positions[1] - dir_chars, 1);
    assert_eq!(name.chars().nth(1), Some('n'));
}

/// A query beginning with `:` asks by place in the list: the exact
/// place first, then every place with those digits in it, in order,
/// or counted from the end after a `-`;
/// `:` alone keeps the whole list, and anything but digits after it
/// fits nothing.
#[test]
fn a_colon_query_asks_by_index() {
    let rows = |digits: &str| -> Vec<usize> {
        rank_by_index(digits, 25)
            .into_iter()
            .map(|(index, positions)| {
                assert!(positions.is_empty());
                index + 1
            })
            .collect()
    };
    assert_eq!(rows("3"), vec![3, 13, 23]);
    assert_eq!(
        rows("1"),
        vec![1, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 21]
    );
    assert_eq!(rows("25"), vec![25]);
    assert_eq!(rows("26"), Vec::<usize>::new());
    assert_eq!(rows("2"), vec![2, 12, 20, 21, 22, 23, 24, 25]);
    assert_eq!(rows("").len(), 25);
    assert_eq!(rows("1x"), Vec::<usize>::new());
    assert_eq!(rows("99999999999999999999999"), Vec::<usize>::new());
    assert_eq!(rows("-1"), vec![25, 16, 15, 14, 13, 12, 11, 10, 9, 8, 7, 5]);
    assert_eq!(rows("-25"), vec![1]);
    assert_eq!(rows("-26"), Vec::<usize>::new());
    assert_eq!(rows("-").len(), 25);
    assert_eq!(rows("-").first(), Some(&25));
    assert_eq!(rows("--1"), Vec::<usize>::new());
    assert_eq!(rows("-1x"), Vec::<usize>::new());
    assert_eq!(index_query(":12"), Some("12"));
    assert_eq!(index_query("12"), None);

    // Through the chooser: the cursor lands on the exact place, and
    // the row's path is that file's.
    let mut chooser = Chooser::with(Box::new(Plain));
    let list: Vec<PathBuf> = (1..=12)
        .map(|n| PathBuf::from(format!("{n}.png")))
        .collect();
    chooser.relist(&list);
    chooser.open(0);
    chooser.set_query(":2".to_string());
    assert_eq!(chooser.cursor, 0);
    assert_eq!(chooser.path_at(0), Some(Path::new("2.png")));
    assert_eq!(chooser.path_at(1), Some(Path::new("12.png")));
    let input = chooser.input(&Thumbs::default(), None);
    assert_eq!(input.count, 12);
    assert_eq!(input.rows.len(), 2);
}

/// One directory is nothing to show; nested directories show their
/// tails under the shared parent; disjoint roots show every parent
/// whole.
#[test]
fn the_common_directory_is_the_deepest_shared_one() {
    assert_eq!(
        common_dir(&paths(&["pics/a.png", "pics/b.png"])),
        PathBuf::from("pics")
    );
    assert_eq!(
        relative(Path::new("pics/a.png"), Path::new("pics")),
        ("".into(), "a.png".into())
    );

    let nested = paths(&["/home/me/pics/2024/a.png", "/home/me/pics/2025/june/b.png"]);
    assert_eq!(common_dir(&nested), PathBuf::from("/home/me/pics"));
    assert_eq!(
        relative(&nested[1], Path::new("/home/me/pics")),
        ("2025/june".into(), "b.png".into())
    );

    let disjoint = paths(&["/mnt/a/x.png", "/srv/b/y.png"]);
    assert_eq!(common_dir(&disjoint), PathBuf::from("/"));
    assert_eq!(
        relative(&disjoint[0], &common_dir(&disjoint)),
        ("mnt/a".into(), "x.png".into())
    );
    let relatives = paths(&["a/x.png", "b/y.png"]);
    assert_eq!(common_dir(&relatives), PathBuf::new());
    assert_eq!(
        relative(&relatives[1], &common_dir(&relatives)),
        ("b".into(), "y.png".into())
    );
    assert_eq!(common_dir(&paths(&["a.png"])), PathBuf::new());
    assert_eq!(common_dir(&[]), PathBuf::new());
}

/// The cursor stays inside the list whatever is asked of it, and a
/// page is as many rows as the list had room for.
#[test]
fn the_cursor_is_clamped_to_the_list() {
    let mut chooser = Chooser::with(Box::new(Plain));
    chooser.relist(&paths(&["a.png", "b.png", "c.png", "d.png"]));
    chooser.open(1);
    assert_eq!(chooser.cursor, 1);
    chooser.step(Step::Up);
    chooser.step(Step::Up);
    assert_eq!(chooser.cursor, 0);
    chooser.step(Step::Page {
        down: true,
        rows: 10,
    });
    assert_eq!(chooser.cursor, 3);
    chooser.step(Step::Down);
    assert_eq!(chooser.cursor, 3);
    chooser.step(Step::Page {
        down: false,
        rows: 2,
    });
    assert_eq!(chooser.cursor, 1);
    chooser.step(Step::Last);
    assert_eq!(chooser.cursor, 3);
    chooser.step(Step::First);
    assert_eq!(chooser.cursor, 0);

    // A query resets it to the best fit, and an empty list has nowhere
    // for it to be.
    chooser.step(Step::Last);
    chooser.set_query("c".to_string());
    assert_eq!(chooser.cursor, 0);
    assert_eq!(chooser.path_at(0), Some(Path::new("c.png")));
    chooser.set_query("zzz".to_string());
    assert_eq!(chooser.cursor, 0);
    assert_eq!(chooser.path_at(0), None);
    chooser.step(Step::Down);
    assert_eq!(chooser.cursor, 0);
}

/// The rows say what is known: the kind from the name at first, then
/// from the header; the size once it is known; and the file on screen
/// is marked wherever the query has put it.
#[test]
fn the_rows_fill_in_as_facts_arrive() {
    let mut chooser = Chooser::with(Box::new(Plain));
    let list = paths(&["clip.gif", "scan.tiff", "photo.jpeg"]);
    chooser.relist(&list);
    chooser.open(2);
    let thumbs = Thumbs::default();
    let input = chooser.input(&thumbs, Some(Path::new("photo.jpeg")));
    assert_eq!(input.rows.len(), 3);
    assert_eq!(input.rows[0].kind, "GIF");
    assert_eq!(input.rows[0].index, 1);
    assert_eq!(input.rows[0].dimensions, None);
    assert_eq!(input.current, Some(2));
    assert_eq!(input.cursor, 2);
    assert!(input.reveal);
    assert!(!input.several_dirs);
    let again = chooser.input(&thumbs, None);
    assert!(!again.reveal, "revealed once");

    assert!(!chooser.learn(
        Path::new("clip.gif"),
        Facts {
            size: Some((320, 240)),
            sequence: Sequence::Animation {
                count: 12,
                loops: crate::image::sequence::Loops::Forever,
            },
            title: None,

            format: None,

            bytes: None,
            modified: None,
        },
    ));
    chooser.take(Delivered {
        path: PathBuf::from("scan.tiff"),
        news: News::Facts(Facts {
            size: None,
            sequence: Sequence::Pages {
                count: 1,
                default: 0,
            },
            title: None,

            format: None,

            bytes: None,
            modified: None,
        }),
    });
    let input = chooser.input(&thumbs, None);
    assert_eq!(input.rows[0].kind, "GIF, 12 frames");
    assert_eq!(input.rows[0].dimensions, Some((320, 240)));
    assert_eq!(input.rows[1].kind, "TIFF, 1 page");
    assert_eq!(input.current, None);

    // The query reorders the rows, and the mark follows the file.
    chooser.set_query("photo".to_string());
    let input = chooser.input(&thumbs, Some(Path::new("photo.jpeg")));
    assert_eq!(input.rows.len(), 1);
    assert_eq!(input.rows[0].index, 3);
    assert_eq!(input.current, Some(0));
    assert_eq!(input.rows[0].positions, vec![0, 1, 2, 3, 4]);
}

/// A title, once it arrives, is matched on beside the name: a query
/// that fits only the title finds the file, the row shows the title
/// with the hit lit in it, and the matches are made again once for the
/// frame rather than as each title lands — with the cursor kept on the
/// file it was on.
#[test]
fn a_title_is_matched_on_once_it_is_known() {
    let mut chooser = Chooser::with(Box::new(Plain));
    let list = paths(&["buteo-buteo-2.webp", "falco-1.webp", "aquila-3.webp"]);
    chooser.relist(&list);
    chooser.open(0);
    let thumbs = Thumbs::default();
    chooser.set_query("zzard".to_string());
    assert!(chooser.input(&thumbs, None).rows.is_empty());

    let titled = |title: &str| Facts {
        size: Some((887, 1200)),
        sequence: Sequence::Still,
        title: Some(title.to_string()),

        format: None,

        bytes: None,
        modified: None,
    };
    chooser.take(Delivered {
        path: PathBuf::from("buteo-buteo-2.webp"),
        news: News::Facts(titled("Common Buzzard")),
    });
    // Not yet: the matches wait for the frame.
    assert!(chooser.stale);
    assert_eq!(chooser.path_at(0), None);
    let input = chooser.input(&thumbs, None);
    assert_eq!(input.rows.len(), 1);
    assert_eq!(input.rows[0].name, "buteo-buteo-2.webp");
    assert_eq!(input.rows[0].title.as_deref(), Some("Common Buzzard"));
    assert!(input.rows[0].positions.is_empty(), "{:?}", input.rows[0]);
    // "zzard" starts at the tenth char of the title.
    assert_eq!(input.rows[0].title_positions, (9..14).collect::<Vec<_>>());
    assert_eq!(chooser.path_at(0), Some(Path::new("buteo-buteo-2.webp")));

    // A query that spans the name and the title finds it too, with the
    // hit in the name lit there and the space between the two lit
    // nowhere.
    chooser.set_query("buteo buzz".to_string());
    let input = chooser.input(&thumbs, None);
    assert_eq!(input.rows.len(), 1);
    assert_eq!(input.rows[0].positions, vec![0, 1, 2, 3, 4]);
    assert_eq!(input.rows[0].title_positions, vec![7, 8, 9, 10]);

    // The cursor stays on its file when titles arriving reorder the
    // rows around it.
    chooser.set_query("a".to_string());
    chooser.step(Step::Down);
    let on = chooser.path_at(chooser.cursor).unwrap().to_path_buf();
    chooser.take(Delivered {
        path: PathBuf::from("aquila-3.webp"),
        news: News::Facts(titled("Golden Eagle")),
    });
    chooser.take(Delivered {
        path: PathBuf::from("falco-1.webp"),
        news: News::Facts(titled("Peregrine Falcon")),
    });
    chooser.input(&thumbs, None);
    assert_eq!(chooser.path_at(chooser.cursor), Some(on.as_path()));

    // The same facts again are nothing new; a changed title is.
    chooser.input(&thumbs, None);
    assert!(!chooser.stale);
    chooser.learn(Path::new("falco-1.webp"), titled("Peregrine Falcon"));
    assert!(!chooser.stale);
    chooser.learn(Path::new("falco-1.webp"), titled("Peregrine"));
    assert!(chooser.stale);
}

/// A query puts different files under the same rows, so the range
/// the screen reported is forgotten and the pass reports it again —
/// the interface asks for the visible rows' thumbnails only when the
/// range it sees differs from the one it was given.
#[test]
fn a_new_query_asks_for_the_rows_under_the_screen_again() {
    let mut chooser = Chooser::with(Box::new(Plain));
    chooser.relist(&paths(&["a.png", "b.png", "c.png"]));
    chooser.open(0);
    let thumbs = Thumbs::default();
    chooser.wanted(0..3, &thumbs);
    assert_eq!(chooser.input(&thumbs, None).visible, 0..3);
    chooser.set_query("c".into());
    assert_eq!(chooser.input(&thumbs, None).visible, 0..0);
}

/// What the visible rows want is what they lack and have not been
/// given up on.
#[test]
fn the_visible_rows_ask_for_what_they_lack() {
    let mut chooser = Chooser::with(Box::new(Plain));
    chooser.relist(&paths(&["a.png", "b.png", "c.png"]));
    chooser.open(0);
    let mut thumbs = Thumbs::default();
    chooser.take(Delivered {
        path: PathBuf::from("b.png"),
        news: News::Failed,
    });
    let ctx = egui::Context::default();
    let texture = ctx.load_texture(
        "c",
        egui::ColorImage::filled([1, 1], egui::Color32::BLACK),
        egui::TextureOptions::LINEAR,
    );
    thumbs.insert(
        PathBuf::from("c.png"),
        [texture.clone(), texture.clone(), texture],
    );
    chooser.learn(
        Path::new("c.png"),
        Facts {
            size: Some((1, 1)),
            sequence: Sequence::Still,
            title: None,

            format: None,

            bytes: None,
            modified: None,
        },
    );
    assert_eq!(chooser.wanted(0..3, &thumbs), paths(&["a.png"]));
    assert_eq!(chooser.input(&thumbs, None).visible, 0..3);

    // A file given up on that then decodes on screen is wanted again.
    let facts = Facts {
        size: Some((1, 1)),
        sequence: Sequence::Still,
        title: None,

        format: None,

        bytes: None,
        modified: None,
    };
    assert!(chooser.learn(Path::new("b.png"), facts.clone()));
    assert!(!chooser.learn(Path::new("b.png"), facts));
    assert_eq!(chooser.wanted(0..3, &thumbs), paths(&["a.png", "b.png"]));
}

/// The screen keeps the newest thumbnails and the ones most recently
/// seen, and lets the rest go in the order they were last seen.
#[test]
fn thumbnails_are_let_go_least_recently_seen_first() {
    let ctx = egui::Context::default();
    let texture = |name: &str| {
        ctx.load_texture(
            name,
            egui::ColorImage::filled([1, 1], egui::Color32::BLACK),
            egui::TextureOptions::LINEAR,
        )
    };
    let mut thumbs = Thumbs::default();
    for index in 0..MAX_THUMBS {
        thumbs.insert(
            PathBuf::from(format!("{index}.png")),
            [texture("t"), texture("t"), texture("t")],
        );
    }
    assert_eq!(thumbs.len(), MAX_THUMBS);
    assert!(thumbs.get(Path::new("0.png")).is_some());

    // Seeing the oldest again spares it; the next oldest goes instead.
    thumbs.touch(Path::new("0.png"));
    let before = thumbs.generation;
    thumbs.insert(
        PathBuf::from("new.png"),
        [texture("t"), texture("t"), texture("t")],
    );
    assert_eq!(thumbs.len(), MAX_THUMBS);
    assert!(thumbs.get(Path::new("0.png")).is_some());
    assert!(thumbs.get(Path::new("1.png")).is_none());
    assert!(thumbs.get(Path::new("new.png")).is_some());
    assert_eq!(thumbs.generation, before + 1);

    // Held again under the same path is one entry, not two.
    thumbs.insert(
        PathBuf::from("new.png"),
        [texture("t"), texture("t"), texture("t")],
    );
    assert_eq!(thumbs.len(), MAX_THUMBS);
}
