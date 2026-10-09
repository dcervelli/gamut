use crate::gestures::spelled_here;
use std::path::Path;
use std::time::Duration;

use super::*;
use crate::image::region::Region;
use crate::image::{Stats, exif};
use crate::ui::Selection;
use crate::view::Fit;
use region::Framing;

const WINDOW: [f32; 2] = [1000.0, 700.0];
/// The same window with nothing taken out of it, for the tests that are
/// about stepping between files rather than about where the panels are.
const VIEWPORT: Viewport = Viewport::whole(WINDOW);

/// A gray PNG of the given size, written where the test can step onto it.
fn write_png(dir: &Path, name: &str, width: u32, height: u32) -> PathBuf {
    let path = dir.join(name);
    let pixels = vec![128u8; (width * height * 3) as usize];
    ::image::save_buffer(&path, &pixels, width, height, ::image::ColorType::Rgb8)
        .expect("the temporary directory is writable");
    path
}

/// The files are written under a directory of their own so that the tests,
/// which run alongside each other, cannot tread on each other's files.
fn opening(name: &str, files: &[(&str, u32, u32)]) -> (App, PathBuf) {
    let (dir, paths) = written(name, files);
    (open(paths.clone(), paths), dir)
}

/// The same, opened the way `gamut some-dir/` opens it: the directory is
/// what was named, and the files in it are only what it held at the time.
fn opening_directory(name: &str, files: &[(&str, u32, u32)]) -> (App, PathBuf) {
    let (dir, paths) = written(name, files);
    (open(paths, vec![dir.clone()]), dir)
}

