//! The grid laid over the image: hairlines a round distance apart in
//! whatever the picture is measured in — its own pixels, a print's inches or
//! millimeters, or the ground the file is placed on — spaced so that they
//! land about every fifty logical pixels of window.
//!
//! What it measures in is the readout's coordinate, through the one reading
//! of it the region's labels take too ([`Ruler::of`]), so that what the
//! grid divides the picture into is what the pointer reads off it. The
//! spacing is not the user's to choose. What a grid is for is reading
//! distances off the image, and a grid whose lines are too far apart to
//! bracket anything — or so close that they are a wash — does not do that at
//! any zoom. So one spacing is picked per frame from the round numbers a
//! ruler is marked in, and the toggle says which one is in force.
//!
//! The lines are where the measure reads a whole number of spacings, not a
//! whole number of spacings in from the picture's edge: a print's measure
//! starts at its edge, so the two are the same there, but the ground's does
//! not, and its lines fall on the round eastings and northings a map is
//! ruled on — which two rasters on the same ground share.

use crate::render::Placement;

use super::Rect;
use super::ruler::{Ruler, Unit, unsigned_nought};
use crate::theme::Theme;

/// What the spacing aims at, in logical pixels. Laid out in logical rather
/// than physical ones so the grid reads the same size on any display, the
/// way the rest of the interface does.
const TARGET_SPACING: f64 = 50.0;

/// The mantissas the spacing is chosen from, a decade at a time: the marks on
/// a ruler.
const STEPS: [f64; 3] = [1.0, 2.0, 5.0];

/// How close together lines may fall, in physical pixels, before the grid is
/// left off entirely. Two of them: a line and the gap beside it, which is the
/// least that still reads as a grid rather than as a film over the image.
const MIN_SPACING: f64 = 2.0;

/// How far a line may be from the picture's edge, in its pixels, and still
/// be taken to be on it — and so not drawn, the edge being no division.
/// Ground coordinates are large numbers worked back to pixels, and a raster
/// cut on a round easting comes back a hair off its own edge.
const ON_EDGE: f64 = 1e-6;

/// How far apart the lines are, in `ruler`'s unit, at a zoom of `zoom`
/// physical pixels to the image pixel on a display of `scale`.
///
/// Whichever of 1, 2, 5, 10, 20, 50, … — or 0.1, 0.2, 0.5, … — puts them
/// nearest to [`TARGET_SPACING`] apart, nearest by ratio, since that is how
/// a spacing is read. Along the axis they fall closer together on, where
/// the two differ: a print or a raster whose pixels are not square has
/// squares of its own unit on the paper or the ground, and so not on the
/// screen. Never finer than the image's own pixel grid where the unit is
/// the pixel, which is as far as dividing an image up can go.
pub(super) fn step(ruler: &Ruler, zoom: f32, scale: f32) -> f64 {
    if !zoom.is_finite() || zoom <= 0.0 {
        return 1.0;
    }
    let per_pixel = ruler.per_pixel(0).max(ruler.per_pixel(1));
    let target =
        TARGET_SPACING * f64::from(scale.max(f32::MIN_POSITIVE)) / f64::from(zoom) * per_pixel;
    if !(target.is_finite() && target > 0.0) {
        return 1.0;
    }
    let decade = 10f64.powf(target.log10().floor());
    let mantissa = target / decade;
    // The boundaries are the geometric means of the neighboring steps, so
    // that "nearest" is nearest by ratio: a spacing 40% out either way is the
    // worst any zoom can be given.
    let chosen = STEPS
        .iter()
        .copied()
        .chain([10.0])
        .find(|&candidate| mantissa <= candidate * next_step(candidate).sqrt())
        .unwrap_or(10.0);
    let step = chosen * decade;
    match ruler.unit {
        Unit::Pixel => step.max(1.0),
        _ => step,
    }
}

/// The ratio from one step to the next: 1 → 2, 2 → 5, 5 → 10.
fn next_step(step: f64) -> f64 {
    match step {
        s if s < 2.0 => 2.0,
        s if s < 5.0 => 2.5,
        _ => 2.0,
    }
}

/// What the grid is not drawn over: the two things on the picture that are
/// the image layer's, drawn underneath the whole interface, so that unlike
/// the panels they cannot cover a grid line laid across them.
///
/// `minimap` is the thumbnail's rectangle when the minimap is on screen. A
/// grid belongs to the image being looked at, not to the map of where in it
/// that is. `glass` is the loupe's circle, as center and radius, while the
/// loupe is up: the grid's spacing is the view's, which is not the glass's.
#[derive(Clone, Copy, Default)]
pub(super) struct Clear {
    pub minimap: Option<Rect>,
    pub glass: Option<([f32; 2], f32)>,
}

