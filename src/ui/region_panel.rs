//! The panel a region brings up while one is asked for or drawn, at the
//! top left of the content area beside the region button: the cross that
//! takes the region off, its copy and the window fitted to it at its head;
//! under that the aspect it is held to, typed or chosen from a menu, and
//! the swap of its width and height; and under that its place and size in
//! what the picture is measured in — its pixels, a print's inches or
//! millimeters, or the ground's coordinates, as the grid is ruled — each a
//! box that can be typed in.
//!
//! Up as soon as a region is asked for, so that its shape can be chosen
//! before it is drawn; what needs a region is dead until there is one.
//!
//! A box keeps what is typed in it to itself, in egui's memory, for as long
//! as it has the keyboard, and hands it over when it lets go — `Enter`,
//! `Tab` or a click elsewhere — so that a ratio or a size typed a digit at
//! a time does not reshape the region at every digit. `Esc` lets go and
//! throws what was typed away.

use egui::{Align, Key, Layout, Modifiers, RichText, Sense, Ui, vec2};

use super::chrome::{BUTTON_SIZE, Corners, Pass};
use super::control::{Command, Control};
use super::info::HEADER_GAP;
use super::rename::{Boxed, boxed_field, dialog_frame};
use super::style::MENU_PADDING;
use super::{Current, RULE_WIDTH, Rect, TEXT_SIZE, fonts, help, icon, menu, panel};
use crate::image::aspect::Aspect;
use crate::image::region::Region;

use super::ruler::Ruler;

/// How wide the panel is inside its padding.
const INSIDE: f32 = 204.0;
/// How tall each row under the head is: a box's height.
const ROW: f32 = 24.0;
/// Between the rows, and between a word and its box.
const GAP: f32 = 6.0;
/// Between the two boxes of a row, and the buttons at the head.
const APART: f32 = 12.0;
/// The word before the aspect's box.
const ASPECT_WORD: f32 = 44.0;
/// The letter before each of the four boxes.
const LETTER: f32 = 12.0;
/// About as tall as the title's line, for the room the panel asks for:
/// the line itself is the face's, known only once the fonts are.
const HEAD: f32 = 18.0;

/// One of the four boxes the region's place and size are typed in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    X,
    Y,
    Width,
    Height,
}

impl Field {
    /// The two rows, as they stand.
    const ROWS: [[Field; 2]; 2] = [[Field::X, Field::Y], [Field::Width, Field::Height]];

    /// The letter before the box.
    fn letter(self) -> &'static str {
        match self {
            Field::X => "X",
            Field::Y => "Y",
            Field::Width => "W",
            Field::Height => "H",
        }
    }

    /// The box's name in egui's memory.
    fn name(self) -> &'static str {
        match self {
            Field::X => "x",
            Field::Y => "y",
            Field::Width => "width",
            Field::Height => "height",
        }
    }

    /// Which axis it is along: 0 across, 1 down.
    pub fn axis(self) -> usize {
        match self {
            Field::X | Field::Width => 0,
            Field::Y | Field::Height => 1,
        }
    }

    /// Whether it is a size rather than a place.
    pub fn is_size(self) -> bool {
        matches!(self, Field::Width | Field::Height)
    }

    /// What it says of `region` measured by `ruler`: the place of its top
    /// or left edge, or its width or height, as the region's own labels
    /// write them.
    fn of(self, region: Region, ruler: &Ruler) -> String {
        let axis = self.axis();
        let [place, size] = match axis {
            0 => [region.x, region.width],
            _ => [region.y, region.height],
        }
        .map(f64::from);
        match self.is_size() {
            false => ruler.write(ruler.at(axis, place)),
            true => ruler.write(ruler.length(axis, size)),
        }
    }

    /// What is typed, measured by `ruler`, in the picture's pixels: a place
    /// is the nearest pixel edge to it, and before the first edge is the
    /// first; a size is the nearest whole number of pixels to it, and one
    /// at least. `None` for what is not a number, or a size that is not
    /// more than nothing.
    fn read(self, typed: &str, ruler: &Ruler) -> Option<u32> {
        let value = ruler.read(typed)?;
        let axis = self.axis();
        let pixels = match self.is_size() {
            false => ruler.pixels(axis, value).round().max(0.0),
            true if value > 0.0 => (value / ruler.per_pixel(axis)).round().max(1.0),
            true => return None,
        };
        pixels
            .is_finite()
            .then(|| pixels.min(f64::from(u32::MAX)) as u32)
    }
}

