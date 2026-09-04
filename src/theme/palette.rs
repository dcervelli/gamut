//! The active Omarchy palette: reading `colors.toml`, and the alias and
//! derivation cascade that turns what a theme wrote into what it means.
//!
//! The file is not read literally. Omarchy resolves it — short names, ANSI
//! `color0`..`color15` in both directions, shades mixed out of the base
//! colors — before any consumer sees it, and a theme is free to define only
//! one side of any of those pairs. [`Palette`] reimplements that cascade
//! rather than shelling out to `omarchy-theme-color`, which costs a process
//! per read and is not there to be called off Omarchy anyway. The tests check
//! the result against what that script prints for the same file.
//!
//! Nothing here knows what the interface does with a color; that is
//! [`super::Theme`]'s business.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

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
/// The palette says which; it is not inferred from the colors here, since
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

/// Where Omarchy materializes the active theme. A real directory rewritten
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
    /// The largest a palette file may be before it is ignored. A real
    /// `colors.toml` is a few hundred bytes; the cap keeps this read — which
    /// runs on the event-loop thread every time the theme changes — from
    /// stalling on a huge file, a FIFO, or a device someone pointed the theme
    /// directory at.
    const MAX_BYTES: u64 = 64 * 1024;

    pub fn read(path: &Path) -> Option<Self> {
        // A regular file, and a small one. `read_to_string` on a FIFO or
        // `/dev/zero` would block the interface or exhaust memory, and it has
        // no length of its own to stop at.
        let metadata = fs::metadata(path).ok()?;
        if !metadata.is_file() || metadata.len() > Self::MAX_BYTES {
            return None;
        }
        let text = fs::read_to_string(path).ok()?;
        let light_marker = path
            .parent()
            .is_some_and(|dir| dir.join(LIGHT_MARKER).exists());
        Some(Self::resolve(&text, light_marker))
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// One resolved key as a color, or `None` when the theme does not define
    /// it or defines it as something that is not a hex color — a palette may
    /// hold gradient angles and `rgba()` lists as well, and those are not for
    /// us.
    pub fn color(&self, key: &str) -> Option<Color> {
        parse_hex(self.get(key)?)
    }

    /// An empty value counts as absent, the way the shell resolver's own
    /// tests of a key do, so a key aliased from something undefined does not
    /// shadow the derivation that would otherwise have filled it.
    pub(super) fn get(&self, key: &str) -> Option<&str> {
        self.values
            .get(key)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    /// Parses the file and applies the cascade. `light_marker` is whether a
    /// `light.mode` file sits beside it.
    pub(super) fn resolve(text: &str, light_marker: bool) -> Self {
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
/// unexpected in through a color.
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
/// mix colors that earlier ones may just have supplied.
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

pub(super) const BLACK: Color = Color::rgb(0, 0, 0);
pub(super) const WHITE: Color = Color::rgb(255, 255, 255);

/// `amount` of the way from `from` to `to`, per channel, on the encoded
/// values rather than on light. Not a color-managed blend on purpose: it has
/// to land on the same bytes as the mixes baked into every other themed
/// config on the desktop, and those are done this way.
pub(super) fn mix(from: Color, to: Color, amount: f32) -> Color {
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
/// not a color this interface can use and comes back as `None`.
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

/// Palettes the tests are written against, shared with the theme's own tests.
#[cfg(test)]
pub(crate) mod fixtures {
    use super::Palette;

    /// A theme written before the semantic palette: ANSI slots only.
    pub(crate) const ANSI: &str = "\
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
    pub(crate) const SEMANTIC: &str = "\
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
    pub(crate) const SPARSE: &str = "\
bg = \"#f5f0e8\"   # a light theme naming only the short forms
fg = \"#33322e\"
";

    /// The palette this was written against, trimmed to what the tests read.
    pub(crate) const TOKYO: &str = "\
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

    pub(crate) fn palette(text: &str) -> Palette {
        Palette::resolve(text, false)
    }

    pub(crate) fn resolved(palette: &Palette, key: &str) -> String {
        palette.get(key).unwrap_or("").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

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
            // No eighth color and no dark foreground, so the muted gray the
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
        // here and lands on a gray nobody chose. Saying nothing instead is
        // what lets the interface fall back to a color that was chosen.
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
}
