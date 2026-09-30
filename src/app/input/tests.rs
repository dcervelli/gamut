use super::*;
use crate::gestures::spelled_here;

/// The free readings of the table, over the default keys.
fn names(tip: Tip) -> Option<String> {
    super::names(&Keymap::table(), tip)
}

fn hint(action: Action) -> Option<String> {
    super::hint(&Keymap::table(), action)
}

/// What a key asks for with no region selected, at the default keys.
fn action_for(key: &Key, position: PhysicalKey, mods: Mods) -> Option<Action> {
    Keymap::table().action_for(key, position, mods, false)
}

/// The same with a region selected.
fn with_region(key: &Key, position: PhysicalKey, mods: Mods) -> Option<Action> {
    Keymap::table().action_for(key, position, mods, true)
}

/// The camera's switch says which picture is up, and under it the one a
/// press switches to; and on a file with no JPEG, where the switch is not
/// drawn, the key is refused with the reason.
#[test]
fn the_camera_switch_says_what_a_press_brings_up() {
    let namer = |camera, conditions| Namer {
        path: String::new(),
        index: 0,
        count: 0,
        show_histogram: false,
        state: Vec::new(),
        keys: Rc::new(Keymap::table()),
        gestures: Rc::new(Gestures::table()),
        camera,
        conditions,
    };
    let lines = |namer: Namer| {
        namer
            .tooltip(Tip::Control(Control::CameraJpeg))
            .map(|tooltip| [tooltip.title, tooltip.hints].concat())
    };
    assert_eq!(
        lines(namer(Some(Rendering::Developed), Conditions::ALIVE)),
        Some(vec![
            "Showing developed picture".to_string(),
            "Switch to camera's JPEG (v)".to_string(),
        ])
    );
    assert_eq!(
        lines(namer(Some(Rendering::CameraJpeg), Conditions::ALIVE)),
        Some(vec![
            "Showing camera's JPEG".to_string(),
            "Switch to developed picture (v)".to_string(),
        ])
    );
    let without = Conditions {
        camera_jpeg: false,
        ..Conditions::ALIVE
    };
    assert_eq!(
        lines(namer(None, without)),
        Some(vec![ui::tooltip::NO_CAMERA_JPEG.to_string()])
    );
}

/// Every button in the chrome is named by the key that does the same job,
/// in that key's own words: there is one table, so a tooltip and `--help`
/// have nowhere to disagree about a binding.
#[test]
fn a_button_is_named_by_the_key_that_does_the_same_job() {
    let named = |widget| names(Tip::Control(widget));

    assert_eq!(
        named(Control::Previous).as_deref(),
        Some("Previous file ([, Page Up)")
    );
    assert_eq!(
        named(Control::Next).as_deref(),
        Some("Next file (], Page Down)")
    );
    assert_eq!(
        named(Control::Minimap).as_deref(),
        Some("Toggle the minimap (m)")
    );
    assert_eq!(
        named(Control::Histogram).as_deref(),
        Some("Toggle the histogram (h)")
    );
    assert_eq!(
        named(Control::Grid).as_deref(),
        Some("Toggle the pixel grid (g)")
    );
    assert_eq!(
        named(Control::Output).as_deref(),
        Some("Toggle HDR output, when monitor is capable (o)")
    );
    // With nothing known of the file, the camera's switch is named by its
    // key's row; with a raw on screen, by what a press brings up.
    assert_eq!(
        named(Control::CameraJpeg).as_deref(),
        Some("Toggle between the developed picture and the camera's JPEG (v)")
    );
    // The button in the corner is named by the plain press it makes; the
    // press with Shift is the line under it — see `App::tooltip`.
    assert_eq!(
        named(Control::Maximize).as_deref(),
        Some("Toggle the UI (`)")
    );

    // The one button no key reaches names itself, and has no key after
    // it to name.
    let zoom = named(Control::Zoom).expect("the readout names itself");
    assert!(!zoom.contains('('), "{zoom}");

    // The two items of the file menu that are not copies, by the keys
    // that do the same; the button that opens the menu names itself,
    // every item of it having a key of its own.
    assert_eq!(
        named(Control::Rename).as_deref(),
        Some("Rename the current file (F2)")
    );
    let delete = if cfg!(target_os = "macos") {
        "\u{2326}"
    } else {
        "Del"
    };
    assert_eq!(
        named(Control::Delete),
        Some(format!("Trash the current file ({delete})"))
    );
    // The key that takes a file off the list, and the pair at the head
    // of the list by the chords that do the same.
    assert_eq!(
        named(Control::Remove).as_deref(),
        Some("Remove the current file from the file list (\u{232b})")
    );
    let alt = |key| crate::gestures::with_modifiers(ALT, key);
    assert_eq!(
        named(Control::Back),
        Some(format!(
            "Back in image history ({}, {})",
            alt("["),
            alt("Page Up")
        ))
    );
    assert_eq!(
        named(Control::Filmstrip).as_deref(),
        Some("Toggle the file list (Tab)")
    );
    // The menu at its head names itself, no key opening it; a cell of
    // it says what it puts the list in.
    let sorting = named(Control::Sorting).expect("the button names itself");
    assert!(!sorting.contains('('), "{sorting}");
    assert_eq!(
        named(Control::SortBy(ui::filmstrip::Sort::Area)).as_deref(),
        Some("Sort by total pixels")
    );
    let file = named(Control::FileMenu).expect("the button names itself");
    assert!(!file.contains('('), "{file}");

    assert_eq!(
        named(Control::Loupe).as_deref(),
        Some("Toggle the loupe (l)")
    );
}

