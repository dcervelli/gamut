//! What the keyboard and the pointer do.
//!
//! Keys go through one table, [`KEYS`], which is also what `--help` prints:
//! a binding added here is documented by the same edit. Each key names an
//! [`Action`], and [`App::perform`] is the one place an action happens.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use winit::event::{ElementState, MouseButton, MouseScrollDelta};
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};
use winit::window::{Cursor, CursorIcon};

use super::App;
use crate::clipboard;
use crate::image::display::{Colormap, Startup};
use crate::image::encode;
use crate::loader::Source;
use crate::pasted;
use crate::render::Rect;
use crate::timing;
use crate::ui::info::Copyable;
use crate::ui::layers::Hit;
use crate::ui::{self, Current, Menu, Widget};

/// Window pixels moved per arrow-key press. Shift moves one pixel instead,
/// for placing a view exactly, and Ctrl goes as far as the image does.
const PAN_STEP: f32 = 64.0;

/// How far one notch of the wheel scrolls a panel that has more to show than
/// fits, in logical pixels: about three lines of it.
const WHEEL_SCROLL_STEP: f32 = 48.0;

/// Trackpad pixels that add up to one notch of the wheel. Wheels report whole
/// lines and need no conversion; a trackpad reports the scroll it would have
/// done, and this is what turns that into the same zoom increment.
const WHEEL_PIXELS_PER_STEP: f32 = 50.0;

/// Something a key asks for.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    Quit,
    ZoomIn,
    ZoomOut,
    /// Go to this zoom, 1.0 being one image pixel to one screen pixel.
    ZoomTo(f32),
    Pan(Direction, PanStep),
    CycleFit,
    CycleUpscale,
    NextFile,
    PreviousFile,
    ToggleInterface,
    /// The interface, and the panels floating over the image with it: the
    /// bars come and go as [`Action::ToggleInterface`], and the map,
    /// histogram and information panel are closed on the way past.
    ToggleInterfaceAndPanels,
    ToggleHistogram,
    /// Whether that panel's plot counts up its axis or the logarithm of its
    /// counts. About the plot rather than about the image, which is why it
    /// is not one of the things [`Action::ResetDisplay`] puts back.
    ToggleLogCounts,
    ToggleInfo,
    ToggleMinimap,
    ToggleGrid,
    /// Exposure, by this many stops.
    Exposure(f32),
    CycleAutoWindow,
    /// Slide the window by this fraction of its width.
    ShiftWindow(f32),
    /// Scale the window's width by this factor.
    Contrast(f32),
    CycleToneMap,
    CycleColormap,
    ResetDisplay,
    /// Put the absolute path of the file on screen on the clipboard.
    CopyPath,
    /// Put the file on screen on the clipboard as a `file:` URI, under the
    /// MIME type a program that wants the file itself asks for.
    CopyUri,
    /// Put the picture on screen on the clipboard as a PNG.
    CopyImage,
    /// Put everything the info panel says about the file on the clipboard,
    /// as the rows a click on its topmost button would copy.
    CopyMetadata,
    /// Write the picture on the clipboard to a file of its own, put it in the
    /// list beside the one on screen, and show it.
    Paste,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl Direction {
    /// Which way this is, as a sign on each axis.
    fn sign(self) -> [f32; 2] {
        match self {
            Direction::Left => [-1.0, 0.0],
            Direction::Right => [1.0, 0.0],
            Direction::Up => [0.0, -1.0],
            Direction::Down => [0.0, 1.0],
        }
    }
}

/// How far one press of a pan key moves the view.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PanStep {
    /// One window pixel, for lining a view up exactly.
    Fine,
    /// [`PAN_STEP`] window pixels.
    Coarse,
    /// As far as the image goes that way.
    Edge,
}

impl PanStep {
    /// Window pixels one press moves, and `None` for the one that moves as
    /// far as there is to move.
    fn pixels(self) -> Option<f32> {
        match self {
            PanStep::Fine => Some(1.0),
            PanStep::Coarse => Some(PAN_STEP),
            PanStep::Edge => None,
        }
    }
}

/// A key as `winit` reports it: the character it produced, the name of one
/// that produces none, or — where the character would depend on the layout —
/// the place on the keyboard it was pressed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyName {
    Char(&'static str),
    Named(NamedKey),
    /// Matched by position rather than by what it types. For the number row,
    /// whose shifted characters are whatever the layout puts there: `@` is
    /// Shift+`2` on one keyboard and `"` on another, and the zoom that hangs
    /// off `2` should be under `2` on both.
    Position(KeyCode),
}

/// What a binding is held with, over and above whatever Shift a character
/// key already implies.
///
/// Ctrl, Alt and Super must be held exactly as written: a chord this table
/// does not bind belongs to the window manager, and acting on `Super+0` as
/// well would move the view behind its back.
///
/// Shift is a modifier for a named key and not for a character one — see
/// [`satisfies`]. What the user presses is spelled out in [`Binding::shown`]
/// either way.
pub type Mods = ModifiersState;

/// Held with nothing, or with nothing but the Shift a character carries.
const PLAIN: Mods = Mods::empty();
const CTRL: Mods = Mods::CONTROL;
const SHIFT: Mods = Mods::SHIFT;

/// Whether the modifiers `held` are the ones a binding asked for, for a key
/// of this kind. Shift is the difference between the two kinds.
///
/// A character has already had Shift applied — the table says `a` and `A`,
/// not `a` and Shift+`a` — so asking for it again would be asking twice, and
/// asking for it where the character is a capital would refuse the same
/// capital typed under Caps Lock. It is therefore ignored there.
///
/// A named key, or one bound by position, is the same key whether or not
/// Shift is held, so there Shift is a modifier like any other: it is what
/// tells `Shift+Left` from `Left`, and `Shift+2` from `2`.
fn satisfies(required: Mods, held: Mods, key: KeyName) -> bool {
    match key {
        Char(_) => held.difference(Mods::SHIFT) == required,
        Named(_) | Position(_) => held == required,
    }
}

