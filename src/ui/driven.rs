//! The interface driven without a window: laid out by egui headless, pressed
//! through the accessibility tree it builds, and read back as the commands
//! it hands the application.

use std::sync::Arc;

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

use crate::image::display::{Display, Headroom, Startup};
use crate::image::exif::Exif;
use crate::image::sequence::{Loops, Sequence};
use crate::image::{AlphaMode, Channels, ColorSpace, DecodedImage, Samples, Stats};
use crate::theme::Theme;
use crate::view::{View, Viewport};

use crate::image::region::{Grip, Region, Side};

use super::chooser::{self, Input, Row, Step};
use super::chrome::{BAR_HEIGHT, SIDE_WIDTH};
use super::control::{Naming, Unnamed};
use super::help;
use super::menu::Copies;
use super::tooltip::{Tip, Tooltip};
use super::transport::{Kind, Transport};
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
    /// What the help popup lays out: nothing, unless a test hands it a
    /// table of its own.
    help: Vec<help::Section>,
    commands: Vec<Command>,
}

/// The interface with a key table to lay out and nothing else to say:
/// [`Unnamed`] with the help popup's rows.
struct Keyed<'a>(&'a [help::Section]);

impl Naming for Keyed<'_> {
    fn tooltip(&self, tip: Tip) -> Option<Tooltip> {
        Unnamed.tooltip(tip)
    }

    fn shortcut(&self, control: Control) -> Option<String> {
        Unnamed.shortcut(control)
    }

    fn help(&self) -> Vec<help::Section> {
        self.0.to_vec()
    }
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
        sequence: Sequence::Still,
        page: 0,
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
        handle: Grip::Middle,
        grabbing: None,
        over_region: false,
        box_zoom: false,
        move_region: false,
        zoom_box: None,
        transport: None,
        chooser: None,
    }
}

/// The harness over an animation of `count` frames a tenth of a second
/// each, stopped on its first: the transport bar is up.
fn animated(count: usize) -> Harness<'static, State> {
    let mut harness = build(WINDOW, 1, panels());
    let state = harness.state_mut();
    if let Some(current) = state.current.as_mut() {
        current.sequence = Sequence::Animation {
            count,
            loops: Loops::Forever,
        };
    }
    state.input.transport = Some(Transport {
        index: 0,
        count,
        kind: Kind::Animation {
            playing: false,
            delays: vec![std::time::Duration::from_millis(100); count],
        },
    });
    state.input.viewport.height -= BAR_HEIGHT;
    harness.run();
    harness
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
        help: Vec::new(),
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
                    &Keyed(&state.help),
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
        .filter(|command| !matches!(command, Command::OverImage(_) | Command::OverGrip(_)))
        .cloned()
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

