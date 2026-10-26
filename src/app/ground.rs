//! A georeferenced file arriving beside another in the same system: the
//! view that keeps the ground under the window where it was, so that two
//! rasters of the same place — a map and the photograph of it, a sheet at
//! two resolutions, the tile beside this one — line up as they are flicked
//! between. Pure, told the two placements and the view; tested here.

use crate::image::geo::Georeference;
use crate::image::orient::Turn;
use crate::view::{View, Viewport};

/// A georeferenced picture as it is shown, or is about to be: where its
/// pixels are on the ground, its size as stored, and the turn it is shown
/// under.
#[derive(Clone, Copy)]
pub(super) struct Placed<'a> {
    pub geo: &'a Georeference,
    pub stored: [u32; 2],
    pub turn: Turn,
}

impl Placed<'_> {
    fn stored_size(&self) -> [f64; 2] {
        self.stored.map(f64::from)
    }

    /// The picture's size as shown, in the view's terms.
    fn size(&self) -> [f32; 2] {
        self.turn.size(self.stored.map(|side| side as f32))
    }

    /// The point `at` of the picture as shown, on the ground.
    fn ground(&self, at: [f32; 2]) -> [f64; 2] {
        let at = self
            .turn
            .stored_point(at.map(f64::from), self.stored_size());
        self.geo.ground(at)
    }

    /// Where `ground` falls on the picture as shown.
    fn picture(&self, ground: [f64; 2]) -> Option<[f64; 2]> {
        let at = self.geo.raster(ground)?;
        Some(self.turn.turned_point(at, self.stored_size()))
    }
}

