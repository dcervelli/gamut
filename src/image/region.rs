//! A rectangle of the image's own pixels: what a selection is, and every way
//! one is changed.
//!
//! In image pixels rather than screen ones because a selection is about the
//! file — the pixels copied out of it are the same pixels at any zoom — and
//! held as whole pixels because a file has no others. The edges are
//! half-open boundaries between pixels, so a region one pixel wide at `x`
//! holds column `x` and nothing else, and the region the whole image is has
//! the image's own width.
//!
//! Nothing here knows about the screen. A drag arrives as fractional image
//! coordinates already, mapped through the picture's placement by the
//! interface, and what happens between a press and a release is the
//! application's to hold; these are the pure changes to a rectangle that
//! either one asks for, each clamped to the image and never below one pixel.

/// A rectangle of image pixels: `width` columns from `x`, `height` rows from
/// `y`, both at least one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// One edge of a region, named from the side it lies on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    /// Which axis the edge moves along: 0 across, 1 down.
    fn axis(self) -> usize {
        match self {
            Side::Left | Side::Right => 0,
            Side::Top | Side::Bottom => 1,
        }
    }
}

/// Where a region is taken hold of: one of its eight handles, or anywhere
/// inside it.
///
/// A corner is named by the two edges that meet there, across first, so
/// that pulling one moves both edges and pulling an edge moves one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grip {
    Corner(Side, Side),
    Edge(Side),
    Inside,
}

impl Grip {
    /// The eight handles, clockwise from the top left. The corners come
    /// before the edges beside them so that, where a small region's handles
    /// overlap, the one that moves two edges is the one found.
    pub const HANDLES: [Grip; 8] = [
        Grip::Corner(Side::Left, Side::Top),
        Grip::Corner(Side::Right, Side::Top),
        Grip::Corner(Side::Right, Side::Bottom),
        Grip::Corner(Side::Left, Side::Bottom),
        Grip::Edge(Side::Top),
        Grip::Edge(Side::Right),
        Grip::Edge(Side::Bottom),
        Grip::Edge(Side::Left),
    ];

    /// The edge this grip moves on each axis, across then down: both for a
    /// corner, one for an edge, and neither for the inside, which moves the
    /// whole region rather than any edge of it.
    fn sides(self) -> [Option<Side>; 2] {
        match self {
            Grip::Corner(across, down) => [Some(across), Some(down)],
            Grip::Edge(side) => {
                let mut sides = [None, None];
                sides[side.axis()] = Some(side);
                sides
            }
            Grip::Inside => [None, None],
        }
    }
}

impl Region {
    /// The whole of an image `image` pixels across and down.
    pub fn whole(image: [u32; 2]) -> Self {
        Self {
            x: 0,
            y: 0,
            width: image[0].max(1),
            height: image[1].max(1),
        }
    }

    /// The pixels a drag from `a` to `b` encloses, in image coordinates that
    /// may be fractional and may lie off the image: every pixel either point
    /// touches is taken in, so a drag of any length holds at least the pixel
    /// it began on. `None` when the two points enclose nothing of the image
    /// — both off the same edge of it.
    pub fn from_corners(a: [f32; 2], b: [f32; 2], image: [u32; 2]) -> Option<Self> {
        let span = |axis: usize| {
            let (low, high) = (a[axis].min(b[axis]), a[axis].max(b[axis]));
            if !low.is_finite() || !high.is_finite() {
                return None;
            }
            let extent = image[axis] as f32;
            let low = low.floor().clamp(0.0, extent) as u32;
            let high = high.ceil().clamp(0.0, extent) as u32;
            (high > low).then_some((low, high))
        };
        let (left, right) = span(0)?;
        let (top, bottom) = span(1)?;
        Some(Self::between(left, top, right, bottom))
    }

    fn between(left: u32, top: u32, right: u32, bottom: u32) -> Self {
        Self {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        }
    }

    pub fn right(&self) -> u32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> u32 {
        self.y + self.height
    }

