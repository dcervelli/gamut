//! The region on the picture: where its outline and its eight handles go,
//! which of them the pointer is on, and the drawing of all of it — with the
//! region's size written at its middle and its four edges' coordinates
//! written inside the marks on those edges, for as long as the pointer is
//! on it.
//!
//! Painted straight on the picture's painter rather than in an area of its
//! own: an area takes the pointer from what is under it, and the picture's
//! own response is what the drag on a handle is read off. So this is
//! geometry and paint only; the gestures are read in `Pass::picture`, against
//! the same geometry.

use egui::{CursorIcon, Stroke};

use crate::image::region::{Grip, Region, Side};
use crate::render::Placement;

use super::chrome::Pass;
use super::control::Grab;
use super::{Current, PANEL_RADIUS, Rect, icon, outline};

/// The side of a handle, in logical pixels: large enough to take hold of,
/// small enough not to hide what is at the corner it marks.
const HANDLE: f32 = 8.0;

/// How far past a handle's edge the pointer still has hold of it.
const REACH: f32 = 3.0;

/// The outline's weight, in logical pixels — the weight the minimap's marker
/// has, this being the same kind of thing: part of the image, marked out.
const OUTLINE: f32 = 1.5;

/// The room around a label's words, inside the pill they are written on.
const INSET: [f32; 2] = [8.0, 4.0];

/// The least a label keeps between itself and the mark it is written inside
/// of, and between itself and the next label along.
const LABEL_GAP: f32 = 6.0;

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

/// One of the words written on the region: what it says, the pill it is
/// written on, and whether it is the size.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Label {
    pub text: String,
    pub pill: Rect,
    /// The size leads — it is what a drag is judged by — and is written in
    /// the ink the bars keep for what is being read; the coordinates go in
    /// the dimmer one, as the facts beside a file's name do.
    pub size: bool,
}

/// The words a region wears, and where each goes: its size at the middle,
/// and each edge's coordinate inside the mark in the middle of that edge.
///
/// A label is left out rather than written where it will not fit, so that
/// the words never spill over the outline or cover one another on a region
/// drawn small. They are given room in one order — the size first, then the
/// edges — and each is dropped if it does not fit inside `visible` or would
/// land on one already placed. So the size is the last thing to go, and a
/// region too small for all of it says the one thing worth saying.
///
/// `visible` is the part of the region that is on screen, and everything is
/// measured against it: an edge that is off screen has no mark to be
/// written inside of, and words written off screen are words nobody reads.
/// The size is centered on it rather than on the region, so that a region
/// larger than the window still says its size somewhere it can be read.
///
/// The coordinates are the region's boundaries and not the pixels beside
/// them: the right minus the left is the width, which is the number written
/// at the middle, and two readings that did not add up would be worse than
/// either alone.
pub(super) fn labels(
    region: Region,
    rect: Rect,
    visible: Rect,
    grid: icon::Grid,
    mut measure: impl FnMut(&str) -> [f32; 2],
) -> Vec<Label> {
    // Clear of the mark it is written inside of, which is centered on the
    // edge and so reaches half its own width into the region.
    let inset = grid.line_width(HANDLE) / 2.0 + LABEL_GAP;
    let mut placed: Vec<Label> = Vec::new();
    let coordinates = [
        (Side::Left, region.x),
        (Side::Right, region.right()),
        (Side::Top, region.y),
        (Side::Bottom, region.bottom()),
    ];
    let wanted = std::iter::once((dimensions(region), None))
        .chain(coordinates.map(|(side, at)| (at.to_string(), Some(side))));

    for (text, side) in wanted {
        let measured = measure(&text);
        let (width, height) = (measured[0] + 2.0 * INSET[0], measured[1] + 2.0 * INSET[1]);
        let at = match side {
            None => [
                visible.x + (visible.width - width) / 2.0,
                visible.y + (visible.height - height) / 2.0,
            ],
            Some(Side::Left) => [rect.x + inset, rect.y + (rect.height - height) / 2.0],
            Some(Side::Right) => [
                rect.right() - inset - width,
                rect.y + (rect.height - height) / 2.0,
            ],
            Some(Side::Top) => [rect.x + (rect.width - width) / 2.0, rect.y + inset],
            Some(Side::Bottom) => [
                rect.x + (rect.width - width) / 2.0,
                rect.bottom() - inset - height,
            ],
        };
        let pill = Rect::new(grid.snap(at[0]), grid.snap(at[1]), width, height);
        if !within(pill, visible) {
            continue;
        }
        let clear = pill.inset(-LABEL_GAP, -LABEL_GAP);
        if placed
            .iter()
            .any(|other| clear.intersect(other.pill).is_some())
        {
            continue;
        }
        placed.push(Label {
            text,
            pill,
            size: side.is_none(),
        });
    }
    placed
}

