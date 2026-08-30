//! Popup menus: which one is open, what is on it, and how its cells are
//! drawn. The panel itself — where it goes and what a press lands on — is
//! [`Popup`]'s.
//!
//! One at a time: a second would have to say which of the two a press outside
//! dismisses. Adding another is a [`Menu`] variant, its choices, and an arm
//! in [`draw`] — where the panel goes, what a press lands on and how it is
//! dismissed are the same for every menu.

use crate::render::{Color, Popup, PopupGrid, Rect, TextMeasure, UiFrame};
use crate::theme::Theme;
use crate::view::{Fit, View, Viewport};

use super::buttons::{button_ink, centred_text, outline, percent};
use super::{PADDING, Panels, Widget};

/// One cell of a popup menu, and the room around them. A cell is a little
/// wider than it is tall because the widest thing in one is "1600%".
const MENU_CELL: [f32; 2] = [56.0, 34.0];
const MENU_GAP: f32 = 6.0;
const MENU_PADDING: f32 = 8.0;
/// The corner radius of a popup's panel, and of the cells inside it.
const MENU_RADIUS: f32 = 8.0;
pub(super) const CELL_RADIUS: f32 = 5.0;
/// The frame drawn in a fit cell of the zoom menu, which the arrows point out
/// to the edges of.
const FIT_ICON: [f32; 2] = [28.0, 22.0];

/// A popup the interface can have open, and so what it is a menu of.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Menu {
    Zoom,
}

impl Menu {
    pub fn items(self) -> usize {
        match self {
            Menu::Zoom => ZOOM_CHOICES.len(),
        }
    }

    pub fn grid(self) -> PopupGrid {
        let columns = match self {
            // Eight percentages and three fits: two full rows of powers of
            // two, and the fits along the bottom.
            Menu::Zoom => 4,
        };
        PopupGrid {
            cell: MENU_CELL,
            columns,
            gap: MENU_GAP,
            padding: MENU_PADDING,
            margin: PADDING,
            radius: MENU_RADIUS,
        }
    }

    /// Acts on cell `index`. Out-of-range indices cannot arrive — the popup
    /// only hands back cells it laid out — but a menu that has nothing to say
    /// about a cell simply says nothing.
    pub fn choose(self, index: usize, view: &mut View, image: [f32; 2], viewport: Viewport) {
        match self {
            Menu::Zoom => {
                if let Some(choice) = ZOOM_CHOICES.get(index) {
                    choice.apply(view, image, viewport);
                }
            }
        }
    }
}

/// What the zoom menu offers. The order is the order the cells are laid out
/// in, left to right and top to bottom.
const ZOOM_CHOICES: [ZoomChoice; 11] = [
    ZoomChoice::Scale(0.10),
    ZoomChoice::Scale(0.25),
    ZoomChoice::Scale(0.50),
    ZoomChoice::Scale(1.0),
    ZoomChoice::Scale(2.0),
    ZoomChoice::Scale(4.0),
    ZoomChoice::Scale(8.0),
    ZoomChoice::Scale(16.0),
    ZoomChoice::Fit(Fit::Whole),
    ZoomChoice::Fit(Fit::Width),
    ZoomChoice::Fit(Fit::Height),
];

/// One cell of the zoom menu: a zoom to go to, or a fit to hand the view back
/// to.
#[derive(Clone, Copy, PartialEq, Debug)]
enum ZoomChoice {
    Scale(f32),
    Fit(Fit),
}

impl ZoomChoice {
    /// Whether this is what the view is already doing — `fit` and `zoom`
    /// being what it is doing — which is what lights the cell. A fit is only
    /// itself; a scale counts as matched when it is the zoom on screen and
    /// the view is not in a fit that happens to have landed there, since
    /// pressing it would then mean something.
    fn active(self, fit: Option<Fit>, zoom: f32) -> bool {
        match self {
            ZoomChoice::Scale(scale) => fit.is_none() && (zoom - scale).abs() < scale * 1e-3,
            ZoomChoice::Fit(fit_choice) => fit == Some(fit_choice),
        }
    }

    fn apply(self, view: &mut View, image: [f32; 2], viewport: Viewport) {
        match self {
            ZoomChoice::Scale(scale) => view.set_zoom(scale, image, viewport),
            ZoomChoice::Fit(fit) => view.set_fit(fit),
        }
    }
}

/// Draws the open menu — [`Panels::menu`], which `popup` was placed for —
/// as its panel and a cell for each choice in it. `fit` and `zoom` are what
/// the view is doing, so that the cell it matches can be lit.
///
/// The cells are drawn like the toggles in the side panels, and for the same
/// reason: each is a press, and a state it is either in or not.
pub(super) fn draw(
    frame: &mut UiFrame,
    text: &mut dyn TextMeasure,
    popup: &Popup,
    fit: Option<Fit>,
    zoom: f32,
    panels: &Panels,
    theme: &Theme,
) {
    let Some(menu) = panels.menu else {
        return;
    };
    popup.draw(frame, theme.menu_background);
    for (index, cell) in popup.cells() {
        let hover = panels.hover == Some(Widget::Cell(index));
        match menu {
            Menu::Zoom => {
                let choice = ZOOM_CHOICES[index];
                let (background, ink) = button_ink(choice.active(fit, zoom), hover, theme);
                frame.rounded_rect(cell, CELL_RADIUS, background);
                match choice {
                    ZoomChoice::Scale(scale) => {
                        centred_text(frame, text, cell, ink, &percent(scale))
                    }
                    ZoomChoice::Fit(fit) => fit_icon(frame, cell, fit, ink),
                }
            }
        }
    }
}

