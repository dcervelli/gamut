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
//!
//! A change can be held to a ratio of width to height — what the aspect in
//! force makes of the picture, see [`super::aspect`] — and then the edge or
//! corner taken hold of governs and the other side follows it: an edge
//! pulled keeps the region's middle across it, a corner pulled keeps the
//! corner opposite. Held or not, a change stays inside the image, and one
//! held stays held there, giving up length on the side it governs rather
//! than the ratio.

use super::orient::{self, Orientation, Turn};

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

    /// Which way is out of the region across this edge, along its axis.
    fn outward(self) -> i64 {
        match self {
            Side::Left | Side::Top => -1,
            Side::Right | Side::Bottom => 1,
        }
    }

    /// The edge across from this one.
    pub fn opposite(self) -> Self {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
            Side::Top => Side::Bottom,
            Side::Bottom => Side::Top,
        }
    }
}

/// Where a region is taken hold of: one of its nine handles, or anywhere
/// inside it.
///
/// A corner is named by the two edges that meet there, across first, so
/// that pulling one moves both edges and pulling an edge moves one. The
/// handle at the middle and the inside both move the whole of it; they are
/// told apart because the inside is only taken hold of with a key held,
/// where the handle is a handle like the others.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grip {
    Corner(Side, Side),
    Edge(Side),
    Middle,
    Inside,
}

impl Grip {
    /// The nine handles, clockwise from the top left and the middle last.
    /// The corners come before the edges beside them so that, where a small
    /// region's handles overlap, the one that moves two edges is the one
    /// found, and the middle comes after both since the whole region can be
    /// taken hold of from anywhere inside it as well.
    pub const HANDLES: [Grip; 9] = [
        Grip::Corner(Side::Left, Side::Top),
        Grip::Corner(Side::Right, Side::Top),
        Grip::Corner(Side::Right, Side::Bottom),
        Grip::Corner(Side::Left, Side::Bottom),
        Grip::Edge(Side::Top),
        Grip::Edge(Side::Right),
        Grip::Edge(Side::Bottom),
        Grip::Edge(Side::Left),
        Grip::Middle,
    ];

