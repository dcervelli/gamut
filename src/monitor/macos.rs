//! What each monitor is, on macOS: `NSScreen` says whether a display has
//! room above white at all and how much it has right now, which moves with
//! the display's brightness and the light in the room.
//!
//! Asking for that room never switches a Mac's display into another mode, so
//! the rule the Linux side keeps — follow the monitor, never lead it — costs
//! nothing here. AppKit answers these questions on the main thread only, and
//! the application asks from the main thread on every turn of its loop, so
//! the table is read afresh there rather than kept by a thread of its own.

use std::sync::{Arc, Mutex};

use objc2::MainThreadMarker;
use objc2::rc::autoreleasepool;
use objc2_app_kit::NSScreen;
use objc2_foundation::{NSNumber, NSRect, NSString};
use winit::monitor::MonitorHandle;
use winit::platform::macos::MonitorHandleExtMacOS as _;

use super::{Mode, Monitors, Room, Table};

/// The key the table knows `monitor` by: the display's Core Graphics
/// identifier, which winit and `NSScreen` both carry and which, unlike its
/// name, two identical displays do not share.
pub fn key_of(monitor: &MonitorHandle) -> Option<String> {
    Some(monitor.native_id().to_string())
}

/// What the menu bar and the Dock keep of `monitor`, in points from each
/// edge — top, right, bottom, left: the difference between the screen's
/// frame and its visible frame. Nothing off the main thread, or for a
/// monitor no screen answers to.
pub fn reserved(monitor: &MonitorHandle) -> [f64; 4] {
    let Some(main) = MainThreadMarker::new() else {
        return [0.0; 4];
    };
    let key = key_of(monitor);
    autoreleasepool(|_| {
        let Some(screen) = NSScreen::screens(main)
            .iter()
            .find(|screen| identifier(screen) == key)
        else {
            return [0.0; 4];
        };
        let frame = screen.frame();
        let visible = screen.visibleFrame();
        let top = |rect: NSRect| rect.origin.y + rect.size.height;
        let right = |rect: NSRect| rect.origin.x + rect.size.width;
        [
            top(frame) - top(visible),
            right(frame) - right(visible),
            visible.origin.y - frame.origin.y,
            visible.origin.x - frame.origin.x,
        ]
        .map(|inset| inset.max(0.0))
    })
}

/// The table as the displays have it now. `None` off the main thread, where
/// AppKit will not say.
pub fn watch(_notify: impl Fn() + Send + 'static) -> Option<Monitors> {
    let table = read()?;
    Some(Monitors {
        table: Arc::new(Mutex::new(table)),
        speaks_modes: true,
        live: true,
        main_device: None,
    })
}

/// Reads the table again into `monitors`, where it is read live and this is
/// the main thread.
pub(super) fn refresh(monitors: &Monitors) {
    if !monitors.live {
        return;
    }
    if let (Some(table), Ok(mut held)) = (read(), monitors.table.lock()) {
        *held = table;
    }
}

/// Every display's mode, room above white and room on the desk.
fn read() -> Option<Table> {
    let main = MainThreadMarker::new()?;
    autoreleasepool(|_| {
        let mut table = Table::default();
        for screen in NSScreen::screens(main).iter() {
            let Some(key) = identifier(&screen) else {
                continue;
            };
            let potential = screen.maximumPotentialExtendedDynamicRangeColorComponentValue();
            let now = screen.maximumExtendedDynamicRangeColorComponentValue();
            let mode = if potential > 1.0 {
                Mode::Hdr
            } else {
                Mode::Sdr
            };
            table.modes.insert(key.clone(), mode);
            table.headrooms.insert(key, in_steps(now));
            let frame = screen.frame();
            let scale = screen.backingScaleFactor();
            let logical = [frame.size.width, frame.size.height];
            table.rooms.push(Room {
                device: logical.map(|side| (side * scale).round() as u32),
                logical: logical.map(|side| side.round() as u32),
            });
        }
        Some(table)
    })
}

/// How many steps a stop of headroom is read in.
const STEPS_PER_STOP: f64 = 8.0;

/// `headroom` to the nearest eighth of a stop, and never below none.
///
/// A display ramps its room above white up smoothly once a window shows it
/// something that needs it, and eases it back as the light changes, so read
/// exactly it is new on every turn of the loop — and every new value is a
/// frame and a fresh lift of a gain-mapped picture. An eighth of a stop is
/// finer than the eye follows a slow change in, and a ramp of a stop or two
/// becomes a handful of frames.
fn in_steps(headroom: f64) -> f32 {
    let stops = (headroom.max(1.0).log2() * STEPS_PER_STOP).round() / STEPS_PER_STOP;
    stops.exp2() as f32
}

/// A screen's Core Graphics identifier, as [`key_of`] writes it.
fn identifier(screen: &NSScreen) -> Option<String> {
    let number = screen
        .deviceDescription()
        .objectForKey(&NSString::from_str("NSScreenNumber"))?
        .downcast::<NSNumber>()
        .ok()?;
    Some(number.unsignedIntValue().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The room is read in eighths of a stop, and none is the least there is.
    #[test]
    fn the_room_above_white_is_read_in_eighths_of_a_stop() {
        assert_eq!(in_steps(1.0), 1.0);
        assert_eq!(in_steps(0.5), 1.0);
        assert_eq!(in_steps(1.03), 1.0);
        assert_eq!(in_steps(2.0), 2.0);
        assert_eq!(in_steps(2.1), 2.0_f32.powf(1.125));
        assert_eq!(in_steps(16.0), 16.0);
    }
}
