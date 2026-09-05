//! The label that names what the pointer is resting on: when it appears,
//! what it says, and where it goes.
//!
//! Three parts that barely know about each other. [`Tooltips`] is the timing —
//! pure state over [`Instant`]s, held by the application and asked on every
//! motion and on every tick of the loop. [`Tooltip`] is what one says, which
//! the application composes because most of a tooltip is the key that does
//! the same thing and the keys are the application's. [`Tips`] is the
//! drawing, made afresh with each frame.
//!
//! Anything the pointer can be over can have one. A thing earns a tooltip by
//! becoming a [`Tip`] the pointer can be answered with and by the application
//! having something to say about it; whoever draws it says where it went this
//! frame with [`Tips::offer`], and nothing else is needed. The saying and the
//! placing are separate because the rectangle a thing occupies is not known
//! until the frame is laid out, and by then the pointer has long since been
//! answered.

use std::time::{Duration, Instant};

use crate::image::display::Colormap;
use crate::render::{Rect, TextMeasure, UiFrame};
use crate::theme::Theme;

use super::buttons::text_top;
use super::menu::CELL_RADIUS;
use super::{Panels, TEXT_SIZE, Widget};

/// How long the pointer has to rest before a tooltip opens.
///
/// Resting rather than merely being there: the wait starts again with every
/// motion, so a pointer crossing a button on its way somewhere else never
/// opens one, however slowly it crosses.
const DELAY: Duration = Duration::from_millis(500);

/// How long a tooltip that has just closed leaves the next one ready to open
/// at once.
///
/// Someone reading the labels along a bar is reading, not waiting, and having
/// to wait again at every button would make the row unreadable. Long enough
/// to cover the gap between two neighbours and the corner between a bar and a
/// side panel; short enough that a pointer that has gone somewhere else and
/// come back is starting again rather than continuing.
const GRACE: Duration = Duration::from_millis(300);

/// The gap between the thing and the tooltip naming it. Fixed, and measured
/// from the thing rather than from the pointer: a label that followed the
/// pointer would move while it was being read, and one that appeared at a
/// different distance each time would have to be looked for.
const OFFSET: f32 = 6.0;

/// The room around the words inside the tooltip.
const PADDING: [f32; 2] = [8.0, 5.0];

/// The least a tooltip may come to the edge of the area it is placed in.
const MARGIN: f32 = 4.0;

/// How far apart the lines of a tooltip that has more than one are set.
const LINE: f32 = TEXT_SIZE * 1.35;

/// Something in the interface that names itself when the pointer rests on it.
///
/// A widget is one; so is a run of words in a bar that has more to say than
/// there is room for. What the pointer is answered with, so anything that can
/// be pointed at can become one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tip {
    /// A button, a toggle, or a cell of the menu that is open.
    Widget(Widget),
    /// The file's own name in the top bar, which is cut to the room the bar
    /// has and stands for a path that is usually longer.
    Name,
    /// The count of files beside it.
    Counter,
}

/// What a tooltip says: the thing itself on the first line, and under it the
/// keys that do the same job.
///
/// Composed by the application rather than here, because almost every line of
/// one comes out of the key table — a tooltip and `--help` should never be
/// able to disagree about which key does what.
pub struct Tooltip {
    /// What it is about, which is whose rectangle it hangs from.
    pub at: Tip,
    /// The line that names the thing.
    pub title: String,
    /// The lines under it, set dimmer: what to press instead.
    pub hints: Vec<String>,
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
pub fn words(tip: Tip, panels: &Panels) -> Option<&'static str> {
    Some(match tip {
        Tip::Widget(Widget::Zoom) => "Zoom, fit and filter",
        Tip::Widget(Widget::Cell(index)) => panels.menu?.cell_tip(index)?.label,
        Tip::Widget(Widget::Paste) => "Paste a picture",
        // No one key does this and only this — Escape dismisses whatever is
        // up, a menu first — so the cross names itself.
        Tip::Widget(Widget::Dismiss) => "Dismiss this message",
        // The histogram panel's, in as few words as will carry them.
        Tip::Widget(Widget::Luma) => "Luminance plane",
        Tip::Widget(Widget::Planes) => "Color planes",
        Tip::Widget(Widget::Log) => "Logarithmic counts",
        Tip::Widget(Widget::Reset) => "Reset the display",
        // Named rather than merely shown: a swatch of viridis is a green
        // rectangle that could be anything, and the map has a name people
        // ask for it by — the same one `--colormap` takes.
        Tip::Widget(Widget::Ramp(index)) => match Colormap::ALL.get(index)? {
            Colormap::Gray => "No false color",
            Colormap::Viridis => "Viridis",
            Colormap::Magma => "Magma",
            Colormap::Turbo => "Turbo",
        },
        Tip::Widget(_) | Tip::Name | Tip::Counter => return None,
    })
}

