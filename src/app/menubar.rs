//! What the Mac's menu bar holds, what each item does, and how each is
//! shown at the moment. AppKit's side of it — building the menus, handing a
//! key on to the window, reading the snapshot as a menu opens — is
//! `crate::menubar`, which knows none of this.
//!
//! Every item is something a key or a button already does, and is done the
//! way they do it: an [`Action`] through `App::perform`, a [`Control`]
//! through `App::press`. So an item is dead exactly when its button would be
//! drawn dead or its key's line of the help popup dimmed — the same
//! [`Conditions`] read the same way — and it shows the key the keymap in
//! force binds to the same job, rebound or not.

use objc2::MainThreadMarker;
use winit::keyboard::{KeyCode, NamedKey};

use super::App;
use super::edits::Edit;
use super::input::{Action, Conditions, Effect, action_of};
use super::keymap::{Chord, KeyName, Keymap};
use crate::PROGRAM;
use crate::image::auxiliary::{Auxiliary, Showing};
use crate::image::decode::Rendering;
use crate::image::display::{Colormap, EV_STEP, ToneMap};
use crate::menubar::{
    Bar, Chosen, Deliver, Equivalent, Identity, Menu, Modifiers, Node, Role, Shown, Snapshot,
    Standard,
};
use crate::render::Upscale;
use crate::ui::filmstrip::{Direction, Sort};
use crate::ui::histogram::WINDOWS;
use crate::ui::menu::{Copies, ZOOM_CHOICES, ZoomChoice};
use crate::ui::pixel::{CoordinateFormat, GeographicFormat, PixelFormat};
use crate::ui::{self, Control, Selection, Tip, loupe};

/// What an item does when it is chosen.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Does {
    Action(Action),
    Control(Control),
    /// The loupe's magnification, set outright where its key steps it.
    Magnification(f32),
}

/// The menu bar as installed, and what each of its tags does.
pub(super) struct MenuBar {
    bar: Bar,
    tags: Vec<Does>,
}

/// What the program is called at the head of the bar and in its items: the
/// name its bundle gives it, which is the binary's with a capital.
fn name() -> String {
    ui::capitalized(PROGRAM)
}

impl MenuBar {
    /// The bar put up, its keys the ones `keys` binds.
    fn install(keys: &Keymap, deliver: Deliver, mtm: MainThreadMarker) -> Self {
        let mut builder = Builder {
            keys,
            tags: Vec::new(),
        };
        let menus = menus(&mut builder);
        let identity = Identity {
            name: name(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        };
        Self {
            bar: Bar::install(&menus, identity, deliver, mtm),
            tags: builder.tags,
        }
    }
}

/// Builds the tree, numbering each item as it goes.
struct Builder<'a> {
    keys: &'a Keymap,
    tags: Vec<Does>,
}

impl Builder<'_> {
    /// The next tag, doing `does`.
    fn tag(&mut self, does: Does) -> usize {
        self.tags.push(does);
        self.tags.len() - 1
    }

    /// An item that runs `action`, showing its key.
    fn action(&mut self, title: &str, action: Action) -> Node {
        self.item(title, Does::Action(action), Some(action), None)
    }

    /// An item that runs `action`, showing `preferred` of its keys where it
    /// is still one of them.
    fn preferring(&mut self, title: &str, action: Action, preferred: Chord) -> Node {
        self.item(title, Does::Action(action), Some(action), Some(preferred))
    }

    /// An item that presses `control`, showing the key of `key` where there
    /// is one: the key that does the same job.
    fn control(&mut self, title: &str, control: Control, key: Option<Action>) -> Node {
        self.item(title, Does::Control(control), key, None)
    }

    /// An item that presses `control`, showing the key that does the same
    /// job as the button, which is the one its tooltip names.
    fn button(&mut self, title: &str, control: Control) -> Node {
        let key = action_of(Tip::Control(control));
        self.control(title, control, key)
    }

    fn item(
        &mut self,
        title: &str,
        does: Does,
        key: Option<Action>,
        preferred: Option<Chord>,
    ) -> Node {
        let key = key.and_then(|action| shortcut(self.keys, action, preferred));
        Node::Item {
            title: title.to_string(),
            key,
            tag: self.tag(does),
        }
    }

    /// A submenu, openable while `control` is alive.
    fn submenu(&mut self, title: &str, control: Option<Control>, nodes: Vec<Node>) -> Node {
        Node::Submenu {
            title: title.to_string(),
            tag: control.map(|control| self.tag(Does::Control(control))),
            nodes,
        }
    }
}

