//! The square toggles in the side panels, and the outline primitive shared
//! with the minimap.

use crate::render::{Color, Rect, TextMeasure, UiFrame};
use crate::theme::Theme;

use super::TEXT_SIZE;
use super::chrome::{BUTTON_SIZE, OUTPUT_BUTTON, READING_GAP, ZOOM_BUTTON, grid_width};
use super::icon;
use super::menu::CELL_RADIUS;

/// How much of the accent is left behind a switched-on toggle. Enough to read
/// as lit from across the window, little enough that the icon on top of it
/// stays the thing being looked at.
const ACTIVE_BUTTON_WASH: u8 = 64;

/// The corner radius of a side-panel toggle.
const TOGGLE_RADIUS: f32 = 5.0;
/// The room set aside for a toggle's mark: what [`icon::fit`] is given to
/// size a square out of, not the size it comes back with. The side panels are
/// a bar's thickness wide and the buttons fill them, so what this is set
/// against is legibility at that size rather than the button — three logical
/// pixels of air is enough to keep a mark off the button's rounded corners,
/// and every pixel beyond that is one the mark does not have.
pub(super) const ICON_SIDE: f32 = BUTTON_SIZE - 6.0;

/// The histogram toggle: a miniature of what it shows, rather than a letter,
/// since the side panels are too narrow to label anything in words.
pub(super) fn histogram_button(
    frame: &mut UiFrame,
    rect: Rect,
    active: bool,
    hover: bool,
    theme: &Theme,
) {
    toggle(frame, rect, icon::CHART_AREA, active, hover, theme);
}

/// The info toggle. The panel it opens is a column of words about the file
/// rather than a picture of anything the way the other two are, so the mark
/// for it is the one the rest of the world already uses for that.
pub(super) fn info_button(
    frame: &mut UiFrame,
    rect: Rect,
    active: bool,
    hover: bool,
    theme: &Theme,
) {
    toggle(frame, rect, icon::INFO, active, hover, theme);
}

/// The minimap toggle: the panel itself in miniature, a frame for the whole
/// image with a smaller view of it inside.
pub(super) fn minimap_button(
    frame: &mut UiFrame,
    rect: Rect,
    active: bool,
    hover: bool,
    theme: &Theme,
) {
    toggle(frame, rect, icon::SQUARE_SQUARE, active, hover, theme);
}

/// The button that opens the menu of copies, under the minimap toggle. Lit
/// while that menu is open, the way the zoom readout is: it opens a panel
/// rather than switching anything on, so there is no other state for it to be
/// showing.
pub(super) fn copy_button(frame: &mut UiFrame, rect: Rect, open: bool, hover: bool, theme: &Theme) {
    toggle(frame, rect, icon::COPY, open, hover, theme);
}

/// The paste button, under the copy button. Not a toggle: it does
/// something rather than switching something on, so it is never drawn lit —
/// there is no state for it to be showing. It is on screen only while there
/// is a picture on the clipboard to paste, which is what says a press on it
/// would do anything at all.
pub(super) fn paste_button(frame: &mut UiFrame, rect: Rect, hover: bool, theme: &Theme) {
    toggle(frame, rect, icon::CLIPBOARD, false, hover, theme);
}

/// The button at the head of the pixel readout, in the bottom bar: a ring
/// with a point at its center, for the one pixel the readout is about. Lit
/// while the menu it opens is open, the way the zoom readout is — it chooses
/// how the value beside it is written rather than switching anything on, so
/// there is no other state for it to be showing.
pub(super) fn pixel_button(
    frame: &mut UiFrame,
    rect: Rect,
    open: bool,
    hover: bool,
    theme: &Theme,
) {
    toggle(frame, rect, icon::CIRCLE_DOT, open, hover, theme);
}

/// One square toggle in a side panel: the button, and the mark it wears.
///
/// A window too small to hold the button gets no button, rather than a smear
/// of a mark drawn into less room than its own strokes need.
fn toggle(
    frame: &mut UiFrame,
    rect: Rect,
    marks: &[icon::Mark],
    active: bool,
    hover: bool,
    theme: &Theme,
) {
    if rect.width < BUTTON_SIZE {
        return;
    }
    let (background, ink) = button_ink(active, hover, theme);
    frame.rounded_rect(rect, TOGGLE_RADIUS, background);
    // No knockout in any of these three, so the ground goes unused; the
    // button's own is what one would cover anyway.
    icon::draw(
        frame,
        marks,
        icon::fit(frame, rect, ICON_SIDE),
        ink,
        background,
    );
}

