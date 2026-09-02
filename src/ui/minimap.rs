//! The minimap: a thumbnail of the whole image in the top-left corner, with
//! the part of it on screen picked out and the rest washed over.
//!
//! The thumbnail itself is the image layer's — a second draw of the same
//! texture, placed by [`placement`] — so everything that applies to the image
//! applies to it for nothing. Only the border and the wash are drawn here.

use crate::render::{Placement, Rect, UiFrame, Upscale};
use crate::theme::Theme;
use crate::view::{View, Viewport};

use super::buttons::outline;
use super::chrome::content_area;
use super::{Current, FrameInput, PADDING};

/// The largest the minimap's thumbnail may be. It keeps the image's own
/// shape inside this, so a panorama gets a wide short one and a portrait a
/// narrow tall one.
const MINIMAP_SIZE: [f32; 2] = [168.0, 132.0];

/// Below this on either side there is no room for a map worth reading, and
/// the minimap stays off rather than shrinking to a smudge.
const MINIMAP_MIN: f32 = 48.0;

/// Where the minimap's thumbnail goes, in physical pixels: the whole image,
/// drawn small in the corner the interface will then mark up.
///
/// It is the image layer that draws it, from the same texture as the view
/// itself, so this is a placement like any other and everything that applies
/// to the image — the window, the colormap, the tone map — comes with it for
/// nothing. `None` when the window has no room for a map worth reading.
pub fn placement(
    logical: [f32; 2],
    scale: f32,
    show_ui: bool,
    image: [f32; 2],
    upscale: Upscale,
) -> Option<Placement> {
    let rect = thumbnail(content_area(logical, show_ui), image)?;
    Some(Placement {
        x: rect.x * scale,
        y: rect.y * scale,
        width: rect.width * scale,
        height: rect.height * scale,
        zoom: rect.width * scale / image[0].max(1.0),
        upscale,
    })
}

/// Where the minimap's thumbnail goes: the image's own shape, fitted into the
/// top-left of `content` and never enlarged past life size, since a map of a
/// thirty-pixel image blown up to fill the box would be a map of nothing.
///
/// `None` when there is no room for one worth reading, which is what keeps it
/// off screen in a window dragged down small.
///
/// Public because the pointer is tested against it from outside the frame:
/// the thumbnail is opaque, and what lands on it belongs to it rather than to
/// the picture it is covering.
pub fn thumbnail(content: Rect, image: [f32; 2]) -> Option<Rect> {
    if image[0] <= 0.0 || image[1] <= 0.0 {
        return None;
    }
    // A third of the content area at most, as well as the fixed cap: the
    // minimap is a guide to the image, and must not take the room the image
    // itself is being looked at in.
    let room = [
        MINIMAP_SIZE[0].min(content.width / 3.0),
        MINIMAP_SIZE[1].min(content.height / 3.0),
    ];
    if room[0] < MINIMAP_MIN || room[1] < MINIMAP_MIN {
        return None;
    }
    // Whole logical pixels, so the border sits on the thumbnail's edge rather
    // than half a pixel inside it.
    let scale = (room[0] / image[0]).min(room[1] / image[1]).min(1.0);
    let size = [
        (image[0] * scale).round().max(1.0),
        (image[1] * scale).round().max(1.0),
    ];
    Some(Rect::new(
        (content.x + PADDING).round(),
        (content.y + PADDING).round(),
        size[0],
        size[1],
    ))
}

/// The part of `rect` standing for what the viewport is showing.
///
/// The viewport's corners in image pixels, clamped to the image and scaled
/// into the thumbnail. Clamped because a view zoomed out sees past the
/// image's edges, and this marks out part of the image rather than part of
/// the window.
fn minimap_marker(rect: Rect, image: [f32; 2], placement: Placement, viewport: Viewport) -> Rect {
    let corner = |point: [f32; 2]| {
        let point = placement.image_point(point);
        [
            rect.x + (point[0] / image[0]).clamp(0.0, 1.0) * rect.width,
            rect.y + (point[1] / image[1]).clamp(0.0, 1.0) * rect.height,
        ]
    };
    let start = corner([viewport.x, viewport.y]);
    let end = corner([viewport.x + viewport.width, viewport.y + viewport.height]);
    Rect::new(start[0], start[1], end[0] - start[0], end[1] - start[1])
}

