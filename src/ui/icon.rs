//! The marks an icon is drawn from, and the grid they are placed on.
//!
//! An icon here is a short list of [`Mark`]s on a 24-unit square, which is
//! the grid [Lucide](https://lucide.dev) draws on, scaled into whatever
//! square the button has room for. The geometry of the icons below is
//! Lucide's, which is ISC-licensed: `REUSE.toml` records that, and
//! `LICENSES/ISC.txt` is the text it refers to.
//!
//! Why a table of marks rather than the path data Lucide ships: a path can be
//! drawn but it cannot be *hinted*. At the size a toggle wears one — a dozen
//! pixels or so — an icon is sharp when each of its strokes is a whole number
//! of device pixels wide and centered where those pixels meet, and blurred
//! when it is not; nothing else about it matters nearly as much. Marks are
//! placed one at a time so that each can be put there. Rounding to whole
//! *logical* pixels is not the same thing and is not enough: a display with
//! 1.5 device pixels to the logical one puts half of those marks astride a
//! pixel boundary, and egui's feathering then draws them at two different
//! weights.
//!
//! Placing each mark on its own is not enough on its own, either. Lucide's
//! marks are spaced six grid units apart — the lines of a lattice, the bars
//! of a chart, the offset between one sheet and the next — and where six
//! units come to five and a half device pixels there is no placing of four
//! lines that leaves three equal gaps between them: they round outward,
//! inward, outward, inward, and the middle cell comes out a third wider than
//! its neighbors. So the square is sized in whole [`QUANTUM`]s of the grid
//! first, which makes every mark on a multiple of three land on a whole
//! device pixel, and the spacing survives the snapping.

use super::Rect;

/// The side of the square an icon is described on: Lucide's grid.
const GRID: f32 = 24.0;
/// The stroke every mark is drawn with, in grid units. Lucide's, too.
const STROKE: f32 = 2.0;
/// The device pixels one third of the grid must come to, and so the quantum
/// the square is sized in.
///
/// A third because Lucide places almost everything on a multiple of three:
/// the lines of `grid-3x3` at 3, 9, 15 and 21, the bars of a chart at 6, 12
/// and 18, the six units between one sheet of `copy` and the other. Size the
/// square in whole eighths of a device pixel's worth of that — which is to
/// say, in whole 8-pixel steps — and every one of those coordinates is a
/// whole number of device pixels, so marks meant to be evenly spaced are.
const QUANTUM: f32 = 8.0;

/// One stroke or fill of an icon, in grid units.
///
/// Positions are the *center line* of what is drawn, the way an SVG path's
/// are: a stroked rectangle's band reaches half a stroke either side of the
/// rectangle given here, which is why Lucide's icons are inset from the edge
/// of their grid by about a stroke.
pub(super) enum Mark {
    /// A straight stroke between two points, with a round cap at each end.
    Line([f32; 2], [f32; 2]),
    /// A stroked rectangle, `radius` grid units round at the corners.
    Rect {
        at: [f32; 2],
        size: [f32; 2],
        radius: f32,
    },
    /// A stroked circle.
    Circle { at: [f32; 2], radius: f32 },
    /// A stroked arc of `sweep` degrees from `start`, about `at`.
    ///
    /// Angles run from the direction of increasing x and turn clockwise on
    /// screen, since that is the way the interface's y axis points; a
    /// negative sweep turns the other way. Drawn as a chain of straight
    /// strokes, each with the round cap that joins it to the next.
    Arc {
        at: [f32; 2],
        radius: f32,
        start: f32,
        sweep: f32,
    },
    /// A filled dot one stroke across: a round cap on its own, which is how
    /// Lucide writes the tittle over an `i`.
    Dot([f32; 2]),
    /// The filled region between the polyline `top`, given left to right, and
    /// the horizontal line `baseline` under it: a plot standing on its axis.
    Area {
        top: &'static [[f32; 2]],
        baseline: f32,
    },
    /// A filled rectangle in the ground the icon is sitting on, laid down
    /// before the marks that follow so they read as being in front of what is
    /// behind them rather than woven through it.
    ///
    /// Only usable where that ground is a color and not a translucent wash
    /// over something else, since it works by covering rather than by
    /// erasing.
    Knockout {
        at: [f32; 2],
        size: [f32; 2],
        radius: f32,
    },
}

