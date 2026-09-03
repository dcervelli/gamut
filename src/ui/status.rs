//! The words in the bars: what the image is, and what the view is doing to
//! it.

use crate::image::display::{AutoWindow, Colormap};
use crate::render::TextMeasure;

use super::{BECOMES, Current, FrameInput, Reading, TEXT_SIZE};

/// Joins as many leading segments as fit in `width`, keeping at least the
/// first however narrow the window gets.
pub(super) fn fit_segments(text: &mut dyn TextMeasure, segments: &[String], width: f32) -> String {
    const SEPARATOR: &str = "   \u{00b7}   ";
    let Some((first, rest)) = segments.split_first() else {
        return String::new();
    };
    let mut joined = first.clone();
    for segment in rest {
        let candidate = format!("{joined}{SEPARATOR}{segment}");
        if text.measure_text(&candidate, TEXT_SIZE)[0] > width {
            break;
        }
        joined = candidate;
    }
    joined
}

/// The name of the image on screen, and after it whatever the loader is busy
/// with when that has taken long enough to notice.
///
/// One string, clipped as one piece: where there is no room for both, the file
/// you are actually looking at is the one worth keeping.
pub(super) fn top_label(shown: &str, reading: Option<&Reading>) -> String {
    match reading {
        Some(Reading::File(next)) => format!("{shown}, loading {next}"),
        Some(Reading::Again) => format!("{shown}, reloading"),
        None => shown.to_string(),
    }
}

/// The word that goes in front of the name when the file behind the picture
/// is gone. Set apart in the warning colour rather than folded into the name,
/// which is still the name of the file the pixels came from: it is a fact
/// about the file's standing in the world, not part of what it is called —
/// and a file that really is called `DELETED` must not read as this.
pub(super) const DELETED: &str = "DELETED";

/// Where the file on screen comes in the list it was opened with, for in
/// front of its name — or `None` for a single file, "[1/1]" being a count of
/// nothing.
///
/// It leads the bar because it is the one part of the line whose width does
/// not depend on the file, so a reader looking for it always finds it in the
/// same place. It is drawn as its own run rather than as part of the name:
/// the name is what is being looked at and the count is a fact about the
/// list, and the two are set apart to say so.
pub(super) fn counter(index: usize, count: usize) -> Option<String> {
    (count > 1).then(|| format!("[{}/{}]", index + 1, count))
}

pub(super) fn describe_pixels(current: &Current) -> String {
    let channels = current.image.channels().label();
    let stored = current
        .stored
        .as_ref()
        .map(|stored| format!(" {BECOMES} {stored}"))
        .unwrap_or_default();
    format!(
        "{} {channels}{stored}",
        current.image.samples.component_name()
    )
}

/// What is being done to the image, for the bottom bar: only the things
/// actually in force, so a viewer left alone says nothing here.
///
/// Neither the zoom nor the fit it came from, nor the filter the image is
/// magnified with: all three are the button in the top bar, which reads the
/// zoom out and opens a menu of the rest. The bar named the filter once, but
/// naming a thing it could not be used to change is worth less than a cell
/// that both says and sets it.
pub(super) fn describe_state(current: &Current, input: &FrameInput) -> String {
    let mut parts = Vec::new();
    if let Some(label) = input.hdr_output {
        parts.push(label.to_string());
    }
    if current.display.auto != AutoWindow::Off {
        parts.push(format!(
            "{} {}",
            current.display.auto.label(),
            format_window(current)
        ));
    }
    if current.display.exposure_stops != 0.0 {
        parts.push(format!("{:+.1} EV", current.display.exposure_stops));
    }
    if current.display.colormap != Colormap::Gray {
        parts.push(current.display.colormap.label().to_string());
    }
    if current.image.is_high_dynamic_range() {
        parts.push(current.display.tone_map.label().to_string());
    }
    parts.join("   \u{00b7}   ")
}

/// Window bounds in the units of the source file where that is meaningful.
/// Linear integer data reads back as counts, which is what measurement work
/// wants; anything with a curve on it stays in normalised units.
fn format_window(current: &Current) -> String {
    let scale = if current.image.color.transfer.is_linear() {
        current.image.samples.full_scale()
    } else {
        1.0
    };
    let low = current.display.low * scale;
    let high = current.display.high * scale;
    if scale > 1.0 {
        format!("{low:.0}\u{2013}{high:.0}")
    } else {
        format!("{low:.3}\u{2013}{high:.3}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bar names the image on screen first and always. A file on its way
    /// in is mentioned after it, never in place of it: captioning one picture
    /// with another's name is the one thing an image viewer must not do.
    #[test]
    fn the_bar_names_what_is_on_screen_before_what_is_coming() {
        assert_eq!(top_label("a.png", None), "a.png");
        assert_eq!(
            top_label("a.png", Some(&Reading::File("b.heic".into()))),
            "a.png, loading b.heic"
        );
        assert_eq!(
            top_label("a.png", Some(&Reading::Again)),
            "a.png, reloading",
            "a file being re-read has no new name to show, only the wait"
        );
    }

    /// Which of the list you are looking at — and nothing at all when the
    /// list is one file long.
    #[test]
    fn a_file_out_of_several_is_counted_and_a_file_on_its_own_is_not() {
        assert_eq!(counter(0, 6).as_deref(), Some("[1/6]"));
        assert_eq!(counter(5, 6).as_deref(), Some("[6/6]"));
        assert_eq!(counter(0, 1), None);
        assert_eq!(counter(0, 0), None);
    }
}
