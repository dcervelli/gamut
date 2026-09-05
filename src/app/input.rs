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
use crate::image::display::{AutoWindow, Colormap, Startup, ToneMap};
use crate::image::encode;
use crate::loader::Source;
use crate::pasted;
use crate::render::Rect;
use crate::timing;
use crate::ui::histogram;
use crate::ui::info::Copyable;
use crate::ui::layers::Hit;
use crate::ui::menu::{Copies, Reach};
use crate::ui::toast::Level;
use crate::ui::{self, Current, Menu, Panels, Tip, Widget};

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

/// What the window says the first time the interface is hidden. Both keys,
/// since the one that put it away is not the one a reader who pressed the
/// button knows about.
const RESTORE: &str = "Press ` or Esc to restore UI";

/// Something a key asks for.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    Quit,
    /// Take off whatever is up — the menu that is open, or the message at the
    /// foot of the window — and quit when nothing is. Escape's, so that the
    /// key that puts things away is not also the key that leaves; `q` quits
    /// whether or not a message is showing.
    Dismiss,
    ZoomIn,
    ZoomOut,
    /// Go to this zoom, 1.0 being one image pixel to one screen pixel.
    ZoomTo(f32),
    Pan(Direction, PanStep),
    ToggleFit,
    CycleUpscale,
    NextFile,
    PreviousFile,
    ToggleInterface,
    /// The interface, and the panels floating over the image with it: the
    /// bars come and go as [`Action::ToggleInterface`], and the map,
    /// histogram and information panel are closed on the way past.
    ToggleInterfaceAndPanels,
    ToggleHistogram,
    /// Which of that panel's planes are plotted. Both can be off: the panel
    /// still has its response curve and its ramp to read.
    ToggleLuma,
    TogglePlanes,
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
    /// Between the SDR and the HDR surface, where the driver offers the
    /// choice.
    ToggleHdr,
    /// Put the name of the file on screen on the clipboard, with nothing of
    /// the directory it sits in.
    CopyName,
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
    /// Step the bottom bar's readout through the ways of writing a pixel's
    /// value: hexadecimal, decimal, mapped.
    CyclePixelFormat,
    /// Put the value of the pixel under the pointer on the clipboard, written
    /// exactly as the bar is writing it.
    CopyPixelValue,
    /// Put that pixel's coordinate on the clipboard, as `x,y`.
    CopyPixelCoordinate,
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

impl KeyName {
    /// How this key is written for people, where one key of a line has to be
    /// named on its own — the four zooms of the number row are one line of
    /// `--help` and four cells of the zoom menu.
    ///
    /// `None` for a key nothing has yet had to name singly, which leaves
    /// [`Binding::shown`] to answer for the whole line. Only the number row
    /// is bound by position, and the character keys say what they are.
    fn spelled(self) -> Option<&'static str> {
        match self {
            Char(character) => Some(character),
            Named(_) => None,
            Position(code) => Some(match code {
                KeyCode::Digit0 => "0",
                KeyCode::Digit1 => "1",
                KeyCode::Digit2 => "2",
                KeyCode::Digit3 => "3",
                KeyCode::Digit4 => "4",
                KeyCode::Digit5 => "5",
                KeyCode::Digit6 => "6",
                KeyCode::Digit7 => "7",
                KeyCode::Digit8 => "8",
                KeyCode::Digit9 => "9",
                _ => return None,
            }),
        }
    }
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

/// The line of the key table that performs `action`: what to press for it,
/// and what `--help` says it does.
///
/// The first such line. An action bound on more than one line — the arrows
/// and their modified forms — is a different action on each, so the first is
/// the one that answers.
fn binding_for(action: Action) -> Option<&'static Binding> {
    KEYS.iter()
        .find(|binding| binding.keys.iter().any(|(_, bound)| *bound == action))
}

/// What to press for `action`, as the key table writes it.
///
/// The whole key column of the line that binds it — a line that binds two
/// keys to one action offers both — except where the line binds several keys
/// to several different things and only one of them does this. The number
/// row is one such line, `2, 3, 4, 5` for four zooms, and the cell of the
/// zoom menu that goes to one of them is named by the key that reaches it
/// rather than by all four. Where that key cannot be spelled on its own the
/// column answers, as it does everywhere else.
fn shown_for(binding: &Binding, action: Action) -> String {
    let mut doing = binding.keys.iter().filter(|(_, bound)| *bound == action);
    match (doing.next(), doing.next()) {
        (Some((key, _)), None) if binding.keys.len() > 1 => match key.spelled() {
            Some(key) => format!("{}{key}", held(binding.mods)),
            None => binding.shown.to_string(),
        },
        _ => binding.shown.to_string(),
    }
}

/// What is held down with a key, written as [`Binding::shown`] writes it:
/// `Ctrl+Shift+`, and nothing at all for a key held with nothing.
fn held(mods: Mods) -> String {
    let mut prefix = String::new();
    for (modifier, name) in [
        (Mods::CONTROL, "Ctrl"),
        (Mods::ALT, "Alt"),
        (Mods::SUPER, "Super"),
        (Mods::SHIFT, "Shift"),
    ] {
        if mods.contains(modifier) {
            prefix.push_str(name);
            prefix.push('+');
        }
    }
    prefix
}

/// How far one nudge of the display window moves it, as a fraction of its
/// own width, and what one narrowing or widening scales that width by.
///
/// Named because the histogram panel's four nudges are the same four steps
/// as the keys below: a button that moved the window by some other amount
/// would be a second answer to a question that already has one.
const WINDOW_STEP: f32 = 0.05;
const NARROWER: f32 = 0.8;
const WIDER: f32 = 1.25;

/// One line of a tooltip: what a key does, and what to press for it.
///
/// The key table's own words, so that a tooltip and `--help` cannot come to
/// disagree about a binding — there is nowhere for them to disagree.
fn hint(action: Action) -> Option<String> {
    let binding = binding_for(action)?;
    Some(format!("{} ({})", binding.help, binding.shown))
}