/// Which heading a binding is listed under in `--help`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Zoom,
    Files,
    Clipboard,
    Interface,
    Display,
}

/// One line of `--help`, and the keys that do it. A line may bind several
/// keys to several actions (`d, f`), and a line may bind none (`Wheel`).
pub struct Binding {
    pub section: Section,
    /// What is held down with the keys below.
    pub mods: Mods,
    /// The key column, as written for people: `q, Esc`, `Arrows`.
    pub shown: &'static str,
    pub help: &'static str,
    pub keys: &'static [(KeyName, Action)],
}

use Action::*;
use Direction::{Down, Left, Right, Up};
use KeyName::{Char, Named, Position};
use PanStep::{Coarse, Edge, Fine};

/// Every key, in the order `--help` lists them.
///
/// Letter keys are bound in both cases wherever the capital is not itself a
/// binding, so that Caps Lock does not turn the keyboard off. The four that
/// mean two different things — `a`/`A`, `s`/`S`, `c`/`C` — are the exception,
/// and are bound one case at a time.
pub const KEYS: &[Binding] = &[
    // The number row is bound by position, not by what it types: the zooms
    // below 100% are the ones above it with Shift held, and which character
    // that is depends on the layout.
    Binding {
        section: Section::Zoom,
        mods: PLAIN,
        shown: "1, 0",
        help: "Actual size (100%)",
        keys: &[
            (Position(KeyCode::Digit1), ZoomTo(1.0)),
            (Position(KeyCode::Digit0), ZoomTo(1.0)),
        ],
    },
    Binding {
        section: Section::Zoom,
        mods: PLAIN,
        shown: "2, 3, 4, 5",
        help: "200%, 400%, 800%, 1600%",
        keys: &[
            (Position(KeyCode::Digit2), ZoomTo(2.0)),
            (Position(KeyCode::Digit3), ZoomTo(4.0)),
            (Position(KeyCode::Digit4), ZoomTo(8.0)),
            (Position(KeyCode::Digit5), ZoomTo(16.0)),
        ],
    },
    Binding {
        section: Section::Zoom,
        mods: SHIFT,
        shown: "Shift+2, 3, 4",
        help: "50%, 25%, 10%",
        keys: &[
            (Position(KeyCode::Digit2), ZoomTo(0.5)),
            (Position(KeyCode::Digit3), ZoomTo(0.25)),
            (Position(KeyCode::Digit4), ZoomTo(0.1)),
        ],
    },
    Binding {
        section: Section::Zoom,
        mods: PLAIN,
        shown: "+, =",
        help: "Zoom in",
        keys: &[(Char("+"), ZoomIn), (Char("="), ZoomIn)],
    },
    Binding {
        section: Section::Zoom,
        mods: PLAIN,
        shown: "-, _",
        help: "Zoom out",
        keys: &[(Char("-"), ZoomOut), (Char("_"), ZoomOut)],
    },
    Binding {
        section: Section::Zoom,
        mods: PLAIN,
        shown: "Wheel",
        help: "Zoom about the pointer",
        keys: &[],
    },
    Binding {
        section: Section::Zoom,
        mods: PLAIN,
        shown: "Space",
        help: "Cycle fit / fit width / fit height",
        keys: &[(Named(NamedKey::Space), CycleFit)],
    },
    Binding {
        section: Section::Zoom,
        mods: PLAIN,
        shown: "p",
        help: "Cycle the filter used above 100%: nearest, bicubic",
        keys: &[(Char("p"), CycleUpscale), (Char("P"), CycleUpscale)],
    },
    Binding {
        section: Section::Zoom,
        mods: PLAIN,
        shown: "Arrows",
        help: "Pan by 64 pixels",
        keys: &[
            (Named(NamedKey::ArrowLeft), Pan(Left, Coarse)),
            (Named(NamedKey::ArrowRight), Pan(Right, Coarse)),
            (Named(NamedKey::ArrowUp), Pan(Up, Coarse)),
            (Named(NamedKey::ArrowDown), Pan(Down, Coarse)),
        ],
    },
    // Shift belongs to the modifiers here, where it does not for a character:
    // an arrow is the same key whichever way it is held, so this is the one
    // place the table has to ask for it.
    Binding {
        section: Section::Zoom,
        mods: SHIFT,
        shown: "Shift+Arrows",
        help: "Pan by one pixel",
        keys: &[
            (Named(NamedKey::ArrowLeft), Pan(Left, Fine)),
            (Named(NamedKey::ArrowRight), Pan(Right, Fine)),
            (Named(NamedKey::ArrowUp), Pan(Up, Fine)),
            (Named(NamedKey::ArrowDown), Pan(Down, Fine)),
        ],
    },
    Binding {
        section: Section::Zoom,
        mods: CTRL,
        shown: "Ctrl+Arrows",
        help: "Pan to the far side of the image",
        keys: &[
            (Named(NamedKey::ArrowLeft), Pan(Left, Edge)),
            (Named(NamedKey::ArrowRight), Pan(Right, Edge)),
            (Named(NamedKey::ArrowUp), Pan(Up, Edge)),
            (Named(NamedKey::ArrowDown), Pan(Down, Edge)),
        ],
    },
    Binding {
        section: Section::Files,
        mods: PLAIN,
        shown: "], Page Down",
        help: "Next file",
        keys: &[(Char("]"), NextFile), (Named(NamedKey::PageDown), NextFile)],
    },
    Binding {
        section: Section::Files,
        mods: PLAIN,
        shown: "[, Page Up",
        help: "Previous file",
        keys: &[
            (Char("["), PreviousFile),
            (Named(NamedKey::PageUp), PreviousFile),
        ],
    },
    // The first two are both the capital, so both are typed with Shift held;
    // only the Ctrl that parts one from the other is a modifier as far as the
    // table is concerned. `shown` says what the fingers do.
    Binding {
        section: Section::Clipboard,
        mods: PLAIN,
        shown: "Shift+C",
        help: "Copy the absolute path of the file on screen",
        keys: &[(Char("C"), CopyPath)],
    },
    Binding {
        section: Section::Clipboard,
        mods: CTRL,
        shown: "Ctrl+Shift+C",
        help: "Copy the file on screen as a URI another program can open",
        keys: &[(Char("C"), CopyUri)],
    },
    Binding {
        section: Section::Clipboard,
        mods: CTRL,
        shown: "Ctrl+C",
        help: "Copy the picture itself, as the display settings show it",
        keys: &[(Char("c"), CopyImage)],
    },
    Binding {
        section: Section::Clipboard,
        mods: CTRL,
        shown: "Ctrl+I",
        help: "Copy everything the info panel says about the file",
        keys: &[(Char("i"), CopyMetadata), (Char("I"), CopyMetadata)],
    },
    Binding {
        section: Section::Clipboard,
        mods: CTRL,
        shown: "Ctrl+V",
        help: "Paste a picture, saved among your pictures and shown",
        keys: &[(Char("v"), Paste), (Char("V"), Paste)],
    },
    Binding {
        section: Section::Interface,
        mods: PLAIN,
        shown: "`",
        help: "Toggle the interface panels",
        keys: &[(Char("`"), ToggleInterface)],
    },
    Binding {
        section: Section::Interface,
        mods: PLAIN,
        shown: "~",
        help: "Toggle the panels, closing the map, histogram and information",
        keys: &[(Char("~"), ToggleInterfaceAndPanels)],
    },
    Binding {
        section: Section::Interface,
        mods: PLAIN,
        shown: "m",
        help: "Toggle the minimap",
        keys: &[(Char("m"), ToggleMinimap), (Char("M"), ToggleMinimap)],
    },
    Binding {
        section: Section::Interface,
        mods: PLAIN,
        shown: "h",
        help: "Toggle the histogram",
        keys: &[(Char("h"), ToggleHistogram), (Char("H"), ToggleHistogram)],
    },
    Binding {
        section: Section::Interface,
        mods: PLAIN,
        shown: "i",
        help: "Toggle the file information panel",
        keys: &[(Char("i"), ToggleInfo), (Char("I"), ToggleInfo)],
    },
    Binding {
        section: Section::Interface,
        mods: PLAIN,
        shown: "g",
        help: "Toggle the grid over the image",
        keys: &[(Char("g"), ToggleGrid), (Char("G"), ToggleGrid)],
    },
    Binding {
        section: Section::Interface,
        mods: PLAIN,
        shown: "l",
        help: "Toggle a logarithmic count axis on the histogram",
        keys: &[(Char("l"), ToggleLogCounts), (Char("L"), ToggleLogCounts)],
    },
    Binding {
        section: Section::Interface,
        mods: PLAIN,
        shown: "q, Esc",
        help: "Quit",
        keys: &[
            (Char("q"), Quit),
            (Char("Q"), Quit),
            (Named(NamedKey::Escape), Quit),
        ],
    },
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "d, f",
        help: "Exposure down / up, half a stop",
        keys: &[
            (Char("d"), Exposure(-0.5)),
            (Char("D"), Exposure(-0.5)),
            (Char("f"), Exposure(0.5)),
            (Char("F"), Exposure(0.5)),
        ],
    },
    // One case each: the capitals are the width of the window, below.
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "a, s",
        help: "Slide the window down / up",
        keys: &[
            (Char("a"), ShiftWindow(-0.05)),
            (Char("s"), ShiftWindow(0.05)),
        ],
    },
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "A, S",
        help: "Narrow / widen the window",
        keys: &[(Char("A"), Contrast(0.8)), (Char("S"), Contrast(1.25))],
    },
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "e",
        help: "Cycle the automatic window: unit, min/max, 99.8%",
        keys: &[(Char("e"), CycleAutoWindow), (Char("E"), CycleAutoWindow)],
    },
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "t",
        help: "Cycle tone mapping: clip, reinhard, neutral",
        keys: &[(Char("t"), CycleToneMap), (Char("T"), CycleToneMap)],
    },
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "r",
        help: "Cycle false colour for single-channel images",
        keys: &[(Char("r"), CycleColormap), (Char("R"), CycleColormap)],
    },
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "z",
        help: "Reset the window, exposure and tone map",
        keys: &[(Char("z"), ResetDisplay), (Char("Z"), ResetDisplay)],
    },
];

