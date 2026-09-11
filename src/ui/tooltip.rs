//! The label that names what the pointer is resting on: what it says.
//!
//! When it appears and where it goes are egui's, which opens one when the
//! pointer has rested on a control for long enough and places it clear of
//! the thing it names. [`Tooltip`] is what one says, which the application
//! composes because most of a tooltip is the key that does the same thing
//! and the keys are the application's — see [`Naming`](super::Naming). A
//! thing earns a tooltip by becoming a [`Tip`] the application has something
//! to say about.

use crate::image::display::{AutoWindow, Colormap, ToneMap};
use crate::theme::Theme;

use super::control::Control;
use super::histogram::{EV_STEP, WINDOWS, stops_label};
use super::{Room, menu};

/// Something in the interface that names itself when the pointer rests on it.
///
/// A widget is one; so is a run of words in a bar that has more to say than
/// there is room for. What the pointer is answered with, so anything that can
/// be pointed at can become one.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tip {
    /// A button, a toggle, or a cell of a menu.
    Control(Control),
    /// The file's own name in the top bar, which is cut to the room the bar
    /// has and stands for a path that is usually longer.
    Name,
    /// The count of files beside it.
    Counter,
    /// The timeline in the transport bar, which a press or a drag scrubs.
    Timeline,
    /// The words at the end of the bottom bar that say what is being done to
    /// the picture. What they say in the room a bar has is the names of the
    /// things in force; the tooltip is the whole of it in sentences, which is
    /// where the window's own bounds went — see `App::tooltip`.
    State,
}

/// What a tooltip says: the thing itself first, and under it the keys that do
/// the same job.
///
/// Composed by the application rather than here, because almost every line of
/// one comes out of the key table — a tooltip and `--help` should never be
/// able to disagree about which key does what.
pub struct Tooltip {
    /// What names the thing. One line for almost everything; the words at the
    /// end of the bottom bar are about several things at once, and each of
    /// them is a line, since a paragraph of them would be read as prose
    /// rather than as a list of what is in force.
    pub title: Vec<String>,
    /// The lines under those, set dimmer: what to press instead.
    pub hints: Vec<String>,
}

/// What a toggle says when the content area has no room for the panel it
/// opens, in place of the name of the panel.
///
/// The button is drawn dead and the press is refused, so naming the panel
/// and the key beside it
/// would be describing something that is not going to happen. Said as a
/// sentence rather than as a label because it is a reason and not a name.
pub const NO_ROOM: &str = "Disabled because display is too small.";

/// What the open button says where nothing on the desktop offers to open the
/// file on screen.
///
/// A sentence rather than a name, for the same reason as [`NO_ROOM`]: the
/// button is drawn dead and the press is refused, so what it owes the reader
/// is the reason. Said of this file rather than of the desktop, since the
/// next file along may well have somewhere to go.
pub const NOTHING_OPENS_IT: &str = "No other application offers to open this file.";

/// What the surface switch says on a monitor that is not in HDR mode, and
/// under it the one thing that would change the answer.
///
/// The request is a start-up decision because it is the one that a
/// compositor may answer with a modeset — see `App::toggle_hdr` and
/// [`crate::render::HdrPreference`] — so it is named as something to restart
/// with rather than offered as a press.
pub const NOT_HDR_MODE: &str = "HDR disabled because monitor is not in HDR mode.";
/// The line under [`NOT_HDR_MODE`].
pub const REQUEST_HDR_MODE: &str = "Restart with --output hdr to request mode set.";

/// What the switch says where the driver offers no HDR color space for this
/// window at all. Nothing to advise under it: `--output hdr` would come back
/// to the sRGB surface exactly as it has.
pub const NO_HDR_OUTPUT: &str = "HDR disabled because no HDR output is available.";

/// Whether the surface switch has anything to switch, and where it has not,
/// which of the two reasons — worked out by `App::hdr_state`, since both
/// halves of the answer are the application's: what the driver offers for
/// the window, and what the compositor says the monitor is in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hdr {
    /// A press does something.
    Available,
    /// An HDR color space is offered, but the monitor is not in HDR mode —
    /// or the compositor has not yet said which monitor the window is on,
    /// which is the same answer until it does.
    NotInHdrMode,
    /// No HDR color space is offered for this window.
    Unsupported,
}

/// Why a dead control is dead.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Refused {
    /// The sentence that stands in for the name.
    pub said: &'static str,
    /// What could be done about it, where anything can, set under it the way
    /// a key would be.
    pub hint: Option<&'static str>,
}