/// Whether the whole of `inner` is inside `outer`.
fn within(inner: Rect, outer: Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.right() <= outer.right()
        && inner.bottom() <= outer.bottom()
}

/// Draws the region over the picture: its outline in the accent, its eight
/// handles, and — while the pointer is on it — what [`labels`] gives room
/// to. Clipped to `content`, since a region on a zoomed-in picture runs
/// under the bars like the picture does.
///
/// The words come and go with the pointer rather than with a clock: they
/// are about the region under the hand, and the hand is what says which
/// region is being worked on. A region left on the picture keeps only its
/// outline, which is the thing it is for.
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

    if !pass.input.over_region {
        return;
    }
    let Some(visible) = rect.intersect(content) else {
        return;
    };
    let font = egui::TextStyle::Body.resolve(ui.style());
    let laid = |text: &str| {
        ui.ctx().fonts_mut(|fonts| {
            fonts.layout_no_wrap(text.to_string(), font.clone(), egui::Color32::WHITE)
        })
    };
    for label in labels(region, rect, visible, grid, |text| {
        let size = laid(text).size();
        [size.x, size.y]
    }) {
        painter.rect_filled(label.pill.into(), PANEL_RADIUS, theme.menu_background);
        painter.rect_stroke(
            label.pill.into(),
            PANEL_RADIUS,
            Stroke::new(1.0, theme.border),
            egui::StrokeKind::Inside,
        );
        let ink = match label.size {
            true => theme.text_primary,
            false => theme.text_dim,
        };
        painter.galley(
            egui::pos2(label.pill.x + INSET[0], label.pill.y + INSET[1]),
            laid(&label.text),
            ink.into(),
        );
    }
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

    /// Words wide enough to be worth fitting, and a line tall enough to
    /// stack: a stand-in for the interface's own face, so the layout can be
    /// checked without one.
    fn measured(text: &str) -> [f32; 2] {
        [text.chars().count() as f32 * 8.0, 14.0]
    }

    fn written(labels: &[Label]) -> Vec<&str> {
        labels.iter().map(|label| label.text.as_str()).collect()
    }

    /// A region with room for all of it says its size at its middle and
    /// each edge's coordinate inside the mark on that edge — the left and
    /// right either side of the size, the top and bottom above and below
    /// it.
    #[test]
    fn a_region_with_room_wears_its_size_and_its_four_edges() {
        let region = Region {
            x: 100,
            y: 200,
            width: 300,
            height: 240,
        };
        let rect = Rect::new(50.0, 60.0, 300.0, 240.0);
        let content = Rect::new(0.0, 0.0, 1000.0, 700.0);
        let labels = super::labels(region, rect, rect, icon::Grid::new(1.0), measured);
        assert_eq!(
            written(&labels),
            ["300 \u{00d7} 240", "100", "400", "200", "440"]
        );
        assert!(labels[0].size);
        assert!(labels[1..].iter().all(|label| !label.size));

        // Each inside the region, clear of the others, and each where its
        // own mark is: the coordinates on the middle lines of the edges.
        for label in &labels {
            assert!(within(label.pill, rect), "{label:?}");
            assert!(within(label.pill, content), "{label:?}");
        }
        let middle = |pill: Rect| [pill.x + pill.width / 2.0, pill.y + pill.height / 2.0];
        assert_eq!(middle(labels[0].pill), [200.0, 180.0]);
        assert_eq!(
            middle(labels[1].pill)[1],
            180.0,
            "the left is on its edge's middle"
        );
        assert_eq!(middle(labels[2].pill)[1], 180.0);
        assert_eq!(
            middle(labels[3].pill)[0],
            200.0,
            "the top is on its edge's middle"
        );
        assert_eq!(middle(labels[4].pill)[0], 200.0);
        assert!(labels[1].pill.x < labels[0].pill.x);
        assert!(labels[2].pill.x > labels[0].pill.right());
        assert!(labels[3].pill.bottom() < labels[0].pill.y);
        assert!(labels[4].pill.y > labels[0].pill.bottom());
    }

    /// The size has precedence: a region too short for the words above and
    /// below it keeps its size and drops those, and one too narrow drops
    /// the two beside it. Nothing is ever written over anything else.
    #[test]
    fn the_size_keeps_its_place_and_the_others_give_way() {
        let region = Region {
            x: 10,
            y: 20,
            width: 300,
            height: 300,
        };
        let grid = icon::Grid::new(1.0);

        let short = Rect::new(0.0, 0.0, 300.0, 40.0);
        let labels = super::labels(region, short, short, grid, measured);
        assert_eq!(written(&labels), ["300 \u{00d7} 300", "10", "310"]);

        let narrow = Rect::new(0.0, 0.0, 90.0, 300.0);
        let labels = super::labels(region, narrow, narrow, grid, measured);
        assert_eq!(written(&labels), ["300 \u{00d7} 300", "20", "320"]);

        // Too small for any of it, and nothing is written rather than
        // something spilling over the outline.
        let tiny = Rect::new(0.0, 0.0, 30.0, 20.0);
        assert!(super::labels(region, tiny, tiny, grid, measured).is_empty());

        // Whatever is placed, no two of them touch.
        for width in [40.0, 90.0, 140.0, 300.0, 600.0] {
            for height in [20.0, 45.0, 90.0, 300.0] {
                let rect = Rect::new(0.0, 0.0, width, height);
                let labels = super::labels(region, rect, rect, grid, measured);
                for (index, label) in labels.iter().enumerate() {
                    assert!(within(label.pill, rect), "{width}x{height}: {label:?}");
                    for other in &labels[index + 1..] {
                        assert!(
                            label.pill.intersect(other.pill).is_none(),
                            "{width}x{height}: {label:?} over {other:?}"
                        );
                    }
                }
            }
        }
    }

    /// A region running off the window keeps only the words that are on
    /// screen, and its size goes at the middle of what can be seen of it
    /// rather than at the middle of the region.
    #[test]
    fn a_region_off_the_window_writes_only_what_can_be_read() {
        let region = Region {
            x: 0,
            y: 0,
            width: 400,
            height: 300,
        };
        // The left edge and the top are off the content area.
        let rect = Rect::new(-200.0, -100.0, 600.0, 500.0);
        let content = Rect::new(0.0, 0.0, 500.0, 400.0);
        let visible = rect.intersect(content).expect("part of it is on screen");
        let labels = super::labels(region, rect, visible, icon::Grid::new(1.0), measured);
        assert_eq!(written(&labels), ["400 \u{00d7} 300", "400", "300"]);
        for label in &labels {
            assert!(within(label.pill, visible), "{label:?}");
        }
        // Centered on what is visible, which is not the region's own middle:
        // the region runs from -200 to 400 across and -100 to 400 down, and
        // what is on screen of it is the square from the origin to 400.
        let size = labels[0].pill;
        assert_eq!(size.x + size.width / 2.0, 200.0);
        assert_eq!(size.y + size.height / 2.0, 200.0);
        assert_ne!(size.y + size.height / 2.0, rect.y + rect.height / 2.0);
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
