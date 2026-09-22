//! The strip of buttons down the left of the panel, the toggle beside the
//! band, and the row of false colors under the ramp.

use egui::pos2;

use super::*;

/// The strip of buttons down the left of the panel, the handles on the
/// band, the rows under it, and the row of false colors under its ramp.
/// Hands back what the hand on the band wants written above the plot.
///
/// Drawn here rather than with the chrome's toggles because these belong to
/// the panel: they say what the plot beside them is showing and what the band
/// beneath them is painted with, and two of them are pictures of the very
/// thing they switch. The one beside the band is about the picture rather
/// than the plot — the marks on the clipped pixels — but what it marks is
/// the band's two ends, and this is where those are looked at.
pub(super) fn controls(
    pass: &mut Pass,
    ui: &mut egui::Ui,
    current: &Current,
    panel: Rect,
) -> Option<String> {
    let theme = pass.theme;
    let panels = pass.panels;
    let scale = pass.input.scale;
    let gray = current.image.is_gray();
    let bars = plot_area(panel, gray);

    for (slot, widget) in toolbar(gray).iter().enumerate() {
        let rect = toolbar_button(panel, gray, slot);
        let active = match widget {
            Control::Luma => panels.show_luma,
            Control::Planes => panels.show_planes,
            Control::Log => panels.log_counts,
            // The reset is never lit, where the two above it are: it does
            // something rather than being something, and a momentary button
            // holding a state is a button that has to explain itself.
            _ => false,
        };
        let (_, background, ink) = button(pass, ui, rect, *widget, active, true, TOGGLE_RADIUS);
        let grid = pass.grid;
        let square = icon::square(grid, area(rect), ICON_SIDE);
        let painter = ui.painter();
        match widget {
            // The two plane toggles are drawn here rather than taken from
            // `ui::icon` because they are pictures of the planes themselves,
            // each in the color that plane is plotted in — which is not
            // something a mark drawn in one ink can be.
            //
            // Luminance is one plane, so it is one disc, in the neutral the
            // plot draws that plane in.
            Control::Luma => {
                let place = icon::Placer::new(
                    grid,
                    Rect::new(square.min.x, square.min.y, square.width(), square.height()),
                );
                let at = place.free([GRID_MIDDLE, GRID_MIDDLE]);
                painter.circle_filled(
                    pos2(at[0], at[1]),
                    place.units(LUMA_DISC),
                    theme.histogram_luma,
                );
            }
            // And the color planes are three, so they are three smaller
            // discs, in their own colors: nothing else in the window is red,
            // green and blue together.
            Control::Planes => {
                let place = icon::Placer::new(
                    grid,
                    Rect::new(square.min.x, square.min.y, square.width(), square.height()),
                );
                for (turn, plane) in theme.histogram_planes.into_iter().enumerate() {
                    // Struck about the middle at a third of a turn each,
                    // starting at the top, so the three read as one mark
                    // rather than as a row.
                    let angle = (-90.0 + 120.0 * turn as f32).to_radians();
                    let at = place.free([
                        GRID_MIDDLE + PLANE_ORBIT * angle.cos(),
                        GRID_MIDDLE + PLANE_ORBIT * angle.sin(),
                    ]);
                    painter.circle_filled(pos2(at[0], at[1]), place.units(PLANE_DISC), plane);
                }
            }
            // The count axis as a curve, which is what the switch puts it on.
            Control::Log => icon::paint(painter, icon::SPLINE, square, ink, background),
            // Back to the start.
            _ => icon::paint(painter, icon::ROTATE_CCW, square, ink, background),
        }
    }

    // The marks on the picture, beside the band whose ends they are: lit
    // while they are on, a state to be left in as the plane toggles are.
    // The warning sign, for what the display has thrown away.
    {
        let rect = marks_button(panel);
        let (_, background, ink) = button(
            pass,
            ui,
            rect,
            Control::Marks,
            panels.mark_clipped,
            true,
            TOGGLE_RADIUS,
        );
        let square = icon::square(pass.grid, area(rect), ICON_SIDE);
        icon::paint(ui.painter(), icon::TRIANGLE_ALERT, square, ink, background);
    }

    let held = track(pass, ui, current, bars);
    rows(pass, ui, current, panel);

    // The false colors, each showing itself, and only where the display
    // would act on the choice. The whole ramp rather than one color off it:
    // a map is a sequence, and a single swatch of viridis is a green
    // rectangle that could be anything.
    if !gray {
        return held;
    }
    for (index, map) in Colormap::ALL.into_iter().enumerate() {
        let rect = swatch_button(bars, index);
        let chosen = current.display.colormap() == map;
        button(
            pass,
            ui,
            rect,
            Control::Ramp(index),
            chosen,
            true,
            SWATCH_RADIUS,
        );

        // The gradient on the device's pixels, as the band above it is: a
        // swatch is the same row of one-pixel cells, over less room.
        let face = rect.inset(SWATCH_INSET, SWATCH_INSET);
        let grid = pass.grid;
        let snap = |value: f32| grid.snap(value);
        let (top, bottom) = (snap(face.y), snap(face.bottom()));
        let steps = (face.width * scale).max(1.0) as usize;
        for step in 0..steps {
            let edge = |step: usize| snap(face.x + face.width * step as f32 / steps as f32);
            let (left, right) = (edge(step), edge(step + 1));
            let t = (step as f32 + 0.5) / steps as f32;
            ui.painter().rect_filled(
                egui::Rect::from_min_max(pos2(left, top), pos2(right, bottom)),
                0.0,
                Color::from_linear(map.color(t)),
            );
        }
    }
    held
}
