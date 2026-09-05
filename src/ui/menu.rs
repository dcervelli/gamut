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

use super::buttons::{button_ink, centered_text, percent};
use super::icon;
use super::pixel::PixelFormat;
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
/// The room set aside for the mark in a fit cell. Larger than a toggle's,
/// the cells of a menu being larger than a button in a bar.
const FIT_ICON: f32 = 24.0;

/// A popup the interface can have open, and so what it is a menu of.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Menu {
    Zoom,
    /// How the bottom bar writes out the pixel under the pointer, opened from
    /// the dot at the head of that readout.
    PixelFormat,
    /// What a copy takes with it, opened from the button at the top of the
    /// left strip. The one menu of things to do rather than states to be in.
    Copy,
}

/// A copy the interface can be asked for, and so a cell of [`Menu::Copy`].
///
/// The copies of something the window is already showing, which is what a
/// menu can ask for at all: the two that take the pixel under the pointer are
/// not here, since the pointer is over the menu while the menu is open and
/// there would never be a pixel under it to take.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Copies {
    /// The file's own name, with nothing of the path it sits in.
    Name,
    Path,
    Uri,
    /// The picture itself, as the display settings show it.
    Image,
    /// Everything the information panel says about the file.
    Facts,
}

impl Copies {
    /// In the order the cells are laid out, two to a row.
    pub const ALL: [Copies; 5] = [
        Copies::Name,
        Copies::Path,
        Copies::Uri,
        Copies::Facts,
        Copies::Image,
    ];

    /// The word the cell wears. What the copy actually takes is the key
    /// table's to say — see `App::tooltip` — so these name the thing rather
    /// than describe the copy, and are short enough to sit in a cell.
    pub fn label(self) -> &'static str {
        match self {
            Copies::Name => "Name",
            Copies::Path => "Path",
            Copies::Uri => "URI",
            Copies::Image => "Image",
            Copies::Facts => "Info",
        }
    }
}

/// What a cell of a menu is, for anything outside the menu that has to say
/// something about it: the words that name the choice, and what the keyboard
/// does the same job with.
///
/// The action rather than the key: which key stands for an action is the
/// application's, and a menu that named one would be a second place for a
/// binding to be written down.
#[derive(Clone, PartialEq, Debug)]
pub struct CellTip {
    /// What the cell is, where the menu has words of its own for it. `None`
    /// where the key that does the same job already describes it at a length
    /// a label can carry, which leaves the key table to name it — a copy is
    /// described in a phrase, and writing that phrase here as well would be
    /// somewhere for the two to disagree.
    pub label: Option<String>,
    pub reach: Reach,
}

/// How the keyboard reaches what a cell of a menu sets directly: one of the
/// interface's cycles, stepped through until it arrives, or — for the
/// numbered cells — a zoom a key goes straight to.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Reach {
    Fit,
    Upscale,
    PixelFormat,
    Zoom(f32),
    /// A copy, which no cycle reaches: the key for it does exactly what the
    /// cell does, and is what names the cell.
    Copy(Copies),
}

