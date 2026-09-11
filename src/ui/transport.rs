//! The transport bar: the controls a file of frames or pages brings with
//! it, in a bar of their own above the bottom bar.
//!
//! An animation gets the full set — one frame back, play or pause, one frame
//! on, a readout of where the head is, and a timeline to scrub — and a file
//! of pages gets the two steps and a count, since pages have no clock and so
//! nothing to play or to scrub. The timeline is laid out in time rather than
//! in frames, so that a frame shown for a second takes ten times the track a
//! frame shown for a tenth does, and dragging along it runs at the speed the
//! animation plays.
//!
//! What is drawn here is handed in as [`Transport`] and never owned: the
//! application's clock is what knows where the head is, and a press comes
//! back as a [`Command`](super::Command) for it, as every press does.

use std::time::Duration;

use egui::{Align, Label, Layout, RichText, Sense, pos2, vec2};

use super::chrome::{BAR_PADDING, BUTTON_SIZE, Corners, Pass, STEP_SEAM};
use super::control::Control;
use super::style::TOGGLE_RADIUS;
use super::tooltip::Tip;
use super::{PADDING, icon};

/// The bar's state for one frame, as the application knows it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Transport {
    /// The frame or page on screen, from zero.
    pub index: usize,
    pub count: usize,
    pub kind: Kind,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Several pictures and no clock: the steps and the count.
    Pages,
    /// Frames on a clock.
    Animation {
        playing: bool,
        /// How long each frame decoded so far is shown for, in order. The
        /// timeline is laid out from them, and a frame not yet decoded is
        /// given the mean of those that are.
        delays: Vec<Duration>,
    },
}

/// The timeline's track is this tall, in the middle of the bar; the handle
/// on it is a button's height.
const TRACK_HEIGHT: f32 = 4.0;
const HANDLE_WIDTH: f32 = 6.0;
/// The least the timeline is worth drawing at. Narrower than this it is
/// left out, and the buttons and the readout have the bar.
const MIN_TIMELINE: f32 = 60.0;

/// When each frame begins, and when the last one ends: the timeline's
/// layout. Frames whose delay is not yet known are given the mean of those
/// that are, so that the track is the right length from the start and
/// firms up as the frames arrive.
pub fn timeline(delays: &[Duration], count: usize) -> (Vec<Duration>, Duration) {
    let known: Duration = delays.iter().sum();
    let mean = if delays.is_empty() {
        Duration::from_millis(100)
    } else {
        known / delays.len() as u32
    };
    let mut starts = Vec::with_capacity(count);
    let mut at = Duration::ZERO;
    for index in 0..count {
        starts.push(at);
        at += delays.get(index).copied().unwrap_or(mean);
    }
    (starts, at)
}

/// Which frame is under a point `fraction` of the way along the timeline.
pub fn frame_at(starts: &[Duration], total: Duration, fraction: f32) -> usize {
    if starts.is_empty() {
        return 0;
    }
    let time = total.mul_f32(fraction.clamp(0.0, 1.0));
    starts
        .iter()
        .rposition(|&start| start <= time)
        .unwrap_or(0)
        .min(starts.len() - 1)
}

/// Seconds to a tenth, for the readout.
fn seconds(duration: Duration) -> String {
    format!("{:.1} s", duration.as_secs_f64())
}

