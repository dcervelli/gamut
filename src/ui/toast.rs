//! The message the window shows about something that has just happened, at
//! the foot of the content area.
//!
//! Two parts: [`Toasts`] is the timing, held by the application and asked on
//! every tick of the loop, and [`show`] is the drawing, done afresh with each
//! frame.
//!
//! What a toast is for is the thing that leaves no other mark. A copy takes
//! the selection and changes nothing on screen; without a word about it there
//! is no way to tell a copy that worked from a key that was never read. So a
//! message says what was taken, and goes on its own after [`LINGER`] — it is
//! about what just happened, and what just happened stops being news.
//!
//! It can also be taken off: by the cross it carries, or by Escape, which
//! dismisses whatever is up before it means anything else. The cross is a
//! [`Control`] like any other.

use std::time::{Duration, Instant};

use egui::{Align, Label, Layout, RichText, Sense, WidgetInfo, WidgetType, vec2};

use crate::theme::Theme;

use super::Rect;
use super::chrome::Pass;
use super::control::Control;
use super::style::TOGGLE_RADIUS;
use super::tooltip::Tip;
use super::{PADDING, PANEL_RADIUS, icon};

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
    pub fn show(&mut self, now: Instant, message: String, level: Level, linger: Duration) {
        self.showing = Some(Toast {
            message,
            level,
            until: now + linger,
        });
    }

    /// Takes off a message that has had its time. Returns whether anything
    /// went, and so whether a redraw is owed.
    pub fn tick(&mut self, now: Instant) -> bool {
        if self
            .showing
            .as_ref()
            .is_some_and(|toast| now >= toast.until)
        {
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

/// Draws `toast` at the foot of `content`, with the cross that takes it off.
///
/// Centered along the foot of the area the panels leave: the middle of the
/// window is where the eye already is, and the foot is the one edge of that
/// area with nothing floating against it. Not in a bar, because a bar
/// describes what is on screen and this describes what was just done. Over
/// the panels that float over the picture, so that a message raised while
/// the histogram is open is still read: it is about what just happened, and
/// nothing on screen outranks that for as long as it is up. A menu is drawn
/// over it still, a menu being the thing that is being looked at while it is
/// open.
///
/// Held to what the area can carry: a long message is cut rather than hung
/// off the side of the window, so the cross is always reachable.
pub(super) fn show(pass: &mut Pass, ui: &mut egui::Ui, toast: &Toast, content: Rect) {
    let theme = pass.theme;
    let held = 2.0 * INSET[0] + GAP + CLOSE;
    let room = content.width - 2.0 * LIFT - held;
    if room <= 0.0 || content.height < CLOSE + 2.0 * INSET[1] + 2.0 * LIFT {
        return;
    }
    let area = egui::Rect::from_min_size(
        egui::pos2(content.x, content.y),
        vec2(content.width, content.height),
    );
    egui::Area::new(egui::Id::new("toast"))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::CENTER_BOTTOM)
        .fixed_pos(egui::pos2(area.center().x, area.max.y - LIFT))
        .constrain_to(area)
        .interactable(true)
        .show(ui.ctx(), |ui| {
            egui::Frame::NONE
                .fill(theme.menu_background.into())
                .stroke(egui::Stroke::new(1.0, theme.border))
                .corner_radius(PANEL_RADIUS)
                .inner_margin(egui::Margin::symmetric(INSET[0] as i8, INSET[1] as i8))
                .show(ui, |ui| {
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing = vec2(GAP, 0.0);
                        ui.set_max_width(room + GAP + CLOSE);
                        ui.add(
                            Label::new(RichText::new(&toast.message).color(toast.level.ink(theme)))
                                .truncate(),
                        );
                        cross(pass, ui);
                    });
                });
        });
}

/// The cross that takes the message off. Never lit: it takes the message off
/// rather than switching anything on, so there is no state for it to be
/// showing.
fn cross(pass: &mut Pass, ui: &mut egui::Ui) {
    let (rect, response) = ui.allocate_exact_size(vec2(CLOSE, CLOSE), Sense::CLICK);
    let (background, ink) = pass.button_ink(false, &response, true);
    ui.painter().rect_filled(rect, TOGGLE_RADIUS, background);
    icon::paint(
        ui.painter(),
        icon::X,
        icon::square(icon::Grid::new(ui.pixels_per_point()), rect, CLOSE - 4.0),
        ink,
        background,
    );
    response
        .widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, Control::Dismiss.label()));
    let response = pass.tooltip(response, Tip::Control(Control::Dismiss), true);
    if response.clicked() {
        pass.press(Control::Dismiss);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raised(message: &str) -> Toasts {
        let mut toasts = Toasts::default();
        toasts.show(Instant::now(), message.to_string(), Level::Message, LINGER);
        toasts
    }

    /// A message goes on its own after its time, and the loop is told when to
    /// come back and look.
    #[test]
    fn a_message_takes_itself_off_when_its_time_is_up() {
        let now = Instant::now();
        let mut toasts = Toasts::default();
        toasts.show(now, "Copied file path.".to_string(), Level::Message, LINGER);

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
            Instant::now(),
            "Copied pixel value.".to_string(),
            Level::Warning,
            LINGER,
        );
        let toast = toasts.showing().expect("one is up");
        assert_eq!(toast.message, "Copied pixel value.");
        assert_eq!(toast.level, Level::Warning);
    }
}
