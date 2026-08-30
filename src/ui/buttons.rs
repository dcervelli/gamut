//! The square toggles in the side panels, and the outline primitive shared
//! with the minimap.

use crate::render::{Color, Rect, TextMeasure, UiFrame};
use crate::theme::Theme;

use super::TEXT_SIZE;
use super::chrome::{BUTTON_SIZE, ZOOM_BUTTON};
use super::menu::CELL_RADIUS;

/// How much of the accent is left behind a switched-on toggle. Enough to read
/// as lit from across the window, little enough that the icon on top of it
/// stays the thing being looked at.
const ACTIVE_BUTTON_WASH: u8 = 64;

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
    frame.rounded_rect(rect, 5.0, background);

    const BARS: [f32; 4] = [0.45, 1.0, 0.7, 0.3];
    let plot = rect.inset(9.0, 9.0);
    let step = plot.width / BARS.len() as f32;
    for (index, fraction) in BARS.iter().enumerate() {
        let height = plot.height * fraction;
        frame.rect(
            Rect::new(
                plot.x + index as f32 * step,
                plot.bottom() - height,
                (step - 1.5).max(1.0),
                height,
            ),
            ink,
        );
    }
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
    frame.rounded_rect(rect, 5.0, background);

    let icon = rect.inset(8.0, 10.0);
    outline(frame, icon, 1.5, ink);
    frame.rect(
        Rect::new(
            icon.x + 3.5,
            icon.y + 3.5,
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
pub(super) fn outline(frame: &mut UiFrame, rect: Rect, thickness: f32, color: Color) {
    let edge = thickness.min(rect.width / 2.0).min(rect.height / 2.0);
    if edge <= 0.0 {
        return;
    }
    let middle = rect.height - 2.0 * edge;
    frame.rect(Rect::new(rect.x, rect.y, rect.width, edge), color);
    frame.rect(
        Rect::new(rect.x, rect.bottom() - edge, rect.width, edge),
        color,
    );
    frame.rect(Rect::new(rect.x, rect.y + edge, edge, middle), color);
    frame.rect(
        Rect::new(rect.right() - edge, rect.y + edge, edge, middle),
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