/// Draws the grid over the image, at `step` of `ruler`'s unit between
/// lines.
///
/// Only where the image actually is, and only where the interface has left it
/// visible: the panels are opaque and are drawn after this, but the image can
/// also be zoomed until it runs off every edge, and the grid marks up an
/// image rather than a window.
///
/// `clear` is what is left clear of it — see [`Clear`].
#[allow(clippy::too_many_arguments)]
pub(super) fn paint(
    painter: &egui::Painter,
    placement: Placement,
    scale: f32,
    content: Rect,
    clear: Clear,
    ruler: &Ruler,
    step: f64,
    theme: &Theme,
    font: &egui::FontId,
) {
    let Clear { minimap, glass } = clear;
    if !placement.zoom.is_finite() || placement.zoom <= 0.0 || scale <= 0.0 {
        return;
    }
    // The image's own rectangle, in the logical pixels the interface is laid
    // out in, and the part of it there is anything to draw on.
    let image = Rect::new(
        placement.x / scale,
        placement.y / scale,
        placement.width / scale,
        placement.height / scale,
    );
    let Some(area) = image.intersect(content) else {
        return;
    };
    // Image pixels to the logical one, and the picture's own size in them.
    let per = placement.zoom / scale;
    let extent = [placement.width, placement.height].map(|side| f64::from(side / placement.zoom));
    let zoom = f64::from(placement.zoom);
    if (0..2).any(|axis| step / ruler.per_pixel(axis) * zoom < MIN_SPACING) {
        return;
    }
    let across = Along {
        ruler,
        axis: 0,
        step,
        origin: image.x,
        per,
        extent: extent[0],
    };
    let down = Along {
        axis: 1,
        origin: image.y,
        extent: extent[1],
        ..across
    };

    // One physical pixel wide, on the device's grid: a hairline that lands
    // between pixels is feathered across two of them, and a grid of those
    // reads as a haze rather than as lines.
    let width = 1.0 / scale;
    let snap = |value: f32| (value * scale).round() / scale;
    let ink: egui::Color32 = theme.bar_background.into();

    for x in across.lines(area.x, area.right()) {
        let line = Rect::new(snap(x), area.y, width, area.height);
        let holes = [
            minimap,
            glass.and_then(|(center, radius)| chord(line, center, radius)),
        ];
        fill_around(painter, line, &holes, ink);
    }
    for y in down.lines(area.y, area.bottom()) {
        let line = Rect::new(area.x, snap(y), area.width, width);
        let holes = [
            minimap,
            glass.and_then(|(center, radius)| chord(line, center, radius)),
        ];
        fill_around(painter, line, &holes, ink);
    }

    // Laid out with no ink of their own, so that the painter's is the one
    // they are drawn in, whatever the theme.
    let laid = |text: &str| {
        painter.layout_no_wrap(text.to_string(), font.clone(), egui::Color32::PLACEHOLDER)
    };
    let measured = |text: &str| {
        let size = laid(text).size();
        [size.x, size.y]
    };
    for label in labels(across, down, image, content, clear, measured) {
        let rect = Rect::new(
            snap(label.rect.x),
            snap(label.rect.y),
            label.rect.width,
            label.rect.height,
        );
        // Over the picture, on a ground of the menus' to be read against
        // whatever is under it; beside it, on the window's own empty
        // ground, in the dimmer ink the bars' facts are written in.
        let text = if label.over {
            painter.rect_filled(rect.into(), 3.0, theme.menu_background);
            theme.text_primary
        } else {
            theme.text_dim
        };
        painter.galley(
            egui::pos2(rect.x + LABEL_INSET[0], rect.y + LABEL_INSET[1]),
            laid(&label.text),
            text.into(),
        );
    }
}

/// Where a hairline crosses the circle of `radius` about `center`: the
/// piece of `line` inside the circle, as a rectangle the line's own width,
/// or `None` where the line passes it by. The line is taken along its
/// middle, being a pixel wide; the rim drawn over the circle's edge covers
/// what that leaves at either end of the chord.
fn chord(line: Rect, center: [f32; 2], radius: f32) -> Option<Rect> {
    let vertical = line.height > line.width;
    let (along, across) = if vertical {
        (line.x + line.width / 2.0 - center[0], center[1])
    } else {
        (line.y + line.height / 2.0 - center[1], center[0])
    };
    let half = (radius * radius - along * along).sqrt();
    if !half.is_finite() || half <= 0.0 {
        return None;
    }
    let hole = if vertical {
        Rect::new(line.x, across - half, line.width, 2.0 * half)
    } else {
        Rect::new(across - half, line.y, 2.0 * half, line.height)
    };
    line.intersect(hole)
}

/// Fills `rect`, less whatever the `holes` cover of it.
fn fill_around(painter: &egui::Painter, rect: Rect, holes: &[Option<Rect>], color: egui::Color32) {
    let fill = |piece: Rect| {
        painter.rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(piece.x, piece.y),
                egui::vec2(piece.width, piece.height),
            ),
            0.0,
            color,
        );
    };
    for piece in pieces(rect, holes) {
        fill(piece);
    }
}

/// The pieces of `rect` left once every one of the `holes` is taken out
/// of it: each hole cuts every piece the ones before it left, so the
/// pieces never overlap and none of them touches a hole. What the loupe
/// clips its rings to as well, around the minimap.
pub(super) fn pieces(rect: Rect, holes: &[Option<Rect>]) -> Vec<Rect> {
    let mut left = vec![rect];
    for hole in holes.iter().flatten() {
        left = left
            .into_iter()
            .flat_map(|piece| match piece.intersect(*hole) {
                Some(cut) => around(piece, cut).to_vec(),
                None => vec![piece],
            })
            .filter(|piece| piece.width > 0.0 && piece.height > 0.0)
            .collect();
    }
    left
}