/// Every menu of the bar, in order.
fn menus(b: &mut Builder) -> Vec<Menu> {
    let name = name();
    let copies = vec![
        b.button("Name", Control::Copies(Copies::Name)),
        b.button("Path", Control::Copies(Copies::Path)),
        b.button("File URI", Control::Copies(Copies::Uri)),
        b.button("Info", Control::Copies(Copies::Facts)),
    ];
    vec![
        Menu {
            title: name.clone(),
            role: Role::Application,
            nodes: vec![
                Node::About {
                    title: format!("About {name}"),
                },
                Node::Separator,
                b.action("Settings\u{2026}", Action::OpenSettings),
                Node::Separator,
                Node::Services {
                    title: "Services".to_string(),
                },
                Node::Separator,
                standard(
                    &format!("Hide {name}"),
                    Standard::Hide,
                    Some(command('h', false)),
                ),
                standard(
                    "Hide Others",
                    Standard::HideOthers,
                    Some(command('h', true)),
                ),
                standard("Show All", Standard::ShowAll, None),
                Node::Separator,
                b.preferring(&format!("Quit {name}"), Action::Quit, chord_command('q')),
            ],
        },
        Menu {
            title: "File".to_string(),
            role: Role::Other,
            nodes: vec![
                b.button("Open\u{2026}", Control::OpenFiles),
                b.button("Open Folder\u{2026}", Control::OpenFolder),
                Node::List {
                    title: "Open With".to_string(),
                    tag: b.tag(Does::Control(Control::OpenIn)),
                },
                Node::Separator,
                b.preferring("Close Window", Action::Quit, chord_command('w')),
                Node::Separator,
                b.button("Export\u{2026}", Control::Export),
                b.button("Rename\u{2026}", Control::Rename),
                b.button("Move to Trash", Control::Delete),
                b.button("Remove from List", Control::Remove),
            ],
        },
        Menu {
            title: "Edit".to_string(),
            role: Role::Other,
            nodes: vec![
                b.action("Undo", Action::Undo),
                Node::Separator,
                b.button("Copy Image", Control::Copies(Copies::Image)),
                b.submenu("Copy", Some(Control::Copy), copies),
                b.button("Paste", Control::Paste),
                Node::Separator,
                b.button("Select Region", Control::Region),
            ],
        },
        Menu {
            title: "View".to_string(),
            role: Role::Other,
            nodes: view(b),
        },
        Menu {
            title: "Image".to_string(),
            role: Role::Other,
            nodes: image(b),
        },
        Menu {
            title: "Go".to_string(),
            role: Role::Other,
            nodes: go(b),
        },
        Menu {
            title: "Window".to_string(),
            role: Role::Window,
            nodes: vec![
                standard("Minimize", Standard::Minimize, Some(command('m', false))),
                standard("Zoom", Standard::Zoom, None),
                Node::Separator,
                standard("Bring All to Front", Standard::BringAllToFront, None),
            ],
        },
        Menu {
            title: "Help".to_string(),
            role: Role::Help,
            nodes: vec![b.button("Keyboard Shortcuts", Control::Help)],
        },
    ]
}