    /// The region as the four numbers geometry works in.
    pub fn as_f32(&self) -> [f32; 4] {
        [
            self.x as f32,
            self.y as f32,
            self.width as f32,
            self.height as f32,
        ]
    }

    /// The edges as boundaries: left, top, right, bottom.
    fn edges(&self) -> [i64; 4] {
        [
            i64::from(self.x),
            i64::from(self.y),
            i64::from(self.right()),
            i64::from(self.bottom()),
        ]
    }

    /// The region moved by `dx` columns and `dy` rows, stopping at the edge
    /// of the image rather than leaving it: a selection is of pixels the
    /// image has.
    pub fn moved_by(self, dx: i64, dy: i64, image: [u32; 2]) -> Self {
        let place = |at: u32, extent: u32, by: i64, limit: u32| {
            let room = i64::from(limit.saturating_sub(extent));
            (i64::from(at) + by).clamp(0, room) as u32
        };
        Self {
            x: place(self.x, self.width, dx, image[0]),
            y: place(self.y, self.height, dy, image[1]),
            ..self
        }
    }

    /// The region with the edges `grip` holds pulled to `to`, a point in
    /// image coordinates that may be fractional and may lie off the image.
    /// An edge pulled past the one opposite flips the region over rather
    /// than collapsing it: what is being drawn is the rectangle between the
    /// anchored edge and the hand, whichever side of it the hand has gone.
    ///
    /// The inside is not pulled — it is moved, by [`Region::moved_by`] —
    /// and asking leaves the region as it is.
    pub fn pulled(self, grip: Grip, to: [f32; 2], image: [u32; 2]) -> Self {
        let mut edges = self.edges();
        for (axis, side) in grip.sides().into_iter().enumerate() {
            let Some(side) = side else {
                continue;
            };
            if !to[axis].is_finite() {
                continue;
            }
            let at = to[axis].round().clamp(0.0, image[axis] as f32) as i64;
            let (low, high) = (axis, axis + 2);
            match side {
                Side::Left | Side::Top => edges[low] = at,
                Side::Right | Side::Bottom => edges[high] = at,
            }
            let (a, b) = (edges[low].min(edges[high]), edges[low].max(edges[high]));
            let (a, b) = if a == b {
                // The hand is exactly on the anchored edge, and a region
                // has to hold a pixel: it takes the one on the near side of
                // that edge, or the far side where the image ends there.
                if b < i64::from(image[axis]) {
                    (a, b + 1)
                } else {
                    (a - 1, b)
                }
            } else {
                (a, b)
            };
            edges[low] = a;
            edges[high] = b;
        }
        Self::between(
            edges[0] as u32,
            edges[1] as u32,
            edges[2] as u32,
            edges[3] as u32,
        )
    }

    /// The region with the edge on `side` moved `by` pixels outward, as far
    /// as the image goes that way.
    pub fn grown(self, side: Side, by: u32, image: [u32; 2]) -> Self {
        let mut edges = self.edges();
        let axis = side.axis();
        let extent = i64::from(image[axis]);
        match side {
            Side::Left | Side::Top => edges[axis] = (edges[axis] - i64::from(by)).max(0),
            Side::Right | Side::Bottom => {
                edges[axis + 2] = (edges[axis + 2] + i64::from(by)).min(extent);
            }
        }
        Self::between(
            edges[0] as u32,
            edges[1] as u32,
            edges[2] as u32,
            edges[3] as u32,
        )
    }

    /// The region with the edges `grip` holds moved one pixel along
    /// `direction`, a sign on each axis: an arrow key pressed with the
    /// pointer resting on a handle. An edge never crosses the one opposite —
    /// a press that would leaves a pixel — and `None` where the grip holds
    /// no edge on the axis the arrow points along, a left edge having no up
    /// to move in.
    pub fn nudged(self, grip: Grip, direction: [i64; 2], image: [u32; 2]) -> Option<Self> {
        let mut edges = self.edges();
        let mut moved = false;
        for (axis, side) in grip.sides().into_iter().enumerate() {
            let step = direction[axis].signum();
            let Some(side) = side.filter(|_| step != 0) else {
                continue;
            };
            let extent = i64::from(image[axis]);
            let (low, high) = (axis, axis + 2);
            match side {
                Side::Left | Side::Top => {
                    edges[low] = (edges[low] + step).clamp(0, edges[high] - 1);
                }
                Side::Right | Side::Bottom => {
                    edges[high] = (edges[high] + step).clamp(edges[low] + 1, extent);
                }
            }
            moved = true;
        }
        moved.then(|| {
            Self::between(
                edges[0] as u32,
                edges[1] as u32,
                edges[2] as u32,
                edges[3] as u32,
            )
        })
    }