/// How many degrees of an arc one straight stroke stands in for. Twelve
/// leaves under a sixth of a pixel between the chord and the curve at the
/// sizes a button draws, which is less than the feather either side of it.
const ARC_STEP: f32 = 12.0;

/// Lucide's `chart-area`: an axis with a filled plot standing on it, which is
/// what the panel it opens draws.
pub(super) const CHART_AREA: &[Mark] = &[
    Mark::Line([3.0, 3.0], [3.0, 21.0]),
    Mark::Line([3.0, 21.0], [21.0, 21.0]),
    Mark::Area {
        top: &[
            [7.0, 11.2],
            [9.0, 9.0],
            [12.3, 12.3],
            [16.6, 8.0],
            [18.0, 8.4],
        ],
        baseline: 19.0,
    },
];

/// Lucide's `info`. The panel this opens is a column of words about the file
/// rather than a picture of anything, so the mark for it is the one the rest
/// of the world already uses for that.
pub(super) const INFO: &[Mark] = &[
    Mark::Circle {
        at: [12.0, 12.0],
        radius: 10.0,
    },
    Mark::Line([12.0, 16.0], [12.0, 12.0]),
    Mark::Dot([12.0, 8.0]),
];

/// Lucide's `square-square`: the whole inside a frame, and a smaller view of
/// it inside that, which is what the minimap shows.
pub(super) const SQUARE_SQUARE: &[Mark] = &[
    Mark::Rect {
        at: [3.0, 3.0],
        size: [18.0, 18.0],
        radius: 2.0,
    },
    Mark::Rect {
        at: [8.0, 8.0],
        size: [8.0, 8.0],
        radius: 1.0,
    },
];

/// Lucide's `clipboard`: the board a pasted picture arrives on.
///
/// Lucide draws the board as one path that starts beside the clip, runs all
/// the way round and stops on the clip's other side, leaving the top edge
/// open where the clip stands; a whole rounded rectangle with the clip laid
/// over it would put a stroke straight through the middle of the clip. So
/// the board is written out here as Lucide draws it — the four straight
/// sides, the four corners as quarter turns, and the top edge in two pieces.
pub(super) const CLIPBOARD: &[Mark] = &[
    Mark::Line([6.0, 4.0], [8.0, 4.0]),
    Mark::Line([16.0, 4.0], [18.0, 4.0]),
    Mark::Arc {
        at: [18.0, 6.0],
        radius: 2.0,
        start: -90.0,
        sweep: 90.0,
    },
    Mark::Line([20.0, 6.0], [20.0, 20.0]),
    Mark::Arc {
        at: [18.0, 20.0],
        radius: 2.0,
        start: 0.0,
        sweep: 90.0,
    },
    Mark::Line([18.0, 22.0], [6.0, 22.0]),
    Mark::Arc {
        at: [6.0, 20.0],
        radius: 2.0,
        start: 90.0,
        sweep: 90.0,
    },
    Mark::Line([4.0, 20.0], [4.0, 6.0]),
    Mark::Arc {
        at: [6.0, 6.0],
        radius: 2.0,
        start: 180.0,
        sweep: 90.0,
    },
    // The clip, last so that it is drawn over the ends of the top edge
    // rather than under them.
    Mark::Rect {
        at: [8.0, 2.0],
        size: [8.0, 4.0],
        radius: 1.0,
    },
];

/// Lucide's `expand`: four corners with an arrow reaching out to each. The
/// fit that takes in the whole image, where the two below take in one axis.
pub(super) const EXPAND: &[Mark] = &[
    Mark::Line([3.0, 9.0], [3.0, 3.0]),
    Mark::Line([3.0, 3.0], [9.0, 3.0]),
    Mark::Line([3.0, 3.0], [9.0, 9.0]),
    Mark::Line([21.0, 9.0], [21.0, 3.0]),
    Mark::Line([21.0, 3.0], [15.0, 3.0]),
    Mark::Line([15.0, 9.0], [21.0, 3.0]),
    Mark::Line([3.0, 15.0], [3.0, 21.0]),
    Mark::Line([3.0, 21.0], [9.0, 21.0]),
    Mark::Line([3.0, 21.0], [9.0, 15.0]),
    Mark::Line([21.0, 15.0], [21.0, 21.0]),
    Mark::Line([21.0, 21.0], [15.0, 21.0]),
    Mark::Line([15.0, 15.0], [21.0, 21.0]),
];

