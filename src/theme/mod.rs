//! Where the interface's colors come from.
//!
//! The window is a viewer, not a desktop: its chrome should disappear into
//! whatever the rest of the desktop looks like rather than announce itself in
//! one fixed gray. On Omarchy the active theme is materialized as a palette
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

use crate::image::stats::COLOR;
use crate::render::Color;

use palette::{BLACK, WHITE, mix};

/// Every color the interface draws with.
///
/// One value per role rather than per widget: the histogram's axis label and
/// the minimap's outline are not separately themeable, they are "text on a
/// floating panel" and "a hairline over the image", and there are few enough
/// roles that a theme can be reasoned about whole.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Theme {
    /// Whether the palette reads light-on-dark or dark-on-light. Nothing here
    /// branches on it any more, the surfaces having been derived to work
    /// either way round, but it is what the derivation itself reads.
    pub mode: Mode,
    /// The four panels, and the window behind the image.
    pub bar_background: Color,
    /// The hairline along a panel's inner edge, and the other square of the
    /// checkerboard behind a transparent image.
    pub border: Color,
    /// The panel a popup's cells sit on. The bars' color rather than the
    /// floating panel's, and for the same reason the cells are drawn like the
    /// toggles in the side panels: what is on a menu are buttons, and the ink
    /// buttons are drawn in is made to read against the bars. Near enough to
    /// opaque to be read through — a menu is what is being looked at while it
    /// is open — but not quite, so that it still reads as lying over the
    /// image rather than as another piece of the chrome.
    pub menu_background: Color,
    /// The panels that float over the image: the histogram and the file's
    /// information. The bars' own color, so that words over the picture are
    /// read on the same ground as the words in the bars — in `text_primary`
    /// and `text_dim`, which are made to sit on it. Mildly transparent, so
    /// that it reads as lying over the picture rather than as another piece
    /// of the chrome.
    pub panel_background: Color,
    /// The ground the histogram's plot itself is drawn on, inside that panel.
    /// Near-black whatever the theme: the plot is drawn by screening the
    /// color planes over one another, and that only reads as three colors
    /// on a dark ground. Opaque, since it is the one surface here that has a
    /// measurement on it rather than words.
    pub plot_background: Color,
    pub button_idle: Color,
    pub button_hover: Color,
    /// Text in the bars: what the image is, and what is being done to it.
    pub text_primary: Color,
    pub text_dim: Color,
    /// The ink that leads, for the one thing in the window that answers
    /// "what am I looking at": the file's own name.
    ///
    /// The theme's `bright_foreground`, where that is actually parted from
    /// the text beside it. A palette is free to define the two the same and
    /// several do, which would leave the name reading exactly like the facts
    /// it shares the bar with; where they collapse this is carried away from
    /// the page until it does not — towards white on a dark theme and
    /// towards black on a light one, "bright" here meaning further from the
    /// ground than the ordinary text rather than lighter in itself.
    pub text_bright: Color,
    /// What is switched on, and where the display window sits.
    pub accent: Color,
    /// The word that says the file behind the picture on screen is gone, and
    /// the message that says something could not be done. Its own role rather
    /// than the accent, which means the opposite: the accent is what is
    /// switched on, and this is what has been lost.
    pub warning: Color,
    /// The message that says something was not quite what was asked for —
    /// short of [`Theme::warning`], which is for what failed outright.
    ///
    /// The theme's own yellow, which is the color its terminal writes a
    /// caution in, and so a color chosen to be read against this very
    /// background. Falls back to the red beside it where the theme names no
    /// yellow that can be seen here: two levels drawn alike say less than
    /// they should, and a level drawn invisibly says nothing at all.
    pub caution: Color,
    /// The minimap's border, and the wash over the part of the image that is
    /// off screen. Both go over a thumbnail drawn by the image layer, so both
    /// stay translucent.
    pub minimap_edge: Color,
    pub minimap_dim: Color,
    /// The luminance plane, under the color ones: a neutral gray, since it
    /// is the value of a pixel and not one of its channels.
    pub histogram_luma: Color,
    /// Red, green and blue channel ink, in that order. The primaries
    /// themselves rather than anything of the theme's: screened over one
    /// another on a near-black ground they give the secondaries where two
    /// planes meet and white where all three do, which is what makes a
    /// channel histogram readable at a glance.
    pub histogram_planes: [Color; COLOR],
}

