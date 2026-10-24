//! The interface driven without a window: laid out by egui headless, pressed
//! through the accessibility tree it builds, and read back as the commands
//! it hands the application.

use std::sync::Arc;

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

use crate::image::display::{Display, Headroom, Startup, ToneMap};
use crate::image::sequence::{Loops, Sequence};
use crate::image::{AlphaMode, Channels, ColorSpace, DecodedImage, Samples, Stats};
use crate::theme::Theme;
use crate::view::{View, Viewport};

use crate::image::region::{Grip, Region, Side};

use super::chooser::{self, Input, Row, Step};
use super::chrome::{self, BAR_HEIGHT, SIDE_WIDTH};
use super::control::{Naming, Unnamed};
use super::filmstrip;
use super::help;
use super::menu::Copies;
use super::region_panel::Field;
use super::rename::{self, Verdict};
use super::tooltip::{Tip, Tooltip};
use super::transport::{Kind, Transport};
use super::{
    Command, Control, CoordinateFormat, Current, FrameInput, GeographicFormat, Grab, PANELS_ROOM,
    Panels, PixelFormat, Selection,
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
    /// Whether everything the pointer rests on names itself, which is off
    /// unless a test is reading tooltips: one opening where the pointer
    /// was left would lie over what the next press is aimed at.
    named: bool,
    commands: Vec<Command>,
}

/// The interface with a key table to lay out and nothing else to say:
/// [`Unnamed`] with the help popup's rows — or, where a test asks, every
/// tip named by its own spelling, so that which tooltip opened can be read
/// back off the tree.
struct Keyed<'a>(&'a [help::Section], bool);

impl Naming for Keyed<'_> {
    fn tooltip(&self, tip: Tip) -> Option<Tooltip> {
        match self.1 {
            true => Some(Tooltip {
                title: vec![format!("{tip:?}")],
                hints: Vec::new(),
            }),
            false => Unnamed.tooltip(tip),
        }
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
    Current::of(image, "photo.png")
}

pub(super) fn panels() -> Panels {
    Panels {
        show_ui: true,
        show_filmstrip: false,
        side: None,
        mark_clipped: false,
        show_minimap: true,
        show_grid: false,
        horizon_grid: false,
        horizon_step: super::attitude::DEFAULT_STEP,
        show_loupe: false,
        loupe_magnification: super::loupe::DEFAULT_MAGNIFICATION,
        pixel_format: PixelFormat::default(),
        coordinate_format: CoordinateFormat::default(),
        geographic_format: GeographicFormat::default(),
        info_tab: super::tags::Tab::Facts,
    }
}

pub(super) fn input(logical: [f32; 2], count: usize) -> FrameInput {
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
        loupe: None,
        loupe_held: false,
        index: 0,
        count,
        arriving: None,
        deleted: false,
        headroom: Headroom::None,
        hdr_available: false,
        room: 1.0,
        can_pan: false,
        openers: Vec::new(),
        toast: None,
        selection: Selection::Off,
        handle: Grip::Middle,
        grabbing: None,
        over_region: false,
        box_zoom: false,
        modifiers: crate::gestures::Mods::empty(),
        gestures: std::rc::Rc::new(crate::gestures::Gestures::table()),
        zoom_box: None,
        aspect: crate::image::aspect::Aspect::Free,
        transport: None,
        filmstrip: None,
        side: None,
        chooser: None,
        tags: None,
        rename: None,
        export: None,
        empty: false,
        paste: false,
        folder: None,
        picking: false,
        standin: None,
        thumb: None,
        waiting: false,
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
    build_at(logical, 1.0, count, panels)
}

/// [`build`] on a device of `scale` pixels to the point, which the frame's
/// input says too, as the application's does.
fn build_at(
    logical: [f32; 2],
    scale: f32,
    count: usize,
    panels: Panels,
) -> Harness<'static, State> {
    let mut input = input(logical, count);
    input.scale = scale;
    // The side panel up where it is asked for and the window has room for
    // it, as `App::side_showing` decides.
    input.side = panels
        .side
        .filter(|side| super::room(logical, panels.show_ui, chrome::Parts::NONE).holds(*side));
    input.viewport = chrome::image_viewport(
        [logical[0] * scale, logical[1] * scale],
        scale,
        panels.show_ui,
        chrome::Parts {
            side: input.side.is_some(),
            ..chrome::Parts::NONE
        },
    );
    let state = State {
        ready: false,
        input,
        panels,
        current: Some(photograph()),
        view: View::new(),
        help: Vec::new(),
        named: false,
        commands: Vec::new(),
    };
    Harness::builder()
        .with_size(egui::vec2(logical[0], logical[1]))
        .with_pixels_per_point(scale)
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
                    &Keyed(&state.help, state.named),
                );
                // The one thing the application says back to the interface
                // about a gesture, done here as `App::act` does it: which
                // hold a drag has, so that the frames of one drag all go the
                // same way.
                for command in &commands {
                    match command {
                        Command::Grab { grab, .. } => state.input.grabbing = Some(*grab),
                        Command::Release => state.input.grabbing = None,
                        // And which of the file list's rows are on screen,
                        // so that it is said once rather than every pass.
                        Command::FilmstripVisible(rows) => {
                            if let Some(strip) = &mut state.input.filmstrip {
                                strip.visible = rows.clone();
                            }
                        }
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
        .filter(|command| {
            !matches!(
                command,
                Command::OverImage(_)
                    | Command::OverGrip(_)
                    | Command::Held { .. }
                    | Command::Dragging(_)
            )
        })
        .cloned()
        .collect()
}

/// Presses the primary button at `from`, moves to `to`, and lets go there,
/// a pass to each, and returns what the interface asked for.
fn drag(harness: &mut Harness<'static, State>, from: [f32; 2], to: [f32; 2]) -> Vec<Command> {
    drag_with(harness, egui::PointerButton::Primary, from, to)
}

/// The same with another button; from a point to itself, a click of it.
fn drag_with(
    harness: &mut Harness<'static, State>,
    pressed: egui::PointerButton,
    from: [f32; 2],
    to: [f32; 2],
) -> Vec<Command> {
    harness.state_mut().commands.clear();
    let (from, to) = (egui::pos2(from[0], from[1]), egui::pos2(to[0], to[1]));
    let button = |pos, down| egui::Event::PointerButton {
        pos,
        button: pressed,
        pressed: down,
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
    // A picture that says which way its camera faced, for the horizon grid.
    harness.state_mut().current = Some(pitched());
    for (label, control) in [
        ("Histogram", Control::Histogram),
        ("Information", Control::Info),
        ("Minimap", Control::Minimap),
        ("Grid", Control::Grid),
        ("Horizon grid", Control::HorizonGrid),
        ("Loupe", Control::Loupe),
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

/// The button held down on the picture is said on every pass, with the
/// pointer's place while it is down — in the physical pixels the
/// application's pointer is kept in — and as nothing once it is up. The
/// view is not dragged by the secondary, which drags nothing by default,
/// and the wheel turned while it is down says it is.
#[test]
fn the_secondary_button_on_the_picture_is_handed_back_while_it_is_down() {
    let mut harness = open(WINDOW, 1, panels());
    harness.state_mut().input.scale = 2.0;
    let secondary = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Secondary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let said = |harness: &Harness<'static, State>| {
        harness
            .state()
            .commands
            .iter()
            .filter_map(|command| match command {
                Command::Held { button, at } => {
                    assert_eq!(button.is_some(), at.is_some());
                    Some(*at)
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let (from, to) = (egui::pos2(400.0, 300.0), egui::pos2(420.0, 330.0));

    harness.state_mut().commands.clear();
    harness.event(egui::Event::PointerMoved(from));
    harness.step();
    assert_eq!(said(&harness), [None]);

    harness.state_mut().commands.clear();
    harness.event(secondary(from, true));
    harness.step();
    assert_eq!(said(&harness), [Some([800.0, 600.0])]);

    harness.state_mut().commands.clear();
    harness.event(egui::Event::PointerMoved(to));
    harness.step();
    assert_eq!(said(&harness), [Some([840.0, 660.0])]);
    assert!(
        !harness
            .state()
            .commands
            .iter()
            .any(|command| matches!(command, Command::Drag(_))),
        "the secondary button does not pan"
    );

    // The wheel, while the button is down, says so; up again, it says
    // nothing is.
    harness.state_mut().commands.clear();
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, 1.0),
        modifiers: egui::Modifiers::NONE,
        phase: egui::TouchPhase::Move,
    });
    harness.step();
    assert_eq!(
        asked(&harness),
        [Command::Wheel {
            delta: [0.0, 1.0],
            notched: true,
            held: Some(crate::gestures::Button::Right),
        }]
    );

    harness.state_mut().commands.clear();
    harness.event(secondary(to, false));
    harness.step();
    assert_eq!(said(&harness), [None]);

    harness.state_mut().commands.clear();
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, 1.0),
        modifiers: egui::Modifiers::NONE,
        phase: egui::TouchPhase::Move,
    });
    harness.step();
    assert_eq!(
        asked(&harness),
        [Command::Wheel {
            delta: [0.0, 1.0],
            notched: true,
            held: None,
        }]
    );
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

/// The timeline keeps its size and place from frame to frame, whatever
/// the readout beside it says: a track that moved as the digits changed
/// would move the handle on it with them.
#[test]
fn the_timeline_holds_still_as_the_readout_changes() {
    let mut harness = animated(120);
    let first = harness.get_by_label_contains("Timeline").rect();
    for index in 1..120 {
        if let Some(transport) = harness.state_mut().input.transport.as_mut() {
            transport.index = index;
        }
        harness.run();
        assert_eq!(
            harness.get_by_label_contains("Timeline").rect(),
            first,
            "frame {index}"
        );
    }
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
        kind: Kind::Pages { name: None },
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

/// A page the file names is counted and then named, in the one readout.
#[test]
fn a_named_page_says_its_name_after_the_count() {
    let mut harness = build(WINDOW, 1, panels());
    harness.state_mut().input.transport = Some(Transport {
        index: 5,
        count: 9,
        kind: Kind::Pages {
            name: Some("native forest".to_string()),
        },
    });
    harness.run();
    assert!(
        harness
            .query_by_label("6 / 9 \u{b7} native forest")
            .is_some()
    );
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
/// exposure's slider asks for the exposure under the hand. A photo has the
/// exposure and the curve under the band and no row of windows — its
/// window is as stored and the handles' — and a press on a curve's button
/// reaches the application; the pair at the head of the panel says the
/// file is read as a photo, and a press on the other reading reaches it.
#[test]
fn the_histogram_panel_hands_back_the_hand_on_its_band() {
    use crate::image::Transfer;
    use crate::image::decode::Reading;

    let mut with_histogram = panels();
    with_histogram.side = Some(super::side::Side::Histogram);
    let mut harness = open(WINDOW, 1, with_histogram);
    assert!(
        harness.query_by_label("Window 0").is_none(),
        "a photo has no window row"
    );
    assert!(harness.query_by_label("Curve 0").is_some());
    assert_eq!(
        click(&mut harness, "Curve 0"),
        [Command::Press(Control::Curve(0))]
    );
    let lit = |harness: &Harness<'static, State>, label: &str| {
        matches!(
            harness.get_by_label(label).accesskit_node().toggled(),
            Some(egui::accesskit::Toggled::True)
        )
    };
    assert!(lit(&harness, "Set photo mode") && !lit(&harness, "Set data mode"));
    assert_eq!(
        click(&mut harness, "Set data mode"),
        [Command::Press(Control::Reading(Reading::Data))]
    );

    // The file is 8-bit sRGB, so the axis is 0..1 in sRGB and the two
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
        .set_exposure(1.0);
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
/// With the interface drawn at one and a half device pixels to the point —
/// a monitor at 1 with the interface a step and a half up, or the other way
/// round — egui's panels land where the application works out they will,
/// so that the picture fitted beside them and the pointer read against them
/// agree with what is drawn; and the plot, a device pixel to the column,
/// draws without falling over.
#[test]
fn a_scaled_interface_lays_out_where_the_application_says() {
    use super::side::Side;

    let scale = 1.5;
    let logical = [1200.0, 800.0];
    let mut with_histogram = panels();
    with_histogram.side = Some(Side::Histogram);
    let mut harness = build_at(logical, scale, 1, with_histogram);
    harness.run();
    assert_eq!(harness.ctx.pixels_per_point(), scale);

    let parts = chrome::Parts {
        side: true,
        ..chrome::Parts::NONE
    };
    // Everything is where the application puts it in the room the harness
    // leaves inside its own margin, which the application's window does not
    // have.
    let margin = 8.0;
    let room = [logical[0] - 2.0 * margin, logical[1] - 2.0 * margin];
    let side = chrome::Chrome::new(room, parts)
        .side
        .expect("the side panel is up");
    let placed = super::histogram::panel(side);
    assert_eq!(
        harness.get_by_label("Histogram panel").rect(),
        egui::Rect::from_min_size(
            egui::pos2(placed.x + margin, placed.y + margin),
            egui::vec2(placed.width, placed.height)
        )
    );
    // What egui's panels leave between them is the content area the
    // application fitted the picture to.
    let inner = chrome::content_area(room, true, parts);
    let panel = |name: &str| {
        egui::PanelState::load(&harness.ctx, egui::Id::new(name))
            .unwrap_or_else(|| panic!("the {name} panel is up"))
            .outer_rect
    };
    let left = egui::pos2(panel("left").max.x, panel("top").max.y);
    let right = egui::pos2(panel("side").min.x, panel("bottom").min.y);
    assert_eq!(
        egui::Rect::from_min_max(left, right),
        egui::Rect::from_min_size(
            egui::pos2(inner.x + margin, inner.y + margin),
            egui::vec2(inner.width, inner.height)
        )
    );
}

/// The pointer on a region's row draws the region on the picture, beside
/// the panel rather than inside it: what the panel's own drawing is
/// clipped to is not what the picture's marks are.
#[test]
fn a_regions_row_marks_it_on_the_picture() {
    use super::side::Side;
    use crate::image::metadata_region::{MetadataRegion, Shape, Units};

    let mut with_info = panels();
    with_info.side = Some(Side::Info);
    let mut harness = open(WINDOW, 1, with_info);
    let mut current = photograph();
    current.exif.regions = vec![MetadataRegion {
        label: "Face".into(),
        name: Some("Jane Doe".into()),
        details: Vec::new(),
        shape: Some(Shape::Rectangle {
            center: [0.5, 0.5],
            size: [0.5, 0.5],
        }),
        units: Units::Shares,
    }];
    harness.state_mut().current = Some(current);
    harness.run();

    let side = egui::PanelState::load(&harness.ctx, egui::Id::new("side"))
        .expect("the side panel is up")
        .outer_rect;
    // The subject written over the region, which is drawn where the
    // picture is: starting left of the panel.
    let marked = |harness: &Harness<'static, State>| {
        harness.output().shapes.iter().any(|shape| {
            matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == "Jane Doe")
                && shape.clip_rect.min.x < side.min.x
        })
    };
    assert!(!marked(&harness));
    harness.get_by_label("Jane Doe").hover();
    harness.run();
    assert!(marked(&harness));
}

/// A thumbnail `width` by `height` held by the harness's egui: red in its
/// top left quarter, so that which way up it was drawn can be seen, and a
/// gray ramp across the rest.
fn thumb_of(harness: &Harness<'static, State>, width: usize, height: usize) -> super::Thumb {
    let pixels = (0..width * height)
        .map(|at| {
            let (x, y) = (at % width, at / width);
            if x < width / 2 && y < height / 2 {
                egui::Color32::RED
            } else {
                egui::Color32::from_gray((255 * x / width) as u8)
            }
        })
        .collect();
    let image = egui::ColorImage::new([width, height], pixels);
    let texture = harness
        .ctx
        .load_texture("thumbnail", image, egui::TextureOptions::LINEAR);
    // Held by egui's manager past this handle, as the application's are
    // past a frame.
    let sized = egui::load::SizedTexture::from_handle(&texture);
    std::mem::forget(texture);
    super::Thumb { copies: [sized; 3] }
}

/// The camera in the Orientation section wears the file's thumbnail on its
/// screen, once there is one.
#[test]
fn the_camera_wears_the_picture_on_its_screen() {
    use super::side::Side;
    use crate::image::attitude::Attitude;

    let mut with_info = panels();
    with_info.side = Some(Side::Info);
    let mut harness = open(WINDOW, 1, with_info);
    let mut current = photograph();
    current.exif.attitude = Attitude::gopro([-0.0377, 0.9992, -0.0092]);
    harness.state_mut().current = Some(current);
    harness.run();
    let thumb = thumb_of(&harness, 64, 48);
    let id = thumb.copies[0].id;
    let worn =
        |harness: &Harness<'static, State>| {
            harness.output().shapes.iter().any(
                |shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == id),
            )
        };
    assert!(!worn(&harness));
    harness.state_mut().input.thumb = Some(thumb);
    harness.run();
    assert!(worn(&harness));
}

