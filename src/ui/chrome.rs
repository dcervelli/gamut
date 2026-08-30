//! The window chrome: four panels, the toggles sitting in them, and what
//! they leave in the middle for the image.

use crate::render::{Corner, Popup, Rect};
use crate::view::Viewport;

use super::{Menu, PADDING, Widget};

/// Height of the top and bottom panels.
pub(crate) const BAR_HEIGHT: f32 = 30.0;

/// Width of the left and right panels. Wide enough for a square button and
/// nothing else, which is the point: they hold tools, not content.
pub(crate) const SIDE_WIDTH: f32 = 50.0;

/// The square buttons that live in the side panels.
pub(super) const BUTTON_SIZE: f32 = 34.0;
/// The gap between two buttons stacked down the same panel.
const BUTTON_GAP: f32 = 8.0;
/// The zoom readout at the right of the bottom bar, which is also the button
/// that opens the zoom menu. Wide enough for the longest reading it takes.
pub(super) const ZOOM_BUTTON: [f32; 2] = [58.0, 22.0];

/// Width of the hairline along a panel's inner edge, in logical pixels. What
/// it is drawn in is the theme's `border`.
const BORDER_WIDTH: f32 = 1.0;

/// The window chrome: four panels, and the widgets sitting in them.
///
/// Top and bottom span the full width; left and right are nested between
/// them, so the corners belong to the horizontal bars and the vertical ones
/// never have to reason about where a bar ends.
///
/// Laid out from the window size alone, so the frame builder and the click
/// handler agree on where everything is without either owning it.
#[derive(Clone, Copy)]
pub struct Chrome {
    pub top: Rect,
    pub bottom: Rect,
    pub left: Rect,
    pub right: Rect,
    /// The minimap toggle, at the top of the left panel.
    pub minimap_button: Rect,
    /// The info toggle, at the top of the right panel, and the histogram
    /// toggle under it.
    pub info_button: Rect,
    pub histogram_button: Rect,
    /// The zoom readout, at the right of the bottom bar. Fixed width rather
    /// than fitted to what it says, so that it neither moves as the zoom
    /// changes nor has to be measured to be pressed.
    pub zoom_button: Rect,
}

impl Chrome {
    /// `size` is the window in logical pixels.
    pub fn new(size: [f32; 2]) -> Self {
        // Half the window each at the very smallest, so that a window dragged
        // down to nothing shrinks the panels rather than letting the opposite
        // pair pass through each other.
        let bar = BAR_HEIGHT.min(size[1] / 2.0);
        let side = SIDE_WIDTH.min(size[0] / 2.0);
        let middle = (size[1] - 2.0 * bar).max(0.0);

        let left = Rect::new(0.0, bar, side, middle);
        let right = Rect::new(size[0] - side, bar, side, middle);
        let bottom = Rect::new(0.0, size[1] - bar, size[0], bar);

        Self {
            top: Rect::new(0.0, 0.0, size[0], bar),
            minimap_button: side_button(left, 0),
            info_button: side_button(right, 0),
            histogram_button: side_button(right, 1),
            zoom_button: bar_button(bottom, ZOOM_BUTTON),
            bottom,
            left,
            right,
        }
    }

    /// What the four panels leave in the middle: the image is drawn in it,
    /// and anything that floats over the image — the histogram, for now — has
    /// to fit in it.
    pub fn content(&self) -> Rect {
        Rect::new(
            self.left.right(),
            self.top.bottom(),
            (self.right.x - self.left.right()).max(0.0),
            (self.bottom.y - self.top.bottom()).max(0.0),
        )
    }

    /// The four panels, for anything that treats them alike.
    pub fn panels(&self) -> [Rect; 4] {
        [self.top, self.bottom, self.left, self.right]
    }

    /// The hairline along each panel's inner edge: the bottom of the top
    /// panel, the right of the left one, and so on.
    ///
    /// Inside the panel rather than beside it, so that adding the line does
    /// not move the edge the image is fitted against.
    pub fn borders(&self) -> [Rect; 4] {
        let width = BORDER_WIDTH.min(self.top.height).min(self.left.width);
        [
            Rect::new(self.top.x, self.top.bottom() - width, self.top.width, width),
            Rect::new(self.bottom.x, self.bottom.y, self.bottom.width, width),
            Rect::new(
                self.left.right() - width,
                self.left.y,
                width,
                self.left.height,
            ),
            Rect::new(self.right.x, self.right.y, width, self.right.height),
        ]
    }

