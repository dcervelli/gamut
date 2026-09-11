//! The words in the bars: what the image is, and what the view is doing to
//! it.

use egui::{Align2, Label, RichText, Sense, TextFormat, pos2, text::LayoutJob, vec2};

use crate::image::display::{AutoWindow, Colormap, Headroom, ToneMap};

use super::chrome::{BAR_PADDING, Corners, Pass, STEP_SEAM, measure};
use super::control::Control;
use super::style::TOGGLE_RADIUS;
use super::tooltip::Tip;
use super::{
    COUNTER_GAP, Current, PADDING, Reading, TEXT_SIZE, capitalized, fonts, histogram, icon,
};

/// Between one segment of a bar and the next. A thin gap: the middot already
/// parts them, and the bars are short of room before they are short of air.
const SEPARATOR: &str = " \u{00b7} ";

/// The facts about the picture the top bar sets at its far end: its size,
/// what each pixel holds, and the color space those numbers are meant in.
/// Least to most disposable, for [`fit_segments`] to cut.
pub(super) fn facts(current: &Current, _measure: impl FnMut(&str) -> f32) -> [String; 3] {
    [
        format!("{} \u{00d7} {}", current.image.width, current.image.height),
        describe_pixels(current),
        current.image.color.label(),
    ]
}

/// Joins as many leading segments as fit in `width`, keeping at least the
/// first however narrow the window gets. The facts are dropped whole rather
/// than clipped: half of "18333 x 15667" is worse than none of it.
pub(super) fn fit_segments(
    mut measure: impl FnMut(&str) -> f32,
    segments: &[String],
    width: f32,
) -> String {
    let Some((first, rest)) = segments.split_first() else {
        return String::new();
    };
    let mut joined = first.clone();
    for segment in rest {
        let candidate = format!("{joined}{SEPARATOR}{segment}");
        if measure(&candidate) > width {
            break;
        }
        joined = candidate;
    }
    joined
}

