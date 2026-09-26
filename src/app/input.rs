//! What the keyboard and the pointer do.
//!
//! Keys go through one table, [`ROWS`], which is also what `--help` prints:
//! a binding added here is documented by the same edit. Each key has a
//! dotted name the configuration file can rebind — see [`super::keymap`] —
//! and runs an [`Action`], and [`App::perform`] is the one place an action
//! happens. The mouse goes through [`crate::gestures`] the same way.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use winit::event::ElementState;
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};

use super::App;
use super::copying::Done;
use super::keymap::{Bound, Chord, KeyName, Keymap, Keys, Row};
use crate::clipboard;
use crate::gestures::{self, Button, Gestures, Surface, WheelAction};
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

/// What the window says the first time the interface is hidden: the keys
/// bound to bring it back, both of them, since the one that put it away is
/// not the one a reader who pressed the button knows about. A half nothing
/// is bound to is left out; with neither, nothing is said.
pub(super) fn restore_message(keys: &Keymap) -> Option<String> {
    let bound: Vec<String> = ["interface.toggle", "interface.dismiss"]
        .into_iter()
        .map(|name| keys.spelled(name))
        .filter(|spelled| !spelled.is_empty())
        .collect();
    match bound.is_empty() {
        true => None,
        false => Some(format!("Press {} to restore UI", bound.join(" or "))),
    }
}

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
    /// Move the region's current handle a pixel that way — the whole of it,
    /// while that is the middle. Nothing without a region, as for the two
    /// below: there is nothing else on screen that they change.
    MoveRegion(Direction),
    /// Push a region's near side out a pixel that way.
    GrowRegion(Direction),
    /// Pull a region's far side in a pixel that way — Left brings the
    /// right edge in — which is the opposite number of growing it.
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
    /// Put the file list up down the left of the picture, or take it down
    /// — see `ui::filmstrip`.
    ToggleFilmstrip,
    /// Back to the file that was on screen before this one, and forward
    /// again — see `app::visited`.
    Back,
    Forward,
    /// Take the file on screen off the list, leaving it as it is on disk,
    /// and step on to the next — see `App::remove_shown`.
    Remove,
    ToggleInterface,
    /// The interface, and the panels floating over the image with it: the
    /// bars come and go as [`Action::ToggleInterface`], and the map,
    /// histogram, information panel and file list are closed on the way
    /// past.
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
    /// The loupe: the toggle the button beside the grid's presses, and the
    /// magnification stepped round the ones on offer, which the wheel
    /// steps with the secondary button held.
    ToggleLoupe,
    CycleMagnification,
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
    /// screen the fit frames it and the copy of the picture is a copy of
    /// it: see `App::perform_on_region`.
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
    /// Turn the picture on screen a quarter counterclockwise, or clockwise
    /// — a reading of it, like the window, and not a change to the file:
    /// see `App::turn_picture`.
    TurnLeft,
    TurnRight,
    /// Open the export dialog on the picture as shown — see `app::exporting`.
    Export,
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

/// What a chord is held with.
pub type Mods = ModifiersState;

/// Held with nothing, or with nothing but the Shift a character carries.
const PLAIN: Mods = Mods::empty();
const CTRL: Mods = Mods::CONTROL;
const SHIFT: Mods = Mods::SHIFT;
const CTRL_SHIFT: Mods = Mods::CONTROL.union(Mods::SHIFT);
const ALT: Mods = Mods::ALT;

/// What to press for `action`: the chords of the name that runs it, and
/// `None` where nothing is bound to it.
///
/// The name's own chords rather than the whole line's: the number row is
/// one line, `2, 3, 4, 5` for four zooms, and the cell of the zoom menu that
/// goes to one of them is named by the key that reaches it.
fn key_of(keys: &Keymap, action: Action) -> Option<String> {
    let (_, name) = keys.bound_for(action)?;
    Some(keys.spelled(name)).filter(|spelled| !spelled.is_empty())
}

/// `words`, and the key after it in brackets where there is one.
fn with_key(words: &str, key: Option<String>) -> String {
    match key {
        Some(key) => format!("{words} ({key})"),
        None => words.to_string(),
    }
}

/// How far one press of the keys moves an end of the display window, as a
/// fraction of the window's width. The hand on the histogram's band moves
/// it by no step at all — the handles go where they are put — so this is
/// the keys' alone.
const WINDOW_STEP: f32 = 0.05;