/// The panel's id in egui's memory, which its boxes are kept under.
pub fn id() -> egui::Id {
    egui::Id::new("region panel")
}

/// Where the panel goes on `content`, or `None` where there is no room for
/// it whole.
pub fn panel(content: Rect) -> Option<Rect> {
    let size = [
        INSIDE + 2.0 * MENU_PADDING,
        HEAD + HEADER_GAP + 3.0 * ROW + 2.0 * GAP + 2.0 * MENU_PADDING,
    ];
    panel::fit(content, size, size, panel::Place::TopLeft)
}

/// Draws the panel, while a region is asked for or drawn.
pub(super) fn show(pass: &mut Pass, ui: &mut Ui, current: &Current) {
    if !pass.input.selection.is_on() {
        return;
    }
    let Some(rect) = panel(pass.content) else {
        return;
    };
    let region = pass.input.selection.region();
    let image = current.pixels();
    let ruler = pass.ruler(current);
    panel::area("region panel", rect, egui::Order::Middle).show(ui.ctx(), |ui| {
        dialog_frame(pass).show(ui, |ui| {
            ui.set_width(INSIDE);
            // The head on the band a menu's title sits on, the same height
            // and over the same hairline, painted under it once laid out.
            let band = ui.painter().add(egui::Shape::Noop);
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            let head = head(pass, ui, region.is_some());
            ui.add_space((HEADER_GAP - RULE_WIDTH) / 2.0);
            let edge = pass.grid.line_width(RULE_WIDTH);
            let (rule, _) = ui.allocate_exact_size(vec2(INSIDE, edge), Sense::HOVER);
            ui.add_space((HEADER_GAP - RULE_WIDTH) / 2.0);
            ui.spacing_mut().item_spacing.y = GAP;
            let inside = egui::Rect::from_x_y_ranges(ui.max_rect().x_range(), head.y_range());
            help::band(pass, ui, band, inside, rule.top());
            ui.painter().rect_filled(
                egui::Rect::from_x_y_ranges(inside.expand(MENU_PADDING).x_range(), rule.y_range()),
                0.0,
                pass.theme.border,
            );
            aspect_row(pass, ui, region, image, &ruler);
            for row in Field::ROWS {
                field_row(pass, ui, row, region, &ruler);
            }
        });
    });
}

/// The panel's name at the left, and at the right the window fitted to the
/// region, its copy, and the cross that takes it off — on one line of the
/// title's own height, as a menu's title stands. The buttons are the
/// ordinary size, standing over the line into the padding above it and
/// the gap below: cut to the line, their marks are too small to read.
fn head(pass: &mut Pass, ui: &mut Ui, drawn: bool) -> egui::Rect {
    let font = egui::FontId::new(TEXT_SIZE, egui::FontFamily::Name(fonts::BOLD.into()));
    let line = ui.fonts_mut(|fonts| fonts.row_height(&font));
    // Exactly the line, where `horizontal` would make the row as tall as
    // an ordinary button.
    let row = vec2(INSIDE, line);
    ui.allocate_ui_with_layout(row, Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_size(row);
        ui.label(
            RichText::new(Control::Region.label())
                .font(font.clone())
                .color(pass.theme.text_primary),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing = vec2(GAP / 2.0, 0.0);
            let button = |pass: &mut Pass, ui: &mut Ui, marks, control, enabled| {
                // The line's height in the row, and the whole button in a
                // child of its own over it, which the row does not grow by.
                let (slot, _) = ui.allocate_exact_size(vec2(BUTTON_SIZE, line), Sense::HOVER);
                let square =
                    egui::Rect::from_center_size(slot.center(), vec2(BUTTON_SIZE, BUTTON_SIZE));
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(square));
                pass.toggle(&mut child, marks, control, false, enabled, Corners::All);
            };
            button(pass, ui, icon::X, Control::RegionClose, true);
            ui.add_space(APART - GAP / 2.0);
            button(pass, ui, icon::COPY, Control::RegionCopy, drawn);
            button(pass, ui, icon::SCAN, Control::RegionFit, drawn);
        });
    })
    .response
    .rect
}

