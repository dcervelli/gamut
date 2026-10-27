//! What each monitor is: the mode it is in, SDR or HDR with room above
//! white, and the room it is laid out in — as the platform says it, on a
//! thread that keeps the answers current.
//!
//! Why it matters: an HDR surface asked for on a monitor in SDR mode makes a
//! compositor that switches monitors automatically switch this one, and on
//! some drivers that is a modeset which blanks every display on the way.
//! With the mode in hand the surface can follow the monitor rather than lead
//! it. Where nothing can be read, the surface is chosen from the request
//! alone.
//!
//! The same connection asks the compositor which GPU it renders on, for
//! `render/adapter.rs` to draw the window there: a window drawn on another
//! GPU has to be imported across, and is shown black where that cannot be
//! done.
//!
//! The same answers say how large each monitor is on the desk, in
//! millimeters, which with its logical size is how many of its logical
//! pixels go to a millimeter: what shows a picture at the size it prints.
//! And they say what make and model it is, which is what a size measured
//! by hand is kept against — a model is the same size wherever it is
//! plugged in, where a connector's name belongs to the port.
//!
//! The table is keyed by what [`key_of`] makes of winit's handle for the
//! same monitor, so that the application never has to know what a platform
//! calls one. `wayland.rs` answers on Linux, `macos.rs` on a Mac.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

#[cfg(target_os = "linux")]
use winit::monitor::MonitorHandle;

#[cfg(target_os = "linux")]
mod wayland;
#[cfg(target_os = "linux")]
pub use wayland::watch;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{key_of, reserved, watch};

/// The key the table knows `monitor` by: the name the compositor gives it
/// on Wayland — "DP-2", say — which is also the name winit gives its handle.
#[cfg(target_os = "linux")]
pub fn key_of(monitor: &MonitorHandle) -> Option<String> {
    monitor.name()
}

/// What the desktop keeps of `monitor` for itself, in logical pixels from
/// each edge — top, right, bottom, left — that a window is not moved over.
/// Nothing on Linux, where the compositor places a window itself.
#[cfg(target_os = "linux")]
pub fn reserved(_monitor: &MonitorHandle) -> [f64; 4] {
    [0.0; 4]
}

/// Which mode a monitor is in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Its white is the brightest thing it shows: a surface with headroom is
    /// mapped down to fit.
    Sdr,
    /// It has room above white, and a surface with headroom lands there.
    Hdr,
}

/// The room a monitor is laid out in: its mode in device pixels, turned the
/// way the compositor has it, and the logical size the compositor lays it out
/// at. The ratio of the two is the scale it is really running, fractional
/// where the `wl_output` alone would have said an integer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Room {
    pub device: [u32; 2],
    pub logical: [u32; 2],
}

/// What the compositor has said so far, shared with the listening thread.
#[derive(Default, PartialEq)]
pub(super) struct Table {
    pub(super) modes: HashMap<String, Mode>,
    /// The room above white each monitor has, where it has said: its peak
    /// over its reference white.
    pub(super) headrooms: HashMap<String, f32>,
    pub(super) rooms: Vec<Room>,
    /// Each monitor's size and model, where its logical size is known.
    pub(super) screens: HashMap<String, Screen>,
}

/// A monitor as the print size sees it.
#[derive(Clone, PartialEq, Debug)]
pub struct Screen {
    /// How large it is laid out, in logical pixels, across and down.
    pub logical: [u32; 2],
    /// How large it says it is in millimeters, across and down as the
    /// pixels stand, where it says and the size is believable — see
    /// [`believed`].
    pub reported: Option<[f64; 2]>,
    /// Its make and model, as one word that a line of the state file can be
    /// named by; the connector's name where the monitor does not say.
    pub model: String,
}

impl Screen {
    /// Its logical pixels to the millimeter, across and down, were it `mm`
    /// millimeters on a side.
    pub fn density(&self, mm: [f64; 2]) -> [f64; 2] {
        let mm = turned(self.logical, mm);
        [0, 1].map(|axis| f64::from(self.logical[axis]) / mm[axis])
    }

