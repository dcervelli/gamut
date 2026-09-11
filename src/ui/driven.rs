//! The interface driven without a window: laid out by egui headless, pressed
//! through the accessibility tree it builds, and read back as the commands
//! it hands the application.

use std::sync::Arc;

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

use crate::image::display::{Display, Headroom, Startup};
use crate::image::exif::Exif;
use crate::image::{AlphaMode, Channels, ColorSpace, DecodedImage, Samples, Stats};
use crate::theme::Theme;
use crate::view::{View, Viewport};

use crate::image::region::{Grip, Region};

use super::chrome::{BAR_HEIGHT, SIDE_WIDTH};
use super::control::Unnamed;
use super::menu::Copies;
use super::{
    Command, Control, Current, FileFacts, FrameInput, Grab, PANELS_ROOM, Panels, PixelFormat,
    Selection,
};

/// A window with room for everything.
const WINDOW: [f32; 2] = [1000.0, 700.0];

/// What a test's interface is drawn from, and what it asked for.
struct State {
    /// Whether the fonts are in. egui's bundled faces know nothing of the
    /// bold family the file's name is set in, so the first pass binds it and
    /// draws nothing; every pass after that draws the interface.
    ready: bool,
    input: FrameInput,
    panels: Panels,
    current: Option<Current>,
    view: View,
    commands: Vec<Command>,
}

/// A small color photograph, on screen.
fn photograph() -> Current {
    picture(4, 3)
}

/// A gray color photograph of the given size, on screen.
fn picture(width: u32, height: u32) -> Current {
    let image = DecodedImage::new(
        width,
        height,
        Samples::U8 {
            channels: Channels::Rgb,
            data: vec![128; (width * height * 3) as usize],
        },
        ColorSpace::SRGB,
        AlphaMode::Opaque,
    );
    let stats = Stats::scan(&image);
    Current {
        display: Display::for_image_with(&image, &stats, Startup::default(), Headroom::None),
        image: Arc::new(image),
        stats,
        label: "photo.png".into(),
        file: FileFacts {
            path: "photo.png".into(),
            bytes: None,
            modified: None,
            reader: None,
        },
        exif: Exif::default(),
        stored: None,
    }
}

fn panels() -> Panels {
    Panels {
        show_ui: true,
        show_histogram: false,
        show_info: false,
        show_luma: true,
        show_planes: true,
        log_counts: false,
        show_minimap: true,
        show_grid: false,
        paste: false,
        pixel_format: PixelFormat::default(),
    }
}

fn input(logical: [f32; 2], count: usize) -> FrameInput {
    FrameInput {
        logical,
        scale: 1.0,
        viewport: Viewport::new(
            SIDE_WIDTH,
            BAR_HEIGHT,
            logical[0] - 2.0 * SIDE_WIDTH,
            logical[1] - 2.0 * BAR_HEIGHT,
        ),
        pointer: None,
        cursor: None,
        minimap_on_screen: false,
        reading: None,
        index: 0,
        count,
        deleted: false,
        headroom: Headroom::None,
        hdr_available: false,
        can_pan: false,
        openers: Vec::new(),
        toast: None,
        selection: Selection::Off,
        grabbing: None,
        over_region: false,
        box_zoom: false,
        zoom_box: None,
    }
}

/// The interface laid out in a window of `logical` pixels, over a list of
/// `count` files, from `panels`.
fn open(logical: [f32; 2], count: usize, panels: Panels) -> Harness<'static, State> {
    let mut harness = build(logical, count, panels);
    harness.run();
    harness
}

