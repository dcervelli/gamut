//! The window chrome: four panels, the toggles sitting in them, and what
//! they leave in the middle for the image.

use crate::render::{Popup, Rect};
use crate::view::Viewport;

use super::{Menu, Widget};

/// Height of the top and bottom panels.
pub(crate) const BAR_HEIGHT: f32 = 30.0;

/// Width of the left and right panels. The bars' own height: they hold a
/// column of toggles and nothing else, so their width is a button's, and
/// making the four panels the same thickness leaves the picture centered in a
/// frame of even weight.
pub(crate) const SIDE_WIDTH: f32 = BAR_HEIGHT;

/// The square buttons that live in the side panels.
pub(super) const BUTTON_SIZE: f32 = 22.0;
/// The gap between two buttons, whether stacked down a panel or side by side
/// in a bar.
const BUTTON_GAP: f32 = 8.0;
/// And the gap between two set against each other instead: a hairline of the
/// bar showing between them, and nothing more.
///
/// The pair that steps through the list is parted by this rather than by
/// [`BUTTON_GAP`], and squared off where they meet — see
/// [`Corners`](super::buttons::Corners). Two buttons that go the two ways of
/// one thing are one control, and a control is not read as one thing with a
/// button's own width of bar down the middle of it.
pub(super) const STEP_SEAM: f32 = 1.0;