fn view(b: &mut Builder) -> Vec<Node> {
    let zooms = ZOOM_CHOICES
        .iter()
        .filter(|choice| !matches!(choice, ZoomChoice::Filter(_)))
        .map(|choice| {
            let key = match choice {
                ZoomChoice::Scale(scale) => Some(Action::ZoomTo(*scale)),
                ZoomChoice::Fit(_) | ZoomChoice::Filter(_) => None,
            };
            b.control(&choice.label(), Control::ZoomTo(*choice), key)
        })
        .collect();
    let filters = Upscale::ALL
        .iter()
        .map(|filter| {
            b.control(
                filter.label(),
                Control::ZoomTo(ZoomChoice::Filter(*filter)),
                None,
            )
        })
        .collect();
    let magnifications = loupe::MAGNIFICATIONS
        .iter()
        .map(|times| {
            b.item(
                &format!("{times}\u{d7}"),
                Does::Magnification(*times),
                None,
                None,
            )
        })
        .collect();
    let formats = PixelFormat::ALL
        .iter()
        .map(|format| b.control(format.label(), Control::Format(*format), None))
        .collect();
    let coordinates = CoordinateFormat::ALL
        .iter()
        .map(|format| b.control(format.label(), Control::Coordinates(*format), None))
        .collect();
    let geographic = GeographicFormat::ALL
        .iter()
        .map(|format| b.control(format.label(), Control::Geographic(*format), None))
        .collect();
    vec![
        b.preferring(
            "Actual Size",
            Action::ZoomTo(1.0),
            Chord::new(CMD, KeyName::Position(KeyCode::Digit0)),
        ),
        b.action("Zoom In", Action::ZoomIn),
        b.action("Zoom Out", Action::ZoomOut),
        b.action("Cycle Fit", Action::CycleFit),
        b.submenu("Zoom", Some(Control::ZoomTo(ZoomChoice::Scale(1.0))), zooms),
        b.submenu("Upscaling", None, filters),
        Node::Separator,
        b.control("Rotate Left", Control::TurnLeft, Some(Action::TurnLeft)),
        b.control("Rotate Right", Control::TurnRight, Some(Action::TurnRight)),
        Node::Separator,
        b.action("Show Interface", Action::ToggleInterface),
        b.action(
            "Hide Interface and Panels",
            Action::ToggleInterfaceAndPanels,
        ),
        Node::Separator,
        b.button("File List", Control::Filmstrip),
        b.button("Minimap", Control::Minimap),
        b.button("Histogram", Control::Histogram),
        b.button("Info", Control::Info),
        b.button("Grid", Control::Grid),
        b.button("Loupe", Control::Loupe),
        b.submenu("Loupe Magnification", None, magnifications),
        Node::Separator,
        b.submenu("Pixel Readout", None, formats),
        b.submenu(
            "Coordinates",
            Some(Control::Coordinates(CoordinateFormat::ALL[0])),
            coordinates,
        ),
        b.submenu(
            "Lat/Long",
            Some(Control::Geographic(GeographicFormat::ALL[0])),
            geographic,
        ),
        Node::Separator,
        // AppKit adds an item of its own to a menu called View unless one
        // already sends `toggleFullScreen:`; its icon would indent the group
        // it lands in, which has no separator of its own.
        // AppKit gives it the system's full-screen key, 🌐F, itself.
        standard("Enter Full Screen", Standard::FullScreen, None),
    ]
}

fn image(b: &mut Builder) -> Vec<Node> {
    let windows = (0..WINDOWS.len())
        .map(|index| b.control(WINDOWS[index].0, Control::Window(index), None))
        .collect();
    let curves = ToneMap::ALL
        .iter()
        .enumerate()
        .map(|(index, curve)| {
            let title = match curve {
                ToneMap::None => "Clip",
                ToneMap::Neutral => "Roll off",
            };
            b.control(title, Control::Curve(index), None)
        })
        .collect();
    let ramps = Colormap::ALL
        .iter()
        .enumerate()
        .map(|(index, map)| b.control(&ui::capitalized(map.label()), Control::Ramp(index), None))
        .collect();
    let plot = vec![
        b.button("Luminance", Control::Luma),
        b.button("Color Planes", Control::Planes),
        b.button("Log Counts", Control::Log),
    ];
    vec![
        b.action("Increase Exposure", Action::Exposure(EV_STEP)),
        b.action("Decrease Exposure", Action::Exposure(-EV_STEP)),
        b.action(
            "Raise Black Point",
            Action::StepBlack(super::input::WINDOW_STEP),
        ),
        b.action(
            "Lower Black Point",
            Action::StepBlack(-super::input::WINDOW_STEP),
        ),
        b.action(
            "Raise White Point",
            Action::StepWhite(super::input::WINDOW_STEP),
        ),
        b.action(
            "Lower White Point",
            Action::StepWhite(-super::input::WINDOW_STEP),
        ),
        Node::Separator,
        b.submenu("Window", Some(Control::Window(0)), windows),
        b.submenu("Highlights", Some(Control::Curve(0)), curves),
        b.submenu("False Color", Some(Control::Ramp(0)), ramps),
        b.button("Mark Clipped Pixels", Control::Marks),
        b.button("Reset Display", Control::Reset),
        Node::Separator,
        b.button("HDR Output", Control::Output),
        b.button("Camera JPEG", Control::CameraJpeg),
        b.button("Depth Map", Control::Depth),
        Node::Separator,
        b.submenu("Histogram Plot", None, plot),
    ]
}

