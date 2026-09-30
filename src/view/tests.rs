use super::*;

const IMAGE: [f32; 2] = [900.0, 600.0];
const WINDOW: Viewport = Viewport::whole([1200.0, 1200.0]);

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn opens_fitted_and_centered() {
    let view = View::new();
    assert_eq!(view.fit(), Some(Fit::Whole));

    // Width is the tighter constraint, so the image spans the window.
    let placement = view.placement(IMAGE, WINDOW);
    assert!(close(placement.zoom, 1200.0 / 900.0));
    assert!(close(placement.x, 0.0));
    assert!(close(placement.width, 1200.0));
    assert!(close(placement.height, 800.0));
    assert!(close(placement.y, 200.0));
}

/// The readout in the bar is this mapping run backwards from the pointer,
/// so it has to invert `placement` exactly — including the half-pixel the
/// centering leaves when the image does not fill the window.
#[test]
fn a_window_point_maps_back_to_the_image_pixel_under_it() {
    let view = View::new();
    let placement = view.placement(IMAGE, WINDOW);

    // The corners of the drawn image are the corners of the image.
    let top_left = placement.image_point([placement.x, placement.y]);
    assert!(close(top_left[0], 0.0) && close(top_left[1], 0.0));
    let bottom_right = placement.image_point([
        placement.x + placement.width,
        placement.y + placement.height,
    ]);
    assert!(close(bottom_right[0], IMAGE[0]) && close(bottom_right[1], IMAGE[1]));

    // And the middle of the window is the middle of a centered image.
    let center = placement.image_point([600.0, 600.0]);
    assert!(close(center[0], IMAGE[0] / 2.0) && close(center[1], IMAGE[1] / 2.0));
}

/// Zoomed in and panned, the point under the pointer is wherever the pan
/// has put it, not where the fitted view had it.
#[test]
fn the_mapping_follows_zoom_and_pan() {
    // A viewport smaller than the image, so there is somewhere to pan to.
    let viewport = Viewport::whole([400.0, 300.0]);
    let mut view = View::new();
    view.set_zoom_at(1.0, viewport.center(), IMAGE, viewport);
    view.pan_by(100.0, 50.0, IMAGE, viewport);

    let placement = view.placement(IMAGE, viewport);
    let point = placement.image_point([200.0, 150.0]);
    // 1:1, so the viewport center sits over the image center plus the pan.
    assert!(close(point[0], IMAGE[0] / 2.0 + 100.0));
    assert!(close(point[1], IMAGE[1] / 2.0 + 50.0));

    // Off the top-left of the image reads negative rather than clamping,
    // which is what lets the bar tell "on the image" from "beside it".
    let outside = placement.image_point([placement.x - 10.0, placement.y - 1.0]);
    assert!(outside[0] < 0.0 && outside[1] < 0.0);
}

#[test]
fn a_viewport_holds_its_own_pixels_only() {
    let viewport = Viewport::new(50.0, 30.0, 100.0, 60.0);
    assert!(viewport.contains([50.0, 30.0]));
    assert!(viewport.contains([149.0, 89.0]));
    // Half-open: the far edge belongs to whatever is next to it.
    assert!(!viewport.contains([150.0, 60.0]));
    assert!(!viewport.contains([100.0, 90.0]));
    assert!(!viewport.contains([49.0, 60.0]));
}

#[test]
fn fit_cycles_through_the_whole_image_a_filled_viewport_and_actual_size() {
    let mut view = View::new();
    assert_eq!(view.fit(), Some(Fit::Whole));
    view.cycle_fit(WINDOW.center(), IMAGE, WINDOW);
    assert_eq!(view.fit(), Some(Fit::Fill));
    view.cycle_fit(WINDOW.center(), IMAGE, WINDOW);
    assert_eq!(view.fit(), None);
    assert!(close(view.zoom(IMAGE, WINDOW), 1.0));
    view.cycle_fit(WINDOW.center(), IMAGE, WINDOW);
    assert_eq!(view.fit(), Some(Fit::Whole));

    // A zoom by hand is where actual size is on the round: the whole
    // image comes next.
    view.zoom_in(WINDOW.center(), IMAGE, WINDOW);
    assert_eq!(view.fit(), None);
    view.cycle_fit(WINDOW.center(), IMAGE, WINDOW);
    assert_eq!(view.fit(), Some(Fit::Whole));
}

