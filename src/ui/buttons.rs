//! The square toggles in the side panels, and the outline primitive shared
//! with the minimap.

use crate::render::{Color, Rect, TextMeasure, UiFrame};
use crate::theme::Theme;

use super::TEXT_SIZE;
use super::chrome::{BUTTON_SIZE, GRID_BUTTON_OFF, GRID_BUTTON_ON, ZOOM_BUTTON};
use super::menu::CELL_RADIUS;

/// How much of the accent is left behind a switched-on toggle. Enough to read
/// as lit from across the window, little enough that the icon on top of it
/// stays the thing being looked at.
const ACTIVE_BUTTON_WASH: u8 = 64;

/// The corner radius of a side-panel toggle.
const TOGGLE_RADIUS: f32 = 5.0;
/// What is left around a toggle's icon, across and down. The side panels are
/// a bar's thickness wide and the buttons fill them, so the icons are small:
/// what these are set against is legibility at that size, not the button.
const ICON_INSET: f32 = 6.0;

/// The histogram toggle: a miniature of what it shows, rather than a letter,
/// since the side panels are too narrow to label anything in words.
pub(super) fn histogram_button(
    frame: &mut UiFrame,
    rect: Rect,
    active: bool,
    hover: bool,
    theme: &Theme,
) {
    // A window too small to hold the button gets no button, rather than a
    // smear of sub-pixel bars.
    if rect.width < BUTTON_SIZE {
        return;
    }
    let (background, ink) = button_ink(active, hover, theme);
    frame.rounded_rect(rect, TOGGLE_RADIUS, background);

    const BARS: [f32; 4] = [0.45, 1.0, 0.7, 0.3];
    let plot = rect.inset(ICON_INSET, ICON_INSET);
    let step = plot.width / BARS.len() as f32;
    for (index, fraction) in BARS.iter().enumerate() {
        let height = plot.height * fraction;
        frame.rect(
            Rect::new(
                (plot.x + index as f32 * step).round(),
                plot.bottom() - height,
                (step - 1.0).max(1.0),
                height,
            ),
            ink,
        );
    }
}

/// The info toggle: the letter i, which is what the panel it opens is — a
/// column of words about the file, rather than a picture of anything the way
/// the other two toggles are.
pub(super) fn info_button(
    frame: &mut UiFrame,
    rect: Rect,
    active: bool,
    hover: bool,
    theme: &Theme,
) {
    if rect.width < BUTTON_SIZE {
        return;
    }
    let (background, ink) = button_ink(active, hover, theme);
    frame.rounded_rect(rect, TOGGLE_RADIUS, background);

    const STROKE: f32 = 3.0;
    const TITTLE_GAP: f32 = 2.5;
    let icon = rect.inset(0.0, ICON_INSET - 1.0);
    let x = (rect.x + (rect.width - STROKE) / 2.0).round();
    frame.rounded_rect(Rect::new(x, icon.y, STROKE, STROKE), STROKE / 2.0, ink);
    let stem = icon.y + STROKE + TITTLE_GAP;
    frame.rounded_rect(
        Rect::new(x, stem, STROKE, (icon.bottom() - stem).max(0.0)),
        STROKE / 2.0,
        ink,
    );
}