/// Nothing in the chrome is left unnamed: a button with no tooltip is one
/// the pointer rests on for nothing. Every kind of control is asked,
/// less the few that wear their own words on screen — an item of the
/// open menu wears the program's name, a row of the chooser the file's,
/// a row of the information panel its fact, the dialog's buttons their
/// labels — and the two that are pressed through something else: the
/// timeline names itself as a whole, and the chooser is opened by a
/// press on the count, which has words of its own.
#[test]
fn every_chrome_button_has_something_to_say() {
    for widget in Control::ALL {
        let wordless = matches!(
            widget,
            Control::Opener(_)
                | Control::Choose(_)
                | Control::Facts(_)
                | Control::RenameTo
                | Control::CancelRename
                | Control::ExportAs(_)
                | Control::ExportTo
                | Control::CancelExport
                | Control::Seek(_)
                | Control::Chooser
                | Control::Thumb(_)
        );
        assert_eq!(
            names(Tip::Control(*widget)).is_some(),
            !wordless,
            "{widget:?}"
        );
    }
}

/// The turn's two buttons share one line of the key table, which names
/// both ways round at once; each button says its own way, with its own
/// key after it.
#[test]
fn the_turn_buttons_each_name_their_own_way_round() {
    assert_eq!(
        names(Tip::Control(Control::TurnLeft)).as_deref(),
        Some("Rotate left 90\u{b0} (;)")
    );
    assert_eq!(
        names(Tip::Control(Control::TurnRight)).as_deref(),
        Some("Rotate right 90\u{b0} (')")
    );
}

/// The two buttons in the middle of an empty window are named in their
/// own words and by the keys that put up the same dialog, and the
/// shortcut printed on each is that key: `Ctrl+O` for files, and the
/// same with Shift for a folder — one case each, as the two `C`s are.
#[test]
fn the_open_buttons_name_the_keys_that_open_the_dialog() {
    assert_eq!(
        names(Tip::Control(Control::OpenFiles)).as_deref(),
        Some(spelled_here("Choose image files to open (Ctrl+O)").as_str())
    );
    assert_eq!(
        names(Tip::Control(Control::OpenFolder)).as_deref(),
        Some(spelled_here("Choose a folder of images to open (Ctrl+Shift+O)").as_str())
    );
    let namer = Namer {
        path: String::new(),
        index: 0,
        count: 0,
        show_histogram: false,
        state: Vec::new(),
        keys: Rc::new(Keymap::table()),
        gestures: Rc::new(Gestures::table()),
        camera: None,
        conditions: Conditions::ALIVE,
    };
    assert_eq!(
        namer.shortcut(Control::OpenFiles).as_deref(),
        Some(spelled_here("Ctrl+O").as_str())
    );
    assert_eq!(
        namer.shortcut(Control::OpenFolder).as_deref(),
        Some(spelled_here("Ctrl+Shift+O").as_str())
    );
    assert_eq!(
        namer.shortcut(Control::Paste).as_deref(),
        Some(spelled_here("Ctrl+V").as_str())
    );

    // And the keys reach them: `o` with Ctrl, `O` with Ctrl and the
    // Shift the capital carries.
    let key = |text: &str, mods| {
        action_for(
            &Key::Character(text.into()),
            PhysicalKey::Code(KeyCode::KeyO),
            mods,
        )
    };
    assert_eq!(key("o", CTRL), Some(OpenFiles));
    assert_eq!(key("O", CTRL_SHIFT), Some(OpenFolder));
    assert_eq!(key("o", PLAIN), Some(ToggleHdr));
    let v = action_for(
        &Key::Character("v".into()),
        PhysicalKey::Code(KeyCode::KeyV),
        PLAIN,
    );
    assert_eq!(v, Some(ToggleCameraJpeg));

    // Dead while the dialog is up, and the label says so instead.
    let picking = Namer {
        path: String::new(),
        index: 0,
        count: 0,
        show_histogram: false,
        state: Vec::new(),
        keys: Rc::new(Keymap::table()),
        gestures: Rc::new(Gestures::table()),
        camera: None,
        conditions: Conditions {
            picking: true,
            ..Conditions::ALIVE
        },
    };
    let tooltip = picking
        .tooltip(Tip::Control(Control::OpenFiles))
        .expect("a reason");
    assert_eq!(tooltip.title, [ui::tooltip::DIALOG_UP]);
}

/// The help button is named in its own words and by both keys that open
/// the same popup, so that the tooltip on it teaches the keys.
#[test]
fn the_help_button_names_the_keys_that_open_it() {
    assert_eq!(
        names(Tip::Control(Control::Help)).as_deref(),
        Some("Keyboard shortcuts (?, /)")
    );
}

