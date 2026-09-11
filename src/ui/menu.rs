//! Popup menus: what is on each, and how its cells are drawn. Where a popup
//! goes and how it is dismissed are egui's.
//!
//! Four of them, one open at a time: a second would have to say which of the
//! two a press outside dismisses. Adding another is a set of choices, a
//! function that lays them out, and the button in the chrome that opens it.
//! The file chooser is a fifth popup under the same rule, though not a menu
//! — see `ui::chooser`.

use egui::{Button, RichText, Sense, Ui, Vec2, WidgetInfo, WidgetType, vec2};

use crate::render::Upscale;
use crate::view::{Axis, Fit, View, Viewport};

use super::chrome::Pass;
use super::control::Control;
use super::icon;
use super::pixel::PixelFormat;
use super::style::TOGGLE_RADIUS;
use super::tooltip::Tip;

/// An ordinary cell of a popup menu. Wider than it is tall because the
/// widest thing in one is "1600%", and no taller than the word in it needs:
/// a cell with room to spare above and below reads as a panel rather than as
/// a button.
const MENU_CELL: [f32; 2] = [56.0, 27.0];
/// A cell in a section that is named in words rather than numbered or drawn.
/// Wide enough for the longest of them with room around it, and no wider:
/// these sit under the numbered cells and are meant to read as the same kind
/// of button, not as a wider one.
const MENU_WORD_CELL: f32 = 84.0;
/// The gap between two cells.
const MENU_GAP: f32 = 6.0;
/// The space under a section's name, and the space between one section and
/// the next. The gap above a name is the wider of the two, so the name reads
/// as belonging to the cells beneath it.
const MENU_HEADING_GAP: f32 = 3.0;
const MENU_SECTION_GAP: f32 = 10.0;
/// The room set aside for the mark in a fit cell. Larger than a toggle's,
/// the cells of a menu being larger than a button in a bar.
const FIT_ICON: f32 = 24.0;

/// A copy the interface can be asked for, and so an item of the menu of
/// copies.
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
    /// In the order the items are laid out.
    pub const ALL: [Copies; 5] = [
        Copies::Name,
        Copies::Path,
        Copies::Uri,
        Copies::Facts,
        Copies::Image,
    ];

    /// The word the item wears. What the copy actually takes is the key
    /// table's to say — see `App::namer` — so these name the thing rather
    /// than describe the copy.
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

/// One cell of the zoom menu: a zoom to go to, a fit to hand the view back
/// to, or the filter the image is magnified with.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ZoomChoice {
    Scale(f32),
    Fit(Fit),
    Filter(Upscale),
}

/// What the zoom menu offers. The order is the order the cells are laid out
/// in, section by section and left to right within each.
pub const ZOOM_CHOICES: [ZoomChoice; 12] = [
    ZoomChoice::Scale(0.10),
    ZoomChoice::Scale(0.25),
    ZoomChoice::Scale(0.50),
    ZoomChoice::Scale(1.0),
    ZoomChoice::Scale(2.0),
    ZoomChoice::Scale(4.0),
    ZoomChoice::Scale(8.0),
    ZoomChoice::Scale(16.0),
    ZoomChoice::Fit(Fit::Whole),
    ZoomChoice::Fit(Fit::Fill),
    ZoomChoice::Filter(Upscale::Nearest),
    ZoomChoice::Filter(Upscale::Bicubic),
];

/// How the zoom menu is divided: a section's name, how many of
/// [`ZOOM_CHOICES`] it takes, in that order, and how many to a row.
///
/// Three things are chosen from it and they are not the same kind of thing:
/// a zoom to go to, a rule for the view to keep, and how the magnified image
/// is resampled. Undivided, the last of them read as a third fit. Eight
/// numbers in fours, the two fits abreast, and two filters named in words
/// rather than drawn as icons — the one pair of cells cut wider, and only
/// because a word needs more room than a number.
pub const ZOOM_SECTIONS: [(&str, usize, usize); 3] =
    [("Zoom", 8, 4), ("Fit", 2, 2), ("Up-scaling", 2, 2)];

impl ZoomChoice {
    /// Whether this is what the view is already doing — `fit`, `zoom` and
    /// `upscale` being what it is doing — which is what lights the cell. A
    /// fit is only itself; a scale counts as matched when it is the zoom on
    /// screen and the view is not in a fit that happens to have landed there,
    /// since pressing it would then mean something. A filter is always one of
    /// the two, so one of that section's cells is always lit.
    pub fn active(self, fit: Option<Fit>, zoom: f32, upscale: Upscale) -> bool {
        match self {
            ZoomChoice::Scale(scale) => fit.is_none() && (zoom - scale).abs() < scale * 1e-3,
            ZoomChoice::Fit(fit_choice) => fit == Some(fit_choice),
            ZoomChoice::Filter(filter) => filter == upscale,
        }
    }

    pub fn apply(self, view: &mut View, image: [f32; 2], viewport: Viewport) {
        match self {
            ZoomChoice::Scale(scale) => view.set_zoom(scale, image, viewport),
            ZoomChoice::Fit(fit) => view.set_fit(fit),
            ZoomChoice::Filter(filter) => view.set_upscale(filter),
        }
    }