fn go(b: &mut Builder) -> Vec<Node> {
    let mut sorts: Vec<Node> = Sort::ALL
        .iter()
        .map(|sort| b.control(sort.label(), Control::SortBy(*sort), None))
        .collect();
    sorts.push(Node::Separator);
    sorts.extend(
        Direction::ALL.iter().map(|direction| {
            b.control(direction.label(), Control::SortDirection(*direction), None)
        }),
    );
    vec![
        b.button("Next File", Control::Next),
        b.button("Previous File", Control::Previous),
        b.button("Back", Control::Back),
        b.button("Forward", Control::Forward),
        b.control(
            "Go to File\u{2026}",
            Control::Chooser,
            Some(Action::OpenChooser),
        ),
        Node::Separator,
        b.submenu("Sort By", None, sorts),
        Node::Separator,
        b.button("Play", Control::Play),
        b.button("Next Frame", Control::StepForward),
        b.button("Previous Frame", Control::StepBack),
    ]
}

fn standard(title: &str, standard: Standard, key: Option<Equivalent>) -> Node {
    Node::Standard {
        title: title.to_string(),
        key,
        standard,
    }
}

/// `⌘` and a letter, with `⌥` as well where `option` says.
fn command(letter: char, option: bool) -> Equivalent {
    Equivalent {
        key: letter.to_string(),
        modifiers: Modifiers {
            command: true,
            option,
            ..Modifiers::default()
        },
    }
}

const CMD: super::input::Mods = super::input::Mods::SUPER;

/// `⌘` and a letter, as the keymap writes it.
fn chord_command(letter: char) -> Chord {
    Chord::new(CMD, KeyName::Char(letter))
}

/// The key an item for `action` shows: `preferred` where the name that runs
/// it answers to it, and otherwise the first of its chords held with `⌘` —
/// the one a Mac's menus would show — or failing that its first. `None`
/// where nothing runs it, or what does cannot be written as a key
/// equivalent.
fn shortcut(keys: &Keymap, action: Action, preferred: Option<Chord>) -> Option<Equivalent> {
    let (_, name) = keys.bound_for(action)?;
    let chords = keys.chords_of(name);
    let chord = preferred
        .filter(|preferred| chords.contains(preferred))
        .or_else(|| chords.iter().copied().find(|chord| chord.mods.super_key()))
        .or_else(|| chords.first().copied())?;
    equivalent(chord)
}

/// `chord` as AppKit writes a key equivalent: the character, a capital
/// carrying its Shift; the number row's digit; or the private-use character
/// AppKit gives a named key.
fn equivalent(chord: Chord) -> Option<Equivalent> {
    let key = match chord.key {
        KeyName::Char(character) => character.to_string(),
        KeyName::Position(code) => digit(code)?.to_string(),
        KeyName::Named(named) => named_key(named)?.to_string(),
    };
    let mods = chord.mods;
    Some(Equivalent {
        key,
        modifiers: Modifiers {
            command: mods.super_key(),
            option: mods.alt_key(),
            control: mods.control_key(),
            shift: mods.shift_key(),
        },
    })
}

/// Which digit of the number row `code` is.
fn digit(code: KeyCode) -> Option<char> {
    Some(match code {
        KeyCode::Digit0 => '0',
        KeyCode::Digit1 => '1',
        KeyCode::Digit2 => '2',
        KeyCode::Digit3 => '3',
        KeyCode::Digit4 => '4',
        KeyCode::Digit5 => '5',
        KeyCode::Digit6 => '6',
        KeyCode::Digit7 => '7',
        KeyCode::Digit8 => '8',
        KeyCode::Digit9 => '9',
        _ => return None,
    })
}