/// Lucide's `chevron-left` and `chevron-right`: one chevron pointing the way
/// it goes, for the two buttons at the head of the top bar that step back and
/// on through the file list. A single chevron rather than the doubled one the
/// window's nudges wear: those move by a step of a size the button chose,
/// where these go to the next thing along, which is what one chevron says
/// everywhere else.
pub(super) const CHEVRON_LEFT: &[Mark] = &[
    Mark::Line([15.0, 18.0], [9.0, 12.0]),
    Mark::Line([9.0, 12.0], [15.0, 6.0]),
];

pub(super) const CHEVRON_RIGHT: &[Mark] = &[
    Mark::Line([9.0, 18.0], [15.0, 12.0]),
    Mark::Line([15.0, 12.0], [9.0, 6.0]),
];

/// Lucide's `chevrons-left` and `chevrons-right`: a pair of chevrons pointing
/// the one way, for the two nudges that slide the display window along the
/// axis without changing how wide it is.
pub(super) const CHEVRONS_LEFT: &[Mark] = &[
    Mark::Line([11.0, 17.0], [6.0, 12.0]),
    Mark::Line([6.0, 12.0], [11.0, 7.0]),
    Mark::Line([18.0, 17.0], [13.0, 12.0]),
    Mark::Line([13.0, 12.0], [18.0, 7.0]),
];

pub(super) const CHEVRONS_RIGHT: &[Mark] = &[
    Mark::Line([6.0, 17.0], [11.0, 12.0]),
    Mark::Line([11.0, 12.0], [6.0, 7.0]),
    Mark::Line([13.0, 17.0], [18.0, 12.0]),
    Mark::Line([18.0, 12.0], [13.0, 7.0]),
];

/// Lucide's `chevrons-right-left`: the two of them facing each other, for the
/// nudge that narrows the window. [`CHEVRONS_LEFT_RIGHT`] below is the same
/// pair back to back, and widens it — the marks are what the window's two
/// ends do.
pub(super) const CHEVRONS_RIGHT_LEFT: &[Mark] = &[
    Mark::Line([4.0, 7.0], [9.0, 12.0]),
    Mark::Line([9.0, 12.0], [4.0, 17.0]),
    Mark::Line([20.0, 7.0], [15.0, 12.0]),
    Mark::Line([15.0, 12.0], [20.0, 17.0]),
];

/// The nudge that widens the display window and, the marks being a pair of
/// edges pushed apart, the fit that fills the window across.
pub(super) const CHEVRONS_LEFT_RIGHT: &[Mark] = &[
    Mark::Line([9.0, 7.0], [4.0, 12.0]),
    Mark::Line([4.0, 12.0], [9.0, 17.0]),
    Mark::Line([15.0, 7.0], [20.0, 12.0]),
    Mark::Line([20.0, 12.0], [15.0, 17.0]),
];

/// Lucide's `chevrons-up-down`: the same pair stood on end, and the mark for
/// filling the window down it as [`CHEVRONS_LEFT_RIGHT`] is for filling it
/// across. Which of the two the fill wears is the shape of the image against
/// the shape of the window — see `ui::menu::fit_icon`.
pub(super) const CHEVRONS_UP_DOWN: &[Mark] = &[
    Mark::Line([7.0, 9.0], [12.0, 4.0]),
    Mark::Line([12.0, 4.0], [17.0, 9.0]),
    Mark::Line([7.0, 15.0], [12.0, 20.0]),
    Mark::Line([12.0, 20.0], [17.0, 15.0]),
];

