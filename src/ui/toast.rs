//! The message the window shows about something that has just happened, at
//! the foot of the content area.
//!
//! Two parts, as the tooltip has: [`Toasts`] is the timing, held by the
//! application and asked on every tick of the loop, and [`place`] and
//! [`draw`] are the drawing, done afresh with each frame.
//!
//! What a toast is for is the thing that leaves no other mark. A copy takes
//! the selection and changes nothing on screen; without a word about it there
//! is no way to tell a copy that worked from a key that was never read. So a
//! message says what was taken, and goes on its own after [`LINGER`] — it is
//! about what just happened, and what just happened stops being news.
//!
//! It can also be taken off: by the cross it carries, or by Escape, which
//! dismisses whatever is up before it means anything else. The cross is a
//! [`Widget`](super::Widget) like any other, so the pointer reaches it
//! through [`layers::hit`](super::layers::hit) the way it reaches a button in
//! a bar.
//!
//! The words are measured once, when the toast is raised, rather than on
//! every frame and again for every question the pointer asks. Nothing about a
//! toast changes while it is up — not its text, not the face it is set in —
//! so the frame builder and the hit test can lay it out from the same few
//! numbers without either of them having the fonts to hand.

use std::time::{Duration, Instant};

use crate::render::{Rect, TextMeasure, UiFrame};
use crate::theme::Theme;

use super::buttons::{button_ink, text_top};
use super::menu::CELL_RADIUS;
use super::{PADDING, PANEL_RADIUS, TEXT_SIZE, icon};

/// How long a message stays up unless something takes it off sooner.
///
/// Long enough to be read by someone who was looking somewhere else when it
/// appeared — a copy is asked for with the eyes on the picture, not on the
/// foot of the window — and short enough that it is gone before the next
/// thing is done.
pub const LINGER: Duration = Duration::from_millis(2600);

/// The room around what a toast says.
const INSET: [f32; 2] = [12.0, 8.0];

/// The side of the cross that takes the message off.
const CLOSE: f32 = 18.0;

/// The gap between the words and that cross.
const GAP: f32 = 10.0;

/// How far a toast floats above the foot of the content area, and the least
/// it may come to either side of it. The gap anything floating over the
/// picture keeps from the edge of the area it floats in.
const LIFT: f32 = PADDING;

/// What a message is about, which is the ink it is set in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    /// Something was done: a copy taken, a setting applied. The ordinary
    /// case, and the only one that is not about a difficulty.
    Message,
    /// What was asked for could not be done, and nothing is broken: there was
    /// no pixel under the pointer, nothing on the clipboard to take.
    Warning,
    /// Something failed that should have worked.
    Error,
}

impl Level {
    /// The ink the words are set in. The panel and its edge stay as they are
    /// whatever the level: what a message says is the thing being read, and a
    /// panel colored three ways would be read before the words on it.
    fn ink(self, theme: &Theme) -> crate::render::Color {
        match self {
            Level::Message => theme.text_primary,
            Level::Warning => theme.caution,
            Level::Error => theme.warning,
        }
    }
}

/// One message, with everything about it that does not change while it is up.
#[derive(Clone, Debug)]
pub struct Toast {
    /// What it says.
    pub message: String,
    pub level: Level,
    /// How wide `message` comes out at [`TEXT_SIZE`], measured once when the
    /// toast was raised — see this module's own note on why.
    width: f32,
    /// When it takes itself off.
    until: Instant,
}

/// The message showing at the foot of the window, if any.
///
/// One at a time: a second message is about something that has happened
/// since, which is what the reader wants to be told, and two of them stacked
/// would be a log rather than a word about what was just done.
#[derive(Default)]
pub struct Toasts {
    showing: Option<Toast>,
}

impl Toasts {
    /// Raises `message`, in place of whatever was up.
    ///
    /// `text` is the fonts the interface is set in, which the words are
    /// measured against here and not again.
    pub fn show(
        &mut self,
        text: &mut dyn TextMeasure,
        now: Instant,
        message: String,
        level: Level,
        linger: Duration,
    ) {
        let width = text.measure_text(&message, TEXT_SIZE)[0];
        self.showing = Some(Toast {
            message,
            level,
            width,
            until: now + linger,
        });
    }