/// The action a thing in the interface stands for, which is what names it.
///
/// The buttons are the keys' twins — `App::press` is careful that a button
/// and a key never drift apart — so a button is named by what its key does.
/// `None` for the things no key reaches, which name themselves instead: see
/// [`ui::tooltip::words`].
fn action_of(tip: Tip, panels: &Panels) -> Option<Action> {
    Some(match tip {
        // The pair at the head of the top bar, which go where the keys beside
        // the count go.
        Tip::Widget(Widget::Previous) => PreviousFile,
        Tip::Widget(Widget::Next) => NextFile,
        Tip::Widget(Widget::Minimap) => ToggleMinimap,
        Tip::Widget(Widget::Histogram) => ToggleHistogram,
        Tip::Widget(Widget::Info) => ToggleInfo,
        Tip::Widget(Widget::Grid) => ToggleGrid,
        // The plain press: Shift on the same button asks for the other key,
        // which the tooltip lists under this one — see `App::tooltip`.
        Tip::Widget(Widget::Maximize) => ToggleInterface,
        Tip::Widget(Widget::Output) => ToggleHdr,
        Tip::Widget(Widget::Paste) => Action::Paste,
        // The dot at the head of the pixel readout, which the key steps
        // through exactly as a press on one of its cells chooses.
        Tip::Widget(Widget::PixelFormat) => CyclePixelFormat,
        // The histogram panel's own, and the row of false colors a key
        // cycles through.
        Tip::Widget(Widget::Luma) => ToggleLuma,
        Tip::Widget(Widget::Planes) => TogglePlanes,
        Tip::Widget(Widget::Log) => ToggleLogCounts,
        Tip::Widget(Widget::Reset) => ResetDisplay,
        // Only the swatches that are actually on offer: an index past the
        // end is not a false color, and naming it after the key that cycles
        // them would be naming nothing.
        Tip::Widget(Widget::Ramp(index)) if index < Colormap::ALL.len() => CycleColormap,
        // The same for the two rows under them: the key steps through the
        // windows and the curves in turn where a button names one outright.
        Tip::Widget(Widget::Window(index)) if index < histogram::WINDOWS.len() => CycleAutoWindow,
        Tip::Widget(Widget::Curve(index)) if index < ToneMap::ALL.len() => CycleToneMap,
        // The four nudges beside the window's reading are the keys with a
        // picture on them: the same four amounts, so a press and a keystroke
        // move the window by the same step and are named by the same words.
        Tip::Widget(Widget::WindowDown) => ShiftWindow(-WINDOW_STEP),
        Tip::Widget(Widget::WindowUp) => ShiftWindow(WINDOW_STEP),
        Tip::Widget(Widget::WindowNarrow) => Contrast(NARROWER),
        Tip::Widget(Widget::WindowWiden) => Contrast(WIDER),
        // A cell of a menu sets one state directly where the key steps
        // through them all: the key is worth naming, the description of the
        // step is not — see `Menu::cell_tip`. A numbered cell of the zoom menu is
        // the exception, its key going straight to the same zoom.
        Tip::Widget(Widget::Cell(index)) => match panels.menu?.cell_tip(index)?.reach {
            Reach::Fit => ToggleFit,
            Reach::Upscale => CycleUpscale,
            Reach::PixelFormat => CyclePixelFormat,
            Reach::Zoom(scale) => ZoomTo(scale),
            // The one menu whose cells are things done rather than states to
            // be in: the key does exactly what the cell does, and its line of
            // the table is what names both.
            Reach::Copy(what) => copy_action(what),
        },
        // The words at the end of the bottom bar are about four settings at
        // once, so no one key does what they do; what a press on them opens
        // is the panel that sets all four, which the tooltip says outright.
        Tip::Widget(_) | Tip::Name | Tip::Counter | Tip::State => return None,
    })
}

/// The action a cell of the menu of copies asks for.
///
/// Which copy each cell is is the menu's; what that copy does, what it is
/// called and which key runs it are all the key table's, and this is the one
/// place the two are put together.
fn copy_action(what: Copies) -> Action {
    match what {
        Copies::Name => CopyName,
        Copies::Path => CopyPath,
        Copies::Uri => CopyUri,
        Copies::Image => CopyImage,
        Copies::Facts => CopyMetadata,
    }
}