/// What `key`, pressed at `position` and held with `mods`, asks for. The
/// place on the keyboard comes along with the character because a handful of
/// bindings are made against it — see [`KeyName::Position`].
pub fn action_for(key: &Key, position: PhysicalKey, mods: Mods) -> Option<Action> {
    KEYS.iter()
        .flat_map(|binding| binding.keys.iter().map(|entry| (binding.mods, entry)))
        .find(|(required, (name, _))| {
            satisfies(*required, mods, *name)
                && match (name, key) {
                    (Char(text), Key::Character(typed)) => typed.as_str() == *text,
                    (Named(name), Key::Named(pressed)) => name == pressed,
                    (Position(code), _) => position == PhysicalKey::Code(*code),
                    _ => false,
                }
        })
        .map(|(_, (_, action))| *action)
}

/// What an event leaves the window owing.
#[must_use]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Effect {
    /// The frame is out of date.
    Redraw,
    Nothing,
    /// The user has asked to leave.
    Quit,
}

impl Effect {
    pub fn redraw_if(changed: bool) -> Self {
        if changed {
            Effect::Redraw
        } else {
            Effect::Nothing
        }
    }
}

/// Where the pointer is and what it is doing.
#[derive(Default)]
pub(super) struct Pointer {
    pub(super) modifiers: ModifiersState,
    /// Physical window pixels, and the point a wheel zoom works about.
    pub(super) cursor: Option<[f32; 2]>,
    /// Whether the left button is down, which is what a drag is.
    pub(super) dragging: bool,
    /// Set instead when the button went down on the info panel: the drag then
    /// belongs to that column and moves it, wherever the pointer goes while
    /// the button is held. Held apart from `dragging` so that a drag is one
    /// thing or the other for as long as it lasts, rather than changing what
    /// it moves the moment the pointer leaves the panel.
    pub(super) scrolling: bool,
    /// Where the pointer was when the drag last moved the view. Held apart
    /// from `cursor` because a press can arrive before any motion has told us
    /// where the pointer is, and because leaving the window clears `cursor`
    /// without ending a drag the pointer grab is still delivering.
    pub(super) drag_from: Option<[f32; 2]>,
    /// What the press that is down would copy out of the info panel, and
    /// where it went down.
    ///
    /// A press there starts a scroll as well, the two gestures being
    /// indistinguishable at the moment the button goes down; so the copy is
    /// only made if the button comes back up without the pointer having gone
    /// anywhere, and any real travel drops it and leaves a drag behind.
    pub(super) copying: Option<(Copyable, [f32; 2])>,
}