    /// What the cell is called to something that cannot see it.
    pub fn label(self) -> String {
        match self {
            ZoomChoice::Scale(scale) => percent(scale),
            ZoomChoice::Fit(Fit::Whole) => "Fit whole".to_string(),
            ZoomChoice::Fit(Fit::Fill) => "Fill".to_string(),
            ZoomChoice::Filter(filter) => filter.label().to_string(),
        }
    }

    /// What the cell does, for the tooltip on it.
    ///
    /// A numbered cell wears its own percentage and there is nothing a
    /// tooltip could add to "200%" — except the key that does the same thing,
    /// which is the whole reason it has one: the row of zooms is where the
    /// number row is there to be learned. A filter is described by what it
    /// does rather than what it is called, the cell already wearing the name.
    pub fn describe(self) -> String {
        match self {
            ZoomChoice::Scale(scale) => format!("Zoom to {}", percent(scale)),
            ZoomChoice::Fit(Fit::Whole) => "Fit the whole image".to_string(),
            ZoomChoice::Fit(Fit::Fill) => "Fill the window with the image".to_string(),
            ZoomChoice::Filter(Upscale::Nearest) => "Magnify to hard pixel edges".to_string(),
            ZoomChoice::Filter(Upscale::Bicubic) => "Magnify smoothly".to_string(),
        }
    }
}

/// How a zoom is written down, on the readout and in the cells alike.
pub fn percent(zoom: f32) -> String {
    format!("{:.0}%", zoom * 100.0)
}

/// What each pixel format answers, rather than what it is called: the cell
/// is already wearing the name, and the name is the one thing about a format
/// that does not say which question it is for.
pub fn describe_format(format: PixelFormat) -> &'static str {
    match format {
        PixelFormat::Hex => "The file's codes, as a color is written",
        PixelFormat::Decimal => "The file's own numbers",
        PixelFormat::Mapped => "What the display makes of them",
    }
}

/// A section's name, in the accent the information panel sets its own
/// headings in: a heading is the one thing on a panel that is picked out, and
/// the two panels should not disagree about how that is done.
fn heading(pass: &Pass, ui: &mut Ui, title: &str, first: bool) {
    if !first {
        ui.add_space(MENU_SECTION_GAP);
    }
    ui.label(RichText::new(title).color(pass.theme.accent));
    ui.add_space(MENU_HEADING_GAP);
}

/// The zoom menu: the name of each section and a cell for each choice in
/// it. `zoom` is what the cells are lit against, and `fills` the axis a fill
/// would fill, which is what the fill cell's mark points along.
pub(super) fn zoom_cells(pass: &mut Pass, ui: &mut Ui, zoom: f32, fills: Axis) {
    ui.spacing_mut().item_spacing = Vec2::ZERO;
    let (fit, upscale) = (pass.view.fit(), pass.view.upscale());
    let mut first = 0;
    for (index, (title, count, columns)) in ZOOM_SECTIONS.into_iter().enumerate() {
        heading(pass, ui, title, index == 0);
        let choices = &ZOOM_CHOICES[first..first + count];
        first += count;
        egui::Grid::new(title)
            .spacing(vec2(MENU_GAP, MENU_GAP))
            .show(ui, |ui| {
                for (place, choice) in choices.iter().enumerate() {
                    let active = choice.active(fit, zoom, upscale);
                    let response = match choice {
                        ZoomChoice::Scale(scale) => {
                            ui.add_sized(MENU_CELL, Button::new(percent(*scale)).selected(active))
                        }
                        // In marks, where the filters below are in words.
                        ZoomChoice::Fit(fit) => fit_cell(pass, ui, *fit, fills, active),
                        // The two filters are not a direction or a size, and
                        // there is no picture of "bicubic" a reader would
                        // arrive at unaided, so their cells are cut wider
                        // and say so.
                        ZoomChoice::Filter(filter) => ui.add_sized(
                            [MENU_WORD_CELL, MENU_CELL[1]],
                            Button::new(filter.label()).selected(active),
                        ),
                    };
                    let control = Control::ZoomTo(*choice);
                    let response = pass.tooltip(response, Tip::Control(control), true);
                    if response.clicked() {
                        pass.press(control);
                        ui.close();
                    }
                    if (place + 1) % columns == 0 {
                        ui.end_row();
                    }
                }
            });
    }
}

/// A fit cell: the four corners of `expand` for the fit that takes the whole
/// image in, and a pair of chevrons pushed apart for the fit that fills the
/// window — pointing the way that one actually fills.
fn fit_cell(pass: &mut Pass, ui: &mut Ui, fit: Fit, fills: Axis, active: bool) -> egui::Response {
    let marks = match (fit, fills) {
        (Fit::Whole, _) => icon::EXPAND,
        (Fit::Fill, Axis::Across) => icon::CHEVRONS_LEFT_RIGHT,
        (Fit::Fill, Axis::Down) => icon::CHEVRONS_UP_DOWN,
    };
    let (rect, response) = ui.allocate_exact_size(vec2(MENU_CELL[0], MENU_CELL[1]), Sense::CLICK);
    let (background, ink) = pass.button_ink(active, &response, true);
    ui.painter().rect_filled(rect, TOGGLE_RADIUS, background);
    icon::paint(
        ui.painter(),
        marks,
        icon::square(icon::Grid::new(ui.pixels_per_point()), rect, FIT_ICON),
        ink,
        background,
    );
    response.widget_info(|| {
        WidgetInfo::selected(
            WidgetType::Button,
            true,
            active,
            Control::ZoomTo(ZoomChoice::Fit(fit)).label(),
        )
    });
    response
}