/// `rect` with its edges on whole physical pixels.
///
/// The quad shader feathers every edge over a pixel, which is what keeps the
/// interface's corners and thin lines smooth. Two feathered edges that meet
/// part-way through a pixel each cover part of it, and two translucent fills
/// covering a pixel between them do not add up to one covering all of it: the
/// join stays visible as a lighter line. The wash around the marker is four
/// quads meeting along the marker's edges, so those edges go on the grid and
/// the four pieces tile exactly.
fn snap_to_pixels(rect: Rect, scale: f32) -> Rect {
    let snap = |value: f32| (value * scale).round() / scale;
    let x = snap(rect.x);
    let y = snap(rect.y);
    // A marker smaller than a pixel — the view into a very large image — still
    // has to be somewhere on the map, so an edge never rounds onto the one
    // opposite it.
    let right = snap(rect.right()).max(x + 1.0 / scale);
    let bottom = snap(rect.bottom()).max(y + 1.0 / scale);
    Rect::new(x, y, right - x, bottom - y)
}

/// Draws the minimap over the thumbnail the image layer has already put in
/// the top-left of `content`: a border around the whole image, and the part
/// of it the viewport is showing left bright while the rest is washed over.
///
/// Nothing here is filled where the thumbnail shows through, and the frame
/// this draws into is composited over the image layer, so the two halves of
/// the widget meet on screen without either knowing about the other.
///
/// Only called with part of the image off screen — see `minimap_on_screen` —
/// so the marked-out part is always smaller than the thumbnail on at least
/// one axis, and there is always something to wash over.
pub(super) fn draw(
    frame: &mut UiFrame,
    current: &Current,
    view: &View,
    input: &FrameInput,
    content: Rect,
    theme: &Theme,
) {
    let image = current.size();
    let Some(rect) = thumbnail(content, image) else {
        return;
    };
    outline(frame, rect, 1.0, theme.minimap_edge);

    let placement = view.placement(image, input.viewport);
    let shown = snap_to_pixels(
        minimap_marker(rect, image, placement, input.viewport),
        input.scale,
    );

    for aside in [
        Rect::new(rect.x, rect.y, rect.width, shown.y - rect.y),
        Rect::new(
            rect.x,
            shown.bottom(),
            rect.width,
            rect.bottom() - shown.bottom(),
        ),
        Rect::new(rect.x, shown.y, shown.x - rect.x, shown.height),
        Rect::new(
            shown.right(),
            shown.y,
            rect.right() - shown.right(),
            shown.height,
        ),
    ] {
        if aside.width > 0.0 && aside.height > 0.0 {
            frame.rect(aside, theme.minimap_dim);
        }
    }
    outline(frame, shown, 1.5, theme.accent);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::chrome::Chrome;

    const WINDOW: [f32; 2] = [1000.0, 700.0];

    /// The thumbnail is the image in miniature, so its shape is the image's
    /// and not the box it is fitted into.
    #[test]
    fn the_minimap_keeps_the_image_shape_and_never_enlarges_it() {
        let content = Chrome::new(WINDOW).content();

        let wide = thumbnail(content, [4000.0, 1000.0]).expect("room in a 1000x700 window");
        assert!(wide.width <= MINIMAP_SIZE[0] && wide.height <= MINIMAP_SIZE[1]);
        assert!((wide.width / wide.height - 4.0).abs() < 0.1);

        let tall = thumbnail(content, [1000.0, 4000.0]).expect("room in a 1000x700 window");
        assert!(tall.width <= MINIMAP_SIZE[0] && tall.height <= MINIMAP_SIZE[1]);
        assert!((tall.height / tall.width - 4.0).abs() < 0.1);

        // Life size at most: a tiny image gets a tiny map.
        assert_eq!(
            thumbnail(content, [24.0, 18.0]),
            Some(Rect::new(
                (content.x + PADDING).round(),
                (content.y + PADDING).round(),
                24.0,
                18.0
            ))
        );

        // Top-left of the content area, and clear of its far edges.
        let rect = thumbnail(content, [4000.0, 1000.0]).expect("room");
        assert!(rect.x >= content.x + PADDING - 0.5);
        assert!(rect.y >= content.y + PADDING - 0.5);
        assert!(rect.right() < content.right() && rect.bottom() < content.bottom());

        // And nothing at all when the window has no room to spare: a map
        // taking a third of a small content area would be in the way.
        assert_eq!(
            thumbnail(Chrome::new([200.0, 160.0]).content(), [800.0, 600.0]),
            None
        );
    }

    /// What the marker is for: it says where you are, so it has to agree with
    /// the view it is drawn from.
    #[test]
    fn the_minimap_marker_follows_the_viewport() {
        let image = [800.0, 600.0];
        let viewport = Viewport::whole(WINDOW);
        let rect = Rect::new(100.0, 20.0, 160.0, 120.0);
        let close = |a: f32, b: f32| (a - b).abs() < 0.5;

        // Fitted, the whole image is on screen and the marker covers the map.
        let view = View::new();
        let marker = minimap_marker(rect, image, view.placement(image, viewport), viewport);
        assert_eq!(marker, rect);

        // At 1:1 in a window half the image's size, half of it in each
        // direction is on screen, and centred that is the middle of the map.
        let half = Viewport::whole([400.0, 300.0]);
        let mut view = View::new();
        view.set_zoom(1.0, image, half);
        let marker = minimap_marker(rect, image, view.placement(image, half), half);
        assert!(close(marker.width, rect.width / 2.0), "{marker:?}");
        assert!(close(marker.height, rect.height / 2.0), "{marker:?}");
        assert!(close(
            marker.x + marker.width / 2.0,
            rect.x + rect.width / 2.0
        ));

        // Panned into the top-left corner it goes to the corner of the map,
        // and stops there rather than running off it.
        view.pan_by(-10_000.0, -10_000.0, image, half);
        let marker = minimap_marker(rect, image, view.placement(image, half), half);
        assert!(
            close(marker.x, rect.x) && close(marker.y, rect.y),
            "{marker:?}"
        );
        assert!(marker.right() <= rect.right() + 0.5 && marker.bottom() <= rect.bottom() + 0.5);
    }

    /// The wash around the marker is four quads meeting along its edges, and
    /// feathered edges only tile without a seam where they fall on the device
    /// grid.
    #[test]
    fn the_marker_lands_on_whole_physical_pixels() {
        for scale in [1.0, 1.5, 2.0] {
            let snapped = snap_to_pixels(Rect::new(10.3, 20.7, 40.4, 30.9), scale);
            for edge in [snapped.x, snapped.y, snapped.right(), snapped.bottom()] {
                let physical = edge * scale;
                assert!(
                    (physical - physical.round()).abs() < 1e-3,
                    "{edge} at scale {scale} is not on the grid"
                );
            }
            // Rounded to the nearest pixel rather than grown to cover one.
            assert!((snapped.x - 10.0).abs() <= 1.0 / scale);
        }

        // The view into a very large image marks out less than a pixel of the
        // map, and still has to be somewhere on it.
        let thin = snap_to_pixels(Rect::new(10.1, 20.1, 0.05, 0.05), 2.0);
        assert_eq!(thin.width, 0.5);
        assert_eq!(thin.height, 0.5);
    }
}