    /// Its diagonal in inches, were it `mm` millimeters on a side: what a
    /// monitor is sold by.
    pub fn diagonal(mm: [f64; 2]) -> f64 {
        mm[0].hypot(mm[1]) / 25.4
    }
}

/// `make` and `model` as one word: what a monitor's size measured by hand is
/// kept against. Anything but a letter, a digit, `.` and `-` becomes `-`.
/// `None` where neither says anything.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(super) fn model_word(make: &str, model: &str) -> Option<String> {
    let said = format!("{} {}", make.trim(), model.trim());
    let said = said.trim();
    if said.is_empty() || said.eq_ignore_ascii_case("unknown unknown") {
        return None;
    }
    Some(
        said.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                    c
                } else {
                    '-'
                }
            })
            .collect(),
    )
}

/// The least and the most logical pixels to the millimeter a monitor is
/// believed at: a large television at a scale of one is a little over one,
/// a phone's panel at a scale of one not quite twenty. Outside them is a
/// size the monitor got wrong — an EDID that gives its aspect ratio in
/// centimeters, 16 by 9, where its size belongs.
const DENSITY_RANGE: std::ops::RangeInclusive<f64> = 1.0..=20.0;

/// How far the two axes' densities may differ: every monitor this runs on
/// has square pixels, and a size giving them otherwise is wrong by more than
/// the millimeter it is rounded to.
const SQUARE_WITHIN: f64 = 0.05;

/// `mm` turned to stand the way `logical` does: not every platform turns a
/// monitor's size with it, and a size measured by hand is kept whichever
/// way the monitor stood.
pub fn turned(logical: [u32; 2], mm: [f64; 2]) -> [f64; 2] {
    if (logical[0] >= logical[1]) == (mm[0] >= mm[1]) {
        mm
    } else {
        [mm[1], mm[0]]
    }
}

/// The size a monitor `logical` pixels across says it is, `mm` on a side,
/// turned to stand as the pixels do — or `None` for a monitor that gave no
/// size, as a projector and most televisions do, or one not to be believed.
pub(super) fn believed(logical: [u32; 2], mm: [f64; 2]) -> Option<[f64; 2]> {
    let mm = turned(logical, mm);
    if mm.iter().any(|side| !side.is_finite() || *side <= 0.0) {
        return None;
    }
    let per_mm = [0, 1].map(|axis| f64::from(logical[axis]) / mm[axis]);
    let square = (per_mm[0] / per_mm[1] - 1.0).abs() <= SQUARE_WITHIN;
    (square && per_mm.iter().all(|value| DENSITY_RANGE.contains(value))).then_some(mm)
}

/// The monitors' modes and rooms, kept current by a thread that listens for
/// changes.
pub struct Monitors {
    pub(super) table: Arc<Mutex<Table>>,
    /// Whether the compositor speaks color management at all. Where it does
    /// not, no monitor will ever have a mode, and the absence means nothing.
    pub(super) speaks_modes: bool,
    /// Whether the table is read afresh each time it is asked, as on a Mac,
    /// rather than kept current by a thread.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(super) live: bool,
    /// The device node the compositor renders on, numbered as the kernel
    /// numbers one — a `dev_t` — where the compositor has said.
    pub(super) main_device: Option<u64>,
}

impl Monitors {
    /// The mode of the monitor [`key_of`] calls `name`, or `None` for one
    /// that has not been described.
    pub fn mode(&self, name: &str) -> Option<Mode> {
        self.read()?.modes.get(name).copied()
    }

    /// How much room above white the monitor called `name` has: the ratio
    /// of its peak luminance to its reference white, as the compositor
    /// describes it, which is what a gain map's lift is weighed against.
    /// `None` for a monitor that has not said, and 1 for one in SDR mode.
    pub fn headroom(&self, name: &str) -> Option<f32> {
        self.read()?.headrooms.get(name).copied()
    }

    /// The size and model of the monitor [`key_of`] calls `name`. `None`
    /// for one whose logical size has not been said.
    pub fn screen(&self, name: &str) -> Option<Screen> {
        self.read()?.screens.get(name).cloned()
    }