/// The pieces of `rect` left over once `hole` — which is inside it — is taken
/// out: the strips above and below it, and the two beside it between them.
/// Some of the four are empty where the hole meets an edge of `rect`.
fn around(rect: Rect, hole: Rect) -> [Rect; 4] {
    [
        Rect::new(rect.x, rect.y, rect.width, hole.y - rect.y),
        Rect::new(
            rect.x,
            hole.bottom(),
            rect.width,
            rect.bottom() - hole.bottom(),
        ),
        Rect::new(rect.x, hole.y, hole.x - rect.x, hole.height),
        Rect::new(
            hole.right(),
            hole.y,
            rect.right() - hole.right(),
            hole.height,
        ),
    ]
}

/// One of the picture's axes as the lines across it are worked out:
/// `ruler`'s `axis`, a line every `step` of it, on a picture whose near edge
/// is at `origin` logical pixels with `per` of them to its pixel, and which
/// is `extent` of its pixels long.
#[derive(Clone, Copy)]
struct Along<'a> {
    ruler: &'a Ruler,
    axis: usize,
    step: f64,
    origin: f32,
    per: f32,
    extent: f64,
}

impl Along<'_> {
    /// Where the lines fall, in logical pixels, in order: wherever the
    /// measure reads a whole number of steps inside the picture and inside
    /// the visible `from..to`.
    ///
    /// The image's own edges are not lines: the grid divides the image up,
    /// and a line along the edge would draw a frame around it instead.
    fn lines(self, from: f32, to: f32) -> Vec<f32> {
        self.ticks(from, to).into_iter().map(|(at, _)| at).collect()
    }

    /// The same lines, each with what the measure reads along it.
    fn ticks(self, from: f32, to: f32) -> Vec<(f32, f64)> {
        let Along {
            ruler,
            axis,
            step,
            origin,
            per,
            extent,
        } = self;
        let mut out = Vec::new();
        if !(step.is_finite() && step > 0.0 && per > 0.0 && extent > 0.0) {
            return out;
        }
        // The part of the picture in view, in its pixels, and what the
        // measure reads at either end of it — the wrong way round where the
        // measure runs back along the axis.
        let low = (f64::from((from - origin) / per)).max(0.0);
        let high = (f64::from((to - origin) / per)).min(extent);
        if low.partial_cmp(&high) != Some(std::cmp::Ordering::Less) {
            return out;
        }
        let [a, b] = [ruler.at(axis, low), ruler.at(axis, high)];
        let (first, last) = ((a.min(b) / step).ceil(), (a.max(b) / step).floor());
        if !(first.is_finite() && last.is_finite()) {
            return out;
        }
        let mut index = first;
        while index <= last {
            let pixel = ruler.pixels(axis, index * step);
            let at = origin + (pixel as f32) * per;
            if pixel > ON_EDGE && pixel < extent - ON_EDGE && at >= from && at < to {
                out.push((at, index * step));
            }
            index += 1.0;
        }
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        out
    }

    /// How far apart the lines are on screen, in logical pixels.
    fn spacing(self) -> f32 {
        (self.step / self.ruler.per_pixel(self.axis)) as f32 * self.per
    }
}

/// How the spacing is written in the toggle: how far apart the lines are,
/// in what they measure. The ground's meters go up to kilometers where they
/// are as many, which is how a map's scale bar writes them; every other unit
/// stays as it is, an inch being written in inches however many of them.
pub(super) fn label(step: f64, ruler: &Ruler) -> String {
    match ruler.unit {
        Unit::Pixel => format!("{step:.0} px"),
        _ if in_kilometers(step, ruler) => {
            format!("{} km", written(step / 1000.0, step / 1000.0))
        }
        _ => format!("{}{}", written(step, step), ruler.suffix()),
    }
}

/// Whether lines `step` apart in what `ruler` measures are written in
/// kilometers: meters on the ground, a thousand or more of them apart.
fn in_kilometers(step: f64, ruler: &Ruler) -> bool {
    ruler.unit == Unit::Ground(Some("m")) && step >= 1000.0
}

/// `value` with as many places as `every`, a round number, needs to be
/// written exactly, and no sign on a nought.
fn written(value: f64, every: f64) -> String {
    // A hair under, so that a tenth worked out as a hair over one place
    // does not ask for two.
    let places = (-every.log10() - 1e-9).ceil().clamp(0.0, 9.0) as usize;
    unsigned_nought(format!("{value:.places$}"))
}

/// What the line at `value` is labeled, where the lines labeled are
/// `every` apart: the bare number, in the unit the toggle writes the spacing
/// in — kilometers where it writes kilometers — the unit being said once
/// there rather than at every line.
fn reading(value: f64, every: f64, step: f64, ruler: &Ruler) -> String {
    match in_kilometers(step, ruler) {
        true => written(value / 1000.0, every / 1000.0),
        false => written(value, every),
    }
}