/// The transport bar's buttons hand back their presses, and none keeps
/// the keyboard; a press on the timeline comes back as the frame under it,
/// and a drag along it as the frames it crosses, ending on the last.
#[test]
fn the_transport_bar_hands_back_its_presses() {
    let mut harness = animated(4);
    for (label, control) in [
        ("Play", Control::Play),
        ("Previous frame", Control::StepBack),
        ("Next frame", Control::StepForward),
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

    let timeline = harness.get_by_label_contains("Timeline").rect();
    let y = timeline.center().y;
    let across = |fraction: f32| [timeline.min.x + timeline.width() * fraction, y];
    let dragged = drag(&mut harness, across(0.3), across(0.99));
    assert_eq!(dragged.first(), Some(&Command::Press(Control::Seek(1))));
    assert_eq!(dragged.last(), Some(&Command::Press(Control::Seek(3))));
}

/// A file of pages gets the steps and the count, and neither a play button
/// nor a timeline: there is no clock to play by. A still gets no bar at
/// all, and its picture the whole height between the two bars.
#[test]
fn pages_get_the_steps_alone_and_a_still_gets_no_bar() {
    let mut harness = build(WINDOW, 1, panels());
    harness.state_mut().input.transport = Some(Transport {
        index: 1,
        count: 3,
        kind: Kind::Pages,
    });
    harness.run();
    assert!(harness.query_by_label("Play").is_none());
    assert!(harness.query_by_label_contains("Timeline").is_none());
    assert_eq!(
        click(&mut harness, "Next frame"),
        [Command::Press(Control::StepForward)]
    );

    let harness = open(WINDOW, 1, panels());
    assert!(harness.query_by_label("Play").is_none());
    assert!(harness.query_by_label("Next frame").is_none());
}

/// A toggle whose panel the window cannot take is drawn dead and refuses
/// the press, rather than quietly setting something no one can see.
#[test]
fn a_toggle_with_no_room_for_its_panel_is_dead() {
    let small = [PANELS_ROOM[0] - 10.0 + 2.0 * SIDE_WIDTH, 200.0];
    let mut harness = open(small, 1, panels());
    for label in ["Histogram", "Information", "Help"] {
        let node = harness.get_by_label(label);
        assert!(node.accesskit_node().is_disabled(), "{label}");
    }
    assert_eq!(click(&mut harness, "Histogram"), []);
    assert_eq!(click(&mut harness, "Help"), []);

    let harness = open(WINDOW, 1, panels());
    for label in ["Histogram", "Help"] {
        assert!(
            !harness.get_by_label(label).accesskit_node().is_disabled(),
            "{label}"
        );
    }
}

/// The band under the histogram is the levels track. A handle dragged
/// along it asks for a window whose end is where the hand is; the band
/// between the handles slides both ends by what the hand moved; and the
/// exposure's slider asks for the exposure under the hand. A photograph is
/// offered the exposure and nothing else under the band: no windows, and
/// no curves.
#[test]
fn the_histogram_panel_hands_back_the_hand_on_its_band() {
    use crate::image::Transfer;

    let mut with_histogram = panels();
    with_histogram.show_histogram = true;
    let mut harness = open(WINDOW, 1, with_histogram);
    assert!(
        harness.query_by_label("Window 0").is_none(),
        "a photograph has no window row"
    );
    assert!(harness.query_by_label("Curve 0").is_none(), "nor curves");

    // The photograph is 8-bit sRGB, so the axis is 0..1 in sRGB and the two
    // handles stand at its ends.
    let band = harness.get_by_label("Window").rect();
    let white = harness.get_by_label("White point").rect();
    let black = harness.get_by_label("Black point").rect();
    assert!(black.center().x < white.center().x);
    let y = band.center().y;
    let across = |fraction: f32| [band.min.x + band.width() * fraction, y];
    // The white handle to the middle of the axis: the value halfway along
    // it, decoded, is to come out white.
    let commands = drag(&mut harness, [white.center().x, y], across(0.5));
    assert!(
        commands
            .iter()
            .all(|command| matches!(command, Command::WhitePoint(_)))
    );
    let Some(Command::WhitePoint(high)) = commands.last() else {
        panic!("{commands:?}");
    };
    assert!(
        (high - Transfer::Srgb.to_linear(0.5)).abs() < 1e-3,
        "{high}"
    );

    // The black handle a quarter of the way along, the same way about.
    let commands = drag(&mut harness, [black.center().x, y], across(0.25));
    let Some(Command::BlackPoint(low)) = commands.last() else {
        panic!("{commands:?}");
    };
    assert!((low - Transfer::Srgb.to_linear(0.25)).abs() < 1e-3, "{low}");

    // The band itself has nowhere to go: the window is the whole of the
    // axis — the interface holds no state, so the earlier drags moved
    // nothing here — and a band slid off the plot would be a lift.
    let commands = drag(&mut harness, across(0.5), across(0.6));
    assert!(commands.is_empty(), "{commands:?}");

    // A stop up brings white in to half, and then the band slides: a tenth
    // of the axis along, both ends go with it, in the axis's own units; and
    // as far as the pointer asks only up to the plot's end.
    harness
        .state_mut()
        .current
        .as_mut()
        .expect("a picture is up")
        .display
        .exposure_stops = 1.0;
    harness.run();
    let commands = drag(&mut harness, across(0.25), across(0.35));
    let Some(Command::Slide {
        black: low,
        white: high,
    }) = commands.last()
    else {
        panic!("{commands:?}");
    };
    let half = Transfer::Srgb.to_encoded(0.5);
    assert!((low - Transfer::Srgb.to_linear(0.1)).abs() < 1e-3, "{low}");
    assert!(
        (high - Transfer::Srgb.to_linear(half + 0.1)).abs() < 1e-3,
        "{high}"
    );
    let commands = drag(&mut harness, across(0.25), across(1.0));
    let Some(Command::Slide {
        black: low,
        white: high,
    }) = commands.last()
    else {
        panic!("{commands:?}");
    };
    assert!(
        (low - Transfer::Srgb.to_linear(1.0 - half)).abs() < 1e-3,
        "{low}"
    );
    assert!((high - 1.0).abs() < 1e-3, "{high}");

    // And the exposure's slider, which is dragged to the pointer as the
    // handles are: its middle is nothing, its far end is the run's end and
    // so is a hand past it, and a press that does not move still lands.
    // The exposure is a stop up from the test above, so the middle is a
    // change — and stays one, on every frame the hand is down, since the
    // interface here is never told it was done.
    let slider = harness.get_by_label("Exposure").rect();
    let y = slider.center().y;
    let middle = [slider.center().x, y];
    let commands = drag(&mut harness, middle, middle);
    assert!(!commands.is_empty());
    assert!(
        commands
            .iter()
            .all(|command| *command == Command::Exposure(0.0)),
        "{commands:?}"
    );
    let commands = drag(&mut harness, middle, [slider.max.x + 30.0, y]);
    assert_eq!(commands.last(), Some(&Command::Exposure(6.0)));
    let commands = drag(&mut harness, middle, [slider.min.x, y]);
    assert_eq!(commands.last(), Some(&Command::Exposure(-6.0)));
}

/// The curves are dead under a false color, which clips at the top of its
/// ramp whatever curve is chosen, and refuse the press; the ramps beside
/// them stay live, and the gray one brings the curves back.
#[test]
fn the_curves_are_dead_under_a_false_color() {
    use crate::image::display::Colormap;

    // Gray, linear, with one sample far above the rest, which the trimmed
    // window leaves out above white: a file that is offered both the false
    // colors and the curves.
    let mut data = vec![0.5f32; 1000];
    data[3] = 50.0;
    let image = DecodedImage::new(
        40,
        25,
        Samples::F32 {
            channels: Channels::Gray,
            data,
        },
        ColorSpace::LINEAR_BT709,
        AlphaMode::Opaque,
    );
    let stats = Stats::scan(&image);
    let mut current = photograph();
    current.display = Display::for_image_with(&image, &stats, Startup::default(), Headroom::None);
    current.image = Arc::new(image);
    current.stats = stats;

    let mut with_histogram = panels();
    with_histogram.show_histogram = true;
    let mut harness = open(WINDOW, 1, with_histogram);
    harness.state_mut().current = Some(current);
    harness.run();
    let dead = |harness: &Harness<'static, State>, label: &str| {
        harness.get_by_label(label).accesskit_node().is_disabled()
    };
    assert!(!dead(&harness, "Curve 1"));
    assert_eq!(
        click(&mut harness, "Curve 1"),
        [Command::Press(Control::Curve(1))]
    );

    harness
        .state_mut()
        .current
        .as_mut()
        .expect("a picture is up")
        .display
        .colormap = Colormap::Viridis;
    harness.run();
    for index in 0..3 {
        assert!(dead(&harness, &format!("Curve {index}")), "curve {index}");
    }
    assert_eq!(click(&mut harness, "Curve 1"), []);
    assert!(!dead(&harness, "False color 0"));
    assert_eq!(
        click(&mut harness, "False color 0"),
        [Command::Press(Control::Ramp(0))]
    );
}