/// The top bar's own words, from the near end: the pair that steps through
/// the list while there is one, the count, the word for a file that has
/// gone, and the name — the one thing in the window set bold, and the only
/// thing drawn in the ink the theme keeps for it.
pub(super) fn top_words(pass: &mut Pass, ui: &mut egui::Ui, current: &Current) {
    let dim: egui::Color32 = pass.theme.text_dim.into();
    // The pair that steps through the list, at the head of the bar in front
    // of the count they move through. Only with a list to step through:
    // stepping a list of one does nothing, and a button that did nothing
    // when pressed would be worse than no button.
    if pass.input.count > 1 {
        let previous = pass.icon_button(
            ui,
            icon::CHEVRON_LEFT,
            Control::Previous,
            false,
            true,
            Corners::Leading,
        );
        if previous.clicked() {
            pass.press(Control::Previous);
        }
        ui.add_space(STEP_SEAM);
        let next = pass.icon_button(
            ui,
            icon::CHEVRON_RIGHT,
            Control::Next,
            false,
            true,
            Corners::Trailing,
        );
        if next.clicked() {
            pass.press(Control::Next);
        }
        ui.add_space(PADDING);
    }
    // The count is a fact about the list, not part of the name, and is set
    // like the other facts in the bar: the name is the one thing here worth
    // picking out, and picking out two things picks out neither.
    if let Some(counter) = counter(pass.input.index, pass.input.count) {
        let response = ui.add(Label::new(RichText::new(counter).color(dim)));
        pass.tooltip(response, Tip::Counter, true);
        ui.add_space(COUNTER_GAP);
    }
    // In front of the name, on the side of the bar the name is read from, so
    // that it is seen before the file it is about rather than after it.
    if pass.input.deleted {
        ui.add(Label::new(RichText::new(DELETED).color(pass.theme.warning)));
        ui.add_space(COUNTER_GAP);
    }
    let name = top_label(&current.label, pass.input.reading.as_ref());
    let response = ui.add(
        Label::new(
            RichText::new(name)
                .color(pass.theme.text_bright)
                .family(egui::FontFamily::Name(fonts::BOLD.into())),
        )
        .truncate(),
    );
    pass.tooltip(response, Tip::Name, true);
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
/// is gone. Set apart in the warning color rather than folded into the name,
/// which is still the name of the file the pixels came from: it is a fact
/// about the file's standing in the world, not part of what it is called —
/// and a file that really is called `DELETED` must not read as this.
pub(super) const DELETED: &str = "DELETED";

/// Where the file on screen comes in the list it was opened with, for in
/// front of its name — or `None` for a single file, "1 / 1" being a count of
/// nothing.
pub(super) fn counter(index: usize, count: usize) -> Option<String> {
    (count > 1).then(|| format!("{} / {}", index + 1, count))
}

/// What each pixel holds, in the bar's shorthand: `RGB8`, `RGBA16`,
/// `G32F`.
///
/// Not the format the GPU stored it in. That is a fact about this machine
/// rather than about the file — the same image lands on a different one on a
/// device without 16-bit norm textures — and it is the info panel's `Stored
/// as`, written out where a reader has gone looking for it.
pub(super) fn describe_pixels(current: &Current) -> String {
    current.image.samples.short_label()
}

/// The room a press keeps around the words at the end of the bottom bar: the
/// wash that comes up under them is a button's wash, and ink laid tight
/// against the letters would not read as one.
const STATE_PAD: f32 = 6.0;

/// The words at the far end of the bottom bar: what is being done to the
/// image, set against the switch that ends the bar, and nothing at all where
/// nothing is being done. Cut by whole segments to half the bar, as the top
/// bar's facts are.
///
/// Pressed as well as pointed at: a press on them opens the panel that sets
/// what they are reading out, and a button's wash comes up under them while
/// the pointer is on them. Nothing at rest — the line is a reading first.
pub(super) fn state_words(pass: &mut Pass, ui: &mut egui::Ui, current: &Current) {
    let segments = describe_state(current, pass.input.headroom);
    if segments.is_empty() {
        return;
    }
    let room = (ui.max_rect().width() / 2.0 - BAR_PADDING * 2.0).max(1.0);
    let line = fit_segments(|text| measure(ui, text), &segments, room);
    // The word for a picture losing its highlights, which is the one thing
    // on this line that nobody asked for — set bold, and in the ink the line
    // takes when the pointer is on it, so that it reads as the thing being
    // said whether or not anyone is pointing.
    let clipped = (segments.last().is_some_and(|last| last == CLIPPED) && line.ends_with(CLIPPED))
        .then(|| line.len() - CLIPPED.len());
    let (head, tail) = match clipped {
        Some(at) => (&line[..at], &line[at..]),
        None => (line.as_str(), ""),
    };

    let body = egui::TextStyle::Body.resolve(ui.style());
    let bold = egui::FontId::new(TEXT_SIZE, egui::FontFamily::Name(fonts::BOLD.into()));
    let job = |ink: egui::Color32, bold_ink: egui::Color32| {
        let mut job = LayoutJob::default();
        job.append(head, 0.0, TextFormat::simple(body.clone(), ink));
        job.append(tail, 0.0, TextFormat::simple(bold.clone(), bold_ink));
        job
    };
    let primary: egui::Color32 = pass.theme.text_primary.into();
    let size = ui
        .ctx()
        .fonts_mut(|fonts| fonts.layout_job(job(primary, primary)).size());
    let (rect, response) = ui.allocate_exact_size(
        vec2(size.x + 2.0 * STATE_PAD, ui.available_height()),
        Sense::CLICK,
    );
    let (wash, ink) = pass.button_ink(false, &response, true);
    if response.hovered() {
        // A button's own wash, at a button's height down the middle of the
        // bar, so that what comes up under the words is the shape everything
        // else in the chrome wears when the pointer is on it.
        let pill = egui::Rect::from_center_size(
            rect.center(),
            vec2(rect.width(), super::chrome::BUTTON_SIZE.min(rect.height())),
        );
        ui.painter().rect_filled(pill, TOGGLE_RADIUS, wash);
    }
    let galley = ui
        .ctx()
        .fonts_mut(|fonts| fonts.layout_job(job(ink, primary)));
    let at = Align2::CENTER_CENTER.anchor_size(rect.center(), galley.size());
    ui.painter().galley(pos2(at.min.x, at.min.y), galley, ink);
    let response = pass.tooltip(response, Tip::State, true);
    if response.clicked() {
        pass.press(Control::Histogram);
    }
}

/// What is being done to the image, segment by segment: only the things
/// actually in force, so a viewer left alone says nothing here.
///
/// Neither the zoom nor the fit it came from, nor the filter the image is
/// magnified with: all three are the button in the top bar, which reads the
/// zoom out and opens a menu of the rest. Nor which surface the picture is
/// on: that is the button at the end of this bar, lit when it is the HDR one.
///
/// The window is named and not measured. Its two bounds are a reading rather
/// than a setting — they want the plot they came off, where the panel writes
/// them along the axis — and the rule is the part of it that says what was
/// asked for. What the bounds are is in the tooltip for anyone reading the
/// bar rather than the panel.
fn describe_state(current: &Current, headroom: Headroom) -> Vec<String> {
    let mut parts = Vec::new();
    if current.display.auto != AutoWindow::Off {
        parts.push(current.display.auto.label().to_string());
    }
    if current.display.exposure_stops != 0.0 {
        parts.push(format!(
            "{} EV",
            histogram::stops_label(current.display.exposure_stops)
        ));
    }
    // The false color is a reading of one channel, and the display leaves
    // it off a color image; so does the bar.
    if current.image.is_gray() && current.display.colormap != Colormap::Gray {
        parts.push(current.display.colormap.label().to_string());
    }
    if let Some(highlights) = describe_highlights(current, headroom) {
        parts.push(highlights.to_string());
    }
    parts
}

/// The word for a picture whose highlights are being thrown away.
///
/// The one thing this line says that is not a setting somebody chose, so it
/// is the one thing on it set bold: everything else is an answer to "what did
/// I ask for", and this is an answer to "what is happening to the picture".
pub(super) const CLIPPED: &str = "clipped";

/// What is becoming of the highlights: the curve that is on them, or —
/// where there is none and the surface stops at white — that they are being
/// clipped, said outright rather than left to be inferred from a picture
/// that has gone flat at the top. Nothing at all where nothing is being
/// done: no curve and nothing above white to clip, or no curve and a surface
/// with the room to show what is.
///
/// The false color clips whatever the curve, and says so by being named
/// itself, in the segment before this one.
fn describe_highlights(current: &Current, headroom: Headroom) -> Option<&'static str> {
    let display = &current.display;
    if current.image.is_gray() && display.colormap != Colormap::Gray {
        return None;
    }
    match (display.tone_map, headroom) {
        (ToneMap::None, Headroom::Above) => None,
        (ToneMap::None, Headroom::None) => display.exceeds_white(&current.stats).then_some(CLIPPED),
        (curve, _) => Some(curve.label()),
    }
}

