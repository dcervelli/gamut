//! Popup menus: which one is open, what is on it, and how its cells are
//! drawn. The panel itself — where it goes and what a press lands on — is
//! [`Popup`]'s.
//!
//! One at a time: a second would have to say which of the two a press outside
//! dismisses. Adding another is a [`Menu`] variant, its choices, and an arm
//! in [`draw`] — where the panel goes, what a press lands on and how it is
//! dismissed are the same for every menu.

use crate::render::{Color, Popup, PopupGrid, PopupSection, Rect, TextMeasure, UiFrame, Upscale};
use crate::theme::Theme;
use crate::view::{Fit, View, Viewport};

use super::buttons::{button_ink, centred_text, outline, percent};
use super::{PADDING, Panels, TEXT_SIZE, Widget};

/// An ordinary cell of a popup menu, and the room around them. Wider than it
/// is tall because the widest thing in one is "1600%", and no taller than the
/// word in it needs: a cell with room to spare above and below reads as a
/// panel rather than as a button.
const MENU_CELL: [f32; 2] = [56.0, 27.0];
/// A cell in a section that is named in words rather than numbered or drawn.
/// Wide enough for the longest of them at [`TEXT_SIZE`] with room around it,
/// and no wider: these sit under the numbered cells and are meant to read as
/// the same kind of button, not as a wider one.
const MENU_WORD_CELL: f32 = 84.0;
const MENU_GAP: f32 = 6.0;
const MENU_PADDING: f32 = 8.0;
/// The line a section's name is set on, the space under it, and the space
/// between one section and the next. The gap above a name is the wider of the
/// two, so the name reads as belonging to the cells beneath it — the same
/// arrangement, and for the same reason, as the information panel's.
const MENU_HEADING: f32 = 15.0;
const MENU_HEADING_GAP: f32 = 3.0;
const MENU_SECTION_GAP: f32 = 10.0;
/// The corner radius of a popup's panel, and of the cells inside it.
const MENU_RADIUS: f32 = 8.0;
pub(super) const CELL_RADIUS: f32 = 5.0;
/// The frame drawn in a fit cell of the zoom menu, which the arrows point out
/// to the edges of.
const FIT_ICON: [f32; 2] = [26.0, 17.0];

/// A popup the interface can have open, and so what it is a menu of.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Menu {
    Zoom,
}

