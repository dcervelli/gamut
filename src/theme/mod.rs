//! Where the interface's colours come from.
//!
//! The window is a viewer, not a desktop: its chrome should disappear into
//! whatever the rest of the desktop looks like rather than announce itself in
//! one fixed grey. On Omarchy the active theme is materialised as a palette
//! file, so that is read once at startup and again whenever it changes, and
//! the interface's handful of roles — panel, hairline, text, accent — are
//! derived from it.
//!
//! Nothing here is required. With no palette to read, which is every machine
//! that is not running Omarchy, [`Theme::FALLBACK`] is used: the neutral dark
//! set the interface was designed in. The same set fills in for a palette too
//! sparse to answer, so a third-party theme that defines half a dozen keys
//! degrades to something wearable rather than to black on black.
//!
//! Reading and resolving the palette file is [`palette`]'s job; this module
//! is only the derivation of the interface's roles from what it hands back.

mod palette;

pub use palette::{Mode, Palette, watch};

use crate::image::stats::COLOUR;
use crate::render::Color;

use palette::{BLACK, mix};

/// Every colour the interface draws with.
///
/// One value per role rather than per widget: the histogram's axis label and
/// the minimap's outline are not separately themeable, they are "text on a
/// floating panel" and "a hairline over the image", and there are few enough
/// roles that a theme can be reasoned about whole.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Theme {
    /// Whether the palette reads light-on-dark or dark-on-light. The floating
    /// panels stay dark either way — see `panel_background` — so this is what
    /// tells the ink on them which way to go.
    pub mode: Mode,
    /// The four panels, and the window behind the image.
    pub bar_background: Color,
    /// The hairline along a panel's inner edge, and the other square of the
    /// checkerboard behind a transparent image.
    pub border: Color,
    /// The panel a popup's cells sit on. The bars' colour rather than the
    /// floating panel's, and for the same reason the cells are drawn like the
    /// toggles in the side panels: what is on a menu are buttons, and the ink
    /// buttons are drawn in is made to read against the bars. Near enough to
    /// opaque to be read through — a menu is what is being looked at while it
    /// is open — but not quite, so that it still reads as lying over the
    /// image rather than as another piece of the chrome.
    pub menu_background: Color,
    /// The floating histogram panel: dark whatever the mode, since the plot
    /// on it is drawn by screening the colour planes over one another and
    /// that only reads on a dark ground. Kept off full opacity so that what
    /// it is covering is still there, though how far off depends on the mode
    /// — a light page shows through far more of a given alpha than its bytes
    /// suggest, the blend being done in light.
    pub panel_background: Color,
    /// Ink on that panel, which is therefore light whatever the mode.
    pub panel_text: Color,
    pub button_idle: Color,
    pub button_hover: Color,
    /// Text in the bars: what the image is, and what is being done to it.
    pub text_primary: Color,
    pub text_dim: Color,
    /// What is switched on, and where the display window sits.
    pub accent: Color,
    /// The minimap's border, and the wash over the part of the image that is
    /// off screen. Both go over a thumbnail drawn by the image layer, so both
    /// stay translucent.
    pub minimap_edge: Color,
    pub minimap_dim: Color,
    /// The luminance plane, under the colour ones.
    pub histogram_luma: Color,
    /// Red, green and blue channel ink, in that order.
    pub histogram_planes: [Color; COLOUR],
}

/// How far the hairline is lifted off the panel colour when the theme has no
/// shade of its own to use. Enough to place an edge, little enough that the
/// eye does not keep going back to it.
const BORDER_LIFT: f32 = 0.15;
/// How far apart, summed over the three channels, two colours have to be
/// before one can be seen against the other. A theme whose `lighter_background`
/// resolves back to its `background` — which is what happens when it defines
/// neither — would otherwise draw the hairline invisibly.
const SEPARATION: u32 = 18;
/// How deep the floating panel goes on a light theme, mixed out of the
/// theme's own ink rather than out of its background, which is the wrong end
/// of the palette for a surface that has to stay dark.
const LIGHT_PANEL_DEPTH: f32 = 0.25;
/// How opaque a popup's panel is. Higher than the panels that float over the
/// image permanently: the picture coming through a menu competes with the
/// choices on it. Not mode-dependent the way the floating panel's alpha is,
/// the surface being the theme's own background either way round.
const MENU_ALPHA: u8 = 246;

/// How opaque that panel is, against the interface's usual alpha for it.
///
/// Higher because the interface's quads blend in light, not in encoded
/// values, and a light backdrop carries far more light than its byte suggests:
/// the interface's usual alpha over a near-white page leaves a dark panel
/// reading as a mid grey, which is not a ground a screened plot shows up on.
const LIGHT_PANEL_ALPHA: u8 = 242;