/// A toggle goes to its zoom from anywhere else — a fit, or another zoom
/// — and from its zoom back to the whole image.
#[test]
fn a_toggle_goes_to_its_zoom_and_back_to_the_whole_image() {
    let mut view = View::new();
    view.cycle_fit(WINDOW.center(), IMAGE, WINDOW);
    assert_eq!(view.fit(), Some(Fit::Fill));
    view.toggle_zoom(1.0, WINDOW.center(), IMAGE, WINDOW);
    assert_eq!(view.fit(), None);
    assert!(close(view.zoom(IMAGE, WINDOW), 1.0));
    view.toggle_zoom(1.0, WINDOW.center(), IMAGE, WINDOW);
    assert_eq!(view.fit(), Some(Fit::Whole));

    view.set_zoom_at(4.0, WINDOW.center(), IMAGE, WINDOW);
    view.toggle_zoom(1.0, WINDOW.center(), IMAGE, WINDOW);
    assert!(close(view.zoom(IMAGE, WINDOW), 1.0));
}

/// A region fitted is a zoom of its own: the region spans the window the
/// way the whole image would, its middle in the middle, and the view is
/// no longer in fit mode.
#[test]
fn a_region_is_fitted_and_centered_like_an_image() {
    let mut view = View::new();
    // The middle third of the image.
    let region = [300.0, 200.0, 300.0, 200.0];
    view.fit_region(Fit::Whole, region, IMAGE, WINDOW);
    assert_eq!(view.fit(), None);
    assert!(close(view.zoom(IMAGE, WINDOW), 1200.0 / 300.0));
    let placement = view.placement(IMAGE, WINDOW);
    let corner = placement.screen_point([300.0, 200.0]);
    assert!(close(corner[0], 0.0), "{corner:?}");
    assert!(close(corner[1], 200.0), "{corner:?}");
    let middle = placement.image_point([600.0, 600.0]);
    assert!(
        close(middle[0], 450.0) && close(middle[1], 300.0),
        "{middle:?}"
    );

    // Filled, the region's short side spans the window instead.
    view.fit_region(Fit::Fill, region, IMAGE, WINDOW);
    assert!(close(view.zoom(IMAGE, WINDOW), 1200.0 / 200.0));

    // A region of a few pixels stops at the zoom limit.
    view.fit_region(Fit::Whole, [10.0, 10.0, 2.0, 2.0], IMAGE, WINDOW);
    assert!(close(view.zoom(IMAGE, WINDOW), MAX_ZOOM));

    // The view stays inside the image: a region in the corner is held
    // against the edge rather than centered off it.
    let viewport = Viewport::whole([400.0, 300.0]);
    view.fit_region(Fit::Whole, [0.0, 0.0, 100.0, 100.0], IMAGE, viewport);
    let placement = view.placement(IMAGE, viewport);
    assert!(placement.x >= -0.5 && placement.y >= -0.5, "{placement:?}");
}

/// The two fits are the two axes of the image, each taken once: 900x600
/// in a square window is held by its width to be seen whole, and by its
/// height to fill the window.
#[test]
fn the_two_fits_take_an_axis_each() {
    let mut view = View::new();
    assert!(close(view.zoom(IMAGE, WINDOW), 1200.0 / 900.0));
    view.cycle_fit(WINDOW.center(), IMAGE, WINDOW);
    assert!(close(view.zoom(IMAGE, WINDOW), 1200.0 / 600.0));
}

/// Which axis each fit lands on, which is what the menu draws the fill
/// with: never the same one, and both of them turning over together when
/// the window becomes the other way round to the image.
#[test]
fn the_axis_a_fit_lands_on_follows_the_shape_of_the_window() {
    let wide = Viewport::whole([2400.0, 600.0]);
    for window in [WINDOW, wide] {
        assert_ne!(
            Fit::Whole.axis(IMAGE, window),
            Fit::Fill.axis(IMAGE, window)
        );
    }
    // 900x600 in a square window is wider than the room it has, so it is
    // held across the window to be seen whole and down it to fill it; in
    // a window wider still than the image, the other way about.
    assert_eq!(Fit::Whole.axis(IMAGE, WINDOW), Axis::Across);
    assert_eq!(Fit::Fill.axis(IMAGE, WINDOW), Axis::Down);
    assert_eq!(Fit::Whole.axis(IMAGE, wide), Axis::Down);
    assert_eq!(Fit::Fill.axis(IMAGE, wide), Axis::Across);
}