/// The help popup lays out the whole table and nothing else: every line
/// once, under the heading `--help` puts it under, in the order the
/// table keeps, and then the mouse. A condition is a phrase, not a
/// sentence: no capital at the front, no full stop at the end, and short
/// enough for its column.
#[test]
fn the_help_popup_shows_every_line_of_the_table_once() {
    let keys = Keymap::table();
    let gestures = Gestures::table();
    let sections = help_sections(&keys, &gestures, &Conditions::default());
    assert_eq!(sections.len(), Section::ALL.len() + 1);
    let rows: Vec<&ui::help::Row> = sections[..Section::ALL.len()]
        .iter()
        .flat_map(|section| section.rows.iter())
        .collect();
    assert_eq!(rows.len(), ROWS.len());
    for (row, line) in rows.iter().zip(ROWS) {
        assert_eq!(row.key, key_column(&keys, &gestures, line));
        assert_eq!(row.does, line.help);
        assert_eq!(
            row.when.map(|when| when.words),
            line.when.map(When::describe)
        );
        // Nothing holds, so every condition is marked unmet.
        assert_eq!(row.when.map(|when| when.met), line.when.map(|_| false));
    }
    for (section, listed) in Section::ALL.into_iter().zip(&sections) {
        assert_eq!(listed.title, section.title());
        assert!(!listed.rows.is_empty(), "{:?} has keys", section);
        assert!(ROWS.iter().filter(|row| row.section == section).count() == listed.rows.len());
    }
    // The mouse last, one row for each gesture that does something, the
    // side buttons named by the keys they run.
    let mouse = sections.last().expect("the mouse's section");
    assert_eq!(mouse.title, MOUSE);
    let row = |key: &str| {
        mouse
            .rows
            .iter()
            .find(|row| row.key == key)
            .map(|row| row.does)
    };
    assert_eq!(row("Drag"), Some("Pan, the image following the pointer"));
    assert_eq!(row("Back"), Some("Back in image history"));
    assert_eq!(
        row("Minimap: Drag"),
        Some("Center the view on the point under the pointer")
    );
    assert_eq!(mouse.rows.len(), 10);
    assert_eq!(
        row("Double-click"),
        Some("Toggle between 100% and fit whole image")
    );
    for when in When::ALL {
        let words = when.describe();
        assert!(
            words.starts_with(char::is_lowercase) && !words.ends_with('.'),
            "{when:?}: {words:?} reads as a phrase"
        );
        assert!(words.len() <= 32, "{when:?}: {words:?} fits its column");
    }
}

/// A line spelled from keys the configuration moved says what is bound
/// now, and a line whose keys are all unbound says nothing in its key
/// column rather than naming a key that does something else.
#[test]
fn the_help_popup_says_what_is_bound() {
    let mut keys = Keymap::table();
    let chord = |token| super::super::keymap::Chord::read(token).unwrap();
    keys.bind("files.undo", vec![chord("ctrl+e")]).unwrap();
    let sections = help_sections(&keys, &Gestures::table(), &Conditions::default());
    let key = |does: &str| {
        sections
            .iter()
            .flat_map(|section| &section.rows)
            .find(|row| row.does == does)
            .map(|row| row.key.clone())
    };
    assert_eq!(
        key("Undo the last rename, deletion or removal").as_deref(),
        Some(spelled_here("Ctrl+E").as_str())
    );
    assert_eq!(
        key("Export the picture as shown to a new JPG or PNG").as_deref(),
        Some("")
    );
    // A line no key reaches but a click does is keyed by the click.
    assert_eq!(
        key("Toggle between 100% and fit whole image").as_deref(),
        Some("Double-click")
    );
}

/// Each condition is answered from its own reading, and one reading
/// answers only the conditions that ask it — an animation is one where
/// a page is not, and a paged file is enough for the keys that step
/// through either.
#[test]
fn each_condition_is_met_by_its_own_reading() {
    let none = Conditions::default();
    for when in When::ALL {
        assert!(!none.met(when), "{when:?} with nothing to hold it");
    }
    let readings = [
        (
            When::RegionSelected,
            Conditions {
                region_selected: true,
                ..none
            },
        ),
        (
            When::SeveralFiles,
            Conditions {
                several_files: true,
                ..none
            },
        ),
        (
            When::Animation,
            Conditions {
                animation: true,
                ..none
            },
        ),
        (
            When::PointerOnPicture,
            Conditions {
                pointer_on_picture: true,
                ..none
            },
        ),
        (
            When::PictureOnClipboard,
            Conditions {
                picture_on_clipboard: true,
                ..none
            },
        ),
        (
            When::HdrMode,
            Conditions {
                hdr: Hdr::Available,
                ..none
            },
        ),
        (
            When::CameraJpeg,
            Conditions {
                camera_jpeg: true,
                ..none
            },
        ),
        (
            When::SingleChannel,
            Conditions {
                single_channel: true,
                ..none
            },
        ),
        (
            When::Undoable,
            Conditions {
                undoable: true,
                ..none
            },
        ),
        (
            When::VisitedBefore,
            Conditions {
                visited_before: true,
                ..none
            },
        ),
        (
            When::VisitedAfter,
            Conditions {
                visited_after: true,
                ..none
            },
        ),
        (
            When::Georeferenced,
            Conditions {
                georeferenced: true,
                ..none
            },
        ),
        (
            When::Geographic,
            Conditions {
                geographic: true,
                ..none
            },
        ),
    ];
    for (held, conditions) in readings {
        for when in When::ALL {
            let expected =
                when == held || (when == When::AnimationOrPages && held == When::Animation);
            assert_eq!(
                conditions.met(when),
                expected,
                "{held:?} read, {when:?} asked"
            );
        }
    }
    let paged = Conditions {
        pages: true,
        ..none
    };
    assert!(paged.met(When::AnimationOrPages));
    assert!(!paged.met(When::Animation));
}