/// The two ends of the display window, in the units of the source file where
/// that is meaningful. Linear integer data reads back as counts, which is
/// what measurement work wants; anything with a curve on it stays in
/// normalized units.
fn window_bounds(current: &Current) -> [String; 2] {
    let scale = if current.image.color.transfer.is_linear() {
        current.image.samples.full_scale()
    } else {
        1.0
    };
    let low = current.display.low * scale;
    let high = current.display.high * scale;
    if scale > 1.0 {
        [format!("{low:.0}"), format!("{high:.0}")]
    } else {
        [format!("{low:.3}"), format!("{high:.3}")]
    }
}

/// Those bounds as the histogram panel writes them along its axis: the two of
/// them with a dash between.
pub(super) fn format_window(current: &Current) -> String {
    let [low, high] = window_bounds(current);
    format!("{low}\u{2013}{high}")
}

/// What is being done to the image, in sentences: the tooltip on the words at
/// the end of the bottom bar.
///
/// Everything in force, whether or not the bar had the room for it, and each
/// thing said rather than named — the bar has a word for a window and this
/// has where the window is. One sentence to a line, since they are a list of
/// what is in force rather than a paragraph about it, and the tooltip is read
/// down the way the settings themselves are set out. Empty exactly when the
/// bar's own line is: nothing is being done, so there are no words there to
/// rest on.
pub fn explain_state(current: &Current, headroom: Headroom) -> Vec<String> {
    let display = &current.display;
    let mut said = Vec::new();
    if display.auto != AutoWindow::Off {
        let [low, high] = window_bounds(current);
        said.push(format!(
            "{} window spans {low} to {high}.",
            capitalized(display.auto.label())
        ));
    }
    if display.exposure_stops != 0.0 {
        said.push(format!(
            "Exposure {} EV.",
            histogram::stops_label(display.exposure_stops)
        ));
    }
    if current.image.is_gray() && display.colormap != Colormap::Gray {
        said.push(format!(
            "{} false color.",
            capitalized(display.colormap.label())
        ));
    }
    match describe_highlights(current, headroom) {
        // The one of them that is not a setting: what is becoming of the
        // picture, in the words the bar sets in bold.
        Some(CLIPPED) => said.push("The image is currently clipped.".to_string()),
        Some(curve) => said.push(format!("{} tone curve.", capitalized(curve))),
        None => {}
    }
    said
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::display::{Display, Startup};
    use crate::image::exif::Exif;
    use crate::image::sequence::Sequence;
    use crate::image::{AlphaMode, Channels, ColorSpace, DecodedImage, Samples, Stats};
    use crate::ui::FileFacts;

    /// A gray photograph on screen: two codes, black and white, sRGB.
    fn photograph() -> Current {
        let image = DecodedImage::new(
            2,
            1,
            Samples::U8 {
                channels: Channels::Gray,
                data: vec![0, 255],
            },
            ColorSpace::SRGB,
            AlphaMode::Opaque,
        );
        let stats = Stats::scan(&image);
        Current {
            display: Display::for_image_with(&image, &stats, Startup::default(), Headroom::None),
            image: std::sync::Arc::new(image),
            stats,
            label: "a.png".into(),
            file: FileFacts {
                path: "a.png".into(),
                bytes: None,
                modified: None,
                reader: None,
            },
            exif: Exif::default(),
            stored: None,
            sequence: Sequence::Still,
            page: 0,
        }
    }

    /// Every glyph one `TEXT_SIZE` square, so that a test can say how much
    /// room a string has in whole characters.
    fn monospace(text: &str) -> f32 {
        text.chars().count() as f32 * TEXT_SIZE
    }

    /// The line at the end of the bottom bar names what is in force and
    /// measures nothing: a window by the rule it came from and not by its
    /// bounds, an exposure in the quarters it is stepped in, and — where the
    /// highlights are going — the word for what is happening to them. A
    /// picture nobody has touched leaves it empty, which is what keeps the
    /// foot of an ordinary photograph's window quiet.
    #[test]
    fn the_bar_names_what_is_in_force_and_measures_nothing() {
        let mut current = photograph();
        assert!(
            describe_state(&current, Headroom::None).is_empty(),
            "a photograph as it was decoded has nothing being done to it"
        );

        current.display.auto = AutoWindow::MinMax;
        current.display.adjust_exposure(0.5);
        assert_eq!(
            describe_state(&current, Headroom::None),
            ["min/max", "+\u{00bd} EV", CLIPPED],
            "the rule and not its numbers, and the exposure in halves and \
             quarters rather than rounded to a tenth"
        );

        current.display.colormap = Colormap::Viridis;
        assert_eq!(
            describe_state(&current, Headroom::None),
            ["min/max", "+\u{00bd} EV", "viridis"],
            "a false color is named itself, and clips whatever the curve"
        );
    }

    /// The words are cut to the room by whole segments, so that they never
    /// say half of anything, and the first of them survives however narrow
    /// the window gets.
    #[test]
    fn the_line_is_cut_by_whole_segments() {
        let mut current = photograph();
        current.display.auto = AutoWindow::MinMax;
        current.display.adjust_exposure(0.5);
        let segments = describe_state(&current, Headroom::None);

        let whole = fit_segments(monospace, &segments, 1000.0);
        assert_eq!(
            whole,
            format!("min/max{SEPARATOR}+\u{00bd} EV{SEPARATOR}{CLIPPED}"),
            "a wide window has room for all of it"
        );

        let cut = fit_segments(monospace, &segments, monospace(&whole) * 0.6);
        assert!(
            cut.len() < whole.len() && whole.starts_with(&cut),
            "\"{cut}\" is not \"{whole}\" with segments taken off the end"
        );
        assert!(!cut.ends_with(CLIPPED));

        assert_eq!(
            fit_segments(monospace, &segments, 1.0),
            "min/max",
            "the first segment stays however narrow the window gets"
        );
    }

    /// What the bar names in the room it has, the tooltip says: the window's
    /// own bounds, which the bar no longer writes out, and a sentence for
    /// every other thing in force whether or not the bar had room for it.
    #[test]
    fn the_tooltip_says_in_full_what_the_bar_has_room_to_name() {
        let mut current = photograph();
        assert!(
            explain_state(&current, Headroom::None).is_empty(),
            "nothing is being done, so there are no words to rest on"
        );

        current.display.auto = AutoWindow::MinMax;
        current.display.low = 0.012;
        current.display.high = 1.0;
        current.display.adjust_exposure(0.25);
        assert_eq!(
            explain_state(&current, Headroom::None),
            [
                "Min/max window spans 0.012 to 1.000.",
                "Exposure +\u{00bc} EV.",
                "The image is currently clipped.",
            ],
            "one line to each thing in force, and the window's own bounds \
             where the bar has only its name"
        );

        current.display.tone_map = ToneMap::Neutral;
        assert_eq!(
            explain_state(&current, Headroom::None).last().unwrap(),
            "Neutral tone curve.",
            "the curve takes the place of the word for losing the highlights"
        );
    }

    /// The bar says what is becoming of the highlights and nothing more: no
    /// word for a picture with none above white, `clipped` once exposure has
    /// pushed some there on a surface that stops at white, the curve's own
    /// name while one is on, and nothing under a false color, which is named
    /// itself and clips whatever the curve.
    #[test]
    fn the_bar_says_what_becomes_of_the_highlights() {
        let mut current = photograph();
        for headroom in [Headroom::None, Headroom::Above] {
            assert_eq!(
                describe_highlights(&current, headroom),
                None,
                "{headroom:?}"
            );
        }

        current.display.adjust_exposure(1.0);
        assert_eq!(describe_highlights(&current, Headroom::None), Some(CLIPPED));
        assert_eq!(
            describe_highlights(&current, Headroom::Above),
            None,
            "an HDR surface has room for them, and nothing is being done"
        );

        current.display.tone_map = ToneMap::Neutral;
        for headroom in [Headroom::None, Headroom::Above] {
            assert_eq!(
                describe_highlights(&current, headroom),
                Some("neutral"),
                "{headroom:?}"
            );
        }

        current.display.colormap = Colormap::Viridis;
        for headroom in [Headroom::None, Headroom::Above] {
            assert_eq!(
                describe_highlights(&current, headroom),
                None,
                "{headroom:?}"
            );
        }
    }

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
        assert_eq!(counter(0, 6).as_deref(), Some("1 / 6"));
        assert_eq!(counter(5, 6).as_deref(), Some("6 / 6"));
        assert_eq!(counter(0, 1), None);
        assert_eq!(counter(0, 0), None);
    }
}