#[test]
fn zooming_continues_from_what_is_on_screen() {
    let mut view = View::new();
    let fitted = view.zoom(IMAGE, WINDOW);
    view.zoom_in(WINDOW.center(), IMAGE, WINDOW);
    assert_eq!(view.fit(), None);
    assert!(close(view.zoom(IMAGE, WINDOW), fitted * ZOOM_STEP));

    view.zoom_out(WINDOW.center(), IMAGE, WINDOW);
    assert!(close(view.zoom(IMAGE, WINDOW), fitted));
}

#[test]
fn actual_size_is_one_to_one() {
    let mut view = View::new();
    view.set_zoom_at(1.0, WINDOW.center(), IMAGE, WINDOW);
    assert_eq!(view.fit(), None);
    let placement = view.placement(IMAGE, WINDOW);
    assert!(close(placement.zoom, 1.0));
    assert!(close(placement.width, IMAGE[0]));
    assert!(close(placement.height, IMAGE[1]));
}

#[test]
fn zoom_is_clamped() {
    let mut view = View::new();
    for _ in 0..200 {
        view.zoom_in(WINDOW.center(), IMAGE, WINDOW);
    }
    assert!(close(view.zoom(IMAGE, WINDOW), MAX_ZOOM));
    for _ in 0..400 {
        view.zoom_out(WINDOW.center(), IMAGE, WINDOW);
    }
    assert!(close(view.zoom(IMAGE, WINDOW), MIN_ZOOM));
}

#[test]
fn an_image_smaller_than_the_window_stays_centered() {
    let mut view = View::new();
    view.set_zoom_at(1.0, WINDOW.center(), IMAGE, WINDOW);
    view.pan_by(500.0, 500.0, IMAGE, WINDOW);
    let placement = view.placement(IMAGE, WINDOW);
    assert!(close(placement.x, (WINDOW.width - IMAGE[0]) / 2.0));
    assert!(close(placement.y, (WINDOW.height - IMAGE[1]) / 2.0));
}

#[test]
fn panning_stops_at_the_image_edge() {
    let mut view = View::new();
    view.set_zoom_at(1.0, WINDOW.center(), IMAGE, WINDOW);
    // 4x zoom makes the image 3600x2400, larger than the window on both axes.
    view.zoom = 4.0;

    view.pan_by(100_000.0, 100_000.0, IMAGE, WINDOW);
    let placement = view.placement(IMAGE, WINDOW);
    // Panning right to the limit puts the image's right edge on the window's.
    assert!(close(placement.x + placement.width, WINDOW.width));
    assert!(close(placement.y + placement.height, WINDOW.height));

    view.pan_by(-100_000.0, -100_000.0, IMAGE, WINDOW);
    let placement = view.placement(IMAGE, WINDOW);
    assert!(close(placement.x, 0.0));
    assert!(close(placement.y, 0.0));
}

/// The far side in one press, and only on the axis asked for: the other
/// keeps whatever pan it had.
#[test]
fn panning_to_the_edge_goes_as_far_as_there_is() {
    let mut view = View::new();
    view.set_zoom_at(1.0, WINDOW.center(), IMAGE, WINDOW);
    // 4x zoom makes the image 3600x2400, larger than the window on both axes.
    view.zoom = 4.0;

    view.pan_to_edge([1.0, 0.0], IMAGE, WINDOW);
    let placement = view.placement(IMAGE, WINDOW);
    assert!(close(placement.x + placement.width, WINDOW.width));
    // Vertically untouched, so still centered.
    assert!(close(
        placement.y + placement.height / 2.0,
        WINDOW.height / 2.0
    ));

    view.pan_to_edge([0.0, -1.0], IMAGE, WINDOW);
    let placement = view.placement(IMAGE, WINDOW);
    assert!(close(placement.x + placement.width, WINDOW.width));
    assert!(close(placement.y, 0.0));
}