/// The message raised when the interface goes names keys that really do
/// bring it back: the table's own words for the one that hid it, and the
/// Escape that takes things off. A message naming a key that did nothing
/// would leave the reader with a window they could not get out of, so a
/// half nothing is bound to is left out, and with neither nothing is
/// said.
#[test]
fn the_message_about_a_hidden_interface_names_keys_that_restore_it() {
    let mut keys = Keymap::table();
    assert_eq!(
        restore_message(&keys).as_deref(),
        Some("Press ` or Esc to restore UI")
    );
    assert_eq!(
        action_for(&Key::Named(NamedKey::Escape), ELSEWHERE, PLAIN),
        Some(Dismiss)
    );
    keys.bind("interface.dismiss", Vec::new()).unwrap();
    assert_eq!(
        restore_message(&keys).as_deref(),
        Some("Press ` to restore UI")
    );
    keys.bind("interface.toggle", Vec::new()).unwrap();
    assert_eq!(restore_message(&keys), None);
}

/// A cell of the zoom menu is named in its own words — the key steps
/// through them all and so describes none of them — with the key that
/// reaches it after.
///
/// The numbered cells are named by the one key that goes to the same
/// zoom, not by the whole of the line that binds it: `2` is the answer to
/// what to press for 200%, and `2, 3, 4, 5` is not.
#[test]
fn a_menu_cell_is_named_in_its_own_words_and_by_the_key_that_reaches_it() {
    use crate::ui::menu::ZOOM_CHOICES;
    let named = |choice| names(Tip::Control(Control::ZoomTo(choice)));

    assert_eq!(
        named(ZOOM_CHOICES[0]).as_deref(),
        Some(spelled_here("Zoom to 10% (Shift+4)").as_str())
    );
    assert_eq!(
        named(ZOOM_CHOICES[3]).as_deref(),
        Some("Zoom to 100% (1, 0)")
    );
    assert_eq!(named(ZOOM_CHOICES[4]).as_deref(), Some("Zoom to 200% (2)"));
    assert_eq!(named(ZOOM_CHOICES[7]).as_deref(), Some("Zoom to 1600% (5)"));

    for choice in ZOOM_CHOICES {
        let words = named(choice).unwrap_or_else(|| panic!("{choice:?} is named"));
        assert!(words.ends_with(')'), "{words} says what to press");
    }
}

/// The histogram panel's buttons are named in the panel's own few words
/// — its labels are read across the plot, so they have a panel's width
/// and not a window's — and by the key that does the same job.
#[test]
fn the_histogram_panels_buttons_are_named_briefly_and_by_their_keys() {
    let named = |widget| names(Tip::Control(widget));

    assert_eq!(named(Control::Luma).as_deref(), Some("Luminance plane (j)"));
    assert_eq!(named(Control::Planes).as_deref(), Some("Color planes (k)"));
    assert_eq!(
        named(Control::Log).as_deref(),
        Some("Logarithmic counts (y)")
    );
    assert_eq!(
        named(Control::Marks).as_deref(),
        Some("Mark the clipped pixels (w)")
    );
    assert_eq!(
        named(Control::Reset).as_deref(),
        Some("Reset the display (z)")
    );

    // Every false color on offer, each by the name `--colormap` takes
    // for it, with the key that cycles to it.
    for (index, map) in Colormap::ALL.into_iter().enumerate() {
        let words = named(Control::Ramp(index)).unwrap_or_else(|| panic!("{map:?} is named"));
        assert!(words.ends_with("(r)"), "{words}");
        assert!(
            map == Colormap::Gray || words.to_lowercase().contains(map.label()),
            "{words} names {map:?}"
        );
    }
    assert_eq!(named(Control::Ramp(Colormap::ALL.len())), None);

    // The band and its handles name themselves, no one key doing what
    // a drag on them does; the keys that step the handles come under
    // them as hints — see `the_handles_say_which_keys_step_them`.
    assert_eq!(names(Tip::BlackPoint).as_deref(), Some("Black point"));
    assert_eq!(names(Tip::WhitePoint).as_deref(), Some("White point"));
    assert!(names(Tip::Window).is_some());
    assert!(names(Tip::Exposure).is_some());

    // And the rows that set a state name the state, with the key that
    // steps through the row after it.
    for (index, window) in histogram::WINDOWS.iter().enumerate() {
        let words = named(Control::Window(index)).unwrap_or_else(|| panic!("{window:?}"));
        assert!(words.ends_with("(e)"), "{words}");
    }
    assert_eq!(named(Control::Window(histogram::WINDOWS.len())), None);
    for (index, curve) in ToneMap::ALL.into_iter().enumerate() {
        let words = named(Control::Curve(index)).unwrap_or_else(|| panic!("{curve:?}"));
        assert!(words.ends_with("(t)"), "{words}");
        assert!(
            curve == ToneMap::None || words.to_lowercase().contains(curve.label()),
            "{words} names {curve:?}"
        );
    }
    assert_eq!(named(Control::Curve(ToneMap::ALL.len())), None);

    // Short enough to be read where they are drawn: beside a toggle, on
    // a panel one panel wide.
    for widget in [
        Control::Luma,
        Control::Planes,
        Control::Log,
        Control::Marks,
        Control::Reset,
        Control::Ramp(1),
        Control::Curve(1),
    ] {
        let words = named(widget).expect("named above");
        assert!(words.len() <= 32, "{words} is too long for the panel");
    }
    // The windows' are sentences, since the button wears two words that
    // need saying in full; they still have to fit under the panel.
    for index in 0..histogram::WINDOWS.len() {
        let words = named(Control::Window(index)).expect("named above");
        assert!(words.len() <= 56, "{words} is too long for the panel");
    }
}