fn build(logical: [f32; 2], count: usize, panels: Panels) -> Harness<'static, State> {
    let state = State {
        ready: false,
        input: input(logical, count),
        panels,
        current: Some(photograph()),
        view: View::new(),
        commands: Vec::new(),
    };
    Harness::builder()
        .with_size(egui::vec2(logical[0], logical[1]))
        .build_ui_state(
            |ui, state: &mut State| {
                if !state.ready {
                    let mut fonts = egui::FontDefinitions::default();
                    let sans = fonts.families[&egui::FontFamily::Proportional].clone();
                    fonts
                        .families
                        .insert(egui::FontFamily::Name(super::fonts::BOLD.into()), sans);
                    ui.ctx().set_fonts(fonts);
                    super::style::apply(ui.ctx(), &Theme::FALLBACK);
                    state.ready = true;
                    return;
                }
                let commands = super::show(
                    ui,
                    &state.input,
                    &state.panels,
                    state.current.as_ref(),
                    &state.view,
                    &Theme::FALLBACK,
                    &Unnamed,
                );
                // The one thing the application says back to the interface
                // about a gesture, done here as `App::act` does it: which
                // hold a drag has, so that the frames of one drag all go the
                // same way.
                for command in &commands {
                    match command {
                        Command::Grab { grab, .. } => state.input.grabbing = Some(*grab),
                        Command::Release => state.input.grabbing = None,
                        _ => {}
                    }
                }
                state.commands.extend(commands);
            },
            state,
        )
}

/// Clicks the control called `label` and returns what the interface asked
/// for.
fn click(harness: &mut Harness<'static, State>, label: &str) -> Vec<Command> {
    harness.state_mut().commands.clear();
    harness
        .get_by_role_and_label(egui::accesskit::Role::Button, label)
        .click();
    harness.run();
    asked(harness)
}

/// What the interface asked for, leaving out what it says on every pass —
/// where the pointer is against the picture and its region — which is not
/// what a gesture is asked about.
fn asked(harness: &Harness<'static, State>) -> Vec<Command> {
    harness
        .state()
        .commands
        .iter()
        .copied()
        .filter(|command| !matches!(command, Command::OverImage(_) | Command::OverGrip(_)))
        .collect()
}

/// Presses the primary button at `from`, moves to `to`, and lets go there,
/// a pass to each, and returns what the interface asked for.
fn drag(harness: &mut Harness<'static, State>, from: [f32; 2], to: [f32; 2]) -> Vec<Command> {
    harness.state_mut().commands.clear();
    let (from, to) = (egui::pos2(from[0], from[1]), egui::pos2(to[0], to[1]));
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    harness.event(egui::Event::PointerMoved(from));
    harness.step();
    harness.event(button(from, true));
    harness.step();
    harness.event(egui::Event::PointerMoved(to));
    harness.step();
    harness.event(button(to, false));
    harness.step();
    harness.step();
    asked(harness)
}

/// Where a point of the window falls on the picture, in image pixels, as
/// the interface maps it.
fn image_point(harness: &Harness<'static, State>, at: [f32; 2]) -> [f32; 2] {
    let state = harness.state();
    let image = state.current.as_ref().expect("a picture is up").size();
    state
        .view
        .placement(image, state.input.viewport)
        .image_point(at)
}

/// And the other way: where a point of the picture is in the window.
fn screen_point(harness: &Harness<'static, State>, at: [f32; 2]) -> [f32; 2] {
    let state = harness.state();
    let image = state.current.as_ref().expect("a picture is up").size();
    state
        .view
        .placement(image, state.input.viewport)
        .screen_point(at)
}

/// A press on a toggle in the chrome comes back as a press on that control
/// and nothing else, and a key pressed afterwards is still the picture's:
/// no button keeps the focus it was clicked with.
#[test]
fn a_toggle_in_the_chrome_hands_back_its_press() {
    let mut harness = open(WINDOW, 1, panels());
    for (label, control) in [
        ("Histogram", Control::Histogram),
        ("Information", Control::Info),
        ("Minimap", Control::Minimap),
        ("Grid", Control::Grid),
        ("Maximize", Control::Maximize),
        ("Region", Control::Region),
    ] {
        assert_eq!(
            click(&mut harness, label),
            [Command::Press(control)],
            "{label}"
        );
        assert!(
            !harness.ctx.egui_wants_keyboard_input(),
            "{label} kept the keyboard"
        );
    }
}