/// What names `tip` on the first line of its tooltip: its own words where the
/// interface has some for it, and otherwise the description of the key that
/// does the same job — with that key after it either way.
fn names(tip: Tip, panels: &Panels) -> Option<String> {
    let pressed = action_of(tip, panels).and_then(|action| {
        let binding = binding_for(action)?;
        Some((binding.help, shown_for(binding, action)))
    });
    match (ui::tooltip::words(tip, panels), pressed) {
        (Some(words), Some((_, key))) => Some(format!("{words} ({key})")),
        (Some(words), None) => Some(words),
        (None, Some((help, key))) => Some(format!("{help} ({key})")),
        (None, None) => None,
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
        help: "Toggle fit between the whole image and filling the window",
        keys: &[(Named(NamedKey::Space), ToggleFit)],
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
    Binding {
        section: Section::Clipboard,
        mods: PLAIN,
        shown: "c",
        help: "Copy the name of the file on screen, without its path",
        keys: &[(Char("c"), CopyName)],
    },
    // The next two are both the capital, so both are typed with Shift held;
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
        help: "Copy the image itself, as the display settings show it",
        keys: &[(Char("c"), CopyImage)],
    },
    Binding {
        section: Section::Clipboard,
        mods: CTRL,
        shown: "Ctrl+I",
        help: "Copy everything the info panel says about the file",
        keys: &[(Char("i"), CopyMetadata), (Char("I"), CopyMetadata)],
    },
    // The full stop and the greater-than are one key on most keyboards, and
    // as with the two `C`s above only the Ctrl that is held either way is a
    // modifier as far as the table is concerned. `shown` says what the
    // fingers do.
    Binding {
        section: Section::Clipboard,
        mods: CTRL,
        shown: "Ctrl+.",
        help: "Copy the value of the pixel under the pointer, as read out",
        keys: &[(Char("."), CopyPixelValue)],
    },
    Binding {
        section: Section::Clipboard,
        mods: CTRL,
        shown: "Ctrl+Shift+.",
        help: "Copy the coordinate of the pixel under the pointer, as x,y",
        keys: &[(Char(">"), CopyPixelCoordinate)],
    },
    Binding {
        section: Section::Clipboard,
        mods: CTRL,
        shown: "Ctrl+V",
        help: "Paste an image, saved among your pictures and shown",
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
    // The three that work the histogram's plot, under the key that opens it.
    Binding {
        section: Section::Interface,
        mods: PLAIN,
        shown: "j",
        help: "Toggle the luminance plane on the histogram",
        keys: &[(Char("j"), ToggleLuma), (Char("J"), ToggleLuma)],
    },
    Binding {
        section: Section::Interface,
        mods: PLAIN,
        shown: "k",
        help: "Toggle the color planes on the histogram",
        keys: &[(Char("k"), TogglePlanes), (Char("K"), TogglePlanes)],
    },
    Binding {
        section: Section::Interface,
        mods: PLAIN,
        shown: "l",
        help: "Toggle a logarithmic count axis on the histogram",
        keys: &[(Char("l"), ToggleLogCounts), (Char("L"), ToggleLogCounts)],
    },
    // The same key as the two copies above, with nothing held: what it
    // switches is what they take away with them.
    Binding {
        section: Section::Interface,
        mods: PLAIN,
        shown: ".",
        help: "Cycle the pixel readout: hex, decimal, mapped",
        keys: &[(Char("."), CyclePixelFormat)],
    },
    Binding {
        section: Section::Interface,
        mods: PLAIN,
        shown: "q, Esc",
        help: "Quit; Esc closes a popup or message, or shows the interface",
        keys: &[
            (Char("q"), Quit),
            (Char("Q"), Quit),
            (Named(NamedKey::Escape), Dismiss),
        ],
    },
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "d, f",
        help: "Exposure down / up, a quarter stop",
        keys: &[
            (Char("d"), Exposure(-histogram::EV_STEP)),
            (Char("D"), Exposure(-histogram::EV_STEP)),
            (Char("f"), Exposure(histogram::EV_STEP)),
            (Char("F"), Exposure(histogram::EV_STEP)),
        ],
    },
    // One case each: the capitals are the width of the window, below.
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "a, s",
        help: "Slide the window down / up",
        keys: &[
            (Char("a"), ShiftWindow(-WINDOW_STEP)),
            (Char("s"), ShiftWindow(WINDOW_STEP)),
        ],
    },
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "A, S",
        help: "Narrow / widen the window",
        keys: &[
            (Char("A"), Contrast(NARROWER)),
            (Char("S"), Contrast(WIDER)),
        ],
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
        help: "Cycle tone mapping: none, reinhard, neutral",
        keys: &[(Char("t"), CycleToneMap), (Char("T"), CycleToneMap)],
    },
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "o",
        help: "Toggle HDR output, where the monitor is in HDR mode",
        keys: &[(Char("o"), ToggleHdr), (Char("O"), ToggleHdr)],
    },
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "r",
        help: "Cycle false color for single-channel images",
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

/// Says on the terminal that something could not be done, with the whole
/// chain of why. What the window says about the same failure is one line —
/// see [`App::toast`] — since a message at the foot of a picture is read at a
/// glance and a cause worth following is worth following at leisure.
fn report(error: &anyhow::Error) {
    eprintln!("gamut: {}", crate::escape_controls(&format!("{error:#}")));
}