/// A file that gives the bearing its lens pointed along and not how the
/// camera was tilted has an Orientation section too: the floor and its
/// arrows with no camera over them, the note saying why, and the bearing
/// as its one row; while the pointer is on it the ring is drawn at the
/// middle of the picture.
#[test]
fn a_heading_alone_has_an_orientation_section_without_a_camera() {
    use super::attitude::NO_TILT;
    use super::side::Side;

    let mut with_info = panels();
    with_info.side = Some(Side::Info);
    let mut harness = open(WINDOW, 1, with_info);
    let mut current = photograph();
    current.exif.heading = Some(349.4);
    current.exif.magnetic = true;
    harness.state_mut().current = Some(current);
    harness.run();
    assert!(harness.query_by_label("Orientation").is_some());
    assert!(harness.query_by_label("Bearing").is_some());
    assert!(harness.query_by_label("Pitch").is_none());
    let said = |harness: &Harness<'static, State>, word: &str| {
        harness.output().shapes.iter().any(
            |shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == word),
        )
    };
    assert!(said(&harness, NO_TILT));
    assert!(said(&harness, "N"));
    let rings = |harness: &Harness<'static, State>| {
        harness
            .output()
            .shapes
            .iter()
            .filter(|shape| matches!(&shape.shape, egui::Shape::Circle(circle) if circle.fill == egui::Color32::TRANSPARENT && circle.stroke.color == egui::Color32::WHITE))
            .count()
    };
    assert_eq!(rings(&harness), 0);
    harness.get_by_label("Bearing").hover();
    harness.run();
    assert_eq!(rings(&harness), 1);
}

/// A file that says which way its camera faced has an Orientation section,
/// and one that does not has none; while the pointer is on the section the
/// level line is drawn across the picture, beside the panel.
#[test]
fn the_orientation_section_draws_the_level_line_on_the_picture() {
    use super::side::Side;
    use crate::image::attitude::Attitude;

    let mut with_info = panels();
    with_info.side = Some(Side::Info);
    let mut harness = open(WINDOW, 1, with_info);
    harness.state_mut().current = Some(photograph());
    harness.run();
    assert!(harness.query_by_label("Orientation").is_none());

    let mut current = photograph();
    current.exif.attitude = Attitude::gopro([-0.0377, 0.9992, -0.0092]);
    harness.state_mut().current = Some(current);
    harness.run();
    assert!(harness.query_by_label("2.2\u{00b0} clockwise").is_some());
    assert!(harness.query_by_label("0.5\u{00b0} up").is_some());

    let side = egui::PanelState::load(&harness.ctx, egui::Id::new("side"))
        .expect("the side panel is up")
        .outer_rect;
    // A dashed line is many segments, drawn where the picture is.
    let segments = |harness: &Harness<'static, State>| {
        harness
            .output()
            .shapes
            .iter()
            .filter(|shape| {
                matches!(&shape.shape, egui::Shape::LineSegment { points, .. }
                    if points[0].x < side.min.x && points[1].x < side.min.x)
            })
            .count()
    };
    let before = segments(&harness);
    harness.get_by_label("Orientation").hover();
    harness.run();
    assert!(segments(&harness) > before + 10);
    // On the heading, its end is the button that copies the section; on
    // the rest of the section, the pill there names the line drawn.
    let said = |harness: &Harness<'static, State>, word: &str| {
        harness.output().shapes.iter().any(
            |shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == word),
        )
    };
    assert!(!said(&harness, "Level"));
    harness.get_by_label("Pitch").hover();
    harness.run();
    assert!(said(&harness, "Level"));
    // With the grid up, the pill says so wherever the pointer is.
    harness.get_by_label("Samples").hover();
    harness.run();
    assert!(!said(&harness, "Level"));
    harness.state_mut().panels.horizon_grid = true;
    harness.run();
    assert!(said(&harness, "Level"));
    harness.state_mut().panels.horizon_grid = false;
    harness.get_by_label("Pitch").hover();
    harness.run();

    // Only the true horizon wears a dot, and only a file that says its
    // focal length has one placed; pitched far enough down, it is off the
    // picture and the lighter level line is drawn instead.
    let dots = |harness: &Harness<'static, State>| {
        harness
            .output()
            .shapes
            .iter()
            .filter(|shape| {
                matches!(&shape.shape, egui::Shape::Circle(circle)
                    if circle.center.x < side.min.x)
            })
            .count()
    };
    let unplaced = dots(&harness);
    for (gravity, placed) in [([-0.0377, 0.9992, -0.0092], 1), ([0.0, 0.8, 0.6], 0)] {
        let mut current = photograph();
        current.exif.attitude = Attitude::gopro(gravity);
        current.exif.focal_equivalent = Some(26.0);
        harness.state_mut().current = Some(current);
        harness.run();
        assert_eq!(dots(&harness), unplaced + placed, "{gravity:?}");
        let word = if placed == 1 { "Horizon" } else { "Level" };
        assert!(said(&harness, word), "{gravity:?}");
        assert!(segments(&harness) > before + 10, "{gravity:?}");
    }

    // With the bearing the lens pointed at, the uprights are on the round
    // bearings and say which: none without it.
    for heading in [None, Some(212.0)] {
        let mut current = photograph();
        current.exif.attitude = Attitude::gopro([-0.0377, 0.9992, -0.0092]);
        current.exif.focal_equivalent = Some(26.0);
        current.exif.heading = heading;
        harness.state_mut().current = Some(current);
        harness.run();
        assert_eq!(said(&harness, "210\u{00b0}"), heading.is_some());
    }
    assert!(said(&harness, "210\u{00b0}"));
    assert!(said(&harness, "220\u{00b0}"));
    // The bearing itself is the camera's, written at the end of the arrow
    // along the floor in the section beside north's and in the Bearing row
    // under it, and on no upright.
    let at = |harness: &Harness<'static, State>, word: &str| -> Vec<f32> {
        harness
            .output()
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == word => Some(text.pos.x),
                _ => None,
            })
            .collect()
    };
    let bearing = at(&harness, "212\u{00b0}");
    assert_eq!(bearing.len(), 2);
    assert!(bearing.iter().all(|&x| x > side.min.x));
    assert!(harness.query_by_label("Bearing").is_some());
    let north = at(&harness, "N");
    assert_eq!(north.len(), 1);
    assert!(north[0] > side.min.x);
}