/// How far each channel plane is pulled towards its own primary before it is
/// balanced. A palette's red is a pastel with green and blue in it, and three
/// pastels screened together climb towards white; some of the theme's
/// character is traded here for the purity that leaves the overlaps readable.
const PLANE_PURITY: f32 = 0.4;
/// What the three planes screened together come to, as light. Mid grey:
/// bright enough that an overlap plainly is not one plane, dark enough that
/// it plainly is not white.
const PLANE_MIX: f32 = 0.5;
/// How many times the three are balanced against one another. Each pass
/// settles what the previous one's adjustments did to the other two channels,
/// and the correction is small by the third.
const PLANE_PASSES: usize = 4;

impl Theme {
    /// The neutral dark set the interface was designed in, and what is used
    /// where there is no palette to read.
    pub const FALLBACK: Theme = Theme {
        mode: Mode::Dark,
        bar_background: Color::rgb(18, 18, 22),
        border: Color::rgb(38, 38, 46),
        menu_background: Color::rgba(18, 18, 22, MENU_ALPHA),
        panel_background: Color::rgba(12, 12, 16, 214),
        panel_text: Color::rgb(150, 152, 160),
        button_idle: Color::rgba(255, 255, 255, 20),
        button_hover: Color::rgba(255, 255, 255, 45),
        text_primary: Color::rgb(238, 238, 238),
        text_dim: Color::rgb(150, 152, 160),
        accent: Color::rgb(120, 180, 255),
        minimap_edge: Color::rgba(255, 255, 255, 70),
        minimap_dim: Color::rgba(6, 6, 10, 150),
        histogram_luma: Color::rgba(150, 152, 160, 200),
        histogram_planes: [
            Color::rgb(184, 44, 44),
            Color::rgb(44, 170, 52),
            Color::rgb(52, 100, 186),
        ],
    };

    /// The theme to draw with: the desktop's, where there is one to read.
    pub fn detect() -> Theme {
        match Palette::load() {
            Some(palette) => Theme::from_palette(&palette),
            None => Theme::FALLBACK,
        }
    }

    /// Derives the interface's roles from a palette.
    ///
    /// A palette with no background or no foreground is not one anything can
    /// be built on — every role below is a shade of one or the other — so it
    /// is left alone entirely rather than half-applied over the fallback,
    /// which is what would put light text on a light panel.
    pub fn from_palette(palette: &Palette) -> Theme {
        let (Some(background), Some(foreground)) =
            (palette.color("background"), palette.color("foreground"))
        else {
            return Theme::FALLBACK;
        };
        let bright = palette.color("bright_foreground").unwrap_or(foreground);
        let accent = palette
            .color("accent")
            .or_else(|| palette.color("blue"))
            .unwrap_or(bright);

        // The theme's own next surface up, where it has one that can actually
        // be seen against the panel; otherwise a step from the panel towards
        // the text, which every palette can supply.
        let border = palette
            .color("lighter_background")
            .filter(|shade| separated(*shade, background))
            .unwrap_or_else(|| mix(background, foreground, BORDER_LIFT));

        // The deepest surface the theme can offer, for the things that have
        // to sit under light ink whichever way round the theme is: the
        // floating panel, and the wash over what the minimap is not showing.
        let deep = match palette.mode() {
            Mode::Dark => palette
                .color("darker_background")
                .unwrap_or_else(|| mix(background, BLACK, 0.5)),
            Mode::Light => mix(foreground, BLACK, LIGHT_PANEL_DEPTH),
        };
        let on_deep = match palette.mode() {
            Mode::Dark => foreground,
            Mode::Light => background,
        };
        let panel_alpha = match palette.mode() {
            Mode::Dark => Theme::FALLBACK.panel_background.a,
            Mode::Light => LIGHT_PANEL_ALPHA,
        };

        let planes = [
            palette.color("red"),
            palette.color("green"),
            palette.color("blue"),
        ];
        // All three or none: a plot with one themed plane beside two default
        // ones would read as three unrelated colours.
        let histogram_planes = match planes {
            [Some(red), Some(green), Some(blue)] => {
                let mut ink = [red, green, blue];
                for (channel, plane) in ink.iter_mut().enumerate() {
                    *plane = channel_ink(*plane, channel);
                }
                balance(ink)
            }
            _ => Theme::FALLBACK.histogram_planes,
        };

        Theme {
            mode: palette.mode(),
            bar_background: background,
            border,
            menu_background: background.with_alpha(MENU_ALPHA),
            panel_background: deep.with_alpha(panel_alpha),
            panel_text: on_deep,
            button_idle: foreground.with_alpha(Theme::FALLBACK.button_idle.a),
            button_hover: foreground.with_alpha(Theme::FALLBACK.button_hover.a),
            text_primary: bright,
            text_dim: foreground,
            accent,
            minimap_edge: foreground.with_alpha(Theme::FALLBACK.minimap_edge.a),
            minimap_dim: deep.with_alpha(Theme::FALLBACK.minimap_dim.a),
            histogram_luma: on_deep.with_alpha(Theme::FALLBACK.histogram_luma.a),
            histogram_planes,
        }
    }
}