/// How far the pointer may wander between a press on the info panel and the
/// release that follows it and still be a click rather than a drag. Physical
/// pixels, being what the pointer reports: this is about the hand holding
/// still, which it does to within about this much whatever the display.
const COPY_SLOP: f32 = 4.0;

impl Pointer {
    /// Whether a wheel event belongs to the window manager rather than to us:
    /// Ctrl with the wheel is a compositor gesture, and zooming on it as well
    /// would move the view behind the user's back. Keys answer the same
    /// question through [`satisfies`], which lets a chord this table does
    /// bind through.
    fn chorded(&self) -> bool {
        self.modifiers.control_key() || self.modifiers.alt_key() || self.modifiers.super_key()
    }
}

/// Says that a copy could not be made. Copies happen because a key was
/// pressed and show nothing on screen when they work, so the only thing worth
/// saying is when one did not.
fn report(error: &anyhow::Error) {
    eprintln!("gamut: {}", crate::escape_controls(&format!("{error:#}")));
}

impl App {
    pub(super) fn handle_key(&mut self, key: &Key, position: PhysicalKey) -> Effect {
        match action_for(key, position, self.pointer.modifiers) {
            Some(action) => self.perform(action),
            None => Effect::Nothing,
        }
    }

    /// Does what a key asked for.
    pub(super) fn perform(&mut self, action: Action) -> Effect {
        let image = self.image_size();
        let viewport = self.viewport();
        match action {
            // An open menu takes the key: dismissing a popup is what Escape
            // is for, and quitting out from under one is not what was being
            // asked for.
            Quit => {
                if self.panels.menu.take().is_some() {
                    self.update_hover();
                    return Effect::Redraw;
                }
                return Effect::Quit;
            }
            ZoomIn => self.view.zoom_in(image, viewport),
            ZoomOut => self.view.zoom_out(image, viewport),
            ZoomTo(scale) => self.view.set_zoom(scale, image, viewport),
            Pan(direction, step) => {
                let sign = direction.sign();
                match step.pixels() {
                    Some(by) => self
                        .view
                        .pan_by(sign[0] * by, sign[1] * by, image, viewport),
                    None => self.view.pan_to_edge(sign, image, viewport),
                }
            }
            CycleFit => self.view.cycle_fit(),
            CycleUpscale => self.view.cycle_upscale(),
            // Nothing to draw yet: the file is only being asked for, and what
            // is on screen stays until it arrives.
            NextFile => {
                self.step(true);
                return Effect::Nothing;
            }
            PreviousFile => {
                self.step(false);
                return Effect::Nothing;
            }
            // A fitted image re-fits on the next frame: the viewport it is
            // measured against is the one the panels leave, and they have
            // just come or gone.
            ToggleInterface => {
                self.panels.show_ui = !self.panels.show_ui;
                // The menu is part of the interface, and goes with it.
                self.panels.menu = None;
                self.panels.hover = None;
            }
            // The three panels float over the image rather than inside the
            // bars, so hiding the interface leaves them behind. This asks for
            // the picture on its own, and closes them on the way. They stay
            // closed when the bars come back: what the key put away, it is
            // not the key's business to bring out again.
            ToggleInterfaceAndPanels => {
                self.panels.show_minimap = false;
                self.panels.show_histogram = false;
                self.panels.show_info = false;
                return self.perform(ToggleInterface);
            }
            ToggleHistogram => self.press(Widget::Histogram),
            ToggleLogCounts => self.press(Widget::Log),
            ToggleInfo => self.press(Widget::Info),
            ToggleMinimap => self.press(Widget::Minimap),
            ToggleGrid => self.press(Widget::Grid),
            Exposure(stops) => {
                return self.adjust(|current, _| {
                    current.display.adjust_exposure(stops);
                    true
                });
            }
            CycleAutoWindow => {
                return self.adjust(|current, _| {
                    current.display.cycle_auto(&current.stats);
                    true
                });
            }
            ShiftWindow(by) => {
                return self.adjust(|current, _| {
                    current.display.shift_window(by);
                    true
                });
            }
            Contrast(by) => {
                return self.adjust(|current, _| {
                    current.display.adjust_contrast(by);
                    true
                });
            }
            CycleToneMap => {
                return self.adjust(|current, _| {
                    current.display.cycle_tone_map();
                    true
                });
            }
            CycleColormap => {
                return self.adjust(|current, _| {
                    if !current.image.is_gray() {
                        return false;
                    }
                    current.display.cycle_colormap();
                    true
                });
            }
            // Nothing on screen changes; a copy is reported only when it
            // could not be made.
            CopyPath => {
                let path = self.shown_path();
                self.copy(path.to_string_lossy().as_bytes(), clipboard::TEXT);
                return Effect::Nothing;
            }
            CopyUri => {
                let list = clipboard::uri_list(&self.shown_path());
                self.copy(list.as_bytes(), clipboard::URI_LIST);
                return Effect::Nothing;
            }
            CopyImage => {
                self.copy_image();
                return Effect::Nothing;
            }
            // Whether or not the panel is open: what it says is a fact about
            // the file, and asking for it should not mean first arranging to
            // look at it.
            CopyMetadata => {
                self.copy_facts(Copyable::All);
                return Effect::Nothing;
            }
            // Nothing to draw yet either: the picture is being written and
            // then read, and what is on screen stays until it arrives.
            Paste => {
                self.paste();
                return Effect::Nothing;
            }
            ResetDisplay => {
                return self.adjust(|current, _| {
                    current.display.reset(&current.stats, &current.image);
                    true
                });
            }
        }
        Effect::Redraw
    }