/// The side panel is laid out where the chrome's geometry says, between
/// the picture and the right strip, and the histogram at its head; while
/// another file is on its way in it keeps that place and says nothing of
/// the picture leaving: none of the histogram's rows, and nothing on the
/// information to copy.
#[test]
fn the_side_panel_waits_for_the_file_coming_in() {
    use super::side::{Side, WIDTH};

    let mut with_histogram = panels();
    with_histogram.side = Some(Side::Histogram);
    let mut harness = open(WINDOW, 2, with_histogram);
    // Up against the right strip, under the top bar: where the strip's
    // first button is, less the room the strip keeps around it.
    let toggle = harness.get_by_label("Histogram").rect();
    let strip = toggle.center().x - SIDE_WIDTH / 2.0;
    let top = toggle.top() - super::chrome::BAR_PADDING;
    let full = harness.get_by_label("Histogram panel").rect();
    assert_eq!(full.min, egui::pos2(strip - WIDTH, top));
    assert_eq!(full.width(), WIDTH);
    let side = egui::PanelState::load(&harness.ctx, egui::Id::new("side"))
        .expect("the side panel is up")
        .outer_rect;
    assert_eq!(full.max.y, side.max.y, "down the whole of the side");
    assert!(harness.query_by_label("Curve 0").is_some());
    assert!(
        harness.query_by_label("Copy All").is_none(),
        "one at a time"
    );

    harness.state_mut().input.waiting = true;
    // A few passes rather than a run: the spinner asks for a pass on every
    // pass, and would never let a run settle.
    harness.run_steps(3);
    assert!(harness.query_by_label("Histogram panel").is_some());
    assert!(harness.query_by_label("Window 0").is_none());
    assert!(harness.query_by_label("Curve 0").is_none());

    let mut with_info = panels();
    with_info.side = Some(Side::Info);
    let mut harness = open(WINDOW, 2, with_info);
    assert!(harness.query_by_label("Copy All").is_some());
    assert!(
        harness.query_by_label("Window 0").is_none(),
        "one at a time"
    );
    harness.state_mut().input.waiting = true;
    harness.run_steps(3);
    assert!(harness.query_by_label("Copy All").is_none());
}

/// A measurement is read as data, which the pair at the head of the panel
/// says, and has the row of windows and neither the curves nor the
/// exposure, which a photo has; the false colors are live on both.
#[test]
fn the_curves_are_dead_under_a_false_color() {
    use crate::image::Referred;
    use crate::image::display::Colormap;

    // Gray and linear: a measurement, offered the false colors and read
    // through the gray ramp without one.
    let mut image = DecodedImage::new(
        40,
        25,
        Samples::F32 {
            channels: Channels::Gray,
            data: vec![0.5f32; 1000],
        },
        ColorSpace::LINEAR_BT709,
        AlphaMode::Opaque,
    );
    let stats = Stats::scan(&image);
    let mut current = photograph();
    current.display = Display::for_image_with(&image, &stats, Startup::default());
    current.image = Arc::new(image.clone());
    current.stats = stats.clone();

    let mut with_histogram = panels();
    with_histogram.side = Some(super::side::Side::Histogram);
    let mut harness = open(WINDOW, 1, with_histogram);
    harness.state_mut().current = Some(current);
    harness.run();
    let dead = |harness: &Harness<'static, State>, label: &str| {
        harness.get_by_label(label).accesskit_node().is_disabled()
    };
    assert!(
        harness.query_by_label("Curve 0").is_none(),
        "data has no curve"
    );
    assert!(
        harness.query_by_label("Exposure").is_none(),
        "and no exposure"
    );
    assert!(!dead(&harness, "False color 0"));
    let lit = |harness: &Harness<'static, State>, label: &str| {
        matches!(
            harness.get_by_label(label).accesskit_node().toggled(),
            Some(egui::accesskit::Toggled::True)
        )
    };
    assert!(lit(&harness, "Set data mode") && !lit(&harness, "Set photo mode"));
    assert!(
        harness.query_by_label("Window 0").is_some(),
        "data has the row of windows"
    );
    assert_eq!(
        click(&mut harness, "Window 0"),
        [Command::Press(Control::Window(0))]
    );
    // The same picture graded is light, and its curves are live until a
    // false color is put on it.
    image.referred = Referred::Display;
    let mut current = photograph();
    current.display = Display::for_image_with(&image, &stats, Startup::default());
    current.image = Arc::new(image);
    current.stats = stats;
    harness.state_mut().current = Some(current);
    harness.run();
    assert!(!dead(&harness, "Curve 1"));
    assert!(!dead(&harness, "Exposure"));
    assert!(lit(&harness, "Set photo mode") && !lit(&harness, "Set data mode"));
    assert!(harness.query_by_label("Window 0").is_none());
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
        .set_colormap(Colormap::Viridis, true);
    harness.run();
    for index in 0..ToneMap::ALL.len() {
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
    assert!(harness.query_by_label("File list").is_none());
    drop(harness);

    let mut harness = build(WINDOW, 3, panels());
    harness.state_mut().input.paste = true;
    harness.run();
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
    assert_eq!(
        click(&mut harness, "File list"),
        [Command::Press(Control::Filmstrip)]
    );
}

/// The file list, as a frame is handed it: `count` files' rows, with
/// `current` the row of the file on screen.
fn strip(count: usize, current: Option<usize>, reveal: bool) -> filmstrip::Input {
    let rows: Arc<[filmstrip::Row]> = (0..count)
        .map(|index| filmstrip::Row {
            index: index + 1,
            name: format!("{index:03}.png"),
            path: format!("birds/{index:03}.png"),
            format: None,
            size: None,
            bytes: None,
            modified: None,
            taken: None,
            thumb: None,
            missing: None,
        })
        .collect();
    let height = filmstrip::row_height(filmstrip::SLOT_MIN, None, 1.0);
    let tops: Vec<f32> = (0..=count).map(|row| row as f32 * height).collect();
    filmstrip::Input {
        rows,
        tops: Arc::from(tops),
        slot: filmstrip::SLOT_MIN,
        listing: 0,
        current,
        order: filmstrip::Order::default(),
        back: false,
        forward: true,
        reveal,
        visible: 0..0,
    }
}

/// The file list up: it says which rows are on screen, a press on a row
/// asks for that file, its menu offers every sort,
/// and the pair at its head are dead or alive by whether there is
/// anywhere to go.
#[test]
fn the_file_list_offers_its_rows_its_menus_and_the_way_back() {
    let mut with_list = panels();
    with_list.show_filmstrip = true;
    let mut harness = open(WINDOW, 3, with_list);
    harness.state_mut().input.filmstrip = Some(strip(3, Some(0), false));
    harness.run();
    assert!(
        asked(&harness).iter().any(|command| matches!(
            command,
            Command::FilmstripVisible(rows) if rows.start == 0 && rows.end == 3
        )),
        "{:?}",
        asked(&harness)
    );
    assert_eq!(
        click(&mut harness, "Show file 2"),
        [Command::Press(Control::Thumb(1))]
    );

    assert!(harness.get_by_label("Back").accesskit_node().is_disabled());
    assert!(
        !harness
            .get_by_label("Forward")
            .accesskit_node()
            .is_disabled()
    );
    assert_eq!(
        click(&mut harness, "Forward"),
        [Command::Press(Control::Forward)]
    );

    assert_eq!(click(&mut harness, "Sort"), []);
    assert_eq!(
        click(&mut harness, "Area"),
        [Command::Press(Control::SortBy(filmstrip::Sort::Area))]
    );
    assert_eq!(click(&mut harness, "Sort"), []);
    assert_eq!(
        click(&mut harness, "Descending"),
        [Command::Press(Control::SortDirection(
            filmstrip::Direction::Descending
        ))]
    );
}

/// A dead control still names itself when the pointer rests on it — with
/// why it is dead, which is what the application says there — whether it is
/// a button drawn by hand that only senses the hover, or a widget egui itself
/// has disabled.
#[test]
fn a_dead_control_still_has_a_tooltip() {
    let mut with_list = panels();
    with_list.show_filmstrip = true;
    let mut harness = open(WINDOW, 3, with_list);
    harness.state_mut().input.filmstrip = Some(strip(3, Some(0), false));
    harness.state_mut().named = true;
    harness.run();

    for (label, tip) in [
        ("Back", Tip::Control(Control::Back)),
        ("HDR", Tip::Control(Control::Output)),
        ("Forward", Tip::Control(Control::Forward)),
    ] {
        harness.get_by_label(label).hover();
        harness.run();
        let said = format!("{tip:?}");
        assert!(harness.query_by_label(&said).is_some(), "{label}");
    }
    assert!(harness.get_by_label("Back").accesskit_node().is_disabled());
    assert!(harness.get_by_label("HDR").accesskit_node().is_disabled());
}

/// The file list's right edge is a grip: dragged, it asks for the slot
/// that puts the edge under the pointer, held between the narrowest and
/// the widest the list goes, however far past either the pointer went.
#[test]
fn the_file_list_is_widened_by_dragging_its_edge() {
    let mut with_list = panels();
    with_list.show_filmstrip = true;
    let mut harness = open(WINDOW, 3, with_list);
    harness.state_mut().input.filmstrip = Some(strip(3, Some(0), false));
    harness.run();
    // The panel's left edge is its first row's.
    let row = harness.get_by_label("Show file 1").rect();
    let edge = row.min.x + filmstrip::width(filmstrip::SLOT_MIN);
    let y = row.center().y;
    let slots = |commands: Vec<Command>| -> Vec<f32> {
        commands
            .into_iter()
            .filter_map(|command| match command {
                Command::FilmstripSlot(slot) => Some(slot),
                _ => None,
            })
            .collect()
    };
    assert_eq!(
        slots(drag(&mut harness, [edge, y], [edge + 60.0, y])),
        [filmstrip::SLOT_MIN + 60.0]
    );
    assert_eq!(
        slots(drag(&mut harness, [edge, y], [WINDOW[0], y])),
        [filmstrip::SLOT_MAX]
    );
    assert_eq!(
        slots(drag(&mut harness, [edge - 1.0, y], [0.0, y])),
        [],
        "already at its narrowest"
    );
}

/// The list is put at the file on screen when it is revealed — the file
/// may have been stepped to from anywhere — and only then.
#[test]
fn the_file_list_scrolls_to_the_file_on_screen() {
    let mut with_list = panels();
    with_list.show_filmstrip = true;
    let mut harness = open(WINDOW, 200, with_list);
    harness.state_mut().input.filmstrip = Some(strip(200, Some(150), true));
    harness.step();
    harness
        .state_mut()
        .input
        .filmstrip
        .as_mut()
        .expect("set above")
        .reveal = false;
    harness.run_steps(30);
    let visible = asked(&harness)
        .into_iter()
        .filter_map(|command| match command {
            Command::FilmstripVisible(rows) => Some(rows),
            _ => None,
        })
        .next_back()
        .expect("the rows on screen were said");
    assert!(visible.contains(&150), "{visible:?}");
    assert!(!visible.contains(&0), "{visible:?}");
    assert!(harness.query_by_label("Show file 151").is_some());
    assert!(harness.query_by_label("Show file 1").is_none());
}