/// Centering on a point puts that point in the middle of the window,
/// until the edge of the image gets there first.
#[test]
fn centering_on_a_point_goes_as_far_as_the_edge_lets_it() {
    let mut view = View::new();
    view.set_zoom_at(4.0, WINDOW.center(), IMAGE, WINDOW);
    let middle = [WINDOW.width / 2.0, WINDOW.height / 2.0];

    // 4x zoom makes the image 3600x2400: a point well inside lands in
    // the middle of the window, at the same zoom.
    view.center_on([300.0, 200.0], IMAGE, WINDOW);
    let placement = view.placement(IMAGE, WINDOW);
    let shown = placement.image_point(middle);
    assert!(
        close(shown[0], 300.0) && close(shown[1], 200.0),
        "{shown:?}"
    );
    assert!(close(placement.zoom, 4.0));

    // A point by the corner stops with the image's edges at the
    // window's, as a drag there would.
    view.center_on([10.0, 590.0], IMAGE, WINDOW);
    let placement = view.placement(IMAGE, WINDOW);
    assert!(close(placement.x, 0.0), "{placement:?}");
    assert!(
        close(placement.y + placement.height, WINDOW.height),
        "{placement:?}"
    );

    // Fitted, there is nowhere to go: the image stays centered.
    let mut fitted = View::new();
    fitted.center_on([10.0, 10.0], IMAGE, WINDOW);
    let placement = fitted.placement(IMAGE, WINDOW);
    assert!(close(
        placement.x + placement.width / 2.0,
        WINDOW.width / 2.0
    ));
}

/// An image with nothing to pan stays where it is rather than being
/// shoved against a side of the window.
#[test]
fn panning_to_the_edge_of_a_fitted_image_does_nothing() {
    let mut view = View::new();
    view.pan_to_edge([-1.0, 1.0], IMAGE, WINDOW);
    let placement = view.placement(IMAGE, WINDOW);
    assert!(close(
        placement.x + placement.width / 2.0,
        WINDOW.width / 2.0
    ));
    assert!(close(
        placement.y + placement.height / 2.0,
        WINDOW.height / 2.0
    ));
}

#[test]
fn a_filled_viewport_still_pans_vertically() {
    let mut view = View::new();
    view.cycle_fit(WINDOW.center(), IMAGE, WINDOW);
    assert_eq!(view.fit(), Some(Fit::Fill));
    // 900x600 filling a 1200x600 window is 1200x800: taller than the
    // window, so there is room to scroll down but not sideways.
    let window = Viewport::whole([1200.0, 600.0]);
    view.pan_by(400.0, 400.0, IMAGE, window);
    assert_eq!(view.fit(), Some(Fit::Fill));
    let placement = view.placement(IMAGE, window);
    assert!(close(placement.x, 0.0));
    assert!(close(placement.y + placement.height, window.height));
}

/// What a wheel zoom is for: the pixel under the pointer is still under it
/// afterwards, so zooming in on a detail does not also require panning back
/// to it.
#[test]
fn a_wheel_zoom_keeps_the_point_under_the_pointer() {
    let mut view = View::new();
    view.set_zoom_at(1.0, WINDOW.center(), IMAGE, WINDOW);
    // 4x makes the image larger than the window, so there is pan to give.
    view.zoom = 4.0;

    let anchor = [300.0, 900.0];
    let under = |view: &View| {
        let placement = view.placement(IMAGE, WINDOW);
        [
            (anchor[0] - placement.x) / placement.zoom,
            (anchor[1] - placement.y) / placement.zoom,
        ]
    };

    let before = under(&view);
    view.zoom_steps_at(1.0, anchor, IMAGE, WINDOW);
    assert!(view.zoom(IMAGE, WINDOW) > 4.0);
    let after = under(&view);
    assert!(close(before[0], after[0]), "{before:?} -> {after:?}");
    assert!(close(before[1], after[1]), "{before:?} -> {after:?}");
}