/// Whether `shade` can be told apart from `against` at hairline width.
fn separated(shade: Color, against: Color) -> bool {
    let distance = |a: u8, b: u8| a.abs_diff(b) as u32;
    distance(shade.r, against.r) + distance(shade.g, against.g) + distance(shade.b, against.b)
        >= SEPARATION
}

/// Turns a palette colour into ink for one histogram channel: pulled towards
/// its own primary, then opened up to full strength, which is what leaves
/// [`balance`] room to bring it down to where the mix wants it.
fn channel_ink(color: Color, channel: usize) -> Color {
    let mut pure = [0u8; COLOUR];
    pure[channel] = 255;
    let tinted = mix(color, Color::rgb(pure[0], pure[1], pure[2]), PLANE_PURITY);
    let peak = tinted.r.max(tinted.g).max(tinted.b);
    if peak == 0 {
        return Theme::FALLBACK.histogram_planes[channel];
    }
    scale(tinted, 255.0 / peak as f32)
}

/// Dims each plane until all three screened together come out neutral.
///
/// A plane is scaled whole, so its hue — the theme's — survives; only its
/// strength moves. Each plane dominates its own channel of the mix and barely
/// touches the other two, so one pass very nearly settles it and the rest
/// clean up the crosstalk. Without this a palette of pastels screens to a
/// tinted grey, and which way it is tinted depends on the theme, which is
/// exactly the thing a channel histogram must not do.
fn balance(mut planes: [Color; COLOUR]) -> [Color; COLOUR] {
    for _ in 0..PLANE_PASSES {
        for channel in 0..COLOUR {
            let others: f32 = planes
                .iter()
                .enumerate()
                .filter(|(plane, _)| *plane != channel)
                .map(|(_, plane)| 1.0 - plane.to_linear()[channel])
                .product();
            let have = planes[channel].to_linear()[channel];
            // What this plane's own channel has to be for the three of them
            // to multiply out to the target.
            let wanted = 1.0 - (1.0 - PLANE_MIX) / others;
            if wanted <= 0.0 || have <= 0.0 || !wanted.is_finite() {
                continue;
            }
            planes[channel] = scale(planes[channel], encode(wanted) / encode(have));
        }
    }
    planes
}

/// Multiplies a colour's channels, leaving its alpha and — since all three
/// move together — its hue alone.
fn scale(color: Color, by: f32) -> Color {
    let channel = |value: u8| (value as f32 * by + 0.5).clamp(0.0, 255.0) as u8;
    Color::rgba(
        channel(color.r),
        channel(color.g),
        channel(color.b),
        color.a,
    )
}

