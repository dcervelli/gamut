//! The loupe: a circle around the pointer, and a larger circle beside it
//! showing what is inside the first magnified.
//!
//! The magnified picture is the image layer's — a third draw of the same
//! texture, placed by [`glass`] and cut to the circle in the shader — so
//! everything that applies to the image applies to it for nothing. Only
//! the two rings are drawn here, and where the circles go is worked out
//! here, by [`place`], for both halves to agree on.

use egui::Stroke;

use crate::render::{Glass, Placement};

use super::Rect;
use super::chrome::Pass;

/// The radius of the circle around the pointer, in logical pixels: what
/// the glass magnifies.
pub const RADIUS: f32 = 20.0;

/// How much larger the glass shows it.
pub const MAGNIFICATION: f32 = 4.0;

/// And so the glass's own radius: the eye's circle at that magnification,
/// so that the glass shows exactly what the eye rings and nothing more.
pub const GLASS_RADIUS: f32 = RADIUS * MAGNIFICATION;

/// The gap between the eye's ring and the glass's, so that the two read as
/// two things rather than as one touching itself.
const GAP: f32 = 16.0;

/// The rings' weights, in logical pixels: the eye's the weight of a
/// hairline, the glass's the weight the region's outline has, this being
/// the same kind of thing — part of the image, marked out.
const EYE_STROKE: f32 = 1.0;
const RIM_STROKE: f32 = 1.5;

/// Where the loupe is, in the logical pixels the interface is laid out in:
/// the center of the eye — the pointer — and the center of the glass.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Loupe {
    pub eye: [f32; 2],
    pub glass: [f32; 2],
}

/// Where the loupe goes for a pointer at `cursor` over a content area of
/// `content`: the eye on the pointer, and the glass diagonally away from
/// it — up and to the right, where the hand is least likely to be over what
/// it shows, and the other way on whichever axis that would run it off the
/// content area. The glass is then held inside the area, which in a window
/// too small to hold it beside the eye puts it over the eye rather than off
/// screen: a loupe half under a panel shows half of what it is for.
pub fn place(cursor: [f32; 2], content: Rect) -> Loupe {
    let reach = (RADIUS + GAP + GLASS_RADIUS) / std::f32::consts::SQRT_2;
    let fits_right = cursor[0] + reach + GLASS_RADIUS <= content.right();
    let fits_up = cursor[1] - reach - GLASS_RADIUS >= content.y;
    let dx = if fits_right { reach } else { -reach };
    let dy = if fits_up { -reach } else { reach };
    Loupe {
        eye: cursor,
        glass: [
            within(
                cursor[0] + dx,
                content.x + GLASS_RADIUS,
                content.right() - GLASS_RADIUS,
            ),
            within(
                cursor[1] + dy,
                content.y + GLASS_RADIUS,
                content.bottom() - GLASS_RADIUS,
            ),
        ],
    }
}

/// `value` held between `low` and `high`, and the middle of the two where
/// there is no room between them at all — a window narrower than the glass
/// — since a clamp with its bounds crossed is a panic rather than an answer.
fn within(value: f32, low: f32, high: f32) -> f32 {
    if low > high {
        (low + high) / 2.0
    } else {
        value.clamp(low, high)
    }
}

/// The glass's draw, in physical pixels: the image placed so that the point
/// of it under the eye lands at the glass's center, [`MAGNIFICATION`] times
/// larger than `placement` shows it, and cut to the glass's circle.
pub fn glass(loupe: Loupe, placement: Placement, scale: f32) -> Glass {
    let zoom = placement.zoom * MAGNIFICATION;
    let under = placement.image_point([loupe.eye[0] * scale, loupe.eye[1] * scale]);
    let center = [loupe.glass[0] * scale, loupe.glass[1] * scale];
    Glass {
        placement: Placement {
            x: center[0] - under[0] * zoom,
            y: center[1] - under[1] * zoom,
            width: placement.width * MAGNIFICATION,
            height: placement.height * MAGNIFICATION,
            zoom,
            upscale: placement.upscale,
        },
        center,
        radius: GLASS_RADIUS * scale,
    }
}