/// A zoom asked for by name — a number, a step — keeps the point under
/// its anchor too, as the wheel does, however far the zoom goes at once.
#[test]
fn a_named_zoom_keeps_the_point_under_its_anchor() {
    let mut view = View::new();
    view.set_zoom_at(4.0, WINDOW.center(), IMAGE, WINDOW);

    let anchor = [300.0, 900.0];
    let under = |view: &View| {
        let placement = view.placement(IMAGE, WINDOW);
        [
            (anchor[0] - placement.x) / placement.zoom,
            (anchor[1] - placement.y) / placement.zoom,
        ]
    };

    let before = under(&view);
    view.set_zoom_at(16.0, anchor, IMAGE, WINDOW);
    assert!(close(view.zoom(IMAGE, WINDOW), 16.0));
    assert_eq!(view.fit(), None);
    let after = under(&view);
    assert!(close(before[0], after[0]), "{before:?} -> {after:?}");
    assert!(close(before[1], after[1]), "{before:?} -> {after:?}");

    // Out of a fit the same way: the point is the one the fit had there.
    let mut view = View::new();
    let before = under(&view);
    view.set_zoom_at(4.0, anchor, IMAGE, WINDOW);
    let after = under(&view);
    assert!(close(before[0], after[0]), "{before:?} -> {after:?}");
    assert!(close(before[1], after[1]), "{before:?} -> {after:?}");
}

/// The image sits in what the panels leave, not in the window, so
/// everything measured against the viewport has to allow for where it
/// begins — a wheel zoom's anchor included.
#[test]
fn an_offset_viewport_carries_the_image_and_the_anchor_with_it() {
    // Narrow enough that the fit is below 1:1, where placement is not
    // rounded to whole pixels and the centering can be checked exactly.
    let inset = Viewport::new(50.0, 30.0, 450.0, 1140.0);

    // Fitted on width, so it spans the viewport and is centered in it —
    // rather than spanning the window it is a hole in.
    let placement = View::new().placement(IMAGE, inset);
    assert!(close(placement.x, inset.x));
    assert!(close(placement.width, inset.width));
    assert!(close(
        placement.y + placement.height / 2.0,
        inset.y + inset.height / 2.0
    ));

    let mut view = View::new();
    view.set_zoom_at(1.0, inset.center(), IMAGE, inset);
    // 4x makes the image larger than the viewport, so there is pan to give.
    view.zoom = 4.0;

    let anchor = [300.0, 900.0];
    let under = |view: &View| {
        let placement = view.placement(IMAGE, inset);
        [
            (anchor[0] - placement.x) / placement.zoom,
            (anchor[1] - placement.y) / placement.zoom,
        ]
    };

    let before = under(&view);
    view.zoom_steps_at(1.0, anchor, IMAGE, inset);
    assert!(close(view.zoom(IMAGE, inset), 5.0));
    let after = under(&view);
    // Placement lands on whole pixels above 1:1, so the point can move by
    // half an output pixel at each of the two zooms and no further. Which
    // is the same half pixel the anchor would drift by in a window-sized
    // viewport: the offset itself adds nothing.
    let tolerance = 0.5 / 4.0 + 0.5 / 5.0;
    assert!(
        (before[0] - after[0]).abs() <= tolerance,
        "{before:?} -> {after:?}"
    );
    assert!(
        (before[1] - after[1]).abs() <= tolerance,
        "{before:?} -> {after:?}"
    );
}

/// A trackpad sends fractions of a notch rather than whole ones.
#[test]
fn a_wheel_zoom_takes_fractional_steps() {
    let mut view = View::new();
    view.set_zoom_at(1.0, WINDOW.center(), IMAGE, WINDOW);
    view.zoom_steps_at(0.5, [600.0, 600.0], IMAGE, WINDOW);
    assert!(close(view.zoom(IMAGE, WINDOW), ZOOM_STEP.powf(0.5)));
}

#[test]
fn a_wheel_zoom_stops_at_the_same_limits_as_the_keyboard() {
    let mut view = View::new();
    view.zoom_steps_at(-200.0, [0.0, 0.0], IMAGE, WINDOW);
    assert!(close(view.zoom(IMAGE, WINDOW), MIN_ZOOM));
    view.zoom_steps_at(400.0, [0.0, 0.0], IMAGE, WINDOW);
    assert!(close(view.zoom(IMAGE, WINDOW), MAX_ZOOM));
}