/// The aspect: its word, the box it can be typed in, the menu of them at
/// the end of the box, and the swap.
fn aspect_row(
    pass: &mut Pass,
    ui: &mut Ui,
    region: Option<Region>,
    image: [u32; 2],
    ruler: &Ruler,
) {
    let aspect = pass.input.aspect;
    // The menu offers its ratios the way up the region is, or the aspect
    // where it holds one, both in what the picture is measured in.
    let portrait = match aspect {
        Aspect::Free => region.is_some_and(|region| {
            ruler.length(1, f64::from(region.height)) > ruler.length(0, f64::from(region.width))
        }),
        _ => aspect.portrait(image, ruler.shape()),
    };
    ui.horizontal(|ui| {
        ui.set_min_height(ROW);
        ui.spacing_mut().item_spacing = vec2(GAP, 0.0);
        word(ui, "Aspect", ASPECT_WORD);
        let width = INSIDE - ASPECT_WORD - 2.0 * BUTTON_SIZE - 3.0 * GAP;
        let typed = held_field(
            pass,
            ui,
            id().with("aspect"),
            &aspect.label(),
            vec2(width, ROW),
            None,
            |typed| Aspect::parse(typed).is_some(),
        );
        if let Some(chosen) = typed.as_deref().and_then(Aspect::parse) {
            pass.press(Control::Aspect(chosen));
        }
        ui.spacing_mut().item_spacing = vec2(GAP / 2.0, 0.0);
        pass.menu_button(
            ui,
            icon::CHEVRON_DOWN,
            Control::AspectMenu,
            egui::RectAlign::BOTTOM_END,
            true,
            |pass, ui| menu::aspect_items(pass, ui, aspect, portrait),
        );
        ui.add_space(GAP / 2.0);
        pass.toggle(
            ui,
            icon::ARROW_RIGHT_LEFT,
            Control::RegionSwap,
            false,
            region.is_some() || aspect != Aspect::Free,
            Corners::All,
        );
    });
}

/// Two of the region's numbers abreast, each a letter and a box wearing
/// the unit `ruler` measures in; dead, and empty, until there is a region
/// for them to be of.
///
/// A box is kept under its measure as well as its name, so that what was
/// being typed when the measure changed is thrown away rather than read in
/// a unit it was not typed in; and what is typed is handed over only where
/// it is not what the box said, so that a box let go of unchanged cannot
/// move the region by the rounding of its own words.
fn field_row(pass: &mut Pass, ui: &mut Ui, row: [Field; 2], region: Option<Region>, ruler: &Ruler) {
    ui.add_enabled_ui(region.is_some(), |ui| {
        ui.horizontal(|ui| {
            ui.set_min_height(ROW);
            ui.spacing_mut().item_spacing = vec2(GAP, 0.0);
            let width = (INSIDE - 2.0 * (LETTER + GAP) - APART) / 2.0;
            for (index, field) in row.into_iter().enumerate() {
                if index > 0 {
                    ui.add_space(APART - GAP);
                }
                word(ui, field.letter(), LETTER);
                let shown = region.map_or(String::new(), |region| field.of(region, ruler));
                let typed = held_field(
                    pass,
                    ui,
                    id().with((field.name(), format!("{:?}", ruler.unit))),
                    &shown,
                    vec2(width, ROW),
                    ruler.unit_word(),
                    |typed| field.read(typed, ruler).is_some(),
                );
                if let Some(number) = typed
                    .as_deref()
                    .filter(|typed| typed.trim() != shown)
                    .and_then(|typed| field.read(typed, ruler))
                {
                    pass.commands.push(Command::RegionField(field, number));
                }
            }
        });
    });
}