/// How far the hairline is lifted off the panel color when the theme has no
/// shade of its own to use. Enough to place an edge, little enough that the
/// eye does not keep going back to it.
const BORDER_LIFT: f32 = 0.15;
/// How far the file name's ink is carried away from the page when the theme's
/// own bright text is not parted from its ordinary text. Enough that the name
/// leads the bar it is in, little enough that it is still the theme's color
/// and not simply the end it was carried towards.
const BRIGHT_LIFT: f32 = 0.45;
/// How far apart, summed over the three channels, two colors have to be
/// before one can be seen against the other. A theme whose `lighter_background`
/// resolves back to its `background` — which is what happens when it defines
/// neither — would otherwise draw the hairline invisibly.
const SEPARATION: u32 = 18;
/// The most light the wash over the minimap may carry, as an HSV value. It
/// goes over the part of the image the view is not showing, and has to read
/// as "not this" whichever way round the theme runs, so whatever color the
/// theme offers is taken down until it is dark.
///
/// Set above the deepest surface every theme the interface was tried against
/// defines — the highest was 0.137 — so a theme that has thought about its
/// own dark end is never overridden; what the cap catches is the light theme,
/// whose deepest color is nothing of the kind.
const DEEP_VALUE_CEIL: f32 = 0.14;
/// How opaque a floating panel is. Enough of the image comes through to place
/// the panel over it; not enough to compete with what is written on it — and
/// the information panel is a long column of small words, which is the most
/// that is ever asked of this ground.
const PANEL_ALPHA: u8 = 245;

/// How opaque a popup's panel is. Higher than the panels that float over the
/// image permanently: the picture coming through a menu competes with the
/// choices on it.
const MENU_ALPHA: u8 = 251;

/// The ground the plot is drawn on. Not quite black, so that the panel's own
/// edge is still an edge rather than a hole in it.
const PLOT_BACKGROUND: Color = Color::rgb(8, 8, 10);
/// The luminance plane. Neutral, and no theme's business: it is the one plane
/// that stands for a pixel's value rather than for a channel, and a value
/// with a hue on it would read as a fourth color.
const HISTOGRAM_LUMA: Color = Color::rgba(170, 170, 170, 200);
/// The color planes: the primaries themselves. Screened over one another on
/// [`PLOT_BACKGROUND`] these give yellow, cyan and magenta where two overlap
/// and white where all three do, which is the reading a channel histogram is
/// looked at for — and is the same reading in every theme.
const HISTOGRAM_PLANES: [Color; COLOR] = [
    Color::rgb(255, 0, 0),
    Color::rgb(0, 255, 0),
    Color::rgb(0, 0, 255),
];