impl Menu {
    pub fn sections(self) -> &'static [PopupSection] {
        match self {
            Menu::Zoom => &ZOOM_SECTIONS,
        }
    }

    pub fn grid(self) -> PopupGrid {
        PopupGrid {
            cell_height: MENU_CELL[1],
            gap: MENU_GAP,
            padding: MENU_PADDING,
            margin: PADDING,
            radius: MENU_RADIUS,
            heading: MENU_HEADING,
            heading_gap: MENU_HEADING_GAP,
            section_gap: MENU_SECTION_GAP,
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

/// How the zoom menu is divided. Three things are chosen from it and they
/// are not the same kind of thing: a zoom to go to, a rule for the view to
/// keep, and how the magnified image is resampled. Undivided, the last of
/// them read as a fourth fit.
///
/// The counts are [`ZOOM_CHOICES`] split up, in that order, and the columns
/// are what each group wants: eight numbers in fours, three fits abreast, and
/// two filters named in words rather than drawn as icons.
///
/// Only the last takes a cell of its own width, and only because a word needs
/// more room than a number. The rest keep the ordinary cell and stop where
/// their own cells stop, so the fits sit under the first three percentages
/// rather than being spread across the panel to fill it.
const ZOOM_SECTIONS: [PopupSection; 3] = [
    PopupSection {
        title: "Zoom",
        items: 8,
        columns: 4,
        cell_width: MENU_CELL[0],
    },
    PopupSection {
        title: "Fit",
        items: 3,
        columns: 3,
        cell_width: MENU_CELL[0],
    },
    PopupSection {
        title: "Up-scaling",
        items: Upscale::ALL.len(),
        columns: Upscale::ALL.len(),
        cell_width: MENU_WORD_CELL,
    },
];

/// What the zoom menu offers. The order is the order the cells are laid out
/// in, section by section and left to right within each.
const ZOOM_CHOICES: [ZoomChoice; 13] = [
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
    ZoomChoice::Filter(Upscale::Nearest),
    ZoomChoice::Filter(Upscale::Bicubic),
];

/// One cell of the zoom menu: a zoom to go to, a fit to hand the view back
/// to, or the filter the image is magnified with.
#[derive(Clone, Copy, PartialEq, Debug)]
enum ZoomChoice {
    Scale(f32),
    Fit(Fit),
    Filter(Upscale),
}

impl ZoomChoice {
    /// Whether this is what the view is already doing — `fit`, `zoom` and
    /// `upscale` being what it is doing — which is what lights the cell. A
    /// fit is only itself; a scale counts as matched when it is the zoom on
    /// screen and the view is not in a fit that happens to have landed there,
    /// since pressing it would then mean something. A filter is always one of
    /// the two, so one of that section's cells is always lit.
    fn active(self, fit: Option<Fit>, zoom: f32, upscale: Upscale) -> bool {
        match self {
            ZoomChoice::Scale(scale) => fit.is_none() && (zoom - scale).abs() < scale * 1e-3,
            ZoomChoice::Fit(fit_choice) => fit == Some(fit_choice),
            ZoomChoice::Filter(filter) => filter == upscale,
        }
    }

    fn apply(self, view: &mut View, image: [f32; 2], viewport: Viewport) {
        match self {
            ZoomChoice::Scale(scale) => view.set_zoom(scale, image, viewport),
            ZoomChoice::Fit(fit) => view.set_fit(fit),
            ZoomChoice::Filter(filter) => view.set_upscale(filter),
        }
    }
}

/// Draws the open menu — [`Panels::menu`], which `popup` was placed for — as
/// its panel, the name of each section, and a cell for each choice in it.
/// The view is what every cell is measured against, so that the one it
/// matches can be lit; `zoom` comes with it because working it out needs the
/// image and the viewport, which the caller has already had to hand.
///
/// The cells are drawn like the toggles in the side panels, and for the same
/// reason: each is a press, and a state it is either in or not.
pub(super) fn draw(
    frame: &mut UiFrame,
    text: &mut dyn TextMeasure,
    popup: &Popup,
    view: &View,
    zoom: f32,
    panels: &Panels,
    theme: &Theme,
) {
    let Some(menu) = panels.menu else {
        return;
    };
    let (fit, upscale) = (view.fit(), view.upscale());
    popup.draw(frame, theme.menu_background);

    // The names, in the accent the information panel sets its own headings
    // in: a heading is the one thing on a panel that is picked out, and the
    // two panels should not disagree about how that is done.
    for (title, line) in popup.headings() {
        frame.text(
            [line.x, (line.bottom() - TEXT_SIZE * 1.15).round()],
            TEXT_SIZE,
            theme.accent,
            title,
        );
    }

    for (index, cell) in popup.cells() {
        let hover = panels.hover == Some(Widget::Cell(index));
        match menu {
            Menu::Zoom => {
                let choice = ZOOM_CHOICES[index];
                let active = choice.active(fit, zoom, upscale);
                let (background, ink) = button_ink(active, hover, theme);
                frame.rounded_rect(cell, CELL_RADIUS, background);
                match choice {
                    ZoomChoice::Scale(scale) => {
                        centred_text(frame, text, cell, ink, &percent(scale))
                    }
                    ZoomChoice::Fit(fit) => fit_icon(frame, cell, fit, ink),
                    // In words, where the fits above are in arrows: the two
                    // filters are not a direction or a size, and there is no
                    // picture of "bicubic" a reader would arrive at unaided.
                    // Their cells are cut wider so there is room to say so.
                    ZoomChoice::Filter(filter) => {
                        centred_text(frame, text, cell, ink, filter.label())
                    }
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

    /// The menu hangs from the readout that opens it: under it, and with the
    /// two right edges in line. Where it lands is settled by the button and
    /// by the window, never by the frame the picture is in.
    #[test]
    fn the_zoom_menu_hangs_from_the_readout_that_opens_it() {
        let chrome = Chrome::new(WINDOW);
        for grid_on in [false, true] {
            let button = chrome.zoom_button(grid_on);
            let popup = chrome
                .popup(Menu::Zoom, grid_on)
                .expect("a window with room for it");

            assert_eq!(popup.cells().count(), ZOOM_CHOICES.len());
            // Right edges in line, and hanging by the grid's own margin: the
            // whole placement comes from the button, so the menu goes
            // wherever the button has gone rather than to a fixed corner.
            let panel = popup.panel();
            assert_eq!(panel.right(), button.right());
            assert_eq!(panel.y, button.bottom() + PADDING);
            // Clear of the bar it hangs from, and inside the window.
            assert!(panel.y >= chrome.top.bottom());
            assert!(panel.bottom() <= WINDOW[1] - PADDING);
        }

        // A window with no room for the whole of it gets no menu at all,
        // which is also what stops one being opened there.
        assert!(
            Chrome::new([220.0, 200.0])
                .popup(Menu::Zoom, false)
                .is_none()
        );
    }

    /// Which section a choice belongs to, which is also which other choices
    /// it is exclusive with: picking a filter says nothing about the zoom,
    /// and picking a zoom says nothing about the filter.
    fn section_of(choice: ZoomChoice) -> usize {
        match choice {
            ZoomChoice::Scale(_) => 0,
            ZoomChoice::Fit(_) => 1,
            ZoomChoice::Filter(_) => 2,
        }
    }

    /// What a cell says it does is what pressing it does: the state each one
    /// puts the view in is the state that lights that cell and no other in
    /// its own section.
    #[test]
    fn every_zoom_choice_lands_on_itself() {
        let image = [900.0, 600.0];
        let viewport = Viewport::whole(WINDOW);

        for choice in ZOOM_CHOICES {
            let mut view = View::new();
            choice.apply(&mut view, image, viewport);
            let (fit, zoom, upscale) = (view.fit(), view.zoom(image, viewport), view.upscale());
            assert!(choice.active(fit, zoom, upscale), "{choice:?}");

            for other in ZOOM_CHOICES {
                if section_of(other) != section_of(choice) {
                    continue;
                }
                assert_eq!(
                    other.active(fit, zoom, upscale),
                    other == choice,
                    "{other:?} after {choice:?}"
                );
            }
            if let ZoomChoice::Scale(scale) = choice {
                assert!((view.zoom(image, viewport) - scale).abs() < 1e-4);
            }
        }
    }

    /// The sections are [`ZOOM_CHOICES`] cut into three, and the cut has to
    /// stay in step with it: a choice in no section could never be pressed,
    /// and a section reaching past its own kind would light a cell that
    /// stands for something else.
    #[test]
    fn the_sections_account_for_every_choice_in_order() {
        assert_eq!(
            ZOOM_SECTIONS
                .iter()
                .map(|section| section.items)
                .sum::<usize>(),
            ZOOM_CHOICES.len()
        );

        let mut first = 0;
        for (index, section) in ZOOM_SECTIONS.iter().enumerate() {
            for choice in &ZOOM_CHOICES[first..first + section.items] {
                assert_eq!(
                    section_of(*choice),
                    index,
                    "{choice:?} in {}",
                    section.title
                );
            }
            first += section.items;
        }

        // And the up-scaling section is exactly the filters on offer, in the
        // order the key cycles them.
        let up_scaling = &ZOOM_CHOICES[ZOOM_CHOICES.len() - ZOOM_SECTIONS[2].items..];
        let filters: Vec<Upscale> = up_scaling
            .iter()
            .map(|choice| match choice {
                ZoomChoice::Filter(filter) => *filter,
                other => panic!("{other:?} is not a filter"),
            })
            .collect();
        assert_eq!(filters, Upscale::ALL);
    }

    /// Two to a row is what makes room for the words, so the cells that wear
    /// them are wider than the numbered ones — and wide enough for the
    /// longest name at the size it is set in.
    #[test]
    fn the_filters_are_named_in_cells_cut_wide_enough_for_the_words() {
        let chrome = Chrome::new(WINDOW);
        let popup = chrome.popup(Menu::Zoom, false).expect("room for it");
        let scale = popup.cell(0);
        let filter = popup.cell(ZOOM_CHOICES.len() - 1);

        assert!(filter.width > scale.width, "{filter:?} vs {scale:?}");
        // Written out, not drawn: the longest of them, with room to spare.
        let longest = Upscale::ALL
            .iter()
            .map(|filter| filter.label().len())
            .max()
            .expect("two filters");
        assert!(filter.width > longest as f32 * TEXT_SIZE * 0.7);
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
                ZoomChoice::Fit(_) | ZoomChoice::Filter(_) => None,
            })
            .max();
        assert_eq!(widest, Some("1600%".len()));
    }
}
