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
//! The palette file is not read literally. Omarchy resolves it through an
//! alias and derivation cascade — short names, ANSI `color0`..`color15` in
//! both directions, shades mixed out of the base colours — before any
//! consumer sees it, and a theme is free to define only one side of any of
//! those pairs. [`Palette`] reimplements that cascade rather than shelling
//! out to `omarchy-theme-color`, which costs a process per read and is not
//! there to be called off Omarchy anyway.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::image::stats::COLOUR;
use crate::render::Color;
use crate::watch::Watch;

/// A watch on the palette file, so that changing the desktop's theme reaches
/// a window that is already open.
///
/// The file rather than the directory holding it, even though Omarchy
/// replaces that directory wholesale: what is stat'ed is the path, so the
/// replacement is seen as a change to it rather than leaving the watch
/// looking at an inode nobody will write to again. With no `HOME` there is no
/// path to watch, and the watch quietly never fires.
pub fn watch() -> Watch {
    Watch::new(&colors_file().unwrap_or_default())
}

/// Whether the theme is meant to be read as light-on-dark or dark-on-light.
/// The palette says which; it is not inferred from the colours here, since
/// the file's own answer is the one every other application on the desktop
/// is using.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Dark,
    Light,
}

/// The active Omarchy palette, resolved.
///
/// Keys are Omarchy's own: `background`, `foreground`, `accent`, `muted`,
/// `red`..`bright_magenta`, `color0`..`color15`, and the shades derived from
/// them. Every key the cascade can reach is present, so a lookup that returns
/// nothing means the theme genuinely says nothing about it.
pub struct Palette {
    values: HashMap<String, String>,
    mode: Mode,
}

/// Where Omarchy materialises the active theme. A real directory rewritten
/// wholesale on every theme change, not a symlink into a themes folder, which
/// is why the file itself is what gets watched.
fn colors_file() -> Option<PathBuf> {
    let home = env::var_os("HOME")?;
    Some(
        Path::new(&home)
            .join(".local/state/omarchy/current/theme")
            .join(COLORS_NAME),
    )
}

const COLORS_NAME: &str = "colors.toml";
/// Written beside `colors.toml` by themes that predate the `mode` key.
const LIGHT_MARKER: &str = "light.mode";

impl Palette {
    /// The palette of the theme currently set, or `None` where there is no
    /// Omarchy on the machine and so no palette to read.
    pub fn load() -> Option<Self> {
        Self::read(&colors_file()?)
    }

    /// As [`Palette::load`], but from a named file, which is what makes the
    /// cascade testable against a palette other than this machine's.
    pub fn read(path: &Path) -> Option<Self> {
        let text = fs::read_to_string(path).ok()?;
        let light_marker = path
            .parent()
            .is_some_and(|dir| dir.join(LIGHT_MARKER).exists());
        Some(Self::resolve(&text, light_marker))
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// One resolved key as a colour, or `None` when the theme does not define
    /// it or defines it as something that is not a hex colour — a palette may
    /// hold gradient angles and `rgba()` lists as well, and those are not for
    /// us.
    pub fn color(&self, key: &str) -> Option<Color> {
        parse_hex(self.get(key)?)
    }

    /// An empty value counts as absent, the way the shell resolver's own
    /// tests of a key do, so a key aliased from something undefined does not
    /// shadow the derivation that would otherwise have filled it.
    fn get(&self, key: &str) -> Option<&str> {
        self.values
            .get(key)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    /// Parses the file and applies the cascade. `light_marker` is whether a
    /// `light.mode` file sits beside it.
    fn resolve(text: &str, light_marker: bool) -> Self {
        let mut values = parse(text);
        cascade(&mut values);
        let mode = resolve_mode(&values, light_marker);
        values.insert("mode".into(), mode_name(mode).into());
        values.insert("theme_type".into(), mode_name(mode).into());
        Self { values, mode }
    }
}

fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Dark => "dark",
        Mode::Light => "light",
    }
}