/// The two handles under the plot are dragged, which no key does; what
/// the keys do is step the same handles, and the tooltip on each says
/// which pair. The band between them slides the window, which no key
/// does at all, so it says what it is and no more.
#[test]
fn the_handles_say_which_keys_step_them() {
    let namer = Namer {
        path: String::new(),
        index: 0,
        count: 1,
        show_histogram: true,
        state: Vec::new(),
        keys: Rc::new(Keymap::table()),
        gestures: Rc::new(Gestures::table()),
        camera: None,
        conditions: Conditions {
            openable: false,
            ..Conditions::ALIVE
        },
    };
    let tooltip = |tip| namer.tooltip(tip).expect("named");
    assert!(tooltip(Tip::Window).hints.is_empty());
    assert_eq!(
        tooltip(Tip::BlackPoint).hints,
        ["Black point down / up (a, s)"]
    );
    assert_eq!(
        tooltip(Tip::WhitePoint).hints,
        [spelled_here("White point down / up (Shift+A, Shift+S)")]
    );
    // And the exposure's slider names the keys that step it.
    assert_eq!(
        tooltip(Tip::Exposure).hints,
        ["Exposure down / up, a quarter stop (d, f)"]
    );
}

/// The dot at the head of the pixel readout names itself, and under that
/// the key that steps it on and the two copies that take what it is
/// showing away: they have no button anywhere, so that label is the only
/// place either of them is written down.
#[test]
fn the_pixel_readout_names_its_key_and_the_copies_that_have_none() {
    let namer = Namer {
        path: String::new(),
        index: 0,
        count: 1,
        show_histogram: false,
        state: Vec::new(),
        keys: Rc::new(Keymap::table()),
        gestures: Rc::new(Gestures::table()),
        camera: None,
        conditions: Conditions::ALIVE,
    };
    let tooltip = namer
        .tooltip(Tip::Control(Control::PixelFormat))
        .expect("named");
    assert_eq!(tooltip.title, ["Pixel options"]);
    assert_eq!(
        tooltip.hints,
        [
            spelled_here("Cycle pixel format: hex, decimal, mapped, depth (.)"),
            spelled_here("Copy pixel value under pointer (Ctrl+.)"),
            spelled_here("Copy coordinate of pixel under pointer (Ctrl+>)"),
        ]
    );

    // A map brings the coordinate's key, and one that can be a latitude
    // the key that writes it, each after the format's own.
    let map = Namer {
        conditions: Conditions {
            georeferenced: true,
            geographic: true,
            ..Conditions::ALIVE
        },
        ..namer
    };
    let tooltip = map
        .tooltip(Tip::Control(Control::PixelFormat))
        .expect("named");
    assert_eq!(
        tooltip.hints,
        [
            spelled_here("Cycle pixel format: hex, decimal, mapped, depth (.)"),
            spelled_here("Cycle coordinate: pixel, projected, geographic (,)"),
            spelled_here("Switch latitude and longitude: decimal, DMS (<)"),
            spelled_here("Copy pixel value under pointer (Ctrl+.)"),
            spelled_here("Copy coordinate of pixel under pointer (Ctrl+>)"),
        ]
    );
}