    /// Runs `change` on the image on screen, if there is one. `change` says
    /// whether it changed anything, since some settings apply only to some
    /// images.
    fn adjust(&mut self, change: impl FnOnce(&mut Current, Startup) -> bool) -> Effect {
        let startup = self.startup;
        let Some(current) = &mut self.current else {
            return Effect::Nothing;
        };
        Effect::redraw_if(change(current, startup))
    }

    /// The absolute path of the file on screen. Absolute because what is
    /// copied is bound for somewhere else, where the directory this was
    /// started in means nothing — and because a URI has no other kind. The
    /// path as given stands in if it cannot be made absolute, which needs the
    /// working directory and so can fail.
    fn shown_path(&self) -> PathBuf {
        let path = self.files.shown_path();
        std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
    }

    /// Puts the picture on screen on the clipboard as a PNG.
    ///
    /// The image at its own size with the display settings baked in, not a
    /// picture of the window: the zoom, the pan and the panels are how this
    /// is being looked at, and none of them belong to what is being copied.
    ///
    /// Done on a thread of its own. Walking every pixel takes long enough on
    /// a large image to be felt as the window going quiet, and a viewer that
    /// stops answering the pointer is a viewer that looks broken. The image
    /// is shared rather than copied, and the display state is a handful of
    /// numbers, so handing the work over costs nothing worth measuring.
    fn copy_image(&mut self) {
        let Some(current) = &self.current else {
            return;
        };
        let image = Arc::clone(&current.image);
        let display = current.display.clone();
        let (asked, copies) = (self.claim_copy(), Arc::clone(&self.copies));

        // Threads that have already handed their bytes over are dropped as
        // each new copy is asked for, so the list is what is still in flight
        // rather than every copy the session has ever made.
        self.copying.retain(|thread| !thread.is_finished());
        self.copying.push(std::thread::spawn(move || {
            let (width, height) = (image.width, image.height);

            let walked = Instant::now();
            let raster = encode::displayed(&image, &display);
            timing::mapped_image(width, height, walked.elapsed());

            let encoded = Instant::now();
            let png = match encode::png(&raster) {
                Ok(png) => png,
                Err(error) => return report(&error),
            };
            timing::encoded_png(width, height, png.len(), encoded.elapsed());

            // Something has been copied since this was asked for, and taking
            // the selection now would put back a picture the user has already
            // moved on from.
            if copies.load(Ordering::Relaxed) != asked {
                return;
            }
            if let Err(error) = clipboard::copy(&png, clipboard::PNG) {
                report(&error);
            }
        }));
    }

    /// Writes the picture on the clipboard to a file of its own and shows it.
    ///
    /// A file and not just pixels: a paste comes from somewhere with no file
    /// behind it — a screenshot, a browser, an editor — and showing it without
    /// writing it would leave the user nothing to come back to and nothing to
    /// step back to. So it is written where the desktop keeps pictures, and
    /// joins the list beside the one on screen.
    ///
    /// Asking what the clipboard is offering is a word with the compositor and
    /// nothing more, so it is done here. Fetching the bytes means waiting on
    /// whichever program holds the selection, and that goes to the loader with
    /// the reading — which also means a paste that will not arrive, or will not
    /// decode, is reported exactly as an unreadable file is.
    fn paste(&mut self) {
        let offer = match clipboard::offered_image() {
            Ok(Some(offer)) => offer,
            // Not a failure: a key was pressed and there was nothing there.
            Ok(None) => {
                eprintln!("gamut: nothing on the clipboard that could be shown");
                return;
            }
            Err(error) => return report(&error),
        };
        let path = match pasted::reserve(offer.extension) {
            Ok(path) => path,
            Err(error) => return report(&error),
        };
        let request = self.files.adopt(path, Source::Clipboard(offer.mime));
        self.send(request);
    }

    /// Puts what the info panel says on the clipboard: as much of a table as
    /// what was clicked actually is — see [`ui::info::copied`].
    ///
    /// Plain text under the hood, whatever the rows are shaped like: a copy
    /// is bound for somewhere else, and every place words can be pasted takes
    /// those. Offered as CSV alone it would paste into a spreadsheet and
    /// nowhere else.
    fn copy_facts(&mut self, copies: Copyable) {
        let Some(current) = &self.current else {
            return;
        };
        let rows = ui::info::copied(current, copies);
        if rows.is_empty() {
            return;
        }
        self.copy(rows.as_bytes(), clipboard::TEXT);
    }