    /// The edge this grip moves on each axis, across then down: both for a
    /// corner, one for an edge, and neither for the middle or the inside,
    /// which move the whole region rather than any edge of it.
    fn sides(self) -> [Option<Side>; 2] {
        match self {
            Grip::Corner(across, down) => [Some(across), Some(down)],
            Grip::Edge(side) => {
                let mut sides = [None, None];
                sides[side.axis()] = Some(side);
                sides
            }
            Grip::Middle | Grip::Inside => [None, None],
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
    /// The middle and the inside are not pulled — they are moved, by
    /// [`Region::moved_by`] — and asking leaves the region as it is.
    ///
    /// Held to `ratio`, a corner reaches from the corner opposite toward
    /// the hand as far as it goes along whichever side it has gone further
    /// along in the ratio's terms, and an edge moves to the hand with the
    /// sides beside it following about the middle.
    pub fn pulled(self, grip: Grip, to: [f32; 2], ratio: Option<f64>, image: [u32; 2]) -> Self {
        if let Some(ratio) = ratio {
            return self.pulled_held(grip, to, ratio, image);
        }
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
    /// as the image goes that way — held to `ratio`, with the sides beside
    /// it following about the middle.
    pub fn grown(self, side: Side, by: u32, ratio: Option<f64>, image: [u32; 2]) -> Self {
        if let Some(ratio) = ratio {
            let at = self.edge(side) + side.outward() * i64::from(by);
            return self.edge_held(side, at, ratio, image);
        }
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

    /// The region with the edge on `side` moved `by` pixels inward,
    /// stopping a pixel short of the edge opposite: a region never shrinks
    /// to nothing, since nothing would be left to grow again. Held to
    /// `ratio`, as [`Region::grown`] is.
    pub fn shrunk(self, side: Side, by: u32, ratio: Option<f64>, image: [u32; 2]) -> Self {
        if let Some(ratio) = ratio {
            let at = self.edge(side) - side.outward() * i64::from(by);
            return self.edge_held(side, self.short_of_opposite(side, at), ratio, image);
        }
        let mut edges = self.edges();
        let axis = side.axis();
        let (low, high) = (axis, axis + 2);
        match side {
            Side::Left | Side::Top => {
                edges[low] = (edges[low] + i64::from(by)).min(edges[high] - 1)
            }
            Side::Right | Side::Bottom => {
                edges[high] = (edges[high] - i64::from(by)).max(edges[low] + 1);
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
    ///
    /// Held to `ratio`, the edge the arrow moves governs: an edge's sides
    /// follow about the middle, and a corner's follow from the corner
    /// opposite.
    pub fn nudged(
        self,
        grip: Grip,
        direction: [i64; 2],
        ratio: Option<f64>,
        image: [u32; 2],
    ) -> Option<Self> {
        if let Some(ratio) = ratio {
            return self.nudged_held(grip, direction, ratio, image);
        }
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

    /// The region drawn by a drag from `from` to `to`, held to `ratio`: from
    /// the boundary nearest the press, toward the hand, as far as the hand
    /// has gone along whichever side it has gone further along in the
    /// ratio's terms. `None` where [`Region::from_corners`] would enclose
    /// nothing.
    pub fn drawn(
        from: [f32; 2],
        to: [f32; 2],
        ratio: Option<f64>,
        image: [u32; 2],
    ) -> Option<Self> {
        let free = Self::from_corners(from, to, image)?;
        let Some(ratio) = ratio else {
            return Some(free);
        };
        let anchor = [0, 1].map(|axis| {
            f64::from(from[axis])
                .round()
                .clamp(0.0, f64::from(image[axis])) as i64
        });
        Some(Self::reaching(anchor, to, [1, 1], ratio, image))
    }

    /// The same middle and about the same number of pixels, in the shape
    /// `ratio` asks for: what choosing an aspect does to the region on
    /// screen. Smaller where the image has no room for it that shape.
    pub fn shaped(self, ratio: f64, image: [u32; 2]) -> Self {
        let ratio = sane(ratio);
        let area = f64::from(self.width) * f64::from(self.height);
        let width = (area * ratio).sqrt();
        let height = width / ratio;
        // The longer side governs, its rounding being the smaller share of
        // it.
        let (axis, extent) = if width >= height {
            (0, width)
        } else {
            (1, height)
        };
        let middle = self.middle();
        Self::held(
            axis,
            extent,
            ratio,
            [Placing::About(middle[0]), Placing::About(middle[1])],
            image,
        )
    }

    /// The region with its width — `axis` 0 — or its height set to
    /// `extent`, its top left corner where it was: a size typed. Held to
    /// `ratio`, the other side follows from the same corner. As much as the
    /// image has room for from there.
    pub fn sized(self, axis: usize, extent: u32, ratio: Option<f64>, image: [u32; 2]) -> Self {
        let at = [i64::from(self.x), i64::from(self.y)];
        let Some(ratio) = ratio else {
            let mut sides = [self.width, self.height];
            let room = image[axis].saturating_sub([self.x, self.y][axis]).max(1);
            sides[axis] = extent.clamp(1, room);
            return Self {
                width: sides[0],
                height: sides[1],
                ..self
            };
        };
        Self::held(
            axis,
            f64::from(extent),
            ratio,
            at.map(|at| Placing::From { at, toward: 1 }),
            image,
        )
    }

    /// The region the other way up about its middle: its width and height
    /// traded, and smaller where the image has no room for that.
    pub fn swapped(self, image: [u32; 2]) -> Self {
        self.shaped(f64::from(self.height) / f64::from(self.width), image)
    }

    /// The boundary on `side`.
    fn edge(&self, side: Side) -> i64 {
        let axis = side.axis();
        match side {
            Side::Left | Side::Top => self.edges()[axis],
            Side::Right | Side::Bottom => self.edges()[axis + 2],
        }
    }

    /// `at`, a place for the edge on `side`, held a pixel short of the edge
    /// opposite on the side of it the edge is on.
    fn short_of_opposite(&self, side: Side, at: i64) -> i64 {
        let anchor = self.edge(side.opposite());
        match side.outward() {
            1 => at.max(anchor + 1),
            _ => at.min(anchor - 1),
        }
    }

    /// The middle, in image coordinates.
    fn middle(&self) -> [f64; 2] {
        [
            f64::from(self.x) + f64::from(self.width) / 2.0,
            f64::from(self.y) + f64::from(self.height) / 2.0,
        ]
    }

    fn pulled_held(self, grip: Grip, to: [f32; 2], ratio: f64, image: [u32; 2]) -> Self {
        match grip {
            Grip::Corner(across, down) => {
                if !to.iter().all(|at| at.is_finite()) {
                    return self;
                }
                let anchor = [self.edge(across.opposite()), self.edge(down.opposite())];
                Self::reaching(anchor, to, [across.outward(), down.outward()], ratio, image)
            }
            Grip::Edge(side) => {
                let at = to[side.axis()];
                if !at.is_finite() {
                    return self;
                }
                self.edge_held(side, f64::from(at).round() as i64, ratio, image)
            }
            Grip::Middle | Grip::Inside => self,
        }
    }

    fn nudged_held(
        self,
        grip: Grip,
        direction: [i64; 2],
        ratio: f64,
        image: [u32; 2],
    ) -> Option<Self> {
        let axis = (0..2).find(|&axis| direction[axis] != 0)?;
        let step = direction[axis].signum();
        let side = grip.sides()[axis]?;
        let at = self.short_of_opposite(side, self.edge(side) + step);
        Some(match grip {
            Grip::Corner(across, down) => {
                let anchor = [self.edge(across.opposite()), self.edge(down.opposite())];
                let toward = [across.outward(), down.outward()];
                let placing = [0, 1].map(|axis| Placing::From {
                    at: anchor[axis],
                    toward: toward[axis],
                });
                let extent = (at - anchor[axis]).abs() as f64;
                Self::held(axis, extent, ratio, placing, image)
            }
            _ => self.edge_held(side, at, ratio, image),
        })
    }

    /// The edge on `side` moved to `at`, held to `ratio`: the sides beside
    /// it follow about the middle. Past the edge opposite, it flips over
    /// it, as [`Region::pulled`] does.
    fn edge_held(self, side: Side, at: i64, ratio: f64, image: [u32; 2]) -> Self {
        let axis = side.axis();
        let anchor = self.edge(side.opposite());
        let reach = at - anchor;
        let toward = match reach {
            0 => side.outward(),
            reach => reach.signum(),
        };
        let mut placing = [
            Placing::About(self.middle()[0]),
            Placing::About(self.middle()[1]),
        ];
        placing[axis] = Placing::From { at: anchor, toward };
        Self::held(axis, reach.abs() as f64, ratio, placing, image)
    }

    /// The region from the boundary `anchor` toward `to`, held to `ratio`,
    /// as far as `to` has gone along whichever side it has gone further
    /// along in the ratio's terms. Where the hand is level with the anchor
    /// on an axis, the region goes `otherwise` along it.
    fn reaching(
        anchor: [i64; 2],
        to: [f32; 2],
        otherwise: [i64; 2],
        ratio: f64,
        image: [u32; 2],
    ) -> Self {
        let ratio = sane(ratio);
        let reach = [0, 1].map(|axis| f64::from(to[axis]) - anchor[axis] as f64);
        let placing = [0, 1].map(|axis| Placing::From {
            at: anchor[axis],
            toward: if reach[axis] == 0.0 {
                otherwise[axis]
            } else {
                reach[axis].signum() as i64
            },
        });
        let (axis, extent) = if reach[0].abs() >= reach[1].abs() * ratio {
            (0, reach[0].abs())
        } else {
            (1, reach[1].abs())
        };
        Self::held(axis, extent, ratio, placing, image)
    }

    /// The one shape every held change makes: `extent` pixels along `axis`
    /// and what `ratio` makes of that along the other, each side placed as
    /// `placing` says, all of it as much as the image has room for in the
    /// ratio — the side along `axis` rounded first and the other worked out
    /// from it, so that a pixel's step along the governing side is never
    /// lost to the rounding. A side placed from a boundary that has no room
    /// the way it goes goes the other way.
    fn held(axis: usize, extent: f64, ratio: f64, placing: [Placing; 2], image: [u32; 2]) -> Self {
        let ratio = sane(ratio);
        let limit = image.map(|side| i64::from(side.max(1)));
        let mut placing = placing;
        let mut room = limit;
        for (place, (room, limit)) in placing.iter_mut().zip(room.iter_mut().zip(limit)) {
            if let Placing::From { at, toward } = place {
                *at = (*at).clamp(0, limit);
                let ahead = if *toward > 0 { limit - *at } else { *at };
                if ahead > 0 {
                    *room = ahead;
                } else {
                    *toward = -*toward;
                }
            }
        }
        // What one pixel along `axis` makes along the other.
        let per = if axis == 0 { 1.0 / ratio } else { ratio };
        let other = 1 - axis;
        let mut size = [0.0; 2];
        size[axis] = if extent.is_finite() {
            extent.max(1.0)
        } else {
            1.0
        };
        size[other] = size[axis] * per;
        let fit = (room[0] as f64 / size[0])
            .min(room[1] as f64 / size[1])
            .min(1.0);
        let mut sides = [0; 2];
        sides[axis] = ((size[axis] * fit).round() as i64).clamp(1, room[axis]);
        sides[other] = ((sides[axis] as f64 * per).round() as i64).clamp(1, room[other]);
        let start = |axis: usize| match placing[axis] {
            Placing::From { at, toward } if toward > 0 => at,
            Placing::From { at, .. } => at - sides[axis],
            Placing::About(middle) => ((middle - sides[axis] as f64 / 2.0).round() as i64)
                .clamp(0, limit[axis] - sides[axis]),
        };
        Self {
            x: start(0) as u32,
            y: start(1) as u32,
            width: sides[0] as u32,
            height: sides[1] as u32,
        }
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

    /// The region as it lies once the picture it is on, `shown` pixels
    /// across and down before the turn, is turned by `turn` more: the same
    /// pixels, marked out in the turned picture's coordinates.
    pub fn turned(self, turn: Turn, shown: [u32; 2]) -> Self {
        let mut region = self;
        let mut sides = shown;
        for _ in 0..turn.quarters() {
            // A quarter clockwise: the left column becomes the top row, and
            // the bottom row the left column.
            region = Region {
                x: sides[1] - (region.y + region.height),
                y: region.x,
                width: region.height,
                height: region.width,
            };
            sides.swap(0, 1);
        }
        region
    }

    /// The same pixels, each moved by `map`: a turn or a flip of the whole
    /// picture carries a rectangle to a rectangle, so its two opposite
    /// corner pixels say where all of it went.
    fn mapped(self, map: impl Fn([u32; 2]) -> [u32; 2]) -> Self {
        let [ax, ay] = map([self.x, self.y]);
        let [bx, by] = map([self.right() - 1, self.bottom() - 1]);
        Self::between(ax.min(bx), ay.min(by), ax.max(bx) + 1, ay.max(by) + 1)
    }

    /// The region of a picture turned by `turn` as it lies in the picture
    /// before the turn, `stored` pixels across and down: [`Region::turned`]
    /// the other way.
    pub fn stored(self, turn: Turn, stored: [u32; 2]) -> Self {
        self.mapped(|at| turn.stored(at, stored))
    }

    /// The region of a picture decoded upright, `upright` pixels across and
    /// down, as it lies in the file's own grid before `orientation` turned
    /// it: what a tool that reads the file's bytes counts in.
    pub fn raw(self, orientation: Orientation, upright: [u32; 2]) -> Self {
        self.mapped(|at| orient::raw(orientation, at, upright))
    }
}

/// Where a side of a held region goes along its axis: from a boundary,
/// one way or the other, or about a middle.
#[derive(Clone, Copy)]
enum Placing {
    From { at: i64, toward: i64 },
    About(f64),
}

/// A ratio a region can be held to: a square for anything that is not a
/// positive number.
fn sane(ratio: f64) -> f64 {
    if ratio.is_finite() && ratio > 0.0 {
        ratio
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A region marked on a turned picture is the same pixels before the
    /// turn: [`Region::stored`] undoes [`Region::turned`] at every turn,
    /// and the region in the file's own grid covers what it covered.
    #[test]
    fn a_region_undone_through_the_turn_and_the_tag_covers_the_same_pixels() {
        let stored = [8, 6];
        let region = Region {
            x: 1,
            y: 2,
            width: 3,
            height: 2,
        };
        let mut turn = Turn::NONE;
        for _ in 0..4 {
            let turned = region.turned(turn, stored);
            assert_eq!(turned.stored(turn, stored), region, "{turn:?}");
            turn = turn.clockwise();
        }
        // The file's own grid, under a tag that turned it a quarter: the
        // upright picture's top left is the raw picture's bottom left.
        let corner = Region {
            x: 0,
            y: 0,
            width: 2,
            height: 1,
        };
        assert_eq!(
            corner.raw(Orientation::Rotate90, [8, 6]),
            Region {
                x: 0,
                y: 6,
                width: 1,
                height: 2,
            }
        );
        assert_eq!(corner.raw(Orientation::NoTransforms, [8, 6]), corner);
    }

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

        let right = start.pulled(Grip::Edge(Side::Right), [45.4, 99.0], None, IMAGE);
        assert_eq!(
            right,
            region(10, 10, 35, 10),
            "an edge moves on its own axis"
        );

        let corner = start.pulled(Grip::Corner(Side::Left, Side::Top), [5.0, 2.0], None, IMAGE);
        assert_eq!(corner, region(5, 2, 25, 18));

        // Past the far edge: the rectangle between the anchored edge and the
        // hand, on the other side of it.
        let flipped = start.pulled(Grip::Edge(Side::Left), [36.0, 0.0], None, IMAGE);
        assert_eq!(flipped, region(30, 10, 6, 10));

        // Exactly on the far edge, a pixel is kept on the near side of it.
        let touching = start.pulled(Grip::Edge(Side::Right), [10.0, 0.0], None, IMAGE);
        assert_eq!(touching, region(10, 10, 1, 10));
        // And on the far side where the image ends at the anchored edge.
        let at_end = region(90, 0, 10, 5).pulled(Grip::Edge(Side::Left), [100.0, 0.0], None, IMAGE);
        assert_eq!(at_end, region(99, 0, 1, 5));

        // Off the image: held to it.
        let off = start.pulled(
            Grip::Corner(Side::Right, Side::Bottom),
            [500.0, 500.0],
            None,
            IMAGE,
        );
        assert_eq!(off, region(10, 10, 90, 50));

        // The inside is not an edge to pull.
        assert_eq!(start.pulled(Grip::Inside, [0.0, 0.0], None, IMAGE), start);
        assert_eq!(start.pulled(Grip::Middle, [0.0, 0.0], None, IMAGE), start);
        // Nor is a NaN a place to pull to.
        assert_eq!(
            start.pulled(Grip::Edge(Side::Right), [f32::NAN, 0.0], None, IMAGE),
            start
        );
    }

    /// Growing moves one edge outward and stops at the image.
    #[test]
    fn growing_pushes_one_edge_out_as_far_as_the_image_goes() {
        let start = region(10, 10, 20, 10);
        assert_eq!(
            start.grown(Side::Right, 1, None, IMAGE),
            region(10, 10, 21, 10)
        );
        assert_eq!(
            start.grown(Side::Left, 1, None, IMAGE),
            region(9, 10, 21, 10)
        );
        assert_eq!(
            start.grown(Side::Top, 3, None, IMAGE),
            region(10, 7, 20, 13)
        );
        assert_eq!(
            start.grown(Side::Bottom, 100, None, IMAGE),
            region(10, 10, 20, 50)
        );
        assert_eq!(
            start.grown(Side::Left, 100, None, IMAGE),
            region(0, 10, 30, 10)
        );
    }

    /// Shrinking moves one edge inward and stops a pixel short of the edge
    /// opposite.
    #[test]
    fn shrinking_pulls_one_edge_in_and_leaves_a_pixel() {
        let start = region(10, 10, 20, 10);
        assert_eq!(
            start.shrunk(Side::Right, 1, None, IMAGE),
            region(10, 10, 19, 10)
        );
        assert_eq!(
            start.shrunk(Side::Left, 1, None, IMAGE),
            region(11, 10, 19, 10)
        );
        assert_eq!(
            start.shrunk(Side::Top, 3, None, IMAGE),
            region(10, 13, 20, 7)
        );
        assert_eq!(
            start.shrunk(Side::Bottom, 100, None, IMAGE),
            region(10, 10, 20, 1)
        );
        assert_eq!(
            start.shrunk(Side::Left, 100, None, IMAGE),
            region(29, 10, 1, 10)
        );
        // A region a pixel wide has nothing left to give.
        let thin = region(10, 10, 1, 10);
        assert_eq!(thin.shrunk(Side::Right, 1, None, IMAGE), thin);
        assert_eq!(thin.shrunk(Side::Left, 1, None, IMAGE), thin);
    }

    /// A nudge moves a handle's edges a pixel the way the arrow points, never
    /// through the edge opposite, and not at all along an edge.
    #[test]
    fn a_nudge_moves_a_handle_one_pixel() {
        let start = region(10, 10, 20, 10);
        let right = Grip::Edge(Side::Right);
        assert_eq!(
            start.nudged(right, [1, 0], None, IMAGE),
            Some(region(10, 10, 21, 10))
        );
        assert_eq!(
            start.nudged(right, [-1, 0], None, IMAGE),
            Some(region(10, 10, 19, 10))
        );
        // Up has no meaning for the right edge.
        assert_eq!(start.nudged(right, [0, -1], None, IMAGE), None);
        // A corner answers to both axes, one at a time.
        let corner = Grip::Corner(Side::Left, Side::Top);
        assert_eq!(
            start.nudged(corner, [0, 1], None, IMAGE),
            Some(region(10, 11, 20, 9))
        );
        // An edge stops a pixel short of the one opposite.
        let thin = region(10, 10, 1, 10);
        assert_eq!(thin.nudged(right, [-1, 0], None, IMAGE), Some(thin));
        assert_eq!(
            thin.nudged(Grip::Edge(Side::Left), [1, 0], None, IMAGE),
            Some(thin)
        );
        // And at the edge of the image.
        let flush = region(80, 0, 20, 10);
        assert_eq!(flush.nudged(right, [1, 0], None, IMAGE), Some(flush));
        // The inside holds no edge.
        assert_eq!(start.nudged(Grip::Inside, [1, 0], None, IMAGE), None);
        assert_eq!(start.nudged(Grip::Middle, [1, 0], None, IMAGE), None);
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

    /// A turn carries the region's pixels with it, and turning back puts it
    /// where it was.
    #[test]
    fn a_region_turns_with_the_picture() {
        let r = region(10, 5, 30, 20);
        let one = Turn::NONE.clockwise();
        assert_eq!(r.turned(one, IMAGE), region(35, 10, 20, 30));
        assert_eq!(r.turned(one.clockwise(), IMAGE), region(60, 35, 30, 20));
        assert_eq!(
            r.turned(Turn::NONE.counterclockwise(), IMAGE),
            region(5, 60, 20, 30)
        );
        assert_eq!(r.turned(Turn::NONE, IMAGE), r);

        let turned = r.turned(one, IMAGE);
        assert_eq!(turned.turned(Turn::NONE.counterclockwise(), [60, 100]), r);
        let mut back = r;
        let mut shown = IMAGE;
        for _ in 0..4 {
            back = back.turned(one, shown);
            shown = [shown[1], shown[0]];
        }
        assert_eq!(back, r);

        // Every pixel inside lands inside, every one outside stays outside:
        // under a quarter clockwise, pixel (x, y) goes to (h - 1 - y, x).
        for y in 0..IMAGE[1] {
            for x in 0..IMAGE[0] {
                let before = r.contains([x as f32 + 0.5, y as f32 + 0.5]);
                let at = [(IMAGE[1] - 1 - y) as f32 + 0.5, x as f32 + 0.5];
                assert_eq!(turned.contains(at), before, "({x}, {y})");
            }
        }
    }

    /// Whether `region` is `ratio` to within the pixel its shorter side was
    /// rounded to, and inside the image.
    fn holds(region: Region, ratio: f64) -> bool {
        let (width, height) = (f64::from(region.width), f64::from(region.height));
        let inside = region.right() <= IMAGE[0] && region.bottom() <= IMAGE[1];
        let near = if width >= height {
            (width / ratio - height).abs() <= 0.5 + 1e-9
        } else {
            (height * ratio - width).abs() <= 0.5 + 1e-9
        };
        inside && near
    }

    /// A drag held to a ratio reaches from the press toward the hand as far
    /// as it has gone along the side it has gone further along, in any
    /// direction, and stops where the image does without leaving the ratio.
    #[test]
    fn a_held_drag_reaches_along_the_side_it_went_further_along() {
        let square = Some(1.0);
        // Further across than down: the width governs.
        assert_eq!(
            Region::drawn([10.0, 10.0], [30.0, 15.0], square, IMAGE),
            Some(region(10, 10, 20, 20))
        );
        // Further down: the height does.
        assert_eq!(
            Region::drawn([10.0, 10.0], [12.0, 40.0], square, IMAGE),
            Some(region(10, 10, 30, 30))
        );
        // Up and to the left, from the same boundary.
        assert_eq!(
            Region::drawn([30.0, 30.0], [20.0, 25.0], square, IMAGE),
            Some(region(20, 20, 10, 10))
        );
        // At 3:2, a reach of 30 across is 20 down.
        assert_eq!(
            Region::drawn([0.0, 0.0], [30.0, 1.0], Some(1.5), IMAGE),
            Some(region(0, 0, 30, 20))
        );
        // Past the foot of the image: as large as the room down allows.
        let held = Region::drawn([10.0, 40.0], [90.0, 45.0], square, IMAGE).unwrap();
        assert_eq!(held, region(10, 40, 20, 20));
        // Nothing enclosed is nothing, held or not.
        assert_eq!(Region::drawn([-5.0, 3.0], [-1.0, 8.0], square, IMAGE), None);
        // And free is what it always was.
        assert_eq!(
            Region::drawn([10.2, 5.7], [20.1, 15.0], None, IMAGE),
            Region::from_corners([10.2, 5.7], [20.1, 15.0], IMAGE)
        );
    }

    /// A corner pulled keeps the corner opposite; an edge pulled keeps the
    /// edge opposite and the middle across it; each flips past the far
    /// side as a free one does.
    #[test]
    fn a_held_handle_keeps_the_far_corner_or_the_middle() {
        let start = region(20, 20, 30, 20);
        let ratio = Some(1.5);
        let corner = start.pulled(
            Grip::Corner(Side::Right, Side::Bottom),
            [65.0, 30.0],
            ratio,
            IMAGE,
        );
        assert_eq!(corner, region(20, 20, 45, 30));
        let edge = start.pulled(Grip::Edge(Side::Right), [56.0, 0.0], ratio, IMAGE);
        assert_eq!(edge, region(20, 18, 36, 24), "the middle across it kept");
        // Past the left edge: the region goes the other way from it.
        let flipped = start.pulled(Grip::Edge(Side::Right), [5.0, 0.0], ratio, IMAGE);
        assert_eq!((flipped.x, flipped.right()), (5, 20));
        assert!(holds(flipped, 1.5), "{flipped:?}");
        // An edge pulled off the image stops at its edge, the height
        // following about the middle.
        let wide = start.pulled(Grip::Edge(Side::Right), [500.0, 0.0], ratio, IMAGE);
        assert_eq!(wide, region(20, 4, 80, 53));
        // And where the height runs out first, it stops there instead.
        let tall = start.pulled(Grip::Edge(Side::Bottom), [0.0, 500.0], ratio, IMAGE);
        assert_eq!((tall.y, tall.height), (20, 40));
        assert_eq!(tall.width, 60, "the width the room down allows");
        // The middle is moved, not pulled.
        assert_eq!(start.pulled(Grip::Middle, [0.0, 0.0], ratio, IMAGE), start);
    }

    /// A key's step along the side it governs is never lost to the
    /// rounding: each press grows the region by a pixel, held, until the
    /// image stops it.
    #[test]
    fn a_held_step_moves_a_pixel_every_time() {
        let ratio = Some(1.5);
        let mut region = region(10, 10, 15, 10);
        for _ in 0..20 {
            let grown = region.grown(Side::Right, 1, ratio, IMAGE);
            assert_eq!(grown.width, region.width + 1, "{region:?}");
            assert!(holds(grown, 1.5), "{grown:?}");
            region = grown;
        }
        let shrunk = region.shrunk(Side::Bottom, 1, ratio, IMAGE);
        assert_eq!(shrunk.height, region.height - 1);
        assert_eq!(shrunk.y, region.y, "the top is where it was");
        // A corner, along the axis the arrow points.
        let corner = Grip::Corner(Side::Left, Side::Top);
        let nudged = region.nudged(corner, [-1, 0], ratio, IMAGE).unwrap();
        assert_eq!(nudged.width, region.width + 1);
        assert_eq!(
            (nudged.right(), nudged.bottom()),
            (region.right(), region.bottom())
        );
        // An edge has no step across itself, held or not.
        assert_eq!(
            region.nudged(Grip::Edge(Side::Right), [0, 1], ratio, IMAGE),
            None
        );
        // A region a pixel high has nothing left to shrink.
        let thin = Region {
            x: 0,
            y: 0,
            width: 2,
            height: 1,
        };
        let shrunk = thin.shrunk(Side::Bottom, 1, Some(2.0), IMAGE);
        assert_eq!((shrunk.width, shrunk.height), (2, 1));
    }

    /// Choosing an aspect keeps the middle and about the area; where the
    /// image has no room for that, it gives up size rather than shape.
    #[test]
    fn shaping_keeps_the_middle_and_the_area() {
        let start = region(30, 20, 40, 10);
        let square = start.shaped(1.0, IMAGE);
        assert_eq!(square, region(40, 15, 20, 20));
        let tall = region(0, 0, 100, 60).shaped(0.5, IMAGE);
        assert_eq!((tall.width, tall.height), (30, 60));
        assert_eq!(tall.x, 35, "about the middle");
        // Swapped, a free region trades its sides about its middle.
        assert_eq!(start.swapped(IMAGE), region(45, 5, 10, 40));
    }

    /// A size typed keeps the top left corner, and the other side follows
    /// it where the region is held.
    #[test]
    fn a_size_typed_keeps_the_corner() {
        let start = region(10, 10, 20, 10);
        assert_eq!(start.sized(0, 40, None, IMAGE), region(10, 10, 40, 10));
        assert_eq!(start.sized(0, 400, None, IMAGE), region(10, 10, 90, 10));
        assert_eq!(start.sized(1, 30, Some(2.0), IMAGE), region(10, 10, 60, 30));
        // As far as the room from the corner allows, in the ratio.
        assert_eq!(
            start.sized(0, 400, Some(2.0), IMAGE),
            region(10, 10, 90, 45)
        );
    }
}