/// A word before a box, right against it, in a column `width` wide.
fn word(ui: &mut Ui, words: &str, width: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::HOVER);
    ui.put(
        rect,
        egui::Label::new(RichText::new(words).size(TEXT_SIZE)).selectable(false),
    );
}

/// A box that says `shown` until it is typed in, then what is typed for as
/// long as it has the keyboard, with `suffix` after it, outlined while that
/// is not what `valid` takes. Hands back what was typed on the pass it lets go of the keyboard
/// — by `Enter`, which it takes, or by `Tab` or a click elsewhere — and
/// nothing where it was `Esc` that took it, or where nothing was typed.
fn held_field(
    pass: &mut Pass,
    ui: &mut Ui,
    id: egui::Id,
    shown: &str,
    size: egui::Vec2,
    suffix: Option<&'static str>,
    valid: impl Fn(&str) -> bool,
) -> Option<String> {
    let ctx = ui.ctx().clone();
    let typed: Option<String> = ctx.data(|data| data.get_temp(id));
    let text = typed.clone().unwrap_or_else(|| shown.to_string());
    let refused = typed.as_deref().is_some_and(|typed| !valid(typed));
    let (output, changed) = boxed_field(
        pass,
        ui,
        Boxed {
            id,
            text: &text,
            refused,
            size,
            suffix,
        },
    );
    if let Some(changed) = changed {
        ctx.data_mut(|data| data.insert_temp(id, changed));
    }
    let focused = output.response.has_focus();
    if focused && ui.input_mut(|keys| keys.consume_key(Modifiers::NONE, Key::Enter)) {
        ui.memory_mut(|memory| memory.surrender_focus(id));
    } else if focused {
        return None;
    }
    let typed: Option<String> = ctx.data_mut(|data| data.remove_temp(id));
    let escaped = ui.input(|keys| keys.key_pressed(Key::Escape));
    typed.filter(|_| !escaped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::ruler::Unit;

    /// A box says what the region's labels say, and what it says reads
    /// back as the same pixels: on the ground, from the raster's corner,
    /// the northing of the top edge being the greater, and the height a
    /// length all the same.
    #[test]
    fn a_box_reads_back_what_it_says() {
        let ground = Ruler::test(
            [564_973.0, 5_219_037.0],
            [12.5, -12.5],
            Unit::Ground(Some("m")),
        );
        let region = Region {
            x: 8,
            y: 4,
            width: 100,
            height: 30,
        };
        let shown: Vec<String> = Field::ROWS
            .concat()
            .into_iter()
            .map(|field| field.of(region, &ground))
            .collect();
        assert_eq!(shown, ["565073", "5218987", "1250", "375"]);
        for (field, shown) in Field::ROWS.concat().into_iter().zip(&shown) {
            let back = field.read(shown, &ground);
            let [place, size] = match field.axis() {
                0 => [region.x, region.width],
                _ => [region.y, region.height],
            };
            let want = if field.is_size() { size } else { place };
            assert_eq!(back, Some(want), "{field:?}");
        }
        // A place before the picture's first edge is that edge.
        assert_eq!(Field::X.read("500000", &ground), Some(0));
        assert_eq!(Field::Y.read("5300000 m", &ground), Some(0));
        assert_eq!(Field::Height.read("-5", &ground), None);
        assert_eq!(Field::X.read("east", &ground), None);
    }
}