/// Lucide's `rotate-ccw`: a turn back to where the rendering started.
///
/// The arc runs counterclockwise from the left of the circle almost the whole
/// way round, and the two short strokes are the head of the arrow it arrives
/// as. Lucide draws that last stretch on a slightly wider radius than the
/// rest; one radius throughout is a difference no button is large enough to
/// show.
pub(super) const ROTATE_CCW: &[Mark] = &[
    Mark::Arc {
        at: [12.0, 12.0],
        radius: 9.0,
        start: 180.0,
        sweep: -336.0,
    },
    Mark::Line([3.0, 3.0], [3.0, 8.0]),
    Mark::Line([3.0, 8.0], [8.0, 8.0]),
];

/// Lucide's `spline`: a curve between two of its own control points, for the
/// switch that bends the count axis.
pub(super) const SPLINE: &[Mark] = &[
    Mark::Arc {
        at: [17.0, 17.0],
        radius: 12.0,
        start: 180.0,
        sweep: 90.0,
    },
    Mark::Circle {
        at: [5.0, 19.0],
        radius: 2.0,
    },
    Mark::Circle {
        at: [19.0, 5.0],
        radius: 2.0,
    },
];

/// Lucide's `grid-3x3`: a frame with two lines each way through it, which is
/// the smallest thing that reads as squares rather than as a hash.
pub(super) const GRID_3X3: &[Mark] = &[
    Mark::Rect {
        at: [3.0, 3.0],
        size: [18.0, 18.0],
        radius: 2.0,
    },
    Mark::Line([3.0, 9.0], [21.0, 9.0]),
    Mark::Line([3.0, 15.0], [21.0, 15.0]),
    Mark::Line([9.0, 3.0], [9.0, 21.0]),
    Mark::Line([15.0, 3.0], [15.0, 21.0]),
];

/// Lucide's `maximize-2`: two arrows reaching into opposite corners, for the
/// press that gives the picture the whole window.
///
/// Not [`EXPAND`], which reaches into all four: that one is the fit that
/// takes the whole image in, and two marks a window apart meaning two
/// different things would be two marks to tell apart.
pub(super) const MAXIMIZE_2: &[Mark] = &[
    Mark::Line([15.0, 3.0], [21.0, 3.0]),
    Mark::Line([21.0, 3.0], [21.0, 9.0]),
    Mark::Line([21.0, 3.0], [14.0, 10.0]),
    Mark::Line([3.0, 15.0], [3.0, 21.0]),
    Mark::Line([3.0, 21.0], [9.0, 21.0]),
    Mark::Line([3.0, 21.0], [10.0, 14.0]),
];

/// Lucide's `circle-dot`: a ring with a point at its center, which is one
/// pixel picked out of everything around it — the button that says how the
/// pixel under the pointer is read out.
pub(super) const CIRCLE_DOT: &[Mark] = &[
    Mark::Circle {
        at: [12.0, 12.0],
        radius: 10.0,
    },
    Mark::Circle {
        at: [12.0, 12.0],
        radius: 1.0,
    },
];

/// Lucide's `copy`: one sheet behind another and offset from it, which is
/// what a copy is.
///
/// Lucide leaves the corner of the sheet behind out of its path where the
/// one in front covers it; here the sheet in front is knocked out of the
/// ground first instead, which draws the same silhouette and does not need
/// the path to know what is in front of it.
pub(super) const COPY: &[Mark] = &[
    Mark::Rect {
        at: [2.0, 2.0],
        size: [14.0, 14.0],
        radius: 2.0,
    },
    Mark::Knockout {
        at: [8.0, 8.0],
        size: [14.0, 14.0],
        radius: 2.0,
    },
    Mark::Rect {
        at: [8.0, 8.0],
        size: [14.0, 14.0],
        radius: 2.0,
    },
];

/// Lucide's `x`: the cross that takes a thing off the screen. Its two strokes
/// cross at the middle of the grid and reach the same distance into each
/// corner, so it stays square however the square it is fitted into rounds.
pub(super) const X: &[Mark] = &[
    Mark::Line([18.0, 6.0], [6.0, 18.0]),
    Mark::Line([6.0, 6.0], [18.0, 18.0]),
];

/// The device's grid, in the terms a mark is placed on it: physical pixels
/// to the logical one, and the arithmetic that puts a coordinate on a whole
/// one. The same arithmetic the display list does for itself, held here so
/// that an icon painted by egui lands where one drawn by the list did.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) struct Grid {
    scale: f32,
}