/// The interface over nothing: no list, no picture, nothing on its way.
fn empty(paste: bool, picking: bool) -> Harness<'static, State> {
    let mut harness = build(WINDOW, 0, panels());
    let state = harness.state_mut();
    state.current = None;
    state.input.empty = true;
    state.input.paste = paste;
    state.input.picking = picking;
    harness.run();
    harness
}

/// Before the first file is read, the top bar already holds what is about
/// the list and the window — with a list, the file list's toggle and the
/// pair that steps through it with the count between them; the toggle that
/// hides the interface always — and nothing about a file.
#[test]
fn the_top_bar_has_its_toggles_before_the_first_picture() {
    let mut harness = build(WINDOW, 3, panels());
    harness.state_mut().current = None;
    harness.run();
    assert!(harness.query_by_label("Zoom").is_none());
    assert!(harness.query_by_label("File").is_none());
    assert_eq!(
        click(&mut harness, "File list"),
        [Command::Press(Control::Filmstrip)]
    );
    assert_eq!(
        click(&mut harness, "Previous file"),
        [Command::Press(Control::Previous)]
    );
    assert_eq!(
        click(&mut harness, "Choose a file"),
        [Command::Press(Control::Chooser)]
    );
    assert_eq!(
        click(&mut harness, "Next file"),
        [Command::Press(Control::Next)]
    );
    assert_eq!(
        click(&mut harness, "Maximize"),
        [Command::Press(Control::Maximize)]
    );
    drop(harness);

    let mut harness = build(WINDOW, 1, panels());
    harness.state_mut().current = None;
    harness.run();
    for label in ["File list", "Previous file", "Next file", "Choose a file"] {
        assert!(harness.query_by_label(label).is_none(), "{label}");
    }
    assert!(harness.query_by_label("Maximize").is_some());
}

/// The facts at the far end of the top bar yield to the name: a wide window
/// has all of them, and a window too narrow for the name beside them has
/// the name and none of them.
#[test]
fn the_top_bar_drops_the_facts_before_the_name() {
    let harness = open(WINDOW, 3, panels());
    assert!(harness.query_by_label("photo.png").is_some());
    assert!(
        harness.query_by_label_contains("\u{00d7}").is_some(),
        "a wide window has room for the size"
    );
    assert!(
        harness.query_by_label_contains("sRGB").is_some(),
        "and for the color space"
    );
    drop(harness);

    let harness = open([300.0, 300.0], 3, panels());
    assert!(
        harness.query_by_label("photo.png").is_some(),
        "the name is still there"
    );
    assert!(
        harness.query_by_label_contains("\u{00d7}").is_none(),
        "the facts are not"
    );
    assert!(harness.query_by_label("Zoom").is_some());
    assert!(harness.query_by_label("Maximize").is_some());
}