    /// Which of the widgets fixed to the panels a point lands on, if any.
    /// The cells of an open menu float above these and are tested first, by
    /// the application.
    pub fn widget_at(&self, point: [f32; 2]) -> Option<Widget> {
        if self.minimap_button.contains(point) {
            Some(Widget::Minimap)
        } else if self.info_button.contains(point) {
            Some(Widget::Info)
        } else if self.histogram_button.contains(point) {
            Some(Widget::Histogram)
        } else if self.zoom_button.contains(point) {
            Some(Widget::Zoom)
        } else {
            None
        }
    }

    /// Where `menu` goes when it is open: the lower right of the content
    /// area, over the image and just above the button that opens it.
    ///
    /// `None` when the window has no room for the whole grid, which is also
    /// what keeps the menu from being opened at all in a window that small.
    pub fn popup(&self, menu: Menu) -> Option<Popup> {
        Popup::new(
            menu.items(),
            menu.grid(),
            self.content(),
            Corner::BottomRight,
        )
    }

    /// Whether a click at `point` belongs to the interface rather than to the
    /// image behind it.
    pub fn contains(&self, point: [f32; 2]) -> bool {
        self.top.contains(point)
            || self.bottom.contains(point)
            || self.left.contains(point)
            || self.right.contains(point)
    }
}

/// The `index`-th square button down a side panel, counting from the top.
/// The inset that centres it across the strip is also the gap above the first
/// one, so a column of buttons reads as set into the panel rather than as
/// merely fitted to it.
///
/// Empty when the panel is too short for that many buttons — a window dragged
/// down small loses them from the bottom up. Neither drawn nor pressable
/// then: both go through the rectangle, and an empty one contains nothing.
fn side_button(panel: Rect, index: usize) -> Rect {
    let size = BUTTON_SIZE.min(panel.width);
    let inset = (panel.width - size) / 2.0;
    let top = inset + index as f32 * (size + BUTTON_GAP);
    if top + size > panel.height {
        return Rect::new(panel.x, panel.y, 0.0, 0.0);
    }
    Rect::new(panel.x + inset, panel.y + top, size, size)
}

/// A button at the right-hand end of a bar, centred across it. Clamped to the
/// bar, so a window dragged narrow shrinks the button rather than pushing it
/// out of the window.
fn bar_button(bar: Rect, size: [f32; 2]) -> Rect {
    let width = size[0].min(bar.width);
    let height = size[1].min(bar.height);
    Rect::new(
        (bar.right() - PADDING - width).max(bar.x),
        bar.y + (bar.height - height) / 2.0,
        width,
        height,
    )
}

/// What the interface leaves for the image, in logical pixels: the middle
/// when the panels are showing, the whole window when they are not.
///
/// With the panels hidden a floating panel still sits in the corner of the
/// window rather than where the panels that are not there would have put it.
/// The frame builder and the minimap's placement both lay out against this,
/// which is what keeps the thumbnail under the border drawn around it.
pub fn content_area(logical: [f32; 2], show_ui: bool) -> Rect {
    if show_ui {
        Chrome::new(logical).content()
    } else {
        Rect::new(0.0, 0.0, logical[0], logical[1])
    }
}