/// Reads the flat `key = "value"` file.
///
/// Deliberately not a TOML parse: the palette is one flat table of strings by
/// definition — every consumer on the system reads it with the same line-at-a-
/// time rules — and matching those rules matters more than being able to read
/// a nested document that will never be written.
///
/// A key or value holding anything outside the character set Omarchy accepts
/// is dropped rather than passed on, so a palette cannot smuggle something
/// unexpected in through a colour.
fn parse(text: &str) -> HashMap<String, String> {
    let mut values = HashMap::new();
    for line in text.lines() {
        let (key, rest) = match line.split_once('=') {
            Some(split) => split,
            // A line with no `=` cannot name anything, and comments are the
            // usual reason for one.
            None => continue,
        };
        let key: String = key
            .chars()
            .filter(|c| !matches!(c, '"' | '\'' | ' '))
            .collect();
        if key.is_empty() || key.starts_with('#') {
            continue;
        }
        // Anything between the first pair of quotes, which is also what drops
        // a trailing comment; an unquoted value is simply trimmed.
        let value = match rest.find(['"', '\'']) {
            Some(start) => {
                let after = &rest[start + 1..];
                &after[..after.find(['"', '\'']).unwrap_or(after.len())]
            }
            None => rest.trim(),
        };
        if !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            continue;
        }
        if !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "#(),._+/% -".contains(c))
        {
            continue;
        }
        values.insert(key, value.to_string());
    }
    values
}

/// The short names a theme may use instead of the canonical ones, and which
/// are written back afterwards for anything still reading them.
const SHORT_NAMES: [(&str, &str); 8] = [
    ("background", "bg"),
    ("dark_background", "dark_bg"),
    ("darker_background", "darker_bg"),
    ("lighter_background", "lighter_bg"),
    ("foreground", "fg"),
    ("dark_foreground", "dark_fg"),
    ("light_foreground", "light_fg"),
    ("bright_foreground", "bright_fg"),
];

/// The semantic name each ANSI slot carries, in both directions: a theme
/// written before the semantic palette defines only the left column, one
/// written after it only the right, and consumers read either.
const ANSI_NAMES: [(&str, &str); 16] = [
    ("color0", "background"),
    ("color1", "red"),
    ("color2", "green"),
    ("color3", "yellow"),
    ("color4", "blue"),
    ("color5", "magenta"),
    ("color6", "cyan"),
    ("color7", "foreground"),
    ("color8", "muted"),
    ("color9", "bright_red"),
    ("color10", "bright_green"),
    ("color11", "bright_yellow"),
    ("color12", "bright_blue"),
    ("color13", "bright_magenta"),
    ("color14", "bright_cyan"),
    ("color15", "bright_foreground"),
];

/// Fills in every key the palette does not define itself, following the same
/// order Omarchy's own resolver does — the order matters, since later steps
/// mix colours that earlier ones may just have supplied.
fn cascade(values: &mut HashMap<String, String>) {
    // The complete legacy short-name palette first, before ANSI fallbacks or
    // derived shades. A theme defining both forms keeps the canonical one.
    for (canonical, short) in SHORT_NAMES {
        alias(values, canonical, short);
    }

    // Themes written before the semantic palette name only the ANSI slots.
    alias(values, "background", "color0");
    alias(values, "foreground", "color7");
    if let Some(background) = get(values, "background") {
        values.insert("color0".into(), background);
    }
    if let Some(foreground) = get(values, "foreground") {
        values.insert("color7".into(), foreground);
    }
    for (ansi, semantic) in ANSI_NAMES {
        // Not `background`/`foreground`, which were just settled, and not
        // `muted` or `bright_foreground`, which have richer rules below.
        if !matches!(
            semantic,
            "background" | "foreground" | "muted" | "bright_foreground"
        ) {
            alias(values, semantic, ansi);
        }
    }
    alias(values, "magenta", "purple");
    alias(values, "bright_magenta", "bright_purple");

    alias_any(values, "light_foreground", &["color7", "foreground"]);
    alias_any(values, "bright_foreground", &["color15", "foreground"]);
    // The cursor is the bright foreground, always: a theme that sets it to
    // something else is overruled here exactly as it is everywhere else.
    if let Some(bright) = get(values, "bright_foreground") {
        values.insert("cursor".into(), bright);
    }
    alias_any(values, "lighter_background", &["color0", "background"]);
    alias_any(values, "dark_foreground", &["color8", "foreground"]);
    alias_any(values, "muted", &["color8", "dark_foreground"]);
    alias_any(
        values,
        "selection",
        &["selection_background", "color8", "color0", "background"],
    );
    alias(values, "selection_background", "selection");
    alias(values, "selection_foreground", "bright_foreground");
    alias(values, "orange", "yellow");
    derive(values, "brown", "orange", BLACK, 0.5);

    derive(values, "dark_background", "background", BLACK, 0.25);
    derive(values, "darker_background", "background", BLACK, 0.5);
    for base in ["red", "yellow", "green", "cyan", "blue", "magenta"] {
        derive(values, &format!("bright_{base}"), base, WHITE, 0.2);
    }
    alias(values, "purple", "magenta");
    alias(values, "bright_purple", "bright_magenta");

    for (ansi, semantic) in ANSI_NAMES {
        alias(values, ansi, semantic);
    }
    for (canonical, short) in SHORT_NAMES {
        if let Some(value) = get(values, canonical) {
            values.insert(short.into(), value);
        }
    }
}