impl Grid {
    pub(super) fn new(pixels_per_point: f32) -> Self {
        Self {
            scale: pixels_per_point,
        }
    }

    fn usable(self) -> bool {
        self.scale.is_finite() && self.scale > 0.0
    }

    /// A stroke `thickness` logical pixels thick, as the nearest whole
    /// number of device pixels and never fewer than one.
    pub(super) fn line_width(self, thickness: f32) -> f32 {
        if self.usable() {
            (thickness * self.scale).round().max(1.0) / self.scale
        } else {
            thickness
        }
    }

    /// One coordinate moved onto the device's own pixel grid.
    pub(super) fn snap(self, value: f32) -> f32 {
        if self.usable() {
            (value * self.scale).round() / self.scale
        } else {
            value
        }
    }

    /// The largest whole number of `step`s that fits inside `value`, and
    /// never fewer than one — see [`fit`] for what it is for.
    fn snap_within(self, value: f32, step: f32) -> f32 {
        if !self.usable() || step <= 0.0 {
            return value;
        }
        let step = step * self.scale;
        ((value * self.scale / step).floor().max(1.0) * step) / self.scale
    }

    /// `pixels` device pixels, in logical ones.
    pub(super) fn device_pixels(self, pixels: f32) -> f32 {
        if self.usable() {
            pixels / self.scale
        } else {
            pixels
        }
    }

    /// Where a stroke `thickness` wide, wanted at `at` device pixels, has to
    /// be centered for both of its edges to land on device pixel boundaries:
    /// the middle of a pixel when it is an odd number of them across, and
    /// the seam between two when it is even. In logical pixels.
    fn stroke_center_in_device(self, at: f32, thickness: f32) -> f32 {
        if !self.usable() {
            return at;
        }
        let phase = if (thickness * self.scale).round() as i32 % 2 == 0 {
            0.0
        } else {
            0.5
        };
        ((at - phase).round() + phase) / self.scale
    }

    /// `value` in device pixels, rounded to a whole one.
    fn to_device(self, value: f32) -> f32 {
        if self.usable() {
            (value * self.scale).round()
        } else {
            value
        }
    }
}

/// The square an icon is drawn in: the largest whole number of [`QUANTUM`]s
/// that fits in `budget` logical pixels, centered in `within`.
///
/// `budget` is room set aside rather than a size asked for, and the square
/// comes back no larger — so a caller that reserved `budget` in its layout
/// has reserved enough, and a button squeezed by a window dragged narrow
/// crops its mark rather than growing one that spills out of it. What it
/// costs is that the mark is drawn at whole steps: a little under the budget
/// at one scale, right at it on another.
pub(super) fn fit(grid: Grid, within: Rect, budget: f32) -> Rect {
    let budget = budget.min(within.width).min(within.height).max(0.0);
    let step = grid.device_pixels(QUANTUM);
    // A budget too small to hold one step gets what there is: an icon under a
    // third of the grid across has no even spacing left to protect anyway.
    let side = if budget < step {
        grid.snap(budget)
    } else {
        grid.snap_within(budget, step)
    };
    Rect::new(
        grid.snap(within.x + (within.width - side) / 2.0),
        grid.snap(within.y + (within.height - side) / 2.0),
        side,
        side,
    )
}

/// The same, for a square egui is asked to draw in.
pub(super) fn square(grid: Grid, within: egui::Rect, budget: f32) -> egui::Rect {
    let fitted = fit(
        grid,
        Rect::new(within.min.x, within.min.y, within.width(), within.height()),
        budget,
    );
    egui::Rect::from_min_size(
        egui::pos2(fitted.x, fitted.y),
        egui::vec2(fitted.width, fitted.height),
    )
}

/// The stroke an icon `side` logical pixels across is drawn with: Lucide's
/// two units of twenty-four, taken to the nearest whole device pixel and
/// never less than one.
fn stroke_for(grid: Grid, side: f32) -> f32 {
    grid.line_width(side * STROKE / GRID)
}

