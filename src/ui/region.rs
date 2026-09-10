//! The region on the picture: where its outline and its eight handles go,
//! which of them the pointer is on, and the drawing of all of it — with the
//! region's size written at its middle for a moment after that size changes.
//!
//! Painted straight on the picture's painter rather than in an area of its
//! own: an area takes the pointer from what is under it, and the picture's
//! own response is what the drag on a handle is read off. So this is
//! geometry and paint only; the gestures are read in `Pass::picture`, against
//! the same geometry.

use std::time::Duration;

use egui::{CursorIcon, Stroke};

use crate::image::region::{Grip, Region, Side};
use crate::render::Placement;

use super::chrome::Pass;
use super::control::Grab;
use super::{Current, PANEL_RADIUS, Rect, icon, outline};

/// How long the region's size stays written at its middle after it changes.
/// Long enough to be read, short enough to be gone before the next thing is
/// done to the region — which writes it again.
pub const LINGER: Duration = Duration::from_secs(1);

/// The side of a handle, in logical pixels: large enough to take hold of,
/// small enough not to hide what is at the corner it marks.
const HANDLE: f32 = 8.0;

/// How far past a handle's edge the pointer still has hold of it.
const REACH: f32 = 3.0;

/// The outline's weight, in logical pixels — the weight the minimap's marker
/// has, this being the same kind of thing: part of the image, marked out.
const OUTLINE: f32 = 1.5;

/// The room around the size where it is written.
const INSET: [f32; 2] = [8.0, 4.0];

/// Where `region` is on screen, in the logical pixels the interface is laid
/// out in, for a picture placed by `placement`.
pub(super) fn rect(region: Region, placement: Placement, scale: f32) -> Rect {
    let [x, y, width, height] = region.as_f32();
    let start = placement.screen_point([x, y]);
    let end = placement.screen_point([x + width, y + height]);
    Rect::new(
        start[0] / scale,
        start[1] / scale,
        (end[0] - start[0]) / scale,
        (end[1] - start[1]) / scale,
    )
}

/// The eight handles of a region drawn at `rect`: a square on each corner
/// and in the middle of each edge, on the device's own grid so that the
/// squares are all the same size and their edges sharp.
pub(super) fn handles(rect: Rect, grid: icon::Grid) -> [(Grip, Rect); 8] {
    let side = grid.line_width(HANDLE);
    let along = |low: f32, high: f32, at: Side| match at {
        Side::Left | Side::Top => low,
        Side::Right | Side::Bottom => high,
    };
    let center = |grip: Grip| match grip {
        Grip::Corner(across, down) => [
            along(rect.x, rect.right(), across),
            along(rect.y, rect.bottom(), down),
        ],
        Grip::Edge(side @ (Side::Left | Side::Right)) => [
            along(rect.x, rect.right(), side),
            rect.y + rect.height / 2.0,
        ],
        Grip::Edge(side) => [
            rect.x + rect.width / 2.0,
            along(rect.y, rect.bottom(), side),
        ],
        Grip::Inside => [rect.x + rect.width / 2.0, rect.y + rect.height / 2.0],
    };
    Grip::HANDLES.map(|grip| {
        let [x, y] = center(grip);
        (
            grip,
            Rect::new(
                grid.snap(x - side / 2.0),
                grid.snap(y - side / 2.0),
                side,
                side,
            ),
        )
    })
}

/// What the pointer at `point` has hold of: the handle under it, or the
/// inside of the region, or nothing. The handles are asked first, and in
/// the order [`Grip::HANDLES`] lists them, so a corner outranks the edge it
/// overlaps on a region drawn small.
pub(super) fn grip_at(rect: Rect, handles: &[(Grip, Rect); 8], point: [f32; 2]) -> Option<Grip> {
    handles
        .iter()
        .find(|(_, handle)| handle.inset(-REACH, -REACH).contains(point))
        .map(|(grip, _)| *grip)
        .or_else(|| rect.contains(point).then_some(Grip::Inside))
}

/// The cursor a hold on the region wears: the crosshair for drawing one,
/// the resize arrows across the axis a handle moves along, the move cursor
/// for the whole of it.
pub(super) fn cursor(grab: Grab) -> CursorIcon {
    match grab {
        Grab::New => CursorIcon::Crosshair,
        Grab::Handle(Grip::Corner(Side::Left, Side::Top))
        | Grab::Handle(Grip::Corner(Side::Right, Side::Bottom)) => CursorIcon::ResizeNwSe,
        Grab::Handle(Grip::Corner(_, _)) => CursorIcon::ResizeNeSw,
        Grab::Handle(Grip::Edge(Side::Left | Side::Right)) => CursorIcon::ResizeHorizontal,
        Grab::Handle(Grip::Edge(Side::Top | Side::Bottom)) => CursorIcon::ResizeVertical,
        Grab::Handle(Grip::Inside) => CursorIcon::Move,
    }
}

/// What the label at the region's middle says.
pub(super) fn dimensions(region: Region) -> String {
    format!("{} \u{00d7} {}", region.width, region.height)
}