/// A toggle's background and ink. Active outranks hover: what is on says more
/// than what the pointer happens to be over.
pub(super) fn button_ink(active: bool, hover: bool, theme: &Theme) -> (Color, Color) {
    match (active, hover) {
        (true, _) => (theme.accent.with_alpha(ACTIVE_BUTTON_WASH), theme.accent),
        (false, true) => (theme.button_hover, theme.text_primary),
        (false, false) => (theme.button_idle, theme.text_dim),
    }
}

/// A rectangle drawn as four edges. What is behind an outline stays visible,
/// which is the whole point for anything laid over the minimap: the thumbnail
/// under it belongs to the image layer, and a filled quad would hide it.
///
/// Four snapped lines, so that an outline is the same weight as itself
/// wherever on the device's grid it lands, and — at the one pixel most of
/// them ask for — the same weight as the rules that part the bars from the
/// content and one section of the info panel from the next.
pub(super) fn outline(frame: &mut UiFrame, rect: Rect, thickness: f32, color: Color) {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }
    let edge = frame.line_width(thickness);
    // The two down the sides stop where the two across meet them, so that a
    // color with anything less than full alpha is not laid twice at the
    // corners and drawn darker there.
    let middle = (rect.height - 2.0 * edge).max(0.0);
    frame.line(
        Rect::new(rect.x, rect.y, rect.width, edge),
        thickness,
        color,
    );
    frame.line(
        Rect::new(rect.x, rect.bottom() - edge, rect.width, edge),
        thickness,
        color,
    );
    frame.line(
        Rect::new(rect.x, rect.y + edge, edge, middle),
        thickness,
        color,
    );
    frame.line(
        Rect::new(rect.right() - edge, rect.y + edge, edge, middle),
        thickness,
        color,
    );
}

/// The zoom readout, drawn as the button it is: what the view is doing now,
/// and one press from a menu of what it could be doing instead. Lit while
/// that menu is open, the way a toggle is lit while it is on.
pub(super) fn zoom_button(
    frame: &mut UiFrame,
    text: &mut dyn TextMeasure,
    rect: Rect,
    zoom: f32,
    open: bool,
    hover: bool,
    theme: &Theme,
) {
    // As with the toggles: a window too narrow for the whole button gets no
    // button rather than a label spilling out of one.
    if rect.width < ZOOM_BUTTON[0] || rect.height < ZOOM_BUTTON[1] {
        return;
    }
    let (background, ink) = button_ink(open, hover, theme);
    frame.rounded_rect(rect, CELL_RADIUS, background);
    centered_text(frame, text, rect, ink, &percent(zoom));
}

/// How much of the ink is left on the surface switch when there is nothing
/// to switch to. Enough to read the word, little enough to read as a control
/// that is not taking presses.
const DEAD_BUTTON_INK: u8 = 90;

/// The headroom switch: one word, lit while the picture is going out with
/// room above white. A toggle rather than a readout, since whether the
/// picture uses the room is the viewer's to choose — where there is any: the
/// driver offers an HDR color space and the monitor is not in SDR mode.
/// Where there is none, the button is drawn dead rather than left out: a
/// control that is sometimes there is a control that has to be found again.
pub(super) fn output_button(
    frame: &mut UiFrame,
    text: &mut dyn TextMeasure,
    rect: Rect,
    hdr: bool,
    available: bool,
    hover: bool,
    theme: &Theme,
) {
    if rect.width < OUTPUT_BUTTON[0] || rect.height < OUTPUT_BUTTON[1] {
        return;
    }
    let (background, ink) = if available {
        button_ink(hdr, hover, theme)
    } else {
        (
            theme.button_idle,
            theme.text_dim.with_alpha(DEAD_BUTTON_INK),
        )
    };
    frame.rounded_rect(rect, CELL_RADIUS, background);
    centered_text(frame, text, rect, ink, "HDR");
}

/// Side of the grid mark. A side toggle's, since the grid toggle is the same
/// size as one and the two meet in the corner of the window, where a mark
/// drawn at a second size would read as a mistake.
const GRID_ICON: f32 = ICON_SIDE;