/// A toggle whose panel the window cannot take is drawn dead and refuses
/// the press, rather than quietly setting something no one can see.
#[test]
fn a_toggle_with_no_room_for_its_panel_is_dead() {
    let small = [PANELS_ROOM[0] - 10.0 + 2.0 * SIDE_WIDTH, 200.0];
    let mut harness = open(small, 1, panels());
    for label in ["Histogram", "Information"] {
        let node = harness.get_by_label(label);
        assert!(node.accesskit_node().is_disabled(), "{label}");
    }
    assert_eq!(click(&mut harness, "Histogram"), []);

    let harness = open(WINDOW, 1, panels());
    assert!(
        !harness
            .get_by_label("Histogram")
            .accesskit_node()
            .is_disabled()
    );
}

/// The paste button is on screen only while the clipboard is holding a
/// picture, and the pair that steps through the list only while there is a
/// list to step through.
#[test]
fn buttons_that_would_do_nothing_are_not_there() {
    let harness = open(WINDOW, 1, panels());
    assert!(harness.query_by_label("Paste").is_none());
    assert!(harness.query_by_label("Previous file").is_none());
    assert!(harness.query_by_label("Next file").is_none());
    drop(harness);

    let mut pasteable = panels();
    pasteable.paste = true;
    let mut harness = open(WINDOW, 3, pasteable);
    assert_eq!(
        click(&mut harness, "Paste"),
        [Command::Press(Control::Paste)]
    );
    assert_eq!(
        click(&mut harness, "Next file"),
        [Command::Press(Control::Next)]
    );
    assert_eq!(
        click(&mut harness, "Previous file"),
        [Command::Press(Control::Previous)]
    );
}

/// The surface switch is dead where the driver offers nothing to switch
/// to.
#[test]
fn the_surface_switch_is_dead_without_an_hdr_output() {
    let harness = open(WINDOW, 1, panels());
    assert!(harness.get_by_label("HDR").accesskit_node().is_disabled());

    let mut harness = open(WINDOW, 1, panels());
    harness.state_mut().input.hdr_available = true;
    harness.run();
    assert_eq!(
        click(&mut harness, "HDR"),
        [Command::Press(Control::Output)]
    );
}

/// The copy button opens a menu of the copies on offer, and an item of it
/// asks for that copy.
#[test]
fn the_copy_menu_offers_every_copy() {
    let mut harness = open(WINDOW, 1, panels());
    assert!(harness.query_by_label("Path").is_none());
    assert_eq!(click(&mut harness, "Copy"), []);
    for copies in Copies::ALL {
        assert!(
            harness
                .query_by_role_and_label(egui::accesskit::Role::Button, copies.label())
                .is_some(),
            "{copies:?} is on the menu"
        );
    }
    assert_eq!(
        click(&mut harness, "Path"),
        [Command::Press(Control::Copies(Copies::Path))]
    );
    harness.run();
    assert!(
        harness.query_by_label("Path").is_none(),
        "the menu closes on the press"
    );
}

/// The open button offers whatever the desktop says can open this file, and
/// an item of it asks for that program by its place in the list. With
/// nothing offering, the button is dead rather than opening an empty menu.
#[test]
fn the_open_menu_offers_the_applications_that_can_open_the_file() {
    let mut harness = open(WINDOW, 1, panels());
    assert!(
        harness
            .get_by_label("Open with")
            .accesskit_node()
            .is_disabled()
    );
    assert_eq!(click(&mut harness, "Open with"), []);

    harness.state_mut().input.openers = vec!["Pinta".to_string(), "Darktable".to_string()];
    harness.run();
    assert!(
        !harness
            .get_by_label("Open with")
            .accesskit_node()
            .is_disabled()
    );
    assert_eq!(click(&mut harness, "Open with"), []);
    assert_eq!(
        click(&mut harness, "Darktable"),
        [Command::Press(Control::OpenIn(1))]
    );
    harness.run();
    assert!(
        harness.query_by_label("Darktable").is_none(),
        "the menu closes on the press"
    );
}