/// The view `to` arrives with, from `view` over `from` in `viewport`: the
/// ground at the middle of the window kept there, and as much ground to a
/// screen pixel as there was. `None` where the two are not in one system,
/// or where none of `to` is under the part of `from` on screen — a file
/// somewhere else is not lined up with anything, and arrives as any other
/// file does.
///
/// The pan is held to its limit as any other is, so a middle of the window
/// off the edge of `to` is brought back to as near as the pan goes, and
/// the ground lines up across the rest of the window only so far.
pub(super) fn aligned(view: View, from: Placed, to: Placed, viewport: Viewport) -> Option<View> {
    if !from.geo.shares_system(to.geo) {
        return None;
    }
    let [width, height] = from.size();
    let placement = view.placement([width, height], viewport);
    // The part of the picture on screen, in its own pixels.
    let first = placement.image_point([viewport.x, viewport.y]);
    let last = placement.image_point([viewport.x + viewport.width, viewport.y + viewport.height]);
    let seen = [
        first[0].max(0.0),
        first[1].max(0.0),
        last[0].min(width),
        last[1].min(height),
    ];
    if seen[0] >= seen[2] || seen[1] >= seen[3] {
        return None;
    }
    // The same ground on the picture arriving, by the box around its
    // corners there: the two rasters' rows need not run the same way.
    let corners = [
        [seen[0], seen[1]],
        [seen[2], seen[1]],
        [seen[2], seen[3]],
        [seen[0], seen[3]],
    ];
    let (mut low, mut high) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for corner in corners {
        let at = to.picture(from.ground(corner))?;
        for axis in 0..2 {
            low[axis] = low[axis].min(at[axis]);
            high[axis] = high[axis].max(at[axis]);
        }
    }
    let size = to.size().map(f64::from);
    if (0..2).any(|axis| high[axis] <= 0.0 || low[axis] >= size[axis]) {
        return None;
    }
    // The middle of the window on the picture arriving, and the ground one
    // screen pixel across and one down from it, which is the zoom.
    let under = |screen: [f32; 2]| to.picture(from.ground(placement.image_point(screen)));
    let middle = viewport.center();
    let center = under(middle)?;
    let across = under([middle[0] + 1.0, middle[1]])?;
    let down = under([middle[0], middle[1] + 1.0])?;
    let (u, v) = (
        [across[0] - center[0], across[1] - center[1]],
        [down[0] - center[0], down[1] - center[1]],
    );
    let area = (u[0] * v[1] - u[1] * v[0]).abs();
    if !area.is_normal() {
        return None;
    }
    let image = to.size();
    let mut aligned = view;
    aligned.set_zoom_at((1.0 / area.sqrt()) as f32, middle, image, viewport);
    aligned.center_on([center[0] as f32, center[1] as f32], image, viewport);
    Some(aligned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::geo::Tags;

    const VIEWPORT: Viewport = Viewport::whole([400.0, 300.0]);

    /// A projected raster under `system`, its first corner at `corner` and
    /// a pixel `pixel` across.
    fn raster(system: u16, corner: [f64; 2], pixel: f64) -> Georeference {
        Georeference::read(&Tags {
            // Projected, under `system`.
            directory: vec![1, 1, 0, 2, 1024, 0, 1, 1, 3072, 0, 1, system],
            scale: vec![pixel, pixel, 0.0],
            tiepoint: vec![0.0, 0.0, 0.0, corner[0], corner[1], 0.0],
            ..Tags::default()
        })
        .expect("placed")
    }

    /// The view over 7 m pixels, at 2x with the pixel `at` in the middle.
    fn looking_at(at: [f32; 2], size: [f32; 2]) -> View {
        let mut view = View::new();
        view.set_zoom_at(2.0, VIEWPORT.center(), size, VIEWPORT);
        view.center_on(at, size, VIEWPORT);
        view
    }

    fn middle(view: View, size: [f32; 2]) -> [f32; 2] {
        view.placement(size, VIEWPORT)
            .image_point(VIEWPORT.center())
    }

    fn near(have: [f32; 2], want: [f32; 2]) {
        assert!(
            (have[0] - want[0]).abs() < 1e-2 && (have[1] - want[1]).abs() < 1e-2,
            "{have:?} is not {want:?}"
        );
    }

    /// The same ground at twice the detail: the same place in the middle,
    /// at the same ground to a screen pixel, which is half the zoom.
    #[test]
    fn a_finer_raster_of_the_same_ground_keeps_the_place_and_its_scale() {
        let coarse = raster(2193, [1_000.0, 9_000.0], 7.0);
        let fine = raster(2193, [1_000.0, 9_000.0], 3.5);
        let from = Placed {
            geo: &coarse,
            stored: [400, 300],
            turn: Turn::NONE,
        };
        let to = Placed {
            geo: &fine,
            stored: [800, 600],
            turn: Turn::NONE,
        };
        let view = looking_at([150.0, 100.0], from.size());

        let aligned = aligned(view, from, to, VIEWPORT).expect("the same ground");
        assert_eq!(aligned.zoom(to.size(), VIEWPORT), 1.0);
        near(middle(aligned, to.size()), [300.0, 200.0]);
    }

    /// The tile beside this one, overlapping it: the ground in the middle of
    /// the window is found on it where it is on that tile.
    #[test]
    fn an_overlapping_tile_lines_up_where_it_lies() {
        let here = raster(2193, [1_000.0, 9_000.0], 7.0);
        // A hundred pixels east and fifty south.
        let beside = raster(2193, [1_700.0, 8_650.0], 7.0);
        let from = Placed {
            geo: &here,
            stored: [400, 300],
            turn: Turn::NONE,
        };
        let to = Placed {
            geo: &beside,
            stored: [400, 300],
            turn: Turn::NONE,
        };
        let view = looking_at([250.0, 150.0], from.size());

        let aligned = aligned(view, from, to, VIEWPORT).expect("overlapping");
        assert_eq!(aligned.zoom(to.size(), VIEWPORT), 2.0);
        near(middle(aligned, to.size()), [150.0, 100.0]);
    }

    /// The ground turned a quarter on screen is found where the turn puts it.
    #[test]
    fn a_turned_raster_lines_up_through_its_turn() {
        let geo = raster(2193, [1_000.0, 9_000.0], 7.0);
        let from = Placed {
            geo: &geo,
            stored: [400, 300],
            turn: Turn::NONE,
        };
        let to = Placed {
            geo: &geo,
            stored: [400, 300],
            turn: Turn::NONE.clockwise(),
        };
        let view = looking_at([150.0, 100.0], from.size());

        let aligned = aligned(view, from, to, VIEWPORT).expect("the same ground");
        // A quarter clockwise puts the stored (x, y) at (height - y, x).
        near(middle(aligned, to.size()), [200.0, 150.0]);
    }

    /// Ground somewhere else, ground the part on screen does not reach, and
    /// ground in another system are none of them lined up with.
    #[test]
    fn a_raster_elsewhere_is_not_lined_up() {
        let here = raster(2193, [1_000.0, 9_000.0], 7.0);
        let far = raster(2193, [100_000.0, 9_000.0], 7.0);
        // Just east of the picture's own edge.
        let next = raster(2193, [1_000.0 + 400.0 * 7.0, 9_000.0], 7.0);
        let other = raster(32759, [1_000.0, 9_000.0], 7.0);
        let from = Placed {
            geo: &here,
            stored: [400, 300],
            turn: Turn::NONE,
        };
        let view = looking_at([100.0, 150.0], from.size());
        for (geo, why) in [
            (&far, "far away"),
            (&next, "off screen"),
            (&other, "another system"),
        ] {
            let to = Placed {
                geo,
                stored: [400, 300],
                turn: Turn::NONE,
            };
            assert!(aligned(view, from, to, VIEWPORT).is_none(), "{why}");
        }
        // Even with the whole picture on screen, a tile that only shares its
        // edge has nothing under it.
        let to = Placed {
            geo: &next,
            stored: [400, 300],
            turn: Turn::NONE,
        };
        let whole = View::new();
        assert!(
            aligned(whole, from, to, VIEWPORT).is_none(),
            "touching is not overlapping"
        );
    }
}