/// The paste button is on screen only while the clipboard is holding a
/// picture, and the pair that steps through the list, with the count
/// between them that opens the chooser, only while there is a list to step
/// through.
#[test]
fn buttons_that_would_do_nothing_are_not_there() {
    let harness = open(WINDOW, 1, panels());
    assert!(harness.query_by_label("Paste").is_none());
    assert!(harness.query_by_label("Previous file").is_none());
    assert!(harness.query_by_label("Next file").is_none());
    assert!(harness.query_by_label("Choose a file").is_none());
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
    assert_eq!(
        click(&mut harness, "Choose a file"),
        [Command::Press(Control::Chooser)]
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

/// With a region on screen, a drag from inside it pans the picture as a
/// drag from anywhere else does — a region that covers the window would
/// otherwise pin the view — unless `Shift` is held, when it takes hold of
/// the whole region. The handle at the region's middle takes hold of the
/// whole with no key held, as the eight others take hold of their edges.
#[test]
fn a_region_on_screen_is_moved_with_shift_or_by_its_middle() {
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
    // Inside the region, well off its middle and its edges.
    let inside = screen_point(&harness, [1.25, 1.25]);
    assert!(region.contains(image_point(&harness, inside)));
    let to = screen_point(&harness, [2.25, 2.25]);

    let panned = |commands: &[Command]| {
        !commands.is_empty()
            && commands
                .iter()
                .all(|command| matches!(command, Command::Drag(_)))
    };
    let commands = drag(&mut harness, inside, to);
    assert!(panned(&commands), "{commands:?}");

    let held = |commands: &[Command], grip: Grip| {
        matches!(
            commands.first(),
            Some(Command::Grab { grab: Grab::Handle(g), .. }) if *g == grip
        ) && commands.last() == Some(&Command::Release)
            && !commands
                .iter()
                .any(|command| matches!(command, Command::Drag(_)))
    };
    harness.state_mut().input.move_region = true;
    harness.run();
    let commands = drag(&mut harness, inside, to);
    assert!(held(&commands, Grip::Inside), "{commands:?}");

    // Off the region, the key held changes nothing: the picture pans.
    let outside = [60.0, 100.0];
    assert!(!region.contains(image_point(&harness, outside)));
    let commands = drag(&mut harness, outside, [90.0, 130.0]);
    assert!(panned(&commands), "{commands:?}");

    harness.state_mut().input.move_region = false;
    harness.run();
    let middle = screen_point(&harness, [1.5, 1.5]);
    let commands = drag(&mut harness, middle, to);
    assert!(held(&commands, Grip::Middle), "{commands:?}");
}

/// A click on a handle of the region — the button down and up again in the
/// same place — names it the current handle, and asks for nothing else: no
/// hold, no pull, no pan. A click inside the region, off its handles, names
/// nothing, and neither does a click on the bare picture.
#[test]
fn a_click_on_a_handle_makes_it_the_current_one() {
    let mut harness = open(WINDOW, 1, panels());
    let region = Region {
        x: 1,
        y: 1,
        width: 1,
        height: 1,
    };
    harness.state_mut().input.selection = Selection::Shown(region);
    harness.run();
    let click = |harness: &mut Harness<'static, State>, at: [f32; 2]| drag(harness, at, at);

    let right = screen_point(&harness, [2.0, 1.5]);
    assert_eq!(
        click(&mut harness, right),
        [Command::Handle(Grip::Edge(Side::Right))]
    );
    let corner = screen_point(&harness, [1.0, 1.0]);
    assert_eq!(
        click(&mut harness, corner),
        [Command::Handle(Grip::Corner(Side::Left, Side::Top))]
    );
    let middle = screen_point(&harness, [1.5, 1.5]);
    assert_eq!(click(&mut harness, middle), [Command::Handle(Grip::Middle)]);

    let inside = screen_point(&harness, [1.25, 1.25]);
    assert_eq!(click(&mut harness, inside), []);
    assert_eq!(click(&mut harness, [60.0, 100.0]), []);
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

    // The key up again, the same drag — from the region's middle handle —
    // takes hold of the region as before.
    harness.state_mut().input.box_zoom = false;
    harness.run();
    let commands = drag(&mut harness, inside, to);
    assert!(
        matches!(
            commands.first(),
            Some(Command::Grab {
                grab: Grab::Handle(Grip::Middle),
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
    let content = super::chrome::content_area(WINDOW, true, false);
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

/// The chooser: opened, its field takes the keyboard, and what is typed
/// and pressed comes back as commands rather than reaching the window —
/// the query, the cursor's moves, `Enter` on the row under the cursor, a
/// click on a row, and `Ctrl+P` again. `Esc` closes it, and with it gone
/// the keyboard is the window's again.
#[test]
fn the_chooser_takes_the_keys_while_open_and_gives_them_back() {
    let mut harness = open(WINDOW, 3, panels());
    assert!(!harness.ctx.egui_wants_keyboard_input());

    egui::Popup::open_id(&harness.ctx, chooser::id());
    let rows: Arc<[Row]> = ["alpha.png", "beta.png", "gamma.png"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| Row {
            name: name.to_string(),
            dir: String::new(),
            kind: "PNG".to_string(),
            index: index + 1,
            dimensions: None,
            thumb: None,
            positions: Vec::new(),
            title: (index == 1).then(|| "Common Buzzard".to_string()),
            title_positions: Vec::new(),
        })
        .collect();
    harness.state_mut().input.chooser = Some(Input {
        query: String::new(),
        rows,
        cursor: 0,
        current: Some(0),
        count: 3,
        several_dirs: false,
        opened: true,
        reveal: true,
        visible: 0..0,
    });
    // The chord that opened it is still in egui's input on the first
    // frame, and must not be read as the chord that closes it.
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::P);
    harness.run();
    assert!(egui::Popup::is_id_open(&harness.ctx, chooser::id()));
    assert!(
        !asked(&harness).contains(&Command::Press(Control::Chooser)),
        "{:?}",
        asked(&harness)
    );
    harness
        .state_mut()
        .input
        .chooser
        .as_mut()
        .expect("set above")
        .opened = false;
    harness.run();
    assert!(harness.ctx.egui_wants_keyboard_input());
    // The rows on screen were said, so their thumbnails can be asked for.
    assert!(
        asked(&harness).iter().any(
            |command| matches!(command, Command::Visible(rows) if rows.start == 0 && rows.end == 3)
        ),
        "{:?}",
        asked(&harness)
    );

    let pressed = |harness: &mut Harness<'static, State>, key| {
        harness.state_mut().commands.clear();
        harness.key_press(key);
        harness.step();
        asked(harness)
    };

    harness.state_mut().commands.clear();
    harness.event(egui::Event::Text("b".to_string()));
    harness.step();
    assert!(
        asked(&harness).contains(&Command::Query("b".to_string())),
        "{:?}",
        asked(&harness)
    );

    assert!(pressed(&mut harness, egui::Key::ArrowDown).contains(&Command::Cursor(Step::Down)));
    assert!(pressed(&mut harness, egui::Key::ArrowUp).contains(&Command::Cursor(Step::Up)));
    assert!(pressed(&mut harness, egui::Key::End).contains(&Command::Cursor(Step::Last)));
    assert!(pressed(&mut harness, egui::Key::Enter).contains(&Command::Press(Control::Choose(0))));
    assert!(click(&mut harness, "Choose file 2").contains(&Command::Press(Control::Choose(1))));

    harness.state_mut().commands.clear();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::P);
    harness.step();
    assert!(asked(&harness).contains(&Command::Press(Control::Chooser)));
    // Still up: closing it is the application's, on that press.
    assert!(egui::Popup::is_id_open(&harness.ctx, chooser::id()));

    // Escape is egui's own, and closes the popup; the application then
    // stops handing the chooser in, and the field goes with it.
    harness.key_press(egui::Key::Escape);
    harness.step();
    assert!(!egui::Popup::is_id_open(&harness.ctx, chooser::id()));
    harness.state_mut().input.chooser = None;
    harness.run();
    assert!(!harness.ctx.egui_wants_keyboard_input());
    assert!(harness.query_by_label("Choose file 2").is_none());
}

/// A cursor moved by a key is scrolled into view, and the rows on screen
/// are said again once they have changed: a long list opened with the
/// cursor far down it shows the cursor's row, not the first.
#[test]
fn the_chooser_scrolls_a_moved_cursor_into_view() {
    let mut harness = open(WINDOW, 200, panels());
    egui::Popup::open_id(&harness.ctx, chooser::id());
    let rows: Arc<[Row]> = (0..200)
        .map(|index| Row {
            name: format!("{index:03}.png"),
            dir: String::new(),
            kind: "PNG".to_string(),
            index: index + 1,
            dimensions: None,
            thumb: None,
            positions: Vec::new(),
            title: None,
            title_positions: Vec::new(),
        })
        .collect();
    harness.state_mut().input.chooser = Some(Input {
        query: String::new(),
        rows,
        cursor: 150,
        current: None,
        count: 200,
        several_dirs: false,
        opened: false,
        reveal: true,
        visible: 0..0,
    });
    // One frame asks for the scroll; the frames after it play the scroll
    // animation out, as `run` will not while the caret is also blinking.
    harness.step();
    harness
        .state_mut()
        .input
        .chooser
        .as_mut()
        .expect("set above")
        .reveal = false;
    harness.run_steps(30);
    let visible = asked(&harness)
        .into_iter()
        .filter_map(|command| match command {
            Command::Visible(rows) => Some(rows),
            _ => None,
        })
        .next_back()
        .expect("the rows on screen were said");
    assert!(visible.contains(&150), "{visible:?}");
    assert!(!visible.contains(&0), "{visible:?}");
    assert!(harness.query_by_label("Choose file 151").is_some());
    assert!(harness.query_by_label("Choose file 1").is_none());
}

/// The help button at the foot of the right strip hands back its press; the popup
/// it opens lays the table out — the three headings, each section's title
/// and each row's three columns — and takes no key from the window.
#[test]
fn the_help_popup_lays_the_keys_out() {
    let mut harness = open(WINDOW, 1, panels());
    assert_eq!(click(&mut harness, "Help"), [Command::Press(Control::Help)]);

    harness.state_mut().help = vec![
        help::Section {
            title: "Zoom and position",
            rows: vec![
                help::Row {
                    key: "1, 0".to_string(),
                    does: "Actual size (100%)",
                    when: None,
                },
                help::Row {
                    key: "Ctrl+Shift+Arrows".to_string(),
                    does: "Shrink a region that way a pixel",
                    when: Some("a region selected"),
                },
            ],
        },
        help::Section {
            title: "Files",
            rows: vec![help::Row {
                key: "], Page Down".to_string(),
                does: "Next file",
                when: Some("more than one file"),
            }],
        },
    ];
    assert!(harness.query_by_label("Zoom and position").is_none());
    egui::Popup::open_id(&harness.ctx, help::id());
    harness.run();
    assert!(egui::Popup::is_id_open(&harness.ctx, help::id()));
    for label in [
        "Key",
        "Does",
        "When",
        "Zoom and position",
        "1, 0",
        "Actual size (100%)",
        "Ctrl+Shift+Arrows",
        "a region selected",
        "Files",
        "], Page Down",
        "more than one file",
    ] {
        assert!(
            harness.query_by_label(label).is_some(),
            "{label} is on the popup"
        );
    }
    assert!(!harness.ctx.egui_wants_keyboard_input());

    // A click on the lit button hands back the same press, and the popup is
    // still up when the frame is over: closing it is that press's to do.
    // Closed by egui on the click and then opened by the press, it would
    // never close from its button at all.
    assert_eq!(click(&mut harness, "Help"), [Command::Press(Control::Help)]);
    assert!(egui::Popup::is_id_open(&harness.ctx, help::id()));
    // A click anywhere else closes it, as it closes a menu.
    harness.state_mut().commands.clear();
    drag(&mut harness, [60.0, 100.0], [60.0, 100.0]);
    harness.run();
    assert!(!egui::Popup::is_id_open(&harness.ctx, help::id()));
}

/// The chooser, the other popup the application opens: a click on the
/// count while it is up leaves it up for the press to close, likewise.
#[test]
fn a_click_on_the_count_leaves_the_chooser_for_the_press_to_close() {
    let mut harness = open(WINDOW, 3, panels());
    egui::Popup::open_id(&harness.ctx, chooser::id());
    harness.state_mut().input.chooser = Some(Input {
        query: String::new(),
        rows: Arc::from([]),
        cursor: 0,
        current: Some(0),
        count: 3,
        several_dirs: false,
        opened: false,
        reveal: false,
        visible: 0..0,
    });
    harness.run();
    harness.run();
    assert!(egui::Popup::is_id_open(&harness.ctx, chooser::id()));
    assert!(
        click(&mut harness, "Choose a file").contains(&Command::Press(Control::Chooser)),
        "{:?}",
        asked(&harness)
    );
    assert!(egui::Popup::is_id_open(&harness.ctx, chooser::id()));
}

/// In a window too narrow for the three columns the rows stack, each part
/// on a line of its own, and the column headings — which would then head
/// nothing — are left out. The popup does not ask for a column less than
/// nothing wide, which is what used to bring the program down.
#[test]
fn the_help_popup_stacks_its_rows_in_a_narrow_window() {
    let mut harness = open(
        [PANELS_ROOM[0] + 2.0 * SIDE_WIDTH + 20.0, 700.0],
        1,
        panels(),
    );
    harness.state_mut().help = vec![help::Section {
        title: "Files",
        rows: vec![help::Row {
            key: "], Page Down".to_string(),
            does: "Next file",
            when: Some("more than one file"),
        }],
    }];
    egui::Popup::open_id(&harness.ctx, help::id());
    harness.run();
    assert!(egui::Popup::is_id_open(&harness.ctx, help::id()));
    for label in ["Files", "], Page Down", "Next file", "more than one file"] {
        assert!(
            harness.query_by_label(label).is_some(),
            "{label} is on the popup"
        );
    }
    for heading in ["Key", "Does", "When"] {
        assert!(
            harness.query_by_label(heading).is_none(),
            "{heading} heads nothing when stacked"
        );
    }
}
