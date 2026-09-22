//! What the keyboard and the pointer do.
//!
//! Keys go through one table, [`KEYS`], which is also what `--help` prints:
//! a binding added here is documented by the same edit. Each key names an
//! [`Action`], and [`App::perform`] is the one place an action happens.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use winit::event::ElementState;
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};

use super::App;
use crate::clipboard;
use crate::image::display::{Colormap, EV_STEP, Startup, ToneMap};
use crate::image::encode;
use crate::image::region::{Region, Side};
use crate::loader::Source;
use crate::openers;
use crate::pasted;
use crate::portal::Pick;
use crate::timing;
use crate::ui::histogram;
use crate::ui::info::Copyable;
use crate::ui::menu::{Copies, ZoomChoice};
use crate::ui::toast::Level;
use crate::ui::tooltip::{Hdr, Reasons};
use crate::ui::{self, Control, Current, Grab, Naming, Selection, Tip};

use super::region::Framing;
use crate::view::Fit;

/// Window pixels moved per arrow-key press. Shift moves one pixel instead,
/// for placing a view exactly, and Ctrl goes as far as the image does.
const PAN_STEP: f32 = 64.0;

/// How many rows of the chooser to ask thumbnails for as it opens, before
/// the popup has said which rows it is showing: more than any window has
/// room for, so that the first frame's rows are all on their way.
const FIRST_ROWS: usize = 32;

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
    /// Pull a region's far side in a pixel that way — Left brings the
    /// right edge in — which is the opposite number of `Ctrl` with an
    /// arrow pushing the near side out. Nothing without a region: there is
    /// nothing else on screen that shrinks.
    ShrinkRegion(Direction),
    CycleFit,
    CycleUpscale,
    NextFile,
    PreviousFile,
    /// Open the file chooser: a popup that lists the session's files, with
    /// a field that narrows them as it is typed in. While it is up its own
    /// keys are read by the popup — see `ui::chooser` — and the same
    /// chord closes it.
    OpenChooser,
    ToggleInterface,
    /// The interface, and the panels floating over the image with it: the
    /// bars come and go as [`Action::ToggleInterface`], and the map,
    /// histogram and information panel are closed on the way past.
    ToggleInterfaceAndPanels,
    /// Open the help popup — every key, what it does and when — and close
    /// it if it is up. The same toggle as the button at the foot of the
    /// right strip.
    ShowHelp,
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
    /// Move the value that comes out black by this fraction of the
    /// window's width: the black handle's key.
    StepBlack(f32),
    /// And the value that comes out white: the white handle's key.
    StepWhite(f32),
    CycleToneMap,
    CycleColormap,
    ResetDisplay,
    /// Paint the pixels the window has taken to black or to white in the
    /// warning colors, for as long as the key is held: it is answered on
    /// the way down and taken back on the way up, like `Space`.
    MarkClipped,
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
    /// Play a stopped animation, or stop a playing one.
    TogglePlay,
    /// One frame on or back through an animation, stopping it there; or
    /// one page on or back through a file that holds several pictures.
    NextFrame,
    PreviousFrame,
    /// Open the rename dialog on the file on screen — see `ui::rename`.
    Rename,
    /// Move the file on screen to the desktop's trash and step on to the
    /// next — see `App::delete_shown`.
    Delete,
    /// Put back the last thing done to a file on disk: the file restored
    /// from the trash, or its old name — see `app::edits`.
    Undo,
    /// Put up the desktop's file dialog for image files, and open what is
    /// chosen in it as a command line naming them would — see
    /// `App::open_named`.
    OpenFiles,
    /// The same dialog for a folder, which stands for the images inside
    /// it as a directory on the command line does.
    OpenFolder,
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
const CTRL_SHIFT: Mods = Mods::CONTROL.union(Mods::SHIFT);

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
/// The first such line that is not a region's own. An action bound on more
/// than one line is either a different action on each — the arrows and
/// their modified forms — or one that does one thing plainly and another
/// with a region up, written on a line under each condition; the plain one
/// answers, since what asks is a button that does the plain thing, and
/// what it does with a region is the region's line to say.
pub(super) fn binding_for(action: Action) -> Option<&'static Binding> {
    let binds = |binding: &&'static Binding| binding.keys.iter().any(|(_, bound)| *bound == action);
    KEYS.iter()
        .find(|binding| binds(binding) && binding.when != Some(When::RegionSelected))
        .or_else(|| KEYS.iter().find(binds))
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

/// How far one press of the keys moves an end of the display window, as a
/// fraction of the window's width. The hand on the histogram's band moves
/// it by no step at all — the handles go where they are put — so this is
/// the keys' alone.
const WINDOW_STEP: f32 = 0.05;

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
        Tip::Control(Control::Help) => ShowHelp,
        Tip::Control(Control::Play) => TogglePlay,
        Tip::Control(Control::StepBack) => PreviousFrame,
        Tip::Control(Control::StepForward) => NextFrame,
        // The dot at the head of the pixel readout, which the key steps
        // through exactly as a press on one of its cells chooses.
        Tip::Control(Control::PixelFormat) => CyclePixelFormat,
        // The histogram panel's own, and the row of false colors a key
        // cycles through.
        Tip::Control(Control::Luma) => ToggleLuma,
        Tip::Control(Control::Planes) => TogglePlanes,
        Tip::Control(Control::Log) => ToggleLogCounts,
        Tip::Control(Control::Marks) => MarkClipped,
        Tip::Control(Control::Reset) => ResetDisplay,
        // Only the swatches that are actually on offer: an index past the
        // end is not a false color, and naming it after the key that cycles
        // them would be naming nothing.
        Tip::Control(Control::Ramp(index)) if index < Colormap::ALL.len() => CycleColormap,
        // The same for the two rows under them: the key steps through the
        // windows and the curves in turn where a button names one outright.
        Tip::Control(Control::Window(index)) if index < histogram::WINDOWS.len() => CycleAutoWindow,
        Tip::Control(Control::Curve(index)) if index < ToneMap::ALL.len() => CycleToneMap,
        // A cell of a menu sets one state directly where the key steps
        // through them all: the key is worth naming, the description of the
        // step is not. A numbered cell of the zoom menu is the exception, its
        // key going straight to the same zoom.
        Tip::Control(Control::ZoomTo(ZoomChoice::Scale(scale))) => ZoomTo(scale),
        Tip::Control(Control::ZoomTo(ZoomChoice::Fit(_))) => CycleFit,
        Tip::Control(Control::ZoomTo(ZoomChoice::Filter(_))) => CycleUpscale,
        Tip::Control(Control::Format(_)) => CyclePixelFormat,
        // The one menu whose items are things done rather than states to be
        // in: the key does exactly what the item does, and its line of the
        // table is what names both.
        Tip::Control(Control::Copies(what)) => copy_action(what),
        // And the two items of the menu of the file that are not copies,
        // named the same way: by the key that does the same thing.
        Tip::Control(Control::Rename) => Rename,
        Tip::Control(Control::Delete) => Delete,
        // The two buttons in the middle of an empty window, by the keys
        // that put up the same dialog.
        Tip::Control(Control::OpenFiles) => OpenFiles,
        Tip::Control(Control::OpenFolder) => OpenFolder,
        // A swatch, a window or a curve past the end of its row is nothing.
        Tip::Control(Control::Ramp(_) | Control::Window(_) | Control::Curve(_)) => return None,
        // What no key reaches, and so names itself or wears its own name:
        // the buttons that open a menu, the items that wear a program's or
        // a file's name, the rows of the information panel, the cross on a
        // message, the timeline, and the dialog's two buttons.
        Tip::Control(
            Control::Copy
            | Control::OpenIn
            | Control::Opener(_)
            | Control::Seek(_)
            | Control::Zoom
            | Control::Dismiss
            | Control::Facts(_)
            | Control::Chooser
            | Control::Choose(_)
            | Control::FileMenu
            | Control::RenameTo
            | Control::CancelRename,
        ) => return None,
        // The words at the end of the bottom bar are about four settings at
        // once, so no one key does what they do; what a press on them opens
        // is the panel that sets all four, which the tooltip says outright.
        // The band under the histogram, its handles and the exposure's
        // slider are dragged, which no key does either: the keys that step
        // the same things come under them as hints — see `App::tooltip`.
        Tip::Name
        | Tip::Counter
        | Tip::State
        | Tip::Timeline
        | Tip::BlackPoint
        | Tip::WhitePoint
        | Tip::Window
        | Tip::Exposure => return None,
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

/// Which heading a binding is listed under, in `--help`, the manual page
/// and the help popup alike.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Zoom,
    Region,
    Files,
    Playback,
    Clipboard,
    Interface,
    Display,
}