/// The character AppKit writes `named` as in a key equivalent: a control
/// character for the few that type one, and for the rest the private-use
/// character `NSEvent.h` gives it.
fn named_key(named: NamedKey) -> Option<char> {
    let function = |offset: u32| char::from_u32(0xF700 + offset);
    match named {
        NamedKey::Space => Some(' '),
        NamedKey::Enter => Some('\r'),
        NamedKey::Tab => Some('\t'),
        NamedKey::Escape => Some('\u{1b}'),
        NamedKey::Backspace => Some('\u{8}'),
        NamedKey::ArrowUp => function(0x00),
        NamedKey::ArrowDown => function(0x01),
        NamedKey::ArrowLeft => function(0x02),
        NamedKey::ArrowRight => function(0x03),
        NamedKey::F1 => function(0x04),
        NamedKey::F2 => function(0x05),
        NamedKey::F3 => function(0x06),
        NamedKey::F4 => function(0x07),
        NamedKey::F5 => function(0x08),
        NamedKey::F6 => function(0x09),
        NamedKey::F7 => function(0x0A),
        NamedKey::F8 => function(0x0B),
        NamedKey::F9 => function(0x0C),
        NamedKey::F10 => function(0x0D),
        NamedKey::F11 => function(0x0E),
        NamedKey::F12 => function(0x0F),
        NamedKey::Insert => function(0x27),
        NamedKey::Delete => function(0x28),
        NamedKey::Home => function(0x29),
        NamedKey::End => function(0x2B),
        NamedKey::PageUp => function(0x2C),
        NamedKey::PageDown => function(0x2D),
        _ => None,
    }
}

impl App {
    /// Takes the way a choice in the menu bar reaches the loop, for the bar
    /// installed once the application has finished launching: see
    /// [`App::install_menubar`].
    pub fn deliver_menu(&mut self, deliver: Deliver) {
        self.menu_deliver = Some(deliver);
    }

    /// Puts the menu bar up, the first time it is asked: called as winit
    /// says the application is running, which is after AppKit has finished
    /// launching and would otherwise put up a bar of its own.
    pub(super) fn install_menubar(&mut self) {
        let Some(deliver) = self.menu_deliver.take() else {
            return;
        };
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        self.menubar = Some(MenuBar::install(&self.keys, deliver, mtm));
        self.publish_menu();
    }

    /// Gives the menu bar's items the keys the keymap now binds, after the
    /// configuration has been read again: the same tree, built again, so
    /// that each tag is the same item and only its key has moved.
    pub(super) fn rekey_menubar(&self) {
        let Some(menubar) = &self.menubar else {
            return;
        };
        let mut builder = Builder {
            keys: &self.keys,
            tags: Vec::new(),
        };
        let menus = menus(&mut builder);
        debug_assert_eq!(
            builder.tags, menubar.tags,
            "the same items, in the same order"
        );
        menubar.bar.rekey(&menus);
    }

    /// Hands the menu bar how each item is to be shown now. Called as each
    /// handler settles, when whatever it changed has been changed: a menu
    /// opens between handlers, never during one.
    pub(super) fn publish_menu(&self) {
        let Some(menubar) = &self.menubar else {
            return;
        };
        let conditions = self.conditions();
        let shown = menubar
            .tags
            .iter()
            .map(|does| Shown {
                enabled: self.alive(*does, &conditions),
                checked: self.checked(*does),
                title: self.retitled(*does, &conditions),
            })
            .collect();
        let list = self
            .openers
            .iter()
            .map(|opener| opener.name.clone())
            .collect();
        menubar.bar.publish(Snapshot { shown, list });
    }

    /// Does what was chosen in the menu bar with the pointer. A key that
    /// reached an item never arrives here: it went on to the window.
    pub(super) fn chose(&mut self, chosen: Chosen) -> Effect {
        let does = match chosen {
            Chosen::Listed(index) => Does::Control(Control::Opener(index)),
            Chosen::Item(tag) => match self.menubar.as_ref().and_then(|bar| bar.tags.get(tag)) {
                Some(does) => *does,
                None => return Effect::Nothing,
            },
        };
        match does {
            Does::Action(action) => self.perform(action),
            Does::Control(control) => self.press(control).also(Effect::Redraw),
            Does::Magnification(times) => {
                self.panels.loupe_magnification = times;
                Effect::Redraw
            }
        }
    }