/// The filter is a preference about how to read images, not part of where
/// this one is scrolled to, so stepping to the next file keeps it.
#[test]
fn the_upscale_filter_outlives_the_image() {
    let mut view = View::new();
    assert_eq!(view.upscale(), Upscale::Nearest);
    view.cycle_upscale();
    assert_eq!(view.upscale(), Upscale::Bicubic);

    view.zoom_in(WINDOW.center(), IMAGE, WINDOW);
    view.reset();
    assert_eq!(view.fit(), Some(Fit::Whole));
    assert_eq!(view.upscale(), Upscale::Bicubic);
    assert_eq!(view.placement(IMAGE, WINDOW).upscale, Upscale::Bicubic);
}

/// Antialiased nearest resolves a texel edge that lands mid-pixel, which
/// is right at 4.5:1 and wrong at 1:1 — there every texel edge would land
/// mid-pixel, and the 100% view would come out uniformly soft.
#[test]
fn magnified_views_land_on_whole_pixels() {
    let mut view = View::new();
    // An odd window against an even image is what puts the center on a
    // half pixel.
    let window = Viewport::whole([1201.0, 1201.0]);
    view.set_zoom_at(1.0, window.center(), IMAGE, window);
    let placement = view.placement(IMAGE, window);
    assert_eq!(placement.x, placement.x.round());
    assert_eq!(placement.y, placement.y.round());

    // Below 1:1 it is left alone.
    view.cycle_fit(WINDOW.center(), IMAGE, WINDOW);
    let placement = view.placement([4000.0, 4000.0], window);
    assert!(placement.zoom < 1.0);
    assert!(close(placement.x, 0.0));
}

/// What a grab cursor is offered on: a fitted image has nowhere to go, and
/// a filled viewport leaves only the axis that overflows.
#[test]
fn there_is_nothing_to_pan_while_the_whole_image_is_visible() {
    let mut view = View::new();
    assert!(!view.can_pan(IMAGE, WINDOW));

    view.set_zoom_at(1.0, WINDOW.center(), IMAGE, WINDOW);
    assert!(!view.can_pan(IMAGE, WINDOW));
    view.zoom_in(WINDOW.center(), IMAGE, WINDOW);
    view.zoom_in(WINDOW.center(), IMAGE, WINDOW);
    assert!(view.can_pan(IMAGE, WINDOW));

    // 900x600 filling a 1200x600 window is 1200x800: taller than the
    // window, so the vertical axis has somewhere to go.
    view.reset();
    view.cycle_fit(WINDOW.center(), IMAGE, WINDOW);
    assert!(view.can_pan(IMAGE, Viewport::whole([1200.0, 600.0])));
}

/// The straight line in space-scale coordinates is a straight line on
/// screen for every point of the image, travelled at a steady rate: the
/// point half way along the path is half way between where it began and
/// where it ends. Interpolating pan and zoom separately fails this — the
/// zoom runs ahead of the pan and the point swings out and back.
#[test]
fn a_pan_and_zoom_together_carry_every_point_in_a_straight_line() {
    // Zooms below 1:1, where placement is not rounded to whole pixels
    // and the check can be exact; an image large enough at both to have
    // somewhere to pan to.
    let image = [4000.0, 4000.0];
    let mut from = View::new();
    from.set_zoom_at(0.5, WINDOW.center(), image, WINDOW);
    from.pan_by(150.0, -100.0, image, WINDOW);
    let mut to = View::new();
    to.set_zoom_at(0.9, WINDOW.center(), image, WINDOW);
    to.pan_by(800.0, 350.0, image, WINDOW);
    let (a, b) = (from.position(image, WINDOW), to.position(image, WINDOW));
    assert!(a.v < b.v && a.u != b.u);

    let on_screen = |t: f32, point: [f32; 2]| {
        let placement = from.at(Position::between(a, b, t)).placement(image, WINDOW);
        [
            placement.x + point[0] * placement.zoom,
            placement.y + point[1] * placement.zoom,
        ]
    };
    for point in [
        [0.0, 0.0],
        [2000.0, 2000.0],
        [3500.0, 700.0],
        [4000.0, 4000.0],
    ] {
        let start = on_screen(0.0, point);
        let end = on_screen(1.0, point);
        for t in [0.25, 0.5, 0.75] {
            let along = on_screen(t, point);
            for axis in 0..2 {
                let expected = start[axis] + (end[axis] - start[axis]) * t;
                assert!(
                    close(along[axis], expected),
                    "{point:?} at {t}: {along:?}, expected {expected} on axis {axis}"
                );
            }
        }
    }
}