/// A cell of the menu of copies is named in words shorter than the key
/// table's sentence, and by the key that runs it.
///
/// The button that opens the menu names itself, no one key opening it.
#[test]
fn a_copy_cell_is_named_by_its_words_and_its_key() {
    let named = |copies| names(Tip::Control(Control::Copies(copies)));

    assert_eq!(
        named(Copies::Name).as_deref(),
        Some("Copy the name of the current file, without its path (c)")
    );
    assert_eq!(
        named(Copies::Path).as_deref(),
        Some(spelled_here("Copy the absolute path of the current file (Shift+C)").as_str())
    );

    for copies in Copies::ALL {
        let words = named(copies).unwrap_or_else(|| panic!("{copies:?} is named"));
        assert!(words.starts_with("Copy "), "{words}");
        assert!(words.ends_with(')'), "{words} says what to press");
    }

    // The menu prints the key beside each item, and it is the key the
    // table binds to the same copy.
    let namer = Namer {
        path: String::new(),
        index: 0,
        count: 1,
        show_histogram: false,
        state: Vec::new(),
        keys: Rc::new(Keymap::table()),
        gestures: Rc::new(Gestures::table()),
        camera: None,
        conditions: Conditions::ALIVE,
    };
    assert_eq!(
        namer.shortcut(Control::Copies(Copies::Path)).as_deref(),
        Some(spelled_here("Shift+C").as_str())
    );
    assert_eq!(
        namer.shortcut(Control::Copies(Copies::Image)).as_deref(),
        Some(spelled_here("Ctrl+C").as_str())
    );

    // And the button it hangs from says what the menu is of, no one key
    // doing that job.
    let button = names(Tip::Control(Control::Copy)).expect("the button names itself");
    assert!(!button.contains('('), "{button}");
}

/// Every copy the menu offers is a key as well, which is what lets a cell
/// be named by the key table — and what keeps the two ways of asking for
/// the same copy from drifting apart.
#[test]
fn every_copy_on_the_menu_is_bound_to_a_key() {
    let mut actions = Vec::new();
    for what in Copies::ALL {
        let action = copy_action(what);
        assert!(
            Keymap::table().row_for(action).is_some(),
            "{what:?} is bound"
        );
        assert!(!actions.contains(&action), "{what:?} twice");
        actions.push(action);
    }
}

/// A cell of the pixel menu is named by what it answers rather than by
/// what it is called — the cell is already wearing the name — with the key
/// that steps through them after it.
#[test]
fn a_pixel_format_cell_says_which_question_it_answers() {
    for format in ui::PixelFormat::ALL {
        let words = names(Tip::Control(Control::Format(format)))
            .unwrap_or_else(|| panic!("{format:?} is named"));
        assert!(words.ends_with("(.)"), "{words}");
    }
}

/// The full stop is three bindings, told apart by what is held with it:
/// alone it steps the readout on, and with Ctrl the two of them copy what
/// it is showing. Shift is part of the character, so the shifted copy
/// arrives as `>` with it held.
#[test]
fn the_full_stop_reads_out_a_pixel_three_ways() {
    use winit::keyboard::SmolStr;
    let stop = Key::Character(SmolStr::new("."));
    let greater = Key::Character(SmolStr::new(">"));

    assert_eq!(action_for(&stop, ELSEWHERE, PLAIN), Some(CyclePixelFormat));
    assert_eq!(action_for(&stop, ELSEWHERE, CTRL), Some(CopyPixelValue));
    assert_eq!(
        action_for(&greater, ELSEWHERE, CTRL | SHIFT),
        Some(CopyPixelCoordinate)
    );
    // Shifted and unheld it is a greater-than and nothing else.
    assert_eq!(action_for(&greater, ELSEWHERE, SHIFT), None);
}

/// The keys the top bar's own words stand for are all bound, so neither
/// readout is left pointing at a key that does not exist.
#[test]
fn the_bars_own_readouts_have_keys_to_name() {
    for action in [CopyPath, NextFile, PreviousFile, OpenChooser] {
        let hint = hint(action).unwrap_or_else(|| panic!("{action:?} is bound"));
        assert!(hint.ends_with(')'), "{hint}");
    }
}

/// The count in the top bar says which file this is, then that a press
/// on it opens the chooser — with the key that does the same — and
/// then the keys that step through the list instead.
#[test]
fn the_count_says_where_it_is_and_what_a_press_on_it_opens() {
    let namer = Namer {
        path: String::new(),
        index: 2,
        count: 12,
        show_histogram: false,
        state: Vec::new(),
        keys: Rc::new(Keymap::table()),
        gestures: Rc::new(Gestures::table()),
        camera: None,
        conditions: Conditions::ALIVE,
    };
    let tooltip = namer
        .tooltip(Tip::Counter)
        .expect("the count has a tooltip");
    assert_eq!(tooltip.title, ["File 3 of 12"]);
    assert_eq!(
        tooltip.hints,
        [
            spelled_here("Click to choose a file from the list (Ctrl+P)"),
            spelled_here("Next file (], Page Down)"),
            spelled_here("Previous file ([, Page Up)"),
        ]
    );
}

/// A chord bound twice in one context would do whichever came first,
/// silently. The same key under different modifiers is a different
/// chord; the same chord under a region's name and a plain one is two
/// contexts, the region's tried first.
#[test]
fn no_chord_is_bound_twice_to_different_actions() {
    let mut seen: Vec<(Chord, Option<super::super::keymap::Context>, &str)> = Vec::new();
    for row in ROWS {
        let Keys::Bound(binds) = row.keys else {
            continue;
        };
        let context = row.when.and_then(When::context);
        for bound in binds {
            for chord in bound.defaults {
                if let Some((.., first)) = seen
                    .iter()
                    .find(|(each, held, _)| each == chord && *held == context)
                {
                    panic!("{chord:?} is both {first} and {}", bound.name);
                }
                seen.push((*chord, context, bound.name));
            }
        }
    }
}

