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
//! The table is keyed by what [`key_of`] makes of winit's handle for the
//! same monitor, so that the application never has to know what a platform
//! calls one. `wayland.rs` is the one platform that answers.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use winit::monitor::MonitorHandle;

#[cfg(target_os = "linux")]
mod wayland;
#[cfg(target_os = "linux")]
pub use wayland::watch;

/// The key the table knows `monitor` by: the name the compositor gives it
/// on Wayland — "DP-2", say — which is also the name winit gives its handle.
pub fn key_of(monitor: &MonitorHandle) -> Option<String> {
    monitor.name()
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
}

/// The monitors' modes and rooms, kept current by a thread that listens for
/// changes.
pub struct Monitors {
    pub(super) table: Arc<Mutex<Table>>,
    /// Whether the compositor speaks color management at all. Where it does
    /// not, no monitor will ever have a mode, and the absence means nothing.
    pub(super) speaks_modes: bool,
}

impl Monitors {
    /// The mode of the monitor [`key_of`] calls `name`, or `None` for one
    /// that has not been described.
    pub fn mode(&self, name: &str) -> Option<Mode> {
        self.table.lock().ok()?.modes.get(name).copied()
    }

    /// How much room above white the monitor called `name` has: the ratio
    /// of its peak luminance to its reference white, as the compositor
    /// describes it, which is what a gain map's lift is weighed against.
    /// `None` for a monitor that has not said, and 1 for one in SDR mode.
    pub fn headroom(&self, name: &str) -> Option<f32> {
        self.table.lock().ok()?.headrooms.get(name).copied()
    }

    /// Whether the compositor can say what mode a monitor is in. Where it
    /// cannot, [`Monitors::mode`] is `None` for every monitor, and that is not
    /// a monitor in SDR mode but a question nothing answers.
    pub fn speaks_modes(&self) -> bool {
        self.speaks_modes
    }

    /// The room of every monitor that has said both its mode and its logical
    /// size. Empty under a compositor without `xdg_output`.
    pub fn rooms(&self) -> Vec<Room> {
        self.table
            .lock()
            .map(|table| table.rooms.clone())
            .unwrap_or_default()
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
        }
    }

    /// What the compositor would have said about the monitor called
    /// `name`: its mode, and its room above white.
    pub fn set(&self, name: &str, mode: Mode, headroom: f32) {
        let mut table = self.table.lock().expect("the table is not poisoned");
        table.modes.insert(name.to_string(), mode);
        table.headrooms.insert(name.to_string(), headroom);
    }
}