/// A box of `size` centred in `rect`: where an icon goes in a cell it is not
/// meant to fill.
fn centred(rect: Rect, size: [f32; 2]) -> Rect {
    Rect::new(
        (rect.x + (rect.width - size[0]) / 2.0).round(),
        (rect.y + (rect.height - size[1]) / 2.0).round(),
        size[0].min(rect.width),
        size[1].min(rect.height),
    )
}

/// The three fits, as the frame each of them fills and the directions it
/// fills it in: arrows out to left and right for a fit to the width, up and
/// down for one to the height, and both for the fit that takes in the whole
/// image.
fn fit_icon(frame: &mut UiFrame, cell: Rect, fit: Fit, ink: Color) {
    let icon = centred(cell, FIT_ICON);
    outline(frame, icon, 1.5, ink);
    let inner = icon.inset(3.0, 3.0);
    if fit != Fit::Height {
        double_arrow(frame, inner, true, ink);
    }
    if fit != Fit::Width {
        double_arrow(frame, inner, false, ink);
    }
}

/// A double-headed arrow spanning `rect` along one axis and centred across
/// the other: a shaft with a triangle pointing out at each end.
fn double_arrow(frame: &mut UiFrame, rect: Rect, horizontal: bool, color: Color) {
    /// How far back from the point an arrowhead reaches, and how wide it is
    /// there.
    const HEAD: [f32; 2] = [5.0, 7.0];
    const SHAFT: f32 = 1.5;

    let (span, across) = if horizontal {
        (rect.width, rect.height)
    } else {
        (rect.height, rect.width)
    };
    // Two heads and nothing between them is still an arrow; less than that is
    // a smudge, and the cell is better left with just its frame.
    let head = HEAD[0].min(span / 2.0);
    if span <= 0.0 || across < HEAD[1] {
        return;
    }
    let middle = |low: f32, extent: f32, width: f32| low + (extent - width) / 2.0;

    if horizontal {
        let centre = rect.y + rect.height / 2.0;
        frame.rect(
            Rect::new(
                rect.x + head,
                middle(rect.y, rect.height, SHAFT),
                span - 2.0 * head,
                SHAFT,
            ),
            color,
        );
        for (point, back) in [(rect.x, rect.x + head), (rect.right(), rect.right() - head)] {
            frame.triangle(
                [
                    [point, centre],
                    [back, centre - HEAD[1] / 2.0],
                    [back, centre + HEAD[1] / 2.0],
                ],
                color,
            );
        }
    } else {
        let centre = rect.x + rect.width / 2.0;
        frame.rect(
            Rect::new(
                middle(rect.x, rect.width, SHAFT),
                rect.y + head,
                SHAFT,
                span - 2.0 * head,
            ),
            color,
        );
        for (point, back) in [
            (rect.y, rect.y + head),
            (rect.bottom(), rect.bottom() - head),
        ] {
            frame.triangle(
                [
                    [centre, point],
                    [centre - HEAD[1] / 2.0, back],
                    [centre + HEAD[1] / 2.0, back],
                ],
                color,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::chrome::Chrome;

    const WINDOW: [f32; 2] = [1000.0, 700.0];

    #[test]
    fn the_zoom_menu_pops_up_in_the_lower_right_of_the_content_area() {
        let chrome = Chrome::new(WINDOW);
        let content = chrome.content();
        let popup = chrome.popup(Menu::Zoom).expect("a window with room for it");

        assert_eq!(popup.cells().count(), ZOOM_CHOICES.len());
        // Over the image, clear of the panels: the menu is drawn on the frame
        // the image is in, and half of it under the bottom bar would be half
        // a menu.
        let panel = popup.panel();
        assert!(panel.x >= content.x && panel.right() <= content.right());
        assert!(panel.y >= content.y && panel.bottom() <= content.bottom());
        // In the corner nearest the button that opens it.
        assert_eq!(panel.right(), content.right() - PADDING);
        assert_eq!(panel.bottom(), content.bottom() - PADDING);

        // A window with no room for the whole of it gets no menu at all,
        // which is also what stops one being opened there.
        assert!(Chrome::new([220.0, 200.0]).popup(Menu::Zoom).is_none());
    }

    /// What a cell says it does is what pressing it does: the state each one
    /// puts the view in is the state that lights that cell and no other.
    #[test]
    fn every_zoom_choice_lands_on_itself() {
        let image = [900.0, 600.0];
        let viewport = Viewport::whole(WINDOW);

        for choice in ZOOM_CHOICES {
            let mut view = View::new();
            choice.apply(&mut view, image, viewport);
            let (fit, zoom) = (view.fit(), view.zoom(image, viewport));
            assert!(choice.active(fit, zoom), "{choice:?}");

            for other in ZOOM_CHOICES {
                assert_eq!(
                    other.active(fit, zoom),
                    other == choice,
                    "{other:?} after {choice:?}"
                );
            }
            if let ZoomChoice::Scale(scale) = choice {
                assert!((view.zoom(image, viewport) - scale).abs() < 1e-4);
            }
        }
    }

    /// The button reads out the same zoom the cells are chosen from, so the
    /// two have to agree on how a zoom is written down.
    #[test]
    fn the_readout_is_written_the_way_the_menu_writes_it() {
        assert_eq!(percent(0.1), "10%");
        assert_eq!(percent(1.0), "100%");
        assert_eq!(percent(16.0), "1600%");
        let widest = ZOOM_CHOICES
            .iter()
            .filter_map(|choice| match choice {
                ZoomChoice::Scale(scale) => Some(percent(*scale).len()),
                ZoomChoice::Fit(_) => None,
            })
            .max();
        assert_eq!(widest, Some("1600%".len()));
    }
}
