//! The grid laid over the image: hairlines on image-pixel boundaries, spaced
//! so that they land about every fifty logical pixels of window.
//!
//! The spacing is not the user's to choose. What a grid is for is reading
//! distances off the image, and a grid whose lines are too far apart to
//! bracket anything — or so close that they are a wash — does not do that at
//! any zoom. So one spacing is picked per frame from the round numbers a
//! ruler is marked in, and the toggle says which one is in force.

use crate::render::{Color, Placement, Rect, UiFrame};
use crate::theme::Theme;

/// What the spacing aims at, in logical pixels. Laid out in logical rather
/// than physical ones so the grid reads the same size on any display, the
/// way the rest of the interface does.
const TARGET_SPACING: f32 = 50.0;

/// The mantissas the spacing is chosen from, a decade at a time: the marks on
/// a ruler.
const STEPS: [f32; 3] = [1.0, 2.0, 5.0];

/// How close together lines may fall, in physical pixels, before the grid is
/// left off entirely. Two of them: a line and the gap beside it, which is the
/// least that still reads as a grid rather than as a film over the image.
const MIN_SPACING: f32 = 2.0;

/// How far apart the lines are, in image pixels, at a zoom of `zoom` physical
/// pixels to the image pixel on a display of `scale`.
///
/// Whichever of 1, 2, 5, 10, 20, 50, … puts them nearest to [`TARGET_SPACING`]
/// apart — nearest by ratio, since that is how a spacing is read. Never finer
/// than the image's own pixel grid, which is as far as dividing an image up
/// can go.
pub(super) fn step(zoom: f32, scale: f32) -> f32 {
    if !zoom.is_finite() || zoom <= 0.0 {
        return 1.0;
    }
    let target = TARGET_SPACING * scale.max(f32::MIN_POSITIVE) / zoom;
    let decade = 10f32.powf(target.log10().floor());
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
    (chosen * decade).max(1.0)
}

/// The ratio from one step to the next: 1 → 2, 2 → 5, 5 → 10.
fn next_step(step: f32) -> f32 {
    match step {
        s if s < 2.0 => 2.0,
        s if s < 5.0 => 2.5,
        _ => 2.0,
    }
}