/// A key described on a region's line as well as its own is named by
/// its own: what asks is a button that does the plain thing, and a
/// line that only describes never answers.
#[test]
fn a_key_with_a_region_line_is_named_by_its_plain_line() {
    let keys = Keymap::table();
    assert_eq!(
        keys.row_for(CopyImage).map(|row| row.help),
        Some("Copy the image as displayed")
    );
    assert_eq!(
        keys.row_for(CycleFit).map(|row| row.section),
        Some(Section::Zoom)
    );
    assert_eq!(keys.row_for(ToggleRegion).map(|row| row.when), Some(None));
    // A key only a region answers is still found.
    assert_eq!(
        keys.row_for(ShrinkRegion(Left)).map(|row| row.section),
        Some(Section::Region)
    );
}

/// Shift belongs to the character, not to the chord: a chord on a
/// character that asked for it as well would never match, since the
/// lookup takes it out of what is held before comparing. Only a key
/// Shift does not change — one bound by name or by position — may ask
/// for it.
#[test]
fn only_layout_free_keys_are_bound_with_shift() {
    for row in ROWS {
        let Keys::Bound(binds) = row.keys else {
            continue;
        };
        for bound in binds {
            for chord in bound.defaults {
                assert!(
                    !chord.mods.shift_key() || !matches!(chord.key, KeyName::Char(_)),
                    "{} asks for Shift on {:?}; say it with the character instead",
                    bound.name,
                    chord.key
                );
            }
        }
    }
}

/// A key whose position the table does not care about. Every binding but
/// the number row's is made against the character or the name, so what is
/// under the key is beside the point.
const ELSEWHERE: PhysicalKey = PhysicalKey::Code(KeyCode::F13);

#[test]
fn keys_resolve_to_their_actions() {
    use winit::keyboard::SmolStr;
    let plain = |text: &str| action_for(&Key::Character(SmolStr::new(text)), ELSEWHERE, PLAIN);
    assert_eq!(plain("q"), Some(Quit));
    // The same line of `--help`, and not the same action: Escape takes
    // off what is up before it means anything else, and `q` leaves.
    assert_eq!(
        action_for(&Key::Named(NamedKey::Escape), ELSEWHERE, PLAIN),
        Some(Dismiss)
    );
    assert_eq!(
        action_for(&Key::Named(NamedKey::PageDown), ELSEWHERE, PLAIN),
        Some(NextFile)
    );
    assert_eq!(plain("]"), Some(NextFile));
    assert_eq!(plain("F"), Some(Exposure(EV_STEP)));
    // The turns, on the two keys at the end of the home row.
    assert_eq!(plain(";"), Some(TurnLeft));
    assert_eq!(plain("'"), Some(TurnRight));
    // What is done to the file: one named key for the trash, one that
    // takes the file off the list, one for the dialog, and the undo
    // under Ctrl — the plain `z` being the display's reset.
    assert_eq!(
        action_for(&Key::Named(NamedKey::Delete), ELSEWHERE, PLAIN),
        Some(Delete)
    );
    assert_eq!(
        action_for(&Key::Named(NamedKey::Backspace), ELSEWHERE, PLAIN),
        Some(Remove)
    );
    // The file list, and the files seen, under the keys that step the
    // list held with Alt — a character ignoring the Shift it carries,
    // a named key held with exactly Alt.
    assert_eq!(
        action_for(&Key::Named(NamedKey::Tab), ELSEWHERE, PLAIN),
        Some(ToggleFilmstrip)
    );
    assert_eq!(
        action_for(&Key::Character(SmolStr::new("[")), ELSEWHERE, ALT),
        Some(Back)
    );
    assert_eq!(
        action_for(&Key::Character(SmolStr::new("]")), ELSEWHERE, ALT | SHIFT),
        Some(Forward)
    );
    assert_eq!(
        action_for(&Key::Named(NamedKey::PageUp), ELSEWHERE, ALT),
        Some(Back)
    );
    assert_eq!(
        action_for(&Key::Named(NamedKey::PageDown), ELSEWHERE, ALT),
        Some(Forward)
    );
    assert_eq!(
        action_for(&Key::Named(NamedKey::PageDown), ELSEWHERE, CTRL | ALT),
        None
    );
    assert_eq!(
        action_for(&Key::Named(NamedKey::F2), ELSEWHERE, PLAIN),
        Some(Rename)
    );
    assert_eq!(
        action_for(&Key::Character(SmolStr::new("z")), ELSEWHERE, CTRL),
        Some(Undo)
    );
    assert_eq!(plain("z"), Some(ResetDisplay));
    // The window's position and its width are the same two keys in
    // different cases.
    assert_eq!(plain("a"), Some(StepBlack(-0.05)));
    assert_eq!(plain("A"), Some(StepWhite(-0.05)));
    assert_eq!(plain("w"), Some(MarkClipped));
    assert_eq!(plain("W"), Some(MarkClipped));
    assert_eq!(plain("u"), None);
    assert_eq!(plain("x"), Some(ToggleRegion));
    assert_eq!(plain("X"), Some(ToggleRegion));
    // The backquote and the tilde are the same key, and Shift is the
    // difference between hiding the bars and clearing the screen.
    assert_eq!(plain("`"), Some(ToggleInterface));
    assert_eq!(
        action_for(&Key::Character(SmolStr::new("~")), ELSEWHERE, Mods::SHIFT),
        Some(ToggleInterfaceAndPanels)
    );
}