/// A window with nothing open offers the three ways of giving it
/// something, in the middle, each handing back the press its key would;
/// the paste is dead without a picture on the clipboard, and the two that
/// put up the dialog are dead while it is up. The buttons about the
/// picture are dead in the strip, and the ones that would only ever be
/// about a file are not there. With a picture up, or one on its way,
/// none of the three is on screen.
#[test]
fn an_empty_window_offers_to_be_given_something() {
    let mut harness = empty(false, false);
    assert_eq!(
        click(&mut harness, "Open files"),
        [Command::Press(Control::OpenFiles)]
    );
    assert_eq!(
        click(&mut harness, "Open folder"),
        [Command::Press(Control::OpenFolder)]
    );
    assert!(harness.get_by_label("Paste").accesskit_node().is_disabled());
    for label in ["Copy", "Region"] {
        assert!(
            harness.get_by_label(label).accesskit_node().is_disabled(),
            "{label}"
        );
    }
    assert!(harness.query_by_label("File").is_none());
    assert!(harness.query_by_label("Zoom").is_none());
    // The toggles that outlast the picture stay live: what they set is
    // waiting for the next one.
    for label in ["Histogram", "Information", "Help", "Minimap"] {
        assert!(
            !harness.get_by_label(label).accesskit_node().is_disabled(),
            "{label}"
        );
    }
    drop(harness);

    let mut harness = empty(true, false);
    assert_eq!(
        click(&mut harness, "Paste"),
        [Command::Press(Control::Paste)]
    );
    drop(harness);

    // Come back to after a picture, the folder it came from is offered
    // above the rest, and handed back as its own press.
    let mut harness = empty(false, false);
    assert!(harness.query_by_label("Open last folder").is_none());
    harness.state_mut().input.folder = Some("Holiday".to_string());
    harness.run();
    assert_eq!(
        click(&mut harness, "Open last folder"),
        [Command::Press(Control::OpenLastFolder)]
    );
    assert_eq!(
        click(&mut harness, "Open files"),
        [Command::Press(Control::OpenFiles)]
    );
    drop(harness);

    let harness = empty(true, true);
    for label in ["Open files", "Open folder"] {
        assert!(
            harness.get_by_label(label).accesskit_node().is_disabled(),
            "{label}"
        );
    }
    assert!(!harness.get_by_label("Paste").accesskit_node().is_disabled());
    drop(harness);

    // A picture up: the buttons are gone, and the strip's are live again.
    let harness = open(WINDOW, 1, panels());
    assert!(harness.query_by_label("Open files").is_none());
    assert!(harness.query_by_label("Open folder").is_none());
    for label in ["Copy", "Region"] {
        assert!(
            !harness.get_by_label(label).accesskit_node().is_disabled(),
            "{label}"
        );
    }
    drop(harness);

    // A picture on its way: nothing yet, and no buttons either.
    let mut harness = build(WINDOW, 1, panels());
    harness.state_mut().current = None;
    harness.run();
    assert!(harness.query_by_label("Open files").is_none());
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

/// The camera's switch is drawn only for a file that carries the camera's
/// JPEG, wears the name of the picture on screen, and asks for the other.
#[test]
fn the_camera_switch_is_there_only_for_a_file_with_a_camera_jpeg() {
    use crate::image::decode::{CameraJpeg, Rendering};

    let harness = open(WINDOW, 1, panels());
    assert!(harness.query_by_label(chrome::CAMERA_RAW).is_none());
    assert!(harness.query_by_label(chrome::CAMERA_JPEG).is_none());

    let mut harness = open(WINDOW, 1, panels());
    let current = harness.state_mut().current.as_mut().expect("a picture");
    current.camera_jpeg = CameraJpeg::Present([6000, 4000]);
    harness.run();
    assert!(harness.query_by_label(chrome::CAMERA_JPEG).is_none());
    assert_eq!(
        click(&mut harness, chrome::CAMERA_RAW),
        [Command::Press(Control::CameraJpeg)]
    );

    let current = harness.state_mut().current.as_mut().expect("a picture");
    current.rendering = Rendering::CameraJpeg;
    harness.run();
    assert!(harness.query_by_label(chrome::CAMERA_RAW).is_none());
    assert_eq!(
        click(&mut harness, chrome::CAMERA_JPEG),
        [Command::Press(Control::CameraJpeg)]
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
            .get_by_label("Open in")
            .accesskit_node()
            .is_disabled()
    );
    assert_eq!(click(&mut harness, "Open in"), []);

    harness.state_mut().input.openers = vec!["Pinta".to_string(), "Darktable".to_string()];
    harness.run();
    assert!(
        !harness
            .get_by_label("Open in")
            .accesskit_node()
            .is_disabled()
    );
    assert_eq!(click(&mut harness, "Open in"), []);
    assert_eq!(
        click(&mut harness, "Darktable"),
        [Command::Press(Control::Opener(1))]
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
    harness.state_mut().input.modifiers = crate::gestures::Mods::SHIFT;
    harness.run();
    let commands = drag(&mut harness, inside, to);
    assert!(held(&commands, Grip::Inside), "{commands:?}");

    // Off the region, the key held changes nothing: the picture pans. At
    // the picture's top right, clear of the region's panel at its top left.
    let outside = screen_point(&harness, [3.5, 0.5]);
    assert!(!region.contains(image_point(&harness, outside)));
    let commands = drag(
        &mut harness,
        outside,
        [outside[0] - 30.0, outside[1] + 30.0],
    );
    assert!(panned(&commands), "{commands:?}");

    harness.state_mut().input.modifiers = crate::gestures::Mods::empty();
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

/// A drag of a button other than the primary pans only where its slot
/// says so: the middle button drags nothing by default, and pans once the
/// configuration gives it that.
#[test]
fn a_middle_drag_pans_only_where_its_slot_says_so() {
    use crate::gestures::{Behavior, DragAction, Gestures, Slot};
    let mut harness = open(WINDOW, 1, panels());
    let (from, to) = ([400.0, 300.0], [430.0, 340.0]);
    let middle = egui::PointerButton::Middle;
    assert_eq!(drag_with(&mut harness, middle, from, to), []);

    let mut gestures = Gestures::table();
    gestures.set(
        Slot::read("image.middle.drag").unwrap(),
        Behavior::Drag(DragAction::Pan),
    );
    harness.state_mut().input.gestures = std::rc::Rc::new(gestures);
    harness.run();
    let commands = drag_with(&mut harness, middle, from, to);
    assert!(
        !commands.is_empty()
            && commands
                .iter()
                .all(|command| matches!(command, Command::Drag(_))),
        "{commands:?}"
    );
    // And the primary, given a box to draw, draws it rather than panning.
    let mut gestures = Gestures::table();
    gestures.set(
        Slot::read("image.left.drag").unwrap(),
        Behavior::Drag(DragAction::ZoomBox),
    );
    harness.state_mut().input.gestures = std::rc::Rc::new(gestures);
    harness.run();
    let commands = drag(&mut harness, from, to);
    assert!(
        matches!(
            commands.first(),
            Some(Command::Grab {
                grab: Grab::Zoom,
                ..
            })
        ),
        "{commands:?}"
    );
}

/// A click of a button whose click slot names a key is handed back as a
/// click of that button — the side button's by default — and one whose
/// slot names nothing is not handed back at all.
#[test]
fn a_click_is_handed_back_where_its_slot_names_a_key() {
    use crate::gestures::{Behavior, Button, Gestures, Kind, Slot};
    let mut harness = open(WINDOW, 1, panels());
    let at = [400.0, 300.0];
    let click = |harness: &mut Harness<'static, State>, button| drag_with(harness, button, at, at);
    assert_eq!(
        click(&mut harness, egui::PointerButton::Extra1),
        [Command::Click(Button::Back, Kind::Click)]
    );
    assert_eq!(
        click(&mut harness, egui::PointerButton::Extra2),
        [Command::Click(Button::Forward, Kind::Click)]
    );
    assert_eq!(click(&mut harness, egui::PointerButton::Middle), []);
    // The secondary holds the loupe up, so letting go of it is no click.
    assert_eq!(click(&mut harness, egui::PointerButton::Secondary), []);

    let mut gestures = Gestures::table();
    gestures.set(
        Slot::read("image.middle.click").unwrap(),
        Behavior::Click("interface.grid".to_string()),
    );
    harness.state_mut().input.gestures = std::rc::Rc::new(gestures);
    harness.run();
    assert_eq!(
        click(&mut harness, egui::PointerButton::Middle),
        [Command::Click(Button::Middle, Kind::Click)]
    );
}

/// A double click of the primary on the picture is handed back as one —
/// its first click being nothing, the primary's click having no slot — and
/// a double click on one of the region's handles is the handle's.
#[test]
fn a_double_click_is_handed_back_where_its_slot_names_a_key() {
    use crate::gestures::{Button, Kind};
    let mut harness = open(WINDOW, 1, panels());
    // A step of the harness is a quarter of a second, and a double click
    // is two clicks inside three tenths of one: the clock is set by hand,
    // a twentieth of a second on for each press and release.
    let twice = |harness: &mut Harness<'static, State>, at: [f32; 2]| {
        let pos = egui::pos2(at[0], at[1]);
        harness.event(egui::Event::PointerMoved(pos));
        harness.step();
        let mut time = harness.ctx.input(|input| input.time);
        let mut commands = Vec::new();
        for _ in 0..2 {
            harness.state_mut().commands.clear();
            for pressed in [true, false] {
                time += 0.05;
                harness.input_mut().time = Some(time);
                harness.event(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
                harness.step();
            }
            commands.extend(asked(harness));
        }
        commands
    };
    assert_eq!(
        twice(&mut harness, [400.0, 300.0]),
        [Command::Click(Button::Left, Kind::DoubleClick)]
    );

    let region = Region {
        x: 1,
        y: 1,
        width: 1,
        height: 1,
    };
    harness.state_mut().input.selection = Selection::Shown(region);
    // Stepped rather than run: the region brings its panel up, which fades
    // in by a clock this test holds still.
    harness.step();
    let right = screen_point(&harness, [2.0, 1.5]);
    let handle = Command::Handle(Grip::Edge(Side::Right));
    assert_eq!(twice(&mut harness, right), [handle.clone(), handle]);
}

/// The minimap centers for the button its slot names and for no other.
#[test]
fn the_minimap_centers_for_the_button_its_slot_names() {
    use crate::gestures::{Behavior, Gestures, Slot};
    let mut harness = open(WINDOW, 1, panels());
    let image = [400.0, 300.0];
    harness.state_mut().current = Some(picture(400, 300));
    harness.state_mut().input.minimap_on_screen = true;
    harness.run();
    let content = super::chrome::content_area(WINDOW, true, super::chrome::Parts::NONE);
    let map = super::minimap::thumbnail(content, image).expect("room for a map");
    let at = [map.x + map.width / 2.0, map.y + map.height / 2.0];
    let centers = |commands: &[Command]| {
        commands
            .iter()
            .any(|command| matches!(command, Command::Center(_)))
    };
    let middle = egui::PointerButton::Middle;
    assert!(centers(&drag_with(
        &mut harness,
        egui::PointerButton::Primary,
        at,
        at
    )));
    assert!(!centers(&drag_with(&mut harness, middle, at, at)));

    let mut gestures = Gestures::table();
    gestures.set(Slot::read("minimap.left.drag").unwrap(), Behavior::None);
    gestures.set(Slot::read("minimap.middle.drag").unwrap(), Behavior::Center);
    harness.state_mut().input.gestures = std::rc::Rc::new(gestures);
    harness.run();
    assert!(centers(&drag_with(&mut harness, middle, at, at)));
    // The left button still centers on a click, its drag gone.
    let to = [at[0] + 10.0, at[1]];
    let commands = drag_with(&mut harness, egui::PointerButton::Primary, at, to);
    assert!(!centers(&commands), "{commands:?}");
    assert!(centers(&drag_with(
        &mut harness,
        egui::PointerButton::Primary,
        at,
        at
    )));
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
    let content = super::chrome::content_area(WINDOW, true, super::chrome::Parts::NONE);
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

/// Only a georeferenced file brings the rows of coordinates to that menu,
/// and only the coordinates it can give: a raster in a system the table
/// holds offers all three and the two ways of writing a latitude.
#[test]
fn the_pixel_menu_offers_coordinates_only_for_a_map() {
    let mut harness = open(WINDOW, 1, panels());
    assert_eq!(click(&mut harness, "Pixel format"), []);
    assert!(harness.query_by_label("Projected").is_none());
    assert!(harness.query_by_label("DMS").is_none());

    let mut harness = build(WINDOW, 1, panels());
    let placed = crate::image::geo::Tags {
        // Projected, on UTM zone 18N.
        directory: vec![1, 1, 0, 2, 1024, 0, 1, 1, 3072, 0, 1, 32618],
        scale: vec![1.0, 1.0, 0.0],
        tiepoint: vec![0.0, 0.0, 0.0, 583_000.0, 4_507_000.0, 0.0],
        ..Default::default()
    };
    harness
        .state_mut()
        .current
        .as_mut()
        .expect("a picture")
        .exif
        .georeference = crate::image::geo::Georeference::read(&placed);
    harness.run();
    assert_eq!(click(&mut harness, "Pixel format"), []);
    assert_eq!(
        click(&mut harness, "Geographic"),
        [Command::Press(Control::Coordinates(
            CoordinateFormat::Geographic
        ))]
    );
    assert_eq!(click(&mut harness, "Pixel format"), []);
    assert_eq!(
        click(&mut harness, "DMS"),
        [Command::Press(Control::Geographic(GeographicFormat::Dms))]
    );
}

/// A raster placed in latitude and longitude wears two maps at the head of
/// its Georeference section, its middle and its box; one placed in a system
/// nothing can lift wears neither.
#[test]
fn a_georeference_offers_its_middle_and_its_box() {
    let mut shown = panels();
    shown.side = Some(super::side::Side::Info);
    let place = |system: u16| {
        let placed = crate::image::geo::Tags {
            directory: vec![1, 1, 0, 2, 1024, 0, 1, 1, 3072, 0, 1, system],
            scale: vec![1.0, 1.0, 0.0],
            tiepoint: vec![0.0, 0.0, 0.0, 583_000.0, 4_507_000.0, 0.0],
            ..Default::default()
        };
        let mut harness = open(WINDOW, 1, shown);
        // A picture of its own, so the panel is not the one already drawn.
        let mut current = photograph();
        let exif = &mut current.exif;
        exif.georeference = crate::image::geo::Georeference::read(&placed);
        exif.sections.push(crate::image::exif::Section {
            group: crate::image::exif::Group::Georeference,
            entries: vec![crate::image::exif::Entry {
                name: "System".to_string(),
                value: format!("EPSG:{system}"),
            }],
        });
        harness.state_mut().current = Some(current);
        harness.run();
        harness
    };
    // Projected, on UTM zone 18N.
    let mut harness = place(32618);
    assert_eq!(
        click(&mut harness, "Open image center in map"),
        [Command::Press(Control::OpenMapCenter)]
    );
    assert_eq!(
        click(&mut harness, "Open image bounds in map"),
        [Command::Press(Control::OpenMapBounds)]
    );
    // User-defined.
    let harness = place(32767);
    assert!(harness.query_by_label("Open image center in map").is_none());
    assert!(harness.query_by_label("Open image bounds in map").is_none());
}

/// The chooser: opened, its field takes the keyboard, and what is typed
/// and pressed comes back as commands rather than reaching the window —
/// the query, the cursor's moves, `Enter` on the row under the cursor, a
/// click on a row. `Esc` closes it, and with it gone
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
            missing: None,
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
        reveal: true,
        visible: 0..0,
    });
    harness.run();
    assert!(egui::Popup::is_id_open(&harness.ctx, chooser::id()));
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

    // The key that closes it is kept from egui by the window, and reaches
    // the key table instead: nothing here reads it.
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

/// With more than one file, the menu of the file offers taking it off the
/// list too, with its key beside it; with one, it does not.
#[test]
fn the_file_menu_offers_the_removal_only_from_a_list() {
    let mut harness = open(WINDOW, 1, panels());
    assert_eq!(click(&mut harness, "File"), []);
    assert!(harness.query_by_label("Remove from list").is_none());

    let mut harness = open(WINDOW, 3, panels());
    assert_eq!(click(&mut harness, "File"), []);
    assert_eq!(
        click(&mut harness, "Remove from list"),
        [Command::Press(Control::Remove)]
    );
}

/// The button before the name opens the menu of the file: its name, path
/// and URI copied, the rename and the deletion, each asking for its own
/// control, and the menu closing on the press.
#[test]
fn the_file_menu_offers_the_copies_the_rename_and_the_deletion() {
    let mut harness = open(WINDOW, 1, panels());
    assert!(harness.query_by_label("Copy name").is_none());
    assert_eq!(click(&mut harness, "File"), []);
    for label in [
        "Copy name",
        "Copy path",
        "Copy URI",
        "Rename\u{2026}",
        "Delete",
        "Export\u{2026}",
    ] {
        assert!(
            harness
                .query_by_role_and_label(egui::accesskit::Role::Button, label)
                .is_some(),
            "{label} is on the menu"
        );
    }
    assert_eq!(
        click(&mut harness, "Rename\u{2026}"),
        [Command::Press(Control::Rename)]
    );
    harness.run();
    assert!(
        harness.query_by_label("Delete").is_none(),
        "the menu closes on the press"
    );
    assert_eq!(click(&mut harness, "File"), []);
    assert_eq!(
        click(&mut harness, "Delete"),
        [Command::Press(Control::Delete)]
    );
    assert_eq!(click(&mut harness, "File"), []);
    assert_eq!(
        click(&mut harness, "Copy name"),
        [Command::Press(Control::Copies(Copies::Name))]
    );
    assert_eq!(click(&mut harness, "File"), []);
    assert_eq!(
        click(&mut harness, "Copy URI"),
        [Command::Press(Control::Copies(Copies::Uri))]
    );
}

/// The rename dialog takes the keyboard with the name's stem selected, so
/// that typing replaces the stem and keeps the extension; says what the
/// field holds as it changes; renames on `Enter` and OK only while the
/// name will do, OK being dead otherwise; and goes on `Esc`, giving the
/// keyboard back.
#[test]
fn the_rename_dialog_takes_the_keys_and_hands_back_the_name() {
    let mut harness = open(WINDOW, 1, panels());
    assert!(!harness.ctx.egui_wants_keyboard_input());
    let dialog = |name: &str, verdict: Verdict, opened: bool| {
        Some(rename::Input {
            current: "photo.png".to_string(),
            name: name.to_string(),
            verdict,
            opened,
        })
    };
    harness.state_mut().input.rename = dialog("photo.png", Verdict::Unchanged, true);
    harness.run();
    harness.state_mut().input.rename = dialog("photo.png", Verdict::Unchanged, false);
    harness.run();
    assert!(harness.ctx.egui_wants_keyboard_input());

    harness.state_mut().commands.clear();
    harness.event(egui::Event::Text("x".to_string()));
    harness.step();
    assert!(
        asked(&harness).contains(&Command::Name("x.png".to_string())),
        "{:?}",
        asked(&harness)
    );

    let pressed = |harness: &mut Harness<'static, State>, key| {
        harness.state_mut().commands.clear();
        harness.key_press(key);
        harness.step();
        asked(harness)
    };
    // The name as it is: `Enter` has nothing to do but put the dialog away.
    assert!(
        pressed(&mut harness, egui::Key::Enter).contains(&Command::Press(Control::CancelRename))
    );

    // A name that will not do: OK is dead, and `Enter` does nothing.
    harness.state_mut().input.rename = dialog(
        "b.png",
        Verdict::Refused(super::rename::Refusal::Taken),
        false,
    );
    harness.run();
    assert!(harness.get_by_label("OK").accesskit_node().is_disabled());
    let after_enter = pressed(&mut harness, egui::Key::Enter);
    assert!(
        !after_enter.iter().any(|command| matches!(
            command,
            Command::Press(Control::RenameTo | Control::CancelRename)
        )),
        "{after_enter:?}"
    );

    // One that will: `Enter` and OK both rename.
    harness.state_mut().input.rename = dialog("c.png", Verdict::Fine(None), false);
    harness.run();
    assert!(!harness.get_by_label("OK").accesskit_node().is_disabled());
    assert!(pressed(&mut harness, egui::Key::Enter).contains(&Command::Press(Control::RenameTo)));
    assert_eq!(
        click(&mut harness, "OK"),
        [Command::Press(Control::RenameTo)]
    );
    assert_eq!(
        click(&mut harness, "Cancel"),
        [Command::Press(Control::CancelRename)]
    );

    // `Esc` is the modal's own, and comes back as the cancel.
    assert!(
        pressed(&mut harness, egui::Key::Escape).contains(&Command::Press(Control::CancelRename))
    );
    harness.state_mut().input.rename = None;
    harness.run();
    harness.run();
    assert!(!harness.ctx.egui_wants_keyboard_input());
    assert!(harness.query_by_label("OK").is_none());
}

/// The export dialog takes the keyboard as it opens, with the stem of its
/// name selected; hands back what the field holds and which format was
/// pressed; says its warnings; exports on `Enter` and Export only while the
/// name will do; and goes on `Esc`.
#[test]
fn the_export_dialog_takes_the_keys_and_hands_back_the_name_and_format() {
    use super::export::{self, Format, Warning};
    let mut harness = open(WINDOW, 1, panels());
    let dialog = |name: &str, verdict: export::Verdict, opened: bool| {
        Some(export::Input {
            source: "photo.png".to_string(),
            name: name.to_string(),
            format: Format::Png,
            quality: 90,
            resize: export::Resize::new([800, 600]),
            verdict,
            warnings: vec![Warning::MetadataDropped],
            opened,
            busy: false,
        })
    };
    harness.state_mut().input.export = dialog("photo-edited.png", export::Verdict::Fine, true);
    harness.run();
    harness.state_mut().input.export = dialog("photo-edited.png", export::Verdict::Fine, false);
    harness.run();
    assert!(harness.ctx.egui_wants_keyboard_input());
    assert!(
        harness
            .query_by_label_contains("Metadata are not exported")
            .is_some()
    );

    harness.state_mut().commands.clear();
    harness.event(egui::Event::Text("x".to_string()));
    harness.step();
    assert!(
        asked(&harness).contains(&Command::ExportName("x.png".to_string())),
        "{:?}",
        asked(&harness)
    );
    assert!(
        harness.query_by_label("Export\u{2026}").is_some(),
        "the title"
    );
    assert!(
        harness.query_by_label("Warnings").is_none(),
        "painted, not a label"
    );
    harness.state_mut().commands.clear();
    harness
        .get_by_role_and_label(egui::accesskit::Role::RadioButton, "JPG")
        .click();
    harness.run();
    assert_eq!(
        asked(&harness),
        [Command::Press(Control::ExportAs(Format::Jpeg))]
    );
    assert!(
        harness
            .get_by_role(egui::accesskit::Role::Slider)
            .accesskit_node()
            .is_disabled(),
        "the quality is JPG's alone"
    );

    // The size boxes: the name and three more, and typing in one asks for
    // that box's text.
    let boxes = harness
        .get_all_by_role(egui::accesskit::Role::TextInput)
        .count();
    assert_eq!(boxes, 4);
    harness
        .get_all_by_role(egui::accesskit::Role::TextInput)
        .find(|node| node.value().as_deref() == Some("800"))
        .expect("the width's box")
        .focus();
    harness.run();
    harness.state_mut().commands.clear();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.event(egui::Event::Text("5".to_string()));
    harness.step();
    assert_eq!(
        asked(&harness),
        [Command::ExportSize(
            export::Dimension::Width,
            "5".to_string()
        )]
    );

    let pressed = |harness: &mut Harness<'static, State>, key| {
        harness.state_mut().commands.clear();
        harness.key_press(key);
        harness.step();
        asked(harness)
    };
    // A name that will not do: Export is dead, and `Enter` does nothing.
    harness.state_mut().input.export = dialog(
        "photo.png",
        export::Verdict::Refused(super::rename::Refusal::Taken),
        false,
    );
    harness.run();
    assert!(
        harness
            .get_by_label("Export")
            .accesskit_node()
            .is_disabled()
    );
    let after_enter = pressed(&mut harness, egui::Key::Enter);
    assert!(
        !after_enter
            .iter()
            .any(|command| matches!(command, Command::Press(Control::ExportTo))),
        "{after_enter:?}"
    );

    // A size that will not do is as dead as a name that will not.
    let mut refused = dialog("b.png", export::Verdict::Fine, false).expect("a dialog");
    refused
        .resize
        .edit(export::Dimension::Height, "0".to_string());
    harness.state_mut().input.export = Some(refused);
    harness.run();
    assert!(
        harness
            .get_by_label("Export")
            .accesskit_node()
            .is_disabled()
    );
    assert!(
        !pressed(&mut harness, egui::Key::Enter)
            .iter()
            .any(|command| matches!(command, Command::Press(Control::ExportTo)))
    );

    // So is an export still being written, and the dialog says so.
    let mut busy = dialog("b.png", export::Verdict::Fine, false).expect("a dialog");
    busy.busy = true;
    harness.state_mut().input.export = Some(busy);
    harness.run();
    assert!(
        harness
            .get_by_label("Export")
            .accesskit_node()
            .is_disabled()
    );
    assert!(
        !pressed(&mut harness, egui::Key::Enter)
            .iter()
            .any(|command| matches!(command, Command::Press(Control::ExportTo)))
    );

    // One that will: `Enter` and Export both export.
    harness.state_mut().input.export = dialog("b.png", export::Verdict::Fine, false);
    harness.run();
    assert!(pressed(&mut harness, egui::Key::Enter).contains(&Command::Press(Control::ExportTo)));
    assert_eq!(
        click(&mut harness, "Export"),
        [Command::Press(Control::ExportTo)]
    );
    assert!(
        pressed(&mut harness, egui::Key::Escape).contains(&Command::Press(Control::CancelExport))
    );
    harness.state_mut().input.export = None;
    harness.run();
    harness.run();
    assert!(!harness.ctx.egui_wants_keyboard_input());
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
            missing: None,
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
/// and each row's three columns — with the button under it, and takes no
/// key from the window.
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
                    when: Some(help::Condition {
                        words: "a region selected",
                        met: false,
                    }),
                },
            ],
        },
        help::Section {
            title: "Files",
            rows: vec![help::Row {
                key: "], Page Down".to_string(),
                does: "Next file",
                when: Some(help::Condition {
                    words: "more than one file",
                    met: true,
                }),
            }],
        },
    ];
    assert!(harness.query_by_label("Zoom and position").is_none());
    egui::Popup::open_id(&harness.ctx, help::id());
    harness.run();
    assert!(egui::Popup::is_id_open(&harness.ctx, help::id()));
    for label in [
        "Key",
        "Action",
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

    // The button under the table hands back its press, and takes no key
    // from the window either.
    assert_eq!(
        click(&mut harness, "Edit configuration file"),
        [Command::Press(Control::EditConfig)]
    );
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
/// nothing wide, which egui refuses to lay out.
/// A line the configuration left with no key says so in its key column,
/// in the table and stacked alike, rather than leaving it blank.
#[test]
fn the_help_popup_says_a_line_is_unbound() {
    for window in [
        WINDOW,
        [
            super::PANEL_WIDTH + 2.0 * super::PADDING + 2.0 * SIDE_WIDTH + 20.0,
            700.0,
        ],
    ] {
        let mut harness = open(window, 1, panels());
        harness.state_mut().help = vec![help::Section {
            title: "Files",
            rows: vec![
                help::Row {
                    key: String::new(),
                    does: "Undo the last rename, deletion or removal",
                    when: None,
                },
                help::Row {
                    key: "Ctrl+E".to_string(),
                    does: "Export the picture as shown to a new JPG or PNG",
                    when: None,
                },
            ],
        }];
        egui::Popup::open_id(&harness.ctx, help::id());
        harness.run();
        assert_eq!(harness.query_all_by_label(help::UNBOUND).count(), 1);
        assert!(harness.query_by_label("Ctrl+E").is_some());
    }
}

#[test]
fn the_help_popup_stacks_its_rows_in_a_narrow_window() {
    let mut harness = open(
        [
            super::PANEL_WIDTH + 2.0 * super::PADDING + 2.0 * SIDE_WIDTH + 20.0,
            700.0,
        ],
        1,
        panels(),
    );
    harness.state_mut().help = vec![help::Section {
        title: "Files",
        rows: vec![help::Row {
            key: "], Page Down".to_string(),
            does: "Next file",
            when: Some(help::Condition {
                words: "more than one file",
                met: true,
            }),
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
    for heading in ["Key", "Action", "When"] {
        assert!(
            harness.query_by_label(heading).is_none(),
            "{heading} heads nothing when stacked"
        );
    }
}

/// The depth toggle is drawn only for a picture that carries a depth map,
/// and a press on it asks for the map to be drawn in the picture's place.
#[test]
fn the_depth_toggle_is_there_only_for_a_picture_with_a_depth_map() {
    use crate::image::depth::DepthMap;

    let harness = open(WINDOW, 1, panels());
    let name = Control::Depth.label();
    assert!(harness.query_by_label(&name).is_none());

    let mut harness = open(WINDOW, 1, panels());
    let current = harness.state_mut().current.as_mut().expect("a picture");
    let mut image = (*current.image).clone();
    image.depth = Some(Arc::new(DepthMap {
        width: 2,
        height: 2,
        samples: Samples::U8 {
            channels: Channels::Gray,
            data: vec![0, 85, 170, 255],
        },
        scale: None,
    }));
    current.image = Arc::new(image);
    harness.run();
    assert_eq!(click(&mut harness, &name), [Command::Press(Control::Depth)]);
}

/// The gain map's toggle is drawn only for a picture that carries a gain
/// map, and a press on it asks for the map to be drawn in the picture's
/// place.
#[test]
fn the_gain_map_toggle_is_there_only_for_a_picture_with_a_gain_map() {
    use crate::image::gain_map::{GainMap, Lift};

    let harness = open(WINDOW, 1, panels());
    let name = Control::GainMap.label();
    assert!(harness.query_by_label(&name).is_none());

    let mut harness = open(WINDOW, 1, panels());
    let current = harness.state_mut().current.as_mut().expect("a picture");
    let mut image = (*current.image).clone();
    image.gain_map = Some(Arc::new(GainMap {
        width: 2,
        height: 2,
        channels: 1,
        data: crate::image::gain_map::Data::Codes(vec![0, 85, 170, 255]),
        lift: Lift::Apple { headroom: 4.0 },
    }));
    current.image = Arc::new(image);
    harness.run();
    assert_eq!(
        click(&mut harness, &name),
        [Command::Press(Control::GainMap)]
    );
}

/// Where `text` was painted inside the side panel, if it was: the words of
/// the histogram's headers, which are painted rather than named.
fn painted_in_side(harness: &Harness<'static, State>, text: &str) -> Option<egui::Rect> {
    let side = egui::PanelState::load(&harness.ctx, egui::Id::new("side"))?.outer_rect;
    harness
        .output()
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(shown) if shown.galley.text() == text => {
                let rect = egui::Rect::from_min_size(shown.pos, shown.galley.size());
                (rect.min.x >= side.min.x && rect.max.x <= side.max.x).then_some(rect)
            }
            _ => None,
        })
}

/// The histogram is a column of sections, one for each stage the picture
/// passes through: a gain map's only for a picture that carries one, whose
/// button beside it asks for the map in the picture's place.
#[test]
fn the_histogram_has_a_section_for_each_stage_the_picture_passes_through() {
    use crate::image::gain_map::{GainMap, Lift};

    let mut with_histogram = panels();
    with_histogram.side = Some(super::side::Side::Histogram);
    let mut harness = open(WINDOW, 1, with_histogram);
    for title in ["Output", "Display", "Image"] {
        assert!(painted_in_side(&harness, title).is_some(), "{title}");
    }
    assert!(painted_in_side(&harness, "Gain map").is_none());
    assert!(harness.query_by_label("Show gain map").is_none());

    let current = harness.state_mut().current.as_mut().expect("a picture");
    let mut image = (*current.image).clone();
    image.gain_map = Some(Arc::new(GainMap {
        width: 2,
        height: 2,
        channels: 1,
        data: crate::image::gain_map::Data::Codes(vec![0, 85, 170, 255]),
        lift: Lift::Apple { headroom: 4.0 },
    }));
    current.image = Arc::new(image);
    harness.run();
    let output = painted_in_side(&harness, "Output").expect("Output");
    let display = painted_in_side(&harness, "Display").expect("Display");
    let gain_map = painted_in_side(&harness, "Gain map").expect("Gain map");
    let file = painted_in_side(&harness, "Image").expect("Image");
    assert!(
        output.min.y < display.min.y
            && display.min.y < gain_map.min.y
            && gain_map.min.y < file.min.y,
        "in the order the data flows, read from the screen back"
    );
    assert_eq!(
        click(&mut harness, "Show gain map"),
        [Command::Press(Control::ShowGainMap)]
    );
}

/// A raw's profile has the Profile section in the Gain map's place, its
/// square the profile's switch and its parts under its lift in the order
/// they are applied, read upward; and the bottom bar's toggle shows the
/// gain map it amounts to.
#[test]
fn a_raws_profile_has_a_section_and_a_switch_of_its_own() {
    use crate::image::gain_map::{Data, GainMap, Lift, Profile};

    let mut with_histogram = panels();
    with_histogram.side = Some(super::side::Side::Histogram);
    let mut harness = open(WINDOW, 1, with_histogram);
    assert!(harness.query_by_label(&Control::Profile.label()).is_none());

    let current = harness.state_mut().current.as_mut().expect("a picture");
    let mut image = (*current.image).clone();
    image.gain_map = Some(Arc::new(GainMap {
        width: 2,
        height: 2,
        channels: 3,
        data: Data::Ratios(
            [0.5, 1.0, 2.0, 8.0]
                .repeat(3)
                .into_iter()
                .map(half::f16::from_f32)
                .collect(),
        ),
        lift: Lift::Profile(Profile {
            baseline: 4.12,
            table: None,
            curve: None,
            range: [-1.0, 3.0],
            to_curve: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        }),
    }));
    current.image = Arc::new(image);
    harness.state_mut().input.pointer = Some([0, 0]);
    harness.run();
    let display = painted_in_side(&harness, "Display").expect("Display");
    let profile = painted_in_side(&harness, "Profile").expect("Profile");
    // The pointer's lift a channel to the cell on the row under the plot,
    // not the gain map's one line of words; the header carries nothing
    // but the switch.
    assert!(painted_in_side(&harness, "\u{2212}1.0").is_some());
    assert!(painted_in_side(&harness, "\u{2212}1.0 / \u{2212}1.0 / \u{2212}1.0 stops").is_none());
    let total = painted_in_side(&harness, super::histogram::TOTAL_LIFT).expect("the lift's row");
    assert!(painted_in_side(&harness, "Not applied").is_none());
    let file = painted_in_side(&harness, "Image").expect("Image");
    assert!(display.min.y < profile.min.y && profile.min.y < file.min.y);
    assert!(painted_in_side(&harness, "Gain map").is_none());
    let curve = painted_in_side(&harness, super::histogram::TONE_CURVE).expect("the curve");
    let table = painted_in_side(&harness, super::histogram::GAIN_TABLE).expect("the table");
    // The exposure's row, by its value: the Display section has a row of
    // its own called Exposure above.
    let exposure = painted_in_side(&harness, "+4.1 EV").expect("the exposure's row");
    assert!(
        profile.min.y < total.min.y
            && total.min.y < curve.min.y
            && curve.min.y < table.min.y
            && table.min.y < exposure.min.y,
        "read upward in the order they are applied, under the lift's row"
    );
    let prophoto = painted_in_side(&harness, super::histogram::PROPHOTO).expect("the ProPhoto row");
    assert!(exposure.min.y < prophoto.min.y && prophoto.min.y < file.min.y);
    assert_eq!(
        click(&mut harness, &Control::Profile.label()),
        [Command::Press(Control::Profile)]
    );
    assert_eq!(
        click(&mut harness, &Control::GainMap.label()),
        [Command::Press(Control::GainMap)],
        "the gain map's toggle in the bottom bar"
    );
}

/// On a window too short for the column, the sections run on below the
/// side panel and the wheel over it scrolls them up into view.
#[test]
fn a_short_histogram_scrolls_to_the_sections_below() {
    let mut with_histogram = panels();
    with_histogram.side = Some(super::side::Side::Histogram);
    let mut harness = open([WINDOW[0], 380.0], 1, with_histogram);
    let side = egui::PanelState::load(&harness.ctx, egui::Id::new("side"))
        .expect("the side panel is up")
        .outer_rect;
    assert!(side.height() < 320.0, "{side:?}");
    let before = painted_in_side(&harness, "Image").expect("laid out below");
    assert!(before.min.y > side.max.y, "{before:?} below {side:?}");

    harness.event(egui::Event::PointerMoved(side.center()));
    for _ in 0..4 {
        harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -200.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        // Steps rather than a run: the pointer rests on whatever the
        // column has scrolled under it, which may be a control whose
        // tooltip asks for pass after pass.
        harness.run_steps(4);
    }
    let after = painted_in_side(&harness, "Image").expect("still laid out");
    assert!(after.min.y < before.min.y, "{after:?} from {before:?}");
    assert!(side.contains(after.center()), "{after:?} in {side:?}");
}

/// The information panel's Tags tab, with tags handed in: the field takes
/// the keyboard only when clicked and hands it back on `Esc`, what is typed
/// in it and what is clicked come back as commands, and the Facts tab is a
/// click away.
#[test]
fn the_tags_tab_filters_copies_and_folds() {
    use super::tags;

    let mut shown = panels();
    shown.side = Some(super::side::Side::Info);
    shown.info_tab = tags::Tab::Tags;
    let mut harness = build(WINDOW, 1, shown);
    let row = |kind, depth, name: &str, value: &str| tags::Row {
        kind,
        depth,
        name: name.to_string(),
        value: value.to_string(),
        raw: None,
        group: None,
        name_lit: Vec::new(),
        value_lit: Vec::new(),
        collapsed: false,
        count: 1,
    };
    let rows = vec![
        row(tags::Kind::Group, 0, "EXIF", ""),
        row(tags::Kind::Group, 1, "IFD0", ""),
        row(tags::Kind::Tag, 2, "Orientation", "Rotate 180"),
    ];
    let tops = tags::tops(&rows);
    harness.state_mut().input.tags = Some(tags::Input {
        file: "photo.png".to_string(),
        query: String::new(),
        version: Some("13.55".to_string()),
        state: tags::State::Ready,
        rows: rows.into(),
        tops: tops.into(),
        shown: 1,
        total: 1,
        fold: Some(tags::Fold::Shut),
        marked: None,
        reveal: false,
    });
    harness.run();
    assert!(harness.query_by_label("Copy All").is_none());
    assert!(
        !harness.ctx.egui_wants_keyboard_input(),
        "not until clicked"
    );

    assert!(click(&mut harness, "Toggle group 0").contains(&Command::Press(Control::TagGroup(0))));
    assert!(click(&mut harness, "Copy tag 2").contains(&Command::Press(Control::TagRow(2))));
    assert!(
        click(&mut harness, "Collapse all")
            .contains(&Command::Press(Control::TagsFold(tags::Fold::Shut)))
    );
    // The copy button opens its menu, and an item of it is the press.
    let _ = click(&mut harness, "Copy tags");
    assert!(
        click(&mut harness, "JSON").contains(&Command::Press(Control::TagsCopy(tags::Table::Json)))
    );

    harness
        .get_by_role(egui::accesskit::Role::TextInput)
        .click();
    harness.run();
    assert!(harness.ctx.egui_wants_keyboard_input());
    harness.state_mut().commands.clear();
    harness.event(egui::Event::Text("b".to_string()));
    harness.step();
    assert!(
        asked(&harness).contains(&Command::Filter("b".to_string())),
        "{:?}",
        asked(&harness)
    );
    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(
        !harness.ctx.egui_wants_keyboard_input(),
        "Esc hands it back"
    );

    assert!(
        click(&mut harness, "Curated")
            .contains(&Command::Press(Control::InfoTab(tags::Tab::Facts)))
    );
}

/// While exiftool is not found, the Raw Data tab says so with the three
/// buttons at its foot, and its copy button is dead: no menu opens.
#[test]
fn the_raw_data_tab_without_exiftool_offers_a_way_out() {
    use super::tags;

    let mut shown = panels();
    shown.side = Some(super::side::Side::Info);
    shown.info_tab = tags::Tab::Tags;
    let mut harness = build(WINDOW, 1, shown);
    harness.state_mut().input.tags = Some(tags::Input {
        file: "photo.png".to_string(),
        query: String::new(),
        version: None,
        state: tags::State::NotInstalled {
            configured: "exiftool".to_string(),
        },
        rows: Arc::from([]),
        tops: Arc::from([0.0]),
        shown: 0,
        total: 0,
        fold: None,
        marked: None,
        reveal: false,
    });
    harness.run();
    for (label, control) in [
        ("Visit https://exiftool.org", Control::VisitExiftool),
        ("Edit configuration file", Control::EditConfig),
        ("Refresh", Control::TagsRefresh),
    ] {
        assert!(
            click(&mut harness, label).contains(&Command::Press(control)),
            "{label}: {:?}",
            asked(&harness)
        );
    }
    let _ = click(&mut harness, "Copy tags");
    assert!(harness.query_by_label("JSON").is_none(), "no menu opens");
}

/// The chevrons over the Output band, as the side panel painted them: each
/// a right-pointing mark in the accent, its point halfway down.
fn chevrons_in_side(harness: &Harness<'static, State>) -> Vec<[egui::Pos2; 3]> {
    let side = egui::PanelState::load(&harness.ctx, egui::Id::new("side"))
        .expect("the side panel")
        .outer_rect;
    let accent = egui::Color32::from(Theme::FALLBACK.accent);
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Path(path)
                if path.points.len() == 3
                    && !path.closed
                    && path.stroke.color == egui::epaint::ColorMode::Solid(accent) =>
            {
                let [top, point, foot] = [path.points[0], path.points[1], path.points[2]];
                (top.x == foot.x
                    && point.x > top.x
                    && (point.y - (top.y + foot.y) / 2.0).abs() < 1e-3
                    && side.contains(point))
                .then_some([top, point, foot])
            }
            _ => None,
        })
        .collect()
}

/// The stretch of the Output band past white — what an HDR surface lifts
/// above it, which the panel can only draw at white — wears chevrons, and an
/// SDR surface's band, which ends at white, wears none.
#[test]
fn the_output_band_wears_chevrons_past_white_only_on_a_surface_with_room() {
    let mut with_histogram = panels();
    with_histogram.side = Some(super::side::Side::Histogram);
    let harness = open(WINDOW, 1, with_histogram);
    assert!(chevrons_in_side(&harness).is_empty());

    let mut harness = build(WINDOW, 1, with_histogram);
    let state = harness.state_mut();
    state.input.headroom = Headroom::Above;
    state.input.room = 4.0;
    harness.run();
    let chevrons = chevrons_in_side(&harness);
    assert!(!chevrons.is_empty());
    // All of them past the white line, which on a room of 4 stands short of
    // the band's end: none reaches back into the stretch below white.
    let white = painted_in_side(&harness, "0").expect("the 0 of the row of stops");
    for [top, ..] in chevrons {
        assert!(top.x >= white.min.x, "{top:?} left of {white:?}");
    }
}

/// A picture pitched down a fifth of a right angle, taken at a 26 mm
/// equivalent: what the horizon grid has to go on.
fn pitched() -> Current {
    use crate::image::attitude::Attitude;
    let mut current = picture(5, 3);
    let (sin, cos) = 20f64.to_radians().sin_cos();
    current.exif.attitude = Attitude::gopro([0.0, cos, sin]);
    current.exif.focal_equivalent = Some(26.0);
    current
}

/// With the horizon grid on, the grid is drawn over the picture, and the
/// curve through the pointer with it while the pointer is on the picture.
#[test]
fn the_horizon_grid_follows_the_pointer() {
    let mut on = panels();
    on.horizon_grid = true;
    let mut harness = open(WINDOW, 1, on);
    harness.state_mut().current = Some(pitched());
    let curves = |harness: &mut Harness<'static, State>| {
        harness.run();
        harness
            .output()
            .shapes
            .iter()
            .filter(|shape| matches!(&shape.shape, egui::Shape::Path(path) if !path.closed))
            .count()
    };
    let without = curves(&mut harness);
    harness.state_mut().input.pointer = Some([2, 1]);
    let with = curves(&mut harness);
    assert!(with > without);
    harness.state_mut().panels.horizon_grid = false;
    assert!(curves(&mut harness) < without);
}