/// Draws the grid over the image, at `step` image pixels between lines.
///
/// Only where the image actually is, and only where the interface has left it
/// visible: the panels are opaque and are drawn after this, but the image can
/// also be zoomed until it runs off every edge, and the grid marks up an
/// image rather than a window.
///
/// `minimap` is the thumbnail's rectangle when the minimap is on screen. It
/// is left clear: the thumbnail is drawn by the image layer, underneath this
/// whole frame, so unlike the panels it cannot cover a grid line laid across
/// it — and a grid belongs to the image being looked at, not to the map of
/// where in it that is.
pub(super) fn draw(
    frame: &mut UiFrame,
    placement: Placement,
    scale: f32,
    content: Rect,
    minimap: Option<Rect>,
    step: f32,
    theme: &Theme,
) {
    if !placement.zoom.is_finite() || scale <= 0.0 {
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
    let Some(area) = intersect(image, content) else {
        return;
    };
    let spacing = step * placement.zoom / scale;
    if spacing * scale < MIN_SPACING {
        return;
    }

    // One physical pixel wide, on the device's grid: a hairline that lands
    // between pixels is feathered across two of them, and a grid of those
    // reads as a haze rather than as lines.
    let width = 1.0 / scale;
    let snap = |value: f32| (value * scale).round() / scale;

    for x in lines(image.x, spacing, image.right(), area.x, area.right()) {
        fill_around(
            frame,
            Rect::new(snap(x), area.y, width, area.height),
            minimap,
            theme.bar_background,
        );
    }
    for y in lines(image.y, spacing, image.bottom(), area.y, area.bottom()) {
        fill_around(
            frame,
            Rect::new(area.x, snap(y), area.width, width),
            minimap,
            theme.bar_background,
        );
    }
}

/// Fills `rect`, less whatever `hole` covers of it.
fn fill_around(frame: &mut UiFrame, rect: Rect, hole: Option<Rect>, color: Color) {
    let Some(hole) = hole.and_then(|hole| intersect(rect, hole)) else {
        frame.rect(rect, color);
        return;
    };
    for piece in around(rect, hole) {
        if piece.width > 0.0 && piece.height > 0.0 {
            frame.rect(piece, color);
        }
    }
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

/// Where the lines fall along one axis, in logical pixels: multiples of
/// `spacing` from the image's near edge at `origin`, that lie inside the
/// image — which ends at `end` — and inside the visible `from..to`.
///
/// The image's own edges are not lines: the grid divides the image up, and a
/// line along the edge would draw a frame around it instead.
fn lines(origin: f32, spacing: f32, end: f32, from: f32, to: f32) -> Vec<f32> {
    let mut out = Vec::new();
    if !spacing.is_finite() || spacing <= 0.0 {
        return out;
    }
    let first = ((from - origin) / spacing).ceil().max(1.0);
    if !first.is_finite() {
        return out;
    }
    let last = end.min(to);
    let mut index = first as i64;
    loop {
        let at = origin + index as f32 * spacing;
        if at >= last {
            return out;
        }
        out.push(at);
        index += 1;
    }
}

/// The rectangle two of them have in common, and `None` when that is nothing.
fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = a.right().min(b.right());
    let bottom = a.bottom().min(b.bottom());
    (right > x && bottom > y).then(|| Rect::new(x, y, right - x, bottom - y))
}

/// How the spacing is written in the toggle: the number of image pixels
/// between lines.
pub(super) fn label(step: f32) -> String {
    format!("{step:.0} px")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The spacing is always a ruler's: a power of ten, or twice or five
    /// times one.
    fn is_round(step: f32) -> bool {
        let decade = 10f32.powf(step.log10().floor());
        let mantissa = step / decade;
        STEPS.iter().any(|&s| (mantissa - s).abs() < 1e-3)
    }

    #[test]
    fn the_spacing_is_a_round_number_of_image_pixels() {
        assert_eq!(step(1.0, 1.0), 50.0);
        assert_eq!(step(0.5, 1.0), 100.0);
        assert_eq!(step(2.0, 1.0), 20.0);
        assert_eq!(step(0.1, 1.0), 500.0);
        assert_eq!(step(8.0, 1.0), 5.0);
        // A ruler's marks, whatever the zoom is doing.
        let mut zoom = 0.02;
        while zoom <= 64.0 {
            let step = step(zoom, 1.0);
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
                let on_screen = step(zoom, scale) * zoom / scale;
                assert!(
                    (on_screen / TARGET_SPACING) < 2.5f32.sqrt() + 1e-3,
                    "{on_screen} at {zoom}"
                );
                assert!(
                    (TARGET_SPACING / on_screen) < 2.5f32.sqrt() + 1e-3,
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
        assert_eq!(step(1.0, 2.0), 100.0);
        assert_eq!(step(0.5, 2.0), 200.0);
    }

    /// A grid finer than the image's own pixels would divide nothing, so the
    /// spacing stops at one pixel however far the view is magnified.
    #[test]
    fn the_spacing_never_goes_finer_than_a_pixel() {
        let mut zoom = 0.02;
        while zoom <= 64.0 {
            for scale in [1.0, 1.5, 2.0] {
                assert!(step(zoom, scale) >= 1.0, "{zoom} at {scale}");
            }
            zoom *= 1.05;
        }
        // Magnification is where the floor is reached: zoomed in as far as
        // the view goes, the lines are the image's own pixel boundaries.
        assert_eq!(step(64.0, 1.0), 1.0);
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
        let pieces: Vec<_> = around(line, intersect(line, hole).expect("crossed"))
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
        let pieces: Vec<_> = around(line, intersect(line, hole).expect("crossed"))
            .into_iter()
            .filter(|p| p.width > 0.0 && p.height > 0.0)
            .collect();
        assert!(pieces.iter().all(|p| intersect(*p, hole).is_none()));
        let covered: f32 = pieces.iter().map(|p| p.width * p.height).sum();
        assert_eq!(covered, line.width * line.height - hole.width * line.height);

        // And a line clear of it is left whole.
        let line = Rect::new(300.0, 0.0, 1.0, 400.0);
        assert!(intersect(line, hole).is_none());
    }

    #[test]
    fn the_spacing_is_written_as_image_pixels() {
        assert_eq!(label(1.0), "1 px");
        assert_eq!(label(5000.0), "5000 px");
    }
}