/// Grid units to logical pixels, on the device's own grid.
///
/// The arithmetic is done in device pixels rather than logical ones, and that
/// is what makes evenly spaced marks come out evenly spaced. [`fit`] leaves
/// the square a whole number of them across and a whole number of [`QUANTUM`]s
/// wide, so a mark at a multiple of three grid units is at an exact whole
/// device pixel — `3k * 8m / 24` is `km`, with nothing left over for the
/// floating point to lose. Ask for the same position in logical pixels and it
/// becomes a repeating fraction, and the marks of one lattice then round to
/// either side of where they belong.
pub(super) struct Placer {
    grid: Grid,
    /// The square's corner and side, in device pixels.
    origin: [f32; 2],
    side: f32,
    /// Logical pixels to the grid unit, for the measures that are not center
    /// lines: a corner radius, a circle's own radius.
    unit: f32,
    stroke: f32,
}

impl Placer {
    pub(super) fn new(grid: Grid, within: Rect) -> Self {
        let side = within.width.min(within.height);
        Self {
            grid,
            origin: [grid.to_device(within.x), grid.to_device(within.y)],
            side: grid.to_device(side),
            unit: side / GRID,
            stroke: stroke_for(grid, side),
        }
    }

    /// One point of a stroke's center line, placed so that the stroke's two
    /// edges land on device pixel boundaries.
    fn at(&self, point: [f32; 2]) -> [f32; 2] {
        [
            self.grid
                .stroke_center_in_device(self.device(self.origin[0], point[0]), self.stroke),
            self.grid
                .stroke_center_in_device(self.device(self.origin[1], point[1]), self.stroke),
        ]
    }

    /// A rectangle's center line, both corners placed by [`Placer::at`].
    fn boxed(&self, at: [f32; 2], size: [f32; 2]) -> Rect {
        let start = self.at(at);
        let end = self.at([at[0] + size[0], at[1] + size[1]]);
        Rect::new(start[0], start[1], end[0] - start[0], end[1] - start[1])
    }

    /// A point placed on the grid but not snapped to the device's: for the
    /// vertices of a filled shape and the center of a round one, where moving
    /// a point onto the pixel grid bends the outline rather than sharpening
    /// it.
    pub(super) fn free(&self, point: [f32; 2]) -> [f32; 2] {
        [
            self.grid
                .device_pixels(self.device(self.origin[0], point[0])),
            self.grid
                .device_pixels(self.device(self.origin[1], point[1])),
        ]
    }

    /// `units` grid units, in logical pixels.
    pub(super) fn units(&self, units: f32) -> f32 {
        units * self.unit
    }

    fn device(&self, origin: f32, at: f32) -> f32 {
        origin + at * self.side / GRID
    }

    /// A filled rectangle, whose *edges* rather than whose center line are
    /// what has to be on the device grid.
    fn filled(&self, at: [f32; 2], size: [f32; 2]) -> Rect {
        let corner = self.free(at);
        let (x, y) = (self.grid.snap(corner[0]), self.grid.snap(corner[1]));
        let least = self.grid.line_width(0.0);
        Rect::new(
            x,
            y,
            (self.grid.snap(corner[0] + size[0] * self.unit) - x).max(least),
            (self.grid.snap(corner[1] + size[1] * self.unit) - y).max(least),
        )
    }

    /// The points of the chain of straight strokes that stands in for an
    /// arc. Nothing here is snapped: a curve meets the pixel grid at every
    /// angle, so there is no placing of it that the feather does not have to
    /// finish; what the grid is for is the straight strokes beside it.
    fn arc(&self, at: [f32; 2], radius: f32, start: f32, sweep: f32) -> Vec<[f32; 2]> {
        let center = self.free(at);
        let radius = self.units(radius);
        let steps = ((sweep.abs() / ARC_STEP).ceil() as usize).max(2);
        (0..=steps)
            .map(|step| {
                let angle = (start + sweep * step as f32 / steps as f32).to_radians();
                [
                    center[0] + radius * angle.cos(),
                    center[1] + radius * angle.sin(),
                ]
            })
            .collect()
    }
}