/// One line of a tooltip: what a key does, and what to press for it — the
/// whole line's keys, a hint being about the line. `None` where nothing is
/// bound on it, a hint being what to press.
///
/// The key table's own words, so that a tooltip and `--help` cannot come to
/// disagree about a binding — there is nowhere for them to disagree.
fn hint(keys: &Keymap, action: Action) -> Option<String> {
    let row = keys.row_for(action)?;
    let column = keys.column(row);
    (!column.is_empty()).then(|| format!("{} ({column})", row.help))
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
        // The toggle before them, the pair at the head of the list it
        // puts up, and the key that takes a file off that list.
        Tip::Control(Control::Filmstrip) => ToggleFilmstrip,
        Tip::Control(Control::Back) => Back,
        Tip::Control(Control::Forward) => Forward,
        Tip::Control(Control::Remove) => Remove,
        Tip::Control(Control::Minimap) => ToggleMinimap,
        Tip::Control(Control::Histogram) => ToggleHistogram,
        Tip::Control(Control::Info) => ToggleInfo,
        Tip::Control(Control::Grid) => ToggleGrid,
        Tip::Control(Control::Loupe) => ToggleLoupe,
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
        Tip::Control(Control::TurnLeft) => TurnLeft,
        Tip::Control(Control::TurnRight) => TurnRight,
        Tip::Control(Control::Export) => Export,
        // The two buttons in the middle of an empty window, by the keys
        // that put up the same dialog.
        Tip::Control(Control::OpenFiles) => OpenFiles,
        Tip::Control(Control::OpenFolder) => OpenFolder,
        // A swatch, a window or a curve past the end of its row is nothing.
        Tip::Control(Control::Ramp(_) | Control::Window(_) | Control::Curve(_)) => return None,
        // What no key reaches, and so names itself or wears its own name:
        // the buttons that open a menu, the items that wear a program's or
        // a file's name, the rows of the information panel, the cross on a
        // message, the timeline, and the dialogs' buttons.
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
            | Control::Sorting
            | Control::SortBy(_)
            | Control::SortDirection(_)
            | Control::Thumb(_)
            | Control::RenameTo
            | Control::CancelRename
            | Control::ExportAs(_)
            | Control::ExportTo
            | Control::CancelExport,
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
fn names(keys: &Keymap, tip: Tip) -> Option<String> {
    let pressed =
        action_of(tip).and_then(|action| Some((keys.row_for(action)?.help, key_of(keys, action))));
    match (ui::tooltip::words(tip), pressed) {
        (Some(words), Some((_, key))) => Some(with_key(&words, key)),
        (Some(words), None) => Some(words),
        (None, Some((help, key))) => Some(with_key(help, key)),
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

/// The condition on which a key does anything at all: the one thing about
/// the moment that decides it, so that the popup can say whether it holds
/// right now as well as what it is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum When {
    /// The one condition that is also a context: a region's names are
    /// tried before the plain ones while it holds — see
    /// [`When::context`].
    RegionSelected,
    SeveralFiles,
    Animation,
    AnimationOrPages,
    PointerOnPicture,
    PictureOnClipboard,
    HdrMode,
    SingleChannel,
    /// A rename, a deletion or a removal has been made this session and
    /// not yet undone.
    Undoable,
    /// A file was on screen before this one, and after it: what the pair
    /// at the head of the file list go back and forward to.
    VisitedBefore,
    VisitedAfter,
}

impl When {
    /// Every condition, for a test to hold them all up against the
    /// application.
    #[cfg(test)]
    pub const ALL: [When; 11] = [
        When::RegionSelected,
        When::SeveralFiles,
        When::Animation,
        When::AnimationOrPages,
        When::PointerOnPicture,
        When::PictureOnClipboard,
        When::HdrMode,
        When::SingleChannel,
        When::Undoable,
        When::VisitedBefore,
        When::VisitedAfter,
    ];

    /// The condition in a few words, as the popup's column reads it: a
    /// phrase, not a sentence.
    pub fn describe(self) -> &'static str {
        match self {
            When::RegionSelected => "a region selected",
            When::SeveralFiles => "more than one file",
            When::Animation => "an animation",
            When::AnimationOrPages => "an animation or a paged file",
            When::PointerOnPicture => "the pointer on the picture",
            When::PictureOnClipboard => "a picture on the clipboard",
            When::HdrMode => "the monitor in HDR mode",
            When::SingleChannel => "a single-channel image",
            When::Undoable => "an edit to undo",
            When::VisitedBefore => "a file shown before this one",
            When::VisitedAfter => "a file shown after this one",
        }
    }

    /// The context a line under this condition binds its names in, where
    /// it is one. Only the region's is: what the other conditions decide is
    /// whether a key does anything, not which key it is.
    pub fn context(self) -> Option<super::keymap::Context> {
        match self {
            When::RegionSelected => Some(super::keymap::Context::Region),
            _ => None,
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
    /// Whether a file was on screen before this one, and after it.
    pub visited_before: bool,
    pub visited_after: bool,
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
            visited_before: false,
            visited_after: false,
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
    /// picture up in its own colors, the dialog down, a picture on the
    /// clipboard and files seen either side of this one — the three
    /// conditions that do hold, the paste button and the pair that go
    /// back and forward being alive only then.
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
        visited_before: true,
        visited_after: true,
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
            When::SeveralFiles => self.several_files,
            When::Animation => self.animation,
            When::AnimationOrPages => self.animation || self.pages,
            When::PointerOnPicture => self.pointer_on_picture,
            When::PictureOnClipboard => self.picture_on_clipboard,
            When::HdrMode => self.hdr == Hdr::Available,
            When::SingleChannel => self.single_channel,
            When::Undoable => self.undoable,
            When::VisitedBefore => self.visited_before,
            When::VisitedAfter => self.visited_after,
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
            visited_before: self.visited_before,
            visited_after: self.visited_after,
        }
    }
}

use Action::*;
use Direction::{Down, Left, Right, Up};
use KeyName::{Char, Named, Position};
use PanStep::{Coarse, Edge, Fine};

/// A character typed with `mods` held besides the Shift it carries.
const fn typed(mods: Mods, character: char) -> Chord {
    Chord::new(mods, Char(character))
}

/// A character typed with nothing else held.
const fn key(character: char) -> Chord {
    typed(PLAIN, character)
}

const fn named(mods: Mods, key: NamedKey) -> Chord {
    Chord::new(mods, Named(key))
}

/// A key of the number row, by its place.
const fn digit(mods: Mods, code: KeyCode) -> Chord {
    Chord::new(mods, Position(code))
}

/// Four names, one for each arrow held with `$mods`: `$prefix.left` and so
/// on, running the action `$action` makes of each direction.
macro_rules! arrows {
    ($prefix:literal, $mods:expr, $action:path $(, $step:expr)?) => {
        &[
            Bound {
                name: concat!($prefix, ".left"),
                action: $action(Left $(, $step)?),
                defaults: &[named($mods, NamedKey::ArrowLeft)],
            },
            Bound {
                name: concat!($prefix, ".right"),
                action: $action(Right $(, $step)?),
                defaults: &[named($mods, NamedKey::ArrowRight)],
            },
            Bound {
                name: concat!($prefix, ".up"),
                action: $action(Up $(, $step)?),
                defaults: &[named($mods, NamedKey::ArrowUp)],
            },
            Bound {
                name: concat!($prefix, ".down"),
                action: $action(Down $(, $step)?),
                defaults: &[named($mods, NamedKey::ArrowDown)],
            },
        ]
    };
}

/// One name on a line of its own.
macro_rules! one {
    ($name:literal, $action:expr, [$($chord:expr),* $(,)?]) => {
        Keys::Bound(&[Bound {
            name: $name,
            action: $action,
            defaults: &[$($chord),*],
        }])
    };
}

/// Every line of the key table, in the order `--help` lists them, with the
/// names on each and the chords each answers to by default.
///
/// A letter is bound in one case: a capital nothing binds answers as its
/// lower case, so Caps Lock does not turn the keyboard off — see
/// [`Keymap::action_for`]. Where a capital is bound it is its own key: `A`
/// is the white point where `a` is the black.
pub static ROWS: &[Row] = &[
    // The number row is bound by position, not by what it types: the zooms
    // below 100% are the ones above it with Shift held, and which character
    // that is depends on the layout.
    Row {
        section: Section::Zoom,
        when: None,
        help: "Actual size (100%)",
        keys: one!(
            "zoom.100",
            ZoomTo(1.0),
            [digit(PLAIN, KeyCode::Digit1), digit(PLAIN, KeyCode::Digit0)]
        ),
    },
    Row {
        section: Section::Zoom,
        when: None,
        help: "200%, 400%, 800%, 1600%",
        keys: Keys::Bound(&[
            Bound {
                name: "zoom.200",
                action: ZoomTo(2.0),
                defaults: &[digit(PLAIN, KeyCode::Digit2)],
            },
            Bound {
                name: "zoom.400",
                action: ZoomTo(4.0),
                defaults: &[digit(PLAIN, KeyCode::Digit3)],
            },
            Bound {
                name: "zoom.800",
                action: ZoomTo(8.0),
                defaults: &[digit(PLAIN, KeyCode::Digit4)],
            },
            Bound {
                name: "zoom.1600",
                action: ZoomTo(16.0),
                defaults: &[digit(PLAIN, KeyCode::Digit5)],
            },
        ]),
    },
    Row {
        section: Section::Zoom,
        when: None,
        help: "50%, 25%, 10%",
        keys: Keys::Bound(&[
            Bound {
                name: "zoom.50",
                action: ZoomTo(0.5),
                defaults: &[digit(SHIFT, KeyCode::Digit2)],
            },
            Bound {
                name: "zoom.25",
                action: ZoomTo(0.25),
                defaults: &[digit(SHIFT, KeyCode::Digit3)],
            },
            Bound {
                name: "zoom.10",
                action: ZoomTo(0.1),
                defaults: &[digit(SHIFT, KeyCode::Digit4)],
            },
        ]),
    },
    Row {
        section: Section::Zoom,
        when: None,
        help: "Zoom in",
        keys: one!("zoom.in", ZoomIn, [key('+'), key('=')]),
    },
    Row {
        section: Section::Zoom,
        when: None,
        help: "Zoom out",
        keys: one!("zoom.out", ZoomOut, [key('-'), key('_')]),
    },
    // Answered on the way up — see `App::handle_key` — since held, a drag
    // zooms to a box, which the next line says.
    Row {
        section: Section::Zoom,
        when: None,
        help: "Fit the whole image, fill the window, then actual size, in turn",
        keys: one!("zoom.fit", CycleFit, [named(PLAIN, NamedKey::Space)]),
    },
    Row {
        section: Section::Zoom,
        when: None,
        help: "Zoom to the box dragged out",
        keys: Keys::Gesture("zoom.fit"),
    },
    Row {
        section: Section::Zoom,
        when: None,
        help: "Cycle the filter used above 100%: nearest, bicubic",
        keys: one!("zoom.filter", CycleUpscale, [key('p')]),
    },
    Row {
        section: Section::Zoom,
        when: None,
        help: "Pan by 64 pixels",
        keys: Keys::Bound(arrows!("pan", PLAIN, Pan, Coarse)),
    },
    // Shift belongs to the chord here, where it does not for a character:
    // an arrow is the same key whichever way it is held.
    Row {
        section: Section::Zoom,
        when: None,
        help: "Pan by one pixel",
        keys: Keys::Bound(arrows!("pan.pixel", SHIFT, Pan, Fine)),
    },
    Row {
        section: Section::Zoom,
        when: None,
        help: "Pan to the far side of the image",
        keys: Keys::Bound(arrows!("pan.edge", CTRL, Pan, Edge)),
    },
    Row {
        section: Section::Interface,
        when: None,
        help: "Toggle the interface panels",
        keys: one!("interface.toggle", ToggleInterface, [key('`')]),
    },
    Row {
        section: Section::Interface,
        when: None,
        help: "Toggle the panels, closing the map, histogram, information and file list",
        keys: one!(
            "interface.toggle-panels",
            ToggleInterfaceAndPanels,
            [key('~')]
        ),
    },
    Row {
        section: Section::Interface,
        when: None,
        help: "Toggle the minimap",
        keys: one!("interface.minimap", ToggleMinimap, [key('m')]),
    },
    Row {
        section: Section::Interface,
        when: None,
        help: "Toggle the histogram",
        keys: one!("interface.histogram", ToggleHistogram, [key('h')]),
    },
    Row {
        section: Section::Interface,
        when: None,
        help: "Toggle the file information panel",
        keys: one!("interface.info", ToggleInfo, [key('i')]),
    },
    Row {
        section: Section::Interface,
        when: None,
        help: "Toggle the grid over the image",
        keys: one!("interface.grid", ToggleGrid, [key('g')]),
    },
    Row {
        section: Section::Interface,
        when: None,
        help: "Toggle the loupe",
        keys: one!("interface.loupe", ToggleLoupe, [key('l')]),
    },
    Row {
        section: Section::Interface,
        when: None,
        help: "Cycle the loupe's magnification: 2, 4, 8, 16",
        keys: one!(
            "interface.loupe-magnification",
            CycleMagnification,
            [key('L')]
        ),
    },
    // The three that work the histogram's plot, under the key that opens it.
    Row {
        section: Section::Interface,
        when: None,
        help: "Toggle the luminance plane on the histogram",
        keys: one!("interface.luma", ToggleLuma, [key('j')]),
    },
    Row {
        section: Section::Interface,
        when: None,
        help: "Toggle the color planes on the histogram",
        keys: one!("interface.planes", TogglePlanes, [key('k')]),
    },
    Row {
        section: Section::Interface,
        when: None,
        help: "Toggle a logarithmic count axis on the histogram",
        keys: one!("interface.log-counts", ToggleLogCounts, [key('y')]),
    },
    // The same key as the two pixel copies, with nothing held: what it
    // switches is what they take away with them.
    Row {
        section: Section::Interface,
        when: None,
        help: "Cycle the pixel readout: hex, decimal, mapped",
        keys: one!("interface.pixel-format", CyclePixelFormat, [key('.')]),
    },
    Row {
        section: Section::Interface,
        when: None,
        help: "Show the keys",
        keys: one!("interface.help", ShowHelp, [key('?'), key('/')]),
    },
    Row {
        section: Section::Interface,
        when: None,
        help: "Quit",
        keys: one!("interface.quit", Quit, [key('q')]),
    },
    Row {
        section: Section::Interface,
        when: None,
        help: "Close a popup, message or region, or show the interface; else quit",
        keys: one!(
            "interface.dismiss",
            Dismiss,
            [named(PLAIN, NamedKey::Escape)]
        ),
    },
    Row {
        section: Section::Files,
        when: Some(When::SeveralFiles),
        help: "Next file",
        keys: one!(
            "files.next",
            NextFile,
            [key(']'), named(PLAIN, NamedKey::PageDown)]
        ),
    },
    Row {
        section: Section::Files,
        when: Some(When::SeveralFiles),
        help: "Previous file",
        keys: one!(
            "files.previous",
            PreviousFile,
            [key('['), named(PLAIN, NamedKey::PageUp)]
        ),
    },
    Row {
        section: Section::Files,
        when: Some(When::SeveralFiles),
        help: "Choose a file from the list",
        keys: one!("files.chooser", OpenChooser, [typed(CTRL, 'p')]),
    },
    Row {
        section: Section::Files,
        when: Some(When::SeveralFiles),
        help: "Show or hide the file list",
        keys: one!("files.list", ToggleFilmstrip, [named(PLAIN, NamedKey::Tab)]),
    },
    // The keys that step through the list, held with Alt, step through
    // the files that have been on screen instead.
    Row {
        section: Section::Files,
        when: Some(When::VisitedBefore),
        help: "Back in image history",
        keys: one!(
            "files.back",
            Back,
            [typed(ALT, '['), named(ALT, NamedKey::PageUp)]
        ),
    },
    Row {
        section: Section::Files,
        when: Some(When::VisitedAfter),
        help: "Forward in image history",
        keys: one!(
            "files.forward",
            Forward,
            [typed(ALT, ']'), named(ALT, NamedKey::PageDown)]
        ),
    },
    // The desktop's own dialog, for files and for a folder: the capital
    // carries the Shift that parts the two.
    Row {
        section: Section::Files,
        when: None,
        help: "Open image files chosen in the desktop's file dialog",
        keys: one!("files.open", OpenFiles, [typed(CTRL, 'o')]),
    },
    Row {
        section: Section::Files,
        when: None,
        help: "Open a folder chosen in the desktop's file dialog",
        keys: one!("files.open-folder", OpenFolder, [typed(CTRL, 'O')]),
    },
    // What is done to the file itself, under the keys that walk the list:
    // the two that change the disk, the one that changes the list, and the
    // one that changes them back.
    Row {
        section: Section::Files,
        when: None,
        help: "Rename the file on screen",
        keys: one!("files.rename", Rename, [named(PLAIN, NamedKey::F2)]),
    },
    Row {
        section: Section::Files,
        when: None,
        help: "Move the file on screen to the trash, and show the next",
        keys: one!("files.delete", Delete, [named(PLAIN, NamedKey::Delete)]),
    },
    Row {
        section: Section::Files,
        when: None,
        help: "Take the file on screen off the list, and show the next",
        keys: one!("files.remove", Remove, [named(PLAIN, NamedKey::Backspace)]),
    },
    Row {
        section: Section::Files,
        when: Some(When::Undoable),
        help: "Undo the last rename, deletion or removal",
        keys: one!("files.undo", Undo, [typed(CTRL, 'z')]),
    },
    Row {
        section: Section::Files,
        when: None,
        help: "Export the picture as shown to a new JPG or PNG",
        keys: one!("files.export", Export, [typed(CTRL, 'e')]),
    },
    // The region's own section: the key that puts one up, what the fit
    // does while it is, and the names that are tried before the plain ones
    // while it is — by default on the same arrows, which then move the
    // region rather than the picture.
    Row {
        section: Section::Region,
        when: None,
        help: "Select a region to draw with a drag and adjust by its handles, or remove it",
        keys: one!("region.select", ToggleRegion, [key('x')]),
    },
    Row {
        section: Section::Region,
        when: Some(When::RegionSelected),
        help: "Fit the region, fill the window with it, then the whole image, in turn",
        keys: Keys::Also("zoom.fit"),
    },
    Row {
        section: Section::Region,
        when: Some(When::RegionSelected),
        help: "Move the region, or its current handle, a pixel",
        keys: Keys::Bound(arrows!("region.move", PLAIN, MoveRegion)),
    },
    Row {
        section: Section::Region,
        when: Some(When::RegionSelected),
        help: "Grow the region that way a pixel",
        keys: Keys::Bound(arrows!("region.grow", CTRL, GrowRegion)),
    },
    Row {
        section: Section::Region,
        when: Some(When::RegionSelected),
        help: "Shrink the region that way a pixel, pulling its far side in",
        keys: Keys::Bound(arrows!("region.shrink", CTRL_SHIFT, ShrinkRegion)),
    },
    Row {
        section: Section::Clipboard,
        when: None,
        help: "Copy the name of the file on screen, without its path",
        keys: one!("clipboard.name", CopyName, [key('c')]),
    },
    Row {
        section: Section::Clipboard,
        when: None,
        help: "Copy the absolute path of the file on screen",
        keys: one!("clipboard.path", CopyPath, [key('C')]),
    },
    Row {
        section: Section::Clipboard,
        when: None,
        help: "Copy the file on screen as a URI another program can open",
        keys: one!("clipboard.uri", CopyUri, [typed(CTRL, 'C')]),
    },
    Row {
        section: Section::Clipboard,
        when: None,
        help: "Copy the image as displayed",
        keys: one!("clipboard.image", CopyImage, [typed(CTRL, 'c')]),
    },
    // The region's copy stays beside the image's, rather than in the
    // region's own section: it is a copy first, and where the two lines
    // are read together they say what one chord does either way.
    Row {
        section: Section::Clipboard,
        when: Some(When::RegionSelected),
        help: "Copy the region as displayed",
        keys: Keys::Also("clipboard.image"),
    },
    Row {
        section: Section::Clipboard,
        when: None,
        help: "Copy everything the info panel says about the file",
        keys: one!("clipboard.info", CopyMetadata, [typed(CTRL, 'i')]),
    },
    // The full stop and the greater-than are one key on most keyboards:
    // the second is the first with Shift, which the character carries.
    Row {
        section: Section::Clipboard,
        when: Some(When::PointerOnPicture),
        help: "Copy the value of the pixel under the pointer, as read out",
        keys: one!("clipboard.pixel", CopyPixelValue, [typed(CTRL, '.')]),
    },
    Row {
        section: Section::Clipboard,
        when: Some(When::PointerOnPicture),
        help: "Copy the coordinate of the pixel under the pointer, as x,y",
        keys: one!(
            "clipboard.coordinate",
            CopyPixelCoordinate,
            [typed(CTRL, '>')]
        ),
    },
    Row {
        section: Section::Clipboard,
        when: Some(When::PictureOnClipboard),
        help: "Paste an image, saved among your pictures and shown",
        keys: one!("clipboard.paste", Paste, [typed(CTRL, 'v')]),
    },
    Row {
        section: Section::Display,
        when: None,
        help: "Exposure down / up, a quarter stop",
        keys: Keys::Bound(&[
            Bound {
                name: "display.exposure.down",
                action: Exposure(-EV_STEP),
                defaults: &[key('d')],
            },
            Bound {
                name: "display.exposure.up",
                action: Exposure(EV_STEP),
                defaults: &[key('f')],
            },
        ]),
    },
    // The capitals of these two are the other handle, below.
    Row {
        section: Section::Display,
        when: None,
        help: "Black point down / up",
        keys: Keys::Bound(&[
            Bound {
                name: "display.black.down",
                action: StepBlack(-WINDOW_STEP),
                defaults: &[key('a')],
            },
            Bound {
                name: "display.black.up",
                action: StepBlack(WINDOW_STEP),
                defaults: &[key('s')],
            },
        ]),
    },
    Row {
        section: Section::Display,
        when: None,
        help: "White point down / up",
        keys: Keys::Bound(&[
            Bound {
                name: "display.white.down",
                action: StepWhite(-WINDOW_STEP),
                defaults: &[key('A')],
            },
            Bound {
                name: "display.white.up",
                action: StepWhite(WINDOW_STEP),
                defaults: &[key('S')],
            },
        ]),
    },
    Row {
        section: Section::Display,
        when: None,
        help: "Cycle the window rule: stored, full, trimmed",
        keys: one!("display.window", CycleAutoWindow, [key('e')]),
    },
    Row {
        section: Section::Display,
        when: None,
        help: "Toggle the curve on the highlights: clip, or roll off",
        keys: one!("display.tone-map", CycleToneMap, [key('t')]),
    },
    Row {
        section: Section::Display,
        when: None,
        help: "Toggle the marks on the clipped pixels: red at white, blue at black",
        keys: one!("display.marks", MarkClipped, [key('w')]),
    },
    Row {
        section: Section::Display,
        when: Some(When::HdrMode),
        help: "Toggle HDR output, where the monitor is in HDR mode",
        keys: one!("display.hdr", ToggleHdr, [key('o')]),
    },
    Row {
        section: Section::Display,
        when: Some(When::SingleChannel),
        help: "Cycle false color for single-channel images",
        keys: one!("display.colormap", CycleColormap, [key('r')]),
    },
    Row {
        section: Section::Display,
        when: None,
        help: "Reset the window, exposure and tone map",
        keys: one!("display.reset", ResetDisplay, [key('z')]),
    },
    Row {
        section: Section::Display,
        when: None,
        help: "Turn the picture a quarter counterclockwise, clockwise",
        keys: Keys::Bound(&[
            Bound {
                name: "display.turn.left",
                action: TurnLeft,
                defaults: &[key(';')],
            },
            Bound {
                name: "display.turn.right",
                action: TurnRight,
                defaults: &[key('\'')],
            },
        ]),
    },
    Row {
        section: Section::Playback,
        when: Some(When::Animation),
        help: "Play or pause an animation",
        keys: one!("playback.play", TogglePlay, [named(PLAIN, NamedKey::Enter)]),
    },
    Row {
        section: Section::Playback,
        when: Some(When::AnimationOrPages),
        help: "Next frame of an animation, or page of a file that holds several",
        keys: one!("playback.next", NextFrame, [key('n')]),
    },
    Row {
        section: Section::Playback,
        when: Some(When::AnimationOrPages),
        help: "Previous frame, or page",
        keys: one!("playback.previous", PreviousFrame, [key('N')]),
    },
];

/// The words the application has for the interface, gathered before a
/// frame: enough to compose any tooltip the pointer might rest for, without
/// the interface reaching back into the application to ask.
///
/// A handful of values rather than the application itself, since the frame
/// is drawn from the application's state while this is read. The keys and
/// the gestures are handles on the application's own, shared rather than
/// borrowed: the frame is drawn with the application borrowed mutably.
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
    /// What each key is bound to, and each gesture.
    keys: Rc<Keymap>,
    gestures: Rc<Gestures>,
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
        let keys = &*self.keys;
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
            // copies, named as the item of the menus that copies it is.
            Tip::Name => (
                vec![self.path.clone()],
                Vec::from_iter(names(keys, Tip::Control(Control::Copies(Copies::Path)))),
            ),
            // The count says which of the list is on screen. A press on it
            // opens the chooser, said the way the state's press is, with the
            // key that opens it too; under that the keys that step through
            // the list without opening anything.
            Tip::Counter => {
                let chooser = with_key(
                    "Click to choose a file from the list",
                    key_of(keys, OpenChooser),
                );
                (
                    vec![format!("File {} of {}", self.index + 1, self.count)],
                    [chooser]
                        .into_iter()
                        .chain(
                            [NextFile, PreviousFile]
                                .into_iter()
                                .filter_map(|action| hint(keys, action)),
                        )
                        .collect(),
                )
            }
            // The dot at the head of the pixel readout: what the key does to
            // it, and under that the two copies that take what it is showing
            // away with them — neither of which has a button anywhere.
            // Its name alone, the key being the first line under it.
            Tip::Control(Control::PixelFormat) => (
                vec![ui::tooltip::words(at)?],
                [
                    (ui::tooltip::PIXEL_CYCLE, CyclePixelFormat),
                    (ui::tooltip::PIXEL_COPY_VALUE, CopyPixelValue),
                    (ui::tooltip::PIXEL_COPY_COORDINATE, CopyPixelCoordinate),
                ]
                .into_iter()
                .filter_map(|(words, action)| Some(format!("{words} ({})", key_of(keys, action)?)))
                .collect(),
            ),
            // The button that hides the interface: what a plain press does,
            // and under it the key for the press that closes the floating
            // panels with it — the one thing on the button the pointer
            // cannot discover by resting on it.
            Tip::Control(Control::Maximize) => (
                vec![names(keys, at)?],
                vec![with_key(
                    ui::tooltip::MAXIMIZE_SHIFTED,
                    key_of(keys, ToggleInterfaceAndPanels),
                )],
            ),
            // The copy of the picture takes the region while one is up, as
            // the chord beside it does.
            Tip::Control(Control::Copies(Copies::Image))
                if self.conditions.met(When::RegionSelected) =>
            {
                (
                    vec![with_key(ui::tooltip::COPY_REGION, key_of(keys, CopyImage))],
                    Vec::new(),
                )
            }
            // The loupe toggle: what it does, and under it the button on
            // the mouse that holds the loupe up without it, which no key
            // table lists, and how the magnification is set — by the wheel
            // with that button held, or by its own key.
            Tip::Control(Control::Loupe) => {
                let held = self
                    .gestures
                    .held_for_loupe()
                    .map(|slot| ui::tooltip::loupe_held(&slot));
                let wheel = ui::tooltip::loupe_wheel(
                    self.gestures.wheel_for_magnification().as_deref(),
                    key_of(keys, CycleMagnification).as_deref(),
                );
                (
                    vec![names(keys, at)?],
                    held.into_iter().chain(wheel).collect(),
                )
            }
            // The exposure's slider, and under it the keys that step what
            // it sets.
            Tip::Exposure => (
                vec![names(keys, at)?],
                Vec::from_iter(hint(keys, Exposure(-EV_STEP))),
            ),
            // The two handles: what each is, and under it the pair of keys
            // that step it. The band between them slides the window, which
            // no key does, so it names itself and nothing more.
            Tip::BlackPoint => (
                vec![names(keys, at)?],
                Vec::from_iter(hint(keys, StepBlack(-WINDOW_STEP))),
            ),
            Tip::WhitePoint => (
                vec![names(keys, at)?],
                Vec::from_iter(hint(keys, StepWhite(-WINDOW_STEP))),
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
                // What the press is for while the panel is closed; what it
                // actually does while the panel is open, the press being the
                // toggle the key is.
                let does = match self.show_histogram {
                    true => "close",
                    false => "open",
                };
                (
                    self.state.clone(),
                    vec![with_key(
                        &format!("Click to {does} the histogram"),
                        key_of(keys, ToggleHistogram),
                    )],
                )
            }
            _ => (vec![names(keys, at)?], Vec::new()),
        };
        Some(ui::Tooltip { title, hints })
    }

    fn shortcut(&self, control: Control) -> Option<String> {
        key_of(&self.keys, action_of(Tip::Control(control))?)
    }

    fn help(&self) -> Vec<ui::help::Section> {
        help_sections(&self.keys, &self.gestures, &self.conditions)
    }
}

