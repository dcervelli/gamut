//! Which layer of the interface the pointer is on.
//!
//! The window is drawn in layers: the picture at the bottom, the panels that
//! float over it, the chrome around it, and whatever menu is open on top of
//! the lot. Every one of them is opaque, so the pointer has one question to
//! ask — which layer is under it — and [`hit`] answers it once for the
//! highlight, the press, the wheel and the bar's pixel readout alike. A press
//! that lands on a panel is spent there whether or not it hit one of the
//! panel's buttons: nothing reaches what is drawn behind a panel, and a
//! gesture aimed at one thing never acts on two.
//!
//! Laid out from the window size and what is on screen, exactly as
//! [`build_frame`](super::build_frame) lays the frame out, so what the
//! pointer reaches is what was drawn under it. Nothing is cached: both work
//! from the same few numbers, and a layout stored between them is a layout
//! that can fall out of step with the window.
//!
//! Being over a layer and being taken by it are not quite the same thing. An
//! open menu takes the pointer wherever it is — a press anywhere off it
//! dismisses it rather than reaching what it landed on, which is what a menu
//! does everywhere — and that grab is applied by the handlers, over the top
//! of the answer here. What the pointer is *over* does not change while a
//! menu is open, which is why the bar goes on reading out the pixel under it.

use super::chrome::{Chrome, content_area};
use super::{Panels, Widget, histogram, info, minimap};

/// The image on screen, as the layers need to know it. `None` before the
/// first decode, when the panels that describe an image are not drawn.
#[derive(Clone, Copy, Debug)]
pub struct Shown {
    /// Its size in pixels, which is what the minimap takes its shape from.
    pub size: [f32; 2],
    /// Whether it is gray, which decides which toggles the histogram panel
    /// puts down its side.
    pub gray: bool,
    /// Whether the minimap is on screen: its toggle is on, and the view has
    /// something for it to point out.
    pub minimap: bool,
}

/// Where the pointer is: the topmost layer that answers for it.
///
/// A variant per layer rather than per widget, so that a handler can say what
/// happens to a press on a panel without enumerating everything on it — and
/// so that the two panels with no buttons at all still take the pointer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hit {
    /// A cell of the menu that is open.
    Cell(usize),
    /// That menu between its cells: a press is spent on it and it stays open.
    Menu,
    /// The information panel. Which of its rows the pointer is on takes the
    /// fonts to answer, so that is asked separately — see
    /// [`info::copyable_at`].
    Info,
    /// The minimap's thumbnail.
    Minimap,
    /// The histogram panel, and which of its own toggles is under the
    /// pointer, if it is over one at all.
    Histogram(Option<Widget>),
    /// One of the four panels of the chrome, and the widget on it.
    Chrome(Option<Widget>),
    /// The picture: nothing in the interface is over this point.
    Image,
}

impl Hit {
    /// The widget the pointer is over, which is both what draws lit and what
    /// a press acts on. `None` on the part of a layer that is not a button,
    /// where a press is spent doing nothing.
    pub fn widget(self) -> Option<Widget> {
        match self {
            Hit::Cell(index) => Some(Widget::Cell(index)),
            Hit::Histogram(widget) | Hit::Chrome(widget) => widget,
            Hit::Menu | Hit::Info | Hit::Minimap | Hit::Image => None,
        }
    }

    /// Whether this is the menu that is open — one of its cells, or its own
    /// body — rather than something behind it.
    pub fn is_menu(self) -> bool {
        matches!(self, Hit::Cell(_) | Hit::Menu)
    }

    /// Whether the pointer is on the picture itself, with nothing over it,
    /// which is what the bar's pixel readout asks.
    pub fn is_image(self) -> bool {
        self == Hit::Image
    }
}