/// The horizon grid's toggle is dead and unlit for a file that does not
/// say which way its camera faced, and while another file is on its way
/// in, though the grid is asked for: a press reaches nothing. For a file
/// that does, it is alive and lit, with the spacing after its mark.
#[test]
fn the_horizon_grid_toggle_is_dead_without_an_orientation() {
    let mut on = panels();
    on.horizon_grid = true;
    let mut harness = open(WINDOW, 1, on);
    let state = |harness: &mut Harness<'static, State>| {
        harness.run();
        let node = harness.get_by_label("Horizon grid").accesskit_node();
        let lit = matches!(node.toggled(), Some(egui::accesskit::Toggled::True));
        (node.is_disabled(), lit)
    };
    assert_eq!(state(&mut harness), (true, false));
    assert_eq!(click(&mut harness, "Horizon grid"), []);

    harness.state_mut().current = Some(pitched());
    assert_eq!(state(&mut harness), (false, true));
    assert_eq!(
        click(&mut harness, "Horizon grid"),
        [Command::Press(Control::HorizonGrid)]
    );

    harness.state_mut().input.arriving = Some("b.png".to_string());
    assert_eq!(state(&mut harness), (true, false));
    assert_eq!(click(&mut harness, "Horizon grid"), []);
}

/// For a file that gives its heading and not how its camera was tilted,
/// the horizon grid's toggle is alive and lit, with no spacing after its
/// mark, and the grid is a ring at the middle of the picture with the
/// bearing over it, marked as magnetic where it is: one ring, and none once
/// the grid is off.
#[test]
fn the_horizon_grid_is_a_ring_for_a_heading_alone() {
    let mut on = panels();
    on.horizon_grid = true;
    let mut harness = open(WINDOW, 1, on);
    let mut current = picture(5, 3);
    current.exif.heading = Some(349.4);
    current.exif.magnetic = true;
    harness.state_mut().current = Some(current);
    harness.run();
    let node = harness.get_by_label("Horizon grid").accesskit_node();
    assert!(!node.is_disabled());
    assert!(matches!(
        node.toggled(),
        Some(egui::accesskit::Toggled::True)
    ));
    assert!(harness.query_by_label("10°").is_none());
    let drawn = |harness: &mut Harness<'static, State>| {
        harness.run();
        let shapes = &harness.output().shapes;
        let rings = shapes
            .iter()
            .filter(|shape| matches!(&shape.shape, egui::Shape::Circle(circle) if circle.fill == egui::Color32::TRANSPARENT && circle.stroke.color == egui::Color32::WHITE))
            .count();
        let bearing = shapes.iter().any(
            |shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == "349°M"),
        );
        (rings, bearing)
    };
    assert_eq!(drawn(&mut harness), (1, true));
    harness.state_mut().panels.horizon_grid = false;
    assert_eq!(drawn(&mut harness), (0, false));
}