impl Theme {
    /// The neutral dark set the interface was designed in, and what is used
    /// where there is no palette to read.
    pub const FALLBACK: Theme = Theme {
        mode: Mode::Dark,
        bar_background: Color::rgb(18, 18, 22),
        border: Color::rgb(38, 38, 46),
        menu_background: Color::rgba(18, 18, 22, MENU_ALPHA),
        panel_background: Color::rgba(18, 18, 22, PANEL_ALPHA),
        plot_background: PLOT_BACKGROUND,
        button_idle: Color::rgba(255, 255, 255, 20),
        button_hover: Color::rgba(255, 255, 255, 45),
        text_primary: Color::rgb(238, 238, 238),
        text_dim: Color::rgb(150, 152, 160),
        // The set the interface was designed in already parts its two inks,
        // so the name is the primary taken the last step to white.
        text_bright: Color::rgb(255, 255, 255),
        accent: Color::rgb(120, 180, 255),
        warning: Color::rgb(255, 116, 108),
        caution: Color::rgb(240, 190, 110),
        minimap_edge: Color::rgba(255, 255, 255, 70),
        minimap_dim: Color::rgba(6, 6, 10, 150),
        histogram_luma: HISTOGRAM_LUMA,
        histogram_planes: HISTOGRAM_PLANES,
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
        // A theme that names its bright text no different from its ordinary
        // text has not parted the two, and the name in the bar has to lead
        // whatever the theme did — the same fallback the hairline gets when
        // `lighter_background` resolves back to the background it sits on.
        // Away from the page, not simply towards white: on a light theme the
        // ink that leads is the darker one.
        let text_bright = match separated(bright, foreground) {
            true => bright,
            false => mix(
                bright,
                match palette.mode() {
                    Mode::Dark => WHITE,
                    Mode::Light => BLACK,
                },
                BRIGHT_LIFT,
            ),
        };
        let accent = palette
            .color("accent")
            .or_else(|| palette.color("blue"))
            .unwrap_or(bright);

        // A theme's own red, which it chose to be read against this very
        // background — it is the color its terminal writes errors in. The
        // leading ink where the theme names no red that can be seen here, so
        // that the word still reads even when it cannot be colored.
        let warning = ["bright_red", "red"]
            .into_iter()
            .filter_map(|key| palette.color(key))
            .find(|shade| separated(*shade, background))
            .unwrap_or(text_bright);

        // And its own yellow, read the same way: the color its terminal
        // writes a caution in. The red beside it where no yellow the theme
        // names can be seen against this background, since a caution drawn in
        // the ground it sits on is a caution nobody reads.
        let caution = ["bright_yellow", "yellow"]
            .into_iter()
            .filter_map(|key| palette.color(key))
            .find(|shade| separated(*shade, background))
            .unwrap_or(warning);

        // The theme's own next surface up, where it has one that can actually
        // be seen against the panel; otherwise a step from the panel towards
        // the text, which every palette can supply.
        let border = palette
            .color("lighter_background")
            .filter(|shade| separated(*shade, background))
            .unwrap_or_else(|| mix(background, foreground, BORDER_LIFT));

        // The deepest surface the theme can offer, for the one wash that has
        // to read as dark whichever way round the theme is: what the minimap
        // lays over the part of the image the view is not showing.
        //
        // A light theme has no such surface to name — its own darkest color
        // is its ink, and how deep a theme takes its ink is a matter of taste
        // it was free to settle either way — so whichever color the mode
        // arrives at is then taken down until it is dark.
        let deep = darkened(match palette.mode() {
            Mode::Dark => palette
                .color("darker_background")
                .unwrap_or_else(|| mix(background, BLACK, 0.5)),
            Mode::Light => foreground,
        });

        Theme {
            mode: palette.mode(),
            bar_background: background,
            border,
            menu_background: background.with_alpha(MENU_ALPHA),
            panel_background: background.with_alpha(PANEL_ALPHA),
            plot_background: PLOT_BACKGROUND,
            button_idle: foreground.with_alpha(Theme::FALLBACK.button_idle.a),
            button_hover: foreground.with_alpha(Theme::FALLBACK.button_hover.a),
            text_primary: bright,
            text_dim: foreground,
            text_bright,
            accent,
            warning,
            caution,
            minimap_edge: foreground.with_alpha(Theme::FALLBACK.minimap_edge.a),
            minimap_dim: deep.with_alpha(Theme::FALLBACK.minimap_dim.a),
            histogram_luma: HISTOGRAM_LUMA,
            histogram_planes: HISTOGRAM_PLANES,
        }
    }
}