/// Draws the region over the picture: its outline in the accent, its eight
/// handles, and — while the size has just changed — the size at its
/// middle. Clipped to `content`, since a region on a zoomed-in picture runs
/// under the bars like the picture does.
///
/// The size is written at the middle of the part of the region that is on
/// screen rather than of the region itself: a region larger than the window
/// has its middle wherever it has it, and words written off screen are
/// words nobody reads.
pub(super) fn show(pass: &Pass, ui: &mut egui::Ui, current: &Current, content: Rect) {
    let Some(region) = pass.input.selection.region() else {
        return;
    };
    let theme = pass.theme;
    let scale = pass.input.scale;
    let grid = icon::Grid::new(ui.pixels_per_point());
    let placement = pass.view.placement(current.size(), pass.input.viewport);
    let rect = rect(region, placement, scale);
    let painter = ui.painter().with_clip_rect(content.into());
    let accent: egui::Color32 = theme.accent.into();

    outline(&painter, grid, rect, OUTLINE, accent);
    let edge: egui::Color32 = theme.bar_background.into();
    for (_, handle) in handles(rect, grid) {
        painter.rect_filled(handle.into(), 0.0, accent);
        painter.rect_stroke(
            handle.into(),
            0.0,
            Stroke::new(grid.line_width(1.0), edge),
            egui::StrokeKind::Inside,
        );
    }

    if !pass.input.dimensions_shown {
        return;
    }
    let Some(visible) = rect.intersect(content) else {
        return;
    };
    let font = egui::TextStyle::Body.resolve(ui.style());
    let galley = ui
        .ctx()
        .fonts_mut(|fonts| fonts.layout_no_wrap(dimensions(region), font, egui::Color32::WHITE));
    let size = galley.size();
    let pill = Rect::new(
        grid.snap(visible.x + (visible.width - size.x) / 2.0 - INSET[0]),
        grid.snap(visible.y + (visible.height - size.y) / 2.0 - INSET[1]),
        size.x + 2.0 * INSET[0],
        size.y + 2.0 * INSET[1],
    );
    painter.rect_filled(pill.into(), PANEL_RADIUS, theme.menu_background);
    painter.rect_stroke(
        pill.into(),
        PANEL_RADIUS,
        Stroke::new(1.0, theme.border),
        egui::StrokeKind::Inside,
    );
    painter.galley(
        egui::pos2(pill.x + INSET[0], pill.y + INSET[1]),
        galley,
        theme.text_primary.into(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::Upscale;

    fn placed(zoom: f32) -> Placement {
        Placement {
            x: 100.0,
            y: 50.0,
            width: 400.0 * zoom,
            height: 300.0 * zoom,
            zoom,
            upscale: Upscale::default(),
        }
    }

    /// The rectangle on screen is the region's pixels through the picture's
    /// own placement, in logical pixels: what is drawn sits exactly on the
    /// pixels that would be copied.
    #[test]
    fn the_region_is_drawn_where_its_pixels_are() {
        let region = Region {
            x: 10,
            y: 20,
            width: 30,
            height: 40,
        };
        let rect = rect(region, placed(2.0), 2.0);
        assert_eq!(rect, Rect::new(60.0, 45.0, 30.0, 40.0));
    }

    /// Eight handles: one on each corner and one in the middle of each edge,
    /// each a square of the same size, and each found by the pointer resting
    /// on it — with a little reach past its edge.
    #[test]
    fn the_handles_sit_on_the_corners_and_the_edges() {
        let rect = Rect::new(100.0, 100.0, 200.0, 100.0);
        let handles = handles(rect, icon::Grid::new(1.0));
        let centered_on = |grip: Grip| {
            let (_, handle) = handles.iter().find(|(g, _)| *g == grip).expect("a handle");
            [
                handle.x + handle.width / 2.0,
                handle.y + handle.height / 2.0,
            ]
        };
        assert_eq!(
            centered_on(Grip::Corner(Side::Left, Side::Top)),
            [100.0, 100.0]
        );
        assert_eq!(
            centered_on(Grip::Corner(Side::Right, Side::Bottom)),
            [300.0, 200.0]
        );
        assert_eq!(centered_on(Grip::Edge(Side::Top)), [200.0, 100.0]);
        assert_eq!(centered_on(Grip::Edge(Side::Left)), [100.0, 150.0]);
        for (_, handle) in &handles {
            assert_eq!(handle.width, HANDLE);
            assert_eq!(handle.height, HANDLE);
        }

        assert_eq!(
            grip_at(rect, &handles, [101.0, 99.0]),
            Some(Grip::Corner(Side::Left, Side::Top))
        );
        assert_eq!(
            grip_at(rect, &handles, [300.0 + HANDLE / 2.0 + REACH - 0.5, 150.0]),
            Some(Grip::Edge(Side::Right))
        );
        assert_eq!(grip_at(rect, &handles, [200.0, 150.0]), Some(Grip::Inside));
        assert_eq!(grip_at(rect, &handles, [50.0, 50.0]), None);
        // Just off the region, past a handle's reach, is nothing.
        assert_eq!(
            grip_at(rect, &handles, [200.0, 100.0 - HANDLE - REACH]),
            None
        );
    }

    /// On a region drawn small the corners overlap the edges, and the corner
    /// wins: it moves two edges where the other moves one.
    #[test]
    fn a_corner_outranks_the_edge_it_overlaps() {
        let rect = Rect::new(100.0, 100.0, 6.0, 6.0);
        let handles = handles(rect, icon::Grid::new(1.0));
        assert_eq!(
            grip_at(rect, &handles, [103.0, 100.0]),
            Some(Grip::Corner(Side::Left, Side::Top))
        );
    }

    #[test]
    fn the_label_is_the_size_in_pixels() {
        let region = Region {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        assert_eq!(dimensions(region), "1920 \u{00d7} 1080");
    }
}