fn get(values: &HashMap<String, String>, key: &str) -> Option<String> {
    values.get(key).filter(|v| !v.is_empty()).cloned()
}

/// Gives `key` the value of `from`, if `key` has none and `from` has one.
fn alias(values: &mut HashMap<String, String>, key: &str, from: &str) {
    alias_any(values, key, &[from]);
}

/// As [`alias`], taking the first of `from` that is defined.
fn alias_any(values: &mut HashMap<String, String>, key: &str, from: &[&str]) {
    if get(values, key).is_some() {
        return;
    }
    if let Some(value) = from.iter().find_map(|name| get(values, name)) {
        values.insert(key.into(), value);
    }
}

/// Gives `key` a shade mixed `amount` of the way from `base` towards `towards`.
///
/// A missing `base` leaves `key` missing, rather than mixing from nothing and
/// producing black: a theme that names no orange has no brown either, and
/// saying so lets the caller fall back to something wearable.
fn derive(
    values: &mut HashMap<String, String>,
    key: &str,
    base: &str,
    towards: Color,
    amount: f32,
) {
    if get(values, key).is_some() {
        return;
    }
    let Some(base) = get(values, base).as_deref().and_then(parse_hex) else {
        return;
    };
    values.insert(key.into(), hex(mix(base, towards, amount)));
}

/// The theme's own answer, then the marker file, then the background's
/// brightness, then dark. Matches the resolver every other consumer uses,
/// including its threshold: a background whose three bytes sum to more than
/// 382 is a light one.
fn resolve_mode(values: &HashMap<String, String>, light_marker: bool) -> Mode {
    if let Some(named) = get(values, "mode").or_else(|| get(values, "theme_type")) {
        return if named.eq_ignore_ascii_case("light") {
            Mode::Light
        } else {
            Mode::Dark
        };
    }
    if light_marker {
        return Mode::Light;
    }
    match get(values, "background").as_deref().and_then(parse_hex) {
        Some(background) => {
            let sum = background.r as u32 + background.g as u32 + background.b as u32;
            if sum > 382 { Mode::Light } else { Mode::Dark }
        }
        None => Mode::Dark,
    }
}

const BLACK: Color = Color::rgb(0, 0, 0);
const WHITE: Color = Color::rgb(255, 255, 255);

/// `amount` of the way from `from` to `to`, per channel, on the encoded
/// values rather than on light. Not a colour-managed blend on purpose: it has
/// to land on the same bytes as the mixes baked into every other themed
/// config on the desktop, and those are done this way.
fn mix(from: Color, to: Color, amount: f32) -> Color {
    let amount = amount.clamp(0.0, 1.0);
    let channel = |from: u8, to: u8| {
        (from as f32 * (1.0 - amount) + to as f32 * amount + 0.5).clamp(0.0, 255.0) as u8
    };
    Color::rgba(
        channel(from.r, to.r),
        channel(from.g, to.g),
        channel(from.b, to.b),
        from.a,
    )
}

fn hex(color: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b)
}

