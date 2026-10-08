//! How large the interface is drawn, on top of the monitor's own scale:
//! the rungs the keys step it along, and the range the state file keeps.
//!
//! The picture is not in it. It is placed in the device's own pixels, so
//! 100% stays one image pixel to one device pixel at every rung; only the
//! chrome, its words and the overlays grow and shrink.

use std::ops::RangeInclusive;

/// The rungs the keys step the interface's scale along.
pub const SCALES: [f32; 8] = [0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0];

/// The scale a new state starts at, and the one the reset key returns to:
/// the monitor's own.
pub const DEFAULT: f32 = 1.0;

/// The range the state file accepts: the ladder's ends, so a value written
/// by hand between two rungs is kept, and stepped onto the ladder from
/// there.
pub const RANGE: RangeInclusive<f32> = SCALES[0]..=SCALES[SCALES.len() - 1];

/// The next rung above `current` when `up`, below it otherwise, from any
/// value in [`RANGE`] — strictly greater or strictly less — and `current`
/// itself at either end: the keys step rather than wrap, as `zoom.in` stops
/// at the top.
pub fn step(current: f32, up: bool) -> f32 {
    let next = if up {
        SCALES.iter().find(|&&rung| rung > current)
    } else {
        SCALES.iter().rev().find(|&&rung| rung < current)
    };
    next.copied().unwrap_or(current)
}

/// What the toast says once the interface is at `scale`: the scale as a
/// whole percentage, which a value written by hand between two rungs is
/// rounded to.
pub fn said(scale: f32) -> String {
    format!("Interface scale: {}%", (scale * 100.0).round())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scale_is_said_as_a_whole_percentage() {
        assert_eq!(said(1.0), "Interface scale: 100%");
        assert_eq!(said(0.75), "Interface scale: 75%");
        assert_eq!(said(1.2), "Interface scale: 120%");
        assert_eq!(said(1.234), "Interface scale: 123%");
    }

    #[test]
    fn every_rung_steps_to_the_next() {
        for pair in SCALES.windows(2) {
            assert_eq!(step(pair[0], true), pair[1]);
            assert_eq!(step(pair[1], false), pair[0]);
        }
    }

    #[test]
    fn a_step_from_between_rungs_lands_on_the_next_rung() {
        assert_eq!(step(1.2, true), 1.25);
        assert_eq!(step(1.2, false), 1.0);
        assert_eq!(step(2.9, true), 3.0);
        assert_eq!(step(0.8, false), 0.75);
    }

    #[test]
    fn the_ends_of_the_ladder_stay_put() {
        assert_eq!(step(SCALES[0], false), SCALES[0]);
        assert_eq!(
            step(SCALES[SCALES.len() - 1], true),
            SCALES[SCALES.len() - 1]
        );
        assert!(SCALES.contains(&DEFAULT));
        assert!(RANGE.contains(&DEFAULT));
    }
}