/// The margin at the ends of the bars: how far the first thing in one is
/// from the edge of the window.
///
/// The inset that centers a toggle across a side panel, and derived from it
/// rather than merely equal to it, so the two cannot drift apart as either is
/// retuned. That makes the button at the end of a bar and the column of
/// toggles below it share one line down the edge of the window — the whole
/// reason the bars are not inset by [`PADDING`](super::PADDING) like the
/// panels that float over the picture.
pub(super) const BAR_PADDING: f32 = (SIDE_WIDTH - BUTTON_SIZE) / 2.0;
/// The zoom readout in the top bar, which is also the button that opens the
/// zoom menu. Wide enough for the longest reading it takes.
pub(super) const ZOOM_BUTTON: [f32; 2] = [58.0, 22.0];
/// The gap between the spacing the grid toggle reads out and the mark it
/// belongs to. The mark keeps the rest of its square clear around the icon
/// in it, so what shows on screen is wider again than this.
pub(super) const READING_GAP: f32 = 4.0;
/// The room the grid toggle gives one digit of its reading.
const READING_DIGIT: f32 = 8.0;
/// And the room it gives the rest of one: the space and the "px" after the
/// number, and the padding in front of it.
const READING_REST: f32 = 25.0;
/// How wide the grid toggle is with `spacing` read out in it, and the mark's
/// own square with nothing read out — an unlit toggle has no spacing in
/// force, and a button holding the room for one it is not using would be a
/// gap in the bar. That square is the side panels' one, so that every button
/// wearing a mark and nothing else is the same size wherever it sits.
///
/// Counted in digits rather than measured in the face the bar is set in,
/// because the button has to be where the frame drew it when a press lands on
/// it, and the pointer is answered where there are no fonts to ask. The
/// allowance is wider than the interface's own face needs; the reading is set
/// against the mark, so what the allowance leaves over falls in front of the
/// number, where it is the button's padding. `buttons` checks the allowance
/// against the fonts the reading is drawn in.
pub(super) fn grid_width(spacing: Option<&str>) -> f32 {
    let Some(spacing) = spacing else {
        return BUTTON_SIZE;
    };
    let digits = spacing.chars().filter(char::is_ascii_digit).count() as f32;
    BUTTON_SIZE + READING_GAP + READING_REST + digits * READING_DIGIT
}
/// The surface switch at the right of the bottom bar: the one word it wears,
/// with the room a button's label keeps around itself.
pub(super) const OUTPUT_BUTTON: [f32; 2] = [42.0, 22.0];

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
    /// The minimap toggle, at the top of the left panel, the button that
    /// opens the menu of copies under it, and the paste button under that.
    /// The paste button is drawn and pressable only while the clipboard is
    /// holding a picture — see [`Chrome::widget_at`]; it is the last of the
    /// three so that the two above it do not move as it comes and goes.
    pub minimap_button: Rect,
    pub copy_button: Rect,
    pub paste_button: Rect,
    /// The histogram toggle, at the top of the right panel, and the info
    /// toggle under it — the order the two panels they open are stacked in.
    pub histogram_button: Rect,
    pub info_button: Rect,
    /// The button that hides the interface, at the very end of the top bar.
    ///
    /// Last in the bar, so that it is the thing in the corner of the window:
    /// it is the only widget up there that is not a measurement of the
    /// picture, and putting it outside the pair that are keeps the two kinds
    /// apart. Being last is also what puts its mark on the line the column of
    /// toggles below it keeps — see [`Chrome::grid_button`].
    pub maximize_button: Rect,
    /// The button at the head of the pixel readout, at the left of the bottom
    /// bar: it says how a pixel's value is written, and the readout follows
    /// it along the bar.
    ///
    /// Always there, where the readout beside it comes and goes with the
    /// pointer: the pointer is never over a pixel while it is over the bar,
    /// so a button that only appeared with the readout could never be
    /// pressed.
    pub pixel_button: Rect,
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
            // Ending a button's width past the padding is beginning at the
            // padding: the left of the bottom bar, under the column of
            // toggles down the left panel and on the same line as them.
            pixel_button: bar_button(
                bottom,
                [BUTTON_SIZE, BUTTON_SIZE],
                bottom.x + BAR_PADDING + BUTTON_SIZE,
            ),
            maximize_button: bar_button(top, [BUTTON_SIZE, BUTTON_SIZE], top.right() - BAR_PADDING),
            minimap_button: side_button(left, 0),
            copy_button: side_button(left, 1),
            paste_button: side_button(left, 2),
            histogram_button: side_button(right, 0),
            info_button: side_button(right, 1),
            top,
            bottom,
            left,
            right,
        }
    }

    /// The two buttons at the head of the top bar: back a file, and on a
    /// file. In front of the count they move through, at the end of the bar
    /// the file is named from, so that the three read as one line about which
    /// of the list is on screen.
    ///
    /// Set against each other across a [`STEP_SEAM`] and turned only at the
    /// ends of the pair: the two go the two ways of one thing, and are drawn
    /// as one thing.
    ///
    /// On screen only while there is more than one file — see
    /// [`Chrome::widget_at`]. Stepping a list of one does nothing, and a
    /// button that did nothing when pressed would be worse than no button;
    /// the count beside them is left out for the same reason.
    pub fn step_buttons(&self) -> [Rect; 2] {
        let size = [BUTTON_SIZE, BUTTON_SIZE];
        let previous = bar_button(self.top, size, self.top.x + BAR_PADDING + BUTTON_SIZE);
        let next = bar_button(self.top, size, previous.right() + STEP_SEAM + BUTTON_SIZE);
        [previous, next]
    }

    /// Where the top bar's words begin: past the pair of step buttons while
    /// they are on screen, and at the bar's own margin while they are not.
    ///
    /// `steps` is whether there is more than one file, which is what puts
    /// those buttons there. Asked here rather than worked out twice, the
    /// pointer having to be answered against the same line the words were
    /// laid out from.
    pub fn bar_text_x(&self, steps: bool) -> f32 {
        if steps {
            self.step_buttons()[1].right() + super::PADDING
        } else {
            self.top.x + BAR_PADDING
        }
    }

    /// The grid toggle, in the top bar just inside the button that hides the
    /// interface. In the bar rather than in a side panel because it reads out
    /// how far apart the lines are as well as whether they are drawn, and the
    /// side panels are too narrow for words.
    ///
    /// Fitted to the `spacing` it is reading out, and the mark's own square
    /// while it is reading out nothing. Its right edge is anchored and the
    /// rest of it grows leftwards, pushing the zoom readout along the bar in
    /// front of it.
    pub fn grid_button(&self, spacing: Option<&str>) -> Rect {
        let size = [grid_width(spacing), BUTTON_SIZE];
        bar_button(self.top, size, self.maximize_button.x - BUTTON_GAP)
    }

    /// The zoom readout, in the top bar just inside the grid toggle. Fixed
    /// width rather than fitted to what it says, so that it does not move as
    /// the zoom changes what it reads.
    ///
    /// Hung off the toggle beside it at a fixed gap, `spacing` being what says
    /// where that toggle ends: the two are the pair at the end of the bar and
    /// they stay a pair, rather than the readout holding a place of its own
    /// with a gap that opens and closes as the toggle is fitted to its
    /// reading.
    ///
    /// In the top bar rather than the bottom one because everything the top
    /// bar says is a measurement of the picture — how many pixels it has, and
    /// now how big they are being drawn.
    pub fn zoom_button(&self, spacing: Option<&str>) -> Rect {
        bar_button(
            self.top,
            ZOOM_BUTTON,
            self.grid_button(spacing).x - BUTTON_GAP,
        )
    }

    /// The switch between the SDR and the HDR surface, at the right of the
    /// bottom bar — the end the grid toggle holds in the top one, so that
    /// the two bars end on the same line.
    ///
    /// In the bottom bar rather than the top because it is not a fact about
    /// the picture: it is what is being done with it, which is what the
    /// bottom bar is for, and the words about the rest of that run up to it.
    pub fn output_button(&self) -> Rect {
        bar_button(
            self.bottom,
            OUTPUT_BUTTON,
            self.bottom.right() - BAR_PADDING,
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
    /// the application. `spacing` is what the grid toggle is reading out,
    /// which is what says how much of the bar it takes.
    ///
    /// `paste` is whether the clipboard is holding a picture and `steps`
    /// whether the list holds more than one file: the two other things about
    /// the interface the window's size does not settle. Each puts a button on
    /// screen only while it is true, so a point where one would be reaches
    /// the panel and no widget when it is not.
    pub fn widget_at(
        &self,
        point: [f32; 2],
        spacing: Option<&str>,
        paste: bool,
        steps: bool,
    ) -> Option<Widget> {
        let [previous, next] = self.step_buttons();
        if steps && previous.contains(point) {
            Some(Widget::Previous)
        } else if steps && next.contains(point) {
            Some(Widget::Next)
        } else if self.maximize_button.contains(point) {
            Some(Widget::Maximize)
        } else if self.minimap_button.contains(point) {
            Some(Widget::Minimap)
        } else if self.copy_button.contains(point) {
            Some(Widget::Copy)
        } else if paste && self.paste_button.contains(point) {
            Some(Widget::Paste)
        } else if self.histogram_button.contains(point) {
            Some(Widget::Histogram)
        } else if self.info_button.contains(point) {
            Some(Widget::Info)
        } else if self.grid_button(spacing).contains(point) {
            Some(Widget::Grid)
        } else if self.zoom_button(spacing).contains(point) {
            Some(Widget::Zoom)
        } else if self.output_button().contains(point) {
            Some(Widget::Output)
        } else if self.pixel_button.contains(point) {
            Some(Widget::PixelFormat)
        } else {
            None
        }
    }

    /// Where `menu` goes when it is open: hanging from the button that opens
    /// it, over whatever is beside it — down from the zoom readout in the top
    /// bar, up from the pixel button in the bottom one, out from the copy
    /// button in the left panel. `spacing` is what says where the first of
    /// those is — see [`Chrome::zoom_button`].
    ///
    /// `None` when the window has no room for the whole grid, which is also
    /// what keeps the menu from being opened at all in a window that small.
    pub fn popup(&self, menu: Menu, spacing: Option<&str>) -> Option<Popup> {
        match menu {
            Menu::Zoom => Popup::below(
                menu.sections(),
                menu.grid(),
                self.zoom_button(spacing),
                self.window(),
            ),
            // From the bottom bar, so it stands over its button rather than
            // hanging off the foot of the window — see [`Popup::above`].
            Menu::PixelFormat => Popup::above(
                menu.sections(),
                menu.grid(),
                self.pixel_button,
                self.window(),
            ),
            // From a button in the column down the left panel, which has its
            // neighbors above and below it and its room to the side — see
            // [`Popup::beside`].
            Menu::Copy => Popup::beside(
                menu.sections(),
                menu.grid(),
                self.copy_button,
                self.window(),
            ),
        }
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
/// The inset that centers it across the strip is also the gap above the first
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

/// A button ending at `right` in a bar, centered across it. Clamped to the
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

    /// A spacing to lay the bar out with, in the middle of the range of
    /// widths a reading can have.
    const SPACING: Option<&str> = Some("50 px");

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

        // Centered in the strip rather than merely fitted into it.
        assert_eq!(
            button.x - chrome.right.x,
            chrome.right.right() - button.right()
        );

        assert!(chrome.contains([button.x + 1.0, button.y + 1.0]));
        assert!(!chrome.contains([chrome.right.x - 1.0, button.y + 1.0]));
    }

    /// The column down the left panel: the minimap toggle, the button that
    /// opens the menu of copies, and the paste button under those. The copy
    /// button is above the one that comes and goes, so nothing moves under
    /// the pointer as the clipboard changes.
    #[test]
    fn the_copy_button_sits_between_the_minimap_toggle_and_the_paste_button() {
        let chrome = Chrome::new(WINDOW);
        let button = chrome.copy_button;

        assert_eq!(button.x, chrome.minimap_button.x);
        assert_eq!(button.width, chrome.minimap_button.width);
        assert!(button.y >= chrome.minimap_button.bottom());
        assert!(chrome.paste_button.y >= button.bottom());

        // There whether or not there is anything to paste, unlike the button
        // under it.
        for paste in [false, true] {
            assert_eq!(
                chrome.widget_at([button.x + 1.0, button.y + 1.0], None, paste, false),
                Some(Widget::Copy)
            );
        }
    }

    /// The paste button is under the copy button, in the same strip, and
    /// it is there for the pointer only while there is something to paste:
    /// the rectangle is always laid out — the layout is the window's size and
    /// nothing else — and what comes and goes is whether anything answers on
    /// it.
    #[test]
    fn the_paste_button_answers_only_while_there_is_a_paste() {
        let chrome = Chrome::new(WINDOW);
        let button = chrome.paste_button;
        let at = [button.x + 1.0, button.y + 1.0];

        assert!(button.y >= chrome.copy_button.bottom());
        assert!(button.bottom() <= chrome.left.bottom());
        assert_eq!(button.x, chrome.minimap_button.x);
        assert_eq!(button.width, chrome.minimap_button.width);

        assert_eq!(chrome.widget_at(at, None, true, false), Some(Widget::Paste));
        assert_eq!(
            chrome.widget_at(at, None, false, false),
            None,
            "with nothing to paste the press reaches the panel and no widget"
        );
        // And it never stands in front of the toggle above it.
        assert_eq!(
            chrome.widget_at(
                [chrome.minimap_button.x + 1.0, chrome.minimap_button.y + 1.0],
                None,
                true,
                false
            ),
            Some(Widget::Minimap)
        );
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
                chrome.widget_at([rect.x + 1.0, rect.y + 1.0], None, false, false),
                Some(widget),
                "{widget:?}"
            );
        }
        assert_eq!(
            chrome.widget_at([WINDOW[0] / 2.0, WINDOW[1] / 2.0], None, false, false),
            None
        );
    }

    /// The right panel holds a column of toggles, in the order the panels
    /// they open are stacked in: the histogram at the top and the information
    /// column under it. A window too short for one of them drops it rather
    /// than stacking it over its neighbor.
    #[test]
    fn the_side_toggles_stack_down_the_panel_and_stop_when_it_runs_out() {
        let chrome = Chrome::new(WINDOW);
        let (histogram, info) = (chrome.histogram_button, chrome.info_button);

        assert_eq!(histogram.x, info.x);
        assert!(histogram.bottom() <= info.y, "{histogram:?} over {info:?}");
        assert!(info.bottom() <= chrome.right.bottom());
        // Set into the panel by the same inset that centers them across it.
        assert_eq!(histogram.y - chrome.right.y, histogram.x - chrome.right.x);

        // A window with room for the first and not the second keeps the
        // first, and the second is neither drawn nor pressable.
        let short = Chrome::new([WINDOW[0], 2.0 * BAR_HEIGHT + BUTTON_SIZE + 16.0]);
        assert_eq!(short.histogram_button.width, BUTTON_SIZE);
        assert_eq!(short.info_button.width, 0.0);
        assert_eq!(
            short.widget_at(
                [short.info_button.x, short.info_button.y],
                None,
                false,
                false
            ),
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
                chrome.maximize_button,
                chrome.grid_button(SPACING),
                chrome.grid_button(None),
                chrome.zoom_button(SPACING),
                chrome.zoom_button(None),
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

    /// The top bar's own readout-and-toggle, just inside the button that
    /// hides the interface. It grows leftwards when the grid comes on, the
    /// end it is anchored to staying put.
    #[test]
    fn the_grid_toggle_is_a_button_inside_the_end_of_the_top_bar() {
        let chrome = Chrome::new(WINDOW);
        let button = chrome.grid_button(SPACING);
        let unlit = chrome.grid_button(None);

        assert_eq!(button.width, grid_width(SPACING));
        assert_eq!(unlit.width, BUTTON_SIZE);
        assert!(unlit.width < button.width);
        // And it is fitted to the reading rather than held at one width for
        // every reading there is: another digit is another button's worth of
        // number, and takes room for one.
        assert!(chrome.grid_button(Some("500 px")).width > button.width);
        for button in [button, unlit] {
            assert_eq!(button.right(), chrome.maximize_button.x - BUTTON_GAP);
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
        assert_eq!(
            chrome.widget_at(wide_only, SPACING, false, false),
            Some(Widget::Grid)
        );
        assert_eq!(chrome.widget_at(wide_only, None, false, false), None);
        assert_eq!(
            chrome.widget_at([unlit.x + 1.0, unlit.y + 1.0], None, false, false),
            Some(Widget::Grid)
        );
        // The bar it sits in is still the interface, so the facts written
        // beside it are not a press on anything.
        assert_eq!(
            chrome.widget_at([button.x - 2.0, button.y + 1.0], SPACING, false, false),
            None
        );
        assert!(chrome.contains([button.x - 2.0, button.y + 1.0]));
    }

    /// The pair that steps through the list leads the top bar, on the same
    /// line down the window as the toggles under it, and the bar's words
    /// begin past them.
    ///
    /// They answer only while there is a list to step through: a press where
    /// one would be reaches the bar and no widget with a single file, exactly
    /// as the paste button behaves with an empty clipboard.
    #[test]
    fn the_step_buttons_lead_the_top_bar_while_there_is_a_list_to_step() {
        let chrome = Chrome::new(WINDOW);
        let [previous, next] = chrome.step_buttons();

        assert_eq!(previous.x, chrome.top.x + BAR_PADDING);
        // The line the column of toggles down the left panel starts on, and
        // the button at the head of the bottom bar with it.
        assert_eq!(previous.x, chrome.minimap_button.x);
        assert_eq!(previous.x, chrome.pixel_button.x);
        // Set against each other rather than spaced like unrelated buttons:
        // the pair is one control with two ends.
        assert_eq!(next.x - previous.right(), STEP_SEAM);
        for button in [previous, next] {
            assert_eq!(button.width, BUTTON_SIZE);
            // Centered across the bar, as every other button in one is.
            assert_eq!(
                button.y - chrome.top.y,
                chrome.top.bottom() - button.bottom()
            );
        }

        // The words start clear of them while they are there, and at the
        // bar's own margin while they are not.
        assert!(chrome.bar_text_x(true) > next.right());
        assert_eq!(chrome.bar_text_x(false), chrome.top.x + BAR_PADDING);

        for (button, widget) in [(previous, Widget::Previous), (next, Widget::Next)] {
            let at = [button.x + 1.0, button.y + 1.0];
            assert_eq!(chrome.widget_at(at, None, false, true), Some(widget));
            assert_eq!(
                chrome.widget_at(at, None, false, false),
                None,
                "with one file the press reaches the bar and no widget"
            );
        }
    }

    /// The bars and the side panels share one line down each edge of the
    /// window: the button at the end of a bar ends where the column of
    /// toggles below it ends, and the words at the other end start where the
    /// toggle on that side starts. Both fall out of the bars' margin being
    /// the inset that centers a toggle across a panel, so neither can drift
    /// as the button size or the panel width is retuned.
    #[test]
    fn the_bars_end_on_the_same_lines_as_the_side_toggles() {
        for size in [WINDOW, [640.0, 480.0], [2000.0, 1400.0]] {
            let chrome = Chrome::new(size);
            assert_eq!(
                chrome.maximize_button.right(),
                chrome.histogram_button.right(),
                "{size:?}"
            );
            assert_eq!(chrome.histogram_button.right(), chrome.info_button.right());
            // The left is the same line the other way round: the file name
            // starts where the minimap toggle does.
            assert_eq!(BAR_PADDING, chrome.minimap_button.x - chrome.left.x);
            assert_eq!(
                BAR_PADDING,
                chrome.top.right() - chrome.maximize_button.right()
            );
        }
    }

    /// The zoom readout shares the top bar with the grid toggle, just inside
    /// it: the two buttons are the measurements of the picture, and both are
    /// at the end the facts about it are written towards. The gap between
    /// them is the same one every pair of buttons in the window is set at,
    /// whatever the toggle is reading out.
    #[test]
    fn the_zoom_readout_sits_inside_the_grid_toggle_in_the_top_bar() {
        let chrome = Chrome::new(WINDOW);

        for spacing in [None, SPACING, Some("10000 px")] {
            let button = chrome.zoom_button(spacing);
            let grid = chrome.grid_button(spacing);

            assert_eq!(button.width, ZOOM_BUTTON[0]);
            assert_eq!(button.right(), grid.x - BUTTON_GAP);
            // Centered across the bar, and inside it.
            assert_eq!(
                button.y - chrome.top.y,
                chrome.top.bottom() - button.bottom()
            );
            assert!(button.y >= chrome.top.y && button.bottom() <= chrome.top.bottom());
            assert_eq!(button.y, grid.y);

            // Each of the two takes only the press that lands on itself.
            assert_eq!(
                chrome.widget_at([button.x + 1.0, button.y + 1.0], spacing, false, false),
                Some(Widget::Zoom)
            );
            assert_eq!(
                chrome.widget_at([grid.x + 1.0, grid.y + 1.0], spacing, false, false),
                Some(Widget::Grid)
            );
            // The bar they sit in is still the interface, so a press between
            // them does not reach the image behind.
            assert_eq!(
                chrome.widget_at([button.x - 2.0, button.y + 1.0], spacing, false, false),
                None
            );
            assert!(chrome.contains([button.x - 2.0, button.y + 1.0]));
        }

        // Lighting the grid widens its button, which pushes the readout along
        // with it rather than letting the two overlap.
        assert!(chrome.zoom_button(SPACING).x < chrome.zoom_button(None).x);
    }

    /// The panels are opaque, so the image is fitted into what they leave —
    /// and gets the whole window back the moment they are hidden, without
    /// anything having to re-fit it by hand.
    /// The pixel button leads the bottom bar, on the line the column of side
    /// toggles keeps down the left of the window — the two ends of the bars
    /// and the panels between them share one margin — and it answers the
    /// pointer where it is drawn.
    #[test]
    fn the_pixel_button_leads_the_bottom_bar_on_the_side_panels_line() {
        let chrome = Chrome::new(WINDOW);
        let button = chrome.pixel_button;

        assert_eq!(button.x, chrome.minimap_button.x);
        assert_eq!(button.width, BUTTON_SIZE);
        let middle = [
            button.x + button.width / 2.0,
            button.y + button.height / 2.0,
        ];
        assert!(chrome.bottom.contains(middle));
        assert_eq!(
            chrome.widget_at(middle, None, false, false),
            Some(Widget::PixelFormat)
        );
        // And nothing else in that bar is where it is: the surface switch is
        // at the far end of it.
        assert!(button.right() < chrome.output_button().x);

        // A window dragged narrow keeps it inside the bar rather than pushing
        // it out of the window.
        let cramped = Chrome::new([40.0, 200.0]);
        assert!(
            cramped
                .bottom
                .contains([cramped.pixel_button.x + 0.5, cramped.pixel_button.y + 0.5])
        );
    }

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
        // centered on the content area, not on the window.
        let placement = view.placement(image, shown);
        assert!(placement.x >= shown.x - 0.5);
        assert!(placement.x + placement.width <= shown.x + shown.width + 0.5);
        assert!(placement.y >= shown.y - 0.5);
        assert!(placement.y + placement.height <= shown.y + shown.height + 0.5);
    }

    /// The grid toggle wears its mark in the last button's width of itself,
    /// so where the mark lands is decided by the toggle's right edge. That
    /// edge does not move when the toggle lights up and grows leftwards to
    /// make room for its reading — so the mark is in the same place from one
    /// press to the next.
    #[test]
    fn the_grid_toggle_keeps_its_mark_where_it_was_when_it_lights_up() {
        let chrome = Chrome::new(WINDOW);
        let (on, off) = (chrome.grid_button(SPACING), chrome.grid_button(None));
        assert!(
            on.width > off.width,
            "the lit toggle makes room for a reading"
        );
        assert_eq!(on.right(), off.right(), "and grows leftwards to do it");
        assert_eq!(off.width, chrome.histogram_button.width);
    }

    /// The button that hides the interface ends the top bar, and its mark is
    /// the one over the column of toggles down the right of the window: the
    /// last thing in a bar is what shares that line, and it is a side
    /// toggle's own square so the marks are drawn at one size all the way
    /// down. Nothing about the grid moves it — the toggle beside it grows
    /// leftwards, into the bar.
    #[test]
    fn the_button_that_hides_the_interface_ends_the_top_bar() {
        let chrome = Chrome::new(WINDOW);
        let button = chrome.maximize_button;

        assert_eq!(button.width, BUTTON_SIZE);
        assert_eq!(button.height, BUTTON_SIZE);
        assert_eq!(button.right(), chrome.top.right() - BAR_PADDING);
        assert_eq!(button.right(), chrome.histogram_button.right());
        // Centered across the bar, as everything else in one is.
        assert_eq!(
            button.y - chrome.top.y,
            chrome.top.bottom() - button.bottom()
        );

        for spacing in [None, SPACING, Some("10000 px")] {
            assert_eq!(chrome.maximize_button, button, "{spacing:?}");
            assert!(chrome.grid_button(spacing).right() < button.x);
            assert_eq!(
                chrome.widget_at([button.x + 1.0, button.y + 1.0], spacing, false, false),
                Some(Widget::Maximize)
            );
        }
        // And it never stands in front of the toggle beside it.
        let grid = chrome.grid_button(SPACING);
        assert_eq!(
            chrome.widget_at([grid.right() - 1.0, grid.y + 1.0], SPACING, false, false),
            Some(Widget::Grid)
        );
    }
}