/// `#rrggbb` or its three-digit short form, with or without the `#`. Anything
/// else in a palette — an `rgba()` list, a gradient angle, a bare word — is
/// not a colour this interface can use and comes back as `None`.
fn parse_hex(value: &str) -> Option<Color> {
    let digits = value.trim().strip_prefix('#').unwrap_or(value.trim());
    if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |at: usize, width: usize| -> Option<u8> {
        let part = digits.get(at..at + width)?;
        let value = u8::from_str_radix(part, 16).ok()?;
        // A short digit stands for itself twice: `f` is `ff`.
        Some(if width == 1 { value * 17 } else { value })
    };
    match digits.len() {
        6 => Some(Color::rgb(byte(0, 2)?, byte(2, 2)?, byte(4, 2)?)),
        3 => Some(Color::rgb(byte(0, 1)?, byte(1, 1)?, byte(2, 1)?)),
        _ => None,
    }
}

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
    use super::*;

    /// A theme written before the semantic palette: ANSI slots only.
    const ANSI: &str = "\
# an ANSI-only theme, of the sort written before the semantic palette
color0 = \"#101218\"
color1 = \"#cc4455\"
color2 = \"#55aa66\"
color3 = \"#ddaa44\"
color4 = \"#4477dd\"
color5 = \"#aa66cc\"
color6 = \"#44aaaa\"
color7 = \"#c8ccd4\"
color8 = \"#404a5a\"
";

    /// A theme written after it: semantic names only, and not all of them.
    const SEMANTIC: &str = "\
background = \"#1e1e2e\"
foreground = \"#cdd6f4\"
accent = \"#89b4fa\"
red = \"#f38ba8\"
green = \"#a6e3a1\"
blue = \"#89b4fa\"
yellow = \"#f9e2af\"
cyan = \"#94e2d5\"
purple = \"#cba6f7\"
";

    /// Two keys, in the short forms, and light.
    const SPARSE: &str = "\
bg = \"#f5f0e8\"   # a light theme naming only the short forms
fg = \"#33322e\"
";

    /// The palette this was written against, trimmed to what the tests read.
    const TOKYO: &str = "\
mode = \"dark\"
accent = \"#7aa2f7\"
selection = \"#292e42\"
muted = \"#414868\"
background = \"#1a1b26\"
dark_background = \"#13141c\"
darker_background = \"#0e0e14\"
lighter_background = \"#24283b\"
foreground = \"#a9b1d6\"
bright_foreground = \"#c0caf5\"
red = \"#f7768e\"
green = \"#9ece6a\"
blue = \"#7aa2f7\"
yellow = \"#e0af68\"
";

    fn palette(text: &str) -> Palette {
        Palette::resolve(text, false)
    }

    fn resolved(palette: &Palette, key: &str) -> String {
        palette.get(key).unwrap_or("").to_string()
    }

    #[test]
    fn a_value_is_taken_from_between_its_quotes() {
        let values = parse("a = \"#112233\"  # trailing note\nb = '#445566'\nc = bare\n");
        assert_eq!(values["a"], "#112233");
        assert_eq!(values["b"], "#445566");
        assert_eq!(values["c"], "bare");
    }

    #[test]
    fn comments_and_lines_that_name_nothing_are_passed_over() {
        let values = parse("# heading\n\n   # indented = not a key\nreal = \"#000000\"\n");
        assert_eq!(values.len(), 1);
        assert_eq!(values["real"], "#000000");
    }

    #[test]
    fn a_key_or_value_outside_the_character_set_is_dropped() {
        let values = parse("we$rd = \"#112233\"\nfine = \"#112233\"\nshell = \"$(id)\"\n");
        assert_eq!(values.keys().collect::<Vec<_>>(), vec!["fine"]);
    }

    /// Every expectation below is what `omarchy-theme-color --file … --all`
    /// prints for the same file, so the cascade is checked against the
    /// resolver the rest of the desktop is themed by rather than against
    /// itself.
    #[test]
    fn an_ansi_only_theme_resolves_to_the_semantic_names() {
        let palette = palette(ANSI);
        for (key, expected) in [
            ("background", "#101218"),
            ("foreground", "#c8ccd4"),
            ("red", "#cc4455"),
            ("green", "#55aa66"),
            ("blue", "#4477dd"),
            ("magenta", "#aa66cc"),
            ("muted", "#404a5a"),
            ("dark_foreground", "#404a5a"),
            ("light_foreground", "#c8ccd4"),
            ("bright_foreground", "#c8ccd4"),
            ("cursor", "#c8ccd4"),
            ("lighter_background", "#101218"),
            ("selection", "#404a5a"),
            ("selection_foreground", "#c8ccd4"),
            ("orange", "#ddaa44"),
            ("bg", "#101218"),
        ] {
            assert_eq!(resolved(&palette, key), expected, "{key}");
        }
        assert_eq!(palette.mode(), Mode::Dark);
    }

    #[test]
    fn shades_a_theme_does_not_name_are_mixed_out_of_the_ones_it_does() {
        let palette = palette(ANSI);
        for (key, expected) in [
            // A quarter and a half of the way to black.
            ("dark_background", "#0c0e12"),
            ("darker_background", "#08090c"),
            // A fifth of the way to white.
            ("bright_red", "#d66977"),
            ("bright_green", "#77bb85"),
            ("bright_blue", "#6992e4"),
            ("bright_magenta", "#bb85d6"),
            ("bright_purple", "#bb85d6"),
            // Half of the way from orange to black.
            ("brown", "#6f5522"),
        ] {
            assert_eq!(resolved(&palette, key), expected, "{key}");
        }
    }

    #[test]
    fn a_semantic_theme_resolves_back_to_the_ansi_names() {
        let palette = palette(SEMANTIC);
        for (key, expected) in [
            ("color0", "#1e1e2e"),
            ("color1", "#f38ba8"),
            ("color4", "#89b4fa"),
            // Nothing named a magenta, so the purple stands in for it, and
            // for the slot it occupies.
            ("color5", "#cba6f7"),
            ("magenta", "#cba6f7"),
            ("color7", "#cdd6f4"),
            // No eighth colour and no dark foreground, so the muted grey the
            // slot holds falls all the way back to the foreground.
            ("color8", "#cdd6f4"),
            ("muted", "#cdd6f4"),
            ("color9", "#f5a2b9"),
            ("color15", "#cdd6f4"),
            // No orange, so the yellow serves, and the brown is mixed from it.
            ("orange", "#f9e2af"),
            ("brown", "#7d7158"),
            ("dark_background", "#171723"),
            ("selection", "#1e1e2e"),
        ] {
            assert_eq!(resolved(&palette, key), expected, "{key}");
        }
    }

    #[test]
    fn a_shade_with_nothing_to_mix_from_is_left_unset() {
        // The resolver every other consumer uses mixes from the empty string
        // here and lands on a grey nobody chose. Saying nothing instead is
        // what lets the interface fall back to a colour that was chosen.
        let palette = palette(SPARSE);
        for absent in ["red", "green", "blue", "brown", "bright_red"] {
            assert_eq!(palette.color(absent), None, "{absent}");
        }
        assert_eq!(resolved(&palette, "background"), "#f5f0e8");
        assert_eq!(resolved(&palette, "foreground"), "#33322e");
    }

    #[test]
    fn the_mode_is_the_themes_own_answer_before_it_is_a_guess() {
        assert_eq!(
            palette("mode = \"light\"\nbackground = \"#000000\"\n").mode(),
            Mode::Light
        );
        assert_eq!(
            palette("theme_type = \"light\"\nbackground = \"#000000\"\n").mode(),
            Mode::Light
        );
        // A marker file beside the palette, for themes older than either key.
        assert_eq!(
            Palette::resolve("background = \"#000000\"\n", true).mode(),
            Mode::Light
        );
    }

    #[test]
    fn a_theme_that_will_not_say_is_judged_by_its_background() {
        // The resolver's own threshold: the three bytes summed, against 382.
        assert_eq!(palette("background = \"#7f8080\"\n").mode(), Mode::Light);
        assert_eq!(palette("background = \"#7f7f80\"\n").mode(), Mode::Dark);
        assert_eq!(palette(SPARSE).mode(), Mode::Light);
        // Nothing to judge at all.
        assert_eq!(palette("accent = \"#ff0000\"\n").mode(), Mode::Dark);
    }

    #[test]
    fn short_hex_stands_for_itself_twice() {
        assert_eq!(parse_hex("#f0a"), Some(Color::rgb(255, 0, 170)));
        assert_eq!(parse_hex("1a1b26"), Some(Color::rgb(26, 27, 38)));
        assert_eq!(parse_hex("rgba(1,2,3,0.5)"), None);
        assert_eq!(parse_hex("-45deg"), None);
    }

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