/// Lays the bar out: the buttons, the readout, and — for an animation —
/// the timeline in whatever width is left.
pub(super) fn show(pass: &mut Pass, ui: &mut egui::Ui, transport: &Transport) {
    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
    let dim: egui::Color32 = pass.theme.text_dim.into();
    ui.horizontal_centered(|ui| {
        ui.add_space(BAR_PADDING);
        let animation = matches!(transport.kind, Kind::Animation { .. });
        let back = pass.icon_button(
            ui,
            icon::STEP_BACK,
            Control::StepBack,
            false,
            true,
            Corners::Leading,
        );
        if back.clicked() {
            pass.press(Control::StepBack);
        }
        ui.add_space(STEP_SEAM);
        if let Kind::Animation { playing, .. } = &transport.kind {
            let marks = if *playing { icon::PAUSE } else { icon::PLAY };
            let play = pass.icon_button(ui, marks, Control::Play, *playing, true, Corners::Middle);
            if play.clicked() {
                pass.press(Control::Play);
            }
            ui.add_space(STEP_SEAM);
        }
        let forward = pass.icon_button(
            ui,
            icon::STEP_FORWARD,
            Control::StepForward,
            false,
            true,
            Corners::Trailing,
        );
        if forward.clicked() {
            pass.press(Control::StepForward);
        }
        ui.add_space(PADDING);

        let counter = format!("{} / {}", transport.index + 1, transport.count);
        let Kind::Animation { delays, .. } = &transport.kind else {
            ui.add(Label::new(RichText::new(counter).color(dim)));
            return;
        };
        let (starts, total) = timeline(delays, transport.count);
        let elapsed = starts.get(transport.index).copied().unwrap_or_default();
        let readout = format!("{counter} \u{b7} {} / {}", seconds(elapsed), seconds(total));
        ui.add(Label::new(RichText::new(readout).color(dim)));
        ui.add_space(PADDING);

        // The timeline has what is left, less the margin at the end of the
        // bar, and is left out where that is too little to scrub.
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(BAR_PADDING);
            let width = ui.available_width();
            if width < MIN_TIMELINE || !animation {
                return;
            }
            let (rect, response) =
                ui.allocate_exact_size(vec2(width, BUTTON_SIZE), Sense::CLICK | Sense::DRAG);
            response.widget_info(|| {
                egui::WidgetInfo::slider(
                    true,
                    transport.index as f64,
                    format!(
                        "Timeline, frame {} of {}",
                        transport.index + 1,
                        transport.count
                    ),
                )
            });
            let grid = icon::Grid::new(ui.pixels_per_point());
            let theme = pass.theme;
            let painter = ui.painter();

            // The track, the played part of it, and the handle over the
            // frame on screen: at the middle of that frame's span, so that
            // a press on the handle lands on the frame it marks.
            let track_height = grid.line_width(TRACK_HEIGHT);
            let track = egui::Rect::from_min_size(
                pos2(rect.min.x, grid.snap(rect.center().y - track_height / 2.0)),
                vec2(rect.width(), track_height),
            );
            painter.rect_filled(track, 0.0, theme.border);
            let span = |index: usize| -> (f32, f32) {
                let start = starts.get(index).copied().unwrap_or_default();
                let end = starts.get(index + 1).copied().unwrap_or(total);
                let fraction = |time: Duration| {
                    if total.is_zero() {
                        0.0
                    } else {
                        (time.as_secs_f32() / total.as_secs_f32()).clamp(0.0, 1.0)
                    }
                };
                (fraction(start), fraction(end))
            };
            let (from, to) = span(transport.index);
            let played = egui::Rect::from_min_max(
                track.min,
                pos2(rect.min.x + rect.width() * (from + to) / 2.0, track.max.y),
            );
            painter.rect_filled(played, 0.0, theme.accent);
            let handle_width = grid.line_width(HANDLE_WIDTH);
            let handle = egui::Rect::from_center_size(
                pos2(grid.snap(played.max.x), rect.center().y),
                vec2(handle_width, BUTTON_SIZE),
            );
            let (_, ink) = pass.button_ink(false, &response, true);
            painter.rect_filled(handle, TOGGLE_RADIUS / 2.0, ink);

            // The button down on the track goes to the frame under the
            // pointer, from the press and for as long as it is held, so a
            // press lands at once and a drag scrubs; once per frame it lands
            // on, since the same frame asked for twice is one ask.
            if (response.is_pointer_button_down_on() || response.dragged())
                && let Some(pointer) = response.interact_pointer_pos()
            {
                let fraction = (pointer.x - rect.min.x) / rect.width().max(1.0);
                let frame = frame_at(&starts, total, fraction);
                if frame != transport.index {
                    pass.press(Control::Seek(frame));
                }
            }
            pass.tooltip(response, Tip::Timeline, true);
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const TENTH: Duration = Duration::from_millis(100);

    /// The track is laid out in time: a long frame takes more of it, and
    /// frames not yet decoded are given the mean of those that are.
    #[test]
    fn the_timeline_is_laid_out_in_time() {
        let delays = [TENTH, 3 * TENTH];
        let (starts, total) = timeline(&delays, 4);
        assert_eq!(starts, [Duration::ZERO, TENTH, 4 * TENTH, 6 * TENTH]);
        assert_eq!(total, 8 * TENTH);

        let (starts, total) = timeline(&[], 2);
        assert_eq!(starts.len(), 2);
        assert!(total > Duration::ZERO, "nothing known still has a length");
    }

    /// A point on the track lands on the frame whose span it is in, and the
    /// ends land on the first and last frames.
    #[test]
    fn a_point_on_the_track_is_the_frame_under_it() {
        let (starts, total) = timeline(&[TENTH, 3 * TENTH, TENTH], 3);
        assert_eq!(frame_at(&starts, total, 0.0), 0);
        assert_eq!(frame_at(&starts, total, 0.1), 0);
        assert_eq!(frame_at(&starts, total, 0.5), 1);
        assert_eq!(frame_at(&starts, total, 0.85), 2);
        assert_eq!(frame_at(&starts, total, 1.0), 2);
        assert_eq!(frame_at(&starts, total, 7.0), 2);
        assert_eq!(frame_at(&[], Duration::ZERO, 0.5), 0);
    }

    #[test]
    fn the_readout_writes_seconds_to_a_tenth() {
        assert_eq!(seconds(Duration::from_millis(1234)), "1.2 s");
        assert_eq!(seconds(Duration::ZERO), "0.0 s");
    }
}
