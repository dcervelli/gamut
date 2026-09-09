//! The theme's roles, handed to egui as the style it draws every widget in.
//!
//! [`Theme`] stays the source of truth — it is what the
//! desktop's palette resolves to, and what the few things drawn by hand read
//! their inks from — and this is the translation of its roles into egui's
//! `Visuals`, applied once at startup and again whenever the palette changes.

use std::collections::BTreeMap;

use egui::{
    Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, Style, TextStyle, Vec2,
    Visuals,
    style::{ScrollStyle, WidgetVisuals},
};

use crate::render::Color;
use crate::theme::{Mode, Theme};

use super::TEXT_SIZE;
use super::chrome::{BUTTON_GAP, BUTTON_SIZE};

/// The corner radius of a toggle, a cell, and everything else pressed.
pub(super) const TOGGLE_RADIUS: f32 = 5.0;
/// The corner radius of a popup's panel.
pub(super) const MENU_RADIUS: f32 = 8.0;
/// How far a floating panel's corners are rounded.
pub(super) const PANEL_RADIUS: f32 = 6.0;
/// What a popup keeps between its edge and its cells.
pub(super) const MENU_PADDING: f32 = 8.0;
/// The alpha the accent is washed to under a toggle that is on: enough to
/// say so, not enough to hide the mark on it.
pub(super) const ACTIVE_BUTTON_WASH: u8 = 64;
/// How much of the ink is left on a control that is not taking presses.
/// Enough to read the mark, little enough to read as a control that is not
/// answering.
pub(super) const DEAD_BUTTON_INK: u8 = 90;
/// The same, as the fraction egui fades a disabled widget by.
const DISABLED_ALPHA: f32 = DEAD_BUTTON_INK as f32 / 255.0;
/// The scrollbar the information panel shows: a thin bar in a gutter of its
/// own, and a thumb never too short to take hold of.
const SCROLLBAR_WIDTH: f32 = 3.0;
const THUMB_MIN: f32 = 24.0;
/// How long the pointer rests on a control before it is named, and how long
/// after leaving one the next is named at once.
const TOOLTIP_DELAY: f32 = 0.5;
const TOOLTIP_GRACE: f32 = 0.3;

impl From<Color> for Color32 {
    fn from(color: Color) -> Self {
        Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a)
    }
}

/// Puts `theme` on `ctx`: every widget drawn from here on wears it.
pub fn apply(ctx: &egui::Context, theme: &Theme) {
    ctx.set_global_style(style(theme));
}

/// The style `theme` resolves to.
pub fn style(theme: &Theme) -> Style {
    let mut style = Style {
        visuals: visuals(theme),
        ..Default::default()
    };

    let mut text_styles = BTreeMap::new();
    text_styles.insert(
        TextStyle::Body,
        FontId::new(TEXT_SIZE, FontFamily::Proportional),
    );
    text_styles.insert(
        TextStyle::Button,
        FontId::new(TEXT_SIZE, FontFamily::Proportional),
    );
    text_styles.insert(
        TextStyle::Small,
        FontId::new(TEXT_SIZE * 0.85, FontFamily::Proportional),
    );
    text_styles.insert(
        TextStyle::Monospace,
        FontId::new(TEXT_SIZE, FontFamily::Monospace),
    );
    text_styles.insert(
        TextStyle::Heading,
        FontId::new(TEXT_SIZE, FontFamily::Name(super::fonts::BOLD.into())),
    );
    style.text_styles = text_styles;

    style.spacing.item_spacing = Vec2::new(BUTTON_GAP, 4.0);
    style.spacing.button_padding = Vec2::new(6.0, 3.0);
    style.spacing.interact_size = Vec2::splat(BUTTON_SIZE);
    style.spacing.menu_margin = Margin::same(MENU_PADDING as i8);
    style.spacing.window_margin = Margin::same(MENU_PADDING as i8);
    style.spacing.tooltip_width = 400.0;
    style.spacing.scroll = ScrollStyle {
        floating: false,
        bar_width: SCROLLBAR_WIDTH,
        handle_min_length: THUMB_MIN,
        bar_inner_margin: 0.0,
        bar_outer_margin: 0.0,
        ..ScrollStyle::solid()
    };

    style.interaction.tooltip_delay = TOOLTIP_DELAY;
    style.interaction.tooltip_grace_time = TOOLTIP_GRACE;
    style.interaction.show_tooltips_only_when_still = true;
    style.interaction.selectable_labels = false;
    style
}

/// The colors of `theme`, in egui's roles.
fn visuals(theme: &Theme) -> Visuals {
    let mut visuals = match theme.mode {
        Mode::Dark => Visuals::dark(),
        Mode::Light => Visuals::light(),
    };
    let bar: Color32 = theme.bar_background.into();
    let border: Color32 = theme.border.into();
    let primary: Color32 = theme.text_primary.into();
    let dim: Color32 = theme.text_dim.into();
    let accent: Color32 = theme.accent.into();
    let idle: Color32 = theme.button_idle.into();
    let hover: Color32 = theme.button_hover.into();
    let active = theme.accent.with_alpha(ACTIVE_BUTTON_WASH).into();

    visuals.panel_fill = bar;
    visuals.window_fill = theme.menu_background.into();
    visuals.faint_bg_color = bar;
    visuals.extreme_bg_color = border;
    visuals.window_stroke = Stroke::new(1.0, border);
    visuals.window_shadow = Shadow::NONE;
    visuals.popup_shadow = Shadow::NONE;
    visuals.window_corner_radius = CornerRadius::same(PANEL_RADIUS as u8);
    visuals.menu_corner_radius = CornerRadius::same(MENU_RADIUS as u8);
    visuals.window_highlight_topmost = false;

    visuals.override_text_color = None;
    visuals.weak_text_color = Some(dim);
    visuals.hyperlink_color = accent;
    visuals.warn_fg_color = theme.caution.into();
    visuals.error_fg_color = theme.warning.into();
    visuals.selection.bg_fill = active;
    visuals.selection.stroke = Stroke::new(1.0, accent);
    visuals.button_frame = true;
    visuals.disabled_alpha = DISABLED_ALPHA;

    let corner = CornerRadius::same(TOGGLE_RADIUS as u8);
    let widget = |bg: Color32, ink: Color32, stroke: Color32| WidgetVisuals {
        bg_fill: bg,
        weak_bg_fill: bg,
        bg_stroke: Stroke::new(1.0, stroke),
        corner_radius: corner,
        fg_stroke: Stroke::new(1.0, ink),
        expansion: 0.0,
    };
    visuals.widgets.noninteractive = widget(bar, dim, border);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, border);
    visuals.widgets.inactive = widget(idle, primary, Color32::TRANSPARENT);
    visuals.widgets.hovered = widget(hover, primary, Color32::TRANSPARENT);
    visuals.widgets.active = widget(hover, primary, accent);
    visuals.widgets.open = widget(active, primary, Color32::TRANSPARENT);
    visuals
}