/// Why `tip` is drawn dead, and `None` for anything taking presses — the two
/// toggles in a window with room for what they open, the surface switch on a
/// monitor with room above white, and the open button where `openable` says
/// something out there offers to open the file.
///
/// Asked before a tooltip is composed out of the key table, since what a dead
/// control owes the reader is the reason and not the binding.
pub fn disabled(tip: Tip, room: Room, hdr: Hdr, openable: bool) -> Option<Refused> {
    let no_room = match tip {
        Tip::Control(Control::Histogram) => !room.histogram,
        Tip::Control(Control::Info) => !room.info,
        _ => false,
    };
    if no_room {
        return Some(Refused {
            said: NO_ROOM,
            hint: None,
        });
    }
    if tip == Tip::Control(Control::OpenWith) && !openable {
        return Some(Refused {
            said: NOTHING_OPENS_IT,
            hint: None,
        });
    }
    match (tip, hdr) {
        (Tip::Control(Control::Output), Hdr::NotInHdrMode) => Some(Refused {
            said: NOT_HDR_MODE,
            hint: Some(REQUEST_HDR_MODE),
        }),
        (Tip::Control(Control::Output), Hdr::Unsupported) => Some(Refused {
            said: NO_HDR_OUTPUT,
            hint: None,
        }),
        _ => None,
    }
}

/// What the interface calls a thing, where the key that does the same job
/// does not already say.
///
/// `None` everywhere else, which leaves the application to name the thing by
/// its key's own description — see `App::tooltip`. Two kinds of thing need
/// words here: what no one key describes — a button no key reaches, or one of
/// a row the key cycles through, where the cycle names the row rather than
/// the button — and what a key describes at a length the label has no room
/// for, which is everything on the histogram panel: its labels are read
/// across the plot they sit on, so they have a panel's width and not a
/// window's.
pub fn words(tip: Tip) -> Option<String> {
    // A menu cell's are its own: what it goes to, or what it does.
    match tip {
        Tip::Control(Control::ZoomTo(choice)) => return Some(choice.describe()),
        Tip::Control(Control::Format(format)) => {
            return Some(menu::describe_format(format).to_string());
        }
        // No words of its own: the key table already says what each of
        // these copies takes, in a sentence, and saying it twice is saying
        // it in two places that can drift apart.
        Tip::Control(Control::Copies(_)) => return None,
        // Nor these: an item of the open menu wears the name of the program
        // it hands the file to, and there is nothing an interface that has
        // never heard of that program could add to it.
        Tip::Control(Control::OpenIn(_)) => return None,
        _ => {}
    }
    // The exposure's two steps say what they are worth, the buttons carrying
    // the number and these the units: one press of one of them, in the words
    // the bottom bar reads an exposure out in.
    if let Tip::Control(step @ (Control::ExposureDown | Control::ExposureUp)) = tip {
        let stops = if step == Control::ExposureUp {
            EV_STEP
        } else {
            -EV_STEP
        };
        return Some(format!("Exposure {} EV", stops_label(stops)));
    }
    Some(
        match tip {
            Tip::Control(Control::Zoom) => "Zoom, fit and filter",
            // No one key opens it — every cell of it has a key of its own —
            // so the button says what the menu is of.
            Tip::Control(Control::Copy) => "Copy the file or the image",
            // The same: no key opens it, and what is on it is whatever the
            // desktop has installed rather than anything this program binds.
            Tip::Control(Control::OpenWith) => "Open the file in another application",
            Tip::Control(Control::Paste) => "Paste an image",
            // No one key does this and only this — Escape dismisses whatever
            // is up, a menu first — so the cross names itself.
            Tip::Control(Control::Dismiss) => "Dismiss this message",
            // The histogram panel's, in as few words as will carry them.
            Tip::Control(Control::Luma) => "Luminance plane",
            Tip::Control(Control::Planes) => "Color planes",
            Tip::Control(Control::Log) => "Logarithmic counts",
            Tip::Control(Control::Reset) => "Reset the display",
            // Named rather than merely shown: a swatch of viridis is a green
            // rectangle that could be anything, and the map has a name people
            // ask for it by — the same one `--colormap` takes.
            Tip::Control(Control::Ramp(index)) => match Colormap::ALL.get(index)? {
                Colormap::Gray => "No false color",
                Colormap::Viridis => "Viridis",
                Colormap::Magma => "Magma",
                Colormap::Turbo => "Turbo",
            },
            // What a window button sets, rather than the two or three
            // characters it wears: a row of numbers needs saying in words
            // once, and the button has no room to say it.
            Tip::Control(Control::Window(index)) => match WINDOWS.get(index)?.1 {
                None => "The image's own window",
                Some(AutoWindow::Off) => "Window on 0 to 1",
                Some(AutoWindow::MinMax) => "Window on the whole range",
                Some(AutoWindow::Percentile) => "Window on the central 99.8%",
                // Not one of the four: a hand-set window is where the window
                // ends up, never something a button puts it on.
                Some(AutoWindow::Manual) => return None,
            },
            // The four nudges beside that reading, one at a time. The key
            // that does the same job is bound on a line with its opposite —
            // `a` and `s`, `A` and `S` — and the table's own words name the
            // pair, which is right for `--help` and one word too many for a
            // button that only goes one way.
            Tip::Control(Control::WindowDown) => "Slide the window down",
            Tip::Control(Control::WindowUp) => "Slide the window up",
            Tip::Control(Control::WindowNarrow) => "Narrow the window",
            Tip::Control(Control::WindowWiden) => "Widen the window",
            // And what a curve is, the labels being the names of the things
            // rather than descriptions of them.
            Tip::Control(Control::Curve(index)) => match ToneMap::ALL.get(index)? {
                ToneMap::None => "No tone curve",
                ToneMap::Reinhard => "Reinhard curve",
                ToneMap::Neutral => "Khronos PBR Neutral curve",
            },
            // The timeline: no key scrubs, so it names itself.
            Tip::Timeline => "Go to a frame",
            Tip::Control(_) | Tip::Name | Tip::Counter | Tip::State => return None,
        }
        .to_string(),
    )
}

