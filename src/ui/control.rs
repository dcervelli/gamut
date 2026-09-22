//! What the interface can be pressed on, and what a pass of it hands back.
//!
//! The interface never acts on the application: it says what was pressed,
//! and the application does it through the same dispatch the keys go
//! through, so a button and a key cannot come to mean different things. That
//! is also what lets a test drive the interface with no application behind
//! it and read off what it asked for.

use std::ops::Range;

use crate::image::region::{Grip, Region};

use super::chooser::Step;
use super::help;
use super::info::Copyable;
use super::menu::{Copies, ZoomChoice};
use super::pixel::PixelFormat;
use super::tooltip::{Tip, Tooltip};

/// Something in the interface that can be pressed: a toggle in a side
/// strip, a button in one of the bars, a cell of a menu.
#[derive(Clone, Copy, PartialEq, Debug)]
#[allow(
    dead_code,
    reason = "the information panel's rows arrive as it moves over"
)]
pub enum Control {
    /// The two buttons at the head of the top bar, which step back and on
    /// through the file list. On screen only while there is more than one
    /// file.
    Previous,
    Next,
    Minimap,
    /// The button that opens the menu of copies, at the top of the left
    /// strip.
    Copy,
    /// The button under it, which opens the menu of the other programs that
    /// can open this file. Drawn dead where nothing offers to — see
    /// [`FrameInput::openers`](super::FrameInput::openers).
    OpenIn,
    /// An item of that menu, by its place in that list.
    Opener(usize),
    /// The transport bar's buttons, on screen only for a file of frames or
    /// pages: play or pause, and one frame or page back or on. `Seek` is a
    /// press on the timeline, at a frame.
    Play,
    StepBack,
    StepForward,
    Seek(usize),
    /// The button that pastes the picture on the clipboard. On screen only
    /// while there is one — see [`Panels::paste`](super::Panels::paste).
    Paste,
    /// The button under those that starts a region: lit while one is being
    /// asked for or is on screen, and a press while it is lit takes the
    /// region off — see [`Selection`].
    Region,
    Histogram,
    Info,
    Grid,
    Zoom,
    /// The button at the end of the top bar that gives the picture the whole
    /// window. Not a toggle: what it hides includes the button itself, so
    /// there is no state for it to be showing and no press of it that puts
    /// the interface back — see [`crate::app::input::Action::Dismiss`].
    Maximize,
    /// The two plane toggles, the switch between a linear and a logarithmic
    /// count axis, and the button that puts the rendering back, down the left
    /// of the histogram panel.
    Luma,
    Planes,
    Log,
    Reset,
    /// The toggle beside the band at the foot of that strip, which paints
    /// the clipped pixels on the picture in the two colors the plot's
    /// corners count.
    Marks,
    /// One of the false colors offered under that panel's ramp, by its place
    /// in [`crate::image::display::Colormap::ALL`].
    Ramp(usize),
    /// The two steps of the exposure row under that ramp, a quarter of a stop
    /// each — see [`crate::image::display::EV_STEP`].
    /// One of the windows the row below those offers, by its place in
    /// [`super::histogram::WINDOWS`]. They set a window rather than showing
    /// which one is in force: the handles on the band are what say that.
    Window(usize),
    /// One of the two choices for the curve in the row under that, by its
    /// place in [`crate::image::display::ToneMap::ALL`].
    Curve(usize),
    /// The switch at the end of the bottom bar between the SDR and the HDR
    /// surface.
    Output,
    /// The button at the foot of the right strip, which opens the help
    /// popup — every key, what it does and when — and closes it while it
    /// is up. `?` and `/` do the same.
    Help,
    /// The dot at the head of the pixel readout, at the other end of that
    /// bar, which opens the menu of ways to write a pixel's value.
    PixelFormat,
    /// The cross on the message at the foot of the content area, which takes
    /// it off. On screen only while there is a message.
    Dismiss,
    /// A cell of the zoom menu: a zoom to go to, a fit, or a filter.
    ZoomTo(ZoomChoice),
    /// A cell of the pixel-format menu.
    Format(PixelFormat),
    /// An item of the menu of copies.
    Copies(Copies),
    /// A row of the information panel, or the button above the column that
    /// takes the whole of it.
    Facts(Copyable),
    /// The file chooser: opened by `Ctrl+P`, and closed by the same key
    /// pressed again while it is up — which arrives through here from the
    /// popup itself, since the field in it has the keyboard.
    Chooser,
    /// A row of the chooser, by its place in the list the same frame was
    /// drawn from: the file to open.
    Choose(usize),
    /// The button before the file's name in the top bar, which opens the
    /// menu of what can be done to the file itself: its name and path
    /// copied, and the file renamed or moved to the trash.
    FileMenu,
    /// The item of that menu that opens the rename dialog, and the item
    /// that moves the file to the trash. `F2` and `Delete` come through
    /// here too.
    Rename,
    Delete,
    /// The rename dialog's two buttons: the rename itself, which `Enter`
    /// also asks for, and putting the dialog away, which `Esc` and a click
    /// outside it also do.
    RenameTo,
    CancelRename,
    /// The buttons in the middle of an empty window that put up the
    /// desktop's file dialog, for image files and for a folder — see
    /// `ui::empty`. `Ctrl+O` and `Ctrl+Shift+O` come through here too,
    /// from anywhere. Dead while the dialog is up.
    OpenFiles,
    OpenFolder,
}