/// Zoomed in until the picture runs past the content area, the horizon
/// grid keeps to the content area rather than crossing the bars, and
/// leaves the loupe's glass clear for what the glass shows.
#[test]
fn the_horizon_grid_keeps_off_the_bars_and_the_glass() {
    let mut on = panels();
    on.horizon_grid = true;
    let mut harness = build(WINDOW, 1, on);
    let content = egui::Rect::from(chrome::content_area(WINDOW, true, chrome::Parts::NONE));
    let glass = content.center();
    let state = harness.state_mut();
    let current = pitched();
    let viewport = state.input.viewport;
    state
        .view
        .set_zoom_at(400.0, [500.0, 350.0], current.size(), viewport);
    state.current = Some(current);
    state.input.loupe = Some(super::loupe::Loupe {
        eye: [glass.x - 150.0, glass.y + 150.0],
        glass: [glass.x, glass.y],
        magnification: 2.0,
    });
    harness.run();
    harness.run();
    // The grid's strokes are the ones laid on a white halo.
    let mut halos = 0;
    for clipped in &harness.output().shapes {
        let points = match &clipped.shape {
            egui::Shape::LineSegment { points, stroke } if stroke.color == egui::Color32::WHITE => {
                points.to_vec()
            }
            egui::Shape::Path(path)
                if !path.closed
                    && path.stroke.color
                        == egui::epaint::ColorMode::Solid(egui::Color32::WHITE) =>
            {
                path.points.clone()
            }
            _ => continue,
        };
        halos += 1;
        assert!(
            content.expand(0.5).contains_rect(clipped.clip_rect),
            "{:?} reaches past {content:?}",
            clipped.clip_rect
        );
        let midpoints = points.windows(2).map(|pair| pair[0].lerp(pair[1], 0.5));
        for point in points.iter().copied().chain(midpoints) {
            assert!(
                point.distance(glass) > super::loupe::GLASS_RADIUS - 0.5,
                "{point:?} is inside the glass"
            );
        }
    }
    assert!(halos > 0, "the grid was drawn");
}