/// When the tooltip opens and when it closes, in the terms the pointer
/// arrives in.
///
/// Held by the application across frames, since it is the one thing about the
/// interface that depends on how long something has been true rather than on
/// what is true now.
#[derive(Default)]
pub struct Tooltips {
    /// What the pointer is resting on and when the rest began — the countdown
    /// [`Tooltips::tick`] is watching, restarted by every motion. `None` when
    /// the pointer is on nothing that names itself.
    resting: Option<(Tip, Instant)>,
    /// Whether the tooltip for it is open.
    open: bool,
    /// Set by a press: the tooltip for what is being rested on is not to open
    /// again until the pointer has been somewhere else. Without it a label
    /// would come back over the panel the press had just opened, the pointer
    /// having gone nowhere.
    blocked: bool,
    /// Until when the next tooltip opens the moment the pointer arrives,
    /// rather than after [`DELAY`]. Set when an open tooltip closes — see
    /// [`GRACE`].
    warm: Option<Instant>,
}

impl Tooltips {
    /// Follows the pointer: `target` is what is under it, or `None` where it
    /// is over nothing that names itself. Returns whether the tooltip on
    /// screen changed, and so whether the frame is out of date.
    ///
    /// Called for every motion, not only when the thing under the pointer
    /// changes: motion over the thing already being timed is what restarts
    /// the wait.
    pub fn point(&mut self, now: Instant, target: Option<Tip>) -> bool {
        let showing = self.showing();
        match target {
            // Off everything. An open tooltip closes and leaves the next one
            // warm; one that had not opened yet leaves nothing behind, there
            // being nothing yet to carry on from.
            None => {
                if self.open {
                    self.warm = Some(now + GRACE);
                }
                self.resting = None;
                self.open = false;
                self.blocked = false;
            }
            Some(tip) => match self.resting {
                // Still on the same one. The wait starts again — a pointer in
                // motion is not resting — but an open tooltip stays open: it
                // is being read, and the hand on the mouse is not always
                // still.
                Some((resting, _)) if resting == tip => {
                    if !self.open && !self.blocked {
                        self.resting = Some((tip, now));
                    }
                }
                // Arrived somewhere new, from another tooltip or from
                // nothing.
                _ => {
                    self.blocked = false;
                    // Straight from one that is open, or across a gap narrow
                    // enough to still be warm: either way the reader is going
                    // along the row and the next label follows at once.
                    self.open = self.open || self.warm.is_some_and(|until| now < until);
                    self.resting = Some((tip, now));
                }
            },
        }
        self.showing() != showing
    }

    /// Opens the tooltip for a pointer that has rested long enough. Returns
    /// whether anything changed, and so whether a redraw is owed.
    pub fn tick(&mut self, now: Instant) -> bool {
        match self.resting {
            Some((_, since)) if self.waiting() && now.duration_since(since) >= DELAY => {
                self.open = true;
                true
            }
            _ => false,
        }
    }

    /// Whether a wait is actually running: something under the pointer, no
    /// tooltip up for it yet, and no press having just said not to.
    fn waiting(&self) -> bool {
        self.resting.is_some() && !self.open && !self.blocked
    }