impl Control {
    /// What the control is called to something that cannot see it: the
    /// accessibility tree, and the tests that drive the interface through
    /// it. A name rather than a description, and stable — the tooltip is
    /// where the words are.
    pub fn label(self) -> String {
        match self {
            Control::Previous => "Previous file".to_string(),
            Control::Next => "Next file".to_string(),
            Control::Minimap => "Minimap".to_string(),
            Control::Copy => "Copy".to_string(),
            Control::OpenIn => "Open in".to_string(),
            Control::Opener(index) => format!("Open in application {index}"),
            Control::Play => "Play".to_string(),
            Control::StepBack => "Previous frame".to_string(),
            Control::StepForward => "Next frame".to_string(),
            Control::Seek(frame) => format!("Frame {frame}"),
            Control::Paste => "Paste".to_string(),
            Control::Region => "Region".to_string(),
            Control::Histogram => "Histogram".to_string(),
            Control::Info => "Information".to_string(),
            Control::Grid => "Grid".to_string(),
            Control::Zoom => "Zoom".to_string(),
            Control::Maximize => "Maximize".to_string(),
            Control::Luma => "Luminance plane".to_string(),
            Control::Planes => "Color planes".to_string(),
            Control::Log => "Logarithmic counts".to_string(),
            Control::Marks => "Clipped pixels".to_string(),
            Control::Reset => "Reset".to_string(),
            Control::Ramp(index) => format!("False color {index}"),
            Control::Window(index) => format!("Window {index}"),
            Control::Curve(index) => format!("Curve {index}"),
            Control::Output => "HDR".to_string(),
            Control::Help => "Help".to_string(),
            Control::PixelFormat => "Pixel format".to_string(),
            Control::Dismiss => "Dismiss".to_string(),
            Control::ZoomTo(choice) => choice.label(),
            Control::Format(format) => format.label().to_string(),
            Control::Copies(copies) => copies.label().to_string(),
            Control::Facts(Copyable::All) => "Copy All".to_string(),
            Control::Facts(Copyable::Section(index)) => format!("Copy section {index}"),
            Control::Facts(Copyable::Fact(index)) => format!("Copy field {index}"),
            Control::Chooser => "Choose a file".to_string(),
            Control::Choose(index) => format!("Choose file {}", index + 1),
            Control::FileMenu => "File".to_string(),
            Control::Rename => "Rename".to_string(),
            Control::Delete => "Delete".to_string(),
            Control::RenameTo => "OK".to_string(),
            Control::CancelRename => "Cancel".to_string(),
            Control::OpenFiles => "Open files".to_string(),
            Control::OpenFolder => "Open folder".to_string(),
        }
    }
}