/// Draws the two rings over the picture, and over the glass the image layer
/// has already put down: the eye's around the pointer, and the glass's rim
/// around the magnified picture. Painted straight on the picture's painter,
/// as the region is, so that the picture under the loupe keeps the pointer.
pub(super) fn show(pass: &Pass, ui: &mut egui::Ui) {
    let Some(loupe) = pass.input.loupe else {
        return;
    };
    let painter = ui.painter().with_clip_rect(pass.content.into());
    let grid = pass.grid;
    let accent: egui::Color32 = pass.theme.accent.into();
    painter.circle_stroke(
        egui::pos2(loupe.eye[0], loupe.eye[1]),
        RADIUS,
        Stroke::new(grid.line_width(EYE_STROKE), accent),
    );
    painter.circle_stroke(
        egui::pos2(loupe.glass[0], loupe.glass[1]),
        GLASS_RADIUS,
        Stroke::new(grid.line_width(RIM_STROKE), accent),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::Upscale;

    fn content() -> Rect {
        Rect::new(30.0, 30.0, 940.0, 640.0)
    }

    /// How far apart the two centers are, when nothing is in the way.
    fn reach() -> f32 {
        RADIUS + GAP + GLASS_RADIUS
    }

    fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
    }

    /// The glass keeps its distance from the eye, up and to the right of
    /// it, and goes the other way on an axis where that would run it off
    /// the content area — never off the area itself.
    #[test]
    fn the_glass_sits_beside_the_eye_and_inside_the_content_area() {
        let content = content();
        let middle = place([500.0, 350.0], content);
        assert_eq!(middle.eye, [500.0, 350.0]);
        assert!((distance(middle.eye, middle.glass) - reach()).abs() < 1e-3);
        assert!(middle.glass[0] > middle.eye[0] && middle.glass[1] < middle.eye[1]);

        // Near the right edge it goes left; near the top it goes down;
        // in the corner, both.
        let right = place([content.right() - 40.0, 350.0], content);
        assert!(right.glass[0] < right.eye[0] && right.glass[1] < right.eye[1]);
        let top = place([500.0, content.y + 40.0], content);
        assert!(top.glass[0] > top.eye[0] && top.glass[1] > top.eye[1]);
        let corner = place([content.right() - 40.0, content.y + 40.0], content);
        assert!(corner.glass[0] < corner.eye[0] && corner.glass[1] > corner.eye[1]);
        for placed in [middle, right, top, corner] {
            assert!((distance(placed.eye, placed.glass) - reach()).abs() < 1e-3);
        }

        // Wherever the pointer is, the glass is whole inside the area.
        for x in [content.x, content.x + 1.0, 500.0, content.right() - 1.0] {
            for y in [content.y, 350.0, content.bottom() - 1.0] {
                let placed = place([x, y], content);
                let [gx, gy] = placed.glass;
                assert!(gx - GLASS_RADIUS >= content.x - 1e-3, "{placed:?}");
                assert!(gx + GLASS_RADIUS <= content.right() + 1e-3, "{placed:?}");
                assert!(gy - GLASS_RADIUS >= content.y - 1e-3, "{placed:?}");
                assert!(gy + GLASS_RADIUS <= content.bottom() + 1e-3, "{placed:?}");
            }
        }

        // A content area too small for the glass at all puts it in the
        // middle rather than panicking over a clamp with no room in it.
        let tiny = Rect::new(0.0, 0.0, 100.0, 100.0);
        assert_eq!(place([50.0, 50.0], tiny).glass, [50.0, 50.0]);
    }

    /// The glass shows what the eye rings: the point under the pointer is
    /// at the glass's center, at the magnification, and the circle is the
    /// glass's in physical pixels.
    #[test]
    fn the_glass_magnifies_the_point_under_the_eye() {
        let placement = Placement {
            x: 100.0,
            y: 60.0,
            width: 800.0,
            height: 600.0,
            zoom: 2.0,
            upscale: Upscale::Bicubic,
        };
        let scale = 2.0;
        let loupe = Loupe {
            eye: [200.0, 150.0],
            glass: [300.0, 80.0],
        };
        let glass = glass(loupe, placement, scale);
        assert_eq!(glass.center, [600.0, 160.0]);
        assert_eq!(glass.radius, GLASS_RADIUS * scale);
        assert_eq!(glass.placement.zoom, placement.zoom * MAGNIFICATION);
        assert_eq!(glass.placement.upscale, Upscale::Bicubic);
        assert_eq!(glass.placement.width, placement.width * MAGNIFICATION);

        let under_eye = placement.image_point([400.0, 300.0]);
        let under_glass = glass.placement.image_point(glass.center);
        assert!((under_eye[0] - under_glass[0]).abs() < 1e-3);
        assert!((under_eye[1] - under_glass[1]).abs() < 1e-3);

        // A pixel a device pixel from the eye is four from the center.
        let beside = glass
            .placement
            .image_point([glass.center[0] + 4.0, glass.center[1]]);
        let expected = placement.image_point([401.0, 300.0]);
        assert!((beside[0] - expected[0]).abs() < 1e-3);
    }
}
