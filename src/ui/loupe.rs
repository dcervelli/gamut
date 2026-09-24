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

use super::chrome::Pass;
use super::{Rect, grid, minimap};

/// The radius of the glass, in logical pixels: the same at every
/// magnification, since it is the size of the thing beside the pointer,
/// and what changes with the magnification is how much of the picture fits
/// in it — the eye's radius, which is [`eye_radius`].
pub const GLASS_RADIUS: f32 = 80.0;

/// The magnifications on offer, in the order the wheel steps through them.
pub const MAGNIFICATIONS: [f32; 4] = [2.0, 4.0, 8.0, 16.0];

/// The one the loupe starts at.
pub const DEFAULT_MAGNIFICATION: f32 = 4.0;

/// The radius of the circle around the pointer at `magnification`: what
/// the glass shows at that magnification, so the glass shows exactly what
/// the eye rings and nothing more.
pub fn eye_radius(magnification: f32) -> f32 {
    GLASS_RADIUS / magnification
}

/// The magnification after `current`, `up` being the larger way, and
/// `current` itself at either end: the wheel steps rather than wraps, so
/// a notch too many does not throw the loupe to the other end.
pub fn step(current: f32, up: bool) -> f32 {
    let at = MAGNIFICATIONS
        .iter()
        .position(|&candidate| candidate >= current)
        .unwrap_or(MAGNIFICATIONS.len() - 1);
    let next = if up {
        (at + 1).min(MAGNIFICATIONS.len() - 1)
    } else {
        at.saturating_sub(1)
    };
    MAGNIFICATIONS[next]
}

/// The magnification after `current` round the ones on offer, the largest
/// followed by the smallest: what the key steps, where the wheel stops at
/// either end.
pub fn cycle(current: f32) -> f32 {
    let at = MAGNIFICATIONS
        .iter()
        .position(|&candidate| candidate >= current)
        .unwrap_or(MAGNIFICATIONS.len() - 1);
    MAGNIFICATIONS[(at + 1) % MAGNIFICATIONS.len()]
}

/// The magnification as the button beside the grid's reads it out.
pub fn label(magnification: f32) -> String {
    format!("{magnification}\u{00d7}")
}

/// The gap between the eye's ring and the glass's, so that the two read as
/// two things rather than as one touching itself.
const GAP: f32 = 16.0;

/// The rings' weight, in logical pixels, and their ink: the minimap's
/// border's, both of them — an outline around a picture the image layer
/// has drawn inside the picture, which is what the glass is — and not the
/// accent, which says what is switched on.
const STROKE: f32 = minimap::INSET_STROKE;

/// Where the loupe is, in the logical pixels the interface is laid out in:
/// the center of the eye — the pointer — and the center of the glass; and
/// how much larger the glass shows what the eye rings, which is what the
/// eye's radius follows.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Loupe {
    pub eye: [f32; 2],
    pub glass: [f32; 2],
    pub magnification: f32,
}