impl Section {
    /// Every section, in the order the keys are listed in: the order the
    /// table below keeps, and the one place it is written down.
    pub const ALL: [Section; 7] = [
        Section::Zoom,
        Section::Interface,
        Section::Files,
        Section::Region,
        Section::Clipboard,
        Section::Display,
        Section::Playback,
    ];

    /// What the section is called, as the popup heads it; `--help` sets the
    /// same words in capitals with `KEYS` after them.
    pub fn title(self) -> &'static str {
        match self {
            Section::Zoom => "Zoom and position",
            Section::Region => "Region selection",
            Section::Files => "Files",
            Section::Playback => "Playback",
            Section::Clipboard => "Clipboard",
            Section::Interface => "Interface",
            Section::Display => "Display",
        }
    }
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
    /// When the key does anything at all, for the help popup's third
    /// column, and `None` for a key that always does. A key that does one
    /// thing plainly and another with a region up is two lines, one under
    /// each condition, binding the same keys to the same action: what the
    /// action does is decided when it is performed, and each line says only
    /// what it does then.
    pub when: Option<When>,
    pub keys: &'static [(KeyName, Action)],
}

/// The condition on which a key does anything at all: the one thing about
/// the moment that decides it, so that the popup can say whether it holds
/// right now as well as what it is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum When {
    RegionSelected,
    NoRegion,
    SeveralFiles,
    Animation,
    AnimationOrPages,
    PointerOnPicture,
    PictureOnClipboard,
    HdrMode,
    SingleChannel,
    /// A rename or a deletion has been made this session and not yet
    /// undone.
    Undoable,
}

impl When {
    /// Every condition, for a test to hold them all up against the
    /// application.
    #[cfg(test)]
    pub const ALL: [When; 10] = [
        When::RegionSelected,
        When::NoRegion,
        When::SeveralFiles,
        When::Animation,
        When::AnimationOrPages,
        When::PointerOnPicture,
        When::PictureOnClipboard,
        When::HdrMode,
        When::SingleChannel,
        When::Undoable,
    ];

    /// The condition in a few words, as the popup's column reads it: a
    /// phrase, not a sentence.
    pub fn describe(self) -> &'static str {
        match self {
            When::RegionSelected => "a region selected",
            When::NoRegion => "no region selected",
            When::SeveralFiles => "more than one file",
            When::Animation => "an animation",
            When::AnimationOrPages => "an animation or a paged file",
            When::PointerOnPicture => "the pointer on the picture",
            When::PictureOnClipboard => "a picture on the clipboard",
            When::HdrMode => "the monitor in HDR mode",
            When::SingleChannel => "a single-channel image",
            When::Undoable => "a rename or deletion to undo",
        }
    }
}

/// What holds at the moment, read off the application once: which of the
/// conditions the keys wait on, for the help popup to dim the keys that
/// would do nothing; and everything that makes a control dead, for the
/// tooltip that says why and for the press that is refused. One reading
/// for all three, so that a button drawn dead, its label and its press
/// cannot come to disagree.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Conditions {
    pub region_selected: bool,
    pub several_files: bool,
    pub animation: bool,
    pub pages: bool,
    pub pointer_on_picture: bool,
    pub picture_on_clipboard: bool,
    pub single_channel: bool,
    pub undoable: bool,
    /// Whether the content area has room for each floating panel.
    pub room: ui::Room,
    /// Whether the surface switch has anything to switch, and why not.
    pub hdr: Hdr,
    /// Whether anything out there offers to open the file on screen.
    pub openable: bool,
    /// Whether a false color is on the picture.
    pub false_colored: bool,
    /// Whether the desktop's file dialog is up.
    pub picking: bool,
    /// Whether there is no picture at all.
    pub nothing_open: bool,
}

impl Default for Conditions {
    /// Nothing holds: no room, no surface to switch to, nothing open.
    fn default() -> Self {
        Self {
            region_selected: false,
            several_files: false,
            animation: false,
            pages: false,
            pointer_on_picture: false,
            picture_on_clipboard: false,
            single_channel: false,
            undoable: false,
            room: ui::Room {
                histogram: false,
                info: false,
                help: false,
            },
            hdr: Hdr::Unsupported,
            openable: false,
            false_colored: false,
            picking: false,
            nothing_open: true,
        }
    }
}

impl Conditions {
    /// Nothing dead for any reason, and no key's condition met: a large
    /// window, a monitor in HDR mode, a file something else opens, a
    /// picture up in its own colors, the dialog down and a picture on the
    /// clipboard — which is the one condition that does hold, the paste
    /// button being alive only then.
    #[cfg(test)]
    pub const ALIVE: Conditions = Conditions {
        region_selected: false,
        several_files: false,
        animation: false,
        pages: false,
        pointer_on_picture: false,
        picture_on_clipboard: true,
        single_channel: false,
        undoable: false,
        room: ui::Room {
            histogram: true,
            info: true,
            help: true,
        },
        hdr: Hdr::Available,
        openable: true,
        false_colored: false,
        picking: false,
        nothing_open: false,
    };

    /// Whether `when` holds.
    pub fn met(&self, when: When) -> bool {
        match when {
            When::RegionSelected => self.region_selected,
            When::NoRegion => !self.region_selected,
            When::SeveralFiles => self.several_files,
            When::Animation => self.animation,
            When::AnimationOrPages => self.animation || self.pages,
            When::PointerOnPicture => self.pointer_on_picture,
            When::PictureOnClipboard => self.picture_on_clipboard,
            When::HdrMode => self.hdr == Hdr::Available,
            When::SingleChannel => self.single_channel,
            When::Undoable => self.undoable,
        }
    }