/// The key table as the help popup lays it out: one section per heading,
/// in `--help`'s order, and in each one row per line of the table, the key
/// column spelled from the chords in force, and each condition marked with
/// whether it holds under `conditions`; then the mouse, one row for each
/// gesture that does anything.
///
/// Free of the `Namer` on purpose: nothing else about the frame changes
/// what the keys are, and a test can read the whole of it without one.
pub(super) fn help_sections(
    keys: &Keymap,
    gestures: &Gestures,
    conditions: &Conditions,
) -> Vec<ui::help::Section> {
    Section::ALL
        .into_iter()
        .map(|section| ui::help::Section {
            title: section.title(),
            rows: keys
                .rows()
                .iter()
                .filter(|row| row.section == section)
                .map(|row| ui::help::Row {
                    key: keys.column(row),
                    does: row.help,
                    when: row.when.map(|when| ui::help::Condition {
                        words: when.describe(),
                        met: conditions.met(when),
                    }),
                })
                .collect(),
        })
        .chain([ui::help::Section {
            title: MOUSE,
            rows: mouse_rows(keys, gestures)
                .into_iter()
                .map(|(key, does)| ui::help::Row {
                    key,
                    does,
                    when: None,
                })
                .collect(),
        }])
        .collect()
}

/// The heading the mouse's gestures are listed under.
pub const MOUSE: &str = "Mouse";