/// The zoom readout opens a menu of zooms, and a cell of it asks for that
/// zoom.
#[test]
fn the_zoom_menu_offers_the_zooms() {
    let mut harness = open(WINDOW, 1, panels());
    assert_eq!(click(&mut harness, "Zoom"), []);
    let pressed = click(&mut harness, "100%");
    assert_eq!(pressed.len(), 1);
    assert!(
        matches!(
            pressed[0],
            Command::Press(Control::ZoomTo(super::menu::ZoomChoice::Scale(scale))) if scale == 1.0
        ),
        "{pressed:?}"
    );
}

/// A drag on the picture is the view's until a region is asked for, and
/// then it is the region's: it begins where the button went down, in image
/// pixels, follows the hand, and ends when the button comes up — and the
/// view does not pan under it.
#[test]
fn a_drag_on_the_picture_is_the_regions_while_one_is_asked_for() {
    let mut harness = open(WINDOW, 1, panels());
    // Across the middle of the picture, from one pixel of it to another.
    let from = screen_point(&harness, [0.5, 0.5]);
    let to = screen_point(&harness, [2.5, 1.5]);

    let commands = drag(&mut harness, from, to);
    assert!(!commands.is_empty());
    assert!(
        commands
            .iter()
            .all(|command| matches!(command, Command::Drag(_))),
        "{commands:?}"
    );

    harness.state_mut().input.selection = Selection::Armed;
    harness.run();
    let commands = drag(&mut harness, from, to);
    let pressed = image_point(&harness, from);
    assert!(
        matches!(commands.first(), Some(Command::Grab { grab: Grab::New, at }) if *at == pressed),
        "{commands:?}"
    );
    assert!(
        commands
            .iter()
            .any(|command| matches!(command, Command::Pull(_))),
        "{commands:?}"
    );
    assert_eq!(commands.last(), Some(&Command::Release));
    assert!(
        !commands
            .iter()
            .any(|command| matches!(command, Command::Drag(_))),
        "the view did not pan: {commands:?}"
    );
}

/// With a region on screen, a drag from inside it takes hold of it, and a
/// drag from anywhere else on the picture pans as it always did.
#[test]
fn a_region_on_screen_is_taken_hold_of_from_inside_it() {
    let mut harness = open(WINDOW, 1, panels());
    // The middle pixel of a 4x3 picture fitted to the window: a good part
    // of the picture, with the picture's own margins outside it.
    let region = Region {
        x: 1,
        y: 1,
        width: 1,
        height: 1,
    };
    harness.state_mut().input.selection = Selection::Shown(region);
    harness.run();
    let inside = screen_point(&harness, [1.5, 1.5]);
    assert!(region.contains(image_point(&harness, inside)));

    let to = screen_point(&harness, [2.5, 2.5]);
    let commands = drag(&mut harness, inside, to);
    assert!(
        matches!(
            commands.first(),
            Some(Command::Grab {
                grab: Grab::Handle(Grip::Inside),
                ..
            })
        ),
        "{commands:?}"
    );
    assert_eq!(commands.last(), Some(&Command::Release));

    let outside = [60.0, 100.0];
    assert!(!region.contains(image_point(&harness, outside)));
    let commands = drag(&mut harness, outside, [90.0, 130.0]);
    assert!(
        commands
            .iter()
            .all(|command| matches!(command, Command::Drag(_))),
        "{commands:?}"
    );
}