    /// Takes off a message that has had its time. Returns whether anything
    /// went, and so whether a redraw is owed.
    pub fn tick(&mut self, now: Instant) -> bool {
        if self.showing.as_ref().is_some_and(|toast| now >= toast.until) {
            self.showing = None;
            return true;
        }
        false
    }

    /// When [`Toasts::tick`] next has something to do, for the loop to sleep
    /// until. `None` when nothing is up.
    pub fn deadline(&self) -> Option<Instant> {
        self.showing.as_ref().map(|toast| toast.until)
    }

    /// Takes the message off now: the cross was pressed, or Escape. Returns
    /// whether there was one to take off.
    pub fn dismiss(&mut self) -> bool {
        self.showing.take().is_some()
    }

    /// What is being said on screen, if anything.
    pub fn showing(&self) -> Option<&Toast> {
        self.showing.as_ref()
    }
}

/// Where a toast and the cross that dismisses it went.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Placed {
    /// The panel the words are on, which is what takes the pointer.
    pub panel: Rect,
    /// The cross at its far end.
    pub close: Rect,
}

/// Where `toast` goes in `area`, or `None` when there is no room for it.
///
/// Centered along the foot of the area the panels leave: the middle of the
/// window is where the eye already is, and the foot is the one edge of that
/// area with nothing floating against it. Not in a bar, because a bar
/// describes what is on screen and this describes what was just done.
///
/// Pure geometry, from the width the words were measured at when the toast
/// was raised — so the frame builder and the hit test lay it out alike.
pub fn place(toast: &Toast, area: Rect) -> Option<Placed> {
    let height = CLOSE.max(TEXT_SIZE * 1.3) + 2.0 * INSET[1];
    if height + 2.0 * LIFT > area.height {
        return None;
    }
    // Held to what the area can carry: a long message is cut rather than
    // hung off the side of the window, and one cut to nothing is not drawn.
    let held = 2.0 * INSET[0] + GAP + CLOSE;
    let width = (held + toast.width).min(area.width - 2.0 * LIFT);
    if width <= held {
        return None;
    }

    let panel = Rect::new(
        area.x + (area.width - width) / 2.0,
        area.bottom() - LIFT - height,
        width,
        height,
    );
    let close = Rect::new(
        panel.right() - INSET[0] - CLOSE,
        panel.y + (height - CLOSE) / 2.0,
        CLOSE,
        CLOSE,
    );
    Some(Placed { panel, close })
}

