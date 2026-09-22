//! The exposure's slider under the band.

use egui::{CursorIcon, Sense, WidgetInfo};

use super::*;

/// The exposure's slider: a groove with a mark at nothing, the run from
/// there to the handle filled in the accent so that the push reads as a
/// length and a direction before the number is read, and a handle drawn as
/// the band's are — the same width, the same ring — since it is the same
/// kind of thing, a value on a line.
///
/// Dragged to the pointer rather than by it, as the band's handles are, so
/// that a press anywhere on the row puts the exposure there and a hand run
/// off the end leaves it at the end. Snapped to the quarter stops the keys
/// count in, so that the reading beside it is always one the keys could
/// have reached. Asked for through [`Command::Exposure`], and only when it
/// would change something, so that a hand resting still is not a frame a
/// second.
///
/// The run is [`SLIDER_STOPS`] each way. An exposure past that, from the
/// keys or `--exposure`, stands hollow at the end, as a band handle out past
/// the plot does.
pub(super) fn slider(pass: &mut Pass, ui: &mut egui::Ui, exposure: f32, room: Rect) {
    let theme = pass.theme;
    let scale = pass.input.scale;
    let grid = icon::Grid::new(scale);
    let track = Rect::new(
        room.x + HANDLE_GRIP / 2.0,
        room.y,
        room.width - HANDLE_GRIP,
        room.height,
    );
    let along = |stops: f32| (stops / SLIDER_STOPS + 1.0) / 2.0;
    let at = |t: f32| track.x + t.clamp(0.0, 1.0) * track.width;

    let response = ui.interact(area(room), ui.id().with("exposure"), Sense::DRAG);
    response.widget_info(|| WidgetInfo::slider(true, f64::from(exposure), "Exposure"));
    if response.dragged()
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let t = ((pointer.x - track.x) / track.width).clamp(0.0, 1.0);
        let asked = (((t * 2.0 - 1.0) * SLIDER_STOPS) / EV_STEP).round() * EV_STEP;
        if asked != exposure {
            pass.commands.push(Command::Exposure(asked));
        }
    }
    let on = response.hovered() || response.dragged();
    if on {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
    pass.tooltip(response, Tip::Exposure, true);

    // The groove, the mark at nothing, and the fill from there to the
    // handle, each on the device's grid so that a line two pixels thick is
    // two pixels thick.
    let painter = ui.painter();
    let middle = room.y + room.height / 2.0;
    let groove = grid.rect(Rect::new(
        track.x,
        middle - SLIDER_TRACK / 2.0,
        track.width,
        SLIDER_TRACK,
    ));
    painter.rect_filled(
        area(groove),
        SLIDER_TRACK / 2.0,
        theme.text_dim.with_alpha(SLIDER_GROOVE_ALPHA),
    );
    let t = along(exposure);
    let (from, to) = (at(0.5).min(at(t)), at(0.5).max(at(t)));
    if to > from {
        let fill = grid.rect(Rect::new(from, groove.y, to - from, groove.height));
        painter.rect_filled(area(fill), SLIDER_TRACK / 2.0, theme.accent);
    }
    let tick = grid.rect(Rect::new(
        at(0.5) - HANDLE_RING / 2.0,
        middle - SLIDER_TICK,
        HANDLE_RING,
        2.0 * SLIDER_TICK,
    ));
    painter.rect_filled(area(tick), 0.0, theme.text_dim);

    // The handle, as the band's are drawn.
    let mark = grid.rect(Rect::new(
        at(t) - HANDLE_WIDTH / 2.0,
        middle - SLIDER_HANDLE / 2.0,
        HANDLE_WIDTH,
        SLIDER_HANDLE,
    ));
    handle(painter, theme, grid, mark, on, t);
}
