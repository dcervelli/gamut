//! The one slider the interface draws, wherever a value is set along a
//! line: a groove, the run from where the value starts to the handle filled
//! in the accent so that it reads as a length before the number is read,
//! and a handle — the same handle the histogram's band wears, since it is
//! the same kind of thing, a value on a line. The histogram's exposure is
//! one; the export dialog's quality another.
//!
//! Dragged to the pointer rather than by it, so that a press anywhere on
//! the row puts the value there and a hand run off the end leaves it at the
//! end; snapped to the step the value counts in; and asked for only when it
//! would change something, so that a hand resting still is not a frame a
//! second.

use egui::{Color32, CursorIcon, Sense, WidgetInfo, pos2, vec2};

use super::Rect;
use super::chrome::Pass;
use super::icon;
use super::outline;
use super::tooltip::Tip;
use crate::render::Color;
use crate::theme::Theme;

/// A handle's mark: how wide it is, and how wide the room around it that
/// takes the pointer is — wider than the mark, since a mark five pixels
/// wide is not something a hand lands on.
pub(super) const HANDLE_WIDTH: f32 = 5.0;
pub(super) const HANDLE_GRIP: f32 = 14.0;
/// The corner a handle is drawn with, and the hairline around it in the
/// ground it stands on, which is what parts it from a band the same color.
const HANDLE_RADIUS: f32 = 1.5;
const HANDLE_RING: f32 = 1.0;
/// The groove: how thick it is, and how tall the mark at the value's
/// origin stands either side of it.
const TRACK: f32 = 2.0;
const TICK: f32 = 4.0;
/// How far the groove is faded toward its ground: the dim ink, held back so
/// that a line two pixels thick reads as a groove under the handle rather
/// than as a rule across the row.
const GROOVE_ALPHA: u8 = 90;
/// How tall the handle stands, the band's handles' width wide.
const HANDLE_HEIGHT: f32 = 12.0;

/// What one slider sets, and how.
pub(super) struct Line<'a> {
    /// What it is called in the accessibility tree, and its id.
    pub name: &'a str,
    pub value: f32,
    /// The ends of the run. A value past either stands hollow at that end,
    /// as a band handle out past the plot does.
    pub low: f32,
    pub high: f32,
    /// What a drag snaps to, counted from `low`.
    pub step: f32,
    /// Where the fill runs from: the value that is no push at all. A mark
    /// stands there where it is inside the run.
    pub origin: f32,
    pub tip: Option<Tip>,
    /// Whether it takes the pointer; drawn in the dim ink when it does not.
    pub live: bool,
    /// What it is drawn on, which the handle's ring is cut in.
    pub ground: Color,
}

/// Draws `line` in `room`, and gives back the value a drag asked for, where
/// it differs from the value it has.
pub(super) fn show(pass: &mut Pass, ui: &mut egui::Ui, line: Line<'_>, room: Rect) -> Option<f32> {
    let theme = pass.theme;
    let grid = pass.grid;
    let track = Rect::new(
        room.x + HANDLE_GRIP / 2.0,
        room.y,
        room.width - HANDLE_GRIP,
        room.height,
    );
    let span = line.high - line.low;
    let along = |value: f32| (value - line.low) / span;
    let at = |t: f32| track.x + t.clamp(0.0, 1.0) * track.width;

    let sense = if line.live { Sense::DRAG } else { Sense::HOVER };
    let response = ui.interact(area(room), ui.id().with(line.name), sense);
    let (live, value, name) = (line.live, line.value, line.name);
    response.widget_info(|| WidgetInfo::slider(live, f64::from(value), name));
    let mut asked = None;
    if line.live
        && response.dragged()
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let t = ((pointer.x - track.x) / track.width).clamp(0.0, 1.0);
        let wanted = line.low + ((t * span) / line.step).round() * line.step;
        let wanted = wanted.clamp(line.low, line.high);
        if wanted != line.value {
            asked = Some(wanted);
        }
    }
    let on = line.live && (response.hovered() || response.dragged());
    if on {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
    if let Some(tip) = line.tip {
        pass.tooltip(response, tip);
    }

    // The groove, the mark at the origin, and the fill from there to the
    // handle, each on the device's grid so that a line two pixels thick is
    // two pixels thick.
    let painter = ui.painter();
    let middle = room.y + room.height / 2.0;
    let groove = grid.rect(Rect::new(track.x, middle - TRACK / 2.0, track.width, TRACK));
    painter.rect_filled(
        area(groove),
        TRACK / 2.0,
        theme.text_dim.with_alpha(GROOVE_ALPHA),
    );
    let t = along(line.value);
    let origin = along(line.origin);
    let (from, to) = (at(origin).min(at(t)), at(origin).max(at(t)));
    if to > from {
        let fill = grid.rect(Rect::new(from, groove.y, to - from, groove.height));
        let ink = if line.live {
            theme.accent
        } else {
            theme.text_dim
        };
        painter.rect_filled(area(fill), TRACK / 2.0, ink);
    }
    if line.low < line.origin && line.origin < line.high {
        let tick = grid.rect(Rect::new(
            at(origin) - HANDLE_RING / 2.0,
            middle - TICK,
            HANDLE_RING,
            2.0 * TICK,
        ));
        painter.rect_filled(area(tick), 0.0, theme.text_dim);
    }

    let mark = grid.rect(Rect::new(
        at(t) - HANDLE_WIDTH / 2.0,
        middle - HANDLE_HEIGHT / 2.0,
        HANDLE_WIDTH,
        HANDLE_HEIGHT,
    ));
    let hand = match (line.live, on) {
        (false, _) => Hand::Dead,
        (true, true) => Hand::On,
        (true, false) => Hand::Off,
    };
    handle(painter, theme, grid, mark, hand, t, line.ground);
    asked
}

/// Whether the hand is on a handle, which lights it, or whether it takes
/// no hand at all, which dims it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Hand {
    Off,
    On,
    Dead,
}

/// A handle on a line — the band's two, and every slider's — drawn as the
/// same kind of thing, a value on a line: `mark`, already on the device's
/// grid, in the accent every mark on the plot wears, or the primary ink
/// while the hand is on it, ringed in `ground` so that it stays a shape
/// against a band that has come round to the same color. Hollow where `t`,
/// its place along the line, is out past either end, so that it can be
/// taken hold of and brought back without claiming a boundary that is not
/// there.
pub(super) fn handle(
    painter: &egui::Painter,
    theme: &Theme,
    grid: icon::Grid,
    mark: Rect,
    hand: Hand,
    t: f32,
    ground: Color,
) {
    let ring = mark.inset(-HANDLE_RING, -HANDLE_RING);
    painter.rect_filled(area(ring), HANDLE_RADIUS + HANDLE_RING, ground);
    let ink: Color32 = match hand {
        Hand::Off => theme.accent,
        Hand::On => theme.text_primary,
        Hand::Dead => theme.text_dim,
    }
    .into();
    if (0.0..=1.0).contains(&t) {
        painter.rect_filled(area(mark), HANDLE_RADIUS, ink);
    } else {
        outline(painter, grid, mark, grid.line_width(1.0), ink);
    }
}

/// An egui rectangle for one of ours.
fn area(rect: Rect) -> egui::Rect {
    egui::Rect::from_min_size(pos2(rect.x, rect.y), vec2(rect.width, rect.height))
}