    /// Whether `point`, in image coordinates, is on one of the region's own
    /// pixels: what the tests that drive the interface ask, to know that a
    /// press landed inside.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "the tests place their presses by it")
    )]
    pub fn contains(&self, point: [f32; 2]) -> bool {
        (self.x as f32..self.right() as f32).contains(&point[0])
            && (self.y as f32..self.bottom() as f32).contains(&point[1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMAGE: [u32; 2] = [100, 60];

    fn region(x: u32, y: u32, width: u32, height: u32) -> Region {
        Region {
            x,
            y,
            width,
            height,
        }
    }

    /// A drag encloses every pixel it touches, whichever way it went, and is
    /// held to the image: a corner dragged off the edge stops at the edge.
    #[test]
    fn a_drag_takes_in_every_pixel_it_touches() {
        assert_eq!(
            Region::from_corners([10.2, 5.7], [20.1, 15.0], IMAGE),
            Some(region(10, 5, 11, 10))
        );
        // The same rectangle drawn from the other corner.
        assert_eq!(
            Region::from_corners([20.1, 15.0], [10.2, 5.7], IMAGE),
            Some(region(10, 5, 11, 10))
        );
        // Off the image on two sides: clamped to it.
        assert_eq!(
            Region::from_corners([-30.0, -4.0], [40.5, 70.0], IMAGE),
            Some(region(0, 0, 41, 60))
        );
        // A drag that never left one pixel still holds that pixel.
        assert_eq!(
            Region::from_corners([3.2, 3.2], [3.4, 3.9], IMAGE),
            Some(region(3, 3, 1, 1))
        );
        // Both points off the same edge enclose nothing.
        assert_eq!(Region::from_corners([-5.0, 3.0], [-1.0, 8.0], IMAGE), None);
        assert_eq!(
            Region::from_corners([f32::NAN, 3.0], [5.0, 8.0], IMAGE),
            None
        );
    }

    /// A move stops at the edge of the image, and the region keeps its size
    /// doing so.
    #[test]
    fn a_move_stops_at_the_edge() {
        let start = region(10, 10, 20, 10);
        assert_eq!(start.moved_by(5, -3, IMAGE), region(15, 7, 20, 10));
        assert_eq!(start.moved_by(1000, 1000, IMAGE), region(80, 50, 20, 10));
        assert_eq!(start.moved_by(-1000, -1000, IMAGE), region(0, 0, 20, 10));
        // A region as large as the image has nowhere to go.
        assert_eq!(
            Region::whole(IMAGE).moved_by(3, 3, IMAGE),
            Region::whole(IMAGE)
        );
    }

    /// A handle pulled moves the edges it holds and nothing else; pulled past
    /// the edge opposite it flips the region over instead of collapsing it.
    #[test]
    fn a_handle_pulls_its_own_edges_and_flips_past_the_far_one() {
        let start = region(10, 10, 20, 10);

        let right = start.pulled(Grip::Edge(Side::Right), [45.4, 99.0], IMAGE);
        assert_eq!(
            right,
            region(10, 10, 35, 10),
            "an edge moves on its own axis"
        );

        let corner = start.pulled(Grip::Corner(Side::Left, Side::Top), [5.0, 2.0], IMAGE);
        assert_eq!(corner, region(5, 2, 25, 18));

        // Past the far edge: the rectangle between the anchored edge and the
        // hand, on the other side of it.
        let flipped = start.pulled(Grip::Edge(Side::Left), [36.0, 0.0], IMAGE);
        assert_eq!(flipped, region(30, 10, 6, 10));

        // Exactly on the far edge, a pixel is kept on the near side of it.
        let touching = start.pulled(Grip::Edge(Side::Right), [10.0, 0.0], IMAGE);
        assert_eq!(touching, region(10, 10, 1, 10));
        // And on the far side where the image ends at the anchored edge.
        let at_end = region(90, 0, 10, 5).pulled(Grip::Edge(Side::Left), [100.0, 0.0], IMAGE);
        assert_eq!(at_end, region(99, 0, 1, 5));

        // Off the image: held to it.
        let off = start.pulled(
            Grip::Corner(Side::Right, Side::Bottom),
            [500.0, 500.0],
            IMAGE,
        );
        assert_eq!(off, region(10, 10, 90, 50));

        // The inside is not an edge to pull.
        assert_eq!(start.pulled(Grip::Inside, [0.0, 0.0], IMAGE), start);
        // Nor is a NaN a place to pull to.
        assert_eq!(
            start.pulled(Grip::Edge(Side::Right), [f32::NAN, 0.0], IMAGE),
            start
        );
    }

    /// Growing moves one edge outward and stops at the image.
    #[test]
    fn growing_pushes_one_edge_out_as_far_as_the_image_goes() {
        let start = region(10, 10, 20, 10);
        assert_eq!(start.grown(Side::Right, 1, IMAGE), region(10, 10, 21, 10));
        assert_eq!(start.grown(Side::Left, 1, IMAGE), region(9, 10, 21, 10));
        assert_eq!(start.grown(Side::Top, 3, IMAGE), region(10, 7, 20, 13));
        assert_eq!(
            start.grown(Side::Bottom, 100, IMAGE),
            region(10, 10, 20, 50)
        );
        assert_eq!(start.grown(Side::Left, 100, IMAGE), region(0, 10, 30, 10));
    }

    /// A nudge moves a handle's edges a pixel the way the arrow points, never
    /// through the edge opposite, and not at all along an edge.
    #[test]
    fn a_nudge_moves_a_handle_one_pixel() {
        let start = region(10, 10, 20, 10);
        let right = Grip::Edge(Side::Right);
        assert_eq!(
            start.nudged(right, [1, 0], IMAGE),
            Some(region(10, 10, 21, 10))
        );
        assert_eq!(
            start.nudged(right, [-1, 0], IMAGE),
            Some(region(10, 10, 19, 10))
        );
        // Up has no meaning for the right edge.
        assert_eq!(start.nudged(right, [0, -1], IMAGE), None);
        // A corner answers to both axes, one at a time.
        let corner = Grip::Corner(Side::Left, Side::Top);
        assert_eq!(
            start.nudged(corner, [0, 1], IMAGE),
            Some(region(10, 11, 20, 9))
        );
        // An edge stops a pixel short of the one opposite.
        let thin = region(10, 10, 1, 10);
        assert_eq!(thin.nudged(right, [-1, 0], IMAGE), Some(thin));
        assert_eq!(
            thin.nudged(Grip::Edge(Side::Left), [1, 0], IMAGE),
            Some(thin)
        );
        // And at the edge of the image.
        let flush = region(80, 0, 20, 10);
        assert_eq!(flush.nudged(right, [1, 0], IMAGE), Some(flush));
        // The inside holds no edge.
        assert_eq!(start.nudged(Grip::Inside, [1, 0], IMAGE), None);
    }

    #[test]
    fn a_region_knows_its_own_pixels() {
        let region = region(10, 10, 20, 10);
        assert!(region.contains([10.0, 10.0]));
        assert!(region.contains([29.9, 19.9]));
        assert!(!region.contains([30.0, 15.0]));
        assert!(!region.contains([9.9, 15.0]));
        assert_eq!(
            Region::whole(IMAGE),
            Region {
                x: 0,
                y: 0,
                width: 100,
                height: 60
            }
        );
    }
}