/// Draws the message and the cross that takes it off.
///
/// Over the panels that float over the picture, so that a message raised
/// while the histogram is open is still read: it is about what just happened,
/// and nothing on screen outranks that for as long as it is up. The menu is
/// drawn after this and so still covers it, a menu being the thing that is
/// being looked at while it is open.
pub fn draw(
    frame: &mut UiFrame,
    text: &mut dyn TextMeasure,
    toast: &Toast,
    placed: Placed,
    hover: bool,
    theme: &Theme,
) {
    let Placed { panel, close } = placed;
    let edge = frame.line_width(1.0);
    let room = (close.x - GAP - (panel.x + INSET[0])).max(0.0);

    frame.over(|frame| {
        frame.rounded_rect(panel, PANEL_RADIUS, theme.menu_background);
        // On the panel's own outline rather than around it, as a tooltip's
        // is, so the edge is the width of the panel and not a hair wider.
        frame.stroke_rect(
            panel.inset(edge / 2.0, edge / 2.0),
            PANEL_RADIUS,
            edge,
            theme.border,
        );
        frame.text_clipped(
            [
                frame.snap(panel.x + INSET[0]),
                text_top(frame, text, panel, TEXT_SIZE),
            ],
            TEXT_SIZE,
            toast.level.ink(theme),
            room,
            toast.message.clone(),
        );

        // Never lit: it takes the message off rather than switching anything
        // on, so there is no state for it to be showing.
        let (background, ink) = button_ink(false, hover, theme);
        frame.rounded_rect(close, CELL_RADIUS, background);
        icon::draw(
            frame,
            icon::X,
            icon::fit(frame, close, CLOSE - 4.0),
            ink,
            background,
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::Monospace;

    const AREA: Rect = Rect {
        x: 0.0,
        y: 30.0,
        width: 800.0,
        height: 500.0,
    };

    fn raised(message: &str) -> Toasts {
        let mut toasts = Toasts::default();
        toasts.show(
            &mut Monospace,
            Instant::now(),
            message.to_string(),
            Level::Message,
            LINGER,
        );
        toasts
    }

    /// A message goes on its own after its time, and the loop is told when to
    /// come back and look.
    #[test]
    fn a_message_takes_itself_off_when_its_time_is_up() {
        let now = Instant::now();
        let mut toasts = Toasts::default();
        toasts.show(
            &mut Monospace,
            now,
            "Copied file path.".to_string(),
            Level::Message,
            LINGER,
        );

        assert_eq!(toasts.deadline(), Some(now + LINGER));
        assert!(!toasts.tick(now + LINGER - Duration::from_millis(1)));
        assert!(toasts.showing().is_some());

        assert!(toasts.tick(now + LINGER));
        assert!(toasts.showing().is_none());
        assert_eq!(toasts.deadline(), None);
        // Nothing left to take off, so nothing more is owed.
        assert!(!toasts.tick(now + LINGER));
    }

    /// The cross, and Escape, take it off before its time — and say whether
    /// there was anything to take off, which is what decides whether Escape
    /// goes on to mean something else.
    #[test]
    fn a_message_can_be_taken_off_early() {
        let mut toasts = raised("Copied file path.");
        assert!(toasts.dismiss());
        assert!(toasts.showing().is_none());
        assert!(!toasts.dismiss());
    }

    /// A second message is about what has happened since, and replaces the
    /// first rather than queueing behind it.
    #[test]
    fn the_newest_message_is_the_one_showing() {
        let mut toasts = raised("Copied file path.");
        toasts.show(
            &mut Monospace,
            Instant::now(),
            "Copied pixel value.".to_string(),
            Level::Warning,
            LINGER,
        );
        let toast = toasts.showing().expect("one is up");
        assert_eq!(toast.message, "Copied pixel value.");
        assert_eq!(toast.level, Level::Warning);
    }

    /// Centered along the foot of the area, with the cross inside it and the
    /// words clear of the cross.
    #[test]
    fn a_message_sits_at_the_foot_of_the_area() {
        let toasts = raised("Copied file path.");
        let placed = place(toasts.showing().expect("one is up"), AREA).expect("room");

        let middle = placed.panel.x + placed.panel.width / 2.0;
        assert!((middle - (AREA.x + AREA.width / 2.0)).abs() < 0.5);
        assert!((placed.panel.bottom() - (AREA.bottom() - LIFT)).abs() < 0.5);
        assert!(AREA.contains([middle, placed.panel.y + 1.0]));

        assert!(placed.close.right() <= placed.panel.right());
        assert!(placed.close.x > placed.panel.x + INSET[0]);
    }

    /// A message longer than the window is cut to it rather than hung off
    /// either side, so the cross that takes it off is always reachable.
    #[test]
    fn a_long_message_is_held_inside_the_area() {
        let toasts = raised(&"a very long message ".repeat(20));
        let placed = place(toasts.showing().expect("one is up"), AREA).expect("room");

        assert!(placed.panel.x >= AREA.x + LIFT - 0.01);
        assert!(placed.panel.right() <= AREA.right() - LIFT + 0.01);
        assert!(AREA.contains([placed.close.x + 1.0, placed.close.y + 1.0]));
    }

    /// A window with no room for the panel gets no panel, rather than one
    /// drawn over the bars or squeezed to nothing.
    #[test]
    fn a_window_with_no_room_shows_nothing() {
        let toasts = raised("Copied file path.");
        let toast = toasts.showing().expect("one is up");
        assert!(place(toast, Rect::new(0.0, 0.0, 800.0, 20.0)).is_none());
        assert!(place(toast, Rect::new(0.0, 0.0, 60.0, 500.0)).is_none());
    }
}
