//! The thumbnail standing in for a file whose read is taking its time.
//!
//! Stepping to a file leaves the picture before it on screen until the new
//! one has decoded, which for most files is a moment nobody notices. For a
//! slow one — a large raw, an EXR off a network share — the wait is long
//! enough to be said, and where the file's thumbnail and its size are both
//! known by then, the thumbnail goes up in the picture's place: enlarged to
//! exactly where the picture will land, so that when it does arrive nothing
//! moves, it only sharpens.
//!
//! Drawn by the interface rather than the image layer, in the sRGB the
//! thumbnail was made in: it is a stand-in, not the picture, and the window,
//! tone map and false color the picture will be shown under are not applied
//! to it.

use crate::image::orient::Turn;
use crate::render::Placement;

use super::Thumb;

/// What stands in for the picture while the file is read.
#[derive(Clone, Copy, Debug)]
pub struct Standin {
    /// The file's thumbnail, as the file list holds it.
    pub thumb: Thumb,
    /// Where the picture will land, in physical pixels.
    pub placement: Placement,
    /// How far the picture will be turned on screen: the thumbnail is of
    /// the picture as the file holds it, and is read through the turn as
    /// the texture is.
    pub turn: Turn,
}

/// Paints `standin` with `painter`, which clips it to the picture's area.
pub fn paint(painter: &egui::Painter, standin: &Standin, scale: f32) {
    let placement = standin.placement;
    let rect = egui::Rect::from_min_size(
        egui::pos2(placement.x / scale, placement.y / scale),
        egui::vec2(placement.width / scale, placement.height / scale),
    );
    let texture = standin
        .thumb
        .for_side(placement.width.max(placement.height));
    let mut mesh = egui::Mesh::with_texture(texture.id);
    for (corner, uv) in corners(rect).into_iter().zip(uvs(standin.turn)) {
        mesh.vertices.push(egui::epaint::Vertex {
            pos: corner,
            uv,
            color: egui::Color32::WHITE,
        });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(mesh);
}

/// The corners of `rect`, clockwise from the top left.
fn corners(rect: egui::Rect) -> [egui::Pos2; 4] {
    [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ]
}

/// The texture coordinate at each of [`corners`] under `turn`: a quarter
/// clockwise brings the thumbnail's bottom left corner to the top left of
/// the screen, and each corner after it follows round.
fn uvs(turn: Turn) -> [egui::Pos2; 4] {
    let stored = corners(egui::Rect::from_min_max(
        egui::Pos2::ZERO,
        egui::pos2(1.0, 1.0),
    ));
    let quarters = turn.quarters() as usize;
    std::array::from_fn(|corner| stored[(corner + 4 - quarters) % 4])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A quarter clockwise puts the thumbnail's left edge along the top of
    /// the screen, its top down the right, and so on round; a half turn
    /// puts each corner opposite itself.
    #[test]
    fn the_thumbnail_is_read_through_the_turn() {
        let at = |x: f32, y: f32| egui::pos2(x, y);
        assert_eq!(
            uvs(Turn::NONE),
            [at(0.0, 0.0), at(1.0, 0.0), at(1.0, 1.0), at(0.0, 1.0)]
        );
        assert_eq!(
            uvs(Turn::NONE.clockwise()),
            [at(0.0, 1.0), at(0.0, 0.0), at(1.0, 0.0), at(1.0, 1.0)]
        );
        assert_eq!(
            uvs(Turn::NONE.clockwise().clockwise()),
            [at(1.0, 1.0), at(0.0, 1.0), at(0.0, 0.0), at(1.0, 0.0)]
        );
        assert_eq!(
            uvs(Turn::NONE.counterclockwise()),
            [at(1.0, 0.0), at(1.0, 1.0), at(0.0, 1.0), at(0.0, 0.0)]
        );
    }
}
