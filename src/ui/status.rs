//! The words in the bars: what the image is, and what the view is doing to
//! it.

use crate::image::display::{AutoWindow, Colormap, Headroom, ToneMap};
use crate::render::{Rect, TextMeasure};

use super::chrome::BAR_PADDING;
use super::{COUNTER_GAP, Current, PADDING, Reading, TEXT_SIZE, capitalized, histogram};

/// Between one segment of a bar and the next. A thin gap: the middot already
/// parts them, and the bars are short of room before they are short of air.
const SEPARATOR: &str = " \u{00b7} ";

/// One run of words in the top bar: where it starts, how much room it was
/// given, and what it says.
pub(super) struct Run {
    pub x: f32,
    /// What the run is cut to: the room left between where it starts and
    /// whatever is set after it.
    pub room: f32,
    /// How wide it actually comes out — its own width, or `room` where it was
    /// too long for the space. What is on screen, and so what the pointer is
    /// answered against.
    pub width: f32,
    pub text: String,
}

impl Run {
    /// The strip of `bar` this run occupies, for the pointer and for anything
    /// that has to be placed against it.
    ///
    /// The bar's whole height rather than the line's: a run of words thirteen
    /// pixels tall is too fine a thing to ask anyone to point at, and the bar
    /// holds nothing above or below it to be confused with.
    pub fn strip(&self, bar: Rect) -> Rect {
        Rect::new(self.x, bar.y, self.width, bar.height)
    }
}

/// The top bar's words, laid out: the count of files, the word for a file
/// that has gone, the name, and the facts about the picture.
///
/// Laid out here rather than inside the frame builder because the pointer has
/// to be answered against the same rectangles between frames, and two
/// readings of where a run of words went are two readings that can disagree.
pub(super) struct TopBar {
    pub counter: Option<Run>,
    pub deleted: Option<Run>,
    pub name: Run,
    pub facts: Run,
}

/// What the top bar has to say about the file, gathered from wherever the
/// caller keeps it: the frame builder has it in a
/// [`FrameInput`](super::FrameInput), and the pointer has to ask the
/// application for it between frames.
pub struct BarText<'a> {
    pub current: &'a Current,
    pub reading: Option<&'a Reading>,
    pub index: usize,
    pub count: usize,
    pub deleted: bool,
}

/// Lays the top bar's words out in `bar`, between `start` — where the two
/// step buttons at its near end leave off — and `limit`, where the buttons at
/// the far end begin.
///
/// Least to most disposable, and the facts are dropped whole rather than
/// clipped: half of "18333 x 15667" is worse than none of it. Half the bar at
/// most, so that the name it is sharing the bar with keeps the other half.
pub(super) fn top_bar(
    text: &mut dyn TextMeasure,
    bar: Rect,
    start: f32,
    limit: f32,
    about: &BarText,
) -> TopBar {
    let run = |text: &mut dyn TextMeasure, x: f32, room: f32, words: String| {
        let width = text.measure_text(&words, TEXT_SIZE)[0].min(room);
        Run {
            x,
            room,
            width,
            text: words,
        }
    };

    let facts = [
        format!(
            "{} \u{00d7} {}",
            about.current.image.width, about.current.image.height
        ),
        describe_pixels(about.current),
        about.current.image.color.label(),
    ];
    let facts = fit_segments(text, &facts, (bar.width / 2.0 - BAR_PADDING * 2.0).max(1.0));
    let facts_width = text.measure_text(&facts, TEXT_SIZE)[0];
    // Clear of the buttons at the end of the bar.
    let facts_x = (limit - PADDING - facts_width).max(start);
    let facts = run(text, facts_x, facts_width, facts);

    // The count is a fact about the list, not part of the name, and is set
    // like the other facts in the bar: the name is the one thing here worth
    // picking out, and picking out two things picks out neither.
    let mut name_x = start;
    let counter = counter(about.index, about.count).map(|counter| {
        let counter = run(text, start, (facts_x - start).max(1.0), counter);
        name_x += counter.width + COUNTER_GAP;
        counter
    });
    // In front of the name, on the side of the bar the name is read from, so
    // that it is seen before the file it is about rather than after it.
    let deleted = about.deleted.then(|| {
        let deleted = run(
            text,
            name_x,
            (facts_x - PADDING - name_x).max(1.0),
            DELETED.to_string(),
        );
        name_x += deleted.width + COUNTER_GAP;
        deleted
    });
    let name = run(
        text,
        name_x,
        (facts_x - PADDING - name_x).max(1.0),
        top_label(&about.current.label, about.reading),
    );

    TopBar {
        counter,
        deleted,
        name,
        facts,
    }
}