/// The grid toggle: the icon always, and — while the grid is on — how far
/// apart its lines are, in `spacing`. The reading is worth the room because
/// the spacing follows the zoom rather than being chosen, so a grid whose
/// size is not stated is a grid that cannot be measured with; switched off
/// there is no spacing in force, and the icon says the rest.
///
/// `rect` is fitted to the reading by [`grid_width`], which is also what the
/// pointer is answered against.
pub(super) fn grid_button(
    frame: &mut UiFrame,
    text: &mut dyn TextMeasure,
    rect: Rect,
    spacing: Option<&str>,
    hover: bool,
    theme: &Theme,
) {
    // As with the toggles: a window too narrow for the whole button gets no
    // button rather than a label spilling out of one.
    if rect.width < grid_width(spacing) || rect.height < BUTTON_SIZE {
        return;
    }
    let (background, ink) = button_ink(spacing.is_some(), hover, theme);
    frame.rounded_rect(rect, CELL_RADIUS, background);

    // The mark sits in the last button's width of the toggle, whether or not
    // there is a reading in front of it. The button grows leftwards to make
    // room for one, so anchoring the mark to the right keeps it exactly where
    // it was — over the column of side-panel toggles it shares a corner of
    // the window with, and in the same place from one press to the next. A
    // mark that moved every time the toggle was pressed, or every time the
    // zoom put another digit in the reading, would be a mark you had to find
    // again.
    let mark = Rect::new(rect.right() - BUTTON_SIZE, rect.y, BUTTON_SIZE, rect.height);
    if let Some(spacing) = spacing {
        // Set against the mark rather than centered in what is left of the
        // button: the number and the icon are one reading, and a number half
        // a button clear of the mark it is qualifying reads as two things
        // sharing a button rather than as a label with a mark after it. What
        // the number does not use of the room [`grid_width`] gave it is left
        // in front of it, where it is the button's own padding.
        let width = text.measure_text(spacing, TEXT_SIZE)[0];
        frame.text(
            [
                frame.snap(mark.x - READING_GAP - width),
                text_top(frame, text, rect, TEXT_SIZE),
            ],
            TEXT_SIZE,
            ink,
            spacing,
        );
    }
    icon::draw(
        frame,
        icon::GRID_3X3,
        icon::fit(frame, mark, GRID_ICON),
        ink,
        background,
    );
}

pub(super) fn percent(zoom: f32) -> String {
    format!("{:.0}%", zoom * 100.0)
}

/// Draws `label` centered in `rect`, the way a button wears its label.
///
/// Leveled on its capitals rather than on the box its line is laid out in:
/// that box keeps room under the baseline for descenders, so centering it puts
/// a label with none — a percentage, a count of pixels — visibly low against
/// whatever sits beside it.
///
/// On the device's grid, since a glyph laid out on part of a pixel is a
/// blurred glyph.
pub(super) fn centered_text(
    frame: &mut UiFrame,
    text: &mut dyn TextMeasure,
    rect: Rect,
    color: Color,
    label: &str,
) {
    let width = text.measure_text(label, TEXT_SIZE)[0];
    frame.text(
        [
            frame.snap(rect.x + (rect.width - width) / 2.0),
            text_top(frame, text, rect, TEXT_SIZE),
        ],
        TEXT_SIZE,
        color,
        label,
    );
}

/// Where a run at `size` starts for its capitals to sit level in `rect`.
pub(super) fn text_top(frame: &UiFrame, text: &mut dyn TextMeasure, rect: Rect, size: f32) -> f32 {
    frame.snap(rect.y + rect.height / 2.0 - text.cap_center(size))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::ui_tests::test_fonts;

    /// The least and the most the room [`grid_width`] sets aside may exceed
    /// the reading it is for, in logical pixels: enough that the number is
    /// not set against the front of the button, and little enough that the
    /// button is not padded out of proportion to the mark at the other end
    /// of it.
    const PADDING: std::ops::RangeInclusive<f32> = 4.0..=16.0;

    /// Every reading the grid toggle can show fits the room the button sets
    /// aside for it, with the button's padding left over and no more.
    ///
    /// The room is counted in digits, the pointer having to be answered where
    /// there are no fonts to ask. This is what holds that count to the face
    /// the bar is actually set in: too mean and the number runs off the front
    /// of the button, too generous and the reading drifts away from the mark
    /// it belongs to.
    #[test]
    fn every_spacing_fits_the_grid_button() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        for spacing in ["1 px", "20 px", "500 px", "5000 px", "10000 px"] {
            let width = fonts.measure_text(spacing, TEXT_SIZE)[0];
            let room = grid_width(Some(spacing)) - BUTTON_SIZE - READING_GAP;
            assert!(
                PADDING.contains(&(room - width)),
                "\"{spacing}\" is {width} wide in a button leaving {room} for it"
            );
        }
    }
}