    /// Whether `does` would do anything now: its button alive, and the
    /// condition its key waits on met.
    fn alive(&self, does: Does, conditions: &Conditions) -> bool {
        let waits = |action: Option<Action>| {
            action
                .and_then(|action| self.keys.row_for(action))
                .and_then(|row| row.when)
                .is_none_or(|when| conditions.met(when))
        };
        match does {
            Does::Control(control) => {
                let tip = Tip::Control(control);
                ui::tooltip::disabled(tip, conditions.reasons()).is_none()
                    && waits(action_of(tip))
                    && !(conditions.nothing_open && about_picture(does))
                    && match control {
                        Control::Coordinates(format) => format.offered(
                            self.current
                                .as_ref()
                                .and_then(|current| current.exif.georeference.as_ref()),
                        ),
                        _ => true,
                    }
            }
            Does::Action(action) => {
                waits(Some(action)) && !(conditions.nothing_open && about_picture(does))
            }
            Does::Magnification(_) => true,
        }
    }

    /// Whether `does` is checked: a toggle that is on, or the one of a set
    /// of choices in force.
    fn checked(&self, does: Does) -> bool {
        let panels = &self.panels;
        let current = self.current.as_ref();
        match does {
            Does::Action(Action::ToggleInterface) => panels.show_ui,
            Does::Action(_) => false,
            Does::Magnification(times) => panels.loupe_magnification == times,
            Does::Control(control) => match control {
                Control::Filmstrip => panels.show_filmstrip,
                Control::Minimap => panels.show_minimap,
                Control::Histogram => panels.show_histogram,
                Control::Info => panels.show_info,
                Control::Grid => panels.show_grid,
                Control::Loupe => panels.show_loupe,
                Control::Depth => current
                    .is_some_and(|current| current.showing == Showing::Auxiliary(Auxiliary::Depth)),
                Control::Luma => panels.show_luma,
                Control::Planes => panels.show_planes,
                Control::Log => panels.log_counts,
                Control::Marks => panels.mark_clipped,
                Control::Region => self.marking.selection.is_on(),
                Control::Output => self.headroom() == crate::image::display::Headroom::Above,
                Control::CameraJpeg => {
                    current.is_some_and(|current| current.rendering == Rendering::CameraJpeg)
                }
                Control::Format(format) => panels.pixel_format == format,
                Control::Coordinates(format) => panels.coordinate_format == format,
                Control::Geographic(format) => panels.geographic_format == format,
                // A filter is in force with no picture too; a zoom is of one.
                Control::ZoomTo(choice) => {
                    let zoom = self.view.zoom(self.image_size(), self.viewport());
                    (current.is_some() || matches!(choice, ZoomChoice::Filter(_)))
                        && choice.active(self.view.fit(), zoom, self.view.upscale())
                }
                Control::Window(index) => current.is_some_and(|current| {
                    WINDOWS
                        .get(index)
                        .is_some_and(|(_, window)| current.display.auto() == *window)
                }),
                Control::Curve(index) => current.is_some_and(|current| {
                    ToneMap::ALL.get(index) == Some(&current.display.tone_map())
                }),
                Control::Ramp(index) => current.is_some_and(|current| {
                    current.image.is_gray()
                        && Colormap::ALL.get(index) == Some(&current.display.colormap())
                }),
                Control::SortBy(sort) => self.filmstrip.order().sort == sort,
                Control::SortDirection(direction) => self.filmstrip.order().direction == direction,
                _ => false,
            },
        }
    }

    /// What `does` is called now, where that is not its own title: what
    /// undo would put back, the copy of a region, the pause of an animation
    /// playing, and the pages of a file that holds several.
    fn retitled(&self, does: Does, conditions: &Conditions) -> Option<String> {
        let pages = conditions.pages && !conditions.animation;
        let title = match does {
            Does::Action(Action::Undo) => match self.edits.last()? {
                Edit::Trashed { .. } => "Undo Move to Trash",
                Edit::Renamed { .. } => "Undo Rename",
                Edit::Removed { .. } => "Undo Remove from List",
            },
            Does::Control(Control::Copies(Copies::Image))
                if matches!(self.marking.selection, Selection::Shown(_)) =>
            {
                "Copy Region"
            }
            Does::Control(Control::Play)
                if self
                    .animation
                    .as_ref()
                    .is_some_and(|animation| animation.playing()) =>
            {
                "Pause"
            }
            Does::Control(Control::StepForward) if pages => "Next Page",
            Does::Control(Control::StepBack) if pages => "Previous Page",
            _ => return None,
        };
        Some(title.to_string())
    }
}