/// What one pass of the interface asked the application for.
///
/// Not `Copy`: the chooser's query is a string, and a command that carries
/// one is cloned where it has to be.
#[derive(Clone, PartialEq, Debug)]
#[allow(
    dead_code,
    reason = "the picture's own gestures arrive as the panels move over"
)]
pub enum Command {
    /// A control was pressed.
    Press(Control),
    /// The picture was dragged this far, in physical pixels: the hand is on
    /// the view, and it goes exactly where it is put.
    Drag([f32; 2]),
    /// The wheel turned over the picture. A wheel's notch is a step asked
    /// for by name and is animated as a key's would be; a trackpad's scroll
    /// is the hand on the view, and goes where the fingers put it.
    Wheel {
        steps: f32,
        notched: bool,
    },
    /// The hand is on one of the histogram band's handles: the value, on
    /// the image's own linear scale, that is to come out black, or white.
    /// Said on every frame of the drag, the value being where the hand is
    /// now. Each lands in its own end of the window — `Display::put_black`,
    /// `Display::put_white` — with the exposure left alone.
    BlackPoint(f32),
    WhitePoint(f32),
    /// The hand is on the band between them, and both are to move: the
    /// window slid along the axis, its width kept.
    Slide {
        black: f32,
        white: f32,
    },
    /// The hand is on the exposure's slider, and this is the exposure it
    /// asks for, in stops.
    Exposure(f32),
    /// The hand is on the minimap, at `at` in image pixels, which is the
    /// point to put in the middle of the window. Said on every frame the
    /// button is down on the map, from the press on, so the marker follows
    /// the hand rather than waiting for the button to come up.
    Center([f32; 2]),
    /// Whether the pointer was over the picture with nothing of the
    /// interface between, which is what the bar's pixel readout asks.
    OverImage(bool),
    /// A drag on the picture began that is the region's rather than the
    /// view's: a new region while one was being asked for, or a hold on the
    /// one on screen. `at` is where the button went down, in image pixels —
    /// the press, not wherever the pointer had got to by the time the
    /// toolkit decided it was a drag.
    Grab {
        grab: Grab,
        at: [f32; 2],
    },
    /// Where the hand is now, in image pixels, on each frame of that drag.
    /// Carried here because the application's own pointer stops moving
    /// while the toolkit holds a drag.
    Pull([f32; 2]),
    /// That drag ended: the button came up, or the toolkit let go of it.
    Release,
    /// Which handle of the region the pointer is resting on, said on every
    /// pass a region is on screen: what the region's words are written for.
    OverGrip(Option<Grip>),
    /// A handle of the region was clicked, and is the current one now: the
    /// one the arrows move. A drag on a handle says the same through
    /// [`Command::Grab`].
    Handle(Grip),
    /// The chooser's field changed: this is what it now says.
    Query(String),
    /// The rename dialog's field changed: this is the name it now holds.
    Name(String),
    /// A key moved the chooser's cursor.
    Cursor(Step),
    /// Which of the chooser's rows are on screen, said when it changes, so
    /// that their thumbnails can be asked for ahead of the rest.
    Visible(Range<usize>),
}

/// What a region on the picture is in: nothing, waiting for the drag that
/// draws one, or drawn.
///
/// The interface reads it — the button is lit for the last two, the drag
/// means something different in each — and the application holds it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Selection {
    #[default]
    Off,
    /// Asked for, and the next drag on the picture draws it.
    Armed,
    Shown(Region),
}

impl Selection {
    /// The region on screen, if there is one.
    pub fn region(self) -> Option<Region> {
        match self {
            Selection::Shown(region) => Some(region),
            Selection::Off | Selection::Armed => None,
        }
    }

    /// Whether the button is lit: a region asked for or drawn.
    pub fn is_on(self) -> bool {
        self != Selection::Off
    }
}

/// What a drag on the picture has hold of, when it is not the view's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grab {
    /// Drawing a new region from the press outward.
    New,
    /// A handle of the region on screen, or the whole of it.
    Handle(Grip),
    /// Drawing a box to zoom to, from the press outward: the drag `Space`
    /// is held for.
    Zoom,
}

/// The words the application has for the interface: what a thing is called
/// when the pointer rests on it, and what key does the same job.
///
/// A trait rather than the application itself because the interface is
/// below the application and may not reach up into it — and because a test
/// drives the interface with no application behind it at all. Most of a
/// tooltip is the key that does the same job, and the keys are the
/// application's, so composing one is its business: see `App::namer`.
pub trait Naming {
    /// What to say about `tip` when the pointer rests on it, or `None` for
    /// a thing with nothing to say.
    fn tooltip(&self, tip: Tip) -> Option<Tooltip>;