/// Light back to the sRGB value that carries it: the inverse of what
/// [`Color::to_linear`] does, for the one place that has to work backwards
/// from a brightness to the colour that would produce it.
fn encode(linear: f32) -> f32 {
    let linear = linear.clamp(0.0, 1.0);
    if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

#[cfg(test)]
mod tests {
    use super::palette::fixtures::*;
    use super::*;

    #[test]
    fn the_interface_takes_its_surfaces_and_its_ink_from_the_palette() {
        let theme = Theme::from_palette(&palette(TOKYO));
        assert_eq!(theme.mode, Mode::Dark);
        assert_eq!(theme.bar_background, Color::rgb(0x1a, 0x1b, 0x26));
        assert_eq!(theme.border, Color::rgb(0x24, 0x28, 0x3b));
        assert_eq!(theme.text_primary, Color::rgb(0xc0, 0xca, 0xf5));
        assert_eq!(theme.text_dim, Color::rgb(0xa9, 0xb1, 0xd6));
        assert_eq!(theme.accent, Color::rgb(0x7a, 0xa2, 0xf7));
        // The floating panel is the theme's deepest surface, kept translucent.
        assert_eq!(theme.panel_background, Color::rgba(0x0e, 0x0e, 0x14, 214));
        assert_eq!(theme.panel_text, theme.text_dim);
        // The washes are the panel colours at the interface's own alphas.
        assert_eq!(theme.button_idle, theme.text_dim.with_alpha(20));
        assert_eq!(theme.minimap_edge, theme.text_dim.with_alpha(70));
    }

    /// A menu is a handful of buttons, and buttons are drawn in ink made to
    /// read against the bars — so a menu's panel is the bars' surface, which
    /// on a light theme means a light one. The floating histogram panel is
    /// the one that stays dark either way, and for a reason a menu does not
    /// share.
    #[test]
    fn a_menu_sits_on_the_bars_surface_whichever_way_the_theme_runs() {
        for source in [TOKYO, SPARSE] {
            let theme = Theme::from_palette(&palette(source));
            assert_eq!(
                theme.menu_background,
                theme.bar_background.with_alpha(MENU_ALPHA)
            );
            // Read through, but only just.
            assert!(theme.menu_background.a > theme.panel_background.a);
        }
    }

    #[test]
    fn a_theme_with_no_shade_to_draw_the_hairline_in_gets_one_mixed() {
        // `lighter_background` falls back to the background itself when the
        // theme names neither, which would draw the panel edge invisibly.
        let theme = Theme::from_palette(&palette(SEMANTIC));
        assert_eq!(theme.bar_background, Color::rgb(0x1e, 0x1e, 0x2e));
        assert!(
            separated(theme.border, theme.bar_background),
            "{:?}",
            theme.border
        );
    }

    #[test]
    fn a_light_theme_keeps_the_floating_panel_dark_and_the_ink_on_it_light() {
        let theme = Theme::from_palette(&palette(SPARSE));
        assert_eq!(theme.mode, Mode::Light);
        assert_eq!(theme.bar_background, Color::rgb(0xf5, 0xf0, 0xe8));
        // Mixed out of the theme's ink rather than its background, which is
        // the wrong end of a light palette for a surface the histogram is
        // screened onto.
        assert_eq!(
            theme.panel_background,
            Color::rgba(0x26, 0x26, 0x23, LIGHT_PANEL_ALPHA)
        );
        assert_eq!(theme.panel_text, theme.bar_background);
        assert!(separated(theme.border, theme.bar_background));
    }

    #[test]
    fn a_palette_too_sparse_to_build_on_is_not_half_applied() {
        // No foreground, so nothing below the background could be derived.
        let theme = Theme::from_palette(&palette("background = \"#ffffff\"\n"));
        assert_eq!(theme, Theme::FALLBACK);
        assert_eq!(Theme::from_palette(&palette("")), Theme::FALLBACK);
    }

    /// What the three colour planes come to where they overlap. Screened, and
    /// in light rather than in encoded values, which is where the blend
    /// actually happens.
    fn screened(planes: [Color; COLOUR]) -> [f32; 3] {
        let mut out = [0.0f32; 3];
        for (channel, value) in out.iter_mut().enumerate() {
            *value = 1.0
                - planes
                    .iter()
                    .map(|plane| 1.0 - plane.to_linear()[channel])
                    .product::<f32>();
        }
        out
    }

    /// The plot's whole point is that overlaps read as the mix. Three pastels
    /// screened together climb to white and lose it, so a themed plane is
    /// pulled towards its own primary first; this is what says by how much.
    #[test]
    fn the_colour_planes_screen_to_a_neutral_rather_than_to_white() {
        for (name, planes) in [
            ("fallback", Theme::FALLBACK.histogram_planes),
            (
                "tokyo",
                Theme::from_palette(&palette(TOKYO)).histogram_planes,
            ),
            (
                "semantic",
                Theme::from_palette(&palette(SEMANTIC)).histogram_planes,
            ),
            ("ansi", Theme::from_palette(&palette(ANSI)).histogram_planes),
        ] {
            let mix = screened(planes);
            let high = mix.iter().copied().fold(f32::MIN, f32::max);
            let low = mix.iter().copied().fold(f32::MAX, f32::min);
            assert!(high < 0.6, "{name} screens to {mix:?}, which is blown out");
            assert!(
                high - low < 0.03,
                "{name} screens to {mix:?}, which is tinted"
            );
        }
    }

    /// Each plane still has to say which channel it is.
    #[test]
    fn a_themed_plane_stays_recognisably_its_own_channel() {
        for (name, text) in [("tokyo", TOKYO), ("semantic", SEMANTIC), ("ansi", ANSI)] {
            let planes = Theme::from_palette(&palette(text)).histogram_planes;
            for (channel, plane) in planes.iter().enumerate() {
                let levels = [plane.r, plane.g, plane.b];
                let dominant = levels
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, v)| **v)
                    .unwrap()
                    .0;
                assert_eq!(dominant, channel, "{name} plane {channel}: {plane:?}");
            }
        }
    }

    #[test]
    fn a_theme_that_names_no_colours_keeps_the_planes_it_was_designed_with() {
        let theme = Theme::from_palette(&palette(SPARSE));
        assert_eq!(theme.histogram_planes, Theme::FALLBACK.histogram_planes);
    }
}