/// The three pan distances are one key held three ways, and a named key
/// takes Shift as a modifier: the plain binding must not answer for the
/// shifted press as well. With a region selected the region's names
/// hold the same arrows, and are tried first; the fine pan has no
/// region name on its chord, and pans under a region as without one.
#[test]
fn the_arrows_pan_by_what_is_held_with_them() {
    let left = Key::Named(NamedKey::ArrowLeft);
    let held = |mods| action_for(&left, ELSEWHERE, mods);
    assert_eq!(held(PLAIN), Some(Pan(Left, Coarse)));
    assert_eq!(held(SHIFT), Some(Pan(Left, Fine)));
    assert_eq!(held(CTRL), Some(Pan(Left, Edge)));
    assert_eq!(held(CTRL | SHIFT), None);
    assert_eq!(held(CTRL | SHIFT | Mods::ALT), None);
    let region = |mods| with_region(&left, ELSEWHERE, mods);
    assert_eq!(region(PLAIN), Some(MoveRegion(Left)));
    assert_eq!(region(SHIFT), Some(Pan(Left, Fine)));
    assert_eq!(region(CTRL), Some(GrowRegion(Left)));
    assert_eq!(region(CTRL | SHIFT), Some(ShrinkRegion(Left)));
    assert_eq!(region(CTRL | SHIFT | Mods::ALT), None);
    // Escape is not bound with Shift, and so does not answer to it.
    assert_eq!(
        action_for(&Key::Named(NamedKey::Escape), ELSEWHERE, SHIFT),
        None
    );
}

/// The number row answers to where it is rather than to what it types, so
/// that Shift+`2` is 50% on a keyboard that puts `@` there and on one that
/// puts `"` there. The character reported alongside is ignored: here it is
/// the one a French layout sends, which is neither.
#[test]
fn the_zoom_digits_go_by_position_rather_than_character() {
    use winit::keyboard::SmolStr;
    let two = PhysicalKey::Code(KeyCode::Digit2);
    let typed = Key::Character(SmolStr::new("é"));
    assert_eq!(action_for(&typed, two, PLAIN), Some(ZoomTo(2.0)));
    assert_eq!(
        action_for(&Key::Character(SmolStr::new("2")), two, SHIFT),
        Some(ZoomTo(0.5))
    );
    // `1` is the whole of that key: nothing hangs off it under Shift.
    assert_eq!(
        action_for(&typed, PhysicalKey::Code(KeyCode::Digit1), SHIFT),
        None
    );
    // And the character on its own reaches nothing, wherever it came from.
    assert_eq!(
        action_for(&Key::Character(SmolStr::new("@")), ELSEWHERE, PLAIN),
        None
    );
}

/// The four things `c` does are told apart by what is held with it,
/// and a chord nothing binds is still left to the window manager.
#[test]
fn modifiers_tell_chords_apart() {
    use winit::keyboard::SmolStr;
    // Shift is what turns the character upper case in the first place,
    // so it is held for every reading of `C`.
    let lower = Key::Character(SmolStr::new("c"));
    let upper = Key::Character(SmolStr::new("C"));
    // The lower case on its own is the shortest of the copies.
    assert_eq!(action_for(&lower, ELSEWHERE, PLAIN), Some(CopyName));
    assert_eq!(action_for(&upper, ELSEWHERE, Mods::SHIFT), Some(CopyPath));
    assert_eq!(
        action_for(&upper, ELSEWHERE, Mods::CONTROL | Mods::SHIFT),
        Some(CopyUri)
    );
    // The same capitals under Caps Lock, which reports no Shift at all.
    assert_eq!(action_for(&upper, ELSEWHERE, PLAIN), Some(CopyPath));
    assert_eq!(action_for(&upper, ELSEWHERE, CTRL), Some(CopyUri));
    assert_eq!(
        action_for(&lower, ELSEWHERE, Mods::CONTROL),
        Some(CopyImage)
    );
    // Chords the table does not bind belong to the window manager.
    assert_eq!(action_for(&lower, ELSEWHERE, Mods::ALT), None);
    assert_eq!(
        action_for(&upper, ELSEWHERE, Mods::CONTROL | Mods::ALT),
        None
    );
    assert_eq!(
        action_for(
            &Key::Character(SmolStr::new("0")),
            PhysicalKey::Code(KeyCode::Digit0),
            Mods::SUPER
        ),
        None
    );
}