/// Draws `tooltip` into the popup egui has opened for it: the thing itself
/// first, and under it, set dimmer, what to press instead.
pub fn show(ui: &mut egui::Ui, tooltip: &Tooltip, theme: &Theme) {
    ui.spacing_mut().item_spacing.y = 2.0;
    for line in &tooltip.title {
        ui.label(egui::RichText::new(line).color(theme.text_primary));
    }
    for line in &tooltip.hints {
        ui.label(egui::RichText::new(line).color(theme.text_dim));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A toggle whose panel the window cannot take says why it is dead, and
    /// says nothing of the sort while there is room for what it opens. The
    /// reason stands in for the name: the label a key would give it names a
    /// press that is going to be refused.
    #[test]
    fn a_toggle_with_nowhere_to_put_its_panel_says_so() {
        let all = Room {
            histogram: true,
            info: true,
        };
        let none = Room {
            histogram: false,
            info: false,
        };
        let no_room = Some(Refused {
            said: NO_ROOM,
            hint: None,
        });

        assert_eq!(
            disabled(Tip::Control(Control::Histogram), none, Hdr::Available, true),
            no_room
        );
        assert_eq!(
            disabled(Tip::Control(Control::Info), none, Hdr::Available, true),
            no_room
        );
        assert_eq!(
            disabled(Tip::Control(Control::Histogram), all, Hdr::Available, true),
            None
        );
        assert_eq!(
            disabled(Tip::Control(Control::Info), all, Hdr::Available, true),
            None
        );

        // Only those two: nothing else on the interface has a panel to make
        // room for, so nothing else goes dead when the window is small.
        assert_eq!(
            disabled(Tip::Control(Control::Minimap), none, Hdr::Available, true),
            None
        );
        assert_eq!(disabled(Tip::Name, none, Hdr::Available, true), None);

        // And one at a time, the way the room itself comes out: a window with
        // height for the column but not for the plot above it.
        let column = Room {
            histogram: false,
            info: true,
        };
        assert_eq!(
            disabled(
                Tip::Control(Control::Histogram),
                column,
                Hdr::Available,
                true
            ),
            no_room
        );
        assert_eq!(
            disabled(Tip::Control(Control::Info), column, Hdr::Available, true),
            None
        );
    }

    /// The surface switch says which of the two reasons it is dead for, and
    /// only the one it is dead for: a monitor that could be switched over is
    /// told what would do it, and a window that will never have an HDR color
    /// space is not sent to restart for nothing.
    #[test]
    fn the_surface_switch_says_why_it_is_dead() {
        let all = Room {
            histogram: true,
            info: true,
        };
        let switch = Tip::Control(Control::Output);

        assert_eq!(disabled(switch, all, Hdr::Available, true), None);
        assert_eq!(
            disabled(switch, all, Hdr::NotInHdrMode, true),
            Some(Refused {
                said: NOT_HDR_MODE,
                hint: Some(REQUEST_HDR_MODE),
            })
        );
        assert_eq!(
            disabled(switch, all, Hdr::Unsupported, true),
            Some(Refused {
                said: NO_HDR_OUTPUT,
                hint: None,
            })
        );

        // The switch is in the bottom bar, which every window has: a small
        // window kills the panel toggles and leaves this one alone.
        let none = Room {
            histogram: false,
            info: false,
        };
        assert_eq!(disabled(switch, none, Hdr::Available, true), None);
    }

    /// The open button says that this file has nowhere to go, and says it
    /// only while that is true and only of itself: the file on screen is what
    /// the answer is about, and every other control on the interface is
    /// unaffected by it.
    #[test]
    fn the_open_button_says_when_nothing_can_open_the_file() {
        let all = Room {
            histogram: true,
            info: true,
        };
        let button = Tip::Control(Control::OpenWith);

        assert_eq!(disabled(button, all, Hdr::Available, true), None);
        assert_eq!(
            disabled(button, all, Hdr::Available, false),
            Some(Refused {
                said: NOTHING_OPENS_IT,
                hint: None,
            })
        );

        // Nothing else goes dead with it, the copy button beside it least of
        // all: what it takes is the picture, which is here whatever the
        // desktop has installed.
        assert_eq!(
            disabled(Tip::Control(Control::Copy), all, Hdr::Available, false),
            None
        );
        assert_eq!(
            disabled(Tip::Control(Control::Paste), all, Hdr::Available, false),
            None
        );
    }
}