/// Where the loupe goes for a pointer at `cursor` over a content area of
/// `content`, at `magnification`: the eye on the pointer, and the glass
/// diagonally away from it — up and to the right, where the hand is least
/// likely to be over what it shows, and the other way on whichever axis
/// that would run it off the content area. The glass is then held inside
/// the area, which in a window too small to hold it beside the eye puts it
/// over the eye rather than off screen: a loupe half under a panel shows
/// half of what it is for.
pub fn place(cursor: [f32; 2], content: Rect, magnification: f32) -> Loupe {
    let reach = (eye_radius(magnification) + GAP + GLASS_RADIUS) / std::f32::consts::SQRT_2;
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
        magnification,
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
/// of it under the eye lands at the glass's center, the loupe's
/// magnification times larger than `placement` shows it, and cut to the
/// glass's circle.
pub fn glass(loupe: Loupe, placement: Placement, scale: f32) -> Glass {
    let zoom = placement.zoom * loupe.magnification;
    let under = placement.image_point([loupe.eye[0] * scale, loupe.eye[1] * scale]);
    let center = [loupe.glass[0] * scale, loupe.glass[1] * scale];
    Glass {
        placement: Placement {
            x: center[0] - under[0] * zoom,
            y: center[1] - under[1] * zoom,
            width: placement.width * loupe.magnification,
            height: placement.height * loupe.magnification,
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
///
/// Not over the minimap: the image layer draws the thumbnail over the glass,
/// and the rings are clipped around the thumbnail's rectangle to match, so
/// the whole loupe goes under the map. Clipped rather than layered, since
/// the thumbnail is the image layer's and nothing egui stacks over the rings
/// covers it.
pub(super) fn show(pass: &Pass, ui: &mut egui::Ui) {
    let Some(loupe) = pass.input.loupe else {
        return;
    };
    let thumbnail = pass
        .current
        .filter(|_| pass.input.minimap_on_screen)
        .and_then(|current| minimap::thumbnail(pass.content, current.size()));
    let grid = pass.grid;
    let ink: egui::Color32 = pass.theme.inset_edge.into();
    let stroke = Stroke::new(grid.line_width(STROKE), ink);
    // egui lays a circle's stroke outside its radius; each ring is drawn
    // half a stroke in so that it straddles its circle, the glass's edge
    // — the pixel the image layer feathers — running down its middle.
    let inset = stroke.width / 2.0;
    for piece in grid::pieces(pass.content, &[thumbnail]) {
        let painter = ui.painter().with_clip_rect(piece.into());
        painter.circle_stroke(
            egui::pos2(loupe.eye[0], loupe.eye[1]),
            eye_radius(loupe.magnification) - inset,
            stroke,
        );
        painter.circle_stroke(
            egui::pos2(loupe.glass[0], loupe.glass[1]),
            GLASS_RADIUS - inset,
            stroke,
        );
    }
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
        eye_radius(DEFAULT_MAGNIFICATION) + GAP + GLASS_RADIUS
    }

    fn place(cursor: [f32; 2], content: Rect) -> Loupe {
        super::place(cursor, content, DEFAULT_MAGNIFICATION)
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
            magnification: 4.0,
        };
        let glass = glass(loupe, placement, scale);
        assert_eq!(glass.center, [600.0, 160.0]);
        assert_eq!(glass.radius, GLASS_RADIUS * scale);
        assert_eq!(glass.placement.zoom, placement.zoom * 4.0);
        assert_eq!(glass.placement.upscale, Upscale::Bicubic);
        assert_eq!(glass.placement.width, placement.width * 4.0);

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

    /// The glass is one size at every magnification, and the eye is the
    /// glass's radius over it: what the glass shows at the magnification.
    /// The wheel steps from one magnification to the next and stops at the
    /// ends; the button reads it out as a multiplier.
    #[test]
    fn the_magnification_sizes_the_eye_and_steps_between_the_offered_ones() {
        assert_eq!(eye_radius(2.0), 40.0);
        assert_eq!(eye_radius(4.0), 20.0);
        assert_eq!(eye_radius(16.0), 5.0);
        for magnification in MAGNIFICATIONS {
            let loupe = super::place([500.0, 350.0], content(), magnification);
            let glass = glass(
                loupe,
                Placement {
                    x: 0.0,
                    y: 0.0,
                    width: 1000.0,
                    height: 700.0,
                    zoom: 1.0,
                    upscale: Upscale::Nearest,
                },
                1.0,
            );
            assert_eq!(glass.radius, GLASS_RADIUS);
            assert_eq!(glass.placement.zoom, magnification);
            // A pixel at the eye's rim lands at the glass's rim.
            let rim = glass
                .placement
                .screen_point([loupe.eye[0] + eye_radius(magnification), loupe.eye[1]]);
            assert!((rim[0] - (glass.center[0] + GLASS_RADIUS)).abs() < 1e-3);
        }

        assert_eq!(step(2.0, true), 4.0);
        assert_eq!(step(4.0, true), 8.0);
        assert_eq!(step(16.0, true), 16.0);
        assert_eq!(step(16.0, false), 8.0);
        assert_eq!(step(2.0, false), 2.0);
        // A magnification between two offered lands on the next up or down.
        assert_eq!(step(3.0, true), 8.0);
        assert_eq!(step(3.0, false), 2.0);

        assert_eq!(cycle(2.0), 4.0);
        assert_eq!(cycle(8.0), 16.0);
        assert_eq!(cycle(16.0), 2.0);
        assert_eq!(cycle(3.0), 8.0);

        assert_eq!(label(4.0), "4\u{00d7}");
        assert_eq!(label(16.0), "16\u{00d7}");
    }
}