    /// What makes a control dead, in the form the interface's tooltips
    /// read it: the same reading, projected.
    pub fn reasons(&self) -> Reasons {
        Reasons {
            room: self.room,
            hdr: self.hdr,
            openable: self.openable,
            false_colored: self.false_colored,
            picking: self.picking,
            clipboard: self.picture_on_clipboard,
            nothing_open: self.nothing_open,
        }
    }
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
        when: None,
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
        when: None,
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
        when: None,
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
        when: None,
        keys: &[(Char("+"), ZoomIn), (Char("="), ZoomIn)],
    },
    Binding {
        section: Section::Zoom,
        mods: PLAIN,
        shown: "-, _",
        help: "Zoom out",
        when: None,
        keys: &[(Char("-"), ZoomOut), (Char("_"), ZoomOut)],
    },
    Binding {
        section: Section::Zoom,
        mods: PLAIN,
        shown: "Wheel",
        help: "Zoom about the pointer",
        when: None,
        keys: &[],
    },
    Binding {
        section: Section::Zoom,
        mods: PLAIN,
        shown: "Space",
        help: "Fit the whole image, fill the window, then actual size, in turn",
        when: Some(When::NoRegion),
        keys: &[(Named(NamedKey::Space), CycleFit)],
    },
    Binding {
        section: Section::Zoom,
        mods: PLAIN,
        shown: "Space+Drag",
        help: "Zoom to the box dragged out",
        when: None,
        keys: &[],
    },
    Binding {
        section: Section::Zoom,
        mods: PLAIN,
        shown: "p",
        help: "Cycle the filter used above 100%: nearest, bicubic",
        when: None,
        keys: &[(Char("p"), CycleUpscale), (Char("P"), CycleUpscale)],
    },
    Binding {
        section: Section::Zoom,
        mods: PLAIN,
        shown: "Arrows",
        help: "Pan by 64 pixels",
        when: Some(When::NoRegion),
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
        when: None,
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
        when: Some(When::NoRegion),
        keys: &[
            (Named(NamedKey::ArrowLeft), Pan(Left, Edge)),
            (Named(NamedKey::ArrowRight), Pan(Right, Edge)),
            (Named(NamedKey::ArrowUp), Pan(Up, Edge)),
            (Named(NamedKey::ArrowDown), Pan(Down, Edge)),
        ],
    },
    Binding {
        section: Section::Interface,
        mods: PLAIN,
        shown: "`",
        help: "Toggle the interface panels",
        when: None,
        keys: &[(Char("`"), ToggleInterface)],
    },
    Binding {
        section: Section::Interface,
        mods: PLAIN,
        shown: "~",
        help: "Toggle the panels, closing the map, histogram and information",
        when: None,
        keys: &[(Char("~"), ToggleInterfaceAndPanels)],
    },
    Binding {
        section: Section::Interface,
        mods: PLAIN,
        shown: "m",
        help: "Toggle the minimap",
        when: None,
        keys: &[(Char("m"), ToggleMinimap), (Char("M"), ToggleMinimap)],
    },
    Binding {
        section: Section::Interface,
        mods: PLAIN,
        shown: "h",
        help: "Toggle the histogram",
        when: None,
        keys: &[(Char("h"), ToggleHistogram), (Char("H"), ToggleHistogram)],
    },
    Binding {
        section: Section::Interface,
        mods: PLAIN,
        shown: "i",
        help: "Toggle the file information panel",
        when: None,
        keys: &[(Char("i"), ToggleInfo), (Char("I"), ToggleInfo)],
    },
    Binding {
        section: Section::Interface,
        mods: PLAIN,
        shown: "g",
        help: "Toggle the grid over the image",
        when: None,
        keys: &[(Char("g"), ToggleGrid), (Char("G"), ToggleGrid)],
    },
    // The three that work the histogram's plot, under the key that opens it.
    Binding {
        section: Section::Interface,
        mods: PLAIN,
        shown: "j",
        help: "Toggle the luminance plane on the histogram",
        when: None,
        keys: &[(Char("j"), ToggleLuma), (Char("J"), ToggleLuma)],
    },
    Binding {
        section: Section::Interface,
        mods: PLAIN,
        shown: "k",
        help: "Toggle the color planes on the histogram",
        when: None,
        keys: &[(Char("k"), TogglePlanes), (Char("K"), TogglePlanes)],
    },
    Binding {
        section: Section::Interface,
        mods: PLAIN,
        shown: "l",
        help: "Toggle a logarithmic count axis on the histogram",
        when: None,
        keys: &[(Char("l"), ToggleLogCounts), (Char("L"), ToggleLogCounts)],
    },
    // The same key as the two copies above, with nothing held: what it
    // switches is what they take away with them.
    Binding {
        section: Section::Interface,
        mods: PLAIN,
        shown: ".",
        help: "Cycle the pixel readout: hex, decimal, mapped",
        when: None,
        keys: &[(Char("."), CyclePixelFormat)],
    },
    Binding {
        section: Section::Interface,
        mods: PLAIN,
        shown: "?, /",
        help: "Show the keys",
        when: None,
        keys: &[(Char("?"), ShowHelp), (Char("/"), ShowHelp)],
    },
    Binding {
        section: Section::Interface,
        mods: PLAIN,
        shown: "q, Esc",
        help: "Quit; Esc closes a popup, message or region, or shows the interface",
        when: None,
        keys: &[
            (Char("q"), Quit),
            (Char("Q"), Quit),
            (Named(NamedKey::Escape), Dismiss),
        ],
    },
    Binding {
        section: Section::Files,
        mods: PLAIN,
        shown: "], Page Down",
        help: "Next file",
        when: Some(When::SeveralFiles),
        keys: &[(Char("]"), NextFile), (Named(NamedKey::PageDown), NextFile)],
    },
    Binding {
        section: Section::Files,
        mods: PLAIN,
        shown: "[, Page Up",
        help: "Previous file",
        when: Some(When::SeveralFiles),
        keys: &[
            (Char("["), PreviousFile),
            (Named(NamedKey::PageUp), PreviousFile),
        ],
    },
    Binding {
        section: Section::Files,
        mods: CTRL,
        shown: "Ctrl+P",
        help: "Choose a file from the list",
        when: Some(When::SeveralFiles),
        keys: &[(Char("p"), OpenChooser), (Char("P"), OpenChooser)],
    },
    // The desktop's own dialog, for files and for a folder: one case each,
    // the Ctrl that both are held with being the only modifier the table
    // sees, as with the two `C`s of the clipboard section.
    Binding {
        section: Section::Files,
        mods: CTRL,
        shown: "Ctrl+O",
        help: "Open image files chosen in the desktop's file dialog",
        when: None,
        keys: &[(Char("o"), OpenFiles)],
    },
    Binding {
        section: Section::Files,
        mods: CTRL,
        shown: "Ctrl+Shift+O",
        help: "Open a folder chosen in the desktop's file dialog",
        when: None,
        keys: &[(Char("O"), OpenFolder)],
    },
    // What is done to the file itself, under the keys that walk the list:
    // the two that change the disk, and the one that changes it back.
    Binding {
        section: Section::Files,
        mods: PLAIN,
        shown: "F2",
        help: "Rename the file on screen",
        when: None,
        keys: &[(Named(NamedKey::F2), Rename)],
    },
    Binding {
        section: Section::Files,
        mods: PLAIN,
        shown: "Del, \u{232b}",
        help: "Move the file on screen to the trash, and show the next",
        when: None,
        keys: &[
            (Named(NamedKey::Delete), Delete),
            (Named(NamedKey::Backspace), Delete),
        ],
    },
    Binding {
        section: Section::Files,
        mods: CTRL,
        shown: "Ctrl+Z",
        help: "Undo the last rename or deletion",
        when: Some(When::Undoable),
        keys: &[(Char("z"), Undo), (Char("Z"), Undo)],
    },
    // The region's own section: the key that puts one up, and what the
    // keys of the other sections do differently while it is. Each of those
    // is the same chord bound to the same action as its line in its own
    // section — which is the one doubling
    // `no_chord_is_bound_twice_to_different_actions` allows — so the table
    // dispatches once and describes twice.
    Binding {
        section: Section::Region,
        mods: PLAIN,
        shown: "x",
        help: "Select a region: drag to draw it, with handles to adjust",
        when: Some(When::NoRegion),
        keys: &[(Char("x"), ToggleRegion), (Char("X"), ToggleRegion)],
    },
    Binding {
        section: Section::Region,
        mods: PLAIN,
        shown: "x, Esc",
        help: "Remove the region",
        when: Some(When::RegionSelected),
        keys: &[
            (Char("x"), ToggleRegion),
            (Char("X"), ToggleRegion),
            (Named(NamedKey::Escape), Dismiss),
        ],
    },
    Binding {
        section: Section::Region,
        mods: PLAIN,
        shown: "Space",
        help: "Fit the region, fill the window with it, then the whole image, in turn",
        when: Some(When::RegionSelected),
        keys: &[(Named(NamedKey::Space), CycleFit)],
    },
    Binding {
        section: Section::Region,
        mods: PLAIN,
        shown: "Arrows",
        help: "Move the region, or the handle under the pointer, a pixel",
        when: Some(When::RegionSelected),
        keys: &[
            (Named(NamedKey::ArrowLeft), Pan(Left, Coarse)),
            (Named(NamedKey::ArrowRight), Pan(Right, Coarse)),
            (Named(NamedKey::ArrowUp), Pan(Up, Coarse)),
            (Named(NamedKey::ArrowDown), Pan(Down, Coarse)),
        ],
    },
    Binding {
        section: Section::Region,
        mods: CTRL,
        shown: "Ctrl+Arrows",
        help: "Grow the region that way a pixel",
        when: Some(When::RegionSelected),
        keys: &[
            (Named(NamedKey::ArrowLeft), Pan(Left, Edge)),
            (Named(NamedKey::ArrowRight), Pan(Right, Edge)),
            (Named(NamedKey::ArrowUp), Pan(Up, Edge)),
            (Named(NamedKey::ArrowDown), Pan(Down, Edge)),
        ],
    },
    Binding {
        section: Section::Region,
        mods: CTRL_SHIFT,
        shown: "Ctrl+Shift+Arrows",
        help: "Shrink the region that way a pixel, pulling its far side in",
        when: Some(When::RegionSelected),
        keys: &[
            (Named(NamedKey::ArrowLeft), ShrinkRegion(Left)),
            (Named(NamedKey::ArrowRight), ShrinkRegion(Right)),
            (Named(NamedKey::ArrowUp), ShrinkRegion(Up)),
            (Named(NamedKey::ArrowDown), ShrinkRegion(Down)),
        ],
    },
    Binding {
        section: Section::Clipboard,
        mods: PLAIN,
        shown: "c",
        help: "Copy the name of the file on screen, without its path",
        when: None,
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
        when: None,
        keys: &[(Char("C"), CopyPath)],
    },
    Binding {
        section: Section::Clipboard,
        mods: CTRL,
        shown: "Ctrl+Shift+C",
        help: "Copy the file on screen as a URI another program can open",
        when: None,
        keys: &[(Char("C"), CopyUri)],
    },
    Binding {
        section: Section::Clipboard,
        mods: CTRL,
        shown: "Ctrl+C",
        help: "Copy the image as displayed",
        when: Some(When::NoRegion),
        keys: &[(Char("c"), CopyImage)],
    },
    // The region's copy stays beside the image's, rather than in the
    // region's own section: it is a copy first, and where the two lines
    // are read together they say what one chord does either way.
    Binding {
        section: Section::Clipboard,
        mods: CTRL,
        shown: "Ctrl+C",
        help: "Copy the region as displayed",
        when: Some(When::RegionSelected),
        keys: &[(Char("c"), CopyImage)],
    },
    Binding {
        section: Section::Clipboard,
        mods: CTRL,
        shown: "Ctrl+I",
        help: "Copy everything the info panel says about the file",
        when: None,
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
        when: Some(When::PointerOnPicture),
        keys: &[(Char("."), CopyPixelValue)],
    },
    Binding {
        section: Section::Clipboard,
        mods: CTRL,
        shown: "Ctrl+Shift+.",
        help: "Copy the coordinate of the pixel under the pointer, as x,y",
        when: Some(When::PointerOnPicture),
        keys: &[(Char(">"), CopyPixelCoordinate)],
    },
    Binding {
        section: Section::Clipboard,
        mods: CTRL,
        shown: "Ctrl+V",
        help: "Paste an image, saved among your pictures and shown",
        when: Some(When::PictureOnClipboard),
        keys: &[(Char("v"), Paste), (Char("V"), Paste)],
    },
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "d, f",
        help: "Exposure down / up, a quarter stop",
        when: None,
        keys: &[
            (Char("d"), Exposure(-EV_STEP)),
            (Char("D"), Exposure(-EV_STEP)),
            (Char("f"), Exposure(EV_STEP)),
            (Char("F"), Exposure(EV_STEP)),
        ],
    },
    // One case each: the capitals are the other handle, below.
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "a, s",
        help: "Black point down / up",
        when: None,
        keys: &[
            (Char("a"), StepBlack(-WINDOW_STEP)),
            (Char("s"), StepBlack(WINDOW_STEP)),
        ],
    },
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "A, S",
        help: "White point down / up",
        when: None,
        keys: &[
            (Char("A"), StepWhite(-WINDOW_STEP)),
            (Char("S"), StepWhite(WINDOW_STEP)),
        ],
    },
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "e",
        help: "Cycle the window rule: stored, full, trimmed",
        when: None,
        keys: &[(Char("e"), CycleAutoWindow), (Char("E"), CycleAutoWindow)],
    },
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "t",
        help: "Toggle the curve on the highlights: clip, or roll off",
        when: None,
        keys: &[(Char("t"), CycleToneMap), (Char("T"), CycleToneMap)],
    },
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "w",
        help: "Toggle the marks on the clipped pixels: red at white, blue at black",
        when: None,
        keys: &[(Char("w"), MarkClipped), (Char("W"), MarkClipped)],
    },
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "o",
        help: "Toggle HDR output, where the monitor is in HDR mode",
        when: Some(When::HdrMode),
        keys: &[(Char("o"), ToggleHdr), (Char("O"), ToggleHdr)],
    },
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "r",
        help: "Cycle false color for single-channel images",
        when: Some(When::SingleChannel),
        keys: &[(Char("r"), CycleColormap), (Char("R"), CycleColormap)],
    },
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "z",
        help: "Reset the window, exposure and tone map",
        when: None,
        keys: &[(Char("z"), ResetDisplay), (Char("Z"), ResetDisplay)],
    },
    Binding {
        section: Section::Playback,
        mods: PLAIN,
        shown: "Enter",
        help: "Play or pause an animation",
        when: Some(When::Animation),
        keys: &[(Named(NamedKey::Enter), TogglePlay)],
    },
    Binding {
        section: Section::Playback,
        mods: PLAIN,
        shown: "n",
        help: "Next frame of an animation, or page of a file that holds several",
        when: Some(When::AnimationOrPages),
        keys: &[(Char("n"), NextFrame)],
    },
    Binding {
        section: Section::Playback,
        mods: PLAIN,
        shown: "N",
        help: "Previous frame, or page",
        when: Some(When::AnimationOrPages),
        keys: &[(Char("N"), PreviousFrame)],
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
    /// What holds this frame: which keys' conditions, and what makes a
    /// control dead, for the tooltip that then says why instead of what.
    conditions: Conditions,
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
        if let Some(refused) = ui::tooltip::disabled(at, self.conditions.reasons()) {
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
            // The count says which of the list is on screen. A press on it
            // opens the chooser, said the way the state's press is, with the
            // key that opens it too; under that the keys that step through
            // the list without opening anything.
            Tip::Counter => {
                let chooser = binding_for(OpenChooser).map(|binding| {
                    format!("Click to choose a file from the list ({})", binding.shown)
                });
                (
                    vec![format!("File {} of {}", self.index + 1, self.count)],
                    chooser
                        .into_iter()
                        .chain([NextFile, PreviousFile].into_iter().filter_map(hint))
                        .collect(),
                )
            }
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
            // The exposure's slider, and under it the keys that step what
            // it sets.
            Tip::Exposure => (
                vec![names(at)?],
                [Exposure(-EV_STEP)].into_iter().filter_map(hint).collect(),
            ),
            // The two handles: what each is, and under it the pair of keys
            // that step it. The band between them slides the window, which
            // no key does, so it names itself and nothing more.
            Tip::BlackPoint => (
                vec![names(at)?],
                Vec::from_iter(hint(StepBlack(-WINDOW_STEP))),
            ),
            Tip::WhitePoint => (
                vec![names(at)?],
                Vec::from_iter(hint(StepWhite(-WINDOW_STEP))),
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

    fn help(&self) -> Vec<ui::help::Section> {
        help_sections(&self.conditions)
    }
}

/// The key table as the help popup lays it out: one section per heading,
/// in `--help`'s order, and in each one row per line of the table, the key
/// column spelled as the tooltips spell it, and each condition marked with
/// whether it holds under `conditions`.
///
/// Free of the `Namer` on purpose: nothing else about the frame changes
/// what the keys are, and a test can read the whole of it without one.
pub(super) fn help_sections(conditions: &Conditions) -> Vec<ui::help::Section> {
    Section::ALL
        .into_iter()
        .map(|section| ui::help::Section {
            title: section.title(),
            rows: KEYS
                .iter()
                .filter(|binding| binding.section == section)
                .map(|binding| ui::help::Row {
                    key: binding.shown.to_string(),
                    does: binding.help,
                    when: binding.when.map(|when| ui::help::Condition {
                        words: when.describe(),
                        met: conditions.met(when),
                    }),
                })
                .collect(),
        })
        .collect()
}

/// What an event leaves the window owing. Ordered: a frame is more than
/// nothing, and leaving is more than a frame, which is what lets two
/// effects fold into one with [`Effect::also`].
#[must_use]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Effect {
    Nothing,
    /// The frame is out of date.
    Redraw,
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

    /// This effect and `other` together: a frame owed by either is owed,
    /// and asking to leave outweighs a frame.
    pub fn also(self, other: Effect) -> Effect {
        self.max(other)
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
    /// Where `Space` is: the one key that fits on its way up.
    pub(super) space: Space,
}

/// Where `Space` is. Held, a drag on the picture draws a box to zoom to,
/// which is why the key fits nothing on its way down — the view would move
/// under the hand about to draw — and fits on its way up instead, unless a
/// box was drawn while it was down. The key's repeats are the same press
/// still going, and are not answered.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) enum Space {
    #[default]
    Up,
    Held {
        /// Whether a box has been drawn while it was down, which is what
        /// letting go of it asks before it fits anything.
        drawn: bool,
    },
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
pub(super) fn report(error: &anyhow::Error) {
    eprintln!("gamut: {}", crate::escape_controls(&format!("{error:#}")));
}

/// The one line about it that goes in the window: the failure itself, without
/// the chain under it.
pub(super) fn briefly(error: &anyhow::Error) -> String {
    crate::escape_controls(&error.to_string())
}

impl App {
    pub(super) fn handle_key(
        &mut self,
        key: &Key,
        position: PhysicalKey,
        state: ElementState,
    ) -> Effect {
        // Space is answered on its way up — see `Space` — and its release
        // is read whatever is held with it by then, so that a chord pressed
        // while it was down cannot leave it held for good.
        if *key == Key::Named(NamedKey::Space) && state == ElementState::Released {
            return self.release_space();
        }
        if state == ElementState::Released {
            return Effect::Nothing;
        }
        match action_for(key, position, self.pointer.modifiers) {
            Some(CycleFit) => self.hold_space(),
            Some(action) => self.perform(action),
            None => Effect::Nothing,
        }
    }

    /// `Space` went down. Nothing moves, but the pointer over the picture
    /// changes to say what a drag would now do, which takes a frame. A
    /// repeat of a key already held is the same press still going, and
    /// changes nothing.
    fn hold_space(&mut self) -> Effect {
        if self.pointer.space != Space::Up {
            return Effect::Nothing;
        }
        self.pointer.space = Space::Held { drawn: false };
        Effect::Redraw
    }

    /// `Space` came up: the fit it asked for, unless a box was drawn while
    /// it was down — in which case the zoom was the box's, the key is
    /// spent, and only the pointer has to change back.
    fn release_space(&mut self) -> Effect {
        let space = std::mem::take(&mut self.pointer.space);
        match space {
            Space::Held { drawn: false } => self.perform(CycleFit),
            Space::Held { drawn: true } => Effect::Redraw,
            Space::Up => Effect::Nothing,
        }
    }

    /// The window lost the keyboard: whatever was held is not held here
    /// any more, and its release will go elsewhere.
    pub(super) fn keys_lost(&mut self) {
        self.pointer.space = Space::Up;
    }

    /// Does what a key asked for.
    pub(super) fn perform(&mut self, action: Action) -> Effect {
        // A region on screen takes the keys that move, fit and copy the
        // picture: the picture is what is being looked at, and the region is
        // what is being done to it.
        if let Selection::Shown(region) = self.marking.selection
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
                // A box being dragged out is dropped before the region is:
                // the toolkit is taking the drag off the hand on this same
                // key, and the release it sends next must find nothing to
                // zoom to.
                if self.marking.drop_box() {
                    return Effect::Redraw;
                }
                // The region after the message: a message is about what
                // was just done, and the region is what was being done to
                // — the one that stops being news first goes first.
                if self.marking.selection.is_on() {
                    self.marking.clear();
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
            CycleFit => self.animate(|view, image, viewport| view.cycle_fit(image, viewport)),
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
            // The button's own press, so that the key and a press from
            // inside the popup — which is how the key arrives while the
            // popup has the keyboard — cannot come to mean different things.
            OpenChooser => return self.press(Control::Chooser),
            ShowHelp => return self.press(Control::Help),
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
            ToggleHistogram => return self.press(Control::Histogram),
            ToggleLuma => return self.press(Control::Luma),
            TogglePlanes => return self.press(Control::Planes),
            ToggleLogCounts => return self.press(Control::Log),
            ToggleInfo => return self.press(Control::Info),
            ToggleMinimap => return self.press(Control::Minimap),
            ToggleGrid => return self.press(Control::Grid),
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
            // The handles' keys: each end of the window stepped along the
            // plot, on the file's own curve and no further than the plot
            // goes, as the handle it stands for is dragged.
            StepBlack(by) => {
                return self.adjust(|current, _| {
                    let transfer = current.image.color.transfer;
                    current
                        .display
                        .step_black(by, transfer, current.stats.plot.min)
                });
            }
            StepWhite(by) => {
                return self.adjust(|current, _| {
                    let transfer = current.image.color.transfer;
                    current
                        .display
                        .step_white(by, transfer, current.stats.plot.max)
                });
            }
            // Refused under a false color, and by the display itself, so that
            // the key and the button beside the histogram cannot drift.
            CycleToneMap => {
                return self
                    .adjust(|current, _| current.display.cycle_tone_map(current.image.is_gray()));
            }
            MarkClipped => return self.press(Control::Marks),
            CycleColormap => {
                return self
                    .adjust(|current, _| current.display.cycle_colormap(current.image.is_gray()));
            }
            // A copy takes the selection and leaves the picture exactly as it
            // was, so the message at the foot of the window is the only sign
            // it happened at all — and the only way to tell a copy that
            // worked from a key that was never read.
            CopyName => {
                let Some(name) = self.shown_name() else {
                    return Effect::Nothing;
                };
                self.copy(name.as_bytes(), clipboard::TEXT, "Copied file name.");
            }
            CopyPath => {
                let Some(path) = self.shown_path() else {
                    return Effect::Nothing;
                };
                self.copy(
                    path.to_string_lossy().as_bytes(),
                    clipboard::TEXT,
                    "Copied file path.",
                );
            }
            CopyUri => {
                let Some(path) = self.shown_path() else {
                    return Effect::Nothing;
                };
                let list = clipboard::uri_list(&path);
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
            ToggleHdr => return self.press(Control::Output),
            ToggleRegion => return self.press(Control::Region),
            // Only a region shrinks, and there is none: see `perform_on_region`.
            ShrinkRegion(_) => return Effect::Nothing,
            TogglePlay => return self.toggle_play(),
            NextFrame => return self.step_frame(1),
            PreviousFrame => return self.step_frame(-1),
            // The menu's own items, so that the key and the item cannot
            // come to mean different things.
            Rename => return self.press(Control::Rename),
            Delete => return self.press(Control::Delete),
            Undo => return self.undo(),
            // The buttons' own presses, so that the key and the button in
            // the middle of an empty window cannot come to mean different
            // things.
            OpenFiles => return self.press(Control::OpenFiles),
            OpenFolder => return self.press(Control::OpenFolder),
        }
        Effect::Redraw
    }

    /// What `action` does to `region`, the region on screen, where it does
    /// something to it: the arrows move its current handle a pixel — the
    /// whole of it, while that is the middle — with Ctrl grow it, with
    /// Ctrl and Shift shrink it, `Space` frames it and then the picture,
    /// and the copy of the picture copies it. `None` for every other
    /// action, which is the picture's as it always was.
    ///
    /// Nothing here is animated: a region moves by a pixel at a time, and a
    /// pixel has nothing to animate.
    fn perform_on_region(&mut self, region: Region, action: Action) -> Option<Effect> {
        let image = self.image_pixels();
        Some(match action {
            Pan(direction, Edge) => {
                self.marking
                    .select(region.grown(direction.side(), 1, image));
                Effect::Redraw
            }
            // Left pulls the right edge in: the edge that moves lies the
            // other way from the arrow, where growing moves the one that
            // lies its way.
            ShrinkRegion(direction) => {
                self.marking
                    .select(region.shrunk(direction.side().opposite(), 1));
                Effect::Redraw
            }
            // The fine pan is left to the picture: a region moves by the
            // pixel already, and the picture under it still wants moving by
            // one.
            Pan(direction, Coarse) => {
                let step = direction.step();
                // The middle moves the whole region, and so does a handle
                // that has no edge to move the way the arrow points — an
                // edge's own axis — rather than leaving the key dead.
                let moved = region
                    .nudged(self.marking.handle, step, image)
                    .unwrap_or_else(|| region.moved_by(step[0], step[1], image));
                self.marking.select(moved);
                Effect::Redraw
            }
            CycleFit => {
                let framing = self.marking.framing;
                self.animate(|view, image, viewport| match framing {
                    Framing::Region(fit) => {
                        view.fit_region(fit, region.as_f32(), image, viewport);
                    }
                    Framing::Picture(fit) => view.set_fit(fit),
                    Framing::Actual => view.set_zoom(1.0, image, viewport),
                });
                self.marking.framing = framing.next();
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

    /// A drag on the picture has taken hold of the region — or of nothing
    /// yet, to draw one, or to draw a box to zoom to — at `at`, in image
    /// pixels. The box is what the held key was for, and the key is spent
    /// on it: letting go of it afterwards fits nothing.
    fn grab(&mut self, grab: Grab, at: [f32; 2]) {
        if grab == Grab::Zoom
            && let Space::Held { drawn } = &mut self.pointer.space
        {
            *drawn = true;
        }
        self.marking.grab(grab, at);
    }

    /// The hand is at `to`, in image pixels.
    fn pull(&mut self, to: [f32; 2]) {
        let image = self.image_pixels();
        self.marking.pull(to, image);
    }

    /// The button came up. A box dragged out is what the view goes to:
    /// fitted whole, as a move, the way a zoom asked for by name is.
    fn release(&mut self) {
        if let Some(boxed) = self.marking.release() {
            self.animate(|view, image, viewport| {
                view.fit_region(Fit::Whole, boxed.as_f32(), image, viewport);
            });
        }
    }

    /// Closes whatever menu is open. Returns whether there was one: the
    /// press that closes a menu is spent doing exactly that.
    ///
    /// Asked of egui rather than of a flag of our own, the menus being its:
    /// it closes one on Escape itself, and this is what keeps the key that
    /// puts things away from quitting out from under one.
    pub(super) fn close_menus(&mut self) -> bool {
        let Some(shown) = &self.shown else {
            return false;
        };
        if !shown.gui.ctx.any_popup_open() {
            return false;
        }
        egui::Popup::close_all(&shown.gui.ctx);
        true
    }

    /// The words the interface may need this frame, gathered from wherever
    /// the application keeps them.
    pub(super) fn namer(&self) -> Namer {
        Namer {
            path: self
                .shown_path()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            index: self.files.index(),
            count: self.files.len(),
            show_histogram: self.panels.show_histogram,
            state: self
                .current
                .as_ref()
                .map(|current| ui::explain_state(current, self.headroom()))
                .unwrap_or_default(),
            conditions: self.conditions(),
        }
    }

    /// What holds right now: which of the conditions the keys wait on,
    /// each read from exactly what the key's own arm of [`App::perform`]
    /// reads, and what makes a control dead. The one reading behind the
    /// help popup's dimming, the tooltips' refusals and [`App::refuses`].
    pub(super) fn conditions(&self) -> Conditions {
        let current = self.current.as_ref();
        Conditions {
            region_selected: matches!(self.marking.selection, Selection::Shown(_)),
            several_files: self.files.len() > 1,
            animation: self.animation.is_some(),
            pages: current.is_some_and(|current| {
                matches!(
                    current.sequence,
                    crate::image::sequence::Sequence::Pages { .. }
                )
            }),
            pointer_on_picture: self.pointer_pixel().is_some(),
            picture_on_clipboard: self.panels.paste,
            single_channel: current.is_some_and(|current| current.image.is_gray()),
            undoable: !self.edits.is_empty(),
            room: self.room(),
            hdr: self.hdr_state(),
            openable: !self.openers.is_empty(),
            false_colored: current
                .is_some_and(|current| current.display.false_colored(current.image.is_gray())),
            picking: self.picking,
            nothing_open: current.is_none(),
        }
    }

    /// Whether a press on `control` is refused: exactly when the control
    /// is drawn dead, and its tooltip says why, since the three read one
    /// [`Conditions`]. A press on a dead control that quietly set something
    /// no one could see would be worse than one that does nothing.
    fn refuses(&self, control: Control) -> bool {
        ui::tooltip::disabled(Tip::Control(control), self.conditions().reasons()).is_some()
    }

    /// Acts on what a pass of the interface asked for, and says what the
    /// window owes for it.
    ///
    /// Nearly everything is a change to something drawn. The two exceptions
    /// are the readings the pass makes on every frame — whether the pointer
    /// is on the picture, and which handle of the region it rests on —
    /// which owe a frame only when they differ from the last: counted as a
    /// change every frame, they would have every frame asking for the
    /// next, and the window spinning at whatever rate the surface allows
    /// while nothing on it moved.
    pub(super) fn act(&mut self, command: ui::Command) -> Effect {
        match command {
            ui::Command::OverImage(over) => {
                return Effect::redraw_if(
                    std::mem::replace(&mut self.pointer.over_image, over) != over,
                );
            }
            ui::Command::OverGrip(grip) => {
                return Effect::redraw_if(std::mem::replace(&mut self.marking.grip, grip) != grip);
            }
            // A press owes a frame whatever it did — egui repaints the
            // button it was on for its own reasons, and the press may have
            // changed what is under it — over and above what the press
            // itself says it owes.
            ui::Command::Press(control) => return self.press(control).also(Effect::Redraw),
            // The hand on the band under the histogram: the values that come
            // out black and white go where the handles are put, as the view
            // goes where a drag puts it. Not animated, and not a step: the
            // hand is on it. The exposure is left alone by both.
            ui::Command::BlackPoint(black) => {
                if let Some(current) = self.current.as_mut() {
                    current.display.put_black(black);
                }
            }
            ui::Command::WhitePoint(white) => {
                if let Some(current) = self.current.as_mut() {
                    current.display.put_white(white);
                }
            }
            ui::Command::Slide { black, white } => {
                if let Some(current) = self.current.as_mut() {
                    current.display.set_displayed_bounds(black, white);
                }
            }
            ui::Command::Exposure(stops) => {
                if let Some(current) = self.current.as_mut() {
                    current.display.set_exposure(stops);
                }
            }
            // The image follows the pointer, so the viewport moves the other
            // way. Not animated: the hand is on the view.
            ui::Command::Drag([dx, dy]) => {
                let (image, viewport) = (self.image_size(), self.viewport());
                self.view.pan_by(-dx, -dy, image, viewport);
            }
            ui::Command::Wheel { steps, notched } => self.wheel(steps, notched),
            // The hand on the minimap: the view goes where it is put, as
            // it does for a drag on the picture.
            ui::Command::Center(at) => {
                let (image, viewport) = (self.image_size(), self.viewport());
                self.view.center_on(at, image, viewport);
            }
            ui::Command::Grab { grab, at } => self.grab(grab, at),
            ui::Command::Handle(grip) => self.marking.handle = grip,
            ui::Command::Pull(to) => self.pull(to),
            ui::Command::Release => self.release(),
            // The chooser's own: what was typed, where the cursor went, and
            // which rows are on screen — whose thumbnails go to the front of
            // the queue, and are the ones the screen keeps.
            ui::Command::Query(query) => self.chooser.set_query(query),
            // The rename dialog's field: what it says now, judged for the
            // next frame to say what is wrong with it.
            ui::Command::Name(name) => self.set_rename_name(name),
            ui::Command::Cursor(step) => self.chooser.step(step),
            ui::Command::Visible(rows) => {
                for row in rows.clone() {
                    if let Some(path) = self.chooser.path_at(row) {
                        self.thumbs.touch(path);
                    }
                }
                let wanted = self.chooser.wanted(rows, &self.thumbs);
                self.thumbnailer.prioritize(wanted);
            }
        }
        Effect::Redraw
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

    /// The absolute path of the file on screen, and `None` with nothing on
    /// the list. Absolute because what is copied is bound for somewhere
    /// else, where the directory this was started in means nothing — and
    /// because a URI has no other kind. The path as given stands in if it
    /// cannot be made absolute, which needs the working directory and so
    /// can fail.
    fn shown_path(&self) -> Option<PathBuf> {
        let path = self.files.shown_path()?;
        Some(std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()))
    }

    /// The name of the file on screen, with nothing of the directory it sits
    /// in — what the top bar shows, at whatever length it actually is — and
    /// `None` with nothing on the list.
    ///
    /// Taken from the path as it was given rather than from the absolute one:
    /// the two end in the same name, and there is nothing here that needs the
    /// working directory. The whole path stands in for one that ends in no
    /// name at all, which a file on the list never does.
    fn shown_name(&self) -> Option<String> {
        let path = self.files.shown_path()?;
        Some(match path.file_name() {
            Some(name) => name.to_string_lossy().into_owned(),
            None => path.to_string_lossy().into_owned(),
        })
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
        let lift = current.lift.clone();
        let said = match region {
            Some(_) => "Copied region.",
            None => "Copied image.",
        };
        let region = region.unwrap_or_else(|| Region::whole([image.width, image.height]));
        self.copying.spawn(move |ticket| {
            let (width, height) = (region.width, region.height);

            let walked = Instant::now();
            let raster = encode::displayed(&image, &display, region, lift.as_deref());
            timing::mapped_image(width, height, walked.elapsed());

            let encoded = Instant::now();
            let png = match encode::png(&raster) {
                Ok(png) => png,
                Err(error) => {
                    report(&error);
                    ticket.report(Err(briefly(&error)));
                    return;
                }
            };
            timing::encoded_png(width, height, png.len(), encoded.elapsed());

            if ticket.superseded() {
                return;
            }
            match clipboard::copy(&png, clipboard::PNG) {
                Ok(()) => ticket.report(Ok(said)),
                Err(error) => {
                    report(&error);
                    ticket.report(Err(briefly(&error)));
                }
            }
        });
    }

    /// Hands the file on screen to the program at `index` of the open menu.
    ///
    /// The file as it is on disk, not the picture as it is being shown: what
    /// is being asked for is another program's reading of the same file, and
    /// a copy with this window's exposure baked into it would be the one
    /// thing that could not answer. The display settings stay here, which is
    /// also why nothing has to be written out first.
    ///
    /// Out of range is not a failure: the list is the file's, and a menu left
    /// open across a file arriving is a menu of the file that has gone.
    fn open_in(&mut self, index: usize) {
        let Some(opener) = self.openers.get(index) else {
            return;
        };
        let name = opener.name.clone();
        let Some(path) = self.shown_path() else {
            return;
        };
        match openers::open(opener, &path) {
            // What was asked for and not what has happened: the program has
            // been started, and how long it takes to put a window up is its
            // own business.
            Ok(()) => self.toast(format!("Opening in {name}."), Level::Message),
            Err(error) => {
                report(&error);
                self.toast(briefly(&error), Level::Error);
            }
        }
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
        // A paste is the window's own doing: from here on nothing showing
        // is the window's to answer, not the command line's.
        self.from_command_line = false;
        let request = self.files.adopt(path, Source::Clipboard(offer.mime));
        self.send(request);
        self.list_changed();
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
        self.copying.claim();
        match clipboard::copy(content, mime_type) {
            Ok(()) => self.toast(said, Level::Message),
            Err(error) => {
                report(&error);
                self.toast(briefly(&error), Level::Error);
            }
        }
    }

    /// Acts on a press, and says what the window owes for it. The keys that
    /// stand in for the toggles come through here too, so that a key and a
    /// click cannot drift apart.
    pub(super) fn press(&mut self, widget: Control) -> Effect {
        if self.refuses(widget) {
            return Effect::Nothing;
        }
        match widget {
            // The key's own action, so that a press and a keystroke cannot
            // come to mean different things. Nothing is drawn differently
            // yet: the file is only being asked for, and what is on screen
            // stays until it arrives.
            Control::Previous => {
                self.step(false);
                Effect::Nothing
            }
            Control::Next => {
                self.step(true);
                Effect::Nothing
            }
            Control::Minimap => {
                self.panels.show_minimap = !self.panels.show_minimap;
                Effect::Redraw
            }
            // Where the window has no room for the panel the press was
            // refused above, as the surface switch is where there is no
            // headroom to switch to.
            Control::Histogram => {
                self.panels.show_histogram = !self.panels.show_histogram;
                Effect::Redraw
            }
            Control::Grid => {
                self.panels.show_grid = !self.panels.show_grid;
                Effect::Redraw
            }
            // The keys' own actions, and which of the two by the modifier
            // the keys are told apart by: a plain press hides the bars, and
            // Shift closes the panels floating over the picture on the way,
            // exactly as `` ` `` and `~` do.
            Control::Maximize => {
                let action = match self.pointer.modifiers.shift_key() {
                    true => ToggleInterfaceAndPanels,
                    false => ToggleInterface,
                };
                self.perform(action)
            }
            Control::Info => {
                self.panels.show_info = !self.panels.show_info;
                Effect::Redraw
            }
            // The five buttons that open a menu: the menu is egui's, and
            // opens itself on the press, so there is nothing here to do.
            Control::Zoom
            | Control::PixelFormat
            | Control::Copy
            | Control::OpenIn
            | Control::FileMenu => Effect::Nothing,
            // The menu of the file's own items, and the keys that do the
            // same, so that the two cannot come to mean different things;
            // and the dialog's two buttons, which `Enter` and `Esc` reach
            // through the dialog itself.
            Control::Rename => {
                self.open_rename();
                Effect::Redraw
            }
            Control::Delete => {
                self.delete_shown();
                Effect::Redraw
            }
            Control::RenameTo => {
                self.rename_shown();
                Effect::Redraw
            }
            Control::CancelRename => {
                self.cancel_rename();
                Effect::Redraw
            }
            // The two buttons in the middle of an empty window, and the
            // keys that put up the same dialog from anywhere. Nothing to
            // draw: the dialog is the desktop's, and what it answers
            // arrives later.
            Control::OpenFiles => {
                self.pick(Pick::Files);
                Effect::Nothing
            }
            Control::OpenFolder => {
                self.pick(Pick::Folder);
                Effect::Nothing
            }
            // An item of the open menu, by its place in the list the same
            // frame was drawn from.
            Control::Opener(index) => {
                self.open_in(index);
                Effect::Redraw
            }
            // The action the key runs, as with the reset below: the button
            // is on screen because the clipboard was holding a picture at the
            // last look, and the paste asks it again rather than trusting
            // that. A selection that has gone in between is answered the way
            // an empty clipboard is.
            Control::Paste => {
                self.paste();
                Effect::Redraw
            }
            // Asks for a region, or takes off the one asked for or drawn.
            // The key's own action goes through here too, so that the
            // button and `x` cannot come to mean different things.
            Control::Region => {
                match self.marking.selection {
                    Selection::Off => self.marking.selection = Selection::Armed,
                    Selection::Armed | Selection::Shown(_) => self.marking.clear(),
                }
                Effect::Redraw
            }
            // The keys' own actions, so that the bar and `Enter`, `n` and
            // `N` cannot come to mean different things.
            Control::Play => self.toggle_play(),
            Control::StepBack => self.step_frame(-1),
            Control::StepForward => self.step_frame(1),
            Control::Seek(frame) => self.seek(frame),
            Control::Luma => {
                self.panels.show_luma = !self.panels.show_luma;
                Effect::Redraw
            }
            Control::Planes => {
                self.panels.show_planes = !self.panels.show_planes;
                Effect::Redraw
            }
            // The plot's own axis rather than anything about the rendering,
            // which is why the reset below leaves it alone: it is how the
            // measurement is being read, not what is being read.
            Control::Log => {
                self.panels.log_counts = !self.panels.log_counts;
                Effect::Redraw
            }
            // Where `w` lands too, so that the key and the button beside
            // the panel's band cannot come to mean different things.
            Control::Marks => {
                self.panels.mark_clipped = !self.panels.mark_clipped;
                Effect::Redraw
            }
            // The action the key runs, rather than a second reading of what
            // "reset" means: two of them would answer differently the first
            // time either was touched, and a button and a key that disagree
            // about one word are worse than either alone.
            Control::Reset => self.perform(ResetDisplay),
            // The display refuses a false color on a color image, for the
            // key and the button alike.
            Control::Ramp(index) => self.adjust(|current, _| {
                Colormap::ALL
                    .get(index)
                    .is_some_and(|map| current.display.set_colormap(*map, current.image.is_gray()))
            }),
            // A window named outright rather than the next one along.
            Control::Window(index) => {
                if let Some(current) = self.current.as_mut()
                    && let Some((_, window)) = histogram::WINDOWS.get(index)
                {
                    current.display.set_auto(*window, &current.stats);
                    return Effect::Redraw;
                }
                Effect::Nothing
            }
            // And a curve under a false color, the same way.
            Control::Curve(index) => self.adjust(|current, _| {
                ToneMap::ALL.get(index).is_some_and(|curve| {
                    current
                        .display
                        .set_tone_map(*curve, current.image.is_gray())
                })
            }),
            // A cell of the zoom menu: a zoom chosen here is a move.
            Control::ZoomTo(choice) => {
                self.animate(|view, image, viewport| choice.apply(view, image, viewport));
                Effect::Redraw
            }
            Control::Format(format) => {
                self.panels.pixel_format = format;
                Effect::Redraw
            }
            // An item of the menu of copies runs the key's action, as the
            // reset and the paste buttons do: what it asks for is done rather
            // than set.
            Control::Copies(what) => self.perform(copy_action(what)),
            Control::Facts(copies) => {
                self.copy_facts(copies);
                Effect::Redraw
            }
            // As with the reset: the key's action, so that the button and the
            // key cannot come to mean different things.
            Control::Output => self.toggle_hdr(),
            // The cross on the message at the foot of the window. The frame
            // after re-tests the pointer, which is what takes the highlight
            // off a button that is no longer there.
            Control::Dismiss => {
                self.toasts.dismiss();
                Effect::Redraw
            }
            // The chooser, toggled. Its open state is egui's, as a menu's is,
            // so opening it is a matter of asking egui — which closes any
            // menu on the way, one popup being open at a time — and closing
            // it is what closes a menu. Opened, it starts over the list as it
            // stands with the cursor on the file on screen, and the first
            // rows' thumbnails are asked for ahead of the rest.
            Control::Chooser => {
                let Some(shown) = &self.shown else {
                    return Effect::Nothing;
                };
                let ctx = &shown.gui.ctx;
                let open = egui::Popup::is_id_open(ctx, ui::chooser::id());
                egui::Popup::close_all(ctx);
                // Nothing to choose from a list of one: the key does
                // nothing, as the count it stands beside is not shown.
                if !open && self.files.len() > 1 {
                    egui::Popup::open_id(ctx, ui::chooser::id());
                    self.chooser.open(self.files.paths(), self.files.index());
                    let wanted = self.chooser.wanted(0..FIRST_ROWS, &self.thumbs);
                    self.thumbnailer.prioritize(wanted);
                }
                Effect::Redraw
            }
            // The help popup: opened, or closed if it is the popup that is
            // up. Any other popup goes first, one being open at a time.
            Control::Help => {
                let Some(shown) = &self.shown else {
                    return Effect::Nothing;
                };
                let ctx = &shown.gui.ctx;
                let open = egui::Popup::is_id_open(ctx, ui::help::id());
                egui::Popup::close_all(ctx);
                // Where the window has no room to draw it the press was
                // refused above, as the panels' are: it would be up and
                // unseen.
                if !open {
                    egui::Popup::open_id(ctx, ui::help::id());
                }
                Effect::Redraw
            }
            // A row of the chooser: the file it names, asked for as a file
            // is when it is named outright rather than stepped to. The row
            // is resolved to its path and the path to its place, since the
            // list can have been rebuilt under the popup.
            Control::Choose(row) => {
                self.close_menus();
                let chosen = self.chooser.path_at(row).map(Path::to_path_buf);
                if let Some(path) = chosen
                    && Some(path.as_path()) != self.files.shown_path()
                    && let Some(index) = self.files.position(&path)
                {
                    let request = self.files.go_to(index);
                    self.send(request);
                }
                Effect::Redraw
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
        let sample = current
            .image
            .sample(at[0], at[1], current.lift.as_deref())?;
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

        // The two items of the file menu that are not copies, by the keys
        // that do the same; the button that opens the menu names itself,
        // every item of it having a key of its own.
        assert_eq!(
            named(Control::Rename).as_deref(),
            Some("Rename the file on screen (F2)")
        );
        assert_eq!(
            named(Control::Delete).as_deref(),
            Some("Move the file on screen to the trash, and show the next (Del, \u{232b})")
        );
        let file = named(Control::FileMenu).expect("the button names itself");
        assert!(!file.contains('('), "{file}");
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
                    | Control::Seek(_)
                    | Control::Chooser
            );
            assert_eq!(
                names(Tip::Control(*widget)).is_some(),
                !wordless,
                "{widget:?}"
            );
        }
    }

    /// The two buttons in the middle of an empty window are named in their
    /// own words and by the keys that put up the same dialog, and the
    /// shortcut printed on each is that key: `Ctrl+O` for files, and the
    /// same with Shift for a folder — one case each, as the two `C`s are.
    #[test]
    fn the_open_buttons_name_the_keys_that_open_the_dialog() {
        assert_eq!(
            names(Tip::Control(Control::OpenFiles)).as_deref(),
            Some("Choose image files to open (Ctrl+O)")
        );
        assert_eq!(
            names(Tip::Control(Control::OpenFolder)).as_deref(),
            Some("Choose a folder of images to open (Ctrl+Shift+O)")
        );
        let namer = Namer {
            path: String::new(),
            index: 0,
            count: 0,
            show_histogram: false,
            state: Vec::new(),
            conditions: Conditions::ALIVE,
        };
        assert_eq!(
            namer.shortcut(Control::OpenFiles).as_deref(),
            Some("Ctrl+O")
        );
        assert_eq!(
            namer.shortcut(Control::OpenFolder).as_deref(),
            Some("Ctrl+Shift+O")
        );
        assert_eq!(namer.shortcut(Control::Paste).as_deref(), Some("Ctrl+V"));

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

        // Dead while the dialog is up, and the label says so instead.
        let picking = Namer {
            path: String::new(),
            index: 0,
            count: 0,
            show_histogram: false,
            state: Vec::new(),
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
    /// table keeps. A condition is a phrase, not a sentence: no capital at
    /// the front, no full stop at the end, and short enough for its column.
    #[test]
    fn the_help_popup_shows_every_line_of_the_table_once() {
        let sections = help_sections(&Conditions::default());
        assert_eq!(sections.len(), Section::ALL.len());
        let rows: Vec<&ui::help::Row> = sections
            .iter()
            .flat_map(|section| section.rows.iter())
            .collect();
        assert_eq!(rows.len(), KEYS.len());
        for (row, binding) in rows.iter().zip(KEYS) {
            assert_eq!(row.key, binding.shown);
            assert_eq!(row.does, binding.help);
            assert_eq!(
                row.when.map(|when| when.words),
                binding.when.map(When::describe)
            );
            // Nothing holds but the absence of a region, so every other
            // condition is marked unmet.
            assert_eq!(
                row.when.map(|when| when.met),
                binding.when.map(|when| when == When::NoRegion)
            );
        }
        for (section, listed) in Section::ALL.into_iter().zip(&sections) {
            assert_eq!(listed.title, section.title());
            assert!(!listed.rows.is_empty(), "{:?} has keys", section);
            assert!(
                KEYS.iter()
                    .filter(|binding| binding.section == section)
                    .count()
                    == listed.rows.len()
            );
        }
        for when in When::ALL {
            let words = when.describe();
            assert!(
                words.starts_with(char::is_lowercase) && !words.ends_with('.'),
                "{when:?}: {words:?} reads as a phrase"
            );
            assert!(words.len() <= 32, "{when:?}: {words:?} fits its column");
        }
    }

    /// Each condition is answered from its own reading, and one reading
    /// answers only the conditions that ask it — an animation is one where
    /// a page is not, and a paged file is enough for the keys that step
    /// through either.
    #[test]
    fn each_condition_is_met_by_its_own_reading() {
        let none = Conditions::default();
        for when in When::ALL {
            assert_eq!(
                none.met(when),
                when == When::NoRegion,
                "{when:?} with nothing to hold it"
            );
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
        ];
        for (held, conditions) in readings {
            for when in When::ALL {
                let expected = when == held
                    || (when == When::AnimationOrPages && held == When::Animation)
                    || (when == When::NoRegion && held != When::RegionSelected);
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
            ["White point down / up (A, S)"]
        );
        // And the exposure's slider names the keys that step it.
        assert_eq!(
            tooltip(Tip::Exposure).hints,
            ["Exposure down / up, a quarter stop (d, f)"]
        );
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
            conditions: Conditions::ALIVE,
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
            conditions: Conditions::ALIVE,
        };
        let tooltip = namer
            .tooltip(Tip::Counter)
            .expect("the count has a tooltip");
        assert_eq!(tooltip.title, ["File 3 of 12"]);
        assert_eq!(
            tooltip.hints,
            [
                "Click to choose a file from the list (Ctrl+P)",
                "Next file (], Page Down)",
                "Previous file ([, Page Up)",
            ]
        );
    }

    /// A chord bound twice to different things would do whichever came
    /// first in the table, silently. The same key under different modifiers
    /// is a different chord; and the same chord on two lines is allowed only
    /// where both bind it to the same action and at most one of them holds
    /// always — a key with one line for what it does plainly and one for
    /// what it does with a region up — since then the table dispatches the
    /// same whichever line is found first, and the lines do not both claim
    /// the same moment.
    #[test]
    fn no_chord_is_bound_twice_to_different_actions() {
        let mut seen: Vec<((Mods, KeyName), Action, Option<When>)> = Vec::new();
        for binding in KEYS {
            for (name, action) in binding.keys {
                let chord = (binding.mods, *name);
                if let Some((_, first, when)) = seen.iter().find(|(bound, ..)| *bound == chord) {
                    assert_eq!(
                        first, action,
                        "{name:?} with {:?} is bound to two different things",
                        binding.mods
                    );
                    assert!(
                        when.is_some() || binding.when.is_some(),
                        "{name:?} with {:?} is on two lines that both hold always",
                        binding.mods
                    );
                    continue;
                }
                seen.push((chord, *action, binding.when));
            }
        }
    }

    /// A key on two lines, one for what it does plainly and one for what it
    /// does with a region up, is named by the plain line: what asks is a
    /// button that does the plain thing.
    #[test]
    fn a_key_with_a_region_line_is_named_by_its_plain_line() {
        assert_eq!(
            binding_for(CopyImage).map(|binding| binding.help),
            Some("Copy the image as displayed")
        );
        assert_eq!(
            binding_for(CycleFit).map(|binding| binding.section),
            Some(Section::Zoom)
        );
        assert_eq!(
            binding_for(ToggleRegion).map(|binding| binding.when),
            Some(Some(When::NoRegion))
        );
        // A key only a region answers is still found.
        assert_eq!(
            binding_for(ShrinkRegion(Left)).map(|binding| binding.section),
            Some(Section::Region)
        );
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
        assert_eq!(plain("F"), Some(Exposure(EV_STEP)));
        // What is done to the file: two named keys for the trash, one for
        // the dialog, and the undo under Ctrl — the plain `z` being the
        // display's reset.
        assert_eq!(
            action_for(&Key::Named(NamedKey::Delete), ELSEWHERE, PLAIN),
            Some(Delete)
        );
        assert_eq!(
            action_for(&Key::Named(NamedKey::Backspace), ELSEWHERE, PLAIN),
            Some(Delete)
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
    /// shifted press as well.
    #[test]
    fn the_arrows_pan_by_what_is_held_with_them() {
        let left = Key::Named(NamedKey::ArrowLeft);
        let held = |mods| action_for(&left, ELSEWHERE, mods);
        assert_eq!(held(PLAIN), Some(Pan(Left, Coarse)));
        assert_eq!(held(SHIFT), Some(Pan(Left, Fine)));
        assert_eq!(held(CTRL), Some(Pan(Left, Edge)));
        assert_eq!(held(CTRL | SHIFT), Some(ShrinkRegion(Left)));
        assert_eq!(held(CTRL | SHIFT | Mods::ALT), None);
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