/// The minimap toggle: the panel itself in miniature, a frame for the image
/// with the viewport sitting in a corner of it.
pub(super) fn minimap_button(
    frame: &mut UiFrame,
    rect: Rect,
    active: bool,
    hover: bool,
    theme: &Theme,
) {
    if rect.width < BUTTON_SIZE {
        return;
    }
    let (background, ink) = button_ink(active, hover, theme);
    frame.rounded_rect(rect, TOGGLE_RADIUS, background);

    // Wider than it is tall, the way a window is, and so inset further across
    // than down.
    let icon = rect.inset(ICON_INSET - 1.0, ICON_INSET);
    outline(frame, icon, 1.0, ink);
    frame.rect(
        Rect::new(
            icon.x + 2.0,
            icon.y + 2.0,
            icon.width * 0.5,
            icon.height * 0.5,
        ),
        ink,
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
    // colour with anything less than full alpha is not laid twice at the
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
    centred_text(frame, text, rect, ink, &percent(zoom));
}

/// Side of the grid icon, and the room between it and the spacing beside it.
/// Smaller than the button it sits in by about what a side toggle's icon is
/// smaller than its own, so that the two read as the same weight where they
/// meet in the corner of the window — a lattice needs more room than a bar
/// chart to stay a lattice, which is why it is not simply the same size.
const GRID_ICON: f32 = 12.0;
const GRID_ICON_GAP: f32 = 7.0;

/// The grid toggle: the icon always, and — while the grid is on — how far
/// apart its lines are, in `spacing`. The reading is worth the room because
/// the spacing follows the zoom rather than being chosen, so a grid whose
/// size is not stated is a grid that cannot be measured with; switched off
/// there is no spacing in force, and the icon says the rest.
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
    let least = if spacing.is_some() {
        GRID_BUTTON_ON
    } else {
        GRID_BUTTON_OFF
    };
    if rect.width < least[0] || rect.height < least[1] {
        return;
    }
    let (background, ink) = button_ink(spacing.is_some(), hover, theme);
    frame.rounded_rect(rect, CELL_RADIUS, background);

    // Icon and reading are centred as one, so that the button reads as a
    // label with a mark after it rather than as two things in a row.
    //
    // The mark comes last so that it stays at the end of the bar as the
    // button grows leftwards to make room for the reading: the icon is what
    // says which toggle this is, and a toggle that swapped ends with its own
    // label every time it was pressed would be a toggle you had to find again.
    let label = spacing.map(|spacing| (spacing, text.measure_text(spacing, TEXT_SIZE)[0]));
    let width = GRID_ICON + label.map_or(0.0, |(_, width)| GRID_ICON_GAP + width);
    let x = (rect.x + (rect.width - width) / 2.0).round();

    if let Some((spacing, _)) = label {
        frame.text(
            [x, (rect.y + (rect.height - TEXT_SIZE * 1.3) / 2.0).round()],
            TEXT_SIZE,
            ink,
            spacing,
        );
    }
    grid_icon(
        frame,
        Rect::new(
            x + label.map_or(0.0, |(_, width)| width + GRID_ICON_GAP),
            (rect.y + (rect.height - GRID_ICON) / 2.0).round(),
            GRID_ICON,
            GRID_ICON,
        ),
        ink,
    );
}

/// The grid in miniature: a frame with two lines each way through it, which
/// is the smallest thing that reads as squares rather than as a hash.
fn grid_icon(frame: &mut UiFrame, rect: Rect, ink: Color) {
    const LINE: f32 = 1.0;
    outline(frame, rect, LINE, ink);
    for fraction in [1.0 / 3.0, 2.0 / 3.0] {
        frame.rect(
            Rect::new(
                (rect.x + rect.width * fraction).round(),
                rect.y,
                LINE,
                rect.height,
            ),
            ink,
        );
        frame.rect(
            Rect::new(
                rect.x,
                (rect.y + rect.height * fraction).round(),
                rect.width,
                LINE,
            ),
            ink,
        );
    }
}

pub(super) fn percent(zoom: f32) -> String {
    format!("{:.0}%", zoom * 100.0)
}

/// Draws `label` centred in `rect`, the way a button wears its label. Whole
/// logical pixels, since a glyph laid out on a half one is a blurred glyph.
pub(super) fn centred_text(
    frame: &mut UiFrame,
    text: &mut dyn TextMeasure,
    rect: Rect,
    color: Color,
    label: &str,
) {
    let width = text.measure_text(label, TEXT_SIZE)[0];
    frame.text(
        [
            (rect.x + (rect.width - width) / 2.0).round(),
            (rect.y + (rect.height - TEXT_SIZE * 1.3) / 2.0).round(),
        ],
        TEXT_SIZE,
        color,
        label,
    );
}
