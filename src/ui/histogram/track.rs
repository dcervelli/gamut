//! The band under the plot as the levels track it is: a handle at the value
//! that comes out black and another at the value that comes out white, and
//! the stretch of band between them, which slides the window along the
//! axis.

use egui::{CursorIcon, Sense, WidgetInfo, WidgetType};

use super::*;

/// The band under the plot as the levels track it is: a handle at the value
/// that comes out black and another at the value that comes out white, each
/// dragged to where it should stand, and the stretch of band between them,
/// which drags the window along the axis without changing its width. What
/// every levels tool in every editor looks like, so that no one has to be
/// told what the two handles do.
///
/// Hands back what to write above the plot while the hand is on it: the
/// value under the handle, or the two the band runs between. Not a tooltip,
/// which the toolkit takes down for the length of a drag, and a drag is
/// exactly when the number is wanted.
///
/// The handles stand at the values that come out black and white — exposure
/// included, since those are the two ends of the band's black run and its
/// white run — and each puts its own value where it is dragged to, the
/// exposure left as it is: [`Display::put_black`] and
/// [`Display::put_white`]. A handle is dragged to the pointer rather
/// than by it, so a drag has no memory to lose: wherever the pointer is
/// along the axis is where the handle goes, and a hand that runs off the
/// end of the band puts the handle at the end.
///
/// The window can end past what is plotted, which a few stops of exposure
/// is enough to do; such a handle is drawn hollow at the edge it went out
/// of, so that it can be taken hold of and brought back, and so that it
/// does not claim a boundary the curve running on past it says is not
/// there.
pub(super) fn track(
    pass: &mut Pass,
    ui: &mut egui::Ui,
    current: &Current,
    bars: Rect,
) -> Option<String> {
    let theme = pass.theme;
    let band = ramp(bars);
    let plotted = &current.stats.plot;
    let (axis_min, axis_max) = (plotted.min, plotted.max);
    let span = axis_max - axis_min;
    if span <= 0.0 {
        return None;
    }
    let transfer = current.image.color.transfer;
    let display = &current.display;
    let (black, white) = display.displayed_bounds();
    if !(black.is_finite() && white.is_finite()) {
        return None;
    }

    // Along the band from 0 at its left end to 1 at its right, which is the
    // plot's own axis: encoded, so that a handle stands under the bin its
    // value was counted in.
    let along = |value: f32| (transfer.to_encoded(value) - axis_min) / span;
    let at = |t: f32| bars.x + t.clamp(0.0, 1.0) * bars.width;
    let value_at = |x: f32| {
        let t = ((x - bars.x) / bars.width).clamp(0.0, 1.0);
        transfer.to_linear(axis_min + t * span)
    };
    // The least a window can be: a bin, so that the two handles cannot be
    // dragged through each other into a window the shader would divide by.
    let least = span / BINS as f32;
    let (black_t, white_t) = (along(black), along(white));

    let id = ui.id().with("levels");
    // The stretch between the handles first and the handles after, so that
    // where they overlap it is the handle that is under the pointer.
    let between = ui.interact(area(band), id.with("window"), Sense::DRAG);
    let black_handle = ui.interact(area(grip(band, at(black_t))), id.with("black"), Sense::DRAG);
    let white_handle = ui.interact(area(grip(band, at(white_t))), id.with("white"), Sense::DRAG);
    between.widget_info(|| WidgetInfo::labeled(WidgetType::Other, true, "Window"));
    black_handle.widget_info(|| WidgetInfo::labeled(WidgetType::Other, true, "Black point"));
    white_handle.widget_info(|| WidgetInfo::labeled(WidgetType::Other, true, "White point"));

    let mut said = None;
    if black_handle.dragged()
        && let Some(pointer) = black_handle.interact_pointer_pos()
    {
        let ceiling = transfer.to_linear(transfer.to_encoded(white) - least);
        pass.commands
            .push(Command::BlackPoint(value_at(pointer.x).min(ceiling)));
    } else if white_handle.dragged()
        && let Some(pointer) = white_handle.interact_pointer_pos()
    {
        let floor = transfer.to_linear(transfer.to_encoded(black) + least);
        pass.commands
            .push(Command::WhitePoint(value_at(pointer.x).max(floor)));
    } else if between.dragged() {
        // Along the axis by what the hand moved, in the axis's own units,
        // both ends together: the width of the window is the handles'
        // business, and the band's is where it is. Only as far as the plot
        // goes, as the handles only go as far as the band: a window slid
        // off what is plotted makes nothing black or nothing white, which
        // is a lift and not a place to look. A window already wider than
        // the plot has nowhere to slide to.
        let room = (
            axis_min - transfer.to_encoded(black),
            axis_max - transfer.to_encoded(white),
        );
        let moved = if room.0 <= room.1 {
            (between.drag_delta().x / bars.width * span).clamp(room.0, room.1)
        } else {
            0.0
        };
        // Short of the rounding an end that is already at the plot's edge
        // comes back from the encoding with.
        if moved.abs() > span * 1e-5 {
            let slid = |value: f32| transfer.to_linear(transfer.to_encoded(value) + moved);
            pass.commands.push(Command::Slide {
                black: slid(black),
                white: slid(white),
            });
        }
    }
    let on_black = black_handle.hovered() || black_handle.dragged();
    let on_white = white_handle.hovered() || white_handle.dragged();
    let on_band = between.hovered() || between.dragged();
    if on_black {
        said = Some(format!(
            "Black at {}",
            axis_words(current, transfer.to_encoded(black))
        ));
    } else if on_white {
        said = Some(format!(
            "White at {}",
            axis_words(current, transfer.to_encoded(white))
        ));
    } else if on_band {
        said = Some(format!(
            "{} \u{2013} {}",
            axis_words(current, transfer.to_encoded(black)),
            axis_words(current, transfer.to_encoded(white))
        ));
    }
    if on_black || on_white {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    } else if between.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    } else if on_band {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
    }
    pass.tooltip(between, Tip::Window, true);
    pass.tooltip(black_handle, Tip::BlackPoint, true);
    pass.tooltip(white_handle, Tip::WhitePoint, true);

    // The handles themselves, over the band and standing up past it.
    let painter = ui.painter();
    let grid = pass.grid;
    for (t, on) in [(black_t, on_black), (white_t, on_white)] {
        let mark = grid.rect(Rect::new(
            at(t) - HANDLE_WIDTH / 2.0,
            band.y - HANDLE_REACH,
            HANDLE_WIDTH,
            band.height + 2.0 * HANDLE_REACH,
        ));
        let hand = if on { Hand::On } else { Hand::Off };
        handle(painter, theme, grid, mark, hand, t, theme.panel_background);
    }
    said
}