/// Whether `does` is about the picture, and so does nothing with none open.
fn about_picture(does: Does) -> bool {
    matches!(
        does,
        Does::Action(
            Action::ZoomIn
                | Action::ZoomOut
                | Action::ZoomTo(_)
                | Action::CycleFit
                | Action::Exposure(_)
                | Action::StepBlack(_)
                | Action::StepWhite(_)
        ) | Does::Control(
            Control::ZoomTo(ZoomChoice::Scale(_) | ZoomChoice::Fit(_))
                | Control::Window(_)
                | Control::Curve(_)
                | Control::Ramp(_)
                | Control::Reset
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every chord of the Mac's keymap that an item could show can be
    /// written as a key equivalent: nothing bound by default falls back to
    /// showing no key for want of a character.
    #[test]
    fn every_default_chord_is_a_key_equivalent() {
        let keys = Keymap::mac();
        for row in keys.rows() {
            if let super::super::keymap::Keys::Bound(binds) = row.keys {
                for bound in binds {
                    for chord in keys.chords_of(bound.name) {
                        assert!(equivalent(*chord).is_some(), "{}: {chord:?}", bound.name);
                    }
                }
            }
        }
    }

    /// An item shows the chord a Mac's menus would: the one held with `⌘`,
    /// ahead of a plain key that does the same, unless another is asked
    /// for by name; and a capital carries its Shift in the character.
    #[test]
    fn an_item_shows_its_command_chord() {
        let keys = Keymap::mac();
        let shown = |action, preferred| shortcut(&keys, action, preferred).unwrap();
        let quit = shown(Action::Quit, None);
        assert_eq!(quit.key, "q");
        assert!(quit.modifiers.command);
        assert_eq!(shown(Action::Quit, Some(chord_command('w'))).key, "w");
        let folder = shown(Action::OpenFolder, None);
        assert_eq!(folder.key, "O");
        assert!(folder.modifiers.command && !folder.modifiers.shift);
        // A key with no `⌘` chord shows the one it has.
        let minimap = shown(Action::ToggleMinimap, None);
        assert_eq!(minimap.key, "m");
        assert_eq!(minimap.modifiers, Modifiers::default());
        let actual = shown(
            Action::ZoomTo(1.0),
            Some(Chord::new(CMD, KeyName::Position(KeyCode::Digit0))),
        );
        assert_eq!(actual.key, "0");
        assert!(actual.modifiers.command);
    }

    /// An item for a name nothing is bound to shows no key, rather than
    /// the key of something else.
    #[test]
    fn an_unbound_name_shows_no_key() {
        let mut keys = Keymap::mac();
        keys.bind("interface.quit", Vec::new()).unwrap();
        assert_eq!(
            shortcut(&keys, Action::Quit, Some(chord_command('q'))),
            None
        );
    }

    /// Every item's tag is its place in the list of what each does, and no
    /// two items share one.
    #[test]
    fn every_item_has_its_own_tag() {
        let keys = Keymap::mac();
        let mut builder = Builder {
            keys: &keys,
            tags: Vec::new(),
        };
        let menus = menus(&mut builder);
        let mut seen = Vec::new();
        fn walk(nodes: &[Node], seen: &mut Vec<usize>) {
            for node in nodes {
                match node {
                    Node::Item { tag, .. } | Node::List { tag, .. } => seen.push(*tag),
                    Node::Submenu { tag, nodes, .. } => {
                        seen.extend(tag);
                        walk(nodes, seen);
                    }
                    _ => {}
                }
            }
        }
        for menu in &menus {
            walk(&menu.nodes, &mut seen);
        }
        seen.sort_unstable();
        assert_eq!(seen, (0..builder.tags.len()).collect::<Vec<_>>());
    }
}