/// Where the image is drawn, in physical pixels, for a window of `size`
/// physical pixels at `scale`.
///
/// The panels are opaque, so with them on screen the image belongs in what
/// they leave in the middle; with them off it has the window. Nothing caches
/// this, which is why toggling the interface re-fits a fitted image on the
/// very next frame.
pub fn image_viewport(size: [f32; 2], scale: f32, show_ui: bool) -> Viewport {
    if !show_ui {
        return Viewport::whole(size);
    }
    let content = Chrome::new([size[0] / scale, size[1] / scale]).content();
    Viewport::new(
        content.x * scale,
        content.y * scale,
        content.width * scale,
        content.height * scale,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::View;

    const WINDOW: [f32; 2] = [1000.0, 700.0];

    #[test]
    fn the_side_panels_are_nested_between_the_bars() {
        let chrome = Chrome::new(WINDOW);

        // The bars own the full width, and so the corners.
        assert_eq!(chrome.top, Rect::new(0.0, 0.0, 1000.0, BAR_HEIGHT));
        assert_eq!(
            chrome.bottom,
            Rect::new(0.0, 700.0 - BAR_HEIGHT, 1000.0, BAR_HEIGHT)
        );

        // The sides start where the top ends and stop where the bottom begins.
        assert_eq!(chrome.left.y, chrome.top.bottom());
        assert_eq!(chrome.left.bottom(), chrome.bottom.y);
        assert_eq!(chrome.right.y, chrome.top.bottom());
        assert_eq!(chrome.right.bottom(), chrome.bottom.y);

        assert_eq!(chrome.left.x, 0.0);
        assert_eq!(chrome.left.width, SIDE_WIDTH);
        assert_eq!(chrome.right.right(), 1000.0);
        assert_eq!(chrome.right.width, SIDE_WIDTH);
    }

    #[test]
    fn the_content_area_is_what_the_four_leave_behind() {
        let content = Chrome::new(WINDOW).content();
        assert_eq!(
            content,
            Rect::new(
                SIDE_WIDTH,
                BAR_HEIGHT,
                1000.0 - 2.0 * SIDE_WIDTH,
                700.0 - 2.0 * BAR_HEIGHT
            )
        );
    }

    #[test]
    fn the_histogram_toggle_sits_inside_the_right_panel() {
        let chrome = Chrome::new(WINDOW);
        let button = chrome.histogram_button;

        assert!(button.x >= chrome.right.x);
        assert!(button.right() <= chrome.right.right());
        assert!(button.y >= chrome.right.y);
        assert!(button.bottom() <= chrome.right.bottom());

        // Centred in the strip rather than merely fitted into it.
        assert_eq!(
            button.x - chrome.right.x,
            chrome.right.right() - button.right()
        );

        assert!(chrome.contains([button.x + 1.0, button.y + 1.0]));
        assert!(!chrome.contains([chrome.right.x - 1.0, button.y + 1.0]));
    }

    #[test]
    fn the_minimap_toggle_sits_inside_the_left_panel() {
        let chrome = Chrome::new(WINDOW);
        let button = chrome.minimap_button;

        assert!(button.x >= chrome.left.x);
        assert!(button.right() <= chrome.left.right());
        assert!(button.y >= chrome.left.y);
        assert!(button.bottom() <= chrome.left.bottom());

        // The minimap toggle and the first of the right-hand pair are the
        // same button on opposite strips, and each click lands on its own.
        assert_eq!(button.width, chrome.info_button.width);
        assert_eq!(button.y, chrome.info_button.y);
        for (widget, rect) in [
            (Widget::Minimap, button),
            (Widget::Info, chrome.info_button),
            (Widget::Histogram, chrome.histogram_button),
        ] {
            assert_eq!(
                chrome.widget_at([rect.x + 1.0, rect.y + 1.0]),
                Some(widget),
                "{widget:?}"
            );
        }
        assert_eq!(chrome.widget_at([WINDOW[0] / 2.0, WINDOW[1] / 2.0]), None);
    }

    /// The right panel holds a column of toggles, and a window too short for
    /// one of them drops it rather than stacking it over its neighbour.
    #[test]
    fn the_side_toggles_stack_down_the_panel_and_stop_when_it_runs_out() {
        let chrome = Chrome::new(WINDOW);
        let (info, histogram) = (chrome.info_button, chrome.histogram_button);

        assert_eq!(info.x, histogram.x);
        assert!(info.bottom() <= histogram.y, "{info:?} over {histogram:?}");
        assert!(histogram.bottom() <= chrome.right.bottom());
        // Set into the panel by the same inset that centres them across it.
        assert_eq!(info.y - chrome.right.y, info.x - chrome.right.x);

        // A window with room for the first and not the second keeps the
        // first, and the second is neither drawn nor pressable.
        let short = Chrome::new([WINDOW[0], 2.0 * BAR_HEIGHT + BUTTON_SIZE + 16.0]);
        assert_eq!(short.info_button.width, BUTTON_SIZE);
        assert_eq!(short.histogram_button.width, 0.0);
        assert_eq!(
            short.widget_at([short.histogram_button.x, short.histogram_button.y]),
            None
        );
    }

    #[test]
    fn a_window_smaller_than_its_own_chrome_stays_within_itself() {
        // Panels are laid out from the window size, so a window dragged down
        // to nothing must not produce rectangles that escape it or run
        // backwards — a negative width would be drawn as a flipped quad.
        for size in [[10.0, 10.0], [0.0, 0.0], [200.0, 20.0]] {
            let chrome = Chrome::new(size);
            for panel in [chrome.top, chrome.bottom, chrome.left, chrome.right] {
                assert!(
                    panel.width >= 0.0 && panel.height >= 0.0,
                    "{panel:?} at {size:?}"
                );
                assert!(panel.x >= 0.0 && panel.y >= 0.0, "{panel:?} at {size:?}");
                assert!(
                    panel.right() <= size[0] + f32::EPSILON,
                    "{panel:?} at {size:?}"
                );
                assert!(
                    panel.bottom() <= size[1] + f32::EPSILON,
                    "{panel:?} at {size:?}"
                );
            }
            for button in [
                chrome.minimap_button,
                chrome.histogram_button,
                chrome.zoom_button,
            ] {
                assert!(
                    button.width >= 0.0 && button.height >= 0.0,
                    "{button:?} at {size:?}"
                );
                assert!(button.x >= 0.0 && button.y >= 0.0, "{button:?} at {size:?}");
            }
            let content = chrome.content();
            assert!(
                content.width >= 0.0 && content.height >= 0.0,
                "{content:?} at {size:?}"
            );
        }
    }

    #[test]
    fn the_zoom_readout_is_a_button_at_the_end_of_the_bottom_bar() {
        let chrome = Chrome::new(WINDOW);
        let button = chrome.zoom_button;

        assert_eq!(button.width, ZOOM_BUTTON[0]);
        assert_eq!(button.right(), chrome.bottom.right() - PADDING);
        // Centred across the bar, and inside it.
        assert_eq!(
            button.y - chrome.bottom.y,
            chrome.bottom.bottom() - button.bottom()
        );
        assert!(button.y >= chrome.bottom.y && button.bottom() <= chrome.bottom.bottom());

        assert_eq!(
            chrome.widget_at([button.x + 1.0, button.y + 1.0]),
            Some(Widget::Zoom)
        );
        // The bar it sits in is still the interface, so a press beside it
        // does not reach the image behind.
        assert_eq!(chrome.widget_at([button.x - 2.0, button.y + 1.0]), None);
        assert!(chrome.contains([button.x - 2.0, button.y + 1.0]));
    }

    /// The panels are opaque, so the image is fitted into what they leave —
    /// and gets the whole window back the moment they are hidden, without
    /// anything having to re-fit it by hand.
    #[test]
    fn the_image_is_fitted_between_the_panels_and_re_fitted_without_them() {
        // A 2x window, to catch a conversion that only holds at scale 1.
        let physical = [2000.0, 1400.0];
        let shown = image_viewport(physical, 2.0, true);
        assert_eq!(
            shown,
            Viewport::new(
                2.0 * SIDE_WIDTH,
                2.0 * BAR_HEIGHT,
                2000.0 - 4.0 * SIDE_WIDTH,
                1400.0 - 4.0 * BAR_HEIGHT,
            )
        );

        let hidden = image_viewport(physical, 2.0, false);
        assert_eq!(hidden, Viewport::whole(physical));

        let view = View::new();
        let image = [900.0, 600.0];
        assert_eq!(view.mode_label(), "fit");
        assert!(view.zoom(image, hidden) > view.zoom(image, shown));

        // Fitted between the panels means fitted *inside* them: the image is
        // centred on the content area, not on the window.
        let placement = view.placement(image, shown);
        assert!(placement.x >= shown.x - 0.5);
        assert!(placement.x + placement.width <= shown.x + shown.width + 0.5);
        assert!(placement.y >= shown.y - 0.5);
        assert!(placement.y + placement.height <= shown.y + shown.height + 0.5);
    }
}