/// The pan limit bends where the image stops overflowing the viewport,
/// so the line between two settled views can cross it — out of a fit
/// toward a detail, the image overflows one edge while still short of
/// the other. A view on its way is shown where the line puts it, not
/// held within the limit, or the move would pin the picture against the
/// edge and then let it catch up: every point still crosses the screen
/// in a straight line at a steady rate.
#[test]
fn a_view_on_its_way_is_not_held_within_the_pan_limit() {
    // Fitted whole across, with a margin above and below; then zoomed
    // in about a point near the bottom-left corner.
    let from = View::new();
    let anchor = [50.0, 950.0];
    let mut to = from;
    to.set_zoom_at(4.0, anchor, IMAGE, WINDOW);
    let (a, b) = (from.position(IMAGE, WINDOW), to.position(IMAGE, WINDOW));
    assert!(a.u == [0.0, 0.0] && b.u[1] > 0.0);

    let on_screen = |t: f32, point: [f32; 2]| {
        from.at(Position::between(a, b, t))
            .placement(IMAGE, WINDOW)
            .screen_point(point)
    };
    let crossed = (1..10).any(|step| {
        let between = Position::between(a, b, step as f32 / 10.0);
        let limit = View::pan_limit(IMAGE, WINDOW.size(), between.v);
        between.u[1].abs() > limit[1] * between.v + 1e-3
    });
    assert!(crossed, "the line does not cross the limit");
    for point in [[0.0, 0.0], [450.0, 300.0], [900.0, 600.0]] {
        let (start, end) = (on_screen(0.0, point), on_screen(1.0, point));
        for t in [0.25, 0.5, 0.75] {
            let along = on_screen(t, point);
            for axis in 0..2 {
                let expected = start[axis] + (end[axis] - start[axis]) * t;
                // Placement lands on whole pixels above 1:1.
                assert!(
                    (along[axis] - expected).abs() <= 1.0,
                    "{point:?} at {t}: {along:?}, expected {expected} on axis {axis}"
                );
            }
        }
    }
}

/// A zoom to the zoom the view is already at is a look at the anchor:
/// what is under it goes to the middle, as far as the edge lets it.
#[test]
fn a_zoom_to_the_zoom_in_force_centers_the_anchor() {
    let mut view = View::new();
    view.set_zoom_at(4.0, WINDOW.center(), IMAGE, WINDOW);
    let anchor = [300.0, 900.0];
    let detail = view.placement(IMAGE, WINDOW).image_point(anchor);

    view.set_zoom_at(4.0, anchor, IMAGE, WINDOW);
    assert!(close(view.zoom(IMAGE, WINDOW), 4.0));
    let middle = view.placement(IMAGE, WINDOW).image_point(WINDOW.center());
    assert!(
        close(middle[0], detail[0]) && close(middle[1], detail[1]),
        "{middle:?}"
    );

    // A detail by the corner goes as far as a drag would take it, and no
    // further: the edge stops at the window's.
    view.pan_to_edge([-1.0, -1.0], IMAGE, WINDOW);
    view.set_zoom_at(4.0, [10.0, 10.0], IMAGE, WINDOW);
    let placement = view.placement(IMAGE, WINDOW);
    assert!(
        close(placement.x, 0.0) && close(placement.y, 0.0),
        "{placement:?}"
    );
}