/// Joins as many leading segments as fit in `width`, keeping at least the
/// first however narrow the window gets.
pub(super) fn fit_segments(text: &mut dyn TextMeasure, segments: &[String], width: f32) -> String {
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
/// is gone. Set apart in the warning color rather than folded into the name,
/// which is still the name of the file the pixels came from: it is a fact
/// about the file's standing in the world, not part of what it is called —
/// and a file that really is called `DELETED` must not read as this.
pub(super) const DELETED: &str = "DELETED";

/// Where the file on screen comes in the list it was opened with, for in
/// front of its name — or `None` for a single file, "1 / 1" being a count of
/// nothing.
///
/// It leads the bar because it is the one part of the line whose width does
/// not depend on the file, so a reader looking for it always finds it in the
/// same place. It is drawn as its own run rather than as part of the name:
/// the name is what is being looked at and the count is a fact about the
/// list, and the two are set apart to say so.
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

/// The room a press keeps around those words: the wash that comes up under
/// them is a button's wash, and ink laid tight against the letters would not
/// read as one. It is also what the pointer finds them by, a little before it
/// is on them.
pub(super) const STATE_PAD: f32 = 6.0;

/// The words at the far end of the bottom bar, laid out: what is being done
/// to the image, set against whatever ends the bar.
///
/// Laid out here rather than inside the frame builder for the same reason the
/// top bar is — the pointer has to be answered against the same rectangle
/// between frames, and it can be pressed as well as named.
pub(super) struct State {
    /// Where the words start, and how wide they come out.
    pub x: f32,
    pub width: f32,
    /// The line as it is set: as many of the segments as the room took.
    pub text: String,
    /// Where the last segment begins in `text`, when that segment is the
    /// word for highlights being thrown away. It is set bold, so it is drawn
    /// as a run of its own — see [`CLIPPED`].
    pub clipped: Option<usize>,
}

impl State {
    /// The strip of `bar` the words answer the pointer over: their own width
    /// and the room around them, at the bar's whole height.
    ///
    /// As deep as the bar for the reason [`Run::strip`] is: a line of
    /// thirteen-pixel words is too fine a thing to ask anyone to point at,
    /// and there is nothing above or below it here to be confused with.
    pub fn strip(&self, bar: Rect) -> Rect {
        Rect::new(
            self.x - STATE_PAD,
            bar.y,
            self.width + 2.0 * STATE_PAD,
            bar.height,
        )
    }
}

/// Lays those words out in `bar`, ending at `limit` — where the surface
/// switch at the far end leaves off.
///
/// `None` when nothing is being done to the image, which is the ordinary case
/// for a photograph: there is then no line, and nothing there to point at.
///
/// Half the bar at most, and cut by whole segments as the top bar's facts
/// are: the readout of the pixel under the pointer is sharing this bar, and
/// half of `min/max` says less than none of it.
pub(super) fn state(
    text: &mut dyn TextMeasure,
    bar: Rect,
    limit: f32,
    current: &Current,
    headroom: Headroom,
) -> Option<State> {
    let segments = describe_state(current, headroom);
    if segments.is_empty() {
        return None;
    }
    let room = (bar.width / 2.0 - BAR_PADDING * 2.0).max(1.0);
    let line = fit_segments(text, &segments, room);
    let width = text.measure_text(&line, TEXT_SIZE)[0];
    // The bold word is measured in the face the rest of the line is set in,
    // which comes out a hair narrow; what that costs is a hair of the padding
    // in front of the switch, and the run is drawn with the room to the
    // switch rather than with its own width, so nothing is cut off.
    let clipped = (segments.last().is_some_and(|last| last == CLIPPED) && line.ends_with(CLIPPED))
        .then(|| line.len() - CLIPPED.len());
    Some(State {
        x: (limit - width).max(bar.x + BAR_PADDING),
        width,
        text: line,
        clipped,
    })
}

/// What is being done to the image, segment by segment: only the things
/// actually in force, so a viewer left alone says nothing here.
///
/// Neither the zoom nor the fit it came from, nor the filter the image is
/// magnified with: all three are the button in the top bar, which reads the
/// zoom out and opens a menu of the rest. The bar named the filter once, but
/// naming a thing it could not be used to change is worth less than a cell
/// that both says and sets it. Nor which surface the picture is on: that is
/// the button at the end of this bar, lit when it is the HDR one.
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
    use super::super::chrome::Chrome;
    use super::*;
    use crate::image::display::{Display, Startup};
    use crate::image::exif::Exif;
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
        }
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

    /// The words are set against the switch that ends the bar and cut to the
    /// room in front of it by whole segments, so that they never lie over the
    /// switch and never say half of anything.
    #[test]
    fn the_line_is_set_against_the_switch_and_cut_by_whole_segments() {
        let Some(mut fonts) = crate::render::ui_tests::test_fonts() else {
            return;
        };
        let mut current = photograph();
        current.display.auto = AutoWindow::MinMax;
        current.display.adjust_exposure(0.5);

        let chrome = Chrome::new([900.0, 600.0]);
        let bar = chrome.bottom;
        let whole = state(
            &mut fonts,
            bar,
            chrome.state_limit(),
            &current,
            Headroom::None,
        )
        .expect("something is being done to the picture");
        assert_eq!(
            whole.text,
            format!("min/max{SEPARATOR}+\u{00bd} EV{SEPARATOR}{CLIPPED}"),
            "a wide window has room for all of it"
        );
        assert_eq!(
            whole.clipped.map(|at| &whole.text[at..]),
            Some(CLIPPED),
            "and the word for what is becoming of the highlights is the run \
             that is set bold"
        );
        assert!(whole.x + whole.width <= chrome.state_limit());
        assert!(
            whole.strip(bar).right() <= chrome.output_button().x,
            "the room a press keeps around the words stops short of the switch"
        );

        // A window with room for about half the line, whatever face it is set
        // in: the fit is asked to drop something without the test having to
        // know how wide the words come out.
        let room = fonts.measure_text(&whole.text, TEXT_SIZE)[0];
        let chrome = Chrome::new([2.0 * (room * 0.6 + 2.0 * BAR_PADDING), 600.0]);
        let cut = state(
            &mut fonts,
            chrome.bottom,
            chrome.state_limit(),
            &current,
            Headroom::None,
        )
        .expect("the line is still there, shorter");
        assert!(
            cut.text.len() < whole.text.len() && whole.text.starts_with(&cut.text),
            "\"{}\" is not \"{}\" with segments taken off the end",
            cut.text,
            whole.text
        );
        assert_eq!(
            cut.clipped, None,
            "and the word that went is not still being pointed at"
        );
    }

    /// The pointer is answered over those words and nowhere else along the
    /// bar: they name themselves and open the histogram when pressed, and a
    /// picture with nothing being done to it has no words there to reach.
    #[test]
    fn the_pointer_is_answered_over_the_words_and_not_over_the_bar() {
        let Some(mut fonts) = crate::render::ui_tests::test_fonts() else {
            return;
        };
        let chrome = Chrome::new([900.0, 600.0]);
        let bar = chrome.bottom;
        let limit = chrome.state_limit();
        let mut current = photograph();
        fn on(
            fonts: &mut dyn TextMeasure,
            current: &Current,
            bar: Rect,
            limit: f32,
            point: [f32; 2],
        ) -> bool {
            crate::ui::state_hover(fonts, point, bar, limit, current, Headroom::None)
        }

        let middle = [bar.x + bar.width / 2.0, bar.y + bar.height / 2.0];
        assert!(
            !on(&mut fonts, &current, bar, limit, middle),
            "a picture nobody has touched has no words there to point at"
        );

        current.display.auto = AutoWindow::MinMax;
        let words = state(&mut fonts, bar, limit, &current, Headroom::None).expect("a line");
        let strip = words.strip(bar);
        assert!(on(
            &mut fonts,
            &current,
            bar,
            limit,
            [strip.x + strip.width / 2.0, middle[1]]
        ));
        assert!(
            !on(&mut fonts, &current, bar, limit, middle),
            "and the rest of the bar is still the panel it always was"
        );
        assert!(
            !on(
                &mut fonts,
                &current,
                bar,
                limit,
                [strip.right() + 1.0, middle[1]]
            ),
            "the switch at the end of the bar answers for itself"
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