/// The one line about it that goes in the window: the failure itself, without
/// the chain under it.
fn briefly(error: &anyhow::Error) -> String {
    crate::escape_controls(&error.to_string())
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
            // Escape's own: it takes things off, topmost first, and only
            // leaves when there is nothing left to take off. A message about
            // a copy does not stand between `q` and quitting — a copy is
            // often followed straight away by `q` — but Escape is the key
            // that puts things away, so it clears the message first.
            Dismiss => {
                if self.panels.menu.take().is_some() {
                    self.update_hover();
                    return Effect::Redraw;
                }
                // The interface is the largest thing that can be put away,
                // and the message raised when it went says this is the key
                // that brings it back — so it comes back before the message
                // is taken off, and the message goes with it. Anything else
                // would make the window disagree with what it had just said.
                if !self.panels.show_ui {
                    self.toasts.dismiss();
                    let effect = self.perform(ToggleInterface);
                    self.update_hover();
                    return effect;
                }
                if self.toasts.dismiss() {
                    self.update_hover();
                    return Effect::Redraw;
                }
                return Effect::Quit;
            }
            ZoomIn => self.animate(|view, image, viewport| view.zoom_in(image, viewport)),
            ZoomOut => self.animate(|view, image, viewport| view.zoom_out(image, viewport)),
            ZoomTo(scale) => {
                self.animate(|view, image, viewport| view.set_zoom(scale, image, viewport));
            }
            Pan(direction, step) => {
                let sign = direction.sign();
                match step.pixels() {
                    // The single pixel is for lining a view up exactly, and
                    // a step of one pixel has nothing to animate: it lands.
                    Some(by) if step == PanStep::Fine => {
                        self.view
                            .pan_by(sign[0] * by, sign[1] * by, image, viewport);
                    }
                    Some(by) => self.animate(|view, image, viewport| {
                        view.pan_by(sign[0] * by, sign[1] * by, image, viewport);
                    }),
                    None => self.animate(|view, image, viewport| {
                        view.pan_to_edge(sign, image, viewport);
                    }),
                }
            }
            ToggleFit => self.animate(|view, _, _| view.toggle_fit()),
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
                // The first time it goes, say how to get it back. With the
                // bars gone there is nothing left on screen that could say
                // it, and a window that has stopped answering the pointer
                // anywhere looks broken rather than tidy. Once only: after
                // that the reader knows, and a message every time would be
                // in the way of the thing they asked to see.
                if !self.panels.show_ui && !self.said_how_to_restore {
                    self.said_how_to_restore = true;
                    self.toast(RESTORE, Level::Message);
                }
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
            ToggleLuma => self.press(Widget::Luma),
            TogglePlanes => self.press(Widget::Planes),
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
            // A copy takes the selection and leaves the picture exactly as it
            // was, so the message at the foot of the window is the only sign
            // it happened at all — and the only way to tell a copy that
            // worked from a key that was never read.
            CopyName => {
                let name = self.shown_name();
                self.copy(name.as_bytes(), clipboard::TEXT, "Copied file name.");
            }
            CopyPath => {
                let path = self.shown_path();
                self.copy(
                    path.to_string_lossy().as_bytes(),
                    clipboard::TEXT,
                    "Copied file path.",
                );
            }
            CopyUri => {
                let list = clipboard::uri_list(&self.shown_path());
                self.copy(list.as_bytes(), clipboard::URI_LIST, "Copied file URI.");
            }
            // The one copy with nothing to say yet: the picture is walked and
            // encoded on a thread of its own, and what it did is said when it
            // comes back — see `App::poll_copies`.
            CopyImage => {
                self.copy_image();
                return Effect::Nothing;
            }
            // Whether or not the panel is open: what it says is a fact about
            // the file, and asking for it should not mean first arranging to
            // look at it.
            CopyMetadata => self.copy_facts(Copyable::All),
            // Nothing to draw yet either: the picture is being written and
            // then read, and what is on screen stays until it arrives.
            Paste => {
                self.paste();
                return Effect::Nothing;
            }
            // The bar alone changes, but that is a frame all the same.
            CyclePixelFormat => {
                self.panels.pixel_format = self.panels.pixel_format.next();
            }
            // As the copies above, with the one case a pixel copy has and the
            // others do not: the pointer nowhere near a pixel, which is worth
            // saying because the key looks as though it did nothing.
            CopyPixelValue => self.copy_pixel(false),
            CopyPixelCoordinate => self.copy_pixel(true),
            ResetDisplay => {
                let headroom = self.headroom();
                return self.adjust(move |current, _| {
                    current
                        .display
                        .reset(&current.stats, &current.image, headroom);
                    true
                });
            }
            ToggleHdr => return Effect::redraw_if(self.toggle_hdr()),
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

    /// The name of the file on screen, with nothing of the directory it sits
    /// in — what the top bar shows, at whatever length it actually is.
    ///
    /// Taken from the path as it was given rather than from the absolute one:
    /// the two end in the same name, and there is nothing here that needs the
    /// working directory. The whole path stands in for one that ends in no
    /// name at all, which a file on the list never does.
    fn shown_name(&self) -> String {
        let path = self.files.shown_path();
        match path.file_name() {
            Some(name) => name.to_string_lossy().into_owned(),
            None => path.to_string_lossy().into_owned(),
        }
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
        // How it turned out, for the message the window shows about it. Sent
        // rather than said here: this thread has no business touching the
        // interface, and the loop picks the outcome up on the same cadence it
        // looks at the file and the clipboard on.
        let outcome = self.copied.0.clone();

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
                Err(error) => {
                    report(&error);
                    let _ = outcome.send(Err(briefly(&error)));
                    return;
                }
            };
            timing::encoded_png(width, height, png.len(), encoded.elapsed());

            // Something has been copied since this was asked for, and taking
            // the selection now would put back a picture the user has already
            // moved on from.
            if copies.load(Ordering::Relaxed) != asked {
                return;
            }
            match clipboard::copy(&png, clipboard::PNG) {
                Ok(()) => {
                    let _ = outcome.send(Ok(()));
                }
                Err(error) => {
                    report(&error);
                    let _ = outcome.send(Err(briefly(&error)));
                }
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
        // Named by how much of the table was asked for, since a click on a
        // field and a click on the whole panel are the same gesture on
        // different buttons and the message is what parts them.
        let said = match copies {
            Copyable::All => "Copied file information.",
            Copyable::Section(_) => "Copied section.",
            Copyable::Fact(_) => "Copied field.",
        };
        self.copy(rows.as_bytes(), clipboard::TEXT, said);
    }

    /// Puts `content` on the clipboard under `mime_type`.
    ///
    /// Done in line, unlike the picture: there is nothing here to prepare, and
    /// a copy the user follows straight away with `q` should be on the
    /// clipboard before the window goes.
    fn copy(&mut self, content: &[u8], mime_type: &str, said: &str) {
        self.claim_copy();
        match clipboard::copy(content, mime_type) {
            Ok(()) => self.toast(said, Level::Message),
            Err(error) => {
                report(&error);
                self.toast(briefly(&error), Level::Error);
            }
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
        // not traveled since is a click on it, and this is where it is
        // answered: the release ends the drag it also started, and only one
        // of the two gestures can have been meant.
        // The message the copy raises is a change on screen, and this is the
        // one path into `copy_facts` that is not a key: the release goes on
        // to end a drag, which usually owes nothing.
        let mut changed = false;
        if state == ElementState::Released
            && let Some((copies, _)) = self.pointer.copying.take()
        {
            self.copy_facts(copies);
            changed = true;
        }

        if state == ElementState::Pressed {
            // Wherever this one is going, it is not the press that went down
            // on the info panel, so there is nothing left to copy.
            self.pointer.copying = None;
            // What the button does is a better answer than the label saying
            // what it is, and the label would be standing over whatever the
            // press opened.
            self.tooltips.dismiss();
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
                    // The words at the end of the bottom bar are not a
                    // widget — where they begin takes the fonts to say — so
                    // they are asked for here, after every widget has had
                    // its chance and before the press is spent on the panel
                    // they are set on.
                    None if self.state_hover() => {
                        self.press(Widget::Histogram);
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
        changed
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
        let (hover, info, state) = if self.menu_has_pointer(hit) {
            (None, None, false)
        } else {
            let hover = hit.and_then(Hit::widget);
            // The bottom bar's words only when nothing else has claimed the
            // pointer, which is the order a press is answered in: a window
            // too narrow to keep them off the button at the head of the
            // readout must not light both.
            let state = hover.is_none() && self.state_hover();
            (hover, self.info_copyable(), state)
        };
        let changed = hover != self.panels.hover
            || info != self.panels.info_hover
            || state != self.panels.state_hover;
        self.panels.hover = hover;
        self.panels.info_hover = info;
        self.panels.state_hover = state;

        // The tooltip follows the same answer, and is asked on every call
        // rather than only when the highlight moves: the pointer being still
        // is what opens one, so motion within a button matters to it even
        // though nothing on screen changed.
        //
        // The top bar's own words name themselves too, and are asked for
        // separately — where a run of words ends takes the fonts to say.
        let tip = match hover {
            _ if self.menu_has_pointer(hit) => None,
            Some(widget) => Some(Tip::Widget(widget)),
            // The bottom bar's own words, which have just been measured for
            // the wash that comes up under them, and then the top bar's.
            None if state => Some(Tip::State),
            None => self.bar_tip(),
        };
        let named = self.tooltips.point(Instant::now(), tip);
        changed || named
    }

    /// What to say about the thing the pointer has rested on, if anything.
    ///
    /// The first line names the thing — several lines where it is several
    /// things — and the lines under them are what to press instead. Both come
    /// out of the key table wherever a key does the same job, so that a
    /// tooltip and `--help` cannot disagree about a binding — see [`names`]
    /// and [`hint`].
    pub(super) fn tooltip(&self) -> Option<ui::Tooltip> {
        let at = self.tooltips.showing()?;
        // A toggle the window has no room for is drawn dead and refuses the
        // press, so the label says why rather than naming the panel and the
        // key beside it — neither of which is going to happen.
        if let Some(said) = ui::tooltip::disabled(at, self.room()) {
            return Some(ui::Tooltip {
                at,
                title: vec![said.to_string()],
                hints: Vec::new(),
            });
        }
        let (title, hints) = match at {
            // The name in the bar is cut to the room the bar has, and is only
            // the last part of the path even when it is not. The tooltip is
            // the path in full — which is also exactly what the key beside it
            // copies.
            Tip::Name => (
                vec![self.shown_path().display().to_string()],
                Vec::from_iter(hint(CopyPath)),
            ),
            // The count says which of the list is on screen; the keys are how
            // to reach the rest of it.
            Tip::Counter => (
                vec![format!(
                    "File {} of {}",
                    self.files.index() + 1,
                    self.files.len()
                )],
                [NextFile, PreviousFile]
                    .into_iter()
                    .filter_map(hint)
                    .collect(),
            ),
            // The dot at the head of the pixel readout: what the key does to
            // it, and under that the two copies that take what it is showing
            // away with them — neither of which has a button anywhere.
            Tip::Widget(Widget::PixelFormat) => (
                vec![names(at, &self.panels)?],
                [CopyPixelValue, CopyPixelCoordinate]
                    .into_iter()
                    .filter_map(hint)
                    .collect(),
            ),
            // The button that hides the interface: what a plain press does,
            // and under it the key for the press that closes the floating
            // panels with it — the one thing on the button the pointer
            // cannot discover by resting on it.
            Tip::Widget(Widget::Maximize) => (
                vec![names(at, &self.panels)?],
                Vec::from_iter(hint(ToggleInterfaceAndPanels)),
            ),
            // The exposure's two steps: which way this one goes and what it
            // is worth, and under it the keys that take the same step.
            Tip::Widget(Widget::ExposureDown | Widget::ExposureUp) => (
                vec![names(at, &self.panels)?],
                [Exposure(-histogram::EV_STEP)]
                    .into_iter()
                    .filter_map(hint)
                    .collect(),
            ),
            // The words at the end of the bottom bar: what the bar says in
            // the room it has, said out in full — a line for each of the
            // things in force — and under them that the panel which sets all
            // of it is a press away. The key comes from the table as every
            // other tooltip's does; the words are about the press, which no
            // key can describe.
            Tip::State => {
                let said = ui::explain_state(self.current.as_ref()?, self.headroom());
                if said.is_empty() {
                    return None;
                }
                let binding = binding_for(ToggleHistogram)?;
                // What the press is for while the panel is closed; what it
                // actually does while the panel is open, the press being the
                // toggle the key is.
                let does = match self.panels.show_histogram {
                    true => "close",
                    false => "open",
                };
                (
                    said,
                    vec![format!("Click to {does} the histogram ({})", binding.shown)],
                )
            }
            _ => (vec![names(at, &self.panels)?], Vec::new()),
        };
        Some(ui::Tooltip { at, title, hints })
    }

    /// Acts on a press. The keys that stand in for the toggles come through
    /// here too, so that a key and a click cannot drift apart.
    fn press(&mut self, widget: Widget) {
        match widget {
            // The key's own action, so that a press and a keystroke cannot
            // come to mean different things. Nothing is drawn differently
            // yet: the file is only being asked for, and what is on screen
            // stays until it arrives.
            Widget::Previous => self.step(false),
            Widget::Next => self.step(true),
            Widget::Minimap => self.panels.show_minimap = !self.panels.show_minimap,
            // Refused where the window has no room for the panel, the way the
            // surface switch refuses where there is no headroom to switch to:
            // the toggle is drawn dead, and a press on a dead control that
            // quietly set something no one could see would be worse than one
            // that does nothing.
            Widget::Histogram => {
                if self.room().histogram {
                    self.panels.show_histogram = !self.panels.show_histogram;
                }
            }
            Widget::Grid => self.panels.show_grid = !self.panels.show_grid,
            // The keys' own actions, and which of the two by the modifier
            // the keys are told apart by: a plain press hides the bars, and
            // Shift closes the panels floating over the picture on the way,
            // exactly as `` ` `` and `~` do.
            Widget::Maximize => {
                let action = match self.pointer.modifiers.shift_key() {
                    true => ToggleInterfaceAndPanels,
                    false => ToggleInterface,
                };
                let _ = self.perform(action);
            }
            Widget::Info => {
                if self.room().info {
                    self.panels.show_info = !self.panels.show_info;
                }
            }
            // Only ever opens one: the press that closes a menu is answered
            // by the menu itself, before the widgets underneath are asked.
            Widget::Zoom => self.open_menu(Menu::Zoom),
            Widget::PixelFormat => self.open_menu(Menu::PixelFormat),
            Widget::Copy => self.open_menu(Menu::Copy),
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
            // The keys' own action, as with the reset, and the keys' own step
            // with it: a press here and `d` or `f` are worth the same quarter
            // of a stop, so nothing but where it is pressed tells them apart.
            Widget::ExposureDown => {
                let _ = self.perform(Exposure(-histogram::EV_STEP));
            }
            Widget::ExposureUp => {
                let _ = self.perform(Exposure(histogram::EV_STEP));
            }
            // A window named outright rather than the next one along: the
            // image's own where the row offers that, which is the one of the
            // four that only the image can answer.
            Widget::Window(index) => {
                if let Some(current) = self.current.as_mut()
                    && let Some((_, window)) = histogram::WINDOWS.get(index)
                {
                    let window = window.unwrap_or_else(|| AutoWindow::default_for(&current.image));
                    current.display.set_auto(window, &current.stats);
                }
            }
            Widget::Curve(index) => {
                if let Some(current) = self.current.as_mut()
                    && let Some(curve) = ToneMap::ALL.get(index)
                {
                    current.display.tone_map = *curve;
                }
            }
            // Whatever the tooltip said the button does, done: these four
            // stand for a key exactly, and asking the same table that names
            // them is what keeps the two from ever meaning different things.
            Widget::WindowDown | Widget::WindowUp | Widget::WindowNarrow | Widget::WindowWiden => {
                if let Some(action) = action_of(Tip::Widget(widget), &self.panels) {
                    let _ = self.perform(action);
                }
            }
            Widget::Cell(index) => {
                if let Some(menu) = self.panels.menu.take() {
                    // A cell of the menu of copies runs the key's action, as
                    // the reset and the paste buttons do: what it asks for is
                    // done rather than set, and there is nothing about the
                    // view for `choose` to settle.
                    if let Some(what) = menu.copy_at(index) {
                        let _ = self.perform(copy_action(what));
                        return;
                    }
                    // The format is lifted out and put back so that the
                    // closure borrows nothing of `self`: a zoom chosen here
                    // is a move, and a format is settled on the spot.
                    let mut format = self.panels.pixel_format;
                    self.animate(|view, image, viewport| {
                        menu.choose(index, view, &mut format, image, viewport);
                    });
                    self.panels.pixel_format = format;
                }
            }
            // As with the reset: the key's action, so that the button and the
            // key cannot come to mean different things.
            Widget::Output => {
                let _ = self.toggle_hdr();
            }
            // The cross on the message at the foot of the window. The caller
            // redraws and re-tests the pointer, which is what takes the
            // highlight off a button that is no longer there.
            Widget::Dismiss => {
                self.toasts.dismiss();
            }
        }
    }

    /// Opens `menu`, if there is anything for it to be about and room to draw
    /// it. A window with no room for the panel gets no menu rather than a
    /// state nothing on screen accounts for.
    fn open_menu(&mut self, menu: Menu) {
        if self.current.is_some()
            && self
                .chrome()
                .popup(menu, self.grid_spacing().as_deref())
                .is_some()
        {
            self.panels.menu = Some(menu);
        }
    }

    /// Puts the pixel under the pointer on the clipboard: its `coordinate`,
    /// or else its value written exactly as the bar is writing it, so that
    /// what is copied is what was read.
    ///
    /// A pixel and not a picture, so this is done in line like the other
    /// small copies. Nothing to copy is not a failure — a key was pressed
    /// with the pointer off the image, or over a panel covering it — but it
    /// is the one thing worth saying, since a copy that worked shows nothing
    /// either.
    fn copy_pixel(&mut self, coordinate: bool) {
        let Some(text) = self.pixel_text(coordinate) else {
            eprintln!("gamut: no pixel under the pointer to copy");
            self.toast("No pixel under the pointer.", Level::Warning);
            return;
        };
        let said = match coordinate {
            true => "Copied pixel coordinate.",
            false => "Copied pixel value.",
        };
        self.copy(text.as_bytes(), clipboard::TEXT, said);
    }

    /// What such a copy says, or `None` when the pointer is not on a pixel.
    fn pixel_text(&self, coordinate: bool) -> Option<String> {
        let at = self.pointer_pixel()?;
        if coordinate {
            return Some(ui::pixel::copied_coordinate(at));
        }
        let current = self.current.as_ref()?;
        let sample = current.image.sample(at[0], at[1])?;
        let mapped = current.display.map(&sample, self.headroom());
        Some(ui::pixel::value(
            &current.image,
            &sample,
            &mapped,
            self.panels.pixel_format,
        ))
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
            Some(
                Hit::Cell(_)
                | Hit::Menu
                | Hit::Toast(_)
                | Hit::Minimap
                | Hit::Histogram(_)
                | Hit::Chrome(_),
            ) => {
                return false;
            }
            // The picture, or a pointer that has not yet said where it is.
            Some(Hit::Image) | None => {}
        }

        // A wheel's notch is a step asked for by name, and is animated as a
        // key's would be; a trackpad's scroll is the hand on the view, as a
        // drag is, and goes where the fingers put it.
        let (steps, notched) = match delta {
            MouseScrollDelta::LineDelta(_, lines) => (lines, true),
            MouseScrollDelta::PixelDelta(pixels) => {
                (pixels.y as f32 / WHEEL_PIXELS_PER_STEP, false)
            }
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
        if notched {
            self.animate(|view, image, viewport| {
                view.zoom_steps_at(steps, anchor, image, viewport);
            });
        } else {
            self.view
                .zoom_steps_at(steps, anchor, self.image_size(), viewport);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `Panels` with the menu open, for naming its cells.
    fn panels(menu: Option<Menu>) -> Panels {
        Panels {
            show_ui: true,
            show_histogram: false,
            show_info: false,
            info_scroll: 0.0,
            show_luma: true,
            show_planes: true,
            log_counts: false,
            show_minimap: true,
            show_grid: false,
            paste: true,
            pixel_format: ui::PixelFormat::default(),
            hover: None,
            info_hover: None,
            state_hover: false,
            menu,
        }
    }

    /// Every button in the chrome is named by the key that does the same job,
    /// in that key's own words: there is one table, so a tooltip and `--help`
    /// have nowhere to disagree about a binding.
    #[test]
    fn a_button_is_named_by_the_key_that_does_the_same_job() {
        let panels = panels(None);
        let named = |widget| names(Tip::Widget(widget), &panels);

        assert_eq!(
            named(Widget::Previous).as_deref(),
            Some("Previous file ([, Page Up)")
        );
        assert_eq!(
            named(Widget::Next).as_deref(),
            Some("Next file (], Page Down)")
        );
        assert_eq!(
            named(Widget::Minimap).as_deref(),
            Some("Toggle the minimap (m)")
        );
        assert_eq!(
            named(Widget::Histogram).as_deref(),
            Some("Toggle the histogram (h)")
        );
        assert_eq!(
            named(Widget::Grid).as_deref(),
            Some("Toggle the grid over the image (g)")
        );
        assert_eq!(
            named(Widget::Output).as_deref(),
            Some("Toggle HDR output, where the monitor is in HDR mode (o)")
        );
        // The button in the corner is named by the plain press it makes; the
        // press with Shift is the line under it — see `App::tooltip`.
        assert_eq!(
            named(Widget::Maximize).as_deref(),
            Some("Toggle the interface panels (`)")
        );

        // The one button no key reaches names itself, and has no key after
        // it to name.
        let zoom = named(Widget::Zoom).expect("the readout names itself");
        assert!(!zoom.contains('('), "{zoom}");
    }

    /// Nothing in the chrome is left unnamed: a button with no tooltip is one
    /// the pointer rests on for nothing.
    #[test]
    fn every_chrome_button_has_something_to_say() {
        let panels = panels(None);
        for widget in [
            Widget::Previous,
            Widget::Next,
            Widget::Minimap,
            Widget::Copy,
            Widget::Paste,
            Widget::Histogram,
            Widget::Info,
            Widget::Grid,
            Widget::Maximize,
            Widget::Output,
            Widget::Zoom,
            Widget::PixelFormat,
        ] {
            assert!(
                names(Tip::Widget(widget), &panels).is_some(),
                "{widget:?} names itself"
            );
        }
    }

    /// The message raised when the interface goes names keys that really do
    /// bring it back: the table's own word for the one that hid it, and the
    /// Escape that takes things off. A message naming a key that did nothing
    /// would leave the reader with a window they could not get out of.
    #[test]
    fn the_message_about_a_hidden_interface_names_keys_that_restore_it() {
        let binding = binding_for(ToggleInterface).expect("`` ` `` is bound");
        assert!(RESTORE.contains(binding.shown), "{RESTORE}");
        assert_eq!(
            action_for(&Key::Named(NamedKey::Escape), ELSEWHERE, PLAIN),
            Some(Dismiss)
        );
        assert!(RESTORE.contains("Esc"), "{RESTORE}");
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
        let panels = panels(Some(Menu::Zoom));
        let named = |index| names(Tip::Widget(Widget::Cell(index)), &panels);

        assert_eq!(named(0).as_deref(), Some("Zoom to 10% (Shift+4)"));
        assert_eq!(named(3).as_deref(), Some("Zoom to 100% (1, 0)"));
        assert_eq!(named(4).as_deref(), Some("Zoom to 200% (2)"));
        assert_eq!(named(7).as_deref(), Some("Zoom to 1600% (5)"));

        let mut named_cells = 0;
        for index in 0..32 {
            let Some(words) = named(index) else { continue };
            named_cells += 1;
            assert!(words.ends_with(')'), "{words} says what to press");
        }
        assert_eq!(named_cells, 12, "eight zooms, two fits and two filters");
    }

    /// The histogram panel's buttons are named in the panel's own few words
    /// — its labels are read across the plot, so they have a panel's width
    /// and not a window's — and by the key that does the same job.
    #[test]
    fn the_histogram_panels_buttons_are_named_briefly_and_by_their_keys() {
        let panels = panels(None);
        let named = |widget| names(Tip::Widget(widget), &panels);

        assert_eq!(named(Widget::Luma).as_deref(), Some("Luminance plane (j)"));
        assert_eq!(named(Widget::Planes).as_deref(), Some("Color planes (k)"));
        assert_eq!(
            named(Widget::Log).as_deref(),
            Some("Logarithmic counts (l)")
        );
        assert_eq!(
            named(Widget::Reset).as_deref(),
            Some("Reset the display (z)")
        );

        // Every false color on offer, each by the name `--colormap` takes
        // for it, with the key that cycles to it.
        for (index, map) in Colormap::ALL.into_iter().enumerate() {
            let words = named(Widget::Ramp(index)).unwrap_or_else(|| panic!("{map:?} is named"));
            assert!(words.ends_with("(r)"), "{words}");
            assert!(
                map == Colormap::Gray || words.to_lowercase().contains(map.label()),
                "{words} names {map:?}"
            );
        }
        assert_eq!(named(Widget::Ramp(Colormap::ALL.len())), None);

        // The exposure's two steps say what one press of them is worth, in
        // the units the bottom bar reads an exposure out in — the quarter
        // stop `d` and `f` take as well.
        assert_eq!(
            named(Widget::ExposureDown).as_deref(),
            Some("Exposure -\u{00bc} EV")
        );
        assert_eq!(
            named(Widget::ExposureUp).as_deref(),
            Some("Exposure +\u{00bc} EV")
        );

        // The four nudges are the keys with a picture on them, and are named
        // by those keys' own words: one case each, since the capitals are the
        // width of the window and the small letters are where it sits.
        // Each of them says which way it goes, where the key table's own
        // words name the pair the key is bound with.
        for (widget, expected) in [
            (Widget::WindowDown, "Slide the window down (a)"),
            (Widget::WindowUp, "Slide the window up (s)"),
            (Widget::WindowNarrow, "Narrow the window (A)"),
            (Widget::WindowWiden, "Widen the window (S)"),
        ] {
            assert_eq!(named(widget).as_deref(), Some(expected));
        }

        // And the rows that set a state name the state, with the key that
        // steps through the row after it.
        for (index, window) in histogram::WINDOWS.iter().enumerate() {
            let words = named(Widget::Window(index)).unwrap_or_else(|| panic!("{window:?}"));
            assert!(words.ends_with("(e)"), "{words}");
        }
        assert_eq!(named(Widget::Window(histogram::WINDOWS.len())), None);
        for (index, curve) in ToneMap::ALL.into_iter().enumerate() {
            let words = named(Widget::Curve(index)).unwrap_or_else(|| panic!("{curve:?}"));
            assert!(words.ends_with("(t)"), "{words}");
            assert!(
                curve == ToneMap::None || words.to_lowercase().contains(curve.label()),
                "{words} names {curve:?}"
            );
        }
        assert_eq!(named(Widget::Curve(ToneMap::ALL.len())), None);

        // Short enough to be read where they are drawn: beside a toggle, on
        // a panel one panel wide.
        for widget in [
            Widget::Luma,
            Widget::Planes,
            Widget::Log,
            Widget::Reset,
            Widget::Ramp(1),
            Widget::ExposureDown,
            Widget::WindowNarrow,
            Widget::Window(0),
            Widget::Window(3),
            Widget::Curve(2),
        ] {
            let words = named(widget).expect("named above");
            assert!(words.len() <= 32, "{words} is too long for the panel");
        }
    }

    /// The dot at the head of the pixel readout is named by the key that
    /// steps it on, and the two copies that take what it is showing away are
    /// bound as well: they have no button anywhere, so that label is the only
    /// place either of them is written down.
    #[test]
    fn the_pixel_readout_names_its_key_and_the_copies_that_have_none() {
        let panels = panels(None);
        assert_eq!(
            names(Tip::Widget(Widget::PixelFormat), &panels).as_deref(),
            Some("Cycle the pixel readout: hex, decimal, mapped (.)")
        );

        for action in [CopyPixelValue, CopyPixelCoordinate] {
            let hint = hint(action).unwrap_or_else(|| panic!("{action:?} is bound"));
            assert!(
                hint.ends_with("(Ctrl+.)") || hint.ends_with("(Ctrl+Shift+.)"),
                "{hint}"
            );
        }
    }

    /// A cell of the menu of copies has no words of its own: the key table
    /// already describes each copy in a sentence, and the cell is named by
    /// that sentence and by the key that runs it.
    ///
    /// The button that opens the menu names itself, no one key opening it.
    #[test]
    fn a_copy_cell_is_named_by_the_key_table_and_nothing_else() {
        let panels = panels(Some(Menu::Copy));
        let named = |index| names(Tip::Widget(Widget::Cell(index)), &panels);

        assert_eq!(
            named(0).as_deref(),
            Some("Copy the name of the file on screen, without its path (c)")
        );
        assert_eq!(
            named(1).as_deref(),
            Some("Copy the absolute path of the file on screen (Shift+C)")
        );

        // Every cell of it, and nothing past the end of the menu.
        for index in 0..Copies::ALL.len() {
            let words = named(index).unwrap_or_else(|| panic!("cell {index} is named"));
            assert!(words.starts_with("Copy "), "{words}");
            assert!(words.ends_with(')'), "{words} says what to press");
        }
        assert_eq!(named(Copies::ALL.len()), None);

        // And the button it hangs from says what the menu is of, no one key
        // doing that job.
        let button = names(Tip::Widget(Widget::Copy), &panels).expect("the button names itself");
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
            assert!(binding_for(action).is_some(), "{what:?} is bound");
            assert!(!actions.contains(&action), "{what:?} twice");
            actions.push(action);
        }
    }

    /// A cell of the pixel menu is named by what it answers rather than by
    /// what it is called — the cell is already wearing the name — with the key
    /// that steps through them after it.
    #[test]
    fn a_pixel_format_cell_says_which_question_it_answers() {
        let panels = panels(Some(Menu::PixelFormat));
        let named = |index| names(Tip::Widget(Widget::Cell(index)), &panels);

        for index in 0..ui::PixelFormat::ALL.len() {
            let words = named(index).unwrap_or_else(|| panic!("cell {index} is named"));
            assert!(words.ends_with("(.)"), "{words}");
        }
        assert_eq!(named(ui::PixelFormat::ALL.len()), None);
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
        for action in [CopyPath, NextFile, PreviousFile] {
            let hint = hint(action).unwrap_or_else(|| panic!("{action:?} is bound"));
            assert!(hint.ends_with(')'), "{hint}");
        }
    }

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
        assert_eq!(plain("F"), Some(Exposure(histogram::EV_STEP)));
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
}