/// How far apart, in `step`s, the lines that are labeled are: every one
/// where a label has `room` logical pixels to go between two lines
/// `spacing` apart, and otherwise the fewest that leave it that much — a
/// round number of the unit apart, so that the lines labeled read as the
/// marks a ruler numbers do: by tens on one ruled in fives, never by
/// fifteens.
fn labeled_every(step: f64, spacing: f32, room: f32) -> u64 {
    if !spacing.is_finite() || spacing <= 0.0 {
        return u64::MAX;
    }
    // The ruler's marks from the decade the step is in up, of which only
    // those a whole number of steps can be.
    let mut decade = 10f64.powf(step.log10().floor());
    loop {
        for mantissa in STEPS {
            let every = mantissa * decade;
            let count = (every / step).round();
            if (every / step - count).abs() > 1e-6 || count < 1.0 {
                continue;
            }
            if count as f32 * spacing >= room {
                return count as u64;
            }
        }
        decade *= 10.0;
        if decade / step > 1e9 {
            return u64::MAX;
        }
    }
}

/// The room between a label's words and the edge of what it is written on,
/// and between a label and the image it sits beside or the window's edge it
/// sits inside.
const LABEL_INSET: [f32; 2] = [4.0, 1.0];
const LABEL_GAP: f32 = 4.0;

/// The least room between two labels along an edge, beyond their own
/// width: enough that two numbers do not read as one.
const LABEL_SPACE: f32 = 12.0;

/// One of the grid's labels: what it says, the rectangle it takes, and
/// whether it is written over the picture — and so on a ground of its own
/// to be read against it — or beside it, where the window is empty.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Label {
    pub text: String,
    pub rect: Rect,
    pub over: bool,
}

/// Which side of the picture the labels along one axis go on, and whether
/// they are written beside it, in the window's empty space, or over it.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Side {
    /// The near edge — the top or the left — beside the picture, or over
    /// it, along the near edge of what is visible of it.
    Near { over: bool },
    /// Beside the far edge, the bottom or the right.
    Far,
}

/// Where the labels go along `axis`'s edges: beside the picture where the
/// window has room for them on either side of it, the near side first, and
/// otherwise over the picture along the near edge of what is visible of it.
fn side(axis: usize, image: Rect, content: Rect, depth: f32) -> Side {
    let (near, far) = match axis {
        0 => (image.y - content.y, content.bottom() - image.bottom()),
        _ => (image.x - content.x, content.right() - image.right()),
    };
    let need = depth + 2.0 * LABEL_GAP;
    if near >= need {
        Side::Near { over: false }
    } else if far >= need {
        Side::Far
    } else {
        Side::Near { over: true }
    }
}

/// The labels the grid's lines wear, at the edges of what can be seen of
/// the picture: the reading of each line down the picture beside it across
/// the top, and of each line across it down the left — or across the
/// bottom and down the right, where the window has room for them beside
/// the picture there and not at the top or the left.
///
/// Beside the picture is where the window has room to spare, panned or
/// zoomed out until there is empty space around it: the labels sit just
/// outside the picture's edge, as a map's sit outside its frame, and cover
/// none of it. With none, they go over it, just inside the window's edge.
///
/// A line is labeled only as often as there is room for its words, by
/// [`labeled_every`]; a label that would run off the window, cover the
/// minimap or the loupe, or touch one already placed — the corner the two
/// edges share — is left out.
fn labels(
    across: Along,
    down: Along,
    image: Rect,
    content: Rect,
    clear: Clear,
    mut measure: impl FnMut(&str) -> [f32; 2],
) -> Vec<Label> {
    let Some(area) = image.intersect(content) else {
        return Vec::new();
    };
    let mut placed: Vec<Label> = Vec::new();
    // Down the side first: its labels are short and every one is wanted,
    // where the top has more to give way.
    for along in [down, across] {
        let axis = along.axis;
        let (from, to) = match axis {
            0 => (area.x, area.right()),
            _ => (area.y, area.bottom()),
        };
        let ticks = along.ticks(from, to);
        if ticks.is_empty() {
            continue;
        }
        // Sized by the widest of what every line would say at the finest
        // a label could be: the labels kept are among them.
        let sizes: Vec<[f32; 2]> = ticks
            .iter()
            .map(|&(_, value)| measure(&reading(value, along.step, along.step, along.ruler)))
            .collect();
        let widest = sizes.iter().fold([0.0f32; 2], |most, size| {
            [most[0].max(size[0]), most[1].max(size[1])]
        });
        let box_size = [
            widest[0] + 2.0 * LABEL_INSET[0],
            widest[1] + 2.0 * LABEL_INSET[1],
        ];
        // Along the edge, a label takes its width across the top and its
        // height down the side.
        let (length, depth) = match axis {
            0 => (box_size[0], box_size[1]),
            _ => (box_size[1], box_size[0]),
        };
        let count = labeled_every(along.step, along.spacing(), length + LABEL_SPACE);
        let every = along.step * count as f64;
        let side = side(axis, image, content, depth);
        for (at, value) in ticks {
            let index = (value / every).round();
            if (value / every - index).abs() > 1e-6 {
                continue;
            }
            let text = reading(value, every, along.step, along.ruler);
            let size = measure(&text);
            let size = [
                size[0] + 2.0 * LABEL_INSET[0],
                size[1] + 2.0 * LABEL_INSET[1],
            ];
            let over = matches!(side, Side::Near { over: true });
            let rect = match (axis, side) {
                (0, Side::Near { over: false }) => Rect::new(
                    at - size[0] / 2.0,
                    image.y - LABEL_GAP - size[1],
                    size[0],
                    size[1],
                ),
                (0, Side::Far) => Rect::new(
                    at - size[0] / 2.0,
                    image.bottom() + LABEL_GAP,
                    size[0],
                    size[1],
                ),
                (0, Side::Near { over: true }) => {
                    Rect::new(at - size[0] / 2.0, area.y + LABEL_GAP, size[0], size[1])
                }
                (_, Side::Near { over: false }) => Rect::new(
                    image.x - LABEL_GAP - size[0],
                    at - size[1] / 2.0,
                    size[0],
                    size[1],
                ),
                (_, Side::Far) => Rect::new(
                    image.right() + LABEL_GAP,
                    at - size[1] / 2.0,
                    size[0],
                    size[1],
                ),
                (_, Side::Near { over: true }) => {
                    Rect::new(area.x + LABEL_GAP, at - size[1] / 2.0, size[0], size[1])
                }
            };
            let clear_of = |hole: Rect| rect.intersect(hole).is_none();
            let fits = inside(rect, content)
                && clear.minimap.is_none_or(clear_of)
                && clear.glass.is_none_or(|(center, radius)| {
                    clear_of(Rect::new(
                        center[0] - radius,
                        center[1] - radius,
                        2.0 * radius,
                        2.0 * radius,
                    ))
                })
                && placed.iter().all(|other| {
                    rect.inset(-LABEL_GAP, -LABEL_GAP)
                        .intersect(other.rect)
                        .is_none()
                });
            if fits {
                placed.push(Label { text, rect, over });
            }
        }
    }
    placed
}