impl Menu {
    pub fn sections(self) -> &'static [PopupSection] {
        match self {
            Menu::Zoom => &ZOOM_SECTIONS,
            Menu::PixelFormat => &PIXEL_SECTIONS,
            Menu::Copy => &COPY_SECTIONS,
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

    /// What cell `index` is, for the tooltip that names it.
    ///
    /// A numbered cell wears its own percentage and there is nothing a
    /// tooltip could add to "200%" — except the key that does the same thing,
    /// which is the whole reason it has one: the row of zooms is where the
    /// number row is there to be learned.
    pub fn cell_tip(self, index: usize) -> Option<CellTip> {
        match self {
            Menu::Zoom => match *ZOOM_CHOICES.get(index)? {
                ZoomChoice::Scale(scale) => Some(CellTip {
                    label: Some(format!("Zoom to {}", percent(scale))),
                    reach: Reach::Zoom(scale),
                }),
                ZoomChoice::Fit(Fit::Whole) => Some(CellTip {
                    label: Some("Fit the whole image".to_string()),
                    reach: Reach::Fit,
                }),
                ZoomChoice::Fit(Fit::Width) => Some(CellTip {
                    label: Some("Fit the image's width".to_string()),
                    reach: Reach::Fit,
                }),
                ZoomChoice::Fit(Fit::Height) => Some(CellTip {
                    label: Some("Fit the image's height".to_string()),
                    reach: Reach::Fit,
                }),
                // What the filter does, rather than what it is called: the
                // cell is already wearing the name.
                ZoomChoice::Filter(Upscale::Nearest) => Some(CellTip {
                    label: Some("Magnify to hard pixel edges".to_string()),
                    reach: Reach::Upscale,
                }),
                ZoomChoice::Filter(Upscale::Bicubic) => Some(CellTip {
                    label: Some("Magnify smoothly".to_string()),
                    reach: Reach::Upscale,
                }),
            },
            // What each format answers, rather than what it is called: the
            // cell is already wearing the name, and the name is the one thing
            // about a format that does not say which question it is for.
            Menu::PixelFormat => Some(CellTip {
                label: Some(
                    match PixelFormat::ALL.get(index)? {
                        PixelFormat::Hex => "The file's codes, as a color is written",
                        PixelFormat::Decimal => "The file's own numbers",
                        PixelFormat::Mapped => "What the display makes of them",
                    }
                    .to_string(),
                ),
                reach: Reach::PixelFormat,
            }),
            // No words of its own: the key table already says what each of
            // these copies takes, in a sentence, and saying it twice is
            // saying it in two places that can drift apart.
            Menu::Copy => Some(CellTip {
                label: None,
                reach: Reach::Copy(self.copy_at(index)?),
            }),
        }
    }

    /// Which copy cell `index` asks for, when this is the menu of copies.
    ///
    /// `None` for every other menu: those set a state and are answered by
    /// [`Menu::choose`], where a copy is something done and is the
    /// application's — nothing here has a file or a clipboard to hand.
    pub fn copy_at(self, index: usize) -> Option<Copies> {
        match self {
            Menu::Copy => Copies::ALL.get(index).copied(),
            Menu::Zoom | Menu::PixelFormat => None,
        }
    }

    /// Acts on cell `index`. Out-of-range indices cannot arrive — the popup
    /// only hands back cells it laid out — but a menu that has nothing to say
    /// about a cell simply says nothing.
    pub fn choose(
        self,
        index: usize,
        view: &mut View,
        format: &mut PixelFormat,
        image: [f32; 2],
        viewport: Viewport,
    ) {
        match self {
            Menu::Zoom => {
                if let Some(choice) = ZOOM_CHOICES.get(index) {
                    choice.apply(view, image, viewport);
                }
            }
            Menu::PixelFormat => {
                if let Some(choice) = PixelFormat::ALL.get(index) {
                    *format = *choice;
                }
            }
            // Nothing about the view or the readout: what its cells ask for
            // is done rather than set, and is [`Menu::copy_at`]'s.
            Menu::Copy => {}
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

/// The one section of the pixel-format menu: the three formats abreast, in
/// cells cut for words as the up-scaling filters' are. One section and no
/// heading would leave the panel saying nothing about what it is a menu of,
/// and it hangs from a dot rather than from a word.
const PIXEL_SECTIONS: [PopupSection; 1] = [PopupSection {
    title: "Pixel value",
    items: PixelFormat::ALL.len(),
    columns: PixelFormat::ALL.len(),
    cell_width: MENU_WORD_CELL,
}];

/// The one section of the menu of copies: everything that can be taken, each
/// cell wearing the name of the thing it takes.
///
/// One group rather than the file's own facts parted from the image itself.
/// The heading is what says the menu is of copies — the panel would otherwise
/// say nothing about what it is a menu of, as the pixel menu's does — and a
/// second heading naming the image would stand over a single cell wearing
/// that same word.
///
/// Two to a row rather than four abreast, and in the ordinary cell rather than
/// the wider one the words elsewhere ask for: the words here are one short
/// noun each, and the menu hangs off a button in the strip down the left of
/// the window, where a panel as wide as the zoom menu's would lie across the
/// picture it is a menu about.
const COPY_SECTIONS: [PopupSection; 1] = [PopupSection {
    title: "Copy",
    items: Copies::ALL.len(),
    columns: 2,
    cell_width: MENU_CELL[0],
}];

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
                        centered_text(frame, text, cell, ink, &percent(scale), TEXT_SIZE)
                    }
                    ZoomChoice::Fit(fit) => fit_icon(frame, cell, fit, background, ink),
                    // In words, where the fits above are in arrows: the two
                    // filters are not a direction or a size, and there is no
                    // picture of "bicubic" a reader would arrive at unaided.
                    // Their cells are cut wider so there is room to say so.
                    ZoomChoice::Filter(filter) => {
                        centered_text(frame, text, cell, ink, filter.label(), TEXT_SIZE)
                    }
                }
            }
            // Never lit: a copy is something done, and there is no state for
            // a cell of this menu to be showing — only the pointer's own
            // highlight tells one cell from the next.
            Menu::Copy => {
                let Some(copy) = Copies::ALL.get(index) else {
                    continue;
                };
                let (background, ink) = button_ink(false, hover, theme);
                frame.rounded_rect(cell, CELL_RADIUS, background);
                centered_text(frame, text, cell, ink, copy.label(), TEXT_SIZE);
            }
            // In words, as the filters are, and for the same reason: there is
            // no picture of "decimal" a reader would arrive at unaided.
            Menu::PixelFormat => {
                let Some(format) = PixelFormat::ALL.get(index) else {
                    continue;
                };
                let (background, ink) = button_ink(*format == panels.pixel_format, hover, theme);
                frame.rounded_rect(cell, CELL_RADIUS, background);
                centered_text(frame, text, cell, ink, format.label(), TEXT_SIZE);
            }
        }
    }
}