/// With `Space` held, a drag on the picture draws a box to zoom to,
/// wherever it begins — a region under the press does not take it — and
/// the view does not pan.
#[test]
fn a_drag_with_space_held_draws_a_box_to_zoom_to() {
    let mut harness = open(WINDOW, 1, panels());
    let region = Region {
        x: 1,
        y: 1,
        width: 1,
        height: 1,
    };
    harness.state_mut().input.selection = Selection::Shown(region);
    harness.state_mut().input.box_zoom = true;
    harness.run();
    let inside = screen_point(&harness, [1.5, 1.5]);
    let to = screen_point(&harness, [3.5, 2.5]);
    let commands = drag(&mut harness, inside, to);
    let pressed = image_point(&harness, inside);
    assert!(
        matches!(commands.first(), Some(Command::Grab { grab: Grab::Zoom, at }) if *at == pressed),
        "{commands:?}"
    );
    assert!(
        commands
            .iter()
            .any(|command| matches!(command, Command::Pull(_))),
        "{commands:?}"
    );
    assert_eq!(commands.last(), Some(&Command::Release));
    assert!(
        !commands
            .iter()
            .any(|command| matches!(command, Command::Drag(_))),
        "the view did not pan: {commands:?}"
    );

    // The key up again, the same drag takes hold of the region as before.
    harness.state_mut().input.box_zoom = false;
    harness.run();
    let commands = drag(&mut harness, inside, to);
    assert!(
        matches!(
            commands.first(),
            Some(Command::Grab {
                grab: Grab::Handle(Grip::Inside),
                ..
            })
        ),
        "{commands:?}"
    );
}

/// The hand on the minimap asks for the place under it to be put in the
/// middle of the window: on the frame the button goes down, before the
/// toolkit has decided whether it is a click or a drag, and on every frame
/// it moves after that. None of it is a drag of the picture under the map.
#[test]
fn the_minimap_asks_to_center_on_what_is_pressed() {
    let mut harness = open(WINDOW, 1, panels());
    let image = [400.0, 300.0];
    harness.state_mut().current = Some(picture(400, 300));
    harness.state_mut().input.minimap_on_screen = true;
    harness.state_mut().input.can_pan = true;
    harness.run();
    let content = super::chrome::content_area(WINDOW, true);
    let map = super::minimap::thumbnail(content, image).expect("room for a map");
    let close = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() < 0.5 && (a[1] - b[1]).abs() < 0.5;

    // The button going down a quarter of the way across and half way down
    // the map asks for that point of the image on that very frame.
    let at = [map.x + map.width / 4.0, map.y + map.height / 2.0];
    harness.state_mut().commands.clear();
    harness.event(egui::Event::PointerMoved(egui::pos2(at[0], at[1])));
    harness.step();
    harness.event(egui::Event::PointerButton {
        pos: egui::pos2(at[0], at[1]),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.step();
    let commands = asked(&harness);
    assert!(
        matches!(commands.as_slice(), [Command::Center(at)] if close(*at, [100.0, 150.0])),
        "{commands:?}"
    );

    // Letting go asks for nothing more: the press already went there.
    harness.state_mut().commands.clear();
    harness.event(egui::Event::PointerButton {
        pos: egui::pos2(at[0], at[1]),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.step();
    assert_eq!(asked(&harness), Vec::new());

    // Moving across the map with the button held follows the hand, and
    // pans nothing.
    let to = [map.x + map.width * 3.0 / 4.0, map.y + map.height / 2.0];
    let commands = drag(&mut harness, at, to);
    assert!(
        commands
            .iter()
            .all(|command| matches!(command, Command::Center(_))),
        "{commands:?}"
    );
    assert!(
        matches!(commands.first(), Some(Command::Center(at)) if close(*at, [100.0, 150.0])),
        "{commands:?}"
    );
    assert!(
        matches!(commands.last(), Some(Command::Center(at)) if close(*at, [300.0, 150.0])),
        "{commands:?}"
    );
}

/// The dot at the head of the pixel readout opens the menu of formats, and
/// a cell of it chooses that format.
#[test]
fn the_pixel_menu_offers_every_format() {
    let mut harness = open(WINDOW, 1, panels());
    assert_eq!(click(&mut harness, "Pixel format"), []);
    assert_eq!(
        click(&mut harness, "Hex"),
        [Command::Press(Control::Format(PixelFormat::Hex))]
    );
}