fn written(name: &str, files: &[(&str, u32, u32)]) -> (PathBuf, Vec<PathBuf>) {
    let dir = std::env::temp_dir().join(format!("gamut-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("the temporary directory is writable");
    let paths = files
        .iter()
        .map(|&(name, width, height)| write_png(&dir, name, width, height))
        .collect();
    (dir, paths)
}

fn open(paths: Vec<PathBuf>, named: Vec<PathBuf>) -> App {
    let size = decode::probe(&paths[0]).expect("we just wrote it");
    App::new(
        paths,
        named,
        Some(Opening {
            index: 0,
            source: Source::Disk,
            size: size.map(|(w, h)| [w as f32, h as f32]),
        }),
        options(),
        StateFile::none(),
        threads(),
    )
}

/// The configuration read again puts its keys, its gestures and its map
/// link in force, and says so — or says what it got wrong. The panels it sets are left
/// as the window has them.
#[test]
fn a_configuration_read_again_rebinds_the_keys() {
    let mut app = opened_on_nothing();
    app.panels.side = Some(ui::side::Side::Histogram);
    let mut config = options().config;
    config
        .keys
        .bind("interface.help", vec![keymap::Chord::read("F1").unwrap()])
        .unwrap();
    config.show_histogram = false;
    config.open_map_link = "https://example.com/?{lat},{lng}".to_string();
    config.exiftool = "/opt/exiftool/exiftool".to_string();
    let _ = app.reconfigure(config, None);
    assert_eq!(app.keys.spelled("interface.help"), "F1");
    assert_eq!(app.open_map_link, "https://example.com/?{lat},{lng}");
    assert_eq!(app.exiftool.configured(), "/opt/exiftool/exiftool");
    assert_eq!(app.panels.side, Some(ui::side::Side::Histogram));
    let toast = app.toasts.showing().expect("the reload is said");
    assert_eq!(toast.message, RECONFIGURED);

    let _ = app.reconfigure(options().config, Some("Configuration line 3: no".into()));
    assert_eq!(app.keys.spelled("interface.help"), "?, /");
    assert_eq!(app.open_map_link, crate::settings::OPEN_MAP_LINK);
    assert_eq!(app.exiftool.configured(), "exiftool");
    let toast = app.toasts.showing().expect("the problem is said");
    assert_eq!(toast.message, "Configuration line 3: no");
}

/// The Tags tab reads the file on screen when it comes up, and not again
/// for the same file as it stands; it reads each file arriving while it is
/// up, and none once it is left.
#[test]
fn the_tags_tab_reads_each_file_once_while_it_is_up() {
    let (mut app, dir) = app_over("tags-tab", &[("a.png", 8, 8), ("b.png", 8, 8)]);
    app.headless = Some(WINDOW);
    // Found where nothing is: a run started fails at once, and its answer
    // goes nowhere, so each stays waited for.
    app.exiftool = exiftool::Program::at(Path::new("/nonexistent/exiftool"));
    let _ = app.press(ui::Control::Info);
    assert_eq!(app.panels.side, Some(ui::side::Side::Info));
    assert_eq!(app.tags.asked(), 0, "the panel opens on the facts");

    let _ = app.press(ui::Control::InfoTab(ui::tags::Tab::Tags));
    assert_eq!(app.tags.asked(), 1);
    let _ = app.press(ui::Control::InfoTab(ui::tags::Tab::Facts));
    let _ = app.press(ui::Control::InfoTab(ui::tags::Tab::Tags));
    assert_eq!(app.tags.asked(), 1, "already on its way");

    let _ = app.step(true);
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.tags.asked(), 2, "the file arriving is read");

    let _ = app.press(ui::Control::InfoTab(ui::tags::Tab::Facts));
    let _ = app.step(false);
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.tags.asked(), 2, "left, the tab reads nothing");

    // Taken down and put up again on the tab, it reads the file on screen.
    let _ = app.press(ui::Control::InfoTab(ui::tags::Tab::Tags));
    assert_eq!(app.tags.asked(), 3);
    let _ = app.press(ui::Control::Info);
    let _ = app.press(ui::Control::Info);
    assert_eq!(app.tags.asked(), 3, "the same file, still on its way");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The application as `gamut` alone opens it: no list, and nothing
/// asked for.
fn opened_on_nothing() -> App {
    App::new(
        Vec::new(),
        Vec::new(),
        None,
        options(),
        StateFile::none(),
        threads(),
    )
}

fn options() -> Options {
    Options {
        overrides: decode::Overrides::default(),
        startup: Startup::default(),
        hdr: HdrPreference::default(),
        // The panels as the tests were written against, rather than as
        // the configuration's defaults have them.
        config: Config {
            show_ui: true,
            show_minimap: false,
            show_filmstrip: false,
            show_histogram: false,
            show_info: false,
            log_counts: false,
            browse_folder: true,
            open_map_link: crate::settings::OPEN_MAP_LINK.to_string(),
            exiftool: "exiftool".to_string(),
            keys: keymap::Keymap::table(),
            gestures: Gestures::table(),
        },
        upscale: Upscale::default(),
        size: None,
        paused: false,
    }
}

/// The other threads, each detached: a test has no event loop for them
/// to reach.
fn threads() -> Threads {
    Threads {
        loader: Loader::detached(),
        wake: Arc::new(|_| true),
        monitors: None,
        thumbnailer: Thumbnailer::detached(),
        picker: Arc::new(|_| {}),
        folder: Arc::new(|_| {}),
        arranged: Arc::new(|_| {}),
        measured: Arc::new(|_| {}),
        tags: Arc::new(|_| {}),
    }
}

/// As [`opening`], with the application's own opening request answered:
/// the state the tests about later behavior want to start from.
fn app_over(name: &str, files: &[(&str, u32, u32)]) -> (App, PathBuf) {
    let (mut app, dir) = opening(name, files);
    answer(&mut app, Reload::Fresh);
    (app, dir)
}

/// Cuts a file off part way through its pixel data: it still says what
/// format it is and how large, so the header check passes, and only the
/// decode fails. That is the case start-up cannot catch up front, and the
/// reason the first file is asked for as a walk.
///
/// Both halves are asserted here rather than assumed, so that a change in
/// what the header check reads fails loudly instead of quietly leaving
/// the tests below testing nothing.
fn corrupt(path: &Path) {
    let length = std::fs::metadata(path).expect("we just wrote it").len();
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("we just wrote it")
        .set_len(length / 2)
        .expect("the file is writable");
    assert!(
        decode::probe(path).is_ok_and(|size| size.is_some()),
        "the header has to survive, or this is not the case being tested"
    );
    assert!(
        decode::load(path, decode::Overrides::default()).is_err(),
        "the pixels have to be beyond saving, or this is not the case being tested"
    );
}

/// The round trip a request makes through the loader, made here instead:
/// a test has no event loop to carry one. Reads whatever the application
/// last asked for, under the generation it asked for it with, so that the
/// staleness check sees exactly what it would in the running program.
fn answer(app: &mut App, mode: Reload) {
    let pending = app.files.pending().expect("a request is in flight");
    let (generation, index, asked) = (pending.generation, pending.index, pending.page);
    let path = app.files.path(index).to_path_buf();
    let watch = Watch::new(&path);
    let sequence = decode::sequence(&path).expect("the header reads");
    let page = match (asked, sequence) {
        (Some(page), _) => page,
        (None, Sequence::Pages { default, .. }) => default,
        (None, _) => 0,
    };
    // Asked for in the rendering the application prefers, as `App::send`
    // fills every request in with.
    let asked_rendering = app.rendering;
    let decoded = crate::loader::decode_rendering(&path, app.overrides, asked, asked_rendering);
    let format = decode::reader(&path).unwrap_or_default();
    let outcome = decoded.map(|(image, _, rendering, camera_jpeg)| Ready {
        stats: Stats::scan(&image),
        exif: exif::Exif::read(&path),
        image,
        gpu: None,
        sequence,
        page,
        rendering,
        camera_jpeg,
        format,
    });
    let _ = app.deliver(Decoded {
        generation,
        file: Opened {
            index,
            path,
            mode,
            rendering: asked_rendering,
            watch,
        },
        outcome,
    });
}

/// One file named alone, answered: the folder beside it unread.
fn alone_in(name: &str, files: &[(&str, u32, u32)], at: usize) -> (App, PathBuf, Vec<PathBuf>) {
    let (dir, paths) = written(name, files);
    let mut app = open(vec![paths[at].clone()], vec![paths[at].clone()]);
    answer(&mut app, Reload::Fresh);
    (app, dir, paths)
}

/// The folder read that the application has just started, read here
/// instead: its thread delivers to nothing in a test.
fn read_beside(app: &mut App, file: &Path) {
    assert!(
        matches!(app.beside.folder, Folder::Reading { .. }),
        "the folder is being read"
    );
    let listed = folder::read(file, app.filmstrip.order().sort, &Default::default());
    let _ = app.folder_read(listed);
}

/// A single file named alone is one file until a step asks for more;
/// then its folder is read, takes its place among the names, and the
/// step lands on the file after it.
#[test]
fn a_single_file_steps_on_into_its_folder() {
    let (mut app, dir, paths) = alone_in(
        "folder-step",
        &[("a.png", 8, 8), ("b.png", 8, 8), ("c.png", 8, 8)],
        1,
    );
    assert_eq!(app.files.len(), 1);
    assert!(app.beside.folder.unread());
    assert!(app.conditions().several_files, "the keys are live");

    let _ = app.step(true);
    assert!(
        app.files.is_idle(),
        "nothing asked for until the folder is in"
    );
    read_beside(&mut app, &paths[1]);
    assert_eq!(app.files.paths(), paths.as_slice());
    let pending = app.files.pending().expect("the next file is asked for");
    assert_eq!(app.files.path(pending.index), paths[2]);
    assert_eq!(
        app.named,
        std::slice::from_ref(&dir),
        "the folder is what is named now"
    );
    assert!(matches!(app.beside.folder, Folder::Closed));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Under a sort that needs more than the names, the read glimpses what
/// it needs of every file, and the step lands on the file that really
/// comes next in that order.
#[test]
fn a_folder_read_steps_in_the_order_in_force() {
    let (mut app, dir, paths) = alone_in(
        "folder-order",
        &[("a.png", 4, 4), ("b.png", 256, 256), ("c.png", 64, 64)],
        2,
    );
    let bytes = |path: &PathBuf| std::fs::metadata(path).unwrap().len();
    assert!(
        bytes(&paths[0]) < bytes(&paths[2]) && bytes(&paths[2]) < bytes(&paths[1]),
        "the sizes run a, c, b, or this is not the case being tested"
    );
    app.filmstrip.set_order(ui::filmstrip::Order {
        sort: ui::filmstrip::Sort::Size,
        direction: ui::filmstrip::Direction::Ascending,
    });

    let _ = app.step(true);
    read_beside(&mut app, &paths[2]);
    let pending = app.files.pending().expect("the next file is asked for");
    assert_eq!(
        app.files.path(pending.index),
        paths[1],
        "after c by size is b, where by name it would be a"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A folder holding nothing but the file says so, and the file stays
/// the only one; a step after that has nothing to read.
#[test]
fn a_folder_of_one_says_so() {
    let (mut app, dir, paths) = alone_in("folder-one", &[("only.png", 8, 8)], 0);
    let _ = app.step(false);
    read_beside(&mut app, &paths[0]);
    assert_eq!(app.files.len(), 1);
    assert!(app.files.is_idle());
    let said = app.showing_toast().expect("a message").message;
    assert!(said.starts_with("No other images in "), "{said}");

    let _ = app.step(true);
    assert!(
        matches!(app.beside.folder, Folder::Closed),
        "read once only"
    );
    let said = app.showing_toast().expect("a message").message;
    assert_eq!(said, "No other files to step to");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A folder read that arrives under a read of a file waits for it,
/// the list being rebuilt between reads only.
#[test]
fn a_folder_read_waits_for_the_file_in_flight() {
    let (mut app, dir, paths) = alone_in("folder-wait", &[("a.png", 8, 8), ("b.png", 8, 8)], 0);
    let _ = app.step(true);
    let reload = app.files.reload().expect("nothing in flight");
    let _ = app.send(reload);
    read_beside(&mut app, &paths[0]);
    assert_eq!(app.files.len(), 1, "not under a read");
    assert!(app.beside.landed.is_some());

    answer(&mut app, Reload::InPlace);
    assert_eq!(app.files.len(), 2);
    let pending = app.files.pending().expect("the step it was read for");
    assert_eq!(app.files.path(pending.index), paths[1]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The file list's toggle with one file reads the folder, and puts
/// the list up once it is in, whatever the toggle stood at.
#[test]
fn the_file_list_reads_the_folder_and_comes_up() {
    let (mut app, dir, paths) = alone_in("folder-list", &[("a.png", 8, 8), ("b.png", 8, 8)], 0);
    assert!(!app.panels.show_filmstrip);
    let _ = app.act(ui::Command::Press(ui::Control::Filmstrip));
    assert!(!app.panels.show_filmstrip, "not until the folder is in");
    read_beside(&mut app, &paths[0]);
    assert!(app.panels.show_filmstrip);
    assert!(app.filmstrip_showing());
    assert!(app.files.is_idle(), "the list, and no step");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The last picture's folder outlasts it: the empty window it leaves
/// offers the folder while it still holds images, and pressing the
/// offer opens them.
#[test]
fn the_empty_window_offers_the_last_folder() {
    let (mut app, dir, _) = alone_in("folder-last", &[("a.png", 8, 8), ("b.png", 8, 8)], 0);
    let whole = std::path::absolute(&dir).unwrap();
    assert_eq!(app.last_folder.as_deref(), Some(whole.as_path()));

    // The file taken off the list, the window empty, the other image
    // still in the folder.
    let _ = app.act(ui::Command::Press(ui::Control::Remove));
    assert!(app.is_empty());
    assert_eq!(app.offered_folder.as_deref(), Some(whole.as_path()));
    assert_eq!(
        app.frame_input([800.0, 600.0], 1.0).folder.as_deref(),
        Some(folder::name(&dir).as_str())
    );

    let _ = app.act(ui::Command::Press(ui::Control::OpenLastFolder));
    assert_eq!(app.files.len(), 2);
    assert!(app.files.pending().is_some(), "the first of them asked for");

    let _ = std::fs::remove_dir_all(&dir);

    // A folder emptied along with the list is not offered.
    let (mut app, dir, paths) = alone_in("folder-emptied", &[("a.png", 8, 8)], 0);
    std::fs::remove_file(&paths[0]).unwrap();
    let _ = app.act(ui::Command::Press(ui::Control::Remove));
    assert!(app.is_empty());
    assert!(app.last_folder.is_some());
    assert!(app.offered_folder.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A directory opens on the first file in the order the list was last
/// left in, not the first by name: under an order that needs more than
/// the names, the window opens with nothing asked for, the list is read
/// for its order, and the first in it is asked for then. The list stands
/// in that order before any header has been read.
#[test]
fn a_directory_opens_in_the_order_last_left() {
    let (dir, paths) = written(
        "arranged",
        &[("a.png", 4, 4), ("b.png", 256, 256), ("c.png", 64, 64)],
    );
    let order = ui::filmstrip::Order {
        sort: ui::filmstrip::Sort::Size,
        direction: ui::filmstrip::Direction::Descending,
    };
    let size = decode::probe(&paths[0]).unwrap();
    let mut app = App::new(
        paths.clone(),
        vec![dir.clone()],
        Some(Opening {
            index: 0,
            source: Source::Disk,
            size: size.map(|(w, h)| [w as f32, h as f32]),
        }),
        options(),
        StateFile::holding(State {
            order,
            ..State::default()
        }),
        threads(),
    );
    assert!(
        app.files.is_idle(),
        "nothing asked for until the order is in"
    );
    assert!(!app.is_empty(), "no buttons while it is read for");
    assert_eq!(app.title(), crate::PROGRAM);
    assert!(app.window_due().is_some(), "the window waits for it");
    assert!(!app.sizing.to_next, "nor sized by the first picture yet");

    let read = arranging::read_now(app.arranging.next_job, paths.clone(), order.sort);
    let _ = app.arranged_read(read);
    assert!(app.window_due().is_none());
    assert_eq!(
        app.opening_size(),
        Some([256.0, 256.0]),
        "the window opens at the first file's size, not the first named"
    );
    let pending = app
        .files
        .pending()
        .expect("the first in the order asked for");
    assert_eq!(app.files.path(pending.index), paths[1]);
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.shown_path(), Some(paths[1].as_path()));
    assert_eq!(
        app.files.paths(),
        [paths[1].clone(), paths[2].clone(), paths[0].clone()],
        "the order holds"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Files chosen in the window join the list in its order, and the
/// first of them in that order is the one shown.
#[test]
fn files_chosen_open_on_the_first_in_the_order() {
    let (mut app, dir) = app_over("arranged-on", &[("x.png", 8, 8)]);
    let (more, paths) = written(
        "arranged-more",
        &[("a.png", 4, 4), ("b.png", 256, 256), ("c.png", 64, 64)],
    );
    let order = ui::filmstrip::Order {
        sort: ui::filmstrip::Sort::Size,
        direction: ui::filmstrip::Direction::Descending,
    };
    app.filmstrip.set_order(order);
    let _ = app.apply_order();

    app.open_named(vec![more.clone()]);
    assert_eq!(app.files.len(), 1, "not on the list until read for");
    let read = arranging::read_now(app.arranging.next_job, paths.clone(), order.sort);
    let _ = app.arranged_read(read);
    assert_eq!(app.files.len(), 4);
    let pending = app.files.pending().expect("the first of them asked for");
    assert_eq!(app.files.path(pending.index), paths[1], "the largest");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&more);
}

/// Only one file named alone has a folder to step into: a directory,
/// or several files, are the list.
#[test]
fn only_a_file_named_alone_has_a_folder() {
    let (app, dir) = opening_directory("folder-dir", &[("a.png", 8, 8)]);
    assert!(matches!(app.beside.folder, Folder::Closed));
    let _ = std::fs::remove_dir_all(&dir);
    let (app, dir) = opening("folder-two", &[("a.png", 8, 8), ("b.png", 8, 8)]);
    assert!(matches!(app.beside.folder, Folder::Closed));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A row of the chooser asks for the file it names, wherever the list
/// has put it, and a row naming the file already on screen asks for
/// nothing; and a list read again is a list the chooser reads again.
#[test]
fn choosing_a_row_asks_for_its_file() {
    use crate::ui::Control;

    let (mut app, dir) = app_over(
        "choose",
        &[("a.png", 8, 8), ("b.png", 8, 8), ("c.png", 8, 8)],
    );
    assert!(app.files.is_idle());
    app.chooser.follow(&app.files);
    app.chooser.open(app.files.index());
    let _ = app.act(ui::Command::Press(Control::Choose(0)));
    assert!(
        app.files.is_idle(),
        "the file on screen is not asked for again"
    );

    let _ = app.act(ui::Command::Press(Control::Choose(2)));
    let pending = app.files.pending().expect("the third file is asked for");
    assert_eq!(pending.index, 2);
    assert_eq!(app.files.path(2).file_name().unwrap(), "c.png");
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.index(), 2);

    // A row past the list asks for nothing rather than panicking.
    let _ = app.act(ui::Command::Press(Control::Choose(99)));
    assert!(app.files.is_idle());

    // The list rebuilt under the popup: the chooser sees the new file,
    // and a row still resolves to its file by name.
    write_png(&dir, "d.png", 8, 8);
    assert!(
        app.files
            .relist(crate::listing::relist(std::slice::from_ref(&dir)))
    );
    app.list_changed();
    app.chooser.follow(&app.files);
    let input = app.chooser.input(&app.thumbs, app.files.shown_path());
    assert_eq!(input.rows.len(), 4);
    assert_eq!(input.current, Some(2));
    assert_eq!(
        app.chooser
            .path_at(3)
            .map(|p| p.file_name().unwrap().to_owned())
            .as_deref(),
        Some(std::ffi::OsStr::new("d.png"))
    );

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// A program opened on nothing is empty rather than failed: it has no
/// file to name, and every key about the file is a key that does
/// nothing, rather than one that reaches for a file that is not there.
#[test]
fn opened_on_nothing_the_window_is_empty_and_the_file_keys_are_dead() {
    use crate::ui::Naming;
    use input::Action;

    let mut app = opened_on_nothing();
    assert!(app.is_empty());
    assert!(
        !app.showed_nothing(),
        "nothing was asked for, so nothing failed"
    );
    assert_eq!(app.title(), crate::PROGRAM);
    assert_eq!(app.files.len(), 0);
    assert!(app.reading().is_none());
    assert_eq!(Effect::Nothing, app.poll_file());
    assert_eq!(Effect::Nothing, app.poll_directories());

    for action in [
        Action::CopyName,
        Action::CopyPath,
        Action::CopyUri,
        Action::CopyImage,
        Action::CopyMetadata,
        Action::NextFile,
        Action::PreviousFile,
        Action::Rename,
        Action::Delete,
        Action::NextFrame,
        Action::TogglePlay,
        Action::ZoomIn,
        Action::CycleFit,
    ] {
        let _ = app.perform(action);
        assert!(app.is_empty(), "{action:?} changes nothing");
        assert!(app.renaming.is_none());
        assert!(app.files.is_idle());
    }
    let namer = app.namer();
    assert!(
        namer
            .tooltip(ui::Tip::Name)
            .is_some_and(|tip| tip.title == [""])
    );
    assert_eq!(
        namer
            .tooltip(ui::Tip::Control(ui::Control::Copy))
            .expect("a reason")
            .title,
        [ui::tooltip::NOTHING_OPEN]
    );
}

/// Files sent before there is a window — Finder's, launching the
/// program — have it open at the first one's size, as a command line
/// naming them would, rather than open empty and be sized again.
#[test]
fn files_sent_before_the_window_size_it() {
    let (dir, paths) = written("sent", &[("a.png", 640, 320), ("b.png", 8, 16)]);
    let mut app = opened_on_nothing();
    assert!(app.sizing.to_next);
    assert_eq!(app.opening_size(), None);

    app.open_named(paths.clone());
    assert_eq!(app.opening_size(), Some([640.0, 320.0]));
    assert_eq!(
        app.sizing.sized_for,
        Some([640.0, 320.0]),
        "opened at its size already"
    );
    answer(&mut app, Reload::Fresh);
    assert!(!app.sizing.to_next, "spent on the arrival");
    assert_eq!(app.sizing.sized_for, None);

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// One file Finder sends to an empty window steps on into its folder,
/// as one named alone on the command line does; several are the list.
#[test]
fn a_single_file_sent_steps_on_into_its_folder() {
    let (dir, paths) = written(
        "sent-alone",
        &[("a.png", 8, 8), ("b.png", 8, 8), ("c.png", 8, 8)],
    );
    let mut app = opened_on_nothing();
    app.open_sent(vec![paths[1].clone()]);
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.len(), 1);
    assert!(app.beside.folder.unread());
    assert!(app.conditions().several_files, "the keys are live");

    let _ = app.step(true);
    read_beside(&mut app, &paths[1]);
    assert_eq!(app.files.paths(), paths.as_slice());
    let pending = app.files.pending().expect("the next file is asked for");
    assert_eq!(app.files.path(pending.index), paths[2]);

    let mut app = opened_on_nothing();
    app.open_sent(paths[..2].to_vec());
    assert!(matches!(app.beside.folder, Folder::Closed));
    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// What the dialog chose is opened as a command line naming it beside
/// the rest would be: a folder for the images in it, the newcomers
/// joining the end of the list and the first of them asked for as a
/// walk; the picture up stays until it arrives, and comes back as it
/// was left. A choice that fails leaves the window as it was and says
/// so, rather than leaving.
#[test]
fn what_the_dialog_chose_joins_the_list() {
    use input::Action;

    let (dir, paths) = written("chosen", &[("a.png", 16, 8), ("b.png", 8, 16)]);
    let mut app = opened_on_nothing();

    // Dismissed: nothing changes.
    app.picking = true;
    let _ = app.picked(Picked { outcome: Ok(None) });
    assert!(!app.picking);
    assert!(app.is_empty());
    assert!(app.toasts.showing().is_none());

    // The dialog could not be had: said, and the window stays empty.
    let _ = app.picked(Picked {
        outcome: Err(anyhow::anyhow!("no portal")),
    });
    assert!(app.is_empty());
    assert!(app.toasts.showing().is_some());
    app.toasts.dismiss();

    // One file: the list is that file, on its way.
    let _ = app.picked(Picked {
        outcome: Ok(Some(vec![paths[1].clone()])),
    });
    assert!(!app.is_empty(), "a read is in flight");
    assert_eq!(app.files.len(), 1);
    answer(&mut app, Reload::Fresh);
    assert!(app.current.is_some());
    assert_eq!(app.files.shown_path(), Some(paths[1].as_path()));
    assert!(!app.showed_nothing());
    assert!(!app.from_command_line);

    // A zoom to remember it by.
    let _ = app.perform(Action::ZoomTo(4.0));
    let zoomed = app.view.zoom(app.image_size(), app.viewport());
    let zoom = |app: &App| app.view.zoom(app.image_size(), app.viewport());

    // A folder chosen while a picture is up: the images in it join the
    // end of the list — the one already there not twice — the folder
    // is watched, the first newcomer is on its way, and the picture
    // stays until it arrives, kept as it was left.
    app.open_named(vec![dir.clone()]);
    assert!(app.current.is_some());
    assert_eq!(app.files.len(), 2);
    assert_eq!(app.files.path(1), paths[0]);
    assert_eq!(app.named, vec![paths[1].clone(), dir.clone()]);
    assert_eq!(app.directories.len(), 1, "the folder is watched");
    assert_eq!(app.files.pending().map(|pending| pending.index), Some(1));
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.shown_path(), Some(paths[0].as_path()));
    assert!(app.kept.left(&paths[1]).is_some());
    assert_ne!(zoom(&app), zoomed, "a new shape, fitted afresh");

    // The first again, through the dialog: nothing new to add, so it
    // is gone to by name, and comes back as it was left.
    app.open_named(vec![paths[1].clone()]);
    assert_eq!(app.files.len(), 2);
    assert_eq!(app.named.len(), 2, "named once already");
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.shown_path(), Some(paths[1].as_path()));
    assert_eq!(zoom(&app), zoomed);

    // The file on screen chosen: nothing to ask for.
    app.open_named(vec![paths[1].clone()]);
    assert!(app.files.is_idle());

    // A folder with no images in it is refused before anything moves.
    let empty = dir.join("empty");
    std::fs::create_dir_all(&empty).expect("the temporary directory is writable");
    app.open_named(vec![empty]);
    assert!(app.current.is_some());
    assert_eq!(app.files.len(), 2);
    assert!(app.toasts.showing().is_some());
    app.toasts.dismiss();

    // A file that will not read: it joins the list, the walk over it
    // fails, and the picture stays up with the reason under it.
    let broken = dir.join("broken.png");
    std::fs::write(&broken, b"not a png at all").expect("the file is writable");
    app.open_named(vec![broken]);
    assert_eq!(app.files.len(), 3);
    answer(&mut app, Reload::Fresh);
    assert!(app.current.is_some());
    assert_eq!(app.files.shown_path(), Some(paths[1].as_path()));
    assert!(
        app.toasts
            .showing()
            .is_some_and(|toast| toast.message.starts_with("Could not read broken.png")),
    );

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// The readings the pass makes on every frame owe a frame only when
/// they change: said again unchanged, they must not ask for another,
/// or an idle window would draw itself over and over.
#[test]
fn a_reading_said_again_unchanged_owes_no_frame() {
    let (mut app, dir) = app_over("readings", &[("a.png", 8, 8)]);
    assert_eq!(Effect::Redraw, app.act(ui::Command::OverImage(true)));
    assert_eq!(Effect::Nothing, app.act(ui::Command::OverImage(true)));
    assert_eq!(Effect::Redraw, app.act(ui::Command::OverImage(false)));
    assert_eq!(Effect::Nothing, app.act(ui::Command::OverGrip(None)));
    assert_eq!(
        Effect::Redraw,
        app.act(ui::Command::OverGrip(Some(
            crate::image::region::Grip::Inside
        )))
    );
    assert_eq!(
        Effect::Redraw,
        app.act(ui::Command::Press(ui::Control::Grid))
    );
    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// A key's pan is a move: the view is where it is going at once, and
/// what is on screen gets there over [`crate::motion::DURATION`]. A
/// single pixel's is not, and lands as it is pressed.
#[test]
fn a_keyboard_pan_moves_and_a_single_pixel_lands() {
    use input::{Action, Direction, PanStep};

    let (mut app, dir) = app_over("motion", &[("a.png", 64, 48)]);
    let (image, viewport) = (app.image_size(), app.viewport());
    app.view
        .set_zoom_at(4.0, viewport.center(), image, viewport);
    let before = app.view.position(image, viewport);

    let _ = app.perform(Action::Pan(Direction::Right, PanStep::Coarse));
    assert!(app.motion.is_some());
    let target = app.view.position(image, viewport);
    assert!(target.u[0] > before.u[0]);
    // Just begun: on screen it has barely left where it was.
    let now = Instant::now();
    let shown = app.view_at(now).position(image, viewport);
    assert!(shown.u[0] < target.u[0]);
    // Landed, and where the view says.
    let landed = app.view_at(now + crate::motion::DURATION);
    assert_eq!(landed.position(image, viewport), target);

    app.motion = None;
    let _ = app.perform(Action::Pan(Direction::Left, PanStep::Fine));
    assert!(app.motion.is_none());

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// Escape puts away whatever is up, topmost first, and leaves only when
/// there is nothing left to put away. `q` is not held up by a message:
/// a copy is often followed straight away by it.
#[test]
fn escape_puts_things_away_before_it_quits() {
    use input::{Action, Effect};

    let (mut app, dir) = app_over("dismiss", &[("a.png", 64, 48)]);
    let raise = |app: &mut App| {
        app.toasts.show(
            Instant::now(),
            "Copied file path.".to_string(),
            Level::Message,
            toast::LINGER,
        );
    };

    // Each press takes off one thing. A menu would outrank the message,
    // but the menus are egui's and there is no window here to open one
    // in; `close_menus` answers for it.
    raise(&mut app);
    assert!(!app.close_menus());
    assert!(app.toasts.showing().is_some());

    assert_eq!(app.perform(Action::Dismiss), Effect::Redraw);
    assert!(app.toasts.showing().is_none());
    assert_eq!(app.perform(Action::Dismiss), Effect::Quit);

    // `q` leaves whether or not there is a message to read.
    raise(&mut app);
    assert_eq!(app.perform(Action::Quit), Effect::Quit);

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// `t` does nothing under a false color, where the curve does nothing:
/// a curve changed there would only show once the ramp came off, from a
/// press made long before. The ramp put back, the key is a key again.
#[test]
fn the_curve_key_is_dead_under_a_false_color() {
    use crate::image::display::{Colormap, Display, ToneMap};
    use crate::ui::{Command, Control};
    use input::Action;

    // A gray file, since a false color is a reading of one channel.
    let (dir, paths) = written("false-color", &[("gray.png", 8, 8)]);
    ::image::save_buffer(&paths[0], &[128u8; 64], 8, 8, ::image::ColorType::L8)
        .expect("the temporary directory is writable");
    let mut app = open(paths.clone(), paths);
    answer(&mut app, Reload::Fresh);
    fn display(app: &App) -> &Display {
        &app.current.as_ref().expect("a picture is up").display
    }
    assert!(
        app.current
            .as_ref()
            .is_some_and(|current| current.image.is_gray())
    );
    assert_eq!(display(&app).tone_map(), ToneMap::None);

    assert_eq!(app.perform(Action::CycleColormap), Effect::Redraw);
    assert_eq!(display(&app).colormap(), Colormap::Viridis);
    assert_eq!(app.perform(Action::CycleToneMap), Effect::Nothing);
    assert_eq!(display(&app).tone_map(), ToneMap::None);
    let _ = app.act(Command::Press(Control::Curve(1)));
    assert_eq!(display(&app).tone_map(), ToneMap::None);

    for _ in 1..Colormap::ALL.len() {
        let _ = app.perform(Action::CycleColormap);
    }
    assert_eq!(display(&app).colormap(), Colormap::Gray);
    assert_eq!(app.perform(Action::CycleToneMap), Effect::Redraw);
    assert_eq!(display(&app).tone_map(), ToneMap::Neutral);

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// A false color is a reading of one channel, and a color image's three
/// are colors already: `r` refuses one, and so does the panel's swatch,
/// or a press of it would put a map on the state that the picture, the
/// bar and the readout all ignore.
#[test]
fn a_false_color_button_is_dead_on_a_color_image() {
    use crate::image::display::Colormap;
    use crate::ui::{Command, Control};
    use input::{Action, Effect};

    let (mut app, dir) = app_over("ramp", &[("a.png", 8, 8)]);
    assert!(
        app.current
            .as_ref()
            .is_some_and(|current| !current.image.is_gray())
    );
    let _ = app.act(Command::Press(Control::Ramp(1)));
    assert_eq!(
        app.current
            .as_ref()
            .expect("a picture is up")
            .display
            .colormap(),
        Colormap::Gray
    );
    assert_eq!(app.perform(Action::CycleColormap), Effect::Nothing);

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// The keys step the handles, no further than the plot goes: on a
/// graded file the plot is 0..1, so the black point cannot be stepped
/// below 0 nor the white point above 1, and the exposure is left alone
/// by both.
#[test]
fn the_window_keys_step_the_handles_within_the_plot() {
    use input::{Action, Effect};

    let (mut app, dir) = app_over("handles", &[("a.png", 8, 8)]);
    fn display(app: &App) -> &crate::image::display::Display {
        &app.current.as_ref().expect("a picture is up").display
    }
    assert_eq!(display(&app).displayed_bounds(), (0.0, 1.0));

    // Black is at the floor already, so a press downward is no press.
    assert_eq!(app.perform(Action::StepBlack(-0.05)), Effect::Nothing);
    assert_eq!(display(&app).displayed_bounds(), (0.0, 1.0));
    // A twentieth of the plot, which is on the file's sRGB curve.
    assert_eq!(app.perform(Action::StepBlack(0.05)), Effect::Redraw);
    let (black, white) = display(&app).displayed_bounds();
    assert!((crate::image::Transfer::Srgb.to_encoded(black) - 0.05).abs() < 1e-5);
    assert!((white - 1.0).abs() < 1e-6);

    // White is at the ceiling already, so a press upward is no press.
    assert_eq!(app.perform(Action::StepWhite(0.05)), Effect::Nothing);
    // And a press down moves it alone, a twentieth of the window along
    // the plot, the exposure untouched.
    assert_eq!(app.perform(Action::StepWhite(-0.05)), Effect::Redraw);
    assert_eq!(display(&app).exposure_stops(), 0.0);
    let (still_black, white) = display(&app).displayed_bounds();
    assert_eq!(still_black, black);
    let encoded = crate::image::Transfer::Srgb.to_encoded(white);
    assert!((encoded - 0.9525).abs() < 1e-4, "{encoded}");

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// The window row is every file's: on a graded file, whose window opens
/// at 0..1, *As stored* is what puts a hand-moved window back. It sets
/// the rule and nothing else, so an exposure on top of the window stays.
#[test]
fn as_stored_puts_a_hand_moved_window_back_on_a_graded_file() {
    use crate::image::display::AutoWindow;
    use crate::ui::{Command, Control};
    use input::{Action, Effect};

    let (mut app, dir) = app_over("stored", &[("a.png", 8, 8)]);
    fn display(app: &App) -> &crate::image::display::Display {
        &app.current.as_ref().expect("a picture is up").display
    }
    assert_eq!(app.perform(Action::StepBlack(0.05)), Effect::Redraw);
    assert_eq!(app.perform(Action::StepWhite(-0.05)), Effect::Redraw);
    assert_eq!(app.perform(Action::Exposure(0.5)), Effect::Redraw);
    assert_eq!(display(&app).auto(), AutoWindow::Manual);
    assert_ne!(display(&app).displayed_bounds(), (0.0, 1.0));

    let _ = app.act(Command::Press(Control::Window(0)));
    assert_eq!(display(&app).auto(), AutoWindow::Off);
    assert_eq!(
        (display(&app).window_low(), display(&app).window_high()),
        (0.0, 1.0)
    );
    assert_eq!(
        display(&app).exposure_stops(),
        0.5,
        "the rule, not the exposure"
    );

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// Escape is what brings the interface back, and it does that before it
/// takes off the message that said so: a window that dismissed its own
/// instructions and left the bars hidden would be disagreeing with what
/// it had just told the reader. `q` still leaves from under it.
#[test]
fn escape_brings_the_interface_back_before_it_quits() {
    use input::{Action, Effect};

    let (mut app, dir) = app_over("restore", &[("a.png", 64, 48)]);
    assert!(app.panels.show_ui);

    // Hidden, and the window has said once how to get it back.
    let _ = app.perform(Action::ToggleInterface);
    assert!(!app.panels.show_ui);
    assert!(app.said_how_to_restore);

    // Escape brings it back rather than quitting out from under it.
    assert_eq!(app.perform(Action::Dismiss), Effect::Redraw);
    assert!(app.panels.show_ui);
    // And with it back, Escape is the quit it always was.
    assert_eq!(app.perform(Action::Dismiss), Effect::Quit);

    // The key that closes the panels on its way says it too:
    // what it hides is the same thing, by the same route.
    app.said_how_to_restore = false;
    let _ = app.perform(Action::ToggleInterfaceAndPanels);
    assert!(app.said_how_to_restore);
    // And `q` leaves from under a hidden interface, as it always did.
    assert_eq!(app.perform(Action::Quit), Effect::Quit);

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// A region on screen takes the keys that move the picture: the arrows
/// move it a pixel — or its current handle, once one has been clicked or
/// dragged — `Ctrl` with one grows it, `Space` fits it, and `Esc` takes
/// it off after a message and before quitting. Stepping to another file
/// takes it off as well.
#[test]
fn a_region_takes_the_keys_that_move_the_picture() {
    use crate::image::region::{Grip, Side};
    use input::{Action, Direction, Effect, PanStep};
    use ui::{Command, Grab};
    use winit::event::ElementState::Pressed;
    use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};

    let (mut app, dir) = app_over("region", &[("a.png", 64, 48), ("b.png", 64, 48)]);
    assert_eq!(app.marking.selection, Selection::Off);
    assert_eq!(app.perform(Action::ToggleRegion), Effect::Redraw);
    assert_eq!(app.marking.selection, Selection::Armed);

    // Drawn as a drag draws it: from the press to wherever the hand is,
    // every pixel touched taken in.
    let draw = |app: &mut App| {
        let _ = app.act(Command::Grab {
            grab: Grab::New,
            at: [10.2, 5.5],
        });
        let _ = app.act(Command::Pull([20.9, 15.1]));
        let _ = app.act(Command::Release);
    };
    draw(&mut app);
    let region = Region {
        x: 10,
        y: 5,
        width: 11,
        height: 11,
    };
    assert_eq!(app.marking.selection, Selection::Shown(region));
    assert!(app.marking.grabbed().is_none());
    // The words are written while the pointer is on the region, and not
    // otherwise: no clock takes them off.
    assert!(!app.marking.over());
    app.marking.grip = Some(Grip::Inside);
    assert!(app.marking.over());
    app.marking.grip = None;

    // The arrows move the region and leave the view alone, at once: a
    // fresh region's current handle is its middle.
    assert_eq!(app.marking.handle, Grip::Middle);
    let (image, viewport) = (app.image_size(), app.viewport());
    let view = app.view.position(image, viewport);
    let right = Key::Named(NamedKey::ArrowRight);
    let at = PhysicalKey::Code(KeyCode::ArrowRight);
    assert_eq!(app.handle_key(&right, at, Pressed), Effect::Redraw);
    assert_eq!(
        app.marking.selection,
        Selection::Shown(Region { x: 11, ..region })
    );
    assert_eq!(app.view.position(image, viewport), view);
    assert!(app.motion.is_none());

    // A handle clicked is the current one, and they move that instead.
    // The pointer resting on another handle does not come into it.
    let _ = app.act(Command::Handle(Grip::Edge(Side::Right)));
    app.marking.grip = Some(Grip::Corner(Side::Left, Side::Top));
    assert_eq!(app.marking.handle, Grip::Edge(Side::Right));
    let _ = app.perform(Action::MoveRegion(Direction::Right));
    assert_eq!(
        app.marking.selection,
        Selection::Shown(Region {
            x: 11,
            width: 12,
            ..region
        })
    );
    // An arrow along that edge moves the whole region instead.
    let _ = app.perform(Action::MoveRegion(Direction::Down));
    assert_eq!(
        app.marking.selection,
        Selection::Shown(Region {
            x: 11,
            y: 6,
            width: 12,
            height: 11
        })
    );
    app.marking.grip = None;

    // A drag on a handle makes it current too, without moving it; a
    // move of the whole by its inside leaves the handle as it was; and
    // a hold on the middle handle brings the arrows back to the whole.
    let _ = app.act(Command::Grab {
        grab: Grab::Handle(Grip::Edge(Side::Top)),
        at: [16.0, 6.0],
    });
    let _ = app.act(Command::Release);
    assert_eq!(app.marking.handle, Grip::Edge(Side::Top));
    let _ = app.act(Command::Grab {
        grab: Grab::Handle(Grip::Inside),
        at: [16.0, 10.0],
    });
    let _ = app.act(Command::Release);
    assert_eq!(app.marking.handle, Grip::Edge(Side::Top));
    let _ = app.act(Command::Grab {
        grab: Grab::Handle(Grip::Middle),
        at: [16.0, 11.0],
    });
    let _ = app.act(Command::Release);
    assert_eq!(app.marking.handle, Grip::Middle);
    let _ = app.perform(Action::MoveRegion(Direction::Up));
    assert_eq!(
        app.marking.selection,
        Selection::Shown(Region {
            x: 11,
            y: 5,
            width: 12,
            height: 11
        })
    );
    // Grown back for the steps below, which read from here.
    let _ = app.perform(Action::MoveRegion(Direction::Down));

    // Shift with an arrow is not the region's: it pans the picture by
    // a pixel under it, as it does with no region up.
    let view = app.view.position(image, viewport);
    let _ = app.perform(Action::Pan(Direction::Down, PanStep::Fine));
    assert_eq!(
        app.marking.selection,
        Selection::Shown(Region {
            x: 11,
            y: 6,
            width: 12,
            height: 11
        })
    );
    assert_ne!(app.view.position(image, viewport), view);

    // Ctrl grows it that way.
    let _ = app.perform(Action::GrowRegion(Direction::Up));
    assert_eq!(
        app.marking.selection,
        Selection::Shown(Region {
            x: 11,
            y: 5,
            width: 12,
            height: 12
        })
    );
    // Ctrl+Shift shrinks it that way: Left brings the right edge in.
    let _ = app.perform(Action::ShrinkRegion(Direction::Left));
    assert_eq!(
        app.marking.selection,
        Selection::Shown(Region {
            x: 11,
            y: 5,
            width: 11,
            height: 12
        })
    );

    // Space frames the region first and the picture after: the region
    // fitted and filled — a zoom of its own rather than a fit the view
    // keeps — then the picture's two fits and its actual size, and
    // round again.
    assert_eq!(app.view.fit(), Some(Fit::Whole));
    assert_eq!(app.marking.framing, Framing::Region(Fit::Whole));
    let _ = app.perform(Action::CycleFit);
    assert_eq!(app.view.fit(), None);
    assert_eq!(app.marking.framing, Framing::Region(Fit::Fill));
    let _ = app.perform(Action::CycleFit);
    assert_eq!(app.view.fit(), None);
    assert_eq!(app.marking.framing, Framing::Picture(Fit::Whole));
    let _ = app.perform(Action::CycleFit);
    assert_eq!(app.view.fit(), Some(Fit::Whole));
    let _ = app.perform(Action::CycleFit);
    assert_eq!(app.view.fit(), Some(Fit::Fill));
    assert_eq!(app.marking.framing, Framing::Actual);
    let _ = app.perform(Action::CycleFit);
    assert_eq!(app.view.fit(), None);
    assert_eq!(app.marking.framing, Framing::Region(Fit::Whole));
    // A change to the region starts the cycle over at the region.
    let _ = app.perform(Action::CycleFit);
    assert_eq!(app.marking.framing, Framing::Region(Fit::Fill));
    let _ = app.perform(Action::MoveRegion(Direction::Left));
    assert_eq!(app.marking.framing, Framing::Region(Fit::Whole));

    // Escape takes it off after the message, and before quitting.
    app.toast("Copied region.", Level::Message);
    assert_eq!(app.perform(Action::Dismiss), Effect::Redraw);
    assert!(app.toasts.showing().is_none());
    assert!(app.marking.selection.is_on());
    assert_eq!(app.perform(Action::Dismiss), Effect::Redraw);
    assert_eq!(app.marking.selection, Selection::Off);
    assert!(!app.marking.over());
    assert_eq!(app.perform(Action::Dismiss), Effect::Quit);

    // The key with a region up takes it off too, and the arrows are the
    // view's again.
    let _ = app.perform(Action::ToggleRegion);
    draw(&mut app);
    assert!(app.marking.selection.region().is_some());
    // The copy of the picture says it takes the region while one is up.
    use crate::ui::Naming;
    let copy_image = |app: &App| {
        app.namer()
            .tooltip(ui::Tip::Control(ui::Control::Copies(
                ui::menu::Copies::Image,
            )))
            .map(|tooltip| tooltip.title)
    };
    assert_eq!(
        copy_image(&app),
        Some(vec![spelled_here("Copy the region as displayed (Ctrl+C)")])
    );
    let _ = app.perform(Action::ToggleRegion);
    assert_eq!(app.marking.selection, Selection::Off);
    assert_eq!(
        copy_image(&app),
        Some(vec![spelled_here("Copy the image as displayed (Ctrl+C)")])
    );
    let _ = app.perform(Action::Pan(Direction::Right, PanStep::Coarse));
    assert!(app.motion.is_some(), "a pan of the view is a move");
    // Without a region the region's own keys do nothing, and the arrow
    // held with Shift is the view's again.
    app.motion = None;
    assert_eq!(
        app.perform(Action::MoveRegion(Direction::Right)),
        Effect::Nothing
    );
    let left = Key::Named(NamedKey::ArrowLeft);
    let back = PhysicalKey::Code(KeyCode::ArrowLeft);
    app.pointer.modifiers = winit::keyboard::ModifiersState::SHIFT;
    assert_eq!(app.handle_key(&left, back, Pressed), Effect::Redraw);
    assert!(app.motion.is_some(), "the arrow pans");
    app.pointer.modifiers = winit::keyboard::ModifiersState::empty();

    // And a region is of the picture it was drawn on: stepping to
    // another file leaves it behind.
    app.motion = None;
    let _ = app.perform(Action::ToggleRegion);
    draw(&mut app);
    let _ = app.step(true);
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.index(), 1);
    assert_eq!(app.marking.selection, Selection::Off);

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// `Space` is answered on its way up, so that a drag while it is held
/// can draw a box to zoom to without the view moving first: a tap fits,
/// a hold with a box drawn under it does not, and the key's repeats are
/// nothing at all.
#[test]
fn space_fits_on_its_way_up_unless_a_box_was_drawn_under_it() {
    use input::{Action, Effect, FitKey};
    use ui::{Command, Grab};
    use winit::event::ElementState::{Pressed, Released};
    use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};

    let (mut app, dir) = app_over("space", &[("a.png", 64, 48)]);
    let space = Key::Named(NamedKey::Space);
    let at = PhysicalKey::Code(KeyCode::Space);
    assert_eq!(app.view.fit(), Some(Fit::Whole));

    // A tap: nothing moves on the way down — a frame for the pointer,
    // no more — and the fit comes on the way up. Held, the key repeats,
    // and a repeat is the same press still going.
    assert_eq!(app.handle_key(&space, at, Pressed), Effect::Redraw);
    assert_eq!(app.view.fit(), Some(Fit::Whole));
    assert!(app.motion.is_none());
    assert_eq!(
        app.pointer.fit_key,
        Some(FitKey {
            key: at,
            drawn: false
        })
    );
    assert_eq!(app.handle_key(&space, at, Pressed), Effect::Nothing);
    assert_eq!(
        app.pointer.fit_key,
        Some(FitKey {
            key: at,
            drawn: false
        })
    );
    assert_eq!(app.handle_key(&space, at, Released), Effect::Redraw);
    assert_eq!(app.view.fit(), Some(Fit::Fill));
    assert_eq!(app.pointer.fit_key, None);

    // Held with a box dragged out under it: the box is not a region,
    // the view goes to it as a move when the drag lets go, and letting
    // go of the key afterwards fits nothing.
    app.motion = None;
    let _ = app.handle_key(&space, at, Pressed);
    let _ = app.act(Command::Grab {
        grab: Grab::Zoom,
        at: [10.2, 5.5],
    });
    assert_eq!(
        app.pointer.fit_key,
        Some(FitKey {
            key: at,
            drawn: true
        })
    );
    let _ = app.act(Command::Pull([20.9, 15.1]));
    assert_eq!(
        app.marking.zoom_box(),
        Some(Region {
            x: 10,
            y: 5,
            width: 11,
            height: 11
        })
    );
    assert_eq!(app.marking.selection, Selection::Off);
    let _ = app.act(Command::Release);
    assert_eq!(app.marking.zoom_box(), None);
    assert!(app.marking.grabbed().is_none());
    assert!(app.motion.is_some(), "the zoom to the box is a move");
    assert_eq!(app.view.fit(), None);
    // Centered on the box — through whatever viewport the application
    // has without a window, which is what the move was made against.
    let (image, viewport) = (app.image_size(), app.viewport());
    let center = app.view.placement(image, viewport).image_point([
        viewport.x + viewport.width / 2.0,
        viewport.y + viewport.height / 2.0,
    ]);
    assert!(
        (center[0] - 15.5).abs() < 0.01 && (center[1] - 10.5).abs() < 0.01,
        "the box is centered: {center:?}"
    );
    assert_eq!(app.handle_key(&space, at, Released), Effect::Redraw);
    assert_eq!(app.view.fit(), None);
    assert_eq!(app.pointer.fit_key, None);

    // Escape drops a box part way through: the toolkit takes the drag
    // off the hand on the same key, and the release that follows finds
    // nothing to zoom to.
    let _ = app.handle_key(&space, at, Pressed);
    let _ = app.act(Command::Grab {
        grab: Grab::Zoom,
        at: [1.0, 1.0],
    });
    let _ = app.act(Command::Pull([30.0, 30.0]));
    assert!(app.marking.zoom_box().is_some());
    app.motion = None;
    let before = app.view.position(image, viewport);
    assert_eq!(app.perform(Action::Dismiss), Effect::Redraw);
    assert_eq!(app.marking.zoom_box(), None);
    assert!(app.marking.grabbed().is_none());
    let _ = app.act(Command::Release);
    assert!(app.motion.is_none());
    assert_eq!(app.view.position(image, viewport), before);
    assert_eq!(app.handle_key(&space, at, Released), Effect::Redraw);
    assert_eq!(app.view.position(image, viewport), before);

    // Another key's release is not the fit key's, whatever it says.
    let _ = app.handle_key(&space, at, Pressed);
    let other = PhysicalKey::Code(KeyCode::KeyA);
    assert_eq!(app.handle_key(&space, other, Released), Effect::Nothing);
    assert!(app.pointer.fit_key.is_some());
    assert_eq!(app.handle_key(&space, at, Released), Effect::Redraw);
    app.motion = None;

    // The window losing the keyboard lets go of the key: its release
    // is going somewhere else.
    let _ = app.handle_key(&space, at, Pressed);
    assert!(app.pointer.fit_key.is_some());
    app.keys_lost();
    assert_eq!(app.pointer.fit_key, None);

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// A zoom asked for by name works about the pointer while it is over the
/// picture — the detail under it stays under it — and about the middle
/// of the viewport when it is not: over a panel, or off the picture.
#[test]
fn a_named_zoom_works_about_the_pointer_over_the_picture() {
    use input::Action::{ZoomIn, ZoomTo};
    let (mut app, _dir) = app_over("anchor", &[("a.png", 400, 300)]);
    app.headless = Some(WINDOW);
    let (image, viewport) = (app.image_size(), app.viewport());
    let under = |app: &App, at: [f32; 2]| app.view.placement(image, viewport).image_point(at);
    let close = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() < 0.2 && (a[1] - b[1]).abs() < 0.2;

    // Over the picture: the pixel under the pointer is the anchor.
    let at = [300.0, 200.0];
    app.pointer.cursor = Some(at);
    app.pointer.over_image = true;
    assert!(app.pointer_pixel().is_some());
    let before = under(&app, at);
    let _ = app.perform(ZoomTo(8.0));
    assert_eq!(app.view.fit(), None);
    assert_eq!(app.view.zoom(image, viewport), 8.0);
    let after = under(&app, at);
    assert!(close(before, after), "{before:?} -> {after:?}");
    let _ = app.perform(ZoomIn);
    let stepped = under(&app, at);
    assert!(close(before, stepped), "{before:?} -> {stepped:?}");

    // Under a panel, the same pointer is not over the picture, and the
    // middle of the viewport is what stays put.
    app.pointer.over_image = false;
    let middle = viewport.center();
    let before = under(&app, middle);
    let _ = app.perform(ZoomTo(16.0));
    let after = under(&app, middle);
    assert!(close(before, after), "{before:?} -> {after:?}");

    // Asked for the zoom it is at — a double-click at actual size — the
    // detail under the pointer goes to the middle, as a move. At a zoom
    // the picture overflows the viewport at, so that it has room to.
    app.pointer.over_image = true;
    app.view.reset();
    app.view.set_zoom_at(4.0, middle, image, viewport);
    app.motion = None;
    let detail = under(&app, at);
    let _ = app.perform(ZoomTo(4.0));
    assert_eq!(app.view.zoom(image, viewport), 4.0);
    let centered = under(&app, middle);
    assert!(close(detail, centered), "{detail:?} -> {centered:?}");
    assert!(app.motion.is_some());
}

/// Stepping between frames of the same size is a comparison — the same
/// detail has to stay under the same pixels, or there is nothing to
/// compare.
#[test]
fn stepping_to_an_image_of_the_same_size_keeps_the_view() {
    let (mut app, dir) = app_over("same", &[("a.png", 64, 48), ("b.png", 64, 48)]);
    app.view
        .set_zoom_at(1.0, VIEWPORT.center(), app.image_size(), VIEWPORT);
    app.view
        .zoom_in(VIEWPORT.center(), app.image_size(), VIEWPORT);
    let zoom = app.view.zoom(app.image_size(), VIEWPORT);

    let _ = app.step(true);
    // Nothing has moved yet: the file has only been asked for.
    assert_eq!(app.files.index(), 0);

    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.index(), 1);
    assert_eq!(app.view.fit(), None);
    assert_eq!(app.view.zoom(app.image_size(), VIEWPORT), zoom);

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// A file coming back at the size of the one it is arriving beside is a
/// comparison, and it is made where the eye is: the pan and zoom carry
/// over from the picture leaving the screen, whatever this file was left
/// in the last time it was looked at.
#[test]
fn a_same_size_neighbor_takes_the_view_it_arrives_beside() {
    let (mut app, dir) = app_over("compared", &[("a.png", 64, 48), ("b.png", 64, 48)]);

    // b.png is left at 4x, so it has a view of its own to be put back.
    let _ = app.step(true);
    answer(&mut app, Reload::Fresh);
    app.view
        .set_zoom_at(4.0, VIEWPORT.center(), app.image_size(), VIEWPORT);

    // Back to a.png, and on to somewhere else in it.
    let _ = app.step(false);
    answer(&mut app, Reload::Fresh);
    app.view
        .set_zoom_at(2.0, VIEWPORT.center(), app.image_size(), VIEWPORT);
    let zoom = app.view.zoom(app.image_size(), VIEWPORT);

    // And on to b.png again: at a.png's zoom, not the 4x it was left in.
    let _ = app.step(true);
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.shown_path(), Some(dir.join("b.png").as_path()));
    assert_eq!(app.view.fit(), None);
    assert_eq!(app.view.zoom(app.image_size(), VIEWPORT), zoom);

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// Flipping between two pictures is how they are compared, so each of
/// them has to come back as it was left: its own pan and zoom, its own
/// window and exposure, its own false color.
#[test]
fn a_file_comes_back_as_it_was_left() {
    use crate::image::display::Colormap;

    let (mut app, dir) = app_over("kept", &[("a.png", 64, 48), ("b.png", 32, 16)]);
    app.view
        .set_zoom_at(4.0, VIEWPORT.center(), app.image_size(), VIEWPORT);
    let zoom = app.view.zoom(app.image_size(), VIEWPORT);
    let display = app.current.as_mut().expect("a.png is on screen");
    display.display.adjust_exposure(2.0);
    // As if the picture were gray: what is kept is the point here, not
    // what a color image refuses.
    assert!(display.display.cycle_colormap(true));
    let colormap = display.display.colormap();

    // Another size, so nothing carries over: b.png opens fitted and with
    // the display its own pixels ask for.
    let _ = app.step(true);
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.view.fit(), Some(Fit::Whole));
    let display = &app.current.as_ref().expect("b.png is on screen").display;
    assert_eq!(display.exposure_stops(), 0.0);
    assert_eq!(display.colormap(), Colormap::Gray);

    // And back, to everything a.png was left in.
    let _ = app.step(false);
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.shown_path(), Some(dir.join("a.png").as_path()));
    assert_eq!(app.view.fit(), None);
    assert_eq!(app.view.zoom(app.image_size(), VIEWPORT), zoom);
    let display = &app.current.as_ref().expect("a.png is on screen").display;
    assert_eq!(display.exposure_stops(), 2.0);
    assert_eq!(display.colormap(), colormap);

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// A turn is the picture's on screen and nothing else's: the size the
/// window reads is the turned one, a region turns with the pixels it
/// marks, and the turn is kept with the file, so that a file stepped
/// away from and back to is still turned while another file is not.
#[test]
fn a_turn_is_kept_per_file_and_put_back() {
    use input::Action::{TurnLeft, TurnRight};

    let (mut app, dir) = app_over("turned", &[("a.png", 64, 48), ("b.png", 32, 16)]);
    let region = Region {
        x: 4,
        y: 2,
        width: 10,
        height: 6,
    };
    app.marking.select(region);
    assert_eq!(app.perform(TurnRight), Effect::Redraw);
    assert_eq!(app.image_size(), [48.0, 64.0]);
    let one = Turn::NONE.clockwise();
    assert_eq!(app.current.as_ref().unwrap().turn, one);
    assert_eq!(
        app.marking.selection,
        Selection::Shown(region.turned(one, [64, 48]))
    );
    // Read through the turn: the turned picture's top left is the
    // stored picture's bottom left.
    let current = app.current.as_ref().unwrap();
    assert_eq!(
        current.sample(0, 0).map(|sample| sample.stored().to_vec()),
        current
            .image
            .sample(0, 47, None)
            .map(|sample| sample.stored().to_vec())
    );
    assert!(current.sample(48, 0).is_none(), "off the turned picture");

    // Another file arrives unturned.
    let _ = app.step(true);
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.image_size(), [32.0, 16.0]);
    assert_eq!(app.current.as_ref().unwrap().turn, Turn::NONE);

    // And the first comes back turned.
    let _ = app.step(false);
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.image_size(), [48.0, 64.0]);
    assert_eq!(app.perform(TurnLeft), Effect::Redraw);
    assert_eq!(app.image_size(), [64.0, 48.0]);
    assert_eq!(app.current.as_ref().unwrap().turn, Turn::NONE);

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// A file rewritten under the window is the same file being read again,
/// not a return to it: one that comes back a different size is a new
/// shape and is fitted afresh, rather than being put back into the view
/// it was being looked at in.
#[test]
fn a_reload_is_not_a_return() {
    let (mut app, dir) = app_over("reloaded", &[("a.png", 64, 48)]);
    app.view
        .set_zoom_at(4.0, VIEWPORT.center(), app.image_size(), VIEWPORT);
    assert_eq!(app.view.fit(), None);

    write_png(&dir, "a.png", 32, 16);
    let request = app.files.reload().expect("nothing else is being read");
    let _ = app.send(request);
    answer(&mut app, Reload::InPlace);

    assert_eq!(app.image_size(), [32.0, 16.0]);
    assert_eq!(app.view.fit(), Some(Fit::Whole), "a new shape to fit");

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// A directory named on the command line is a place to look, not a list
/// fixed when the window opened: an image written into it joins the walk,
/// and one taken out of it leaves.
#[test]
fn a_directory_is_read_again_when_what_is_in_it_changes() {
    let (mut app, dir) = opening_directory("relist", &[("a.png", 8, 8), ("b.png", 8, 8)]);
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.len(), 2);
    assert_eq!(
        Effect::Nothing,
        app.poll_directories(),
        "nothing has happened to it"
    );

    write_png(&dir, "c.png", 8, 8);
    assert_eq!(
        Effect::Nothing,
        app.poll_directories(),
        "the change has not settled yet"
    );
    assert_eq!(Effect::Redraw, app.poll_directories());
    assert_eq!(app.files.len(), 3);
    assert_eq!(app.files.path(2), dir.join("c.png"));
    assert_eq!(
        app.files.shown_path(),
        Some(dir.join("a.png").as_path()),
        "the picture on screen is undisturbed"
    );

    std::fs::remove_file(dir.join("b.png")).expect("we just wrote it");
    assert_eq!(
        Effect::Nothing,
        app.poll_directories(),
        "the change has not settled yet"
    );
    assert_eq!(Effect::Redraw, app.poll_directories());
    assert_eq!(app.files.len(), 2);
    assert_eq!(app.files.path(1), dir.join("c.png"));

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// Deleting the file being looked at does not take the picture off the
/// screen — there is nothing to put in its place — so the bar says what
/// has happened to it, and the walk goes on around it.
#[test]
fn a_deleted_file_stays_on_screen_and_is_marked() {
    let (mut app, dir) = opening_directory("deleted", &[("a.png", 8, 8), ("b.png", 8, 8)]);
    answer(&mut app, Reload::Fresh);
    assert_eq!(
        Effect::Nothing,
        app.poll_file(),
        "nothing has happened to it"
    );
    assert!(!app.watch.missing());

    std::fs::remove_file(dir.join("a.png")).expect("we just wrote it");
    assert_eq!(
        Effect::Nothing,
        app.poll_file(),
        "one poll into a save is not a deletion"
    );
    assert_eq!(
        Effect::Redraw,
        app.poll_file(),
        "the bar has something new to say"
    );
    assert!(app.watch.missing());
    assert_eq!(
        app.poll_file(),
        Effect::Nothing,
        "and having been said once it is not said again"
    );
    assert!(app.current.is_some(), "the picture is untouched");

    // The list still names it, and still steps around it. Rebuilding it
    // changes nothing: the file on screen goes back in where it was, so
    // the count in the bar and the walk are the same as they were.
    assert_eq!(Effect::Nothing, app.poll_directories());
    assert_eq!(Effect::Nothing, app.poll_directories());
    assert_eq!(app.files.len(), 2);
    assert_eq!(app.files.shown_path(), Some(dir.join("a.png").as_path()));
    let _ = app.step(true);
    assert_eq!(
        app.files.pending().map(|pending| pending.index),
        Some(1),
        "`]` goes on to the file that is still there"
    );

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// The list is rebuilt between reads and not during one: a rebuild moves
/// the file on screen to a new index, and the reply on its way is aimed at
/// the old one. The change is not lost — the watch has not seen it yet.
#[test]
fn a_directory_is_not_rebuilt_under_a_read_in_flight() {
    let (mut app, dir) = opening_directory("mid-read", &[("a.png", 8, 8), ("b.png", 8, 8)]);
    answer(&mut app, Reload::Fresh);

    write_png(&dir, "c.png", 8, 8);
    let _ = app.step(true);
    assert!(!app.files.is_idle());
    for _ in 0..4 {
        assert_eq!(
            Effect::Nothing,
            app.poll_directories(),
            "not while a read is in flight"
        );
    }
    assert_eq!(app.files.len(), 2);

    answer(&mut app, Reload::Fresh);
    assert_eq!(
        Effect::Nothing,
        app.poll_directories(),
        "the first look at the change"
    );
    assert_eq!(Effect::Redraw, app.poll_directories());
    assert_eq!(app.files.len(), 3);

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// Holding `]` through a directory asks for each file in turn without
/// waiting for the last, and only the file the user stopped on is shown.
/// Anything else would be a picture they have already scrolled past.
#[test]
fn a_reply_the_user_has_stepped_past_is_dropped() {
    let (mut app, dir) = app_over(
        "stale",
        &[("a.png", 64, 48), ("b.png", 32, 32), ("c.png", 16, 16)],
    );

    let _ = app.step(true);
    let overtaken = app
        .files
        .pending()
        .expect("a request is in flight")
        .generation;
    let _ = app.step(true);
    assert_eq!(app.files.pending().map(|pending| pending.index), Some(2));

    // The first file arrives late, after the user has moved past it.
    let path = app.files.path(1).to_path_buf();
    let image = decode::load(&path, app.overrides).expect("we just wrote it");
    let _ = app.deliver(Decoded {
        generation: overtaken,
        file: Opened {
            index: 1,
            watch: Watch::new(&path),
            path,
            mode: Reload::Fresh,
            rendering: Rendering::Developed,
        },
        outcome: Ok(Ready {
            stats: Stats::scan(&image),
            exif: exif::Exif::default(),
            image,
            gpu: None,
            sequence: Sequence::Still,
            page: 0,
            rendering: Rendering::Developed,
            camera_jpeg: CameraJpeg::Unavailable,
            format: "png",
        }),
    });
    assert_eq!(
        app.files.index(),
        0,
        "an overtaken file must not reach the screen"
    );

    // The one actually waited for still lands.
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.index(), 2);

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// A file whose header reads cleanly and whose pixels do not gets past
/// the check that happens before the window opens. Opening asks for the
/// first file as a walk for exactly that reason, so start-up steps over it
/// as `]` would step over it later.
#[test]
fn a_first_file_that_will_not_decode_is_stepped_over() {
    let (mut app, dir) = opening(
        "first-broken",
        &[("a.png", 64, 48), ("b.png", 32, 32), ("c.png", 16, 16)],
    );
    corrupt(app.files.path(0));

    assert_eq!(app.files.pending().map(|pending| pending.index), Some(0));
    answer(&mut app, Reload::Fresh);
    assert!(app.current.is_none(), "nothing can be shown yet");
    assert_eq!(
        app.files.pending().map(|pending| pending.index),
        Some(1),
        "the walk carries on to the next file"
    );

    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.index(), 1);
    assert!(!app.showed_nothing());

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// When none of them decode there is nothing to look at, and the caller
/// needs to know so it can leave with a failing status rather than sit in
/// an empty window.
#[test]
fn nothing_decoding_at_all_is_reported_as_having_shown_nothing() {
    let (mut app, dir) = opening("all-broken", &[("a.png", 64, 48), ("b.png", 32, 32)]);
    for index in 0..app.files.len() {
        corrupt(app.files.path(index));
    }

    for _ in 0..app.files.len() {
        if app.files.is_idle() {
            break;
        }
        answer(&mut app, Reload::Fresh);
    }
    assert!(app.files.is_idle(), "the walk has to stop asking");
    assert!(app.showed_nothing());

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// Holds a thumbnail for `path` of `size`, and what its header says, as
/// the thumbnail thread would have by the time a step reaches it.
fn thumbnailed(app: &mut App, path: &Path, size: (u32, u32)) {
    let _ = app.chooser.learn(
        path,
        Facts {
            size: Some(size),
            sequence: Sequence::Still,
            title: None,
            format: None,
            bytes: None,
            modified: None,
        },
    );
    let ctx = egui::Context::default();
    let copies = crate::thumbnailer::DISPLAY_SIDES.map(|side| {
        ctx.load_texture(
            "thumbnail",
            egui::ColorImage::filled([side as usize; 2], egui::Color32::GRAY),
            egui::TextureOptions::LINEAR,
        )
    });
    app.thumbs.insert(path.to_path_buf(), copies);
}

/// Where `app`'s picture is on screen: what a stand-in has to have
/// landed on.
fn landed(app: &App) -> [f32; 4] {
    let placement = app.view.placement(app.image_size(), app.viewport());
    [placement.x, placement.y, placement.width, placement.height]
}

/// A read slow enough to be said puts the file's thumbnail up in the
/// picture's place — not before it is said, and not for a file whose
/// thumbnail is not held — exactly where the picture then lands: fitted
/// afresh for a file new to the screen, and put back as it was left,
/// turn and all, for one coming back. The panels about the picture wait
/// for the same read, thumbnail or none.
#[test]
fn a_slow_read_stands_the_thumbnail_where_the_picture_lands_and_the_panels_wait() {
    let (mut app, dir) = app_over("standin", &[("a.png", 64, 48), ("b.png", 32, 16)]);
    app.headless = Some(WINDOW);
    let (a, b) = (
        app.files.path(0).to_path_buf(),
        app.files.path(1).to_path_buf(),
    );
    let said = |app: &mut App| {
        let since = app.files.pending().expect("a read is in flight").since;
        let _ = app.files.announce_slow_read(since + files::SLOW_READ);
    };

    // Left zoomed in and turned, to be put back that way.
    let (image, viewport) = (app.image_size(), app.viewport());
    app.view.zoom_in([400.0, 300.0], image, viewport);
    let _ = app.turn_picture(true);
    let left_a = landed(&app);

    let _ = app.step(true);
    assert!(
        app.standin(&app.sight()).is_none(),
        "the wait has not been said"
    );
    assert!(!app.frame_input([1000.0, 700.0], 1.0).waiting);
    said(&mut app);
    assert!(app.standin(&app.sight()).is_none(), "no thumbnail is held");
    assert!(app.reading().is_some());
    // The panels wait whether or not there is a thumbnail to show.
    assert!(app.frame_input([1000.0, 700.0], 1.0).waiting);
    thumbnailed(&mut app, &b, (32, 16));
    let standin = app
        .standin(&app.sight())
        .expect("the wait is said and the thumbnail held");
    assert_eq!(standin.turn, Turn::NONE);
    assert!(
        app.reading().is_some(),
        "the toast says it too: a blurred picture unexplained reads as a fault"
    );
    let stood = standin.placement;
    assert!(app.frame_input([1000.0, 700.0], 1.0).standin.is_some());
    answer(&mut app, Reload::Fresh);
    assert!(
        app.standin(&app.sight()).is_none(),
        "the picture itself is up"
    );
    assert!(!app.frame_input([1000.0, 700.0], 1.0).waiting);
    assert_eq!(landed(&app), [stood.x, stood.y, stood.width, stood.height]);

    let _ = app.step(false);
    said(&mut app);
    thumbnailed(&mut app, &a, (64, 48));
    let standin = app
        .standin(&app.sight())
        .expect("the wait is said and the thumbnail held");
    assert_eq!(standin.turn, Turn::NONE.clockwise());
    let stood = standin.placement;
    answer(&mut app, Reload::Fresh);
    assert_eq!(landed(&app), [stood.x, stood.y, stood.width, stood.height]);
    assert_eq!(landed(&app), left_a);

    // The file on screen read again stays up as it is.
    let _ = app.files.reload();
    said(&mut app);
    assert!(app.standin(&app.sight()).is_none());
    assert!(!app.frame_input([1000.0, 700.0], 1.0).waiting);

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// A step moves what says where in the list the key has gone — the
/// count, the name in the bar and the title, the file list's highlight —
/// at once, while the picture stays; what acts on the file waits, the
/// file named not being the one on screen. A read of a file chosen
/// outright that fails puts all of it back on the file still up.
#[test]
fn the_list_readouts_follow_the_key_and_go_back_when_a_read_fails() {
    use ui::Naming;

    let (mut app, dir) = app_over("target", &[("a.png", 8, 8), ("b.png", 8, 8)]);
    let input = |app: &mut App| app.frame_input([1000.0, 700.0], 1.0);
    assert_eq!(input(&mut app).arriving, None);

    assert_eq!(app.step(true), Effect::Redraw);
    let stepped = input(&mut app);
    assert_eq!(stepped.index, 1);
    assert_eq!(stepped.arriving.as_deref(), Some("b.png"));
    assert!(app.filmstrip.reveals());
    assert_eq!(app.title(), "b.png — gamut");
    assert_eq!(
        app.namer().tooltip(ui::Tip::Name).map(|tip| tip.title),
        Some(vec![dir.join("b.png").display().to_string()])
    );
    assert_eq!(app.files.shown_path(), Some(dir.join("a.png").as_path()));
    assert!(app.conditions().arriving);
    assert_eq!(app.press(ui::Control::Delete), Effect::Nothing);
    assert!(
        dir.join("a.png").exists(),
        "the file on screen is not the one named"
    );
    assert_eq!(app.perform(input::Action::TurnRight), Effect::Nothing);
    assert_eq!(
        app.current.as_ref().map(|current| current.turn),
        Some(Turn::NONE),
        "nor is it the one a turn would be kept with"
    );
    answer(&mut app, Reload::Fresh);
    assert_eq!(input(&mut app).arriving, None);
    assert!(!app.conditions().arriving);
    assert_eq!(app.title(), "b.png — gamut");

    // Chosen outright rather than walked to: a failure has nowhere to
    // go on to, and everything goes back to the file on screen.
    corrupt(&dir.join("a.png"));
    let request = app.files.go_to(0);
    assert_eq!(app.send(request), Effect::Redraw);
    assert_eq!(app.title(), "a.png — gamut");
    answer(&mut app, Reload::Fresh);
    let failed = input(&mut app);
    assert_eq!((failed.index, failed.arriving), (1, None));
    assert_eq!(app.title(), "b.png — gamut");

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// The transport bar is the file the bar names from the key: a file of
/// pages on its way in brings its own, at the page it opens on, and the
/// room for it with it, before a pixel of it is read, with nothing on
/// it that moves. A file whose header has not been read has none until
/// it arrives.
#[test]
fn the_transport_bar_is_the_arriving_files_from_the_key() {
    let (mut app, dir) = app_over("transport", &[("a.png", 8, 8), ("b.png", 8, 8)]);
    app.headless = Some(WINDOW);
    let b = app.files.path(1).to_path_buf();
    let input = |app: &mut App| app.frame_input([1000.0, 700.0], 1.0);
    assert!(input(&mut app).transport.is_none());

    let _ = app.step(true);
    assert!(input(&mut app).transport.is_none(), "b's header is unread");
    let content =
        |app: &App| ui::chrome::content_area(app.logical_size(), app.panels.show_ui, app.parts());
    let bare = content(&app);
    let _ = app.chooser.learn(
        &b,
        Facts {
            size: Some((8, 8)),
            sequence: Sequence::Pages {
                count: 3,
                default: 1,
            },
            title: None,
            format: None,
            bytes: None,
            modified: None,
        },
    );
    assert_eq!(
        input(&mut app).transport,
        Some(ui::Transport {
            index: 1,
            count: 3,
            kind: ui::transport::Kind::Pages,
        })
    );
    assert!(app.parts().transport, "the bar takes its room at the key");
    assert!(content(&app).height < bare.height);
    assert_eq!(app.press(ui::Control::StepForward), Effect::Nothing);

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// The last file of a kind that took its time says the next of its kind
/// will: the wait is said from the key, not a beat after it — scaled by
/// the pixels, and only for a file whose header has been read.
#[test]
fn a_kind_that_was_slow_is_said_to_be_slow_at_once() {
    let (mut app, dir) = app_over(
        "predicted",
        &[("a.png", 8, 8), ("b.png", 8, 8), ("c.png", 8, 8)],
    );
    let format = app
        .current
        .as_ref()
        .and_then(|current| current.file.reader)
        .expect("the file on screen was read by something");
    assert!(app.read_rates.contains_key(format), "noted as it arrived");
    let said = |app: &App| app.files.pending().and_then(|pending| pending.announced);

    // A rate that makes an 8 by 8 picture take a second.
    app.read_rates.insert(format, 1.0 / 64.0);
    let _ = app.step(true);
    assert!(said(&app).is_none(), "nothing known of the file's header");
    answer(&mut app, Reload::Fresh);

    app.read_rates.insert(format, 1.0 / 64.0);
    let c = dir.join("c.png");
    let _ = app.chooser.learn(
        &c,
        Facts {
            size: Some((8, 8)),
            sequence: Sequence::Still,
            title: None,
            format: Some(format),
            bytes: None,
            modified: None,
        },
    );
    let _ = app.step(true);
    assert!(said(&app).is_some());
    answer(&mut app, Reload::Fresh);

    // Fast at this size: a millionth of the rate.
    app.read_rates.insert(format, 1.0 / 64.0 / 1e6);
    let _ = app.step(true);
    let _ = app.chooser.learn(
        &dir.join("a.png"),
        Facts {
            size: Some((8, 8)),
            sequence: Sequence::Still,
            title: None,
            format: Some(format),
            bytes: None,
            modified: None,
        },
    );
    assert!(said(&app).is_none());

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// While nothing is on screen the title names the file being read, and it
/// follows the walk rather than staying on a file that would not open.
#[test]
fn the_title_names_the_file_being_read_until_there_is_one_to_show() {
    let (mut app, dir) = opening("title", &[("a.png", 64, 48), ("b.png", 32, 32)]);
    corrupt(app.files.path(0));

    assert_eq!(app.title(), "loading a.png — gamut");
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.title(), "loading b.png — gamut");
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.title(), "b.png — gamut");

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// A file that will not decode must not trap navigation: the walk carries
/// on in the direction it was going.
#[test]
fn a_file_that_will_not_decode_is_stepped_over() {
    let (mut app, dir) = app_over(
        "broken",
        &[("a.png", 64, 48), ("b.png", 32, 32), ("c.png", 16, 16)],
    );
    std::fs::write(app.files.path(1), b"not a png at all").expect("the file is writable");

    let _ = app.step(true);
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.index(), 0, "the broken file cannot be shown");
    assert_eq!(
        app.files.pending().map(|pending| pending.index),
        Some(2),
        "and the walk carries on past it"
    );

    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.index(), 2);

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// And it gives up once it has been all the way round, rather than asking
/// for files for ever when none of them will open.
#[test]
fn a_walk_through_files_that_all_fail_comes_to_a_stop() {
    let (mut app, dir) = app_over(
        "hopeless",
        &[("a.png", 64, 48), ("b.png", 32, 32), ("c.png", 16, 16)],
    );
    for index in 1..app.files.len() {
        std::fs::write(app.files.path(index), b"not a png at all").expect("the file is writable");
    }

    let _ = app.step(true);
    for _ in 0..app.files.len() {
        if app.files.is_idle() {
            break;
        }
        answer(&mut app, Reload::Fresh);
    }
    assert!(app.files.is_idle(), "the walk has to stop asking");
    assert_eq!(app.files.index(), 0);

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// A file of another size is another picture, and gets the opening view.
#[test]
fn stepping_to_an_image_of_another_size_fits_it() {
    let (mut app, dir) = app_over("other", &[("a.png", 64, 48), ("b.png", 32, 32)]);
    app.view
        .set_zoom_at(1.0, VIEWPORT.center(), app.image_size(), VIEWPORT);
    app.view
        .zoom_in(VIEWPORT.center(), app.image_size(), VIEWPORT);

    let _ = app.step(true);
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.index(), 1);
    assert_eq!(app.view.fit(), Some(Fit::Whole));

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// A raw with no camera JPEG in it, asked for as the camera's JPEG,
/// shows its developed picture rather than failing, says so, and offers
/// no switch: the key is refused and the preference stays as it was.
#[test]
fn a_raw_without_a_camera_jpeg_falls_back_to_the_developed_picture() {
    let path = fixture("dng-cfa.dng");
    let mut app = open(vec![path.clone()], vec![path]);
    app.rendering = Rendering::CameraJpeg;
    answer(&mut app, Reload::Fresh);
    let current = app.current.as_ref().expect("the developed picture is up");
    assert_eq!(current.rendering, Rendering::Developed);
    assert_eq!(current.camera_jpeg, CameraJpeg::Missing);
    assert_eq!(app.files.index(), 0);
    assert_eq!(
        app.toasts.showing().map(|toast| toast.message.as_str()),
        Some(NO_CAMERA_JPEG_SHOWN)
    );
    assert!(!app.conditions().camera_jpeg);
    assert!(matches!(
        app.press(crate::ui::Control::CameraJpeg),
        Effect::Nothing
    ));
    assert_eq!(app.rendering, Rendering::CameraJpeg);
    assert!(app.files.is_idle(), "nothing asked for");
}

/// The depth toggle answers only for a picture with a depth map, and
/// then turns on without anything being read again: the map came with
/// the picture, and the picture stays.
#[test]
fn the_depth_toggle_answers_only_for_a_picture_with_a_depth_map() {
    let path = fixture("png-rgb8.png");
    let mut app = open(vec![path.clone()], vec![path]);
    answer(&mut app, Reload::Fresh);
    assert!(!app.conditions().depth);
    assert!(matches!(
        app.press(crate::ui::Control::Depth),
        Effect::Nothing
    ));
    assert_eq!(app.show_beside, None, "refused");

    let path = fixture("heic-depth.heic");
    let mut app = open(vec![path.clone()], vec![path]);
    answer(&mut app, Reload::Fresh);
    assert!(app.conditions().depth);
    assert!(matches!(
        app.press(crate::ui::Control::Depth),
        Effect::Redraw
    ));
    assert_eq!(app.show_beside, Some(Auxiliary::Depth));
    assert!(app.files.is_idle(), "nothing asked for");
    assert!(app.current.is_some(), "the picture stays");
    let _ = app.perform(super::input::Action::ToggleDepth);
    assert_eq!(app.show_beside, None, "the key does what the button does");
}

/// The depth map shown in the picture's place is what is on screen in
/// every sense that matters: its size is what `Current` says, and so
/// what the top bar, the readout, a copy and an export read; its display
/// is its own; and the view is rescaled so that it covers what the
/// picture covered. Going back puts the picture back as it was left.
#[test]
fn the_depth_map_is_what_is_on_screen_while_it_is_shown() {
    let path = fixture("jpeg-depth.jpg");
    let mut app = open(vec![path.clone()], vec![path]);
    answer(&mut app, Reload::Fresh);
    let viewport = app.viewport();
    let picture = app.current.as_ref().expect("the picture").size();
    // A zoom the map can be shown at twice over: the window a test
    // opens is small enough that a fitted picture sits at the floor.
    app.view.set_zoom_at(1.0, [0.0, 0.0], picture, viewport);
    let covered = app.view.placement(picture, viewport);
    app.current
        .as_mut()
        .expect("the picture")
        .display
        .set_exposure(1.0);

    let _ = app.press(crate::ui::Control::Depth);
    let current = app.current.as_ref().expect("the map");
    assert_eq!(current.showing, Showing::Auxiliary(Auxiliary::Depth));
    assert_eq!(current.size(), [16.0, 12.0], "the map's own size");
    assert!(current.image.is_gray());
    assert_eq!(
        current.display.exposure_stops(),
        0.0,
        "a display of its own"
    );
    // A copy, or an export, is of the map: what is seen is what is
    // written out.
    let raster = current.seen().raster(Region::whole(current.pixels()));
    assert_eq!((raster.width, raster.height), (16, 12));
    let over = app.view.placement(current.size(), viewport);
    assert!(
        (over.width - covered.width).abs() < 1e-3 && (over.x - covered.x).abs() < 1e-3,
        "the map covers what the picture covered: {over:?} against {covered:?}"
    );
    // What the picture carries, and what it was left in, are still the
    // picture's.
    assert!(current.picture().0.carries(Auxiliary::Depth));
    assert_eq!(current.picture().1.exposure_stops(), 1.0);
    assert_eq!(app.picture_view().map(|(_, size)| size), Some(picture));

    let _ = app.press(crate::ui::Control::Depth);
    let current = app.current.as_ref().expect("the picture");
    assert_eq!(current.showing, Showing::Picture);
    assert_eq!(current.size(), picture);
    assert_eq!(current.display.exposure_stops(), 1.0, "as it was left");
    let back = app.view.placement(current.size(), viewport);
    assert!((back.width - covered.width).abs() < 1e-3);
}

/// The gain map's toggle answers only for a picture with a gain map, and
/// shows the map in stops at its own size; pressed while the depth map is
/// up, the other toggle takes its place, and going back puts the picture
/// back with its lift as it was.
#[test]
fn the_gain_map_toggle_shows_the_map_in_stops() {
    use crate::image::depth::DepthMap;
    use crate::image::gain_map::{GainMap, Lift};
    let (mut app, _dir) = app_over("gain", &[("a.png", 4, 3)]);
    assert!(!app.conditions().gain_map);
    assert!(matches!(
        app.press(crate::ui::Control::GainMap),
        Effect::Nothing
    ));
    assert_eq!(app.show_beside, None, "refused");

    let current = app.current.as_mut().unwrap();
    let mut image = (*current.image).clone();
    image.gain_map = Some(Arc::new(GainMap {
        width: 2,
        height: 1,
        channels: 1,
        data: vec![0, 255],
        lift: Lift::Apple { headroom: 4.0 },
    }));
    image.depth = Some(Arc::new(DepthMap {
        width: 1,
        height: 1,
        samples: crate::image::Samples::U8 {
            channels: crate::image::Channels::Gray,
            data: vec![7],
        },
        scale: None,
    }));
    current.image = Arc::new(image);
    assert!(app.conditions().gain_map);

    let _ = app.perform(super::input::Action::ToggleGainMap);
    assert_eq!(app.show_beside, Some(Auxiliary::GainMap));
    let current = app.current.as_ref().expect("the map");
    assert_eq!(current.showing, Showing::Auxiliary(Auxiliary::GainMap));
    assert_eq!(current.size(), [2.0, 1.0], "the map's own size");
    let stops = |x| current.sample(x, 0).expect("a pixel").stored()[0];
    assert!(stops(0).abs() < 1e-5 && (stops(1) - 2.0).abs() < 1e-3);
    assert!(current.lift.is_none(), "the map is not lifted itself");
    assert!(current.picture().0.carries(Auxiliary::GainMap));

    let _ = app.press(crate::ui::Control::Depth);
    assert_eq!(app.show_beside, Some(Auxiliary::Depth));
    let current = app.current.as_ref().expect("the depth map");
    assert_eq!(current.showing, Showing::Auxiliary(Auxiliary::Depth));

    let _ = app.press(crate::ui::Control::Depth);
    let current = app.current.as_ref().expect("the picture");
    assert_eq!(current.showing, Showing::Picture);
    assert_eq!(current.size(), [4.0, 3.0]);
    assert!(current.lift.is_some(), "the picture keeps its lift");
}

/// A file arriving is weighed against the picture, not against the depth
/// map shown in its place; and with the toggle on, the next picture with
/// a map comes up showing it, the next without one as itself.
#[test]
fn stepping_with_the_depth_map_up_follows_each_picture() {
    // In the order the list sorts them: two with a map, one without.
    let first = fixture("heic-depth.heic");
    let paths = vec![
        first.clone(),
        fixture("jpeg-depth.jpg"),
        fixture("png-rgb8.png"),
    ];
    let mut app = open(paths, vec![first]);
    answer(&mut app, Reload::Fresh);
    let _ = app.press(crate::ui::Control::Depth);

    let _ = app.step(true);
    answer(&mut app, Reload::Fresh);
    let current = app.current.as_ref().expect("the second picture");
    assert_eq!(current.showing, Showing::Auxiliary(Auxiliary::Depth));
    assert_eq!(current.size(), [16.0, 12.0], "its own map, at its size");

    let _ = app.step(true);
    answer(&mut app, Reload::Fresh);
    let current = app.current.as_ref().expect("the third picture");
    assert_eq!(current.showing, Showing::Picture, "no map to show");
    assert_eq!(current.size(), [32.0, 24.0]);
}

/// A file with nothing but the one picture is read the same whichever
/// is asked for, and says nothing about it.
#[test]
fn a_file_that_is_not_a_raw_ignores_the_rendering() {
    let path = fixture("png-rgb8.png");
    let mut app = open(vec![path.clone()], vec![path]);
    app.rendering = Rendering::CameraJpeg;
    answer(&mut app, Reload::Fresh);
    let current = app.current.as_ref().expect("the picture is up");
    assert_eq!(current.rendering, Rendering::Developed);
    assert_eq!(current.camera_jpeg, CameraJpeg::Unavailable);
    assert!(app.toasts.showing().is_none());
    assert!(!app.conditions().camera_jpeg);
}

/// The preference is read from the state file as the window opens.
#[test]
fn the_rendering_comes_back_from_the_state_file() {
    let path = fixture("png-rgb8.png");
    let size = decode::probe(&path).expect("a fixture");
    let app = App::new(
        vec![path.clone()],
        vec![path],
        Some(Opening {
            index: 0,
            source: Source::Disk,
            size: size.map(|(w, h)| [w as f32, h as f32]),
        }),
        options(),
        StateFile::holding(State {
            camera_jpeg: true,
            ..State::default()
        }),
        threads(),
    );
    assert_eq!(app.rendering, Rendering::CameraJpeg);
}

/// How the readout was last written comes back from the state file:
/// the pixel's value, its place, and a latitude's.
#[test]
fn the_readout_s_formats_come_back_from_the_state_file() {
    let app = App::new(
        Vec::new(),
        Vec::new(),
        None,
        options(),
        StateFile::holding(State {
            pixel_format: ui::PixelFormat::Depth,
            coordinate_format: ui::CoordinateFormat::Projected,
            geographic_format: ui::GeographicFormat::Dms,
            ..State::default()
        }),
        threads(),
    );
    assert_eq!(app.panels.pixel_format, ui::PixelFormat::Depth);
    assert_eq!(
        app.panels.coordinate_format,
        ui::CoordinateFormat::Projected
    );
    assert_eq!(app.panels.geographic_format, ui::GeographicFormat::Dms);
}

/// A fixture from `test_images/`, opened on its own.
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test_images")
        .join(name)
}

/// Waits for the player to have every frame, which a two-frame fixture
/// takes no time over.
fn decoded_to_the_end(app: &App) {
    let player = app.animation.as_ref().expect("an animation has a player");
    let started = Instant::now();
    while !player.read(|cache| cache.complete() || cache.error().is_some()) {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the player never finished"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    player.read(|cache| assert_eq!(cache.error(), None));
}

/// Stops the clock and starts it again at `now`, on the frame it was
/// on, so that a tick timed from `now` does not depend on how long the
/// decode before it took.
fn restart_clock(app: &mut App, now: Instant) {
    let animation = app.animation.as_mut().expect("an animation has a clock");
    assert!(animation.playing());
    animation.toggle(now);
    animation.toggle(now);
}

/// The pixel at `(x, y)` of the picture on screen, as bytes.
fn shown_pixel(app: &App, x: u32, y: u32) -> Vec<u8> {
    let image = &app.current.as_ref().expect("a picture is up").image;
    let count = image.channels().count();
    let start = (y as usize * image.width as usize + x as usize) * count;
    match &image.samples {
        crate::image::Samples::U8 { data, .. } => data[start..start + count].to_vec(),
        other => panic!("{other:?}"),
    }
}

/// An animation arriving starts playing from its first frame, and the
/// frame on screen follows the clock: the picture and its statistics
/// are the frame's, so that everything reading them reads the frame.
#[test]
fn an_animation_plays_and_the_picture_follows_the_clock() {
    use input::Action::{NextFrame, PreviousFrame, TogglePlay};

    let path = fixture("gif-animated.gif");
    let mut app = open(vec![path.clone()], vec![path]);
    answer(&mut app, Reload::Fresh);

    let playback = app.animation.as_ref().expect("an animation has a clock");
    assert!(playback.playing());
    assert_eq!(playback.count(), 2);
    assert_eq!(
        app.animation.as_ref().unwrap().uploaded(),
        None,
        "the file's own decode is up first"
    );
    let first = shown_pixel(&app, 8, 6);
    assert_eq!(&first[..3], [255, 0, 0], "red quadrant first");
    decoded_to_the_end(&app);
    let start = Instant::now();
    restart_clock(&mut app, start);

    // A tenth and a half later the second frame is due.
    let (changed, deadline) = app.tick_playback(start + Duration::from_millis(150));
    assert!(changed);
    assert!(deadline.is_some(), "the next frame has a time");
    assert_eq!(app.animation.as_ref().unwrap().head(), 1);
    app.show_due_frame();
    assert_eq!(app.animation.as_ref().unwrap().uploaded(), Some(1));
    let second = shown_pixel(&app, 8, 6);
    assert_eq!(&second[..3], [255, 255, 255], "the pattern upside down");

    // A step pauses and moves; play resumes.
    assert_eq!(app.perform(NextFrame), Effect::Redraw);
    let playback = app.animation.as_ref().unwrap();
    assert!(!playback.playing());
    assert_eq!(playback.head(), 0);
    assert_eq!(app.perform(PreviousFrame), Effect::Redraw);
    assert_eq!(app.animation.as_ref().unwrap().head(), 1);
    assert_eq!(app.perform(TogglePlay), Effect::Redraw);
    assert!(app.animation.as_ref().unwrap().playing());
    app.show_due_frame();
    assert_eq!(app.animation.as_ref().unwrap().uploaded(), Some(1));
}

/// `--paused` opens an animation stopped, and a still has no clock for
/// the keys to act on.
#[test]
fn paused_opens_stopped_and_a_still_has_no_clock() {
    use input::Action::TogglePlay;

    let path = fixture("webp-animated.webp");
    let mut app = open(vec![path.clone()], vec![path]);
    app.open_paused = true;
    answer(&mut app, Reload::Fresh);
    let playback = app.animation.as_ref().expect("an animation has a clock");
    assert!(!playback.playing());
    assert_eq!(
        app.tick_playback(Instant::now() + Duration::from_secs(1)),
        (false, None)
    );

    let path = fixture("png-rgb8.png");
    let mut app = open(vec![path.clone()], vec![path]);
    answer(&mut app, Reload::Fresh);
    assert!(app.animation.is_none());
    assert_eq!(app.perform(TogglePlay), Effect::Nothing);
}

/// Stepping away from an animation and back finds it on the frame it
/// was left on, stopped if it was stopped; a reload starts it over.
#[test]
fn an_animation_comes_back_to_the_frame_it_was_left_on() {
    use input::Action::NextFrame;

    let (dir, stills) = written("left-frame", &[("a.png", 32, 24)]);
    let animated = fixture("gif-animated.gif");
    let mut app = open(vec![animated.clone(), stills[0].clone()], vec![]);
    answer(&mut app, Reload::Fresh);
    let _ = app.perform(NextFrame);
    assert_eq!(app.animation.as_ref().unwrap().head(), 1);

    let _ = app.step(true);
    answer(&mut app, Reload::Fresh);
    assert!(app.animation.is_none(), "a still has no clock");
    let _ = app.step(true);
    answer(&mut app, Reload::Fresh);
    let playback = app.animation.as_ref().expect("back on the animation");
    assert_eq!(playback.head(), 1);
    assert!(
        !playback.playing(),
        "left stopped, so it comes back stopped"
    );

    let request = app.files.reload().expect("nothing is in flight");
    let _ = app.send(request);
    answer(&mut app, Reload::InPlace);
    let playback = app.animation.as_ref().expect("still an animation");
    assert_eq!(playback.head(), 0);
    assert!(playback.playing(), "read again, it starts over");

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// An animation turns like a still, and every frame after the turn is
/// drawn under it; a page of a paged file arrives under the turn the
/// file was already in.
#[test]
fn frames_and_pages_arrive_under_the_turn() {
    use input::Action::{NextFrame, TurnRight};

    let path = fixture("gif-animated.gif");
    let mut app = open(vec![path.clone()], vec![path]);
    answer(&mut app, Reload::Fresh);
    let [width, height] = app.image_size();
    let _ = app.perform(TurnRight);
    decoded_to_the_end(&app);
    let start = Instant::now();
    restart_clock(&mut app, start);
    let (changed, _) = app.tick_playback(start + Duration::from_millis(150));
    assert!(changed);
    app.show_due_frame();
    assert_eq!(app.animation.as_ref().unwrap().uploaded(), Some(1));
    assert_eq!(
        app.image_size(),
        [height, width],
        "the next frame is turned"
    );

    let paged = fixture("tiff-pages.tif");
    let mut app = open(vec![paged.clone()], vec![paged]);
    answer(&mut app, Reload::Fresh);
    let [width, height] = app.image_size();
    let _ = app.perform(TurnRight);
    let _ = app.perform(NextFrame);
    answer(&mut app, Reload::Page);
    assert_eq!(app.current.as_ref().unwrap().page, 1);
    assert_eq!(app.image_size(), [height, width], "the next page is turned");
}

/// A paged file opens on its default page and steps through the rest,
/// keeping the display and, at the same size, the view; a page request
/// waits for the one in flight; and the page it was left on is the one
/// it comes back to.
#[test]
fn a_paged_file_steps_through_its_pages() {
    use input::Action::{NextFrame, PreviousFrame};

    let (dir, stills) = written("pages", &[("a.png", 32, 24)]);
    let paged = fixture("tiff-pages.tif");
    let mut app = open(vec![paged.clone(), stills[0].clone()], vec![]);
    answer(&mut app, Reload::Fresh);
    let current = app.current.as_ref().unwrap();
    assert_eq!(current.page, 0);
    assert!(matches!(current.sequence, Sequence::Pages { count: 2, .. }));
    assert!(app.animation.is_none(), "pages have no clock");
    assert_eq!(&shown_pixel(&app, 8, 6)[..3], [255, 0, 0]);

    app.view
        .set_zoom_at(1.0, VIEWPORT.center(), app.image_size(), VIEWPORT);
    app.view
        .zoom_in(VIEWPORT.center(), app.image_size(), VIEWPORT);
    let zoom = app.view.zoom(app.image_size(), VIEWPORT);
    if let Some(current) = app.current.as_mut() {
        current.display.set_exposure(1.0);
    }

    assert_eq!(app.perform(NextFrame), Effect::Nothing);
    assert_eq!(
        app.files.pending().and_then(|pending| pending.page),
        Some(1)
    );
    // Held down: the second press waits for the first to land.
    assert_eq!(app.perform(NextFrame), Effect::Nothing);
    answer(&mut app, Reload::Page);
    let current = app.current.as_ref().unwrap();
    assert_eq!(current.page, 1);
    assert_eq!(
        &shown_pixel(&app, 8, 6)[..3],
        [255, 255, 255],
        "upside down"
    );
    assert_eq!(current.display.exposure_stops(), 1.0, "the display stays");
    assert_eq!(
        app.view.zoom(app.image_size(), VIEWPORT),
        zoom,
        "the view stays at the same size"
    );

    // Round the end, back to the first.
    let _ = app.perform(NextFrame);
    answer(&mut app, Reload::Page);
    assert_eq!(app.current.as_ref().unwrap().page, 0);
    let _ = app.perform(PreviousFrame);
    answer(&mut app, Reload::Page);
    assert_eq!(app.current.as_ref().unwrap().page, 1);

    // Away and back: the page it was left on.
    let _ = app.step(true);
    answer(&mut app, Reload::Fresh);
    let _ = app.step(true);
    assert_eq!(
        app.files.pending().and_then(|pending| pending.page),
        Some(1)
    );
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.current.as_ref().unwrap().page, 1);

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// What the window last said, for the tests about what a deletion or
/// a rename says.
fn said(app: &App) -> String {
    app.toasts
        .showing()
        .map(|toast| toast.message.clone())
        .unwrap_or_default()
}

/// The key that undoes, as the messages name it, is the key that
/// undoes: a message naming a key that did something else would send
/// the reader to the wrong key at the worst moment.
#[test]
fn the_messages_name_the_key_that_undoes() {
    use crate::app::input::Action;
    use crate::app::keymap::Keymap;
    let keys = Keymap::table();
    assert_eq!(keys.spelled("files.undo"), spelled_here("Ctrl+Z"));
    assert_eq!(keys.action_named("files.undo"), Some(Action::Undo));
    assert_eq!(
        keys.action_for(
            &winit::keyboard::Key::Character("z".into()),
            winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::KeyZ),
            winit::keyboard::ModifiersState::CONTROL,
            false
        ),
        Some(Action::Undo),
        "the plain letter under Ctrl"
    );
}

/// A deletion moves the file to the trash and steps on; the file leaves
/// the list once its neighbor is up; undo puts it back on disk and on
/// the list, and shows it again.
#[test]
fn a_deleted_file_goes_to_the_trash_and_comes_back_on_undo() {
    use crate::app::input::Action;
    let (mut app, dir) = opening_directory(
        "trash-step",
        &[("a.png", 8, 8), ("b.png", 8, 8), ("c.png", 8, 8)],
    );
    answer(&mut app, Reload::Fresh);
    app.trash = Some(Trash::under(dir.join("Trash")));

    assert_eq!(app.perform(Action::Delete), Effect::Redraw);
    assert!(!dir.join("a.png").exists());
    assert!(dir.join("Trash/files/a.png").exists());
    assert!(dir.join("Trash/info/a.png.trashinfo").exists());
    assert_eq!(said(&app), spelled_here("Trashed a.png. Ctrl+Z to undo."));
    assert!(app.watch.missing(), "the bar says so at once");
    assert_eq!(
        app.files.len(),
        3,
        "still on the list while it is on screen"
    );
    assert_eq!(
        app.files.pending().map(|pending| pending.index),
        Some(1),
        "the next file is asked for"
    );
    // Held down: refused until the neighbor is up, the bar naming it
    // and the file on screen not being it.
    assert_eq!(app.perform(Action::Delete), Effect::Nothing);
    assert!(dir.join("b.png").exists());

    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.len(), 2);
    assert_eq!(app.files.shown_path(), Some(dir.join("b.png").as_path()));
    assert_eq!(app.files.index(), 0);
    assert!(app.conditions().undoable);

    assert_eq!(app.perform(Action::Undo), Effect::Redraw);
    assert!(dir.join("a.png").exists(), "back where it was");
    assert!(!dir.join("Trash/files/a.png").exists());
    assert_eq!(app.files.len(), 3);
    assert_eq!(app.files.path(0), dir.join("a.png"));
    assert_eq!(
        app.files.pending().map(|pending| pending.index),
        Some(0),
        "and shown again"
    );
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.shown_path(), Some(dir.join("a.png").as_path()));
    assert!(!app.conditions().undoable);
    assert_eq!(app.perform(Action::Undo), Effect::Redraw);
    assert_eq!(said(&app), "Nothing to undo.");

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// The only file has nowhere to step to: it leaves the list and the
/// screen at once, and the window shows nothing — the empty window,
/// with the next picture to size it — until undo puts the file back
/// at the head of the list and shows it, as it was left.
#[test]
fn deleting_the_only_file_empties_the_window() {
    use crate::app::input::Action;
    let (mut app, dir) = app_over("trash-alone", &[("a.png", 8, 8)]);
    app.trash = Some(Trash::under(dir.join("Trash")));
    let _ = app.perform(Action::ZoomTo(4.0));
    let zoom = |app: &App| app.view.zoom(app.image_size(), app.viewport());
    let zoomed = zoom(&app);

    let _ = app.perform(Action::Delete);
    assert!(!dir.join("a.png").exists());
    assert!(app.is_empty());
    assert!(app.current.is_none());
    assert_eq!(app.files.len(), 0);
    assert!(!app.watch.missing());
    assert!(app.sizing.to_next);
    assert!(!app.from_command_line, "nothing showing is no failure now");
    assert!(!app.showed_nothing());
    assert_eq!(app.title(), crate::PROGRAM);
    assert!(said(&app).starts_with("Trashed a.png"), "{}", said(&app));

    let _ = app.perform(Action::Delete);
    assert_eq!(app.edits.len(), 1, "nothing to delete twice");

    let _ = app.perform(Action::Undo);
    assert!(dir.join("a.png").exists());
    assert_eq!(app.files.len(), 1);
    assert_eq!(app.files.pending().map(|pending| pending.index), Some(0));
    answer(&mut app, Reload::Fresh);
    assert!(app.current.is_some());
    assert_eq!(app.files.index(), 0);
    assert_eq!(app.files.shown_path(), Some(dir.join("a.png").as_path()));
    assert!(!app.sizing.to_next, "spent on the arrival");
    assert_eq!(zoom(&app), zoomed, "as it was left");

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// A removal takes the file off the list and touches nothing on disk:
/// the neighbor comes up, the file leaves the list, and the directory
/// being read again does not bring it back. Undo puts it back where it
/// stood and shows it.
#[test]
fn a_removed_file_leaves_the_list_and_stays_off_it_until_undo() {
    let (mut app, dir) = opening_directory(
        "remove-step",
        &[("a.png", 8, 8), ("b.png", 8, 8), ("c.png", 8, 8)],
    );
    answer(&mut app, Reload::Fresh);
    let _ = app.step(true);
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.shown_path(), Some(dir.join("b.png").as_path()));

    app.remove_shown();
    assert!(dir.join("b.png").exists(), "nothing done to it on disk");
    assert!(!app.watch.missing(), "and the bar does not call it deleted");
    assert_eq!(
        said(&app),
        spelled_here("Took b.png off the list. Ctrl+Z to undo.")
    );
    assert_eq!(
        app.files.len(),
        3,
        "still on the list while it is on screen"
    );
    assert_eq!(app.files.pending().map(|pending| pending.index), Some(2));
    // Held down: nothing more happens until the neighbor is up.
    app.remove_shown();
    assert_eq!(app.edits.len(), 1);

    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.len(), 2);
    assert_eq!(app.files.shown_path(), Some(dir.join("c.png").as_path()));
    assert_eq!(app.files.index(), 1);
    assert!(app.conditions().undoable);
    let rows = |app: &mut App| {
        app.filmstrip.follow(&app.files);
        let chooser = &app.chooser;
        app.filmstrip
            .input(
                &app.thumbs,
                |path| App::key_of(chooser, &HashMap::new(), path),
                None,
                false,
                false,
            )
            .rows
            .len()
    };
    assert_eq!(rows(&mut app), 2, "the strip has let it go too");

    // The directory read again lists it, and the list leaves it out.
    write_png(&dir, "d.png", 8, 8);
    assert_eq!(Effect::Nothing, app.poll_directories(), "not settled yet");
    assert_eq!(Effect::Redraw, app.poll_directories());
    assert_eq!(
        app.files.paths(),
        &[dir.join("a.png"), dir.join("c.png"), dir.join("d.png")]
    );

    assert_eq!(app.undo(), Effect::Redraw);
    assert_eq!(said(&app), "Put b.png back on the list.");
    assert_eq!(app.files.len(), 4);
    assert_eq!(app.files.path(1), dir.join("b.png"), "back where it stood");
    assert_eq!(
        app.files.pending().map(|pending| pending.index),
        Some(1),
        "and shown again"
    );
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.shown_path(), Some(dir.join("b.png").as_path()));
    assert!(!app.conditions().undoable);
    assert_eq!(Effect::Nothing, app.poll_directories());
    assert_eq!(
        Effect::Nothing,
        app.poll_directories(),
        "listed again, it stays"
    );
    assert_eq!(app.files.len(), 4);

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// The only file removed leaves the window showing nothing, as the
/// only file deleted does, with the file itself untouched; undo puts
/// it back at the head of the list and shows it.
#[test]
fn removing_the_only_file_empties_the_window() {
    let (mut app, dir) = app_over("remove-alone", &[("a.png", 8, 8)]);
    app.remove_shown();
    assert!(dir.join("a.png").exists());
    assert!(app.is_empty());
    assert_eq!(app.files.len(), 0);
    assert!(app.sizing.to_next);
    assert!(
        said(&app).starts_with("Took a.png off the list"),
        "{}",
        said(&app)
    );
    app.remove_shown();
    assert_eq!(app.edits.len(), 1, "nothing to take off twice");

    let _ = app.undo();
    assert_eq!(app.files.len(), 1);
    assert_eq!(app.files.pending().map(|pending| pending.index), Some(0));
    answer(&mut app, Reload::Fresh);
    assert!(app.current.is_some());
    assert_eq!(app.files.shown_path(), Some(dir.join("a.png").as_path()));

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// A removal waits for the read in flight, as a deletion does; a file
/// that stays on screen because its neighbor would not decode is
/// refused a second removal, and a deletion, since it is already on
/// its way out; and undo then is a reprieve, with the file never
/// having left.
#[test]
fn a_removal_waits_and_a_file_on_its_way_out_is_refused_again() {
    let (mut app, dir) = opening_directory("remove-busy", &[("a.png", 8, 8), ("b.png", 8, 8)]);
    app.remove_shown();
    assert!(app.edits.is_empty(), "the opening read is still in flight");
    answer(&mut app, Reload::Fresh);

    corrupt(&dir.join("b.png"));
    app.remove_shown();
    assert_eq!(app.edits.len(), 1);
    answer(&mut app, Reload::Fresh);
    assert!(app.files.is_idle(), "the walk had nowhere else to go");
    assert_eq!(app.files.shown_path(), Some(dir.join("a.png").as_path()));
    assert_eq!(
        app.files.len(),
        2,
        "still on the list, nothing having taken the screen"
    );

    app.remove_shown();
    assert_eq!(said(&app), "Already taken off the list.");
    app.trash = Some(Trash::under(dir.join("Trash")));
    app.delete_shown();
    assert_eq!(said(&app), "Already taken off the list.");
    assert!(dir.join("a.png").exists());
    assert_eq!(app.edits.len(), 1);

    let _ = app.undo();
    assert_eq!(app.files.len(), 2);
    assert!(app.files.is_idle(), "never left, so nothing to ask for");
    assert!(!app.files.is_hidden(&dir.join("a.png")));
    assert_eq!(Effect::Nothing, app.poll_directories());
    assert_eq!(Effect::Nothing, app.poll_directories(), "listed as before");

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// The list is put in the order the file list asks for — by name to
/// begin with, whatever order it was named in — and the file on
/// screen stays the file on screen. A sort that reads the headers
/// puts a file whose header is not yet read last, and moves it into
/// place once it is; and one asked for under a read waits for the
/// read to land.
#[test]
fn the_list_is_put_in_order_between_reads_and_the_file_on_screen_stays() {
    use crate::thumbnailer::{Delivered, Facts, News};
    use crate::ui::filmstrip::Sort;
    let (mut app, dir) = opening(
        "order",
        &[("b.png", 8, 8), ("a.png", 4, 4), ("c.png", 16, 16)],
    );
    assert_eq!(app.files.path(0), dir.join("b.png"), "as named");
    answer(&mut app, Reload::Fresh);
    let names = |app: &App| -> Vec<String> {
        app.files
            .paths()
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    };
    assert_eq!(
        names(&app),
        ["a.png", "b.png", "c.png"],
        "in name order once the read landed"
    );
    assert_eq!(app.files.shown_path(), Some(dir.join("b.png").as_path()));
    assert_eq!(app.files.index(), 1);

    // By area: only the file on screen has been read, and the rest
    // wait at the end in the order they stood.
    assert_eq!(app.press(ui::Control::SortBy(Sort::Area)), Effect::Redraw);
    assert_eq!(names(&app), ["b.png", "a.png", "c.png"]);
    assert_eq!(app.files.index(), 0);
    assert!(
        app.filmstrip.reveals(),
        "the strip follows the file on screen"
    );
    let learned = |name: &str, side: u32| Delivered {
        path: dir.join(name),
        news: News::Facts(Facts {
            size: Some((side, side)),
            sequence: Sequence::Still,
            title: None,
            format: Some("PNG"),
            bytes: None,
            modified: None,
        }),
    };
    app.take_thumbnail(learned("a.png", 4));
    app.take_thumbnail(learned("c.png", 16));
    assert_eq!(
        names(&app),
        ["b.png", "a.png", "c.png"],
        "not until the poll"
    );
    assert_eq!(app.poll_order(), Effect::Redraw);
    assert_eq!(names(&app), ["a.png", "b.png", "c.png"]);
    assert_eq!(app.files.shown_path(), Some(dir.join("b.png").as_path()));
    assert_eq!(app.poll_order(), Effect::Nothing, "in order already");

    // Under a read: the list is left alone until the read lands.
    let _ = app.step(true);
    assert_eq!(app.press(ui::Control::SortBy(Sort::Height)), Effect::Redraw);
    let _ = app.press(ui::Control::SortBy(Sort::Size));
    assert_eq!(names(&app), ["a.png", "b.png", "c.png"]);
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.shown_path(), Some(dir.join("c.png").as_path()));
    // By size on disk, which only the two files read so far have
    // said: the smaller first, and the one not read last.
    assert_eq!(names(&app), ["b.png", "c.png", "a.png"]);
    assert_eq!(app.files.index(), 1);

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// Back and forward walk the files that have been on screen, in the
/// order they were: a step after going back cuts off what lay ahead,
/// and a file taken off the list is passed over.
#[test]
fn back_and_forward_walk_the_files_seen() {
    use crate::app::input::Action;
    let (mut app, dir) = opening_directory(
        "visited",
        &[("a.png", 8, 8), ("b.png", 8, 8), ("c.png", 8, 8)],
    );
    answer(&mut app, Reload::Fresh);
    assert!(!app.conditions().visited_before && !app.conditions().visited_after);
    assert_eq!(app.perform(Action::Back), Effect::Nothing);
    assert!(app.files.is_idle(), "nowhere to go");

    let _ = app.step(true);
    answer(&mut app, Reload::Fresh);
    let _ = app.step(true);
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.shown_path(), Some(dir.join("c.png").as_path()));
    assert!(app.conditions().visited_before && !app.conditions().visited_after);

    assert_eq!(app.perform(Action::Back), Effect::Redraw);
    assert_eq!(app.files.pending().map(|pending| pending.index), Some(1));
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.shown_path(), Some(dir.join("b.png").as_path()));
    assert!(app.conditions().visited_before && app.conditions().visited_after);
    let _ = app.perform(Action::Forward);
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.shown_path(), Some(dir.join("c.png").as_path()));
    let _ = app.perform(Action::Back);
    answer(&mut app, Reload::Fresh);
    let _ = app.perform(Action::Back);
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.shown_path(), Some(dir.join("a.png").as_path()));
    assert!(!app.conditions().visited_before);

    // A step from here is somewhere new: what lay ahead is cut off.
    let _ = app.step(true);
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.shown_path(), Some(dir.join("b.png").as_path()));
    assert!(!app.conditions().visited_after);

    // The file taken off the list is passed over on the way back.
    app.remove_shown();
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.shown_path(), Some(dir.join("c.png").as_path()));
    let _ = app.perform(Action::Back);
    assert_eq!(
        app.files
            .pending()
            .map(|pending| app.files.path(pending.index).to_path_buf()),
        Some(dir.join("a.png"))
    );
    answer(&mut app, Reload::Fresh);
    // Undo shows the file it puts back, which is somewhere new to
    // have gone from here: what lay ahead is cut off, and the file
    // put back is on the stack again.
    let _ = app.undo();
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.shown_path(), Some(dir.join("b.png").as_path()));
    assert!(app.conditions().visited_before && !app.conditions().visited_after);
    let _ = app.perform(Action::Back);
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.shown_path(), Some(dir.join("a.png").as_path()));
    assert!(app.conditions().visited_after);

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// `Tab` puts the file list up, which narrows what the picture is
/// fitted into; driven over the application, its rows are the list,
/// the row of the file on screen is marked, and a press on another
/// row asks for that file.
#[test]
fn the_file_list_comes_up_beside_the_picture_and_shows_what_is_pressed() {
    use crate::app::input::Action;
    use egui_kittest::kittest::Queryable;
    let (mut app, dir) = app_over(
        "filmstrip",
        &[("a.png", 8, 8), ("b.png", 8, 8), ("c.png", 8, 8)],
    );
    app.headless = Some(WINDOW);
    let whole = app.viewport();
    assert!(!app.filmstrip_showing());
    assert_eq!(app.perform(Action::ToggleFilmstrip), Effect::Redraw);
    assert!(app.filmstrip_showing());
    assert_eq!(app.parts().filmstrip, Some(ui::filmstrip::SLOT_DEFAULT));
    let narrowed = app.viewport();
    assert_eq!(
        narrowed.x,
        whole.x + ui::filmstrip::width(ui::filmstrip::SLOT_DEFAULT)
    );
    assert_eq!(
        narrowed.width,
        whole.width - ui::filmstrip::width(ui::filmstrip::SLOT_DEFAULT)
    );

    let mut harness = driven(app);
    assert!(harness.query_by_label("Show file 1").is_some());
    assert!(harness.query_by_label("Show file 3").is_some());
    click(&mut harness, "Show file 3");
    let app = harness.state_mut();
    assert_eq!(
        app.files.pending().map(|pending| pending.index),
        Some(2),
        "the third file is asked for"
    );
    answer(app, Reload::Fresh);
    assert_eq!(app.files.shown_path(), Some(dir.join("c.png").as_path()));

    // With one file there is no list to show, and the toggle is
    // remembered for when there is.
    app.remove_shown();
    answer(app, Reload::Fresh);
    app.remove_shown();
    answer(app, Reload::Fresh);
    assert_eq!(app.files.len(), 1);
    assert!(!app.filmstrip_showing());
    assert!(app.panels.show_filmstrip);

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// Hiding the interface leaves the file list up without its head, the
/// picture beside it rather than under it; the key that closes the
/// other panels with the bars closes the list too.
#[test]
fn hiding_the_interface_keeps_the_file_list_and_the_full_hide_closes_it() {
    use crate::app::input::Action;
    use egui_kittest::kittest::Queryable;
    let (mut app, dir) = app_over(
        "filmstrip-hidden",
        &[("a.png", 8, 8), ("b.png", 8, 8), ("c.png", 8, 8)],
    );
    app.headless = Some(WINDOW);
    let _ = app.perform(Action::ToggleFilmstrip);
    let _ = app.perform(Action::ToggleInterface);
    assert!(!app.panels.show_ui);
    assert!(app.filmstrip_showing());
    let viewport = app.viewport();
    assert_eq!(
        viewport.x,
        ui::filmstrip::width(ui::filmstrip::SLOT_DEFAULT)
    );
    assert_eq!(viewport.y, 0.0);
    assert_eq!(
        viewport.width,
        WINDOW[0] - ui::filmstrip::width(ui::filmstrip::SLOT_DEFAULT)
    );
    assert_eq!(viewport.height, WINDOW[1]);

    let mut harness = driven(app);
    assert!(harness.query_by_label("Show file 1").is_some());
    assert!(
        harness.query_by_label("Back").is_none(),
        "the head goes with the bars"
    );
    let app = harness.state_mut();

    // Back up, and down again with everything that floats.
    let _ = app.perform(Action::ToggleInterface);
    let _ = app.perform(Action::ToggleInterfaceAndPanels);
    assert!(!app.panels.show_ui);
    assert!(!app.panels.show_filmstrip);
    assert!(!app.filmstrip_showing());
    assert_eq!(app.viewport().width, WINDOW[0]);

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// A file that has been emptied from the trash cannot come back, and
/// the window says so rather than failing quietly.
#[test]
fn an_emptied_trash_has_nothing_to_put_back() {
    use crate::app::input::Action;
    let (mut app, dir) = app_over("trash-emptied", &[("a.png", 8, 8), ("b.png", 8, 8)]);
    app.trash = Some(Trash::under(dir.join("Trash")));
    let _ = app.perform(Action::Delete);
    answer(&mut app, Reload::Fresh);
    std::fs::remove_dir_all(dir.join("Trash")).expect("emptied");

    let _ = app.perform(Action::Undo);
    assert!(
        said(&app).contains("no longer in the trash"),
        "{}",
        said(&app)
    );
    assert!(app.files.is_idle());
    assert_eq!(app.files.len(), 1);
    assert!(!app.conditions().undoable, "the entry is spent");

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// The export dialog offers a name nothing has, follows the format a
/// typed extension names and the extension a format's button names,
/// refuses a name taken; and Export writes the picture as shown — turned,
/// through its exposure — beside the file on screen, which the list
/// takes in and shows.
#[test]
fn an_export_is_judged_as_typed_and_written_beside_the_source() {
    use crate::ui::export::{Format, Verdict};
    use input::Action::{Export, TurnRight};

    let (mut app, dir) = app_over("export", &[("a.png", 8, 6)]);
    let _ = app.perform(Export);
    let input = app.export_input().expect("the dialog is up");
    assert_eq!(input.name, "a-edited.png");
    assert_eq!(input.format, Format::Png);
    assert_eq!(input.quality, 90);
    assert_eq!(input.verdict, Verdict::Fine);
    assert!(input.warnings.is_empty(), "{:?}", input.warnings);
    assert!(input.opened);
    assert!(!app.export_input().expect("still up").opened);

    let _ = app.act(ui::Command::ExportName("b.jpg".to_string()));
    assert_eq!(app.export_input().unwrap().format, Format::Jpeg);
    let _ = app.act(ui::Command::ExportQuality(40));
    assert_eq!(app.export_input().unwrap().quality, 40);
    let _ = app.act(ui::Command::Press(ui::Control::ExportAs(Format::Png)));
    assert_eq!(app.export_input().unwrap().name, "b.png");
    let _ = app.act(ui::Command::ExportName("a.png".to_string()));
    assert_eq!(
        app.export_input().unwrap().verdict,
        Verdict::Refused(ui::rename::Refusal::Taken)
    );
    // Export does nothing while the name will not do.
    let _ = app.act(ui::Command::Press(ui::Control::ExportTo));
    assert!(app.export_input().is_some(), "still up");
    let _ = app.act(ui::Command::Press(ui::Control::CancelExport));
    assert!(app.export_input().is_none());

    // Turned, and brighter: both written into the file.
    let _ = app.perform(TurnRight);
    app.current.as_mut().unwrap().display.set_exposure(1.0);
    let _ = app.perform(Export);
    assert!(app.export_input().unwrap().warnings.is_empty());
    let _ = app.act(ui::Command::ExportName("b.png".to_string()));
    let _ = app.act(ui::Command::Press(ui::Control::ExportTo));
    assert!(app.export_input().is_none());
    app.copying.join_all();
    let _ = app.poll_copies();
    let written = ::image::open(dir.join("b.png")).expect("b.png was written");
    assert_eq!((written.width(), written.height()), (6, 8), "turned");
    assert!(
        written.to_rgb8().get_pixel(0, 0)[0] > 128,
        "the exposure is in the pixels"
    );
    assert_eq!(said(&app), "Exported b.png.");
    assert_eq!(app.files.pending().map(|pending| pending.index), Some(1));
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.shown_path(), Some(dir.join("b.png").as_path()));
    let current = app.current.as_ref().unwrap();
    assert_eq!(current.turn, Turn::NONE, "the turn is in its pixels");
    assert_eq!(app.image_size(), [6.0, 8.0]);

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// The export dialog's size boxes say one size between them, follow
/// the region where one is up, refuse a side that will not do and hold
/// Export until it is put right, warn of an enlargement the screen
/// shows as nearest; and Export writes the picture at that size.
#[test]
fn an_export_is_written_at_the_size_asked_for() {
    use crate::ui::export::{Dimension, Warning};
    use input::Action::Export;

    let (mut app, dir) = app_over("export-size", &[("a.png", 8, 6)]);
    let _ = app.perform(Export);
    let resize = |app: &mut App| app.export_input().expect("the dialog is up").resize;
    assert_eq!(resize(&mut app).source, [8, 6]);
    assert_eq!(resize(&mut app).typed, ["100", "8", "6"]);

    let _ = app.act(ui::Command::ExportSize(
        Dimension::Percent,
        "50".to_string(),
    ));
    assert_eq!(resize(&mut app).size, [4, 3]);
    assert!(app.export_input().unwrap().warnings.is_empty(), "a shrink");
    let _ = app.act(ui::Command::ExportSize(Dimension::Width, "16".to_string()));
    assert_eq!(resize(&mut app).typed, ["200", "16", "12"]);
    assert_eq!(
        app.export_input().unwrap().warnings,
        [Warning::Bicubic],
        "the screen shows nearest"
    );
    let _ = app.act(ui::Command::ExportSize(Dimension::Height, "0".to_string()));
    assert!(!resize(&mut app).allows());
    let _ = app.act(ui::Command::ExportName("big.png".to_string()));
    let _ = app.act(ui::Command::Press(ui::Control::ExportTo));
    assert!(app.export_input().is_some(), "held until the size will do");
    let _ = app.act(ui::Command::ExportSize(Dimension::Height, "12".to_string()));
    assert_eq!(resize(&mut app).size, [16, 12]);
    let _ = app.act(ui::Command::Press(ui::Control::ExportTo));
    assert!(app.export_input().is_none());
    // One at a time: the dialog opens again, but Export is dead until
    // the write has landed and been looked at.
    let _ = app.perform(Export);
    assert!(app.export_input().unwrap().busy);
    let _ = app.act(ui::Command::ExportName("second.png".to_string()));
    let _ = app.act(ui::Command::Press(ui::Control::ExportTo));
    assert!(app.export_input().is_some(), "held");
    app.copying.join_all();
    assert!(app.export_input().unwrap().busy, "until the loop looks");
    let _ = app.poll_copies();
    assert!(!app.export_input().unwrap().busy);
    let _ = app.act(ui::Command::Press(ui::Control::CancelExport));
    assert!(!dir.join("second.png").exists());
    let written = ::image::open(dir.join("big.png")).expect("big.png was written");
    assert_eq!((written.width(), written.height()), (16, 12));
    assert_eq!(said(&app), "Exported big.png.");
    answer(&mut app, Reload::Fresh);

    // Shrunk, and the region is what the percentage is of.
    let _ = app.perform(input::Action::PreviousFile);
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.image_size(), [8.0, 6.0]);
    app.marking.selection = ui::Selection::Shown(Region {
        x: 0,
        y: 0,
        width: 4,
        height: 2,
    });
    let _ = app.perform(Export);
    assert_eq!(resize(&mut app).source, [4, 2]);
    let _ = app.act(ui::Command::ExportSize(
        Dimension::Percent,
        "50".to_string(),
    ));
    assert_eq!(resize(&mut app).size, [2, 1]);
    let _ = app.act(ui::Command::ExportName("small.png".to_string()));
    let _ = app.act(ui::Command::Press(ui::Control::ExportTo));
    app.copying.join_all();
    let _ = app.poll_copies();
    let written = ::image::open(dir.join("small.png")).expect("small.png was written");
    assert_eq!((written.width(), written.height()), (2, 1));

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// The export dialog stops a playing animation, so that the frame
/// written is the one it opened on, and sets it playing again when it
/// goes, by Cancel or by Export; one that was stopped stays stopped.
#[test]
fn an_export_holds_an_animation_still_while_it_is_up() {
    use input::Action::{Export, TogglePlay};

    let (dir, _) = written("export-animation", &[]);
    let gif = dir.join("animated.gif");
    std::fs::copy(fixture("gif-animated.gif"), &gif).expect("the directory is writable");
    let mut app = open(vec![gif.clone()], vec![gif]);
    answer(&mut app, Reload::Fresh);
    let playing = |app: &App| app.animation.as_ref().unwrap().playing();
    assert!(playing(&app));

    let _ = app.perform(Export);
    assert!(!playing(&app), "stopped while the dialog is up");
    assert!(
        app.export_input()
            .unwrap()
            .warnings
            .contains(&crate::ui::export::Warning::OneFrame(1)),
        "the frame on screen, counted from one"
    );
    let _ = app.act(ui::Command::Press(ui::Control::CancelExport));
    assert!(playing(&app), "and playing again once it goes");

    let _ = app.perform(Export);
    assert!(!playing(&app));
    let _ = app.act(ui::Command::Press(ui::Control::ExportTo));
    assert!(app.export_input().is_none());
    assert!(playing(&app), "exporting puts it away too");
    app.copying.join_all();

    let _ = app.perform(TogglePlay);
    let _ = app.perform(Export);
    let _ = app.act(ui::Command::Press(ui::Control::CancelExport));
    assert!(!playing(&app), "one stopped by hand stays stopped");

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// A name judged free and taken by the time Export is pressed is not
/// written over: the file that arrived stays as it was, and the window
/// says so.
#[test]
fn an_export_does_not_replace_a_file_that_has_arrived() {
    let (mut app, dir) = app_over("export-arrived", &[("a.png", 8, 6)]);
    let _ = app.perform(input::Action::Export);
    let _ = app.act(ui::Command::ExportName("b.png".to_string()));
    std::fs::write(dir.join("b.png"), b"not ours").expect("the directory is writable");
    let _ = app.act(ui::Command::Press(ui::Control::ExportTo));
    app.copying.join_all();
    let _ = app.poll_copies();
    assert_eq!(said(&app), crate::ui::rename::TAKEN);
    assert_eq!(std::fs::read(dir.join("b.png")).unwrap(), b"not ours");
    let names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names.len(), 2, "nothing else left behind: {names:?}");
    assert!(app.files.pending().is_none(), "nothing to show");

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// The dialog judges the name as it is typed — taken, unchanged, an
/// extension changing — and OK renames the file everywhere it is known
/// by name; undo renames it back, and shows it if it had been left.
#[test]
fn a_rename_is_judged_as_typed_and_undone_by_name() {
    use crate::app::input::Action;
    use crate::ui::rename::{ExtensionChange, Verdict};
    let (mut app, dir) = app_over("rename", &[("a.png", 8, 8), ("b.png", 8, 8)]);

    let _ = app.perform(Action::Rename);
    let input = app.rename_input().expect("the dialog is up");
    assert_eq!(input.name, "a.png");
    assert_eq!(input.verdict, Verdict::Unchanged);
    assert!(input.opened);
    assert!(!app.rename_input().expect("still up").opened);

    assert_eq!(
        Effect::Redraw,
        app.act(ui::Command::Name("b.png".to_string()))
    );
    assert_eq!(
        app.rename_input().expect("up").verdict,
        Verdict::Refused(ui::rename::Refusal::Taken)
    );
    assert_eq!(
        Effect::Redraw,
        app.act(ui::Command::Name("c.jpg".to_string()))
    );
    assert_eq!(
        app.rename_input().expect("up").verdict,
        Verdict::Fine(Some(ExtensionChange {
            from: Some("png".to_string()),
            to: Some("jpg".to_string()),
        }))
    );

    // Cancel changes nothing.
    assert_eq!(
        Effect::Redraw,
        app.act(ui::Command::Press(ui::Control::CancelRename))
    );
    assert!(app.rename_input().is_none());
    assert!(dir.join("a.png").exists());

    let _ = app.perform(Action::Rename);
    assert_eq!(
        Effect::Redraw,
        app.act(ui::Command::Name("c.jpg".to_string()))
    );
    assert_eq!(
        Effect::Redraw,
        app.act(ui::Command::Press(ui::Control::RenameTo))
    );
    assert!(app.rename_input().is_none());
    assert!(dir.join("c.jpg").exists() && !dir.join("a.png").exists());
    assert_eq!(app.files.shown_path(), Some(dir.join("c.jpg").as_path()));
    assert_eq!(
        app.current.as_ref().map(|current| current.label.as_str()),
        Some("c.jpg")
    );
    assert_eq!(said(&app), spelled_here("Renamed a.png. Ctrl+Z to undo."));
    assert!(app.conditions().undoable);

    // Step away, then undo: the old name is back, and so is the file.
    // The list is in name order, so the renamed file has moved along
    // it, and moves back once the read of it lands.
    let _ = app.step(true);
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.shown_path(), Some(dir.join("b.png").as_path()));
    assert_eq!(app.files.path(1), dir.join("c.jpg"), "sorted after b.png");
    let _ = app.perform(Action::Undo);
    assert!(dir.join("a.png").exists() && !dir.join("c.jpg").exists());
    assert_eq!(app.files.path(1), dir.join("a.png"));
    assert_eq!(app.files.pending().map(|pending| pending.index), Some(1));
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.shown_path(), Some(dir.join("a.png").as_path()));
    assert_eq!(
        app.files.path(0),
        dir.join("a.png"),
        "and sorted back before b.png"
    );
    assert_eq!(
        app.current.as_ref().map(|current| current.label.as_str()),
        Some("a.png")
    );

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// A rename refuses to replace: a file made under the new name since
/// the dialog judged it is left alone, and the window says so.
#[test]
fn a_rename_does_not_replace_a_file_that_has_arrived() {
    use crate::app::input::Action;
    let (mut app, dir) = app_over("rename-race", &[("a.png", 8, 8)]);
    let _ = app.perform(Action::Rename);
    assert_eq!(
        Effect::Redraw,
        app.act(ui::Command::Name("b.png".to_string()))
    );
    write_png(&dir, "b.png", 4, 4);
    assert_eq!(
        Effect::Redraw,
        app.act(ui::Command::Press(ui::Control::RenameTo))
    );
    assert!(dir.join("a.png").exists());
    assert_eq!(app.files.shown_path(), Some(dir.join("a.png").as_path()));
    assert_eq!(said(&app), crate::ui::rename::TAKEN);
    assert!(!app.conditions().undoable);

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// The interface driven over the application itself, with no window:
/// laid out from `frame_input`, pressed through the accessibility tree
/// egui builds, and what it asked for done by `act`. One pass binds the
/// bold face the file's name is set in, as `ui/driven.rs` does; every
/// pass after that draws the interface.
fn driven(mut app: App) -> egui_kittest::Harness<'static, App> {
    use egui_kittest::Harness;

    app.headless = Some(WINDOW);
    let mut ready = false;
    let mut harness = Harness::builder()
        .with_size(egui::vec2(WINDOW[0], WINDOW[1]))
        .build_ui_state(
            move |ui, app: &mut App| {
                if !ready {
                    let mut fonts = egui::FontDefinitions::default();
                    let sans = fonts.families[&egui::FontFamily::Proportional].clone();
                    fonts
                        .families
                        .insert(egui::FontFamily::Name(ui::fonts::BOLD.into()), sans);
                    ui.ctx().set_fonts(fonts);
                    ui::style::apply(ui.ctx(), &app.theme);
                    ready = true;
                    return;
                }
                let input = app.frame_input(WINDOW, 1.0);
                let namer = app.namer();
                let view = app.shown_view();
                let commands = ui::show(
                    ui,
                    &input,
                    &app.panels,
                    app.current.as_ref(),
                    &view,
                    &app.theme,
                    &namer,
                );
                for command in commands {
                    let _ = app.act(command);
                }
            },
            app,
        );
    harness.run();
    harness
}

/// Presses the button called `label` and runs the pass that does what
/// the press asked for.
fn click(harness: &mut egui_kittest::Harness<'static, App>, label: &str) {
    use egui_kittest::kittest::Queryable;

    harness
        .get_by_role_and_label(egui::accesskit::Role::Button, label)
        .click();
    harness.run();
}

/// The two hints read off the application rather than a key's own line:
/// the file name's, which is the copy of the path it stands for, and the
/// maximize button's, which is its press with Shift.
#[test]
fn the_name_and_the_maximize_button_hint_at_the_other_press() {
    use crate::ui::Naming;

    let (app, _dir) = app_over("hints", &[("a.png", 4, 3)]);
    let hints = |tip| app.namer().tooltip(tip).map(|tooltip| tooltip.hints);
    assert_eq!(
        hints(ui::Tip::Name),
        Some(vec![spelled_here(
            "Copy the absolute path of the current file (Shift+C)"
        )])
    );
    assert_eq!(
        hints(ui::Tip::Control(ui::Control::Maximize)),
        Some(vec!["Hide all panels and toggle the UI (~)".to_string()])
    );
}

/// A press says what it left the window owing: a frame for a toggle,
/// and for a step — the picture stays until the file arrives, but the
/// count, the highlight and the name move — and nothing for a panel the
/// window has no room for, which is refused.
/// The refusal and the tooltip that says why are one reading: the
/// button is drawn dead, says there is no room, and does nothing, all
/// from the same answer.
#[test]
fn a_press_says_what_it_owes() {
    use crate::ui::Naming;

    let (mut app, _dir) = app_over("owed", &[("a.png", 4, 3), ("b.png", 4, 3)]);
    assert_eq!(app.press(ui::Control::Grid), Effect::Redraw);
    assert_eq!(app.press(ui::Control::Next), Effect::Redraw);
    // A window a pixel across has no room for the histogram.
    let histogram = ui::Tip::Control(ui::Control::Histogram);
    assert_eq!(app.press(ui::Control::Histogram), Effect::Nothing);
    assert_eq!(app.panels.side, None);
    assert_eq!(
        app.namer().tooltip(histogram).map(|tooltip| tooltip.title),
        Some(vec![ui::tooltip::NO_ROOM.to_string()])
    );
    app.headless = Some(WINDOW);
    assert_eq!(app.press(ui::Control::Histogram), Effect::Redraw);
    assert_eq!(app.panels.side, Some(ui::side::Side::Histogram));
    assert_ne!(
        app.namer().tooltip(histogram).map(|tooltip| tooltip.title),
        Some(vec![ui::tooltip::NO_ROOM.to_string()])
    );
}

/// The histogram's and the information's buttons are tabs of the side panel:
/// it holds one of the two at a time, the button of the one it holds takes it
/// down, and the picture is fitted into what it leaves at the width its edge
/// was dragged to — held between the least and the most the panel is.
#[test]
fn the_side_panel_holds_one_of_the_two_at_its_own_width() {
    use ui::side::{Side, WIDTH_MAX, WIDTH_MIN};

    let (mut app, dir) = app_over("side-panel", &[("a.png", 8, 8)]);
    app.headless = Some(WINDOW);
    let content =
        |app: &App| ui::chrome::content_area(app.logical_size(), app.panels.show_ui, app.parts());
    let bare = content(&app);

    let _ = app.press(ui::Control::Histogram);
    assert_eq!(app.side_showing(), Some(Side::Histogram));
    assert!(app.panels.lit(ui::Control::Histogram));
    assert!(!app.panels.lit(ui::Control::Info));
    assert_eq!(content(&app).width, bare.width - app.side_width);

    let _ = app.press(ui::Control::Info);
    assert_eq!(app.side_showing(), Some(Side::Info));
    assert!(!app.panels.lit(ui::Control::Histogram));
    assert_eq!(content(&app).width, bare.width - app.side_width);

    let _ = app.act(ui::Command::SideWidth(WIDTH_MAX + 100.0));
    assert_eq!(app.side_width, WIDTH_MAX);
    assert_eq!(content(&app).width, bare.width - WIDTH_MAX);
    let _ = app.act(ui::Command::SideWidth(0.0));
    assert_eq!(app.side_width, WIDTH_MIN);

    let _ = app.press(ui::Control::Info);
    assert_eq!(app.side_showing(), None);
    assert_eq!(content(&app), bare);

    // The bars hidden, it stays, down the window's edge; the key that
    // hides the panels with them takes it down.
    let _ = app.press(ui::Control::Histogram);
    let _ = app.perform(input::Action::ToggleInterface);
    assert_eq!(app.side_showing(), Some(Side::Histogram));
    assert_eq!(content(&app).right(), WINDOW[0] - app.side_width);
    let _ = app.perform(input::Action::ToggleInterface);
    let _ = app.perform(input::Action::ToggleInterfaceAndPanels);
    assert_eq!(app.panels.side, None);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A click on the interface reaches the application: the grid button,
/// pressed through the frame the application itself laid out, toggles
/// the application's own panel, the way the key for it does.
#[test]
fn a_click_on_the_interface_reaches_the_application() {
    let (app, _dir) = app_over("clicked", &[("a.png", 4, 3)]);
    let mut harness = driven(app);
    assert!(!harness.state().panels.show_grid);
    click(&mut harness, &ui::Control::Grid.label());
    assert!(harness.state().panels.show_grid);
    click(&mut harness, &ui::Control::Grid.label());
    assert!(!harness.state().panels.show_grid);
}

/// The loupe is up while its toggle is on or a button whose hold slot is
/// the loupe — the right, by default — is held on the picture, and only with a pixel under the pointer: what
/// the interface draws its rings from and the image layer its glass,
/// from the one pointer the readout reads. The button carries the
/// pointer with it, and lets go of the loupe unless the toggle keeps it.
#[test]
fn the_loupe_is_up_for_the_toggle_or_the_held_button_over_a_pixel() {
    use crate::gestures::Button;
    use crate::ui::{Command, Naming};
    use input::Action::CycleMagnification;
    let magnify = |steps| Command::Wheel {
        delta: [0.0, steps],
        notched: true,
        held: Some(Button::Right),
    };
    let held = |button, at| Command::Held { button, at };

    let (mut app, _dir) = app_over("loupe", &[("a.png", 64, 48)]);
    app.headless = Some(WINDOW);
    let middle = [WINDOW[0] / 2.0, WINDOW[1] / 2.0];
    app.pointer.cursor = Some(middle);
    app.pointer.over_image = true;
    assert!(app.pointer_pixel().is_some());
    assert_eq!(app.loupe(), None);

    // The toggle: up over a pixel, and nowhere else.
    assert_eq!(app.press(ui::Control::Loupe), Effect::Redraw);
    let loupe = app.loupe().expect("the loupe is up");
    assert_eq!(loupe.eye, middle);
    assert_ne!(loupe.glass, middle);
    app.pointer.over_image = false;
    assert_eq!(app.loupe(), None);
    app.pointer.over_image = true;
    // Through a drag it follows the pointer the pass hands over, and
    // stays up.
    let dragged = [middle[0] + 7.0, middle[1] - 2.0];
    assert_eq!(app.act(Command::Dragging(Some(dragged))), Effect::Redraw);
    assert_eq!(app.loupe().map(|loupe| loupe.eye), Some(dragged));
    assert_eq!(app.act(Command::Dragging(Some(dragged))), Effect::Nothing);
    assert_eq!(app.act(Command::Dragging(None)), Effect::Nothing);
    assert_eq!(app.loupe().map(|loupe| loupe.eye), Some(dragged));
    app.pointer.cursor = Some(middle);
    // The wheel with the button held steps the magnification, a notch
    // at a time however the notches arrive, and stops at either end.
    assert_eq!(app.panels.loupe_magnification, 4.0);
    assert_eq!(app.act(magnify(1.0)), Effect::Redraw);
    assert_eq!(app.panels.loupe_magnification, 8.0);
    assert_eq!(app.act(magnify(0.5)), Effect::Nothing);
    assert_eq!(app.act(magnify(0.5)), Effect::Redraw);
    assert_eq!(app.panels.loupe_magnification, 16.0);
    assert_eq!(app.act(magnify(3.0)), Effect::Nothing);
    assert_eq!(app.panels.loupe_magnification, 16.0);
    assert_eq!(app.act(magnify(-1.0)), Effect::Redraw);
    assert_eq!(app.panels.loupe_magnification, 8.0);
    // The key steps it round, the largest back to the smallest.
    assert_eq!(app.perform(CycleMagnification), Effect::Redraw);
    assert_eq!(app.panels.loupe_magnification, 16.0);
    assert_eq!(app.perform(CycleMagnification), Effect::Redraw);
    assert_eq!(app.panels.loupe_magnification, 2.0);
    assert_eq!(app.perform(CycleMagnification), Effect::Redraw);
    assert_eq!(app.panels.loupe_magnification, 4.0);
    assert_eq!(app.perform(CycleMagnification), Effect::Redraw);
    assert_eq!(app.loupe().map(|loupe| loupe.magnification), Some(8.0));
    assert_eq!(app.press(ui::Control::Loupe), Effect::Redraw);
    assert_eq!(app.loupe(), None);
    // Its tooltip says the button is the other way to it.
    assert_eq!(
        app.namer()
            .tooltip(ui::Tip::Control(ui::Control::Loupe))
            .map(|tooltip| tooltip.hints),
        Some(vec![
            ui::tooltip::loupe_held("Right button held"),
            ui::tooltip::loupe_wheel(Some("Right+Wheel"), Some(&spelled_here("Shift+L"))).unwrap()
        ])
    );

    // The button: the loupe comes up on the press, follows the pointer
    // the pass hands over, and goes on the release.
    assert_eq!(
        app.act(held(Some(Button::Right), Some(middle))),
        Effect::Redraw
    );
    assert_eq!(app.loupe().map(|loupe| loupe.eye), Some(middle));
    assert_eq!(
        app.act(held(Some(Button::Right), Some(middle))),
        Effect::Nothing
    );
    let moved = [middle[0] + 5.0, middle[1] + 3.0];
    assert_eq!(
        app.act(held(Some(Button::Right), Some(moved))),
        Effect::Redraw
    );
    assert_eq!(app.loupe().map(|loupe| loupe.eye), Some(moved));
    assert_eq!(app.act(held(None, None)), Effect::Redraw);
    assert_eq!(app.loupe(), None);
    assert_eq!(app.act(held(None, None)), Effect::Nothing);

    // The middle button holds nothing by default, and the loupe stays
    // down under it; given the loupe in the configuration, it holds it
    // up as the right does.
    assert_eq!(
        app.act(held(Some(Button::Middle), Some(middle))),
        Effect::Redraw
    );
    assert_eq!(app.loupe(), None);
    let _ = app.act(held(None, None));
    let mut gestures = Gestures::table();
    gestures.set(
        crate::gestures::Slot::read("image.middle.hold").unwrap(),
        crate::gestures::Behavior::Hold(crate::gestures::HoldAction::Loupe),
    );
    app.gestures = Rc::new(gestures);
    let _ = app.act(held(Some(Button::Middle), Some(middle)));
    assert_eq!(app.loupe().map(|loupe| loupe.eye), Some(middle));
    let _ = app.act(held(None, None));
    assert_eq!(app.loupe(), None);
}

/// The configuration's gestures, each as the application answers it: a
/// wheel with no slot does nothing, as Ctrl with the wheel does by
/// default; a stepper adds a trackpad's fractions up to whole notches;
/// the wheel set to pan pans by both of its deltas; and the side
/// button's click is the key it names.
#[test]
fn the_wheel_and_the_clicks_do_what_their_slots_say() {
    use crate::gestures::{Behavior, Button, Kind, Slot, WheelAction};
    use crate::ui::Command;
    use input::Action;

    let (mut app, dir) = app_over("gestures", &[("a.png", 64, 48), ("b.png", 64, 48)]);
    app.headless = Some(WINDOW);
    let wheel = |x, y, notched| Command::Wheel {
        delta: [x, y],
        notched,
        held: None,
    };

    // Ctrl with the wheel has no slot, and does nothing.
    let (image, viewport) = (app.image_size(), app.viewport());
    let before = app.view.position(image, viewport);
    app.pointer.modifiers = winit::keyboard::ModifiersState::CONTROL;
    assert_eq!(app.act(wheel(0.0, 1.0, true)), Effect::Nothing);
    assert_eq!(app.view.position(image, viewport), before);
    assert!(app.motion.is_none());

    // Given the exposure, it steps a quarter stop a whole notch at a
    // time, a trackpad's fractions added up until there is one.
    let mut gestures = Gestures::table();
    gestures.set(
        Slot::read("image.ctrl+wheel").unwrap(),
        Behavior::Wheel(WheelAction::Exposure),
    );
    gestures.set(
        Slot::read("image.wheel").unwrap(),
        Behavior::Wheel(WheelAction::Pan),
    );
    app.gestures = Rc::new(gestures);
    let stops = |app: &App| {
        app.current
            .as_ref()
            .map(|current| current.display.exposure_stops())
    };
    let start = stops(&app);
    assert_eq!(app.act(wheel(0.0, 1.0, true)), Effect::Redraw);
    let one = stops(&app);
    assert_ne!(one, start);
    assert_eq!(app.act(wheel(0.0, 0.4, false)), Effect::Nothing);
    assert_eq!(app.act(wheel(0.0, 0.4, false)), Effect::Nothing);
    assert_eq!(stops(&app), one);
    assert_eq!(app.act(wheel(0.0, 0.4, false)), Effect::Redraw);
    assert_ne!(stops(&app), one);
    // Down steps it back, from the fifth of a notch left over.
    assert_eq!(app.act(wheel(0.0, -1.0, true)), Effect::Nothing);
    assert_eq!(app.act(wheel(0.0, -1.2, true)), Effect::Redraw);
    assert_eq!(stops(&app), start);

    // The wheel alone pans, by both of its deltas: the picture follows
    // the wheel as it follows a drag.
    app.pointer.modifiers = winit::keyboard::ModifiersState::empty();
    let _ = app.perform(Action::ZoomTo(16.0));
    app.motion = None;
    let (image, viewport) = (app.image_size(), app.viewport());
    let mut dragged = app.view;
    let by = ui::WHEEL_PIXELS_PER_STEP * app.pixels_per_point();
    dragged.pan_by(-by, by, image, viewport);
    assert_ne!(
        dragged.position(image, viewport),
        app.view.position(image, viewport)
    );
    assert_eq!(app.act(wheel(1.0, -1.0, false)), Effect::Redraw);
    assert_eq!(
        app.view.position(image, viewport),
        dragged.position(image, viewport)
    );

    // The side button goes back to the file shown before, as its key
    // does; a click with no slot does nothing.
    let _ = app.perform(Action::NextFile);
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.index(), 1);
    assert_eq!(
        app.act(Command::Click(Button::Middle, Kind::Click)),
        Effect::Nothing
    );
    let _ = app.act(Command::Click(Button::Back, Kind::Click));
    answer(&mut app, Reload::Fresh);
    assert_eq!(app.files.index(), 0);

    // A double click of the primary is actual size, as `1` is.
    let _ = app.perform(Action::CycleFit);
    app.motion = None;
    let (image, viewport) = (app.image_size(), app.viewport());
    assert_ne!(app.view.zoom(image, viewport), 1.0);
    assert_eq!(
        app.act(Command::Click(Button::Left, Kind::DoubleClick)),
        Effect::Redraw
    );
    assert_eq!(app.view.zoom(image, viewport), 1.0);

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// The interface's scale steps a rung at a time by its keys, stops at the
/// top of the ladder owing no frame, comes back to the monitor's own on
/// the reset, and is what the state file is handed as the loop ends.
#[test]
fn the_interface_scale_steps_by_key_and_is_kept() {
    use input::Action::{ScaleDown, ScaleReset, ScaleUp};
    use ui::scale::{DEFAULT, SCALES};

    let (mut app, dir) = app_over("ui-scale", &[("a.png", 64, 48)]);
    app.headless = Some(WINDOW);
    assert_eq!(app.ui_scale, DEFAULT);
    assert_eq!(app.perform(ScaleUp), Effect::Redraw);
    assert_eq!(app.perform(ScaleUp), Effect::Redraw);
    assert_eq!(app.ui_scale, SCALES[3]);
    assert_eq!(app.pixels_per_point(), SCALES[3]);
    assert_eq!(
        app.toasts.showing().map(|toast| toast.message.as_str()),
        Some("Interface scale: 150%")
    );
    assert_eq!(app.perform(ScaleDown), Effect::Redraw);
    assert_eq!(app.ui_scale, SCALES[2]);
    assert_eq!(app.kept_state().ui_scale, SCALES[2]);
    assert_eq!(app.perform(ScaleReset), Effect::Redraw);
    assert_eq!(app.ui_scale, DEFAULT);
    assert_eq!(app.perform(ScaleReset), Effect::Nothing);

    // At the top of the ladder there is nowhere to go.
    while app.perform(ScaleUp) == Effect::Redraw {}
    assert_eq!(app.ui_scale, SCALES[SCALES.len() - 1]);
    assert_eq!(app.perform(ScaleUp), Effect::Nothing);
    assert_eq!(app.kept_state().ui_scale, SCALES[SCALES.len() - 1]);

    // The window is the same size in device pixels; the interface has
    // fewer points of it to lay out in.
    let logical = app.logical_size();
    assert_eq!(logical, WINDOW.map(|side| side / SCALES[SCALES.len() - 1]));

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// A trackpad's scroll arrives in the interface's points and is turned back
/// into device pixels by the same scale, so the picture follows the fingers
/// as far at any interface scale: a scroll of so many device pixels pans the
/// picture by that many.
#[test]
fn the_wheel_pans_as_far_at_any_interface_scale() {
    use crate::gestures::{Behavior, Slot, WheelAction};
    use crate::ui::Command;

    let (mut app, dir) = app_over("ui-scale-wheel", &[("a.png", 64, 48)]);
    app.headless = Some(WINDOW);
    let mut gestures = Gestures::table();
    gestures.set(
        Slot::read("image.wheel").unwrap(),
        Behavior::Wheel(WheelAction::Pan),
    );
    app.gestures = Rc::new(gestures);
    app.pointer.modifiers = winit::keyboard::ModifiersState::empty();
    let _ = app.perform(input::Action::ZoomTo(16.0));
    app.motion = None;
    let device = [30.0, -20.0];
    for ui_scale in [1.0, 2.0] {
        let _ = app.rescale(ui_scale);
        let (image, viewport) = (app.image_size(), app.viewport());
        let mut expected = app.view;
        expected.pan_by(-device[0], -device[1], image, viewport);
        let points = device.map(|pixels| pixels / app.pixels_per_point());
        let delta = points.map(|points| points / ui::WHEEL_PIXELS_PER_STEP);
        assert_eq!(
            app.act(Command::Wheel {
                delta,
                notched: false,
                held: None,
            }),
            Effect::Redraw
        );
        let (got, wanted) = (
            app.view.position(image, viewport),
            expected.position(image, viewport),
        );
        assert!(
            (got.u[0] - wanted.u[0]).abs() < 1e-3
                && (got.u[1] - wanted.u[1]).abs() < 1e-3
                && got.v == wanted.v,
            "{ui_scale}: {got:?} {wanted:?}"
        );
    }

    std::fs::remove_dir_all(dir).expect("we just wrote it");
}

/// What arrives is named by how it stands to what is up: read again at
/// its size or at another; another file of the same size or of
/// another, whether or not anything is up. A step is a move between
/// files, which a file read again is not, whatever it has become.
#[test]
fn an_arrival_is_named_by_what_it_stands_to() {
    let a = Path::new("a.png");
    let b = Path::new("b.png");
    let small = [4.0, 3.0];
    let large = [8.0, 6.0];
    let cases = [
        // The other rendering is its own arrival at any size.
        (
            Reload::Rendering,
            a,
            large,
            Some((a, small)),
            Arrival::Rerendered,
            false,
        ),
        (
            Reload::Rendering,
            a,
            small,
            Some((a, small)),
            Arrival::Rerendered,
            false,
        ),
        (
            Reload::InPlace,
            a,
            small,
            Some((a, small)),
            Arrival::Reread,
            false,
        ),
        (
            Reload::Page,
            a,
            small,
            Some((a, small)),
            Arrival::Reread,
            false,
        ),
        (
            Reload::InPlace,
            a,
            large,
            Some((a, small)),
            Arrival::Reshaped,
            false,
        ),
        (
            Reload::Fresh,
            b,
            small,
            Some((a, small)),
            Arrival::Beside,
            true,
        ),
        (
            Reload::Fresh,
            b,
            large,
            Some((a, small)),
            Arrival::Anew,
            true,
        ),
        (Reload::Fresh, b, large, None, Arrival::Anew, false),
        (
            Reload::Fresh,
            a,
            small,
            Some((a, small)),
            Arrival::Beside,
            false,
        ),
    ];
    for (mode, path, size, shown, expected, stepping) in cases {
        assert_eq!(
            arrival(mode, path, size, shown),
            (expected, stepping),
            "{mode:?} {path:?} {size:?} beside {shown:?}"
        );
    }
}

/// The paste button follows the clipboard and the interface together:
/// up while a picture is on the clipboard and the bars are showing,
/// down when either goes, and back when both are there again. Each
/// change owes a frame and nothing else does.
#[test]
fn the_paste_button_follows_the_clipboard_and_the_interface() {
    use input::Action::ToggleInterface;

    let (mut app, _dir) = app_over("paste", &[("a.png", 4, 3)]);
    assert!(!app.frame_input(WINDOW, 1.0).paste);
    assert_eq!(app.clipboard_changed(true), Effect::Redraw);
    assert!(app.frame_input(WINDOW, 1.0).paste);
    assert_eq!(app.clipboard_changed(true), Effect::Nothing);

    let _ = app.perform(ToggleInterface);
    assert!(!app.frame_input(WINDOW, 1.0).paste, "hidden with the rest");
    assert_eq!(app.clipboard_changed(false), Effect::Nothing);
    assert_eq!(app.clipboard_changed(true), Effect::Nothing);
    let _ = app.perform(ToggleInterface);
    assert!(app.frame_input(WINDOW, 1.0).paste, "back with the bars");

    assert_eq!(app.clipboard_changed(false), Effect::Redraw);
    assert!(!app.frame_input(WINDOW, 1.0).paste);
}

/// Which surface is wanted: the monitor's own where the compositor
/// says, and otherwise only what `--output hdr` asked for.
#[test]
fn the_surface_follows_the_monitor_and_then_the_switch() {
    use HdrPreference::{Follow, Off, On};
    for preference in [Follow, Off, On] {
        assert!(
            surface_wanted(Some(Mode::Hdr), preference),
            "{preference:?}"
        );
        assert_eq!(
            surface_wanted(Some(Mode::Sdr), preference),
            preference == On
        );
        assert_eq!(surface_wanted(None, preference), preference == On);
    }
}

/// Room above white takes all three: the surface with the room, a
/// monitor not known to be in SDR mode, and the switch not off.
#[test]
fn headroom_takes_the_surface_the_monitor_and_the_switch_together() {
    use HdrPreference::{Follow, Off, On};
    for monitor in [None, Some(Mode::Sdr), Some(Mode::Hdr)] {
        for preference in [Follow, Off, On] {
            assert_eq!(headroom_of(false, monitor, preference), Headroom::None);
            let expected = if monitor == Some(Mode::Sdr) || preference == Off {
                Headroom::None
            } else {
                Headroom::Above
            };
            assert_eq!(
                headroom_of(true, monitor, preference),
                expected,
                "{monitor:?} {preference:?}"
            );
        }
    }
}

/// The switch is dead for one of two reasons, and the reason it gives
/// is the one that holds: no HDR color space offered outranks the
/// monitor's mode, and a compositor that can say a monitor's mode and
/// has not is a monitor not in HDR mode.
#[test]
fn the_switch_says_why_it_is_dead() {
    for monitor in [None, Some(Mode::Sdr), Some(Mode::Hdr)] {
        for speaks in [false, true] {
            assert_eq!(hdr_state_of(false, speaks, monitor), Hdr::Unsupported);
        }
    }
    assert_eq!(hdr_state_of(true, false, None), Hdr::Available);
    assert_eq!(hdr_state_of(true, false, Some(Mode::Sdr)), Hdr::Available);
    assert_eq!(hdr_state_of(true, true, None), Hdr::NotInHdrMode);
    assert_eq!(hdr_state_of(true, true, Some(Mode::Sdr)), Hdr::NotInHdrMode);
    assert_eq!(hdr_state_of(true, true, Some(Mode::Hdr)), Hdr::Available);
}

/// The monitor's mode and room are read off the thread's table for the
/// monitor the window is on, kept, and re-read only when they change;
/// a change is a frame owed. Without a window there is no surface to
/// switch, so the switch stays dead whatever the monitor says, and
/// syncing the output changes nothing.
#[test]
fn a_monitor_change_is_noticed_once_and_kept() {
    let (mut app, _dir) = app_over("monitor", &[("a.png", 4, 3)]);
    let monitors = Monitors::stub(true);
    monitors.set("HDMI-A-1", Mode::Hdr, 4.0);
    app.output.monitors = Some(monitors);
    assert_eq!(app.sync_monitor(), Effect::Nothing, "not on a monitor yet");
    assert_eq!(app.output.mode, None);

    app.headless_monitor = Some("HDMI-A-1".to_string());
    assert_eq!(app.sync_monitor(), Effect::Redraw);
    assert_eq!(app.output.mode, Some(Mode::Hdr));
    assert_eq!(app.output.headroom, Some(4.0));
    assert_eq!(app.sync_monitor(), Effect::Nothing, "nothing moved");
    assert!(app.surface_hdr(), "the HDR surface is wanted");
    assert_eq!(app.hdr_state(), Hdr::Unsupported, "no surface to switch");
    assert_eq!(app.headroom(), Headroom::None);
    assert_eq!(app.sync_output(app.headroom()), Effect::Nothing);

    app.output
        .monitors
        .as_ref()
        .expect("still there")
        .set("HDMI-A-1", Mode::Sdr, 1.0);
    assert_eq!(app.sync_monitor(), Effect::Redraw);
    assert_eq!(app.output.mode, Some(Mode::Sdr));
    assert_eq!(app.output.headroom, Some(1.0));
    assert!(!app.surface_hdr());

    app.headless_monitor = Some("DP-2".to_string());
    assert_eq!(
        app.sync_monitor(),
        Effect::Redraw,
        "a monitor nothing has described"
    );
    assert_eq!(app.output.mode, None);
    assert_eq!(app.output.headroom, None);
}

/// A display whose room grows while the headroom stays above white — a
/// Mac's, ramping up from none once the window asks for room — has the
/// lift of a gain-mapped picture weighed again, not left at the weight
/// the picture arrived to.
#[test]
fn the_lift_follows_the_room_as_it_ramps() {
    use crate::image::gain_map::{GainMap, Lift};
    let (mut app, _dir) = app_over("ramp", &[("a.png", 4, 3)]);
    let current = app.current.as_mut().unwrap();
    let mut image = (*current.image).clone();
    image.gain_map = Some(Arc::new(GainMap {
        width: 2,
        height: 1,
        channels: 1,
        data: vec![0, 255],
        lift: Lift::Apple { headroom: 4.0 },
    }));
    current.image = Arc::new(image);
    app.headless_surface_hdr = true;
    app.output.monitors = Some(Monitors::stub(true));
    app.headless_monitor = Some("Built-in".to_string());
    let set = |app: &App, headroom| {
        app.output
            .monitors
            .as_ref()
            .expect("still there")
            .set("Built-in", Mode::Hdr, headroom);
    };
    let weight = |app: &App| {
        app.current
            .as_ref()
            .unwrap()
            .lift
            .as_ref()
            .map(|table| table.weight())
    };

    // The window lands on the display before it has ramped, and the
    // picture arrives to no room.
    set(&app, 1.0);
    assert_eq!(app.sync_monitor(), Effect::Redraw);
    assert_eq!(app.headroom(), Headroom::Above);
    app.refresh_lift();
    assert_eq!(weight(&app), Some(0.0), "no room yet");

    set(&app, 4.0);
    assert_eq!(app.sync_monitor(), Effect::Redraw);
    assert_eq!(weight(&app), Some(1.0), "the room ramped up");
}

/// The lift moves the moment the room does, and the numbers once the
/// thread has measured the picture through it; a report a later step
/// has overtaken is thrown away. The switch then moves between the
/// base and the lift without measuring anything again.
#[test]
fn the_numbers_follow_the_lift_off_the_loop() {
    use crate::image::gain_map::{GainMap, Lift};
    use std::sync::mpsc;
    use std::time::Duration;
    let (mut app, _dir) = app_over("measured", &[("a.png", 4, 3)]);
    let (send, receive) = mpsc::channel();
    let send = std::sync::Mutex::new(send);
    app.measuring = Measuring::new(Arc::new(move |measured| {
        let _ = send.lock().expect("not poisoned").send(measured);
    }));
    let next = || {
        receive
            .recv_timeout(Duration::from_secs(10))
            .expect("the thread reports")
    };
    let current = app.current.as_mut().unwrap();
    let mut image = (*current.image).clone();
    image.gain_map = Some(Arc::new(GainMap {
        width: 2,
        height: 1,
        channels: 1,
        data: vec![0, 255],
        lift: Lift::Apple { headroom: 4.0 },
    }));
    current.image = Arc::new(image);
    app.headless_surface_hdr = true;
    app.output.monitors = Some(Monitors::stub(true));
    app.headless_monitor = Some("Built-in".to_string());
    let set = |app: &App, headroom| {
        app.output
            .monitors
            .as_ref()
            .expect("still there")
            .set("Built-in", Mode::Hdr, headroom);
    };
    let max = |app: &App| app.current.as_ref().unwrap().stats.max;
    let weight = |app: &App| {
        app.current
            .as_ref()
            .unwrap()
            .lift
            .as_ref()
            .map(|table| table.weight())
    };

    set(&app, 1.0);
    let _ = app.sync_monitor();
    assert_eq!(weight(&app), Some(0.0));
    let base = max(&app);

    // Two steps of a ramp: the lift is at the second at once, and the
    // numbers are the base's until the second's measure is in.
    set(&app, 2.0);
    let _ = app.sync_monitor();
    let first = next();
    set(&app, 4.0);
    let _ = app.sync_monitor();
    assert_eq!(weight(&app), Some(1.0), "the lift is not kept waiting");
    assert_eq!(max(&app), base, "the numbers are");
    assert_eq!(app.measured(first), Effect::Nothing, "overtaken");
    assert_eq!(max(&app), base);
    assert_eq!(app.measured(next()), Effect::Redraw);
    let lifted = max(&app);
    assert!(lifted > base, "{lifted} over {base}");

    // The switch, off and on: each measure is there already.
    let _ = app.toggle_hdr();
    assert_eq!(weight(&app), Some(0.0));
    assert_eq!(max(&app), base, "the base, kept");
    let _ = app.toggle_hdr();
    assert_eq!(weight(&app), Some(1.0));
    assert_eq!(max(&app), lifted, "the lift, kept");
    assert!(
        receive.recv_timeout(Duration::from_millis(200)).is_err(),
        "nothing measured again"
    );
}

/// Under `--output hdr` the surface is the HDR one from the start and
/// never moves, so the monitor's mode is all that moves the headroom:
/// read as SDR once the surface is up, the room is gone; read as HDR,
/// it is back. The switch moves it the same way, with the surface left
/// where it is. Each is a change the surface takes no part in, which is
/// why the headroom is read before the change rather than by
/// `sync_output` after it, and each is a frame owed. The curve is not
/// theirs to touch: a file opens with none, and one chosen stays chosen
/// through every one of them.
#[test]
fn the_room_follows_the_monitor_and_the_switch_and_the_curve_stays() {
    use crate::image::display::ToneMap;
    fn curve(app: &App) -> ToneMap {
        app.current
            .as_ref()
            .expect("a picture is up")
            .display
            .tone_map()
    }
    let (mut app, _dir) = app_over("headroom", &[("a.png", 4, 3)]);
    // A picture pushed above white — the fixture is mid gray, three
    // stops up — so that a curve would have something to be for.
    app.current.as_mut().unwrap().display.set_exposure(3.0);
    app.output.asked = HdrPreference::On;
    app.headless_surface_hdr = true;
    app.output.monitors = Some(Monitors::stub(true));
    let set = |app: &App, mode, headroom| {
        app.output
            .monitors
            .as_ref()
            .expect("still there")
            .set("HDMI-A-1", mode, headroom);
    };

    // The surface is up and the monitor not yet read: room by default.
    app.refresh_lift();
    assert_eq!(app.headroom(), Headroom::Above);
    assert_eq!(curve(&app), ToneMap::None);

    // Then read as SDR, which the compositor maps the surface down for.
    set(&app, Mode::Sdr, 1.0);
    app.headless_monitor = Some("HDMI-A-1".to_string());
    assert_eq!(app.sync_monitor(), Effect::Redraw);
    assert_eq!(app.headroom(), Headroom::None);
    assert_eq!(curve(&app), ToneMap::None, "clipped, and said so");

    // A curve chosen, and then the monitor switched into HDR mode —
    // full screen, say — and the switch pressed twice: the room moves
    // every time, and the choice stays.
    let _ = app.perform(input::Action::CycleToneMap);
    assert_eq!(curve(&app), ToneMap::Neutral);
    set(&app, Mode::Hdr, 4.0);
    assert_eq!(app.sync_monitor(), Effect::Redraw);
    assert_eq!(app.headroom(), Headroom::Above);
    assert_eq!(curve(&app), ToneMap::Neutral, "chosen");
    assert_eq!(app.toggle_hdr(), Effect::Redraw);
    assert_eq!(app.headroom(), Headroom::None);
    assert_eq!(curve(&app), ToneMap::Neutral, "the switch off");
    assert_eq!(app.toggle_hdr(), Effect::Redraw);
    assert_eq!(app.headroom(), Headroom::Above);
    assert_eq!(curve(&app), ToneMap::Neutral, "the switch on");
}

/// exiftool not found is looked for again when the configuration is read
/// again, even under the same name, and the tab waiting on it reads the
/// file at once; and the tab's copy button is dead until tags are in.
#[test]
fn exiftool_not_found_is_looked_for_again_on_reload() {
    let (mut app, dir) = app_over("tags-reload", &[("a.png", 8, 8)]);
    app.headless = Some(WINDOW);
    let mut config = options().config;
    config.exiftool = "/nonexistent/exiftool".to_string();
    let _ = app.reconfigure(config.clone(), None);
    let _ = app.press(ui::Control::Info);
    let _ = app.press(ui::Control::InfoTab(ui::tags::Tab::Tags));
    assert_eq!(app.tags.asked(), 1);
    assert!(!app.conditions().tags_in);
    assert!(
        ui::tooltip::disabled(
            ui::Tip::Control(ui::Control::TagsCopy(ui::tags::Table::Json)),
            app.conditions()
        )
        .is_some()
    );

    let _ = app.reconfigure(config.clone(), None);
    assert_eq!(app.tags.asked(), 2, "looked for again, and asked again");

    // Found, it is left alone.
    app.exiftool = exiftool::Program::at(Path::new("/nonexistent/exiftool"));
    let _ = app.reconfigure(config, None);
    assert!(app.exiftool.found());

    // Refresh looks again, as the reload does.
    let _ = app.press(ui::Control::TagsRefresh);
    assert!(!app.exiftool.found());
    let _ = std::fs::remove_dir_all(&dir);
}

/// `i` puts the information panel up on its curated facts and `I` on its
/// raw data, each taking it down where it is up on its own tab already and
/// switching tabs where it is up on the other; the button is the plain
/// toggle, keeping the tab.
#[test]
fn i_and_shift_i_each_put_up_their_own_tab() {
    use ui::tags::Tab;

    let (mut app, dir) = app_over("info-keys", &[("a.png", 8, 8)]);
    app.headless = Some(WINDOW);
    app.exiftool = exiftool::Program::at(Path::new("/nonexistent/exiftool"));
    let shown = |app: &App| {
        (
            app.panels.side == Some(ui::side::Side::Info),
            app.panels.info_tab,
        )
    };

    let _ = app.perform(input::Action::ToggleRawData);
    assert_eq!(shown(&app), (true, Tab::Tags));
    assert_eq!(app.tags.asked(), 1, "the raw data is read");
    let _ = app.perform(input::Action::ToggleInfo);
    assert_eq!(shown(&app), (true, Tab::Facts));
    let _ = app.perform(input::Action::ToggleInfo);
    assert_eq!(app.panels.side, None);
    let _ = app.perform(input::Action::ToggleRawData);
    let _ = app.perform(input::Action::ToggleRawData);
    assert_eq!(app.panels.side, None);

    let _ = app.press(ui::Control::Info);
    assert_eq!(shown(&app), (true, Tab::Tags), "the button keeps the tab");
    let _ = std::fs::remove_dir_all(&dir);
}
