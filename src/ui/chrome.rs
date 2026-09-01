//! The window chrome: four panels, the toggles sitting in them, and what
//! they leave in the middle for the image.

use crate::render::{Popup, Rect};
use crate::view::Viewport;

use super::{Menu, Widget};

/// Height of the top and bottom panels.
pub(crate) const BAR_HEIGHT: f32 = 30.0;

/// Width of the left and right panels. The bars' own height: they hold a
/// column of toggles and nothing else, so their width is a button's, and
/// making the four panels the same thickness leaves the picture centred in a
/// frame of even weight.
pub(crate) const SIDE_WIDTH: f32 = BAR_HEIGHT;

/// The square buttons that live in the side panels.
pub(super) const BUTTON_SIZE: f32 = 22.0;
/// The gap between two buttons, whether stacked down a panel or side by side
/// in a bar.
const BUTTON_GAP: f32 = 8.0;

/// The margin at the ends of the bars: how far the first thing in one is
/// from the edge of the window.
///
/// The inset that centres a toggle across a side panel, and derived from it
/// rather than merely equal to it, so the two cannot drift apart as either is
/// retuned. That makes the button at the end of a bar and the column of
/// toggles below it share one line down the edge of the window — the whole
/// reason the bars are not inset by [`PADDING`](super::PADDING) like the
/// panels that float over the picture.
pub(super) const BAR_PADDING: f32 = (SIDE_WIDTH - BUTTON_SIZE) / 2.0;
/// The zoom readout in the top bar, which is also the button that opens the
/// zoom menu. Wide enough for the longest reading it takes.
pub(super) const ZOOM_BUTTON: [f32; 2] = [58.0, 22.0];
/// The grid toggle at the right of the top bar, switched on: its icon and
/// the widest spacing it reads out, which is "5000 px" at the zoom furthest
/// out on a display that packs two physical pixels into the logical one.
pub(super) const GRID_BUTTON_ON: [f32; 2] = [88.0, 22.0];
/// And switched off, where it is the icon alone: an unlit toggle has no
/// spacing to report, and a button holding the room for one it is not using
/// would be a gap in the bar.
///
/// The side panels' square, so that the only two icon-and-nothing-else
/// buttons in the window are the same size wherever they sit.
pub(super) const GRID_BUTTON_OFF: [f32; 2] = [BUTTON_SIZE, BUTTON_SIZE];

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
    /// The histogram toggle, at the top of the right panel, and the info
    /// toggle under it — the order the two panels they open are stacked in.
    pub histogram_button: Rect,
    pub info_button: Rect,
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

        let top = Rect::new(0.0, 0.0, size[0], bar);
        let left = Rect::new(0.0, bar, side, middle);
        let right = Rect::new(size[0] - side, bar, side, middle);
        let bottom = Rect::new(0.0, size[1] - bar, size[0], bar);

        Self {
            minimap_button: side_button(left, 0),
            histogram_button: side_button(right, 0),
            info_button: side_button(right, 1),
            top,
            bottom,
            left,
            right,
        }
    }

    /// The grid toggle, at the right of the top bar. In the bar rather than
    /// in a side panel because it reads out how far apart the lines are as
    /// well as whether they are drawn, and the side panels are too narrow for
    /// words.
    ///
    /// Wider when the grid is `on`, that reading being what the extra room is
    /// for. Unlike the zoom readout it may move, since the thing it moves for
    /// is the press that was just made on it.
    pub fn grid_button(&self, on: bool) -> Rect {
        let size = if on { GRID_BUTTON_ON } else { GRID_BUTTON_OFF };
        bar_button(self.top, size, self.top.right() - BAR_PADDING)
    }

    /// The zoom readout, in the top bar just inside the grid toggle. Fixed
    /// width rather than fitted to what it says, so that it does not move as
    /// the zoom changes; it does move when the grid toggle beside it does,
    /// `grid_on` being what says where that toggle currently ends.
    ///
    /// In the top bar rather than the bottom one because everything the top
    /// bar says is a measurement of the picture — how many pixels it has, and
    /// now how big they are being drawn.
    pub fn zoom_button(&self, grid_on: bool) -> Rect {
        bar_button(
            self.top,
            ZOOM_BUTTON,
            self.grid_button(grid_on).x - BUTTON_GAP,
        )
    }

    /// The whole window, which is what a popup is placed in: a menu hangs off
    /// the button that opened it and is bounded by the window, not by the
    /// frame the picture is in — a menu pushed around by where the image
    /// happens to be would not stay under its own button.
    fn window(&self) -> Rect {
        Rect::new(0.0, 0.0, self.top.width, self.bottom.bottom())
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
    /// the application. `grid_on` is where the grid toggle currently is, its
    /// width being one of the things its state decides.
    pub fn widget_at(&self, point: [f32; 2], grid_on: bool) -> Option<Widget> {
        if self.minimap_button.contains(point) {
            Some(Widget::Minimap)
        } else if self.histogram_button.contains(point) {
            Some(Widget::Histogram)
        } else if self.info_button.contains(point) {
            Some(Widget::Info)
        } else if self.grid_button(grid_on).contains(point) {
            Some(Widget::Grid)
        } else if self.zoom_button(grid_on).contains(point) {
            Some(Widget::Zoom)
        } else {
            None
        }
    }

    /// Where `menu` goes when it is open: hanging from the button that opens
    /// it, its right edge in line with the button's, over whatever is under
    /// it. `grid_on` is what says where that button is — see
    /// [`Chrome::zoom_button`].
    ///
    /// `None` when the window has no room for the whole grid, which is also
    /// what keeps the menu from being opened at all in a window that small.
    pub fn popup(&self, menu: Menu, grid_on: bool) -> Option<Popup> {
        Popup::below(
            menu.sections(),
            menu.grid(),
            self.zoom_button(grid_on),
            self.window(),
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

/// A button ending at `right` in a bar, centred across it. Clamped to the
/// bar, so a window dragged narrow shrinks the button rather than pushing it
/// out of the window.
fn bar_button(bar: Rect, size: [f32; 2], right: f32) -> Rect {
    let width = size[0].min(bar.width);
    let height = size[1].min(bar.height);
    Rect::new(
        (right - width).max(bar.x),
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
    use crate::view::{Fit, View};

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
        assert_eq!(button.width, chrome.histogram_button.width);
        assert_eq!(button.y, chrome.histogram_button.y);
        for (widget, rect) in [
            (Widget::Minimap, button),
            (Widget::Histogram, chrome.histogram_button),
            (Widget::Info, chrome.info_button),
        ] {
            assert_eq!(
                chrome.widget_at([rect.x + 1.0, rect.y + 1.0], false),
                Some(widget),
                "{widget:?}"
            );
        }
        assert_eq!(
            chrome.widget_at([WINDOW[0] / 2.0, WINDOW[1] / 2.0], false),
            None
        );
    }

    /// The right panel holds a column of toggles, in the order the panels
    /// they open are stacked in: the histogram at the top and the information
    /// column under it. A window too short for one of them drops it rather
    /// than stacking it over its neighbour.
    #[test]
    fn the_side_toggles_stack_down_the_panel_and_stop_when_it_runs_out() {
        let chrome = Chrome::new(WINDOW);
        let (histogram, info) = (chrome.histogram_button, chrome.info_button);

        assert_eq!(histogram.x, info.x);
        assert!(histogram.bottom() <= info.y, "{histogram:?} over {info:?}");
        assert!(info.bottom() <= chrome.right.bottom());
        // Set into the panel by the same inset that centres them across it.
        assert_eq!(histogram.y - chrome.right.y, histogram.x - chrome.right.x);

        // A window with room for the first and not the second keeps the
        // first, and the second is neither drawn nor pressable.
        let short = Chrome::new([WINDOW[0], 2.0 * BAR_HEIGHT + BUTTON_SIZE + 16.0]);
        assert_eq!(short.histogram_button.width, BUTTON_SIZE);
        assert_eq!(short.info_button.width, 0.0);
        assert_eq!(
            short.widget_at([short.info_button.x, short.info_button.y], false),
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
                chrome.info_button,
                chrome.histogram_button,
                chrome.grid_button(true),
                chrome.grid_button(false),
                chrome.zoom_button(true),
                chrome.zoom_button(false),
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

    /// The top bar's own button, laid out like the bottom bar's: at the end
    /// of the bar, so that the two ends of the window read the same way. It
    /// grows leftwards when the grid comes on, the end it is anchored to
    /// staying put.
    #[test]
    fn the_grid_toggle_is_a_button_at_the_end_of_the_top_bar() {
        let chrome = Chrome::new(WINDOW);
        let button = chrome.grid_button(true);
        let unlit = chrome.grid_button(false);

        assert_eq!(button.width, GRID_BUTTON_ON[0]);
        assert_eq!(unlit.width, GRID_BUTTON_OFF[0]);
        assert!(unlit.width < button.width);
        for button in [button, unlit] {
            assert_eq!(button.right(), chrome.top.right() - BAR_PADDING);
            assert_eq!(
                button.y - chrome.top.y,
                chrome.top.bottom() - button.bottom()
            );
            assert!(button.y >= chrome.top.y && button.bottom() <= chrome.top.bottom());
        }

        // Whichever it is, it takes the press that lands on it, and only the
        // width that matches the state it is in: a point inside the wide
        // button and outside the narrow one is the grid switched on and
        // nothing at all switched off.
        let wide_only = [unlit.x - 2.0, button.y + 1.0];
        assert_eq!(chrome.widget_at(wide_only, true), Some(Widget::Grid));
        assert_eq!(chrome.widget_at(wide_only, false), None);
        assert_eq!(
            chrome.widget_at([unlit.x + 1.0, unlit.y + 1.0], false),
            Some(Widget::Grid)
        );
        // The bar it sits in is still the interface, so the facts written
        // beside it are not a press on anything.
        assert_eq!(
            chrome.widget_at([button.x - 2.0, button.y + 1.0], true),
            None
        );
        assert!(chrome.contains([button.x - 2.0, button.y + 1.0]));
    }

    /// The bars and the side panels share one line down each edge of the
    /// window: the button at the end of a bar ends where the column of
    /// toggles below it ends, and the words at the other end start where the
    /// toggle on that side starts. Both fall out of the bars' margin being
    /// the inset that centres a toggle across a panel, so neither can drift
    /// as the button size or the panel width is retuned.
    #[test]
    fn the_bars_end_on_the_same_lines_as_the_side_toggles() {
        for size in [WINDOW, [640.0, 480.0], [2000.0, 1400.0]] {
            let chrome = Chrome::new(size);
            for grid_on in [false, true] {
                assert_eq!(
                    chrome.grid_button(grid_on).right(),
                    chrome.histogram_button.right(),
                    "{size:?} grid_on={grid_on}"
                );
            }
            assert_eq!(chrome.histogram_button.right(), chrome.info_button.right());
            // The left is the same line the other way round: the file name
            // starts where the minimap toggle does.
            assert_eq!(BAR_PADDING, chrome.minimap_button.x - chrome.left.x);
            assert_eq!(
                BAR_PADDING,
                chrome.top.right() - chrome.grid_button(false).right()
            );
        }
    }

    /// The zoom readout shares the top bar with the grid toggle, just inside
    /// it: the two buttons are the measurements of the picture, and both are
    /// at the end the facts about it are written towards.
    #[test]
    fn the_zoom_readout_sits_inside_the_grid_toggle_in_the_top_bar() {
        let chrome = Chrome::new(WINDOW);

        for grid_on in [false, true] {
            let button = chrome.zoom_button(grid_on);
            let grid = chrome.grid_button(grid_on);

            assert_eq!(button.width, ZOOM_BUTTON[0]);
            assert_eq!(button.right(), grid.x - BUTTON_GAP);
            // Centred across the bar, and inside it.
            assert_eq!(
                button.y - chrome.top.y,
                chrome.top.bottom() - button.bottom()
            );
            assert!(button.y >= chrome.top.y && button.bottom() <= chrome.top.bottom());
            assert_eq!(button.y, grid.y);

            // Each of the two takes only the press that lands on itself.
            assert_eq!(
                chrome.widget_at([button.x + 1.0, button.y + 1.0], grid_on),
                Some(Widget::Zoom)
            );
            assert_eq!(
                chrome.widget_at([grid.x + 1.0, grid.y + 1.0], grid_on),
                Some(Widget::Grid)
            );
            // The bar they sit in is still the interface, so a press between
            // them does not reach the image behind.
            assert_eq!(
                chrome.widget_at([button.x - 2.0, button.y + 1.0], grid_on),
                None
            );
            assert!(chrome.contains([button.x - 2.0, button.y + 1.0]));
        }

        // Lighting the grid widens its button, which pushes the readout along
        // with it rather than letting the two overlap.
        assert!(chrome.zoom_button(true).x < chrome.zoom_button(false).x);
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
        assert_eq!(view.fit(), Some(Fit::Whole));
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