    /// Whether the compositor can say what mode a monitor is in. Where it
    /// cannot, [`Monitors::mode`] is `None` for every monitor, and that is not
    /// a monitor in SDR mode but a question nothing answers.
    pub fn speaks_modes(&self) -> bool {
        self.speaks_modes
    }

    /// The device node the compositor renders on, as the kernel numbers it,
    /// which is the GPU a window is best drawn on. `None` where the
    /// compositor has not said: off Wayland, or under one without version 4
    /// of `zwp_linux_dmabuf_v1`.
    pub fn main_device(&self) -> Option<u64> {
        self.main_device
    }

    /// The room of every monitor that has said both its mode and its logical
    /// size. Empty under a compositor without `xdg_output`.
    pub fn rooms(&self) -> Vec<Room> {
        self.read()
            .map(|table| table.rooms.clone())
            .unwrap_or_default()
    }

    /// The table as it stands, read again first where it is read live.
    fn read(&self) -> Option<MutexGuard<'_, Table>> {
        #[cfg(target_os = "macos")]
        macos::refresh(self);
        self.table.lock().ok()
    }
}

#[cfg(test)]
impl Monitors {
    /// Monitors with nothing said about them yet, under a compositor that
    /// does or does not speak modes, for the tests that drive the
    /// application's reading of them.
    pub fn stub(speaks_modes: bool) -> Self {
        Self {
            table: Arc::new(Mutex::new(Table::default())),
            speaks_modes,
            live: false,
            main_device: None,
        }
    }

    /// What the compositor would have said about the monitor called
    /// `name`: its mode, and its room above white.
    pub fn set(&self, name: &str, mode: Mode, headroom: f32) {
        let mut table = self.table.lock().expect("the table is not poisoned");
        table.modes.insert(name.to_string(), mode);
        table.headrooms.insert(name.to_string(), headroom);
    }

    /// What the compositor would have said of the monitor called `name`:
    /// its logical size, and its size in millimeters where it says one.
    pub fn set_screen(&self, name: &str, logical: [u32; 2], mm: Option<[f64; 2]>) {
        let mut table = self.table.lock().expect("the table is not poisoned");
        let screen = Screen {
            logical,
            reported: mm.and_then(|mm| believed(logical, mm)),
            model: format!("model-of-{name}"),
        };
        table.screens.insert(name.to_string(), screen);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 27-inch 4K monitor at a scale of 1.5: 2560 by 1440 logical
    /// pixels over 597 by 336 millimeters.
    #[test]
    fn a_density_is_the_logical_pixels_over_the_millimeters() {
        let screen = Screen {
            logical: [2560, 1440],
            reported: None,
            model: String::new(),
        };
        let [across, down] = screen.density([597.0, 336.0]);
        assert!((across - 2560.0 / 597.0).abs() < 1e-9);
        assert!((down - 1440.0 / 336.0).abs() < 1e-9);
        assert!((Screen::diagonal([597.0, 336.0]) - 26.97).abs() < 0.01);
    }

    /// A monitor stood on its side whose size was given as it was built.
    #[test]
    fn a_size_is_turned_to_stand_as_the_pixels_do() {
        assert_eq!(believed([1440, 2560], [597.0, 336.0]), Some([336.0, 597.0]));
        assert_eq!(turned([2560, 1440], [336.0, 597.0]), [597.0, 336.0]);
    }

    /// No size, an aspect ratio where the size belongs, and pixels that
    /// would not be square: none of them is believed.
    #[test]
    fn a_size_not_to_be_believed_is_none() {
        assert_eq!(believed([1920, 1080], [0.0, 0.0]), None);
        assert_eq!(believed([1920, 1080], [16.0, 9.0]), None);
        assert_eq!(believed([1920, 1080], [600.0, 200.0]), None);
    }

    /// A model is one word a state file's line can be named by.
    #[test]
    fn a_model_is_one_word() {
        assert_eq!(
            model_word("Dell Inc.", "DELL U2720Q").as_deref(),
            Some("Dell-Inc.-DELL-U2720Q")
        );
        assert_eq!(model_word("", " "), None);
        assert_eq!(model_word("Unknown", "Unknown"), None);
    }
}