/// Draws `marks` in `within` — a square from [`square`] — in `ink`, with
/// egui's painter. Each stroke is given round caps, as Lucide draws them.
pub(super) fn paint(
    painter: &egui::Painter,
    marks: &[Mark],
    within: egui::Rect,
    ink: egui::Color32,
    ground: egui::Color32,
) {
    use egui::{Pos2, Stroke, StrokeKind, pos2};

    if within.width() <= 0.0 || within.height() <= 0.0 {
        return;
    }
    let grid = Grid::new(painter.pixels_per_point());
    let place = Placer::new(
        grid,
        Rect::new(within.min.x, within.min.y, within.width(), within.height()),
    );
    let stroke = place.stroke;
    let pen = Stroke::new(stroke, ink);
    let point = |at: [f32; 2]| pos2(at[0], at[1]);
    // A stroke with round caps: the segment, and a dot at each end.
    let capped = |from: Pos2, to: Pos2| {
        painter.line_segment([from, to], pen);
        painter.circle_filled(from, stroke / 2.0, ink);
        painter.circle_filled(to, stroke / 2.0, ink);
    };
    let boxed = |rect: Rect| {
        egui::Rect::from_min_size(pos2(rect.x, rect.y), egui::vec2(rect.width, rect.height))
    };
    for mark in marks {
        match mark {
            Mark::Line(from, to) => capped(point(place.at(*from)), point(place.at(*to))),
            Mark::Rect { at, size, radius } => {
                painter.rect_stroke(
                    boxed(place.boxed(*at, *size)),
                    radius * place.unit,
                    pen,
                    StrokeKind::Middle,
                );
            }
            Mark::Circle { at, radius } => {
                let across = grid.snap(radius * place.unit).max(stroke);
                painter.circle_stroke(point(place.at(*at)), across, pen);
            }
            Mark::Arc {
                at,
                radius,
                start,
                sweep,
            } => {
                let points = place.arc(*at, *radius, *start, *sweep);
                for pair in points.windows(2) {
                    capped(point(pair[0]), point(pair[1]));
                }
            }
            Mark::Dot(at) => {
                painter.circle_filled(point(place.at(*at)), stroke / 2.0, ink);
            }
            Mark::Area { top, baseline } => {
                let foot = grid.snap(place.free([0.0, *baseline])[1]);
                // One trapezoid under each segment of the top, each of them
                // convex, which is what egui can fill.
                for pair in top.windows(2) {
                    let (a, b) = (place.free(pair[0]), place.free(pair[1]));
                    painter.add(egui::Shape::convex_polygon(
                        vec![
                            pos2(a[0], a[1]),
                            pos2(b[0], b[1]),
                            pos2(b[0], foot),
                            pos2(a[0], foot),
                        ],
                        ink,
                        Stroke::NONE,
                    ));
                }
            }
            Mark::Knockout { at, size, radius } => {
                painter.rect_filled(boxed(place.filled(*at, *size)), radius * place.unit, ground);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scales a display actually asks for, whole and fractional.
    const SCALES: [f32; 5] = [1.0, 1.25, 1.5, 1.75, 2.0];

    /// Every icon in the table, so that a new one is held to the same
    /// promises as the rest.
    const ICONS: [&[Mark]; 11] = [
        CHART_AREA,
        INFO,
        SQUARE_SQUARE,
        GRID_3X3,
        COPY,
        EXPAND,
        MAXIMIZE_2,
        CHEVRONS_LEFT_RIGHT,
        CHEVRONS_UP_DOWN,
        ROTATE_CCW,
        SPLINE,
    ];

    fn device(value: f32, scale: f32) -> f32 {
        value * scale
    }

    #[test]
    fn the_square_is_a_whole_number_of_device_pixels() {
        for scale in SCALES {
            let square = fit(Grid::new(scale), Rect::new(0.0, 0.0, 22.0, 22.0), 14.0);
            let side = device(square.width, scale);
            assert!(
                (side - side.round()).abs() < 1e-3,
                "scale {scale}: side {side} device pixels"
            );
            assert_eq!(square.width, square.height);
        }
    }

    #[test]
    fn the_square_never_outgrows_what_holds_it() {
        let button = Rect::new(0.0, 0.0, 9.0, 9.0);
        let square = fit(Grid::new(1.0), button, 14.0);
        assert!(square.width <= button.width, "{square:?}");
    }

    /// The whole point: whatever the scale, every stroke's two edges land on
    /// device pixel boundaries, so the shader's feather has nothing to do.
    #[test]
    fn every_stroke_lands_on_the_device_grid() {
        for scale in SCALES {
            let grid = Grid::new(scale);
            let square = fit(grid, Rect::new(0.0, 0.0, 22.0, 22.0), 14.0);
            let place = Placer::new(grid, square);
            let half = device(place.stroke, scale) / 2.0;
            let mut checked = 0;
            for icon in ICONS {
                for mark in icon {
                    for edge in stroke_edges(&place, mark) {
                        for side in [device(edge, scale) - half, device(edge, scale) + half] {
                            assert!(
                                (side - side.round()).abs() < 1e-3,
                                "scale {scale}: a stroke edge at {side} device pixels"
                            );
                            checked += 1;
                        }
                    }
                }
            }
            assert!(checked > 0);
        }
    }

    /// A stroke a whole number of device pixels wide, whatever the scale, and
    /// never rounded away to nothing: the other half of the same promise.
    #[test]
    fn the_stroke_is_a_whole_number_of_device_pixels() {
        for scale in SCALES {
            let stroke = device(stroke_for(Grid::new(scale), 14.0), scale);
            assert!(stroke >= 1.0, "scale {scale}: stroke {stroke}");
            assert!(
                (stroke - stroke.round()).abs() < 1e-3,
                "scale {scale}: stroke {stroke} device pixels"
            );
        }
    }

    /// Every mark sits inside the grid it is described on, and clear of its
    /// edge by the half stroke the band reaches out — so an icon is never
    /// clipped by the square it is drawn in.
    #[test]
    fn no_mark_leaves_the_grid() {
        let room = STROKE / 2.0..=GRID - STROKE / 2.0;
        for icon in ICONS {
            for mark in icon {
                for point in center_lines(mark) {
                    assert!(
                        room.contains(&point[0]) && room.contains(&point[1]),
                        "{point:?} is off the grid"
                    );
                }
            }
        }
    }

    /// Where a stroked mark's center line falls in logical pixels, as `paint`
    /// places it. A filled mark has no center line: its edges are snapped as
    /// edges, and it contributes nothing here.
    fn stroke_edges(place: &Placer, mark: &Mark) -> Vec<f32> {
        let both = |point: [f32; 2]| {
            let at = place.at(point);
            vec![at[0], at[1]]
        };
        match mark {
            Mark::Line(from, to) => [both(*from), both(*to)].concat(),
            Mark::Rect { at, size, .. } => {
                [both(*at), both([at[0] + size[0], at[1] + size[1]])].concat()
            }
            Mark::Circle { at, radius } => {
                let center = place.at(*at);
                let across = place.grid.snap(radius * place.unit).max(place.stroke);
                vec![
                    center[0] - across,
                    center[0] + across,
                    center[1] - across,
                    center[1] + across,
                ]
            }
            // A curve, a fill and a cap meet the grid at every angle;
            // there is no snapping of them to check.
            Mark::Arc { .. } | Mark::Area { .. } | Mark::Dot(_) | Mark::Knockout { .. } => {
                Vec::new()
            }
        }
    }

    /// Every point a mark is described by, in grid units.
    fn center_lines(mark: &Mark) -> Vec<[f32; 2]> {
        match mark {
            Mark::Line(from, to) => vec![*from, *to],
            Mark::Rect { at, size, .. } | Mark::Knockout { at, size, .. } => {
                vec![*at, [at[0] + size[0], at[1] + size[1]]]
            }
            Mark::Circle { at, radius } => vec![
                [at[0] - radius, at[1] - radius],
                [at[0] + radius, at[1] + radius],
            ],
            Mark::Arc {
                at,
                radius,
                start,
                sweep,
            } => (0..=36)
                .map(|step| {
                    let angle = (start + sweep * step as f32 / 36.0).to_radians();
                    [at[0] + radius * angle.cos(), at[1] + radius * angle.sin()]
                })
                .collect(),
            Mark::Dot(at) => vec![*at],
            Mark::Area { top, baseline } => top
                .iter()
                .copied()
                .chain(top.iter().map(|at| [at[0], *baseline]))
                .collect(),
        }
    }
}