/// A color taken down to [`DEEP_VALUE_CEIL`] if it is above it, scaled whole
/// so that its hue and saturation are exactly what they were. Anything
/// already that deep is its own theme's business and is left alone.
fn darkened(color: Color) -> Color {
    let peak = color.r.max(color.g).max(color.b) as f32 / 255.0;
    if peak <= DEEP_VALUE_CEIL {
        return color;
    }
    scale(color, DEEP_VALUE_CEIL / peak)
}

/// Whether `shade` can be told apart from `against` at hairline width.
fn separated(shade: Color, against: Color) -> bool {
    let distance = |a: u8, b: u8| a.abs_diff(b) as u32;
    distance(shade.r, against.r) + distance(shade.g, against.g) + distance(shade.b, against.b)
        >= SEPARATION
}

/// Multiplies a color's channels, leaving its alpha and — since all three
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
        // The washes are the panel colors at the interface's own alphas.
        assert_eq!(theme.button_idle, theme.text_dim.with_alpha(20));
        assert_eq!(theme.minimap_edge, theme.text_dim.with_alpha(70));
    }

    /// A menu is a handful of buttons, and buttons are drawn in ink made to
    /// read against the bars — so a menu's panel is the bars' surface, and so
    /// is a floating panel's. The menu is only the more opaque of the two:
    /// the picture coming through it competes with the choices on it.
    #[test]
    fn every_panel_over_the_image_sits_on_the_bars_own_surface() {
        for source in [TOKYO, SPARSE] {
            let theme = Theme::from_palette(&palette(source));
            assert_eq!(
                theme.menu_background,
                theme.bar_background.with_alpha(MENU_ALPHA)
            );
            assert_eq!(
                theme.panel_background,
                theme.bar_background.with_alpha(PANEL_ALPHA)
            );
            // Read through, but only just.
            assert!(theme.menu_background.a > theme.panel_background.a);
        }

        // Including on a light theme, where they are light: nothing is
        // screened onto either, so neither has a reason to be dark.
        let light = Theme::from_palette(&palette(SPARSE));
        assert_eq!(light.mode, Mode::Light);
        assert!(light.panel_background.r > light.plot_background.r);
    }

    /// The file's name is the one thing in the window that says what is being
    /// looked at, so it has to lead the facts it shares the bar with. A theme
    /// that parts its bright text from its ordinary text is taken at its
    /// word; one that defines them the same is not left saying nothing.
    #[test]
    fn the_file_names_ink_leads_the_text_beside_it_whatever_the_theme() {
        // A palette that parts them keeps its own.
        let theme = Theme::from_palette(&palette(TOKYO));
        assert_eq!(theme.text_bright, Color::rgb(0xc0, 0xca, 0xf5));
        assert_eq!(theme.text_bright, theme.text_primary);

        // One that defines both the same is carried away from its own page
        // rather than left reading exactly like the facts beside it.
        const FLAT: &str = "\
background = \"#121212\"
foreground = \"#bebebe\"
bright_foreground = \"#bebebe\"
";
        let flat = Theme::from_palette(&palette(FLAT));
        assert_eq!(
            flat.text_primary, flat.text_dim,
            "the theme did collapse them"
        );
        assert!(
            flat.text_bright.r > flat.text_dim.r,
            "{:?}",
            flat.text_bright
        );

        // On a light theme that is downwards: the ink that leads on a pale
        // page is the darker one, not the lighter.
        const FLAT_LIGHT: &str = "\
background = \"#f5f0e8\"
foreground = \"#4a4a4a\"
bright_foreground = \"#4a4a4a\"
";
        let light = Theme::from_palette(&palette(FLAT_LIGHT));
        assert_eq!(light.mode, Mode::Light);
        assert!(
            light.text_bright.r < light.text_dim.r,
            "{:?}",
            light.text_bright
        );

        // Whichever way round, and whatever the theme, it is tellable from
        // the text it sits beside.
        for source in [TOKYO, SEMANTIC, ANSI, SPARSE, FLAT, FLAT_LIGHT] {
            let theme = Theme::from_palette(&palette(source));
            assert!(
                separated(theme.text_bright, theme.text_dim),
                "{source}: {:?}",
                theme.text_bright
            );
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

    /// The plot's ground is the one surface with a job that outranks matching
    /// the desktop: a screened plot has to have a dark ground under it or its
    /// planes stop being three colors. So it is the same near-black in every
    /// theme, and the wash the minimap lays over what it is not showing is
    /// taken down to a dark whatever the theme offered.
    #[test]
    fn the_plot_is_drawn_on_a_dark_ground_whatever_the_theme() {
        // A light theme whose ink is barely darker than its page, and one
        // declaring itself dark over a background that is not.
        const PALE_INK: &str = "background = \"#fdfdfb\"\nforeground = \"#8a6f4e\"\n";
        const NAMED_DARK: &str = "\
mode = \"dark\"
background = \"#c8ccd4\"
foreground = \"#101218\"
darker_background = \"#b0b4bc\"
";
        for source in [SPARSE, PALE_INK, NAMED_DARK, TOKYO, SEMANTIC, ANSI] {
            let theme = Theme::from_palette(&palette(source));
            assert_eq!(theme.plot_background, PLOT_BACKGROUND, "{source}");
            let dim = theme.minimap_dim;
            let value = dim.r.max(dim.g).max(dim.b) as f32 / 255.0;
            assert!(value <= DEEP_VALUE_CEIL + 0.005, "{source}: {dim:?}");
        }

        // Hue and saturation survive the trip down: the wash of a theme whose
        // ink is a warm brown is a warm brown.
        let dim = Theme::from_palette(&palette(PALE_INK)).minimap_dim;
        assert!(dim.r > dim.g && dim.g > dim.b, "{dim:?}");
    }

    #[test]
    fn a_palette_too_sparse_to_build_on_is_not_half_applied() {
        // No foreground, so nothing below the background could be derived.
        let theme = Theme::from_palette(&palette("background = \"#ffffff\"\n"));
        assert_eq!(theme, Theme::FALLBACK);
        assert_eq!(Theme::from_palette(&palette("")), Theme::FALLBACK);
    }

    /// What the three color planes come to where they overlap. Screened, and
    /// in light rather than in encoded values, which is where the blend
    /// actually happens.
    fn screened(planes: [Color; COLOR]) -> [f32; 3] {
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

    /// The planes are the primaries and the same in every theme, which is
    /// what makes the plot read the same everywhere: each plane owns one
    /// channel outright, so two overlapping give a secondary and all three
    /// give white.
    #[test]
    fn the_color_planes_are_the_primaries_in_every_theme() {
        for source in [TOKYO, SEMANTIC, ANSI, SPARSE] {
            let planes = Theme::from_palette(&palette(source)).histogram_planes;
            assert_eq!(planes, HISTOGRAM_PLANES, "{source}");
        }
        assert_eq!(screened(HISTOGRAM_PLANES), [1.0, 1.0, 1.0]);
        for (channel, plane) in HISTOGRAM_PLANES.iter().enumerate() {
            let levels = [plane.r, plane.g, plane.b];
            for (other, level) in levels.iter().enumerate() {
                assert_eq!(*level, if other == channel { 255 } else { 0 }, "{plane:?}");
            }
        }
    }

    /// And the plane under them stands for a pixel's value, not for one of
    /// its channels, so it carries no hue in any theme.
    #[test]
    fn the_luminance_plane_is_neutral_in_every_theme() {
        for source in [TOKYO, SEMANTIC, ANSI, SPARSE] {
            let luma = Theme::from_palette(&palette(source)).histogram_luma;
            assert_eq!(luma, HISTOGRAM_LUMA, "{source}");
            assert!(luma.r == luma.g && luma.g == luma.b, "{luma:?}");
        }
    }
}