    /// Puts `content` on the clipboard under `mime_type`.
    ///
    /// Done in line, unlike the picture: there is nothing here to prepare, and
    /// a copy the user follows straight away with `q` should be on the
    /// clipboard before the window goes.
    fn copy(&mut self, content: &[u8], mime_type: &str) {
        self.claim_copy();
        if let Err(error) = clipboard::copy(content, mime_type) {
            report(&error);
        }
    }

    /// Marks a copy as the one most recently asked for, and says which number
    /// it is. A copy that has to go away and prepare itself compares this
    /// against the counter when it comes back.
    fn claim_copy(&self) -> u64 {
        self.copies.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Starts or ends a drag of the image with the left button. The pointer
    /// keeps its grab until the button comes back up, so a drag that leaves
    /// the window goes on working.
    ///
    /// A press does not need to know where the pointer is: the first motion
    /// after it establishes the point the drag measures from. Waiting for that
    /// costs nothing, and a press can genuinely arrive with no position yet —
    /// the pointer entering the window and clicking without moving.
    /// Returns `true` if the click changed anything on screen, which a press
    /// on a widget does and a press on the image does not.
    pub(super) fn handle_button(&mut self, state: ElementState, button: MouseButton) -> bool {
        if button != MouseButton::Left {
            return false;
        }

        // A press that went down on something the info panel copies and has
        // not travelled since is a click on it, and this is where it is
        // answered: the release ends the drag it also started, and only one
        // of the two gestures can have been meant.
        if state == ElementState::Released
            && let Some((copies, _)) = self.pointer.copying.take()
        {
            self.copy_facts(copies);
        }

        if state == ElementState::Pressed {
            // Wherever this one is going, it is not the press that went down
            // on the info panel, so there is nothing left to copy.
            self.pointer.copying = None;
        }

        // A press goes to the layer it lands on and no further: the panels
        // are opaque to the pointer, so exactly one of them answers, and one
        // that misses a button is still spent where it landed rather than
        // reaching the picture behind it. A press that arrives before any
        // motion has said where the pointer is has no layer to land on, and
        // belongs to the image — as a release always does.
        if state == ElementState::Pressed
            && let Some(hit) = self.pointer_hit()
        {
            // The menu that is open takes it first, wherever it is: a press
            // on a cell chooses and closes, one anywhere else dismisses and
            // is spent doing exactly that.
            if self.menu_has_pointer(Some(hit)) {
                self.panels.menu = None;
                // The cell that had the highlight is no longer there at all.
                self.update_hover();
                return true;
            }
            match hit {
                // The picture: on to the drag below.
                Hit::Image => {}
                // The info panel's column, which the press starts a drag of
                // instead of one of the picture underneath.
                Hit::Info => {
                    // The layer that answered is the panel, so it is on
                    // screen and has a rectangle to be dragged against.
                    if let Some(panel) = self.info_panel() {
                        self.press_info(panel);
                    }
                    return false;
                }
                _ => match hit.widget() {
                    Some(widget) => {
                        self.press(widget);
                        // The zoom readout keeps the pointer over it as it
                        // opens its menu, and the highlight belongs to the
                        // menu from here on.
                        self.update_hover();
                        return true;
                    }
                    // A panel, between its buttons or with none at all.
                    None => return false,
                },
            }
        }

        self.pointer.dragging = state == ElementState::Pressed;
        self.pointer.scrolling = false;
        self.pointer.drag_from = if self.pointer.dragging {
            self.pointer.cursor
        } else {
            None
        };

        if let Some(window) = &self.window {
            // The closed hand is a promise that dragging will move something,
            // so a fitted image — which has nowhere to go — does not make it.
            let icon =
                if self.pointer.dragging && self.view.can_pan(self.image_size(), self.viewport()) {
                    CursorIcon::Grabbing
                } else {
                    CursorIcon::Default
                };
            window.set_cursor(Cursor::Icon(icon));
        }
        false
    }

    /// Takes a press on the info panel: it starts a drag of the column, and
    /// may turn out to have been a click on one of its copy buttons — the two
    /// are indistinguishable at the moment the button goes down, so both are
    /// begun and the release decides which it was.
    fn press_info(&mut self, panel: Rect) {
        self.pointer.copying = self.info_copyable().zip(self.pointer.cursor);
        self.pointer.scrolling = true;
        // As with a drag of the image: the first motion after the press
        // establishes the point the drag is measured from.
        self.pointer.drag_from = self.pointer.cursor;
        // The closed hand is a promise that dragging will move something, so
        // a column with nothing left to scroll does not make it.
        let icon = if self.info_overflow(panel) > 0.0 {
            CursorIcon::Grabbing
        } else {
            CursorIcon::Default
        };
        if let Some(window) = &self.window {
            window.set_cursor(Cursor::Icon(icon));
        }
    }

    /// Follows the pointer. Returns `true` if the frame is now out of date —
    /// because a drag moved the view, or because the bar is reporting a pixel
    /// the pointer has since left.
    pub(super) fn handle_motion(&mut self, position: [f32; 2]) -> bool {
        let was_over = self.pointer_pixel();
        let was_marked = self.histogram_mark();
        self.pointer.cursor = Some(position);
        // Either readout having moved on is a frame out of date: the pixel in
        // the bar, and the bin the histogram is marking — that one on a panel
        // floating over the image rather than in a bar, so it counts whether
        // or not the bars are showing. It follows the pixel as well as the
        // pointer, and a pointer that has crossed into a new bin without
        // leaving its pixel still owes a frame.
        let moved_pixel = (self.panels.show_ui && self.pointer_pixel() != was_over)
            || self.histogram_mark() != was_marked;
        if self.pointer.scrolling {
            // Far enough from where the button went down and this is a drag
            // of the column, not a click on what was under it.
            if let Some((_, at)) = self.pointer.copying
                && ((position[0] - at[0]).abs() > COPY_SLOP
                    || (position[1] - at[1]).abs() > COPY_SLOP)
            {
                self.pointer.copying = None;
            }
            let Some(from) = self.pointer.drag_from.replace(position) else {
                // First motion of this drag: nothing to measure from yet.
                return false;
            };
            let Some(panel) = self.info_panel() else {
                return false;
            };
            // The drag holds the scrollbar's thumb rather than the words:
            // dragging down runs down the column, as pulling the thumb down
            // would, and by as much as pulling it that far would move — which
            // for a long column is a good deal further than the pointer went.
            // Motion arrives in physical pixels and the column is laid out in
            // logical ones.
            let by =
                (position[1] - from[1]) / self.scale_factor() * self.info_scroll_per_drag(panel);
            // The readouts are owed a redraw too, for a drag that has
            // carried the pointer off the panel and onto the image.
            let scrolled = self.scroll_info_by(panel, by);
            return self.forget_info_hover() || scrolled || moved_pixel;
        }
        if !self.pointer.dragging {
            // Nothing else to do out here, so this is where the button's
            // highlight gets to follow the pointer.
            return self.update_hover() || moved_pixel;
        }
        let Some(from) = self.pointer.drag_from.replace(position) else {
            // First motion of this drag: nothing to measure from yet.
            return false;
        };
        // The image follows the pointer, so the viewport moves the other way.
        let (dx, dy) = (from[0] - position[0], from[1] - position[1]);
        if dx == 0.0 && dy == 0.0 {
            return false;
        }
        self.view.pan_by(dx, dy, self.image_size(), self.viewport());
        true
    }

    /// Takes the copy button off the info panel while the column is being
    /// scrolled. Returns `true` if there was one to take off.
    ///
    /// The pointer is not moving; the words under it are, and a button that
    /// followed whichever of them happened to be passing would blink from
    /// field to field all the way down the column. It comes back on the next
    /// motion, which is when the reader is pointing at something again rather
    /// than reading past it.
    fn forget_info_hover(&mut self) -> bool {
        self.panels.info_hover.take().is_some()
    }

    /// Re-tests the pointer against the widgets, and against the info
    /// panel's column. Returns `true` if either highlight moved, and so if
    /// the frame is now out of date.
    ///
    /// The two are asked together because they answer the same question — is
    /// anything under the pointer lit that was not, or dark that was — and
    /// because a press or a scroll that moves one can move the other.
    pub(super) fn update_hover(&mut self) -> bool {
        let hit = self.pointer_hit();
        // Nothing behind an open menu lights up: the press that would land
        // there dismisses the menu rather than reaching the button under it,
        // and a button that lights for a press it will not get is a lie.
        //
        // The info panel's rows are asked separately because they are not
        // chrome widgets: which one the pointer is on takes the fonts to
        // answer. Not gated on the bars either — the panel can be on screen
        // with the chrome hidden, and its buttons go with it.
        let (hover, info) = if self.menu_has_pointer(hit) {
            (None, None)
        } else {
            (hit.and_then(Hit::widget), self.info_copyable())
        };
        let changed = hover != self.panels.hover || info != self.panels.info_hover;
        self.panels.hover = hover;
        self.panels.info_hover = info;
        changed
    }

    /// Acts on a press. The keys that stand in for the toggles come through
    /// here too, so that a key and a click cannot drift apart.
    fn press(&mut self, widget: Widget) {
        match widget {
            Widget::Minimap => self.panels.show_minimap = !self.panels.show_minimap,
            Widget::Histogram => self.panels.show_histogram = !self.panels.show_histogram,
            Widget::Grid => self.panels.show_grid = !self.panels.show_grid,
            Widget::Info => self.panels.show_info = !self.panels.show_info,
            // Only ever opens one: the press that closes a menu is answered
            // by the menu itself, before the widgets underneath are asked.
            // A window with no room for the panel gets no menu rather than a
            // state nothing on screen accounts for.
            Widget::Zoom => {
                let chrome = self.chrome();
                if self.current.is_some()
                    && chrome
                        .popup(Menu::Zoom, self.grid_spacing().as_deref())
                        .is_some()
                {
                    self.panels.menu = Some(Menu::Zoom);
                }
            }
            // The action the key runs, as with the reset below: the button
            // is on screen because the clipboard was holding a picture at the
            // last look, and the paste asks it again rather than trusting
            // that. A selection that has gone in between is answered the way
            // an empty clipboard is.
            Widget::Paste => self.paste(),
            Widget::Luma => self.panels.show_luma = !self.panels.show_luma,
            Widget::Planes => self.panels.show_planes = !self.panels.show_planes,
            // The plot's own axis rather than anything about the rendering,
            // which is why the reset below leaves it alone: it is how the
            // measurement is being read, not what is being read.
            Widget::Log => self.panels.log_counts = !self.panels.log_counts,
            // The action the key runs, rather than a second reading of what
            // "reset" means: two of them would answer differently the first
            // time either was touched, and a button and a key that disagree
            // about one word are worse than either alone.
            Widget::Reset => {
                // The caller redraws for every press, so the effect this
                // hands back says nothing the caller does not already know.
                let _ = self.perform(ResetDisplay);
            }
            Widget::Ramp(index) => {
                if let Some(current) = self.current.as_mut()
                    && let Some(map) = Colormap::ALL.get(index)
                {
                    current.display.colormap = *map;
                }
            }
            Widget::Cell(index) => {
                if let Some(menu) = self.panels.menu.take() {
                    let (image, viewport) = (self.image_size(), self.viewport());
                    menu.choose(index, &mut self.view, image, viewport);
                }
            }
        }
    }

    /// Scrolls the info panel with the wheel.
    ///
    /// `None` when the pointer is not over the panel, and the wheel is the
    /// image's to zoom with; `Some` when it is, whether or not the column
    /// actually moved — a panel with nothing left to scroll to has still
    /// taken the gesture, and must not hand a spin at its last line back to
    /// the image underneath.
    fn scroll_info(&mut self, delta: MouseScrollDelta) -> Option<bool> {
        let panel = self.pointer_over_info()?;

        // A wheel turned away from the reader moves the column up the panel,
        // which is the scroll running down the text.
        let by = match delta {
            MouseScrollDelta::LineDelta(_, lines) => -lines * WHEEL_SCROLL_STEP,
            MouseScrollDelta::PixelDelta(pixels) => -pixels.y as f32,
        };
        Some(self.scroll_info_by(panel, by))
    }

    /// Returns `true` if the wheel changed anything on screen.
    pub(super) fn handle_wheel(&mut self, delta: MouseScrollDelta) -> bool {
        // Same reasoning as `handle_key`: Ctrl+wheel and friends belong to the
        // compositor, and acting on them as well would zoom behind its back.
        if self.pointer.chorded() {
            return false;
        }

        // The wheel goes to the layer the pointer is on, as a press does.
        let hit = self.pointer_hit();
        if self.menu_has_pointer(hit) {
            return false;
        }
        match hit {
            // The info panel takes it: a column with more to say than fits is
            // what a wheel is for, and the image behind the panel is not what
            // the gesture was aimed at.
            Some(Hit::Info) => {
                let scrolled = self.scroll_info(delta).unwrap_or(false);
                return self.forget_info_hover() || scrolled;
            }
            // Every other panel is opaque to the wheel as it is to a press,
            // and has nothing to do with one: the spin is spent there rather
            // than zooming the picture it is floating over.
            Some(Hit::Cell(_) | Hit::Menu | Hit::Minimap | Hit::Histogram(_) | Hit::Chrome(_)) => {
                return false;
            }
            // The picture, or a pointer that has not yet said where it is.
            Some(Hit::Image) | None => {}
        }

        let steps = match delta {
            MouseScrollDelta::LineDelta(_, lines) => lines,
            MouseScrollDelta::PixelDelta(pixels) => pixels.y as f32 / WHEEL_PIXELS_PER_STEP,
        };
        // A trackpad emits a long tail of all but motionless events at the end
        // of a gesture, which would leave the view drifting after the finger
        // has stopped.
        if !steps.is_finite() || steps.abs() < 1e-3 {
            return false;
        }

        let viewport = self.viewport();
        let anchor = self.pointer.cursor.unwrap_or([
            viewport.x + viewport.width / 2.0,
            viewport.y + viewport.height / 2.0,
        ]);
        self.view
            .zoom_steps_at(steps, anchor, self.image_size(), viewport);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A chord bound twice would do whichever came first in the table,
    /// silently. The same key under different modifiers is a different chord.
    #[test]
    fn no_chord_is_bound_twice() {
        let mut seen: Vec<(Mods, KeyName)> = Vec::new();
        for binding in KEYS {
            for (name, _) in binding.keys {
                assert!(
                    !seen.contains(&(binding.mods, *name)),
                    "{name:?} with {:?} is bound more than once",
                    binding.mods
                );
                seen.push((binding.mods, *name));
            }
        }
    }

    /// Shift belongs to the character, not to the modifiers: a binding on a
    /// character that asked for it as well would never match, since
    /// `satisfies` takes it out of what is held before comparing. Only a key
    /// Shift does not change — one bound by name or by position — may ask
    /// for it.
    #[test]
    fn only_layout_free_keys_are_bound_with_shift() {
        for binding in KEYS {
            if !binding.mods.shift_key() {
                continue;
            }
            for (name, _) in binding.keys {
                assert!(
                    matches!(name, Named(_) | Position(_)),
                    "`{}` asks for Shift on {name:?}; say it with the character instead",
                    binding.shown
                );
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
        assert_eq!(
            action_for(&Key::Named(NamedKey::Escape), ELSEWHERE, PLAIN),
            Some(Quit)
        );
        assert_eq!(
            action_for(&Key::Named(NamedKey::PageDown), ELSEWHERE, PLAIN),
            Some(NextFile)
        );
        assert_eq!(plain("]"), Some(NextFile));
        assert_eq!(plain("F"), Some(Exposure(0.5)));
        // The window's position and its width are the same two keys in
        // different cases.
        assert_eq!(plain("a"), Some(ShiftWindow(-0.05)));
        assert_eq!(plain("A"), Some(Contrast(0.8)));
        assert_eq!(plain("w"), None);
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
    /// shifted press as well.
    #[test]
    fn the_arrows_pan_by_what_is_held_with_them() {
        let left = Key::Named(NamedKey::ArrowLeft);
        let held = |mods| action_for(&left, ELSEWHERE, mods);
        assert_eq!(held(PLAIN), Some(Pan(Left, Coarse)));
        assert_eq!(held(SHIFT), Some(Pan(Left, Fine)));
        assert_eq!(held(CTRL), Some(Pan(Left, Edge)));
        assert_eq!(held(CTRL | SHIFT), None);
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

    /// The three things `c` does are told apart by what is held with it,
    /// and a chord nothing binds is still left to the window manager.
    #[test]
    fn modifiers_tell_chords_apart() {
        use winit::keyboard::SmolStr;
        // Shift is what turns the character upper case in the first place,
        // so it is held for every reading of `C`.
        let lower = Key::Character(SmolStr::new("c"));
        let upper = Key::Character(SmolStr::new("C"));
        // The lower case on its own is not bound at all.
        assert_eq!(action_for(&lower, ELSEWHERE, PLAIN), None);
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
}
