//! What the interface can be pressed on, and what a pass of it hands back.
//!
//! The interface never acts on the application: it says what was pressed,
//! and the application does it through the same dispatch the keys go
//! through, so a button and a key cannot come to mean different things. That
//! is also what lets a test drive the interface with no application behind
//! it and read off what it asked for.

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
    OpenWith,
    /// An item of that menu, by its place in that list.
    OpenIn(usize),
    /// The button that pastes the picture on the clipboard. On screen only
    /// while there is one — see [`Panels::paste`](super::Panels::paste).
    Paste,
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
    /// One of the false colors offered under that panel's ramp, by its place
    /// in [`crate::image::display::Colormap::ALL`].
    Ramp(usize),
    /// The two steps of the exposure row under that ramp, a quarter of a stop
    /// each — see [`super::histogram::EV_STEP`].
    ExposureDown,
    ExposureUp,
    /// One of the windows the row below those offers, by its place in
    /// [`super::histogram::WINDOWS`]. They set a window rather than showing
    /// which one is in force: the line above them is what says that.
    Window(usize),
    /// The four nudges at the end of that line, which move the window the
    /// user has rather than putting them on a new one: along the axis either
    /// way, and narrower or wider about its own middle.
    WindowDown,
    WindowUp,
    WindowNarrow,
    WindowWiden,
    /// One of the tone curves in the row under that, by its place in
    /// [`crate::image::display::ToneMap::ALL`].
    Curve(usize),
    /// The switch at the end of the bottom bar between the SDR and the HDR
    /// surface.
    Output,
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
            Control::OpenWith => "Open with".to_string(),
            Control::OpenIn(index) => format!("Open in application {index}"),
            Control::Paste => "Paste".to_string(),
            Control::Histogram => "Histogram".to_string(),
            Control::Info => "Information".to_string(),
            Control::Grid => "Grid".to_string(),
            Control::Zoom => "Zoom".to_string(),
            Control::Maximize => "Maximize".to_string(),
            Control::Luma => "Luminance plane".to_string(),
            Control::Planes => "Color planes".to_string(),
            Control::Log => "Logarithmic counts".to_string(),
            Control::Reset => "Reset".to_string(),
            Control::Ramp(index) => format!("False color {index}"),
            Control::ExposureDown => "Exposure down".to_string(),
            Control::ExposureUp => "Exposure up".to_string(),
            Control::Window(index) => format!("Window {index}"),
            Control::WindowDown => "Slide the window down".to_string(),
            Control::WindowUp => "Slide the window up".to_string(),
            Control::WindowNarrow => "Narrow the window".to_string(),
            Control::WindowWiden => "Widen the window".to_string(),
            Control::Curve(index) => format!("Curve {index}"),
            Control::Output => "HDR".to_string(),
            Control::PixelFormat => "Pixel format".to_string(),
            Control::Dismiss => "Dismiss".to_string(),
            Control::ZoomTo(choice) => choice.label(),
            Control::Format(format) => format.label().to_string(),
            Control::Copies(copies) => copies.label().to_string(),
            Control::Facts(Copyable::All) => "Copy All".to_string(),
            Control::Facts(Copyable::Section(index)) => format!("Copy section {index}"),
            Control::Facts(Copyable::Fact(index)) => format!("Copy field {index}"),
        }
    }
}

/// What one pass of the interface asked the application for.
#[derive(Clone, Copy, PartialEq, Debug)]
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
    Wheel { steps: f32, notched: bool },
    /// Whether the pointer was over the picture with nothing of the
    /// interface between, which is what the bar's pixel readout asks.
    OverImage(bool),
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
}