    /// When [`Tooltips::tick`] next has something to do, for the loop to
    /// sleep until. `None` when nothing is being timed.
    ///
    /// Only the wait is timed. The warm window needs no wake-up of its own:
    /// nothing on screen changes when it lapses, and it is looked at only
    /// when the pointer arrives somewhere.
    pub fn deadline(&self) -> Option<Instant> {
        match self.resting {
            Some((_, since)) if self.waiting() => Some(since + DELAY),
            _ => None,
        }
    }

    /// Takes the tooltip off and stops it coming back while the pointer is
    /// still where it was, for a press: what the button does is the answer to
    /// what it is, and a label still standing over a panel the press has just
    /// opened is in the way.
    ///
    /// Returns whether anything was on screen to take off.
    pub fn dismiss(&mut self) -> bool {
        let showing = self.showing().is_some();
        self.blocked = self.resting.is_some();
        self.open = false;
        self.warm = None;
        showing
    }

    /// What is being named on screen, if anything.
    pub fn showing(&self) -> Option<Tip> {
        self.resting.filter(|_| self.open).map(|(tip, _)| tip)
    }
}

/// Where the thing being named was drawn this frame.
///
/// Made at the top of a frame from what the application settled on and filled
/// in by whoever draws that thing, so that the tooltip lands against the
/// rectangle the frame actually used rather than against a second reading of
/// where the thing goes.
/// Which way a tooltip opens off the thing it names, for a thing that is not
/// pinned to an edge of the content area. Where it is pinned to one — a
/// button in the chrome, a run of words in a bar — the edge decides and this
/// is not consulted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Opens {
    /// Under it, the way a button in a bar is named.
    Below,
    /// Beside it, into whatever it sits on. What the column of toggles down
    /// the histogram panel wants: the label reads across the plot the toggle
    /// acts on, rather than leaving the panel to be read.
    Right,
}

pub struct Tips<'a> {
    naming: Option<&'a Tooltip>,
    /// Where it went, once whoever drew it has said, and which way it opens.
    at: Option<(Rect, Opens)>,
}

impl<'a> Tips<'a> {
    /// `showing` is the tooltip the application settled on, if any.
    pub fn new(showing: Option<&'a Tooltip>) -> Self {
        Self {
            naming: showing,
            at: None,
        }
    }

    /// Said by whoever draws `tip`: this is the rectangle it occupies this
    /// frame. Costs nothing for the things that are not being named, which is
    /// all of them but at most one.
    pub fn offer(&mut self, tip: Tip, rect: Rect) {
        self.offer_toward(tip, rect, Opens::Below);
    }

    /// As [`Tips::offer`], for a thing that wants its label somewhere other
    /// than under it — see [`Opens`].
    pub fn offer_toward(&mut self, tip: Tip, rect: Rect, opens: Opens) {
        if self.naming.is_some_and(|tooltip| tooltip.at == tip) {
            self.at = Some((rect, opens));
        }
    }