/// Whether the whole of `inner` is inside `outer`.
fn inside(inner: Rect, outer: Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.right() <= outer.right()
        && inner.bottom() <= outer.bottom()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::density;

    /// The lines across a picture measured in its pixels whose near edge is
    /// at `origin` and far edge at `end`, one pixel to the logical pixel, a
    /// line every `spacing`, seen from `from` to `to`.
    fn lines(origin: f32, spacing: f64, end: f32, from: f32, to: f32) -> Vec<f32> {
        Along {
            ruler: &Ruler::PIXELS,
            axis: 0,
            step: spacing,
            origin,
            per: 1.0,
            extent: f64::from(end - origin),
        }
        .lines(from, to)
    }

    /// Ground a raster stands on: 30 meters to its pixel, its first corner
    /// at (2655015, 1110000), its rows running south.
    fn ground() -> Ruler {
        Ruler::test(
            [2_655_015.0, 1_110_000.0],
            [30.0, -30.0],
            Unit::Ground(Some("m")),
        )
    }

    /// The ground's lines fall on its round coordinates rather than a
    /// round distance in from the raster's edge, so that two rasters on the
    /// same ground are ruled alike.
    #[test]
    fn the_grounds_lines_fall_on_its_round_coordinates() {
        let ruler = ground();
        let across = Along {
            ruler: &ruler,
            axis: 0,
            step: 1000.0,
            origin: 0.0,
            per: 1.0,
            extent: 100.0,
        };
        // Eastings 2656000 to 2658000, 985 meters in and every thousand on.
        let at = across.lines(0.0, 100.0);
        assert_eq!(at.len(), 3);
        for (index, at) in at.iter().enumerate() {
            let meters = 985.0 + 1000.0 * index as f32;
            assert!((at - meters / 30.0).abs() < 1e-3, "{at:?}");
        }
        // Down, the northing falls as the rows go on, and the lines still
        // come out in order down the screen.
        let down = Along { axis: 1, ..across };
        let at = down.lines(0.0, 100.0);
        assert!(at.windows(2).all(|pair| pair[0] < pair[1]), "{at:?}");
        assert!((at[0] - 1000.0 / 30.0).abs() < 1e-3, "{at:?}");
    }

    /// A raster cut on a round coordinate has that coordinate at its edge,
    /// which is not a line, however it comes back from the arithmetic.
    #[test]
    fn a_round_edge_is_not_a_line() {
        let ruler = Ruler::test(
            [2_655_000.0, 1_110_000.0],
            [0.1, -0.1],
            Unit::Ground(Some("m")),
        );
        let across = Along {
            ruler: &ruler,
            axis: 0,
            step: 10.0,
            origin: 0.0,
            per: 1.0,
            extent: 300.0,
        };
        assert_eq!(across.lines(0.0, 300.0), [100.0, 200.0]);
    }

    /// A print's spacing is a round number of its own unit, finer than a
    /// pixel if it comes to that, and written in it.
    #[test]
    fn a_prints_spacing_is_a_round_length() {
        let inches = Ruler::print([300.0, 300.0], density::Unit::Inches);
        // 300 to the inch at 1:1, fifty pixels apart is a sixth of an inch:
        // the nearest round length is two tenths.
        let spacing = step(&inches, 1.0, 1.0);
        assert!((spacing - 0.2).abs() < 1e-12, "{spacing}");
        assert_eq!(label(spacing, &inches), "0.2 in");
        let mm = Ruler::print([300.0, 300.0], density::Unit::Millimeters);
        let spacing = step(&mm, 1.0, 1.0);
        assert!((spacing - 5.0).abs() < 1e-12, "{spacing}");
        assert_eq!(label(spacing, &mm), "5 mm");
        // Zoomed far in, a tenth of a millimeter is still a line.
        assert!(step(&mm, 64.0, 1.0) < 0.1);
    }

    /// Meters go up to kilometers on the toggle where they are as many; an
    /// angle is written against its number, and a unit the file never
    /// named is no unit.
    #[test]
    fn the_grounds_spacing_is_written_as_a_map_writes_it() {
        assert_eq!(label(500.0, &ground()), "500 m");
        assert_eq!(label(2000.0, &ground()), "2 km");
        let degrees = Ruler::test([5.9, 47.8], [0.001, -0.001], Unit::Ground(Some("\u{00b0}")));
        assert_eq!(label(0.05, &degrees), "0.05\u{00b0}");
        let unnamed = Ruler::test([0.0, 0.0], [2.0, -2.0], Unit::Ground(None));
        assert_eq!(label(100.0, &unnamed), "100");
    }

    /// Pixels that are not square on the ground are spaced by the axis the
    /// lines fall closer together on.
    #[test]
    fn the_spacing_follows_the_denser_axis() {
        let ruler = Ruler::test([0.0, 0.0], [10.0, -40.0], Unit::Ground(Some("m")));
        // Fifty pixels down is 2000 meters, across only 500.
        assert_eq!(step(&ruler, 1.0, 1.0), 2000.0);
    }

    /// The spacing is always a ruler's: a power of ten, or twice or five
    /// times one.
    fn is_round(step: f64) -> bool {
        let decade = 10f64.powf(step.log10().floor());
        let mantissa = step / decade;
        STEPS.iter().any(|&s| (mantissa - s).abs() < 1e-3)
    }

    #[test]
    fn the_spacing_is_a_round_number_of_image_pixels() {
        assert_eq!(step(&Ruler::PIXELS, 1.0, 1.0), 50.0);
        assert_eq!(step(&Ruler::PIXELS, 0.5, 1.0), 100.0);
        assert_eq!(step(&Ruler::PIXELS, 2.0, 1.0), 20.0);
        assert_eq!(step(&Ruler::PIXELS, 0.1, 1.0), 500.0);
        assert_eq!(step(&Ruler::PIXELS, 8.0, 1.0), 5.0);
        // A ruler's marks, whatever the zoom is doing.
        let mut zoom = 0.02;
        while zoom <= 64.0 {
            let step = step(&Ruler::PIXELS, zoom, 1.0);
            assert!(is_round(step), "{step} at {zoom}");
            zoom *= 1.05;
        }
    }

    /// What the spacing is chosen for: lines about fifty logical pixels
    /// apart. The steps are a factor of 2 or 2.5 apart, so the nearest one is
    /// never worse than the square root of that either way.
    #[test]
    fn the_lines_land_near_the_spacing_they_aim_at() {
        let mut zoom = 0.02;
        while zoom <= 64.0 {
            for scale in [1.0, 1.5, 2.0] {
                let on_screen = step(&Ruler::PIXELS, zoom, scale) * f64::from(zoom / scale);
                assert!(
                    (on_screen / TARGET_SPACING) < 2.5f64.sqrt() + 1e-3,
                    "{on_screen} at {zoom}"
                );
                assert!(
                    (TARGET_SPACING / on_screen) < 2.5f64.sqrt() + 1e-3,
                    "{on_screen} at {zoom}"
                );
            }
            zoom *= 1.05;
        }
    }

    /// The grid is laid out in logical pixels, so a display that packs two
    /// physical ones into each takes twice the image to fill the same span.
    #[test]
    fn the_spacing_is_measured_in_logical_pixels() {
        assert_eq!(step(&Ruler::PIXELS, 1.0, 2.0), 100.0);
        assert_eq!(step(&Ruler::PIXELS, 0.5, 2.0), 200.0);
    }

    /// A grid finer than the image's own pixels would divide nothing, so the
    /// spacing stops at one pixel however far the view is magnified.
    #[test]
    fn the_spacing_never_goes_finer_than_a_pixel() {
        let mut zoom = 0.02;
        while zoom <= 64.0 {
            for scale in [1.0, 1.5, 2.0] {
                assert!(
                    step(&Ruler::PIXELS, zoom, scale) >= 1.0,
                    "{zoom} at {scale}"
                );
            }
            zoom *= 1.05;
        }
        // Magnification is where the floor is reached: zoomed in as far as
        // the view goes, the lines are the image's own pixel boundaries.
        assert_eq!(step(&Ruler::PIXELS, 64.0, 1.0), 1.0);
    }

    #[test]
    fn the_lines_divide_the_image_without_framing_it() {
        // A 400-pixel image drawn from 0 to 400, marked every 100.
        assert_eq!(
            lines(0.0, 100.0, 400.0, 0.0, 400.0),
            vec![100.0, 200.0, 300.0]
        );
        // Nothing to divide.
        assert!(lines(0.0, 100.0, 60.0, 0.0, 60.0).is_empty());
    }

    /// Only the visible lines are emitted: an image zoomed until it runs off
    /// every edge must not cost a quad per line of the part nobody can see.
    #[test]
    fn only_the_lines_in_view_are_drawn() {
        let visible = lines(-10_000.0, 10.0, 10_000.0, 0.0, 100.0);
        assert_eq!(visible.len(), 10);
        assert_eq!(visible[0], 0.0);
        assert_eq!(visible[9], 90.0);

        // And the image's far edge stops them, not the window's.
        let short = lines(0.0, 10.0, 45.0, 0.0, 1000.0);
        assert_eq!(short, vec![10.0, 20.0, 30.0, 40.0]);
    }

    /// The minimap's thumbnail is the image layer's, drawn under the whole
    /// interface, so the grid has to step around it rather than count on
    /// being covered the way it is by the panels.
    #[test]
    fn a_line_across_the_minimap_is_broken_around_it() {
        let hole = Rect::new(20.0, 20.0, 100.0, 80.0);

        // A line straight down the middle of it comes back as the part above
        // and the part below, and nothing in between.
        let line = Rect::new(60.0, 0.0, 1.0, 400.0);
        let pieces: Vec<_> = around(line, line.intersect(hole).expect("crossed"))
            .into_iter()
            .filter(|p| p.width > 0.0 && p.height > 0.0)
            .collect();
        assert_eq!(
            pieces,
            vec![
                Rect::new(60.0, 0.0, 1.0, 20.0),
                Rect::new(60.0, 100.0, 1.0, 300.0)
            ]
        );

        // The pieces of a line across it keep every part of the line the hole
        // does not cover, and none of what it does.
        let line = Rect::new(0.0, 40.0, 400.0, 1.0);
        let pieces: Vec<_> = around(line, line.intersect(hole).expect("crossed"))
            .into_iter()
            .filter(|p| p.width > 0.0 && p.height > 0.0)
            .collect();
        assert!(pieces.iter().all(|p| p.intersect(hole).is_none()));
        let covered: f32 = pieces.iter().map(|p| p.width * p.height).sum();
        assert_eq!(covered, line.width * line.height - hole.width * line.height);

        // And a line clear of it is left whole.
        let line = Rect::new(300.0, 0.0, 1.0, 400.0);
        assert!(line.intersect(hole).is_none());
    }

    /// A line across the loupe's glass is broken over the chord it makes
    /// of the circle, and a line beside the circle is left whole; with the
    /// minimap in the way as well, both holes come out of it.
    #[test]
    fn a_line_across_the_glass_is_broken_around_its_chord() {
        let center = [100.0, 100.0];
        let radius = 50.0;

        // Down the middle: the chord is the whole diameter.
        let line = Rect::new(99.5, 0.0, 1.0, 400.0);
        assert_eq!(
            chord(line, center, radius),
            Some(Rect::new(99.5, 50.0, 1.0, 100.0))
        );
        // Thirty across, the chord is eighty tall: a 3-4-5 triangle.
        let line = Rect::new(129.5, 0.0, 1.0, 400.0);
        assert_eq!(
            chord(line, center, radius),
            Some(Rect::new(129.5, 60.0, 1.0, 80.0))
        );
        // Along the other axis the same way.
        let line = Rect::new(0.0, 59.5, 400.0, 1.0);
        assert_eq!(
            chord(line, center, radius),
            Some(Rect::new(70.0, 59.5, 60.0, 1.0))
        );
        // Past the circle, nothing to cut.
        assert_eq!(
            chord(Rect::new(160.0, 0.0, 1.0, 400.0), center, radius),
            None
        );
        assert_eq!(
            chord(Rect::new(0.0, 150.0, 400.0, 1.0), center, radius),
            None
        );

        // Both holes out of one line: the minimap's and the chord's, and
        // what is left touches neither.
        let line = Rect::new(99.5, 0.0, 1.0, 400.0);
        let minimap = Rect::new(20.0, 300.0, 100.0, 80.0);
        let holes = [Some(minimap), chord(line, center, radius)];
        let left = pieces(line, &holes);
        assert_eq!(
            left,
            vec![
                Rect::new(99.5, 0.0, 1.0, 50.0),
                Rect::new(99.5, 150.0, 1.0, 150.0),
                Rect::new(99.5, 380.0, 1.0, 20.0),
            ]
        );
        assert!(left.iter().all(|piece| piece.intersect(minimap).is_none()));
    }

    /// A line is labeled as often as its words leave room for, and the
    /// lines labeled are a round number of the unit apart: on a grid ruled
    /// in fives, by fives, tens or fifties, never by fifteens.
    #[test]
    fn the_lines_labeled_are_a_round_number_apart() {
        assert_eq!(labeled_every(5.0, 50.0, 40.0), 1);
        assert_eq!(labeled_every(5.0, 50.0, 60.0), 2);
        assert_eq!(labeled_every(5.0, 50.0, 120.0), 4);
        assert_eq!(labeled_every(2.0, 50.0, 60.0), 5);
        assert_eq!(labeled_every(1.0, 50.0, 60.0), 2);
        assert_eq!(labeled_every(0.2, 50.0, 60.0), 5);
        assert_eq!(labeled_every(1000.0, 30.0, 70.0), 5);
    }

    /// A label is the bare number, to the places the labels' own spacing
    /// needs, in kilometers where the toggle writes kilometers.
    #[test]
    fn a_label_is_the_bare_number() {
        let inches = Ruler::print([300.0, 300.0], density::Unit::Inches);
        assert_eq!(reading(0.6000000000000001, 0.2, 0.2, &inches), "0.6");
        assert_eq!(reading(1.0, 0.1, 0.1, &inches), "1.0");
        assert_eq!(reading(300.0, 100.0, 100.0, &Ruler::PIXELS), "300");
        assert_eq!(reading(2_656_000.0, 1000.0, 1000.0, &ground()), "2656");
        assert_eq!(reading(2_656_500.0, 500.0, 500.0, &ground()), "2656500");
        assert_eq!(written(-0.0, 1.0), "0");
    }

    /// What a test's words measure: seven pixels a character, ten high.
    fn measured(text: &str) -> [f32; 2] {
        [7.0 * text.chars().count() as f32, 10.0]
    }

    /// A picture `image` on screen at one logical pixel to its pixel,
    /// measured in its pixels and ruled every `step`.
    fn alongs(image: Rect, step: f64) -> (Along<'static>, Along<'static>) {
        let across = Along {
            ruler: &Ruler::PIXELS,
            axis: 0,
            step,
            origin: image.x,
            per: 1.0,
            extent: f64::from(image.width),
        };
        let down = Along {
            axis: 1,
            origin: image.y,
            extent: f64::from(image.height),
            ..across
        };
        (across, down)
    }

    /// With room around the picture, the labels sit just outside it, above
    /// it and to its left, each on its line; filling the window, they go
    /// over it, inside the window's top and left edges.
    #[test]
    fn the_labels_sit_beside_the_picture_where_there_is_room() {
        let content = Rect::new(0.0, 0.0, 800.0, 600.0);
        let image = Rect::new(100.0, 100.0, 400.0, 300.0);
        let (across, down) = alongs(image, 100.0);
        let placed = labels(across, down, image, content, Clear::default(), measured);
        let texts: Vec<&str> = placed.iter().map(|label| label.text.as_str()).collect();
        assert_eq!(texts, ["100", "200", "100", "200", "300"]);
        for label in &placed {
            assert!(!label.over, "{label:?}");
            assert!(label.rect.intersect(image).is_none(), "{label:?}");
        }
        // The first two are down the left, centered on their lines.
        assert_eq!(placed[0].rect.right(), image.x - LABEL_GAP);
        assert_eq!(placed[0].rect.y + placed[0].rect.height / 2.0, 200.0);
        assert_eq!(placed[2].rect.bottom(), image.y - LABEL_GAP);
        assert_eq!(placed[2].rect.x + placed[2].rect.width / 2.0, 200.0);

        let image = Rect::new(-50.0, -50.0, 900.0, 700.0);
        let (across, down) = alongs(image, 100.0);
        let placed = labels(across, down, image, content, Clear::default(), measured);
        assert!(!placed.is_empty());
        for label in &placed {
            assert!(label.over, "{label:?}");
            assert!(inside(label.rect, content), "{label:?}");
        }
    }

    /// Panned so that the room is on the far side, the labels go there,
    /// below the picture and to its right.
    #[test]
    fn the_labels_go_where_the_room_is() {
        let content = Rect::new(0.0, 0.0, 800.0, 600.0);
        let image = Rect::new(-100.0, -100.0, 600.0, 500.0);
        let (across, down) = alongs(image, 100.0);
        let placed = labels(across, down, image, content, Clear::default(), measured);
        assert!(!placed.is_empty());
        for label in &placed {
            assert!(!label.over, "{label:?}");
            let below = label.rect.y >= image.bottom();
            let right = label.rect.x >= image.right();
            assert!(below || right, "{label:?}");
        }
    }

    /// Lines closer together than their words are labeled every so many,
    /// and nothing is written over the minimap or over another label.
    #[test]
    fn crowded_labels_give_way() {
        let content = Rect::new(0.0, 0.0, 800.0, 600.0);
        let image = Rect::new(-50.0, -50.0, 900.0, 700.0);
        let (across, down) = alongs(image, 10.0);
        let minimap = Rect::new(600.0, 0.0, 200.0, 150.0);
        let clear = Clear {
            minimap: Some(minimap),
            glass: None,
        };
        let placed = labels(across, down, image, content, clear, measured);
        for (index, label) in placed.iter().enumerate() {
            assert!(label.rect.intersect(minimap).is_none(), "{label:?}");
            for other in &placed[index + 1..] {
                assert!(label.rect.intersect(other.rect).is_none(), "{label:?}");
            }
        }
        // Three-digit labels 21 wide want 45 between them: every fifth
        // line of ten across, every other one down.
        let across: Vec<&str> = placed
            .iter()
            .filter(|label| label.rect.height == 12.0 && label.rect.y < 10.0)
            .map(|label| label.text.as_str())
            .collect();
        assert!(across.contains(&"100"), "{across:?}");
        assert!(!across.contains(&"110"), "{across:?}");
    }

    #[test]
    fn the_spacing_is_written_as_image_pixels() {
        assert_eq!(label(1.0, &Ruler::PIXELS), "1 px");
        assert_eq!(label(5000.0, &Ruler::PIXELS), "5000 px");
    }
}