/// The pointer's readout in the ways the pixels look: the bearing where the
/// file says which way the lens pointed, and how far round from that where
/// it does not, then the elevation — padded in the bar, bare in a copy, the
/// bar marking a bearing from magnetic north.
#[test]
fn the_readout_says_which_way_a_pixel_looks() {
    use super::pixel::{CoordinateFormat, GeographicFormat, place};
    let mut current = pitched();
    let write = |current: &Current, padded| {
        place(
            current,
            [2, 1],
            CoordinateFormat::Direction,
            GeographicFormat::Decimal,
            padded,
        )
    };
    assert_eq!(write(&current, true), "( +0.0\u{00b0}, -20.0\u{00b0})");
    current.exif.heading = Some(212.0);
    assert_eq!(write(&current, true), "(212.0\u{00b0}, -20.0\u{00b0})");
    assert_eq!(write(&current, false), "212.0,-20.0");
    // From magnetic north, marked in the bar and bare in a copy.
    current.exif.magnetic = true;
    assert_eq!(write(&current, true), "(212.0\u{00b0}M, -20.0\u{00b0})");
    assert_eq!(write(&current, false), "212.0,-20.0");
    // Without a focal length there is no way to tell, and the pixel's own
    // place is written instead.
    current.exif.focal_equivalent = None;
    assert_eq!(write(&current, false), "2,1");
}

/// The region's panel comes up with the region asked for, with what needs a
/// drawn region dead until there is one, and goes with it. A box hands over
/// what is typed in it only when it lets go of the keyboard — `Enter` or
/// `Tab` — and `Esc` throws it away; an aspect typed or chosen from the
/// menu is pressed as the aspect.
#[test]
fn the_region_panel_hands_over_what_is_typed_when_it_is_let_go() {
    use crate::image::aspect::Aspect;
    use egui::accesskit::Role;
    let mut harness = open(WINDOW, 1, panels());
    let close = Control::RegionClose.label();
    assert!(
        harness.query_by_label(&close).is_none(),
        "no region, no panel"
    );

    harness.state_mut().input.selection = Selection::Armed;
    harness.run();
    for dead in [Control::RegionCopy, Control::RegionFit, Control::RegionSwap] {
        let label = dead.label();
        let node = harness.get_by_label(&label);
        assert!(node.accesskit_node().is_disabled(), "{dead:?}");
    }
    harness.state_mut().commands.clear();
    harness.get_by_label(&close).click();
    harness.run();
    assert_eq!(asked(&harness), [Command::Press(Control::RegionClose)]);

    let region = Region {
        x: 1,
        y: 1,
        width: 2,
        height: 1,
    };
    harness.state_mut().input.selection = Selection::Shown(region);
    harness.run();
    let label = Control::RegionCopy.label();
    let copy = harness.get_by_label(&label);
    assert!(!copy.accesskit_node().is_disabled());

    // Typed into and let go of: by `Enter`, by `Esc`, by `Tab`.
    let typed = |harness: &mut Harness<'static, State>, holds: &str, text: &str, key| {
        harness
            .get_all_by_role(Role::TextInput)
            .find(|node| node.value().as_deref() == Some(holds))
            .expect("the box")
            .focus();
        harness.run();
        harness.state_mut().commands.clear();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.event(egui::Event::Text(text.to_string()));
        harness.step();
        assert_eq!(asked(harness), [], "nothing while it is typed");
        harness.key_press(key);
        harness.step();
        harness.step();
        asked(harness)
    };
    assert_eq!(
        typed(&mut harness, "2", "30", egui::Key::Enter),
        [Command::RegionField(Field::Width, 30)]
    );
    assert_eq!(typed(&mut harness, "2", "9", egui::Key::Escape), []);
    assert_eq!(
        typed(&mut harness, "Free", "16:10", egui::Key::Tab),
        [Command::Press(Control::Aspect(Aspect::Ratio(16.0, 10.0)))]
    );
    // What will not do is let go of and nothing more.
    assert_eq!(typed(&mut harness, "2", "0", egui::Key::Enter), []);
    assert_eq!(typed(&mut harness, "Free", "wide", egui::Key::Enter), []);

    // The menu offers the ratios the way up the region is.
    harness.get_by_label(&Control::AspectMenu.label()).click();
    harness.run();
    harness.state_mut().commands.clear();
    harness.get_by_label("16:9").click();
    harness.run();
    assert_eq!(
        asked(&harness),
        [Command::Press(Control::Aspect(Aspect::Ratio(16.0, 9.0)))]
    );
}