    /// Draws the tooltip, over everything else in the frame. Does nothing
    /// when there is none to draw, or when what it would name was not drawn.
    ///
    /// `area` is what the panels leave in the middle of the window, which is
    /// where a tooltip goes: it is about a thing in the chrome, so it belongs
    /// off the chrome and over the picture, where nothing else is competing
    /// for the space.
    pub fn draw(&self, frame: &mut UiFrame, text: &mut dyn TextMeasure, area: Rect, theme: &Theme) {
        let (Some(tooltip), Some((anchor, opens))) = (self.naming, self.at) else {
            return;
        };
        if anchor.width <= 0.0 || anchor.height <= 0.0 {
            return;
        }
        let lines: Vec<&str> = std::iter::once(tooltip.title.as_str())
            .chain(tooltip.hints.iter().map(String::as_str))
            .collect();
        let widest = lines
            .iter()
            .map(|line| text.measure_text(line, TEXT_SIZE)[0])
            .fold(0.0, f32::max);
        let size = [
            widest + 2.0 * PADDING[0],
            lines.len() as f32 * LINE + 2.0 * PADDING[1],
        ];
        let rect = place(size, anchor, opens, area);

        let edge = frame.line_width(1.0);
        // Every line is levelled on the capitals of the box it is given, so
        // that a hint with no descenders sits where a title with them does.
        let tops: Vec<f32> = (0..lines.len())
            .map(|line| {
                let box_ = Rect::new(rect.x, rect.y + PADDING[1] + line as f32 * LINE, 0.0, LINE);
                text_top(frame, text, box_, TEXT_SIZE)
            })
            .collect();
        let left = frame.snap(rect.x + PADDING[0]);

        frame.topmost(|frame| {
            frame.rounded_rect(rect, CELL_RADIUS, theme.menu_background);
            // On the panel's own outline rather than around it, so the edge
            // is the width of the tooltip rather than a hair wider.
            frame.stroke_rect(
                rect.inset(edge / 2.0, edge / 2.0),
                CELL_RADIUS,
                edge,
                theme.border,
            );
            for (line, (words, top)) in lines.iter().zip(tops).enumerate() {
                // The thing leads; what to press instead is set back, the way
                // the bars set a fact behind the name it is about.
                let ink = if line == 0 {
                    theme.text_primary
                } else {
                    theme.text_dim
                };
                frame.text([left, top], TEXT_SIZE, ink, *words);
            }
        });
    }
}

/// Where a tooltip of `size` goes for the thing at `anchor`, given the `area`
/// the panels leave in the middle of the window and which way the thing wants
/// its label to open.
///
/// A thing pinned to an edge of that area is named on the side of it that
/// faces the area, so its label always opens into the picture rather than
/// along the chrome: below a button in the top bar, to the right of one down
/// the left panel, and so on. That is also the side with room, the chrome
/// being one button thick everywhere. `opens` settles the rest, which is
/// everything floating inside the area.
///
/// Then slid along its own edge to stay inside the area, so that a button in
/// a corner is named beside itself rather than off the screen, and flipped to
/// the far side of the thing where even that leaves it hanging out.
fn place(size: [f32; 2], anchor: Rect, opens: Opens, area: Rect) -> Rect {
    let across = middle(anchor.x, anchor.width, size[0]);
    let down = middle(anchor.y, anchor.height, size[1]);
    // Which way the area lies from the thing, where the thing is at an edge
    // of it at all.
    let (x, y) = if anchor.right() <= area.x {
        (anchor.right() + OFFSET, down)
    } else if anchor.x >= area.right() {
        (anchor.x - OFFSET - size[0], down)
    } else if anchor.y >= area.bottom() {
        (across, anchor.y - OFFSET - size[1])
    } else if anchor.bottom() <= area.y {
        (across, anchor.bottom() + OFFSET)
    } else {
        match opens {
            Opens::Below => (across, anchor.bottom() + OFFSET),
            Opens::Right => (anchor.right() + OFFSET, down),
        }
    };

    // Sliding along the edge it hangs from is free; crossing to the other
    // side of the thing is not, so it is only done where the tooltip would
    // otherwise be left hanging outside the area altogether.
    let (x, y) = (
        flipped(x, size[0], anchor.x, anchor.width, area.x, area.width),
        flipped(y, size[1], anchor.y, anchor.height, area.y, area.height),
    );
    Rect::new(
        clamp_within(x, size[0], area.x, area.width),
        clamp_within(y, size[1], area.y, area.height),
        size[0],
        size[1],
    )
}

/// `length` centered on a thing of `span` starting at `start`.
fn middle(start: f32, span: f32, length: f32) -> f32 {
    start + (span - length) / 2.0
}

/// `start` taken to the other side of the thing at `anchor` when what it puts
/// there does not fit between `from` and `from + span`, and left alone when it
/// does — or when the other side is no better.
fn flipped(start: f32, length: f32, anchor: f32, thickness: f32, from: f32, span: f32) -> f32 {
    let fits = |at: f32| at >= from && at + length <= from + span;
    if fits(start) {
        return start;
    }
    let other = if start < anchor {
        anchor + thickness + OFFSET
    } else {
        anchor - OFFSET - length
    };
    if fits(other) { other } else { start }
}