    /// What to press for `control`, as the key table writes it, where a key
    /// does the same job: what a menu prints beside an item.
    fn shortcut(&self, control: Control) -> Option<String>;

    /// Every key there is, in sections, for the help popup to lay out.
    fn help(&self) -> Vec<help::Section>;
}

/// The interface with nothing to say: for the tests that drive it and read
/// off what it asked for, which is not the words on it.
#[cfg(test)]
pub struct Unnamed;

#[cfg(test)]
impl Naming for Unnamed {
    fn tooltip(&self, _: Tip) -> Option<Tooltip> {
        None
    }

    fn shortcut(&self, _: Control) -> Option<String> {
        None
    }

    fn help(&self) -> Vec<help::Section> {
        Vec::new()
    }
}

#[cfg(test)]
impl Control {
    /// Every control, one of each kind — the ones that carry a payload by a
    /// representative, the first cell or the first row — for the tests that
    /// ask something of every button. Kept complete by [`Control::listed`].
    pub const ALL: &[Control] = &[
        Control::Previous,
        Control::Next,
        Control::Minimap,
        Control::Copy,
        Control::OpenIn,
        Control::Opener(0),
        Control::Play,
        Control::StepBack,
        Control::StepForward,
        Control::Seek(0),
        Control::Paste,
        Control::Region,
        Control::Histogram,
        Control::Info,
        Control::Grid,
        Control::Zoom,
        Control::Maximize,
        Control::Luma,
        Control::Planes,
        Control::Log,
        Control::Reset,
        Control::Marks,
        Control::Ramp(0),
        Control::Window(0),
        Control::Curve(0),
        Control::Output,
        Control::Help,
        Control::PixelFormat,
        Control::Dismiss,
        Control::ZoomTo(ZoomChoice::Scale(1.0)),
        Control::Format(PixelFormat::Hex),
        Control::Copies(Copies::Name),
        Control::Facts(Copyable::All),
        Control::Chooser,
        Control::Choose(0),
        Control::FileMenu,
        Control::Rename,
        Control::Delete,
        Control::RenameTo,
        Control::CancelRename,
        Control::OpenFiles,
        Control::OpenFolder,
    ];

    /// Whether `control` is a kind [`Control::ALL`] lists — which is every
    /// kind, and the compiler is what holds it to that: a variant added to
    /// the enum is missing from the match below until it is added here, and
    /// then to `ALL`.
    fn listed(control: Control) -> bool {
        match control {
            Control::Previous
            | Control::Next
            | Control::Minimap
            | Control::Copy
            | Control::OpenIn
            | Control::Opener(_)
            | Control::Play
            | Control::StepBack
            | Control::StepForward
            | Control::Seek(_)
            | Control::Paste
            | Control::Region
            | Control::Histogram
            | Control::Info
            | Control::Grid
            | Control::Zoom
            | Control::Maximize
            | Control::Luma
            | Control::Planes
            | Control::Log
            | Control::Reset
            | Control::Marks
            | Control::Ramp(_)
            | Control::Window(_)
            | Control::Curve(_)
            | Control::Output
            | Control::Help
            | Control::PixelFormat
            | Control::Dismiss
            | Control::ZoomTo(_)
            | Control::Format(_)
            | Control::Copies(_)
            | Control::Facts(_)
            | Control::Chooser
            | Control::Choose(_)
            | Control::FileMenu
            | Control::Rename
            | Control::Delete
            | Control::RenameTo
            | Control::CancelRename
            | Control::OpenFiles
            | Control::OpenFolder => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ALL` has one of every kind of control, and no kind twice.
    #[test]
    fn every_kind_of_control_is_listed_once() {
        let kinds: Vec<_> = Control::ALL.iter().map(std::mem::discriminant).collect();
        for (index, kind) in kinds.iter().enumerate() {
            assert!(Control::listed(Control::ALL[index]));
            assert!(
                !kinds[..index].contains(kind),
                "{:?} is listed twice",
                Control::ALL[index]
            );
        }
    }
}
