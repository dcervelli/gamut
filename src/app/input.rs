//! What the keyboard and the pointer do.
//!
//! Keys go through one table, [`KEYS`], which is also what `--help` prints:
//! a binding added here is documented by the same edit. Each key names an
//! [`Action`], and [`App::perform`] is the one place an action happens.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};

use super::App;
use crate::clipboard;
use crate::image::display::{AutoWindow, Colormap, Startup, ToneMap};
use crate::image::encode;
use crate::image::region::{Grip, Region, Side};
use crate::loader::Source;
use crate::pasted;
use crate::timing;
use crate::ui::histogram;
use crate::ui::info::Copyable;
use crate::ui::menu::{Copies, ZoomChoice};
use crate::ui::toast::Level;
use crate::ui::tooltip::Hdr;
use crate::ui::{self, Control, Current, Grab, Naming, Room, Selection, Tip};

/// Window pixels moved per arrow-key press. Shift moves one pixel instead,
/// for placing a view exactly, and Ctrl goes as far as the image does.
const PAN_STEP: f32 = 64.0;

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
    /// Ask for a region of the picture — the next drag on it draws one —
    /// or, with one asked for or drawn, take it off. While a region is on
    /// screen the arrows move it, `Space` fits it, and the copy of the
    /// picture is a copy of it: see `App::perform_on_region`.
    ToggleRegion,
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

    /// The same, as the whole pixels a region is moved in.
    fn step(self) -> [i64; 2] {
        let sign = self.sign();
        [sign[0] as i64, sign[1] as i64]
    }

    /// The edge of a region that lies this way, which is the one that grows
    /// when the region is grown this way.
    fn side(self) -> Side {
        match self {
            Direction::Left => Side::Left,
            Direction::Right => Side::Right,
            Direction::Up => Side::Top,
            Direction::Down => Side::Bottom,
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
fn action_of(tip: Tip) -> Option<Action> {
    Some(match tip {
        // The pair at the head of the top bar, which go where the keys beside
        // the count go.
        Tip::Control(Control::Previous) => PreviousFile,
        Tip::Control(Control::Next) => NextFile,
        Tip::Control(Control::Minimap) => ToggleMinimap,
        Tip::Control(Control::Histogram) => ToggleHistogram,
        Tip::Control(Control::Info) => ToggleInfo,
        Tip::Control(Control::Grid) => ToggleGrid,
        // The plain press: Shift on the same button asks for the other key,
        // which the tooltip lists under this one — see `App::tooltip`.
        Tip::Control(Control::Maximize) => ToggleInterface,
        Tip::Control(Control::Output) => ToggleHdr,
        Tip::Control(Control::Paste) => Action::Paste,
        Tip::Control(Control::Region) => ToggleRegion,
        // The dot at the head of the pixel readout, which the key steps
        // through exactly as a press on one of its cells chooses.
        Tip::Control(Control::PixelFormat) => CyclePixelFormat,
        // The histogram panel's own, and the row of false colors a key
        // cycles through.
        Tip::Control(Control::Luma) => ToggleLuma,
        Tip::Control(Control::Planes) => TogglePlanes,
        Tip::Control(Control::Log) => ToggleLogCounts,
        Tip::Control(Control::Reset) => ResetDisplay,
        // Only the swatches that are actually on offer: an index past the
        // end is not a false color, and naming it after the key that cycles
        // them would be naming nothing.
        Tip::Control(Control::Ramp(index)) if index < Colormap::ALL.len() => CycleColormap,
        // The same for the two rows under them: the key steps through the
        // windows and the curves in turn where a button names one outright.
        Tip::Control(Control::Window(index)) if index < histogram::WINDOWS.len() => CycleAutoWindow,
        Tip::Control(Control::Curve(index)) if index < ToneMap::ALL.len() => CycleToneMap,
        // The four nudges beside the window's reading are the keys with a
        // picture on them: the same four amounts, so a press and a keystroke
        // move the window by the same step and are named by the same words.
        Tip::Control(Control::WindowDown) => ShiftWindow(-WINDOW_STEP),
        Tip::Control(Control::WindowUp) => ShiftWindow(WINDOW_STEP),
        Tip::Control(Control::WindowNarrow) => Contrast(NARROWER),
        Tip::Control(Control::WindowWiden) => Contrast(WIDER),
        // A cell of a menu sets one state directly where the key steps
        // through them all: the key is worth naming, the description of the
        // step is not. A numbered cell of the zoom menu is the exception, its
        // key going straight to the same zoom.
        Tip::Control(Control::ZoomTo(ZoomChoice::Scale(scale))) => ZoomTo(scale),
        Tip::Control(Control::ZoomTo(ZoomChoice::Fit(_))) => ToggleFit,
        Tip::Control(Control::ZoomTo(ZoomChoice::Filter(_))) => CycleUpscale,
        Tip::Control(Control::Format(_)) => CyclePixelFormat,
        // The one menu whose items are things done rather than states to be
        // in: the key does exactly what the item does, and its line of the
        // table is what names both.
        Tip::Control(Control::Copies(what)) => copy_action(what),
        // The words at the end of the bottom bar are about four settings at
        // once, so no one key does what they do; what a press on them opens
        // is the panel that sets all four, which the tooltip says outright.
        Tip::Control(_) | Tip::Name | Tip::Counter | Tip::State => return None,
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
fn names(tip: Tip) -> Option<String> {
    let pressed = action_of(tip).and_then(|action| {
        let binding = binding_for(action)?;
        Some((binding.help, shown_for(binding, action)))
    });
    match (ui::tooltip::words(tip), pressed) {
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
        help: "Toggle fit between the whole image and filling the window, or of the region",
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
        help: "Pan by 64 pixels; move a region, or the handle under the pointer, a pixel",
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
        help: "Pan to the far side of the image; grow a region that way a pixel",
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
        help: "Copy the image, or the region while one is selected, as displayed",
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
    Binding {
        section: Section::Interface,
        mods: PLAIN,
        shown: "x",
        help: "Select a region: drag to draw it, with handles to adjust; again, or Esc, removes it",
        keys: &[(Char("x"), ToggleRegion), (Char("X"), ToggleRegion)],
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
        help: "Quit; Esc closes a popup, message or region, or shows the interface",
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

/// The words the application has for the interface, gathered before a
/// frame: enough to compose any tooltip the pointer might rest for, without
/// the interface reaching back into the application to ask.
///
/// A handful of values rather than the application itself, since the frame
/// is drawn from the application's state while this is read.
pub(super) struct Namer {
    /// Whether the content area has room for each floating panel, which is
    /// what makes a toggle dead.
    room: Room,
    /// Whether the surface switch has anything to switch.
    hdr: Hdr,
    /// The absolute path of the file on screen, for the tooltip on its name.
    path: String,
    /// Which file is on screen, out of how many.
    index: usize,
    count: usize,
    /// Whether the histogram is open, for what a press on the bottom bar's
    /// words does.
    show_histogram: bool,
    /// What is being done to the picture, in sentences.
    state: Vec<String>,
}

impl Naming for Namer {
    /// What to say about the thing the pointer has rested on, if anything.
    ///
    /// The first line names the thing — several lines where it is several
    /// things — and the lines under them are what to press instead. Both come
    /// out of the key table wherever a key does the same job, so that a
    /// tooltip and `--help` cannot disagree about a binding — see [`names`]
    /// and [`hint`].
    fn tooltip(&self, at: Tip) -> Option<ui::Tooltip> {
        // A control that is drawn dead — a toggle the window has no room for,
        // the surface switch on a monitor with no room above white — refuses
        // the press, so the label says why rather than naming the thing and
        // the key beside it, neither of which is going to happen.
        if let Some(refused) = ui::tooltip::disabled(at, self.room, self.hdr) {
            return Some(ui::Tooltip {
                title: vec![refused.said.to_string()],
                hints: refused.hint.map(str::to_string).into_iter().collect(),
            });
        }
        let (title, hints) = match at {
            // The name in the bar is cut to the room the bar has, and is only
            // the last part of the path even when it is not. The tooltip is
            // the path in full — which is also exactly what the key beside it
            // copies.
            Tip::Name => (vec![self.path.clone()], Vec::from_iter(hint(CopyPath))),
            // The count says which of the list is on screen; the keys are how
            // to reach the rest of it.
            Tip::Counter => (
                vec![format!("File {} of {}", self.index + 1, self.count)],
                [NextFile, PreviousFile]
                    .into_iter()
                    .filter_map(hint)
                    .collect(),
            ),
            // The dot at the head of the pixel readout: what the key does to
            // it, and under that the two copies that take what it is showing
            // away with them — neither of which has a button anywhere.
            Tip::Control(Control::PixelFormat) => (
                vec![names(at)?],
                [CopyPixelValue, CopyPixelCoordinate]
                    .into_iter()
                    .filter_map(hint)
                    .collect(),
            ),
            // The button that hides the interface: what a plain press does,
            // and under it the key for the press that closes the floating
            // panels with it — the one thing on the button the pointer
            // cannot discover by resting on it.
            Tip::Control(Control::Maximize) => (
                vec![names(at)?],
                Vec::from_iter(hint(ToggleInterfaceAndPanels)),
            ),
            // The exposure's two steps: which way this one goes and what it
            // is worth, and under it the keys that take the same step.
            Tip::Control(Control::ExposureDown | Control::ExposureUp) => (
                vec![names(at)?],
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
                if self.state.is_empty() {
                    return None;
                }
                let binding = binding_for(ToggleHistogram)?;
                // What the press is for while the panel is closed; what it
                // actually does while the panel is open, the press being the
                // toggle the key is.
                let does = match self.show_histogram {
                    true => "close",
                    false => "open",
                };
                (
                    self.state.clone(),
                    vec![format!("Click to {does} the histogram ({})", binding.shown)],
                )
            }
            _ => (vec![names(at)?], Vec::new()),
        };
        Some(ui::Tooltip { title, hints })
    }

    fn shortcut(&self, control: Control) -> Option<String> {
        let action = action_of(Tip::Control(control))?;
        let binding = binding_for(action)?;
        Some(shown_for(binding, action))
    }
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

/// Where the pointer is and what it is over.
#[derive(Default)]
pub(super) struct Pointer {
    pub(super) modifiers: ModifiersState,
    /// Physical window pixels, and the point a wheel zoom works about.
    pub(super) cursor: Option<[f32; 2]>,
    /// Whether it was over the picture with nothing of the interface between
    /// on the last pass, which is what the bar's pixel readout asks. Read the
    /// frame after, which is one frame late only when a panel has appeared or
    /// gone under a still pointer — and that frame is being painted anyway.
    pub(super) over_image: bool,
    /// The handle of the region it was resting on at the last pass, if any
    /// — said the same way, and what the arrows ask before they move the
    /// whole region.
    pub(super) grip: Option<Grip>,
}

/// The hold a drag on the picture has on the region, from the press to the
/// release: what was taken hold of, the region as it was when it was, and
/// where the press was in image pixels. Each frame of the drag remakes the
/// region from these and the hand's place, rather than from the frame
/// before, so nothing accumulates.
pub(super) struct Grabbing {
    grab: Grab,
    origin: Option<Region>,
    from: [f32; 2],
}

impl Grabbing {
    /// What the drag has hold of, for the frame to know which of the
    /// picture's gestures it is reading.
    pub(super) fn grab(&self) -> Grab {
        self.grab
    }
}

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
        // A region on screen takes the keys that move, fit and copy the
        // picture: the picture is what is being looked at, and the region is
        // what is being done to it.
        if let Selection::Shown(region) = self.selection
            && let Some(effect) = self.perform_on_region(region, action)
        {
            return effect;
        }
        let image = self.image_size();
        let viewport = self.viewport();
        match action {
            // An open menu takes the key: dismissing a popup is what Escape
            // is for, and quitting out from under one is not what was being
            // asked for.
            Quit => {
                if self.close_menus() {
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
                if self.close_menus() {
                    return Effect::Redraw;
                }
                // The interface is the largest thing that can be put away,
                // and the message raised when it went says this is the key
                // that brings it back — so it comes back before the message
                // is taken off, and the message goes with it. Anything else
                // would make the window disagree with what it had just said.
                if !self.panels.show_ui {
                    self.toasts.dismiss();
                    return self.perform(ToggleInterface);
                }
                if self.toasts.dismiss() {
                    return Effect::Redraw;
                }
                // The region after the message: a message is about what
                // was just done, and the region is what was being done to
                // — the one that stops being news first goes first.
                if self.selection.is_on() {
                    self.clear_region();
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
                // A menu is part of the interface, and goes with it.
                self.close_menus();
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
            ToggleHistogram => self.press(Control::Histogram),
            ToggleLuma => self.press(Control::Luma),
            TogglePlanes => self.press(Control::Planes),
            ToggleLogCounts => self.press(Control::Log),
            ToggleInfo => self.press(Control::Info),
            ToggleMinimap => self.press(Control::Minimap),
            ToggleGrid => self.press(Control::Grid),
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
                self.copy_image(None);
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
            ToggleRegion => self.press(Control::Region),
        }
        Effect::Redraw
    }

    /// What `action` does to `region`, the region on screen, where it does
    /// something to it: the arrows move it a pixel — or the handle the
    /// pointer rests on, where it rests on one — with Ctrl grow it, `Space`
    /// fits it, and the copy of the picture copies it. `None` for every
    /// other action, which is the picture's as it always was.
    ///
    /// Nothing here is animated: a region moves by a pixel at a time, and a
    /// pixel has nothing to animate.
    fn perform_on_region(&mut self, region: Region, action: Action) -> Option<Effect> {
        let image = self.image_pixels();
        Some(match action {
            Pan(direction, Edge) => {
                self.select(region.grown(direction.side(), 1, image));
                Effect::Redraw
            }
            Pan(direction, Fine | Coarse) => {
                let step = direction.step();
                // A handle that has no edge to move the way the arrow
                // points — an edge's own axis — moves the whole region, as
                // the arrow would with the pointer anywhere else.
                let moved = self
                    .pointer
                    .grip
                    .and_then(|grip| region.nudged(grip, step, image))
                    .unwrap_or_else(|| region.moved_by(step[0], step[1], image));
                self.select(moved);
                Effect::Redraw
            }
            ToggleFit => {
                let fit = self.region_fit;
                self.animate(|view, image, viewport| {
                    view.fit_region(fit, region.as_f32(), image, viewport);
                });
                self.region_fit = fit.other();
                Effect::Redraw
            }
            CopyImage => {
                self.copy_image(Some(region));
                Effect::Nothing
            }
            _ => return None,
        })
    }

    /// The image on screen in whole pixels, which is what a region is
    /// measured in.
    fn image_pixels(&self) -> [u32; 2] {
        self.current.as_ref().map_or([1, 1], |current| {
            [current.image.width, current.image.height]
        })
    }

    /// Puts `region` on screen, and — where its size is not what it was —
    /// writes the size at its middle for a moment.
    fn select(&mut self, region: Region) {
        let before = self.selection.region().map(|region| region.size());
        self.selection = Selection::Shown(region);
        if before != Some(region.size()) {
            self.dimensions_until = Some(Instant::now() + ui::region::LINGER);
        }
    }

    /// Takes the region off, and the mode with it.
    pub(super) fn clear_region(&mut self) {
        self.selection = Selection::Off;
        self.grabbing = None;
        self.dimensions_until = None;
    }

    /// A drag on the picture has taken hold of the region — or of nothing
    /// yet, to draw one — at `at`, in image pixels.
    fn grab(&mut self, grab: Grab, at: [f32; 2]) {
        self.grabbing = Some(Grabbing {
            grab,
            origin: self.selection.region(),
            from: at,
        });
    }

    /// The hand is at `to`, in image pixels: the region is what the hold
    /// makes of that. A new region that has not yet enclosed a pixel — the
    /// hand still off the picture — leaves things as they were.
    fn pull(&mut self, to: [f32; 2]) {
        let Some(Grabbing { grab, origin, from }) = self.grabbing else {
            return;
        };
        let image = self.image_pixels();
        let region = match (grab, origin) {
            (Grab::New, _) => Region::from_corners(from, to, image),
            (Grab::Handle(Grip::Inside), Some(origin)) => {
                let by = |axis: usize| (to[axis] - from[axis]).round() as i64;
                Some(origin.moved_by(by(0), by(1), image))
            }
            (Grab::Handle(grip), Some(origin)) => Some(origin.pulled(grip, to, image)),
            (Grab::Handle(_), None) => None,
        };
        if let Some(region) = region {
            self.select(region);
        }
    }

    /// The button came up. A hold that never drew anything leaves the
    /// region asked for, so the next drag draws it.
    fn release(&mut self) {
        self.grabbing = None;
    }

    /// Closes whatever menu is open. Returns whether there was one: the
    /// press that closes a menu is spent doing exactly that.
    ///
    /// Asked of egui rather than of a flag of our own, the menus being its:
    /// it closes one on Escape itself, and this is what keeps the key that
    /// puts things away from quitting out from under one.
    pub(super) fn close_menus(&mut self) -> bool {
        let Some(gui) = &self.gui else {
            return false;
        };
        if !gui.ctx.any_popup_open() {
            return false;
        }
        egui::Popup::close_all(&gui.ctx);
        true
    }

    /// The words the interface may need this frame, gathered from wherever
    /// the application keeps them.
    pub(super) fn namer(&self) -> Namer {
        Namer {
            room: self.room(),
            hdr: self.hdr_state(),
            path: self.shown_path().display().to_string(),
            index: self.files.index(),
            count: self.files.len(),
            show_histogram: self.panels.show_histogram,
            state: self
                .current
                .as_ref()
                .map(|current| ui::explain_state(current, self.headroom()))
                .unwrap_or_default(),
        }
    }

    /// Acts on what a pass of the interface asked for.
    pub(super) fn act(&mut self, command: ui::Command) {
        match command {
            ui::Command::Press(control) => self.press(control),
            // The image follows the pointer, so the viewport moves the other
            // way. Not animated: the hand is on the view.
            ui::Command::Drag([dx, dy]) => {
                let (image, viewport) = (self.image_size(), self.viewport());
                self.view.pan_by(-dx, -dy, image, viewport);
            }
            ui::Command::Wheel { steps, notched } => self.wheel(steps, notched),
            ui::Command::OverImage(over) => self.pointer.over_image = over,
            ui::Command::Grab { grab, at } => self.grab(grab, at),
            ui::Command::Pull(to) => self.pull(to),
            ui::Command::Release => self.release(),
            ui::Command::OverGrip(grip) => self.pointer.grip = grip,
        }
    }

    /// Zooms about the pointer by `steps` notches of the wheel.
    ///
    /// A wheel's notch is a step asked for by name, and is animated as a
    /// key's would be; a trackpad's scroll is the hand on the view, as a drag
    /// is, and goes where the fingers put it.
    fn wheel(&mut self, steps: f32, notched: bool) {
        // Same reasoning as `handle_key`: Ctrl+wheel and friends belong to the
        // compositor, and acting on them as well would zoom behind its back.
        if self.pointer.chorded() {
            return;
        }
        // A trackpad emits a long tail of all but motionless events at the end
        // of a gesture, which would leave the view drifting after the finger
        // has stopped.
        if !steps.is_finite() || steps.abs() < 1e-3 {
            return;
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

    /// Puts the picture on screen on the clipboard as a PNG — the whole of
    /// it, or `region` where one is selected.
    ///
    /// The image at its own size with the display settings baked in, not a
    /// picture of the window: the zoom, the pan and the panels are how this
    /// is being looked at, and none of them belong to what is being copied.
    /// A region is the same picture cut down, at the size its pixels have in
    /// the file.
    ///
    /// Done on a thread of its own. Walking every pixel takes long enough on
    /// a large image to be felt as the window going quiet, and a viewer that
    /// stops answering the pointer is a viewer that looks broken. The image
    /// is shared rather than copied, and the display state is a handful of
    /// numbers, so handing the work over costs nothing worth measuring.
    fn copy_image(&mut self, region: Option<Region>) {
        let Some(current) = &self.current else {
            return;
        };
        let image = Arc::clone(&current.image);
        let display = current.display.clone();
        let said = match region {
            Some(_) => "Copied region.",
            None => "Copied image.",
        };
        let region = region.unwrap_or_else(|| Region::whole([image.width, image.height]));
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
            let (width, height) = (region.width, region.height);

            let walked = Instant::now();
            let raster = encode::displayed(&image, &display, region);
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
                    let _ = outcome.send(Ok(said));
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

    /// Acts on a press. The keys that stand in for the toggles come through
    /// here too, so that a key and a click cannot drift apart.
    fn press(&mut self, widget: Control) {
        match widget {
            // The key's own action, so that a press and a keystroke cannot
            // come to mean different things. Nothing is drawn differently
            // yet: the file is only being asked for, and what is on screen
            // stays until it arrives.
            Control::Previous => self.step(false),
            Control::Next => self.step(true),
            Control::Minimap => self.panels.show_minimap = !self.panels.show_minimap,
            // Refused where the window has no room for the panel, the way the
            // surface switch refuses where there is no headroom to switch to:
            // the toggle is drawn dead, and a press on a dead control that
            // quietly set something no one could see would be worse than one
            // that does nothing.
            Control::Histogram => {
                if self.room().histogram {
                    self.panels.show_histogram = !self.panels.show_histogram;
                }
            }
            Control::Grid => self.panels.show_grid = !self.panels.show_grid,
            // The keys' own actions, and which of the two by the modifier
            // the keys are told apart by: a plain press hides the bars, and
            // Shift closes the panels floating over the picture on the way,
            // exactly as `` ` `` and `~` do.
            Control::Maximize => {
                let action = match self.pointer.modifiers.shift_key() {
                    true => ToggleInterfaceAndPanels,
                    false => ToggleInterface,
                };
                let _ = self.perform(action);
            }
            Control::Info => {
                if self.room().info {
                    self.panels.show_info = !self.panels.show_info;
                }
            }
            // The three buttons that open a menu: the menu is egui's, and
            // opens itself on the press, so there is nothing here to do.
            Control::Zoom | Control::PixelFormat | Control::Copy => {}
            // The action the key runs, as with the reset below: the button
            // is on screen because the clipboard was holding a picture at the
            // last look, and the paste asks it again rather than trusting
            // that. A selection that has gone in between is answered the way
            // an empty clipboard is.
            Control::Paste => self.paste(),
            // Asks for a region, or takes off the one asked for or drawn.
            // The key's own action goes through here too, so that the
            // button and `x` cannot come to mean different things.
            Control::Region => match self.selection {
                Selection::Off => self.selection = Selection::Armed,
                Selection::Armed | Selection::Shown(_) => self.clear_region(),
            },
            Control::Luma => self.panels.show_luma = !self.panels.show_luma,
            Control::Planes => self.panels.show_planes = !self.panels.show_planes,
            // The plot's own axis rather than anything about the rendering,
            // which is why the reset below leaves it alone: it is how the
            // measurement is being read, not what is being read.
            Control::Log => self.panels.log_counts = !self.panels.log_counts,
            // The action the key runs, rather than a second reading of what
            // "reset" means: two of them would answer differently the first
            // time either was touched, and a button and a key that disagree
            // about one word are worse than either alone.
            Control::Reset => {
                // The caller redraws for every press, so the effect this
                // hands back says nothing the caller does not already know.
                let _ = self.perform(ResetDisplay);
            }
            Control::Ramp(index) => {
                if let Some(current) = self.current.as_mut()
                    && let Some(map) = Colormap::ALL.get(index)
                {
                    current.display.colormap = *map;
                }
            }
            // The keys' own action, as with the reset, and the keys' own step
            // with it: a press here and `d` or `f` are worth the same quarter
            // of a stop, so nothing but where it is pressed tells them apart.
            Control::ExposureDown => {
                let _ = self.perform(Exposure(-histogram::EV_STEP));
            }
            Control::ExposureUp => {
                let _ = self.perform(Exposure(histogram::EV_STEP));
            }
            // A window named outright rather than the next one along: the
            // image's own where the row offers that, which is the one of the
            // four that only the image can answer.
            Control::Window(index) => {
                if let Some(current) = self.current.as_mut()
                    && let Some((_, window)) = histogram::WINDOWS.get(index)
                {
                    let window = window.unwrap_or_else(|| AutoWindow::default_for(&current.image));
                    current.display.set_auto(window, &current.stats);
                }
            }
            Control::Curve(index) => {
                if let Some(current) = self.current.as_mut()
                    && let Some(curve) = ToneMap::ALL.get(index)
                {
                    current.display.tone_map = *curve;
                }
            }
            // Whatever the tooltip said the button does, done: these four
            // stand for a key exactly, and asking the same table that names
            // them is what keeps the two from ever meaning different things.
            Control::WindowDown
            | Control::WindowUp
            | Control::WindowNarrow
            | Control::WindowWiden => {
                if let Some(action) = action_of(Tip::Control(widget)) {
                    let _ = self.perform(action);
                }
            }
            // A cell of the zoom menu: a zoom chosen here is a move.
            Control::ZoomTo(choice) => {
                self.animate(|view, image, viewport| choice.apply(view, image, viewport));
            }
            // A cell of the pixel menu is settled on the spot.
            Control::Format(format) => self.panels.pixel_format = format,
            // An item of the menu of copies runs the key's action, as the
            // reset and the paste buttons do: what it asks for is done rather
            // than set.
            Control::Copies(what) => {
                let _ = self.perform(copy_action(what));
            }
            // A row of the information panel, or the button above it.
            Control::Facts(copies) => self.copy_facts(copies),
            // As with the reset: the key's action, so that the button and the
            // key cannot come to mean different things.
            Control::Output => {
                let _ = self.toggle_hdr();
            }
            // The cross on the message at the foot of the window. The caller
            // redraws and re-tests the pointer, which is what takes the
            // highlight off a button that is no longer there.
            Control::Dismiss => {
                self.toasts.dismiss();
            }
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
            Some("Toggle the grid over the image (g)")
        );
        assert_eq!(
            named(Control::Output).as_deref(),
            Some("Toggle HDR output, where the monitor is in HDR mode (o)")
        );
        // The button in the corner is named by the plain press it makes; the
        // press with Shift is the line under it — see `App::tooltip`.
        assert_eq!(
            named(Control::Maximize).as_deref(),
            Some("Toggle the interface panels (`)")
        );

        // The one button no key reaches names itself, and has no key after
        // it to name.
        let zoom = named(Control::Zoom).expect("the readout names itself");
        assert!(!zoom.contains('('), "{zoom}");
    }

    /// Nothing in the chrome is left unnamed: a button with no tooltip is one
    /// the pointer rests on for nothing.
    #[test]
    fn every_chrome_button_has_something_to_say() {
        for widget in [
            Control::Previous,
            Control::Next,
            Control::Minimap,
            Control::Copy,
            Control::Paste,
            Control::Region,
            Control::Histogram,
            Control::Info,
            Control::Grid,
            Control::Maximize,
            Control::Output,
            Control::Zoom,
            Control::PixelFormat,
        ] {
            assert!(
                names(Tip::Control(widget)).is_some(),
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
        use crate::ui::menu::ZOOM_CHOICES;
        let named = |choice| names(Tip::Control(Control::ZoomTo(choice)));

        assert_eq!(
            named(ZOOM_CHOICES[0]).as_deref(),
            Some("Zoom to 10% (Shift+4)")
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
            Some("Logarithmic counts (l)")
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

        // The exposure's two steps say what one press of them is worth, in
        // the units the bottom bar reads an exposure out in — the quarter
        // stop `d` and `f` take as well.
        assert_eq!(
            named(Control::ExposureDown).as_deref(),
            Some("Exposure -\u{00bc} EV")
        );
        assert_eq!(
            named(Control::ExposureUp).as_deref(),
            Some("Exposure +\u{00bc} EV")
        );

        // The four nudges are the keys with a picture on them, and are named
        // by those keys' own words: one case each, since the capitals are the
        // width of the window and the small letters are where it sits.
        // Each of them says which way it goes, where the key table's own
        // words name the pair the key is bound with.
        for (widget, expected) in [
            (Control::WindowDown, "Slide the window down (a)"),
            (Control::WindowUp, "Slide the window up (s)"),
            (Control::WindowNarrow, "Narrow the window (A)"),
            (Control::WindowWiden, "Widen the window (S)"),
        ] {
            assert_eq!(named(widget).as_deref(), Some(expected));
        }

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
            Control::Reset,
            Control::Ramp(1),
            Control::ExposureDown,
            Control::WindowNarrow,
            Control::Window(0),
            Control::Window(3),
            Control::Curve(2),
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
        assert_eq!(
            names(Tip::Control(Control::PixelFormat)).as_deref(),
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
        let named = |copies| names(Tip::Control(Control::Copies(copies)));

        assert_eq!(
            named(Copies::Name).as_deref(),
            Some("Copy the name of the file on screen, without its path (c)")
        );
        assert_eq!(
            named(Copies::Path).as_deref(),
            Some("Copy the absolute path of the file on screen (Shift+C)")
        );

        // Every item of it.
        for copies in Copies::ALL {
            let words = named(copies).unwrap_or_else(|| panic!("{copies:?} is named"));
            assert!(words.starts_with("Copy "), "{words}");
            assert!(words.ends_with(')'), "{words} says what to press");
        }

        // The menu prints the key beside each item, and it is the key the
        // table binds to the same copy.
        let namer = Namer {
            room: Room {
                histogram: true,
                info: true,
            },
            hdr: Hdr::Available,
            path: String::new(),
            index: 0,
            count: 1,
            show_histogram: false,
            state: Vec::new(),
        };
        assert_eq!(
            namer.shortcut(Control::Copies(Copies::Path)).as_deref(),
            Some("Shift+C")
        );
        assert_eq!(
            namer.shortcut(Control::Copies(Copies::Image)).as_deref(),
            Some("Ctrl+C")
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