/// Which layer `point` lands on, in a window of `logical` logical pixels.
///
/// Top down, and the first layer to claim the point wins: no layer is asked
/// what is behind it, and none of them has to.
///
/// `spacing` is what the grid toggle is reading out, from
/// [`grid_spacing`](super::grid_spacing): the one thing about the bar that
/// the window's size does not settle, the toggle being fitted to the number
/// in it. The caller works it out the way the frame builder does, from the
/// zoom the frame was drawn at.
pub fn hit(
    point: [f32; 2],
    panels: &Panels,
    logical: [f32; 2],
    shown: Option<Shown>,
    spacing: Option<&str>,
) -> Hit {
    let chrome = Chrome::new(logical);

    // The menu, which is the thing being looked at while it is open and is
    // drawn over everything for that reason. A window too small for the whole
    // grid has no popup to hit, and the press that finds none dismisses the
    // menu it should never have been left with.
    if let Some(menu) = panels.menu
        && let Some(popup) = chrome.popup(menu, spacing)
        && popup.contains(point)
    {
        return match popup.item_at(point) {
            Some(index) => Hit::Cell(index),
            None => Hit::Menu,
        };
    }

    // What floats over the picture, in the reverse of the order it is drawn
    // in, so that where two of them want the same strip of a narrow window
    // the pointer reaches the one on top. Not gated on the bars: these are
    // over the content area, which is the whole window when the bars are
    // hidden, and they stay on screen and pressable without them.
    if let Some(shown) = shown {
        let content = content_area(logical, panels.show_ui);
        if panels.show_info
            && let Some(panel) = info::panel(content, panels.show_histogram)
            && panel.contains(point)
        {
            return Hit::Info;
        }
        if shown.minimap
            && let Some(thumbnail) = minimap::thumbnail(content, shown.size)
            && thumbnail.contains(point)
        {
            return Hit::Minimap;
        }
        if panels.show_histogram && histogram::panel(content).contains(point) {
            return Hit::Histogram(histogram::widget_at(content, point, shown.gray));
        }
    }

    if panels.show_ui && chrome.contains(point) {
        return Hit::Chrome(chrome.widget_at(point, spacing, panels.paste));
    }
    Hit::Image
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::Rect;
    use crate::ui::Menu;

    const WINDOW: [f32; 2] = [1000.0, 700.0];

    fn shown() -> Option<Shown> {
        Some(Shown {
            size: [4000.0, 3000.0],
            gray: false,
            minimap: true,
        })
    }

    fn panels() -> Panels {
        Panels {
            show_ui: true,
            show_histogram: true,
            show_info: true,
            info_scroll: 0.0,
            show_luma: true,
            show_planes: true,
            log_counts: false,
            show_minimap: true,
            show_grid: false,
            paste: false,
            hover: None,
            info_hover: None,
            menu: None,
        }
    }

    /// Whether a point is inside a rectangle that may not be there at all.
    fn in_rect(rect: Option<Rect>, point: [f32; 2]) -> bool {
        rect.is_some_and(|rect| rect.contains(point))
    }

    fn middle(rect: Rect) -> [f32; 2] {
        [rect.x + rect.width / 2.0, rect.y + rect.height / 2.0]
    }

    /// The paste button is a layer's widget like any other, and the clipboard
    /// having nothing on it leaves the strip under the pointer instead of the
    /// button — the same answer the strip gives anywhere else on it.
    #[test]
    fn the_paste_button_is_on_the_chrome_only_while_there_is_a_paste() {
        let chrome = Chrome::new(WINDOW);
        let at = middle(chrome.paste_button);

        let mut panels = panels();
        panels.paste = true;
        assert_eq!(
            hit(at, &panels, WINDOW, shown(), None),
            Hit::Chrome(Some(Widget::Paste))
        );

        panels.paste = false;
        assert_eq!(hit(at, &panels, WINDOW, shown(), None), Hit::Chrome(None));
    }

    /// The stack, from the top down, each layer claiming its own point.
    #[test]
    fn every_layer_takes_the_pointer_where_it_is_drawn() {
        let panels = panels();
        let chrome = Chrome::new(WINDOW);
        let content = chrome.content();
        let at = |point| hit(point, &panels, WINDOW, shown(), None);

        assert_eq!(at(middle(content)), Hit::Image);
        assert_eq!(
            at(middle(chrome.histogram_button)),
            Hit::Chrome(Some(Widget::Histogram))
        );
        assert_eq!(at([WINDOW[0] / 2.0, 4.0]), Hit::Chrome(None));
        assert_eq!(
            at(middle(
                minimap::thumbnail(content, [4000.0, 3000.0]).expect("room")
            )),
            Hit::Minimap
        );
        assert_eq!(
            at(middle(info::panel(content, true).expect("room"))),
            Hit::Info
        );
        assert!(matches!(
            at(middle(histogram::panel(content))),
            Hit::Histogram(_)
        ));
    }

    /// The bug this stack was built to answer: the menu hangs off the zoom
    /// readout in the top bar, and the two panels down the right of the
    /// window are directly under it. It is drawn over them, so it takes the
    /// pointer from them — the press on a cell must not become a press on
    /// whatever the cell happens to be covering.
    #[test]
    fn the_open_menu_takes_the_pointer_from_the_panels_under_it() {
        let mut panels = panels();
        let chrome = Chrome::new(WINDOW);
        let content = chrome.content();
        panels.menu = Some(Menu::Zoom);
        let popup = chrome.popup(Menu::Zoom, None).expect("room");

        // The cells really are over the panels, or this proves nothing.
        let covered = popup
            .cells()
            .filter(|(_, cell)| {
                histogram::panel(content).contains(middle(*cell))
                    || in_rect(info::panel(content, true), middle(*cell))
            })
            .count();
        assert!(
            covered > 0,
            "the popup should overlap the right-hand panels"
        );

        for (index, cell) in popup.cells() {
            assert_eq!(
                hit(middle(cell), &panels, WINDOW, shown(), None),
                Hit::Cell(index),
                "cell {index}"
            );
        }
        // And the body it leaves between them is the menu's as well.
        let panel = popup.panel();
        assert_eq!(
            hit(
                [panel.x + 0.5, panel.y + 0.5],
                &panels,
                WINDOW,
                shown(),
                None
            ),
            Hit::Menu
        );

        // Off the popup the layers underneath answer as they always did: the
        // grab that makes a press there dismiss the menu is the handlers',
        // not the stack's, so the bar goes on reading out the pixel.
        assert_eq!(
            hit(middle(content), &panels, WINDOW, shown(), None),
            Hit::Image
        );
    }

    /// A panel takes what lands anywhere on it, buttons or no buttons. That
    /// is what keeps a press aimed at one from starting a drag of the picture
    /// it is floating over.
    #[test]
    fn a_panel_takes_the_pointer_between_its_buttons_too() {
        let panels = panels();
        let content = Chrome::new(WINDOW).content();
        let panel = histogram::panel(content);

        // The plot itself: on the panel, and on none of its toggles.
        let plot = [panel.right() - 4.0, panel.y + panel.height / 2.0];
        assert_eq!(
            hit(plot, &panels, WINDOW, shown(), None),
            Hit::Histogram(None),
            "the plot is the panel's, and it is not a button"
        );
        assert_eq!(hit(plot, &panels, WINDOW, shown(), None).widget(), None);
    }

    /// Nothing over the picture is drawn before there is a picture, so
    /// nothing over it takes the pointer either.
    #[test]
    fn the_floating_panels_are_not_there_before_the_first_image() {
        let panels = panels();
        let content = Chrome::new(WINDOW).content();

        for point in [
            middle(histogram::panel(content)),
            middle(info::panel(content, true).expect("room")),
        ] {
            assert_eq!(hit(point, &panels, WINDOW, None, None), Hit::Image);
        }
    }

    /// Hiding the bars gives the picture the whole window, and takes their
    /// widgets with it — but the panels that float over the picture are
    /// switched on separately and stay, over the ground the bars have given
    /// up.
    #[test]
    fn hiding_the_chrome_leaves_the_floating_panels_behind() {
        let mut panels = panels();
        let bar = middle(Chrome::new(WINDOW).bottom);
        assert_eq!(hit(bar, &panels, WINDOW, shown(), None), Hit::Chrome(None));

        panels.show_ui = false;
        assert_eq!(hit(bar, &panels, WINDOW, shown(), None), Hit::Image);
        let content = content_area(WINDOW, false);
        assert!(matches!(
            hit(
                middle(histogram::panel(content)),
                &panels,
                WINDOW,
                shown(),
                None
            ),
            Hit::Histogram(_)
        ));
    }

    /// A panel switched off is not on screen and does not answer for the
    /// ground it would have been drawn on — which the panel below it takes,
    /// the two being stacked down the same strip.
    #[test]
    fn a_panel_switched_off_hands_the_pointer_through() {
        let mut panels = panels();
        let content = Chrome::new(WINDOW).content();
        let panel = middle(histogram::panel(content));
        panels.show_histogram = false;

        assert_eq!(hit(panel, &panels, WINDOW, shown(), None), Hit::Info);
        panels.show_info = false;
        assert_eq!(hit(panel, &panels, WINDOW, shown(), None), Hit::Image);
    }
}