/// The three fits, each as the mark for what it fills: chevrons out to left
/// and right for the fit to the window's width, up and down for the one to
/// its height, and the four corners of `expand` for the fit that takes in the
/// whole image.
fn fit_icon(frame: &mut UiFrame, cell: Rect, fit: Fit, ground: Color, ink: Color) {
    let marks = match fit {
        Fit::Whole => icon::EXPAND,
        Fit::Width => icon::CHEVRONS_LEFT_RIGHT,
        Fit::Height => icon::CHEVRONS_UP_DOWN,
    };
    icon::draw(frame, marks, icon::fit(frame, cell, FIT_ICON), ink, ground);
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
        for spacing in [None, Some("50 px")] {
            let button = chrome.zoom_button(spacing);
            let popup = chrome
                .popup(Menu::Zoom, spacing)
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
                .popup(Menu::Zoom, None)
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
        let popup = chrome.popup(Menu::Zoom, None).expect("room for it");
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

    /// The pixel menu is exactly the formats on offer, in the order the key
    /// steps through them, and pressing a cell puts the readout in the format
    /// that cell wears — the same one the cell is lit for.
    #[test]
    fn every_pixel_format_has_a_cell_that_chooses_it() {
        let chrome = Chrome::new(WINDOW);
        let popup = chrome
            .popup(Menu::PixelFormat, None)
            .expect("a window with room for it");
        assert_eq!(popup.cells().count(), PixelFormat::ALL.len());
        assert_eq!(PIXEL_SECTIONS[0].items, PixelFormat::ALL.len());

        // It stands over the button that opens it, at the other end of the
        // window from the zoom menu.
        assert!(popup.panel().bottom() <= chrome.pixel_button.y);

        let mut view = View::new();
        for (index, expected) in PixelFormat::ALL.into_iter().enumerate() {
            let mut format = PixelFormat::default();
            Menu::PixelFormat.choose(
                index,
                &mut view,
                &mut format,
                [900.0, 600.0],
                Viewport::whole(WINDOW),
            );
            assert_eq!(format, expected);
        }

        // Wide enough for the longest of the names it is cut for.
        let longest = PixelFormat::ALL
            .iter()
            .map(|format| format.label().len())
            .max()
            .expect("three formats");
        assert!(popup.cell(0).width > longest as f32 * TEXT_SIZE * 0.7);
    }

    /// The menu of copies stands beside the button that opens it — that
    /// button is in a column, with its neighbors above and below — and offers
    /// exactly the copies on offer, in order.
    #[test]
    fn the_copy_menu_stands_beside_the_button_that_opens_it() {
        let chrome = Chrome::new(WINDOW);
        let popup = chrome
            .popup(Menu::Copy, None)
            .expect("a window with room for it");
        let (panel, button) = (popup.panel(), chrome.copy_button);

        assert_eq!(popup.cells().count(), Copies::ALL.len());
        assert!(panel.x >= button.right(), "{panel:?} beside {button:?}");
        assert_eq!(panel.y, button.y);
        assert!(panel.bottom() <= WINDOW[1] - PADDING);

        // Every cell is one of the copies, in the order they are listed, and
        // there is nothing past the last of them.
        for (index, expected) in Copies::ALL.into_iter().enumerate() {
            assert_eq!(Menu::Copy.copy_at(index), Some(expected));
        }
        assert_eq!(Menu::Copy.copy_at(Copies::ALL.len()), None);
        assert_eq!(
            COPY_SECTIONS
                .iter()
                .map(|section| section.items)
                .sum::<usize>(),
            Copies::ALL.len()
        );

        // Wide enough for the words the cells wear, which is what keeps them
        // in the ordinary cell rather than the wider one.
        let longest = Copies::ALL
            .iter()
            .map(|copy| copy.label().len())
            .max()
            .expect("five copies");
        assert!(popup.cell(0).width > longest as f32 * TEXT_SIZE * 0.7);
    }

    /// A copy cell leaves the naming to the key that does the same job: it
    /// has a key of its own doing exactly what it does, where every other
    /// cell is reached by a cycle that describes none of them.
    #[test]
    fn only_a_copy_cell_has_no_words_of_its_own() {
        for index in 0..Copies::ALL.len() {
            let tip = Menu::Copy.cell_tip(index).expect("a cell");
            assert_eq!(tip.label, None);
            assert!(matches!(tip.reach, Reach::Copy(_)));
        }
        for (menu, count) in [
            (Menu::Zoom, ZOOM_CHOICES.len()),
            (Menu::PixelFormat, PixelFormat::ALL.len()),
        ] {
            for index in 0..count {
                let tip = menu.cell_tip(index).expect("a cell");
                assert!(tip.label.is_some(), "{menu:?} cell {index}");
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
                ZoomChoice::Fit(_) | ZoomChoice::Filter(_) => None,
            })
            .max();
        assert_eq!(widest, Some("1600%".len()));
    }
}