/// `start` moved as little as it takes for `length` to sit inside the span
/// from `from` with [`MARGIN`] to spare at either end. A tooltip longer than
/// the span has to overhang somewhere, and overhangs the end it is read
/// towards.
fn clamp_within(start: f32, length: f32, from: f32, span: f32) -> f32 {
    start.min(from + span - MARGIN - length).max(from + MARGIN)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::chrome::{BAR_HEIGHT, Chrome, content_area};

    const WINDOW: [f32; 2] = [1000.0, 700.0];

    /// A clock a test can name moments on. One base per test, since two
    /// readings of the real clock are never the same moment and these tests
    /// are about the differences between moments.
    struct Clock(Instant);

    impl Clock {
        fn new() -> Self {
            // Far enough in that no test's arithmetic runs off the start of
            // the clock, whatever the platform's epoch for one is.
            Self(Instant::now() + Duration::from_secs(60))
        }

        fn at(&self, millis: u64) -> Instant {
            self.0 + Duration::from_millis(millis)
        }
    }

    fn tip(widget: Widget) -> Tip {
        Tip::Widget(widget)
    }

    /// The pointer has to rest, not merely arrive: the wait is the whole
    /// point, and it starts again with every motion, so a pointer crossing a
    /// button never opens one.
    #[test]
    fn a_tooltip_opens_only_after_the_pointer_has_rested() {
        let clock = Clock::new();
        let mut tips = Tooltips::default();

        assert!(!tips.point(clock.at(0), Some(tip(Widget::Info))));
        assert!(!tips.tick(clock.at(400)));
        assert_eq!(tips.showing(), None);

        // Motion over the same button, and the wait starts from there.
        assert!(!tips.point(clock.at(400), Some(tip(Widget::Info))));
        assert!(!tips.tick(clock.at(800)));

        assert!(tips.tick(clock.at(900)));
        assert_eq!(tips.showing(), Some(tip(Widget::Info)));
        // And it stays up under a hand that is not quite still.
        assert!(!tips.point(clock.at(1000), Some(tip(Widget::Info))));
        assert_eq!(tips.showing(), Some(tip(Widget::Info)));
    }

    /// The loop sleeps until the wait is up, and has nothing to wake for once
    /// the tooltip is open or the pointer is off everything.
    #[test]
    fn the_loop_is_told_when_the_wait_is_up() {
        let clock = Clock::new();
        let mut tips = Tooltips::default();
        assert_eq!(tips.deadline(), None);

        tips.point(clock.at(0), Some(tip(Widget::Zoom)));
        assert_eq!(tips.deadline(), Some(clock.at(0) + DELAY));

        tips.tick(clock.at(0) + DELAY);
        assert_eq!(tips.deadline(), None);

        tips.point(clock.at(1000), None);
        assert_eq!(tips.deadline(), None);
    }

    /// Along a row of buttons the labels follow the pointer at once: the
    /// reader is reading, not waiting. The tolerance is for the gap between
    /// two of them, which the pointer is over nothing at all.
    #[test]
    fn a_tooltip_already_open_moves_to_the_next_button_at_once() {
        let clock = Clock::new();
        let mut tips = Tooltips::default();
        tips.point(clock.at(0), Some(tip(Widget::Histogram)));
        tips.tick(clock.at(600));
        assert_eq!(tips.showing(), Some(tip(Widget::Histogram)));

        // Straight from one to the next.
        assert!(tips.point(clock.at(700), Some(tip(Widget::Info))));
        assert_eq!(tips.showing(), Some(tip(Widget::Info)));

        // And across the gap between them, which takes the pointer over the
        // panel behind and off it again.
        assert!(tips.point(clock.at(800), None));
        assert_eq!(tips.showing(), None);
        assert!(tips.point(clock.at(900), Some(tip(Widget::Histogram))));
        assert_eq!(tips.showing(), Some(tip(Widget::Histogram)));
    }

    /// Off everything for longer than that, and the next button is being
    /// pointed at rather than read past: it waits its turn like the first.
    #[test]
    fn leaving_for_long_enough_starts_the_wait_again() {
        let clock = Clock::new();
        let mut tips = Tooltips::default();
        tips.point(clock.at(0), Some(tip(Widget::Histogram)));
        tips.tick(clock.at(600));

        tips.point(clock.at(600), None);
        let returned = clock.at(600) + GRACE + Duration::from_millis(1);
        assert!(!tips.point(returned, Some(tip(Widget::Info))));
        assert_eq!(tips.showing(), None);
        assert!(!tips.tick(returned + DELAY - Duration::from_millis(1)));
        assert!(tips.tick(returned + DELAY));
    }

    /// A tooltip that never opened leaves nothing warm behind it: the reader
    /// was not reading labels, so the next one waits.
    #[test]
    fn a_wait_that_was_never_served_leaves_nothing_behind() {
        let clock = Clock::new();
        let mut tips = Tooltips::default();
        tips.point(clock.at(0), Some(tip(Widget::Histogram)));
        tips.point(clock.at(100), None);
        tips.point(clock.at(150), Some(tip(Widget::Info)));
        assert_eq!(tips.showing(), None);
        assert!(!tips.tick(clock.at(400)));
        assert!(tips.tick(clock.at(650)));
    }

    /// A press answers what the button is by doing it, so the label goes —
    /// and stays gone while the pointer is still on the button it pressed,
    /// however long it rests there.
    #[test]
    fn a_press_takes_the_tooltip_off() {
        let clock = Clock::new();
        let mut tips = Tooltips::default();
        tips.point(clock.at(0), Some(tip(Widget::Grid)));
        tips.tick(clock.at(600));

        assert!(tips.dismiss());
        assert_eq!(tips.showing(), None);
        assert_eq!(tips.deadline(), None);
        assert!(!tips.dismiss());

        tips.point(clock.at(700), Some(tip(Widget::Grid)));
        assert_eq!(tips.deadline(), None);
        assert!(!tips.tick(clock.at(2000)));

        // Off it and back, and it is being pointed at again.
        tips.point(clock.at(2100), None);
        tips.point(clock.at(2200), Some(tip(Widget::Grid)));
        assert!(tips.tick(clock.at(2200) + DELAY));
    }

    fn tooltip(at: Tip) -> Tooltip {
        Tooltip {
            at,
            title: "Toggle the histogram (h)".into(),
            hints: Vec::new(),
        }
    }

    /// Only the thing being named is collected: a tooltip for a button that
    /// went off the screen this frame has nothing to hang from.
    #[test]
    fn only_the_thing_being_named_is_collected() {
        let rect = Rect::new(10.0, 10.0, 22.0, 22.0);
        let about = tooltip(tip(Widget::Info));

        let mut tips = Tips::new(Some(&about));
        tips.offer(tip(Widget::Histogram), rect);
        assert_eq!(tips.at, None);
        tips.offer(Tip::Name, rect);
        assert_eq!(tips.at, None);
        tips.offer(tip(Widget::Info), rect);
        assert_eq!(tips.at, Some((rect, Opens::Below)));

        let mut tips = Tips::new(None);
        tips.offer(tip(Widget::Info), rect);
        assert_eq!(tips.at, None);
    }

    /// A tooltip opens into the picture, whichever edge of the window its
    /// button is pinned to: below one in a bar, and beside one in a side
    /// panel. Never along the chrome, where the neighbouring buttons are.
    #[test]
    fn a_tooltip_opens_towards_the_middle_of_the_window() {
        let chrome = Chrome::new(WINDOW);
        let area = content_area(WINDOW, true);
        let size = [120.0, 24.0];

        // Each one clear of the thing it names, on the side of it the
        // picture is on — never along the chrome, where the neighbouring
        // buttons are. Clear by at least the offset, and by more where the
        // area's own margin pushes it further in.
        let beside = place(size, chrome.minimap_button, Opens::Below, area);
        assert!(
            beside.x >= chrome.minimap_button.right() + OFFSET,
            "{beside:?}"
        );

        let inside = place(size, chrome.histogram_button, Opens::Below, area);
        assert!(
            inside.right() <= chrome.histogram_button.x - OFFSET,
            "{inside:?}"
        );

        let button = chrome.zoom_button(None);
        let under = place(size, button, Opens::Below, area);
        assert!(under.y >= button.bottom() + OFFSET, "{under:?}");

        // And a strip of words in the bottom bar is named above itself.
        let readout = Rect::new(300.0, WINDOW[1] - BAR_HEIGHT, 90.0, BAR_HEIGHT);
        let over = place(size, readout, Opens::Below, area);
        assert!(over.bottom() <= readout.y - OFFSET, "{over:?}");
    }

    /// Wherever it opens, it stays inside the area it was given: a button in
    /// a corner is named beside itself rather than off the screen.
    #[test]
    fn a_tooltip_stays_inside_the_area() {
        let chrome = Chrome::new(WINDOW);
        let area = content_area(WINDOW, true);
        for anchor in [
            chrome.minimap_button,
            chrome.paste_button,
            chrome.histogram_button,
            chrome.info_button,
            chrome.grid_button(Some("50 px")),
            chrome.zoom_button(None),
            Rect::new(0.0, 0.0, 60.0, BAR_HEIGHT),
        ] {
            for size in [[120.0, 24.0], [420.0, 56.0]] {
                let tip = place(size, anchor, Opens::Below, area);
                assert!(tip.x >= area.x + MARGIN, "{tip:?} in {area:?}");
                assert!(tip.right() <= area.right() - MARGIN, "{tip:?} in {area:?}");
                assert!(tip.y >= area.y + MARGIN, "{tip:?} in {area:?}");
                assert!(
                    tip.bottom() <= area.bottom() - MARGIN,
                    "{tip:?} in {area:?}"
                );
            }
        }
    }

    /// The histogram panel's toggles are named beside themselves, into the
    /// plot they sit on: the label is about the plot, and reading it should
    /// not mean looking away from what it is about.
    #[test]
    fn a_toggle_on_the_histogram_panel_is_named_across_the_plot() {
        let area = content_area(WINDOW, true);
        let panel = crate::ui::histogram::panel(area);
        let toggle = Rect::new(panel.x + 10.0, panel.y + 30.0, 22.0, 22.0);
        let tip = place([150.0, 24.0], toggle, Opens::Right, area);

        assert_eq!(tip.x, toggle.right() + OFFSET);
        assert!(tip.right() <= panel.right(), "{tip:?} on {panel:?}");
        // Level with the toggle it names.
        let (middle, of) = (tip.y + tip.height / 2.0, toggle.y + toggle.height / 2.0);
        assert!((middle - of).abs() < 0.01, "{tip:?} beside {toggle:?}");
    }

    /// Except where there is no room beside it: the swatch at the panel's far
    /// edge is named back across the row rather than off the screen.
    #[test]
    fn a_swatch_with_no_room_beside_it_is_named_the_other_way() {
        let area = content_area(WINDOW, true);
        let panel = crate::ui::histogram::panel(area);
        let swatch = Rect::new(panel.right() - 70.0, panel.bottom() - 20.0, 60.0, 10.0);
        let tip = place([150.0, 24.0], swatch, Opens::Right, area);

        assert!(
            tip.right() <= swatch.x - OFFSET,
            "{tip:?} left of {swatch:?}"
        );
        assert!(tip.x >= area.x + MARGIN, "{tip:?} in {area:?}");
    }

    /// A cell of an open menu is inside the area already, so it is named
    /// under itself the way a bar's button is.
    #[test]
    fn something_already_in_the_middle_is_named_under_itself() {
        let area = content_area(WINDOW, true);
        let cell = Rect::new(500.0, 300.0, 56.0, 27.0);
        let tip = place([120.0, 24.0], cell, Opens::Below, area);

        assert_eq!(tip.y, cell.bottom() + OFFSET);
        let (middle, of) = (tip.x + tip.width / 2.0, cell.x + cell.width / 2.0);
        assert!((middle - of).abs() < 0.01, "{tip:?} under {cell:?}");
    }
}
