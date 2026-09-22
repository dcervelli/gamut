//! Which rule chooses the display window, and how the command line names it.

use crate::image::{DecodedImage, Referred};

/// How the display window is chosen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AutoWindow {
    /// Take the values at face value: 0..1 is the visible range.
    Off,
    /// Stretch the full observed range to 0..1.
    MinMax,
    /// Stretch the central 99.8% of the range, ignoring outliers. Usually
    /// what you want for sensor data with hot pixels.
    Percentile,
    /// Left where the user put it.
    Manual,
}

impl AutoWindow {
    /// `stored`, `full` or `trimmed`, as the command line names them — the
    /// words the histogram panel's buttons and the bottom bar use. `unit`,
    /// `minmax` and `pct`, and `off`, `min-max` and `percentile`, are taken
    /// as well.
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.to_ascii_lowercase().as_str() {
            "stored" | "unit" | "off" => AutoWindow::Off,
            "full" | "minmax" | "min-max" => AutoWindow::MinMax,
            "trimmed" | "pct" | "percentile" => AutoWindow::Percentile,
            _ => return None,
        })
    }

    /// The rule in one word, the same one the panel's button expands —
    /// *As stored*, *Full range*, *Trimmed* — and the command line takes.
    pub fn label(self) -> &'static str {
        match self {
            AutoWindow::Off => "stored",
            AutoWindow::MinMax => "full",
            AutoWindow::Percentile => "trimmed",
            AutoWindow::Manual => "manual",
        }
    }

    /// The window an image opens with, which is one rule: the image's own
    /// [`Referred`]. Something already graded has a white of its own and 0..1
    /// is exactly right. Scene light is in the file's own units too — a
    /// render's, or cd/m² — and keeps them, the meter working on the
    /// exposure instead ([`super::Display::for_image_with`]). Linear sensor counts
    /// have neither a white nor a middle, and showing them unwindowed is how
    /// you get a black rectangle.
    ///
    /// Held apart from [`super::Display::for_image_with`] because it is also what
    /// [`super::Display::reset`] puts back, and what the histogram panel's row of
    /// windows names outright: the file's own is always one of the three.
    pub fn default_for(image: &DecodedImage) -> Self {
        match image.referred {
            Referred::Display | Referred::Scene => AutoWindow::Off,
            Referred::Measured => AutoWindow::Percentile,
        }
    }

    pub(super) fn next(self) -> Self {
        match self {
            AutoWindow::Off => AutoWindow::MinMax,
            AutoWindow::MinMax => AutoWindow::Percentile,
            // Cycling out of a hand-set window returns to the automatic ones.
            AutoWindow::Percentile | AutoWindow::Manual => AutoWindow::Off,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rules are named the same way everywhere — the buttons, the bar,
    /// the key's help and the command line — and the command line takes the
    /// other spellings as well, so a script written to them does not break.
    #[test]
    fn the_window_rules_answer_to_their_old_names_too() {
        for (word, rule) in [
            ("stored", AutoWindow::Off),
            ("Stored", AutoWindow::Off),
            ("unit", AutoWindow::Off),
            ("off", AutoWindow::Off),
            ("full", AutoWindow::MinMax),
            ("minmax", AutoWindow::MinMax),
            ("min-max", AutoWindow::MinMax),
            ("trimmed", AutoWindow::Percentile),
            ("pct", AutoWindow::Percentile),
            ("percentile", AutoWindow::Percentile),
        ] {
            assert_eq!(AutoWindow::parse(word), Some(rule), "{word}");
        }
        assert_eq!(AutoWindow::parse("99.8%"), None);
        assert_eq!(AutoWindow::parse("manual"), None, "not a rule to ask for");
        for rule in [AutoWindow::Off, AutoWindow::MinMax, AutoWindow::Percentile] {
            assert_eq!(
                AutoWindow::parse(rule.label()),
                Some(rule),
                "the label is a spelling the command line takes"
            );
        }
    }
}
