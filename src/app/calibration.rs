//! The print size's calibration: a monitor's size measured by hand, against
//! a credit card held to the glass, for a monitor that says its size wrongly
//! or not at all.
//!
//! The dialog shows the card's outline at the size the monitor is taken to
//! be, and its slider grows and shrinks that size until the outline covers
//! a real card. What OK keeps is the monitor's size in millimeters, against
//! its model — see [`Screen::model`] — in the state file: millimeters rather
//! than pixels to the millimeter, since a scale changed later changes the
//! second and not the first.
//!
//! The dialog has no field, so the keys do not go to egui while it is up:
//! [`App::calibration_key`] takes every one of them first, the arrows for
//! steps finer than a hand moves the slider, `Enter` and `Esc` for its two
//! buttons, and nothing else at all — a key acting on the picture behind a
//! modal would be acting on something that cannot be seen.

use winit::keyboard::{Key, NamedKey};

use super::{App, Effect};
use crate::monitor::Screen;
use crate::ui::calibrate;

/// How far the slider runs either side of the size the monitor is taken to
/// be as the dialog opens, in stops of the card's size: the card seven
/// tenths as large to four tenths larger, which is further than any monitor
/// that says its size is wrong by, and keeps the dialog small enough to fit
/// a window. A guess further out than that is reached by opening it again,
/// where the slider is centered on what was kept.
pub const RANGE: f32 = 0.5;
/// What the slider snaps to, an arrow's step, and an arrow's held with
/// Shift. A thousandth of a stop is under a tenth of a percent, which is a
/// fraction of a device pixel across the card; two thousandths, an arrow,
/// is about a device pixel on a Retina display.
pub const STEP: f32 = 0.001;
const KEY_STEP: f32 = 0.002;
const SHIFT_STEP: f32 = 0.02;

/// The logical pixels to the inch a monitor that says nothing is taken to
/// have when the dialog opens: the desktop's long-standing guess.
const GUESSED_PER_INCH: f64 = 96.0;

/// The dialog, while it is up.
pub(super) struct Calibrating {
    screen: Screen,
    /// The size the slider is measured from, in millimeters as the pixels
    /// stand: what the monitor says, or the guess where it says nothing.
    origin: [f64; 2],
    /// Where the slider stands: the card `2^stops` times the size it is at
    /// `origin`.
    stops: f32,
    /// Where it stood as the dialog opened, which it runs [`RANGE`] either
    /// side of.
    center: f32,
}

impl Calibrating {
    /// The size the slider says the monitor is.
    fn mm(&self) -> [f64; 2] {
        let factor = f64::from(self.stops).exp2();
        self.origin.map(|side| side / factor)
    }
}

impl App {
    /// The monitor the window is on, as the print size sees it.
    pub(super) fn screen(&self) -> Option<Screen> {
        self.output.monitors.as_ref()?.screen(&self.monitor_name()?)
    }

    /// How large `screen` is taken to be: measured by hand, or else as it
    /// says.
    pub(super) fn screen_size(&self, screen: &Screen) -> Option<[f64; 2]> {
        self.displays
            .get(&screen.model)
            .copied()
            .or(screen.reported)
    }

    /// Puts the dialog up, from the size the monitor is taken to be now.
    pub(super) fn open_calibration(&mut self) {
        // One dialog at a time, and no menu under it.
        if self.renaming.is_some() || self.exporting.is_some() {
            return;
        }
        self.close_menus();
        let Some(screen) = self.screen() else {
            return;
        };
        let guess = screen
            .logical
            .map(|side| f64::from(side) / GUESSED_PER_INCH * 25.4);
        let origin = screen.reported.unwrap_or(guess);
        let start = self.screen_size(&screen).unwrap_or(origin);
        // On the slider's own steps, so that the step back to the size the
        // monitor says lands on it exactly.
        let stops = ((origin[0] / start[0]).log2() as f32 / STEP).round() * STEP;
        self.calibrating = Some(Calibrating {
            screen,
            origin,
            stops,
            center: stops,
        });
    }

    /// What the dialog is drawn from this frame, while it is up.
    pub(super) fn calibration_input(&self) -> Option<calibrate::Input> {
        let calibrating = self.calibrating.as_ref()?;
        let mm = calibrating.mm();
        // The monitor's logical pixels are the window's, and the interface's
        // points are those over the interface's own scale.
        let to_points = f64::from(self.device_scale() / self.pixels_per_point());
        let per_mm = calibrating.screen.density(mm);
        Some(calibrate::Input {
            points_per_mm: per_mm.map(|value| (value * to_points) as f32),
            stops: calibrating.stops,
            low: calibrating.center - RANGE,
            high: calibrating.center + RANGE,
            step: STEP,
            diagonal: Screen::diagonal(mm),
        })
    }

    /// The slider moved.
    pub(super) fn set_calibration(&mut self, stops: f32) -> Effect {
        let Some(calibrating) = &mut self.calibrating else {
            return Effect::Nothing;
        };
        let stops = stops.clamp(calibrating.center - RANGE, calibrating.center + RANGE);
        if stops == calibrating.stops {
            return Effect::Nothing;
        }
        calibrating.stops = stops;
        Effect::Redraw
    }

    /// OK: the size the slider says is kept for the monitor's model. Left
    /// where the monitor says it is, nothing is kept, and a size measured
    /// before is let go — the monitor was right.
    pub(super) fn calibrated(&mut self) -> Effect {
        let Some(calibrating) = self.calibrating.take() else {
            return Effect::Nothing;
        };
        let model = calibrating.screen.model.clone();
        let as_said = calibrating.screen.reported.is_some() && calibrating.stops.abs() < STEP / 2.0;
        if as_said {
            self.displays.remove(&model);
        } else {
            self.displays.insert(model, calibrating.mm());
        }
        Effect::Redraw
    }

    /// Cancel: the dialog goes, and nothing changes.
    pub(super) fn cancel_calibration(&mut self) -> Effect {
        match self.calibrating.take() {
            Some(_) => Effect::Redraw,
            None => Effect::Nothing,
        }
    }

    /// A key, while the dialog is up — see the module's docs.
    pub(super) fn calibration_key(&mut self, key: &Key) -> Effect {
        let Some(calibrating) = &self.calibrating else {
            return Effect::Nothing;
        };
        let step = if self.pointer.modifiers.shift_key() {
            SHIFT_STEP
        } else {
            KEY_STEP
        };
        match key {
            Key::Named(NamedKey::Escape) => self.cancel_calibration(),
            Key::Named(NamedKey::Enter) => self.calibrated(),
            Key::Named(NamedKey::ArrowRight | NamedKey::ArrowUp) => {
                self.set_calibration(calibrating.stops + step)
            }
            Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowDown) => {
                self.set_calibration(calibrating.stops - step)
            }
            _ => Effect::Nothing,
        }
    }
}