/// A wheel zoom keeps the point under the pointer at both ends, and so
/// all the way along: the straight line keeps whatever its ends share.
#[test]
fn a_moving_wheel_zoom_keeps_its_anchor_throughout() {
    let image = [4000.0, 4000.0];
    let mut from = View::new();
    from.set_zoom_at(2.0, WINDOW.center(), image, WINDOW);
    from.pan_by(300.0, -200.0, image, WINDOW);
    let mut to = from;
    let anchor = [900.0, 250.0];
    to.zoom_steps_at(4.0, anchor, image, WINDOW);
    let (a, b) = (from.position(image, WINDOW), to.position(image, WINDOW));

    let under = |t: f32| {
        from.at(Position::between(a, b, t))
            .placement(image, WINDOW)
            .image_point(anchor)
    };
    let start = under(0.0);
    for t in [0.2, 0.5, 0.8, 1.0] {
        let along = under(t);
        // Placement lands on whole pixels above 1:1, so the point can
        // drift by half an output pixel at either zoom and no further.
        let tolerance = 0.5 / a.v + 0.5 / b.v;
        assert!(
            (along[0] - start[0]).abs() <= tolerance,
            "{start:?} -> {along:?} at {t}"
        );
        assert!(
            (along[1] - start[1]).abs() <= tolerance,
            "{start:?} -> {along:?} at {t}"
        );
    }
}

/// A view on its way to a fit is not yet fitted, and the fit it reaches
/// is the same place the fitted view is.
#[test]
fn a_view_at_the_end_of_its_line_is_where_the_settled_view_is() {
    let mut from = View::new();
    from.set_zoom_at(4.0, WINDOW.center(), IMAGE, WINDOW);
    from.pan_by(500.0, 500.0, IMAGE, WINDOW);
    let to = View::new();
    assert_eq!(to.fit(), Some(Fit::Whole));
    let end = from.at(to.position(IMAGE, WINDOW));
    assert_eq!(end.fit(), None);
    let (arrived, settled) = (end.placement(IMAGE, WINDOW), to.placement(IMAGE, WINDOW));
    assert!(close(arrived.x, settled.x) && close(arrived.y, settled.y));
    assert!(close(arrived.zoom, settled.zoom));
}

#[test]
fn reset_returns_to_the_opening_state() {
    let mut view = View::new();
    view.zoom_in(WINDOW.center(), IMAGE, WINDOW);
    view.pan_by(300.0, 300.0, IMAGE, WINDOW);
    view.reset();
    assert_eq!(view.fit(), Some(Fit::Whole));
    let placement = view.placement(IMAGE, WINDOW);
    assert!(close(placement.x, 0.0));
}

/// A turn keeps the detail at the center of the window there, and a
/// turn back puts the view where it was.
#[test]
fn a_turn_keeps_the_same_detail_at_the_center() {
    let viewport = Viewport {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 100.0,
    };
    let image = [400.0, 200.0];
    let mut view = View::new();
    view.fit = None;
    view.zoom = 2.0;
    view.pan = [60.0, -30.0];
    // The detail at the center: 60 right of the picture's center, 30 up.
    // A quarter clockwise brings a point right of center below it and a
    // point above center to its right.
    view.turn(true);
    assert_eq!(view.pan, [30.0, 60.0]);
    let placement = view.placement([image[1], image[0]], viewport);
    let centered = placement.image_point([50.0, 50.0]);
    assert_eq!(centered, [image[1] / 2.0 + 30.0, image[0] / 2.0 + 60.0]);
    view.turn(false);
    assert_eq!(view.pan, [60.0, -30.0]);
}

/// Another rendering of the picture a quarter the size keeps the same
/// detail at the center of the window, at the same size on screen.
#[test]
fn a_rescale_keeps_the_same_detail_at_the_same_size() {
    let viewport = Viewport {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 100.0,
    };
    let large = [4000.0, 2000.0];
    let small = [1000.0, 500.0];
    let mut view = View::new();
    view.fit = None;
    view.zoom = 1.0;
    view.pan = [600.0, -300.0];
    let before = view.placement(large, viewport);
    view.rescale(large, small);
    assert_eq!(view.pan, [150.0, -75.0]);
    assert_eq!(view.zoom, 4.0);
    let after = view.placement(small, viewport);
    let centered = after.image_point([50.0, 50.0]);
    assert_eq!(centered, [small[0] / 2.0 + 150.0, small[1] / 2.0 - 75.0]);
    assert!((after.width - before.width).abs() < 1e-3);
    view.rescale(small, large);
    assert_eq!(view.pan, [600.0, -300.0]);
    assert_eq!(view.zoom, 1.0);
}