/// The pixel-format menu: the three formats abreast, in cells cut for
/// words, under the one heading that says what the menu is of — it hangs
/// from a dot rather than from a word.
pub(super) fn pixel_cells(pass: &mut Pass, ui: &mut Ui) {
    ui.spacing_mut().item_spacing = Vec2::ZERO;
    heading(pass, ui, "Pixel value", true);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = vec2(MENU_GAP, 0.0);
        for format in PixelFormat::ALL {
            let active = format == pass.panels.pixel_format;
            let response = ui.add_sized(
                [MENU_WORD_CELL, MENU_CELL[1]],
                Button::new(format.label()).selected(active),
            );
            let control = Control::Format(format);
            let response = pass.tooltip(response, Tip::Control(control), true);
            if response.clicked() {
                pass.press(control);
                ui.close();
            }
        }
    });
}

/// The menu of copies: one item for everything that can be taken, each
/// wearing the name of the thing it takes and, beside it, the key that takes
/// the same thing. Never lit: a copy is something done, and there is no
/// state for an item to be showing.
pub(super) fn copy_items(pass: &mut Pass, ui: &mut Ui) {
    for copies in Copies::ALL {
        let control = Control::Copies(copies);
        // The picture's item takes the region while one is selected, as the
        // key does, and says so.
        let label = match copies {
            Copies::Image if pass.input.selection.region().is_some() => "Region",
            _ => copies.label(),
        };
        let mut button = Button::new(label);
        if let Some(key) = pass.namer.shortcut(control) {
            button = button.shortcut_text(key);
        }
        let response = ui.add(button);
        let response = pass.tooltip(response, Tip::Control(control), true);
        if response.clicked() {
            pass.press(control);
        }
    }
}

/// The menu of other programs: one item for each application the desktop
/// says can open a file of this kind, wearing the name that application
/// calls itself by.
///
/// No key beside any of them, unlike the menu of copies above: what is on
/// this menu is whatever the desktop has installed, and there is nothing for
/// a key table to have bound. An item is asked for by its place in the list
/// rather than by name — the application is the one that knows what each one
/// runs.
pub(super) fn open_items(pass: &mut Pass, ui: &mut Ui) {
    // Each item is sized to the name on it, rather than the name being
    // fitted into whatever width the popup opened at. The zoom and format
    // menus are laid out from cells of a size this file chose, and the menu
    // of copies prints a key after every item, which is what gives that one
    // its width; this one is a list of words nobody here chose the length of,
    // and left to wrap they come out a letter to a line in a column as narrow
    // as the button that opened it. How long a name may be is
    // `openers::MAX_NAME`.
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
    let openers = pass.input.openers.as_slice();
    for (index, name) in openers.iter().enumerate() {
        let control = Control::OpenIn(index);
        let response = ui.add(Button::new(name));
        let response = pass.tooltip(response, Tip::Control(control), true);
        if response.clicked() {
            pass.press(control);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: [f32; 2] = [1000.0, 700.0];

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
                .map(|(_, count, _)| count)
                .sum::<usize>(),
            ZOOM_CHOICES.len()
        );

        let mut first = 0;
        for (index, (title, count, columns)) in ZOOM_SECTIONS.iter().enumerate() {
            assert!(count % columns == 0, "{title} fills its rows");
            for choice in &ZOOM_CHOICES[first..first + count] {
                assert_eq!(section_of(*choice), index, "{choice:?} in {title}");
            }
            first += count;
        }

        // And the up-scaling section is exactly the filters on offer, in the
        // order the key cycles them.
        let up_scaling = &ZOOM_CHOICES[ZOOM_CHOICES.len() - ZOOM_SECTIONS[2].1..];
        let filters: Vec<Upscale> = up_scaling
            .iter()
            .map(|choice| match choice {
                ZoomChoice::Filter(filter) => *filter,
                other => panic!("{other:?} is not a filter"),
            })
            .collect();
        assert_eq!(filters, Upscale::ALL);
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

    /// Every cell of the zoom menu has words of its own for the tooltip: the
    /// key steps through them all and so describes none of them.
    #[test]
    fn every_zoom_choice_describes_itself() {
        for choice in ZOOM_CHOICES {
            assert!(!choice.describe().is_empty(), "{choice:?}");
            assert!(!choice.label().is_empty(), "{choice:?}");
        }
        for format in PixelFormat::ALL {
            assert!(!describe_format(format).is_empty(), "{format:?}");
        }
    }
}