/// Every gesture that does anything, as it is spelled and what it does: a
/// click by the words of the key it runs.
pub fn mouse_rows(keys: &Keymap, gestures: &Gestures) -> Vec<(String, &'static str)> {
    gestures
        .rows()
        .filter_map(|(spelled, behavior)| {
            let does = match behavior {
                gestures::Behavior::Click(name) => keys.help_of(name)?,
                other => other.describe()?,
            };
            Some((spelled, does))
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
    /// Which button other than the primary is down on the picture, which
    /// holds the loupe up while it is, where its hold slot says so. From the
    /// last pass, as `over_image` is, since the toolkit takes the button —
    /// see `Command::Held`.
    pub(super) held: Option<Button>,
    /// The wheel's turning toward the loupe's next magnification, in
    /// notches: a trackpad arrives in fractions of one, and they add up
    /// here until there is a whole notch to answer.
    pub(super) magnifying: f32,
    /// The same for whichever other stepper the wheel was last turning,
    /// with which one it was: turning a different one starts over.
    pub(super) notches: Option<(WheelAction, f32)>,
    /// The key that fits, while it is held: the one key answered on its
    /// way up.
    pub(super) fit_key: Option<FitKey>,
}

/// The key bound to the fit, held. Held, a drag on the picture draws a box
/// to zoom to, which is why the key fits nothing on its way down — the view
/// would move under the hand about to draw — and fits on its way up instead,
/// unless a box was drawn while it was down. The key's repeats are the same
/// press still going, and are not answered.
///
/// Kept by the place on the keyboard it was pressed at, since that is what
/// its release is sure to say again, whatever is held with it by then.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct FitKey {
    pub(super) key: PhysicalKey,
    /// Whether a box has been drawn while it was down, which is what
    /// letting go of it asks before it fits anything.
    pub(super) drawn: bool,
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

/// What the window says when a paste finds no picture on the clipboard: the
/// paste button's tooltip, as a sentence, whether the paste was a key or
/// `--paste`.
pub fn nothing_to_paste() -> String {
    format!("{}.", ui::tooltip::NOTHING_TO_PASTE)
}

impl App {
    pub(super) fn handle_key(
        &mut self,
        key: &Key,
        position: PhysicalKey,
        state: ElementState,
    ) -> Effect {
        // The fit key is answered on its way up — see `FitKey` — and its
        // release is read whatever is held with it by then, so that a chord
        // pressed while it was down cannot leave it held for good.
        if state == ElementState::Released {
            return match self.pointer.fit_key {
                Some(held) if held.key == position => self.release_fit_key(),
                _ => Effect::Nothing,
            };
        }
        let region_selected = matches!(self.marking.selection, Selection::Shown(_));
        match self
            .keys
            .action_for(key, position, self.pointer.modifiers, region_selected)
        {
            Some(CycleFit) => self.hold_fit_key(position),
            Some(action) => self.perform(action),
            None => Effect::Nothing,
        }
    }

    /// The fit key went down. Nothing moves, but the pointer over the
    /// picture changes to say what a drag would now do, which takes a frame.
    /// A repeat of a key already held is the same press still going, and
    /// changes nothing.
    fn hold_fit_key(&mut self, key: PhysicalKey) -> Effect {
        if self.pointer.fit_key.is_some() {
            return Effect::Nothing;
        }
        self.pointer.fit_key = Some(FitKey { key, drawn: false });
        Effect::Redraw
    }

    /// The fit key came up: the fit it asked for, unless a box was drawn
    /// while it was down — in which case the zoom was the box's, the key is
    /// spent, and only the pointer has to change back.
    fn release_fit_key(&mut self) -> Effect {
        match self.pointer.fit_key.take() {
            Some(FitKey { drawn: false, .. }) => self.perform(CycleFit),
            Some(FitKey { drawn: true, .. }) => Effect::Redraw,
            None => Effect::Nothing,
        }
    }

    /// The window lost the keyboard: whatever was held is not held here
    /// any more, and its release will go elsewhere.
    pub(super) fn keys_lost(&mut self) {
        self.pointer.fit_key = None;
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
            // The buttons' own presses, so that a key and the button at
            // the head of the bar or of the list cannot come to mean
            // different things.
            ToggleFilmstrip => return self.press(Control::Filmstrip),
            Back => return self.press(Control::Back),
            Forward => return self.press(Control::Forward),
            Remove => return self.press(Control::Remove),
            ShowHelp => return self.press(Control::Help),
            // A fitted image re-fits on the next frame: the viewport it is
            // measured against is the one the panels leave, and they have
            // just come or gone.
            ToggleInterface => {
                self.panels.show_ui = !self.panels.show_ui;
                // The paste button goes with the bars, and comes back
                // with them where the clipboard still holds a picture.
                let _ = self.refresh_paste();
                // A menu is part of the interface, and goes with it.
                self.close_menus();
                // The first time it goes, say how to get it back. With the
                // bars gone there is nothing left on screen that could say
                // it, and a window that has stopped answering the pointer
                // anywhere looks broken rather than tidy. Once only: after
                // that the reader knows, and a message every time would be
                // in the way of the thing they asked to see.
                if !self.panels.show_ui
                    && !self.said_how_to_restore
                    && let Some(message) = restore_message(&self.keys)
                {
                    self.said_how_to_restore = true;
                    self.toast(message, Level::Message);
                }
            }
            // The three panels float over the image rather than inside the
            // bars, and the file list keeps its rows when the bars go, so
            // hiding the interface leaves all four behind. This asks for
            // the picture on its own, and closes them on the way. They stay
            // closed when the bars come back: what the key put away, it is
            // not the key's business to bring out again.
            ToggleInterfaceAndPanels => {
                self.panels.show_minimap = false;
                self.panels.show_histogram = false;
                self.panels.show_info = false;
                self.panels.show_filmstrip = false;
                return self.perform(ToggleInterface);
            }
            ToggleHistogram => return self.press(Control::Histogram),
            ToggleLuma => return self.press(Control::Luma),
            TogglePlanes => return self.press(Control::Planes),
            ToggleLogCounts => return self.press(Control::Log),
            ToggleInfo => return self.press(Control::Info),
            ToggleMinimap => return self.press(Control::Minimap),
            ToggleGrid => return self.press(Control::Grid),
            ToggleLoupe => return self.press(Control::Loupe),
            CycleMagnification => {
                self.panels.loupe_magnification = ui::loupe::cycle(self.panels.loupe_magnification);
                return Effect::Redraw;
            }
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
            Paste => return self.paste(),
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
            // Only a region moves, grows and shrinks, and there is none: see
            // `perform_on_region`.
            MoveRegion(_) | GrowRegion(_) | ShrinkRegion(_) => return Effect::Nothing,
            TogglePlay => return self.toggle_play(),
            NextFrame => return self.step_frame(1),
            PreviousFrame => return self.step_frame(-1),
            // The menu's own items, so that the key and the item cannot
            // come to mean different things.
            Rename => return self.press(Control::Rename),
            Delete => return self.press(Control::Delete),
            TurnLeft => return self.press(Control::TurnLeft),
            TurnRight => return self.press(Control::TurnRight),
            Export => return self.press(Control::Export),
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
    /// something to it: the region's own three move its current handle a
    /// pixel — the whole of it, while that is the middle — grow it and
    /// shrink it; the fit frames it and then the picture; and the copy of
    /// the picture copies it. `None` for every other action, which is the
    /// picture's as it always was: the arrows pan under a region unless the
    /// region's names hold them, which by default they do.
    ///
    /// Nothing here is animated: a region moves by a pixel at a time, and a
    /// pixel has nothing to animate.
    fn perform_on_region(&mut self, region: Region, action: Action) -> Option<Effect> {
        let image = self.image_pixels();
        Some(match action {
            GrowRegion(direction) => {
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
            MoveRegion(direction) => {
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
        self.current
            .as_ref()
            .map_or([1, 1], crate::ui::Current::pixels)
    }

    /// A drag on the picture has taken hold of the region — or of nothing
    /// yet, to draw one, or to draw a box to zoom to — at `at`, in image
    /// pixels. A box drawn with the fit key held is what the key was held
    /// for, and the key is spent on it: letting go afterwards fits nothing.
    fn grab(&mut self, grab: Grab, at: [f32; 2]) {
        if grab == Grab::Zoom
            && let Some(FitKey { drawn, .. }) = &mut self.pointer.fit_key
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
            keys: Rc::clone(&self.keys),
            gestures: Rc::clone(&self.gestures),
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
            visited_before: self
                .visited
                .can_back(|path| self.files.position(path).is_some()),
            visited_after: self
                .visited
                .can_forward(|path| self.files.position(path).is_some()),
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
            // A button other than the primary held on the picture, and the
            // pointer with it while it is down: where its hold slot is the
            // loupe, the loupe comes up on the press, follows the hand, and
            // goes on the release — unless its toggle keeps it.
            ui::Command::Held { button, at } => {
                let was = std::mem::replace(&mut self.pointer.held, button);
                let moved = at.is_some_and(|at| self.pointer.cursor.replace(at) != Some(at));
                return Effect::redraw_if(was != button || moved);
            }
            // A click of a button on the picture: the key its slot names.
            ui::Command::Click(button) => return self.click(button),
            // The pointer through a drag on the picture, which winit has
            // stopped reporting: the loupe follows it, as the readout does.
            ui::Command::Dragging(at) => {
                return Effect::redraw_if(
                    at.is_some_and(|at| self.pointer.cursor.replace(at) != Some(at)),
                );
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
            ui::Command::Wheel {
                delta,
                notched,
                held,
            } => return self.wheel(delta, notched, held),
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
            ui::Command::ExportName(name) => self.set_export_name(name),
            ui::Command::ExportQuality(quality) => self.set_export_quality(quality),
            ui::Command::ExportSize(dimension, text) => self.set_export_size(dimension, text),
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
            // And the file list's rows, the same way.
            ui::Command::FilmstripVisible(rows) => {
                for row in rows.clone() {
                    if let Some(path) = self.filmstrip.path_at(row) {
                        self.thumbs.touch(path);
                    }
                }
                let chooser = &self.chooser;
                let wanted = self
                    .filmstrip
                    .wanted(rows, &self.thumbs, |path| chooser.given_up(path));
                self.thumbnailer.prioritize(wanted);
            }
            // The file list's edge was dragged: the picture is fitted into
            // what the wider or narrower list leaves on the next frame.
            ui::Command::FilmstripSlot(slot) => {
                self.filmstrip.set_slot(slot);
            }
        }
        Effect::Redraw
    }

    /// Puts the list under `order`, if that is a change: the file list's
    /// menus press this.
    fn set_order(&mut self, order: ui::filmstrip::Order) -> Effect {
        if !self.filmstrip.set_order(order) {
            return Effect::Nothing;
        }
        self.apply_order().also(Effect::Redraw)
    }

    /// Back to the file that was on screen before this one, or forward to
    /// the one after: asked for as a file is when it is named outright.
    /// Nothing to draw yet, as for a step: what is on screen stays until
    /// the file arrives. A file that has left the list is passed over —
    /// see `Visited`.
    fn visit(&mut self, back: bool) -> Effect {
        let files = &self.files;
        let listed = |path: &Path| files.position(path).is_some();
        let target = if back {
            self.visited.back(listed)
        } else {
            self.visited.forward(listed)
        };
        let Some(path) = target else {
            return Effect::Nothing;
        };
        if Some(path.as_path()) == self.files.shown_path() {
            // Already there: the stack moves and nothing is read.
            self.visited.arrived(&path);
            return Effect::Redraw;
        }
        if let Some(index) = self.files.position(&path) {
            let request = self.files.go_to(index);
            self.send(request);
        }
        Effect::Nothing
    }

    /// The wheel turned over the picture, by `delta` notches across and
    /// down, with `held` down on it: whatever its slot steps.
    ///
    /// A wheel's notch is a step asked for by name, and a zoom or a pan on
    /// it is animated as a key's would be; a trackpad's scroll is the hand
    /// on the view, as a drag is, and goes where the fingers put it. A wheel
    /// turned with a chord no slot names does nothing: Ctrl with the wheel
    /// is a compositor's gesture unless the configuration says otherwise.
    fn wheel(&mut self, delta: [f32; 2], notched: bool, held: Option<Button>) -> Effect {
        let Some(action) = self
            .gestures
            .wheel(Surface::Image, self.pointer.modifiers, held)
        else {
            return Effect::Nothing;
        };
        if !delta.iter().all(|each| each.is_finite()) {
            return Effect::Nothing;
        }
        let steps = delta[1];
        match action {
            WheelAction::Zoom => self.zoom_wheel(steps, notched),
            WheelAction::LoupeMagnification => self.magnify(steps),
            WheelAction::Pan => self.pan_wheel(delta, notched),
            stepper => self.step_wheel(stepper, steps),
        }
    }

    /// Zooms about the pointer by `steps` notches of the wheel.
    fn zoom_wheel(&mut self, steps: f32, notched: bool) -> Effect {
        // A trackpad emits a long tail of all but motionless events at the end
        // of a gesture, which would leave the view drifting after the finger
        // has stopped.
        if steps.abs() < 1e-3 {
            return Effect::Nothing;
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
        Effect::Redraw
    }

    /// Pans by `delta` notches, each as far as a trackpad scrolls for one,
    /// the picture following the wheel as it follows a drag.
    fn pan_wheel(&mut self, delta: [f32; 2], notched: bool) -> Effect {
        if delta.iter().all(|each| each.abs() < 1e-3) {
            return Effect::Nothing;
        }
        let by = ui::WHEEL_PIXELS_PER_STEP * self.scale_factor();
        let [dx, dy] = [delta[0] * by, delta[1] * by];
        if notched {
            self.animate(|view, image, viewport| view.pan_by(-dx, -dy, image, viewport));
        } else {
            let (image, viewport) = (self.image_size(), self.viewport());
            self.view.pan_by(-dx, -dy, image, viewport);
        }
        Effect::Redraw
    }

    /// The wheel on the loupe's magnification: a notch at a time up or down
    /// [`ui::loupe::MAGNIFICATIONS`], the way the wheel steps the zoom. A
    /// trackpad's fractions of a notch add up until there is one. Nothing at
    /// either end, and no frame owed for it.
    fn magnify(&mut self, steps: f32) -> Effect {
        self.pointer.magnifying += steps;
        let mut changed = false;
        while self.pointer.magnifying.abs() >= 1.0 {
            let up = self.pointer.magnifying > 0.0;
            self.pointer.magnifying -= if up { 1.0 } else { -1.0 };
            let was = self.panels.loupe_magnification;
            self.panels.loupe_magnification = ui::loupe::step(was, up);
            changed |= self.panels.loupe_magnification != was;
        }
        Effect::redraw_if(changed)
    }

    /// The wheel on one of the things a key steps: the key's own action,
    /// once for each whole notch. A trackpad's fractions add up until there
    /// is one, and the count starts over when the wheel turns to stepping
    /// something else. Up is more exposure, a higher black or white point,
    /// and the file or frame before.
    fn step_wheel(&mut self, stepper: WheelAction, steps: f32) -> Effect {
        let mut turned = match self.pointer.notches {
            Some((was, turned)) if was == stepper => turned,
            _ => 0.0,
        } + steps;
        let mut effect = Effect::Nothing;
        while turned.abs() >= 1.0 {
            let up = turned > 0.0;
            turned -= if up { 1.0 } else { -1.0 };
            let sign = if up { 1.0 } else { -1.0 };
            let action = match stepper {
                WheelAction::Exposure => Exposure(sign * EV_STEP),
                WheelAction::BlackPoint => StepBlack(sign * WINDOW_STEP),
                WheelAction::WhitePoint => StepWhite(sign * WINDOW_STEP),
                WheelAction::Files if up => PreviousFile,
                WheelAction::Files => NextFile,
                WheelAction::Frames if up => PreviousFrame,
                WheelAction::Frames => NextFrame,
                WheelAction::Zoom | WheelAction::LoupeMagnification | WheelAction::Pan => {
                    unreachable!("{stepper:?} is not a stepper")
                }
            };
            effect = effect.also(self.perform(action));
        }
        self.pointer.notches = Some((stepper, turned));
        effect
    }

    /// A button clicked on the picture: the action of the key its slot
    /// names, as though the key had been pressed. A slot naming no key, or
    /// no slot at all, is nothing.
    fn click(&mut self, button: Button) -> Effect {
        let action = self
            .gestures
            .click(Surface::Image, self.pointer.modifiers, button)
            .and_then(|name| self.keys.action_named(name));
        match action {
            Some(action) => self.perform(action),
            None => Effect::Nothing,
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
        let turn = current.turn;
        let said = match region {
            Some(_) => "Copied region.",
            None => "Copied image.",
        };
        let region = region.unwrap_or_else(|| Region::whole(current.pixels()));
        self.copying.spawn(move |ticket| {
            let (width, height) = (region.width, region.height);

            let walked = Instant::now();
            let raster = encode::displayed(&image, &display, turn, region, lift.as_deref());
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
                Ok(()) => ticket.report(Ok(Done::Copied(said))),
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
    ///
    /// Owes a frame only for the message that there was nothing to paste:
    /// what was pasted arrives through the loader, which asks for its own.
    fn paste(&mut self) -> Effect {
        let offer = match clipboard::offered_image() {
            Ok(Some(offer)) => offer,
            // Not a failure: a key was pressed and there was nothing there,
            // said in the words the paste button's tooltip says it in.
            Ok(None) => {
                self.toast(nothing_to_paste(), Level::Message);
                return Effect::Redraw;
            }
            Err(error) => {
                report(&error);
                return Effect::Nothing;
            }
        };
        let path = match pasted::reserve(offer.extension) {
            Ok(path) => path,
            Err(error) => {
                report(&error);
                return Effect::Nothing;
            }
        };
        // A paste is the window's own doing: from here on nothing showing
        // is the window's to answer, not the command line's.
        self.from_command_line = false;
        let request = self.files.adopt(path, Source::Clipboard(offer.mime));
        self.send(request);
        self.list_changed();
        Effect::Nothing
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
            Control::Loupe => {
                self.panels.show_loupe = !self.panels.show_loupe;
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
            // The seven buttons that open a menu: the menu is egui's, and
            // opens itself on the press, so there is nothing here to do.
            Control::Zoom
            | Control::PixelFormat
            | Control::Copy
            | Control::OpenIn
            | Control::FileMenu
            | Control::Sorting => Effect::Nothing,
            // The file list: up or down, and scrolled to the file on
            // screen as it comes up.
            Control::Filmstrip => {
                self.panels.show_filmstrip = !self.panels.show_filmstrip;
                self.filmstrip.reveal();
                Effect::Redraw
            }
            Control::SortBy(sort) => {
                let mut order = self.filmstrip.order();
                order.sort = sort;
                self.set_order(order)
            }
            Control::SortDirection(direction) => {
                let mut order = self.filmstrip.order();
                order.direction = direction;
                self.set_order(order)
            }
            Control::Back => self.visit(true),
            Control::Forward => self.visit(false),
            Control::Remove => {
                self.remove_shown();
                Effect::Redraw
            }
            // A row of the file list: the file it shows, asked for as a
            // row of the chooser is — by its path, the list being free
            // to have moved under the frame.
            Control::Thumb(row) => {
                let chosen = self.filmstrip.path_at(row).map(Path::to_path_buf);
                if let Some(path) = chosen
                    && Some(path.as_path()) != self.files.shown_path()
                    && let Some(index) = self.files.position(&path)
                {
                    let request = self.files.go_to(index);
                    self.send(request);
                }
                Effect::Redraw
            }
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
            Control::TurnLeft => self.turn_picture(false),
            Control::TurnRight => self.turn_picture(true),
            Control::Export => {
                self.open_export();
                Effect::Redraw
            }
            Control::ExportAs(format) => {
                self.set_export_format(format);
                Effect::Redraw
            }
            Control::ExportTo => {
                self.export_shown();
                Effect::Redraw
            }
            Control::CancelExport => {
                self.cancel_export();
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
            Control::Paste => self.paste().also(Effect::Redraw),
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
        let sample = current.sample(at[0], at[1])?;
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

    /// The free readings of the table, over the default keys.
    fn names(tip: Tip) -> Option<String> {
        super::names(&Keymap::default(), tip)
    }

    fn hint(action: Action) -> Option<String> {
        super::hint(&Keymap::default(), action)
    }

    /// What a key asks for with no region selected, at the default keys.
    fn action_for(key: &Key, position: PhysicalKey, mods: Mods) -> Option<Action> {
        Keymap::default().action_for(key, position, mods, false)
    }

    /// The same with a region selected.
    fn with_region(key: &Key, position: PhysicalKey, mods: Mods) -> Option<Action> {
        Keymap::default().action_for(key, position, mods, true)
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
        assert_eq!(
            named(Control::Delete).as_deref(),
            Some("Trash the current file (Del)")
        );
        // The key that takes a file off the list, and the pair at the head
        // of the list by the chords that do the same.
        assert_eq!(
            named(Control::Remove).as_deref(),
            Some("Remove the current file from the file list (\u{232b})")
        );
        assert_eq!(
            named(Control::Back).as_deref(),
            Some("Back in image history (Alt+[, Alt+Page Up)")
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
            keys: Rc::new(Keymap::default()),
            gestures: Rc::new(Gestures::default()),
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
            keys: Rc::new(Keymap::default()),
            gestures: Rc::new(Gestures::default()),
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
        let keys = Keymap::default();
        let gestures = Gestures::default();
        let sections = help_sections(&keys, &gestures, &Conditions::default());
        assert_eq!(sections.len(), Section::ALL.len() + 1);
        let rows: Vec<&ui::help::Row> = sections[..Section::ALL.len()]
            .iter()
            .flat_map(|section| section.rows.iter())
            .collect();
        assert_eq!(rows.len(), ROWS.len());
        for (row, line) in rows.iter().zip(ROWS) {
            assert_eq!(row.key, keys.column(line));
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
        assert_eq!(mouse.rows.len(), 9);
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
        let mut keys = Keymap::default();
        let chord = |token| super::super::keymap::Chord::read(token).unwrap();
        keys.bind("files.undo", vec![chord("ctrl+e")]).unwrap();
        let sections = help_sections(&keys, &Gestures::default(), &Conditions::default());
        let key = |does: &str| {
            sections
                .iter()
                .flat_map(|section| &section.rows)
                .find(|row| row.does == does)
                .map(|row| row.key.clone())
        };
        assert_eq!(
            key("Undo the last rename, deletion or removal").as_deref(),
            Some("Ctrl+E")
        );
        assert_eq!(
            key("Export the picture as shown to a new JPG or PNG").as_deref(),
            Some("")
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
        let mut keys = Keymap::default();
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
            keys: Rc::new(Keymap::default()),
            gestures: Rc::new(Gestures::default()),
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
            ["White point down / up (Shift+A, Shift+S)"]
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
            keys: Rc::new(Keymap::default()),
            gestures: Rc::new(Gestures::default()),
            conditions: Conditions::ALIVE,
        };
        let tooltip = namer
            .tooltip(Tip::Control(Control::PixelFormat))
            .expect("named");
        assert_eq!(tooltip.title, ["Pixel options"]);
        assert_eq!(
            tooltip.hints,
            [
                "Cycle pixel format: hex, decimal, mapped (.)",
                "Copy pixel value under pointer (Ctrl+.)",
                "Copy coordinate of pixel under pointer as x,y (Ctrl+>)",
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
            Some("Copy the absolute path of the current file (Shift+C)")
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
            keys: Rc::new(Keymap::default()),
            gestures: Rc::new(Gestures::default()),
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
            assert!(
                Keymap::default().row_for(action).is_some(),
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
            keys: Rc::new(Keymap::default()),
            gestures: Rc::new(Gestures::default()),
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
        let keys = Keymap::default();
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
}
