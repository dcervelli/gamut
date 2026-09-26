//! What the program starts from beyond the command line: the configuration
//! the user writes, and the state it keeps for itself between runs.
//!
//! The two are different files because they are written by different hands.
//! The configuration, `$XDG_CONFIG_HOME/gamut/config`, is the user's: how
//! the window should open every time. The program only reads it, and a
//! toggle pressed in the window leaves it alone. The state,
//! `$XDG_STATE_HOME/gamut/state`, is the program's: what was left where it
//! was set by hand — the file list's width where it was dragged, the
//! loupe's magnification where the wheel left it — written when the window
//! closes, and nothing lost when it is deleted.
//!
//! Both are lines of `name = value`, with `#` starting a comment. The
//! configuration's names are its settings, and beside them every key's
//! dotted name under `keys.` and every gesture's slot under `gesture.`. A missing
//! file is every default, and none is ever written for the user: a line
//! written out on their behalf would hold them to today's default after it
//! changed. [`Config::template`] is what `--print-config` prints instead —
//! every setting at its default, commented out. A line the configuration cannot use is said on
//! the terminal and skipped, the rest still taken; the state file is ours,
//! so a line in it that does not read is only dropped.

use std::fs;
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::app::keymap::{Chord, Keymap};
use crate::gestures::{Behavior, Gestures, Slot};
use crate::ui::PixelFormat;
use crate::ui::{filmstrip, loupe};
use crate::{PROGRAM, shown_path, xdg};

/// The configuration: which panels the window opens with, how the
/// pointer's pixel is written, and what the keys and the mouse do. A
/// command-line flag for the same thing wins over it.
#[derive(Clone, PartialEq, Debug)]
pub struct Config {
    pub show_ui: bool,
    pub show_minimap: bool,
    pub show_filmstrip: bool,
    pub show_histogram: bool,
    pub show_info: bool,
    pub pixel_format: PixelFormat,
    pub log_counts: bool,
    pub keys: Keymap,
    pub gestures: Gestures,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            show_ui: true,
            show_minimap: true,
            show_filmstrip: true,
            show_histogram: false,
            show_info: false,
            pixel_format: PixelFormat::Hex,
            log_counts: false,
            keys: Keymap::default(),
            gestures: Gestures::default(),
        }
    }
}

/// Every setting the configuration file takes, in the order the template
/// lists them, with the words it wears there. [`Config::value`] writes each
/// one's value, and [`Config::parse`] reads it back.
const SETTINGS: [(&str, &str); 7] = [
    ("show_ui", "The panels around the picture."),
    (
        "show_minimap",
        "The minimap, while the picture is larger than the window.",
    ),
    (
        "show_filmstrip",
        "The file list down the left, while there is more than one file.",
    ),
    ("show_histogram", "The histogram panel."),
    ("show_info", "The file information panel."),
    (
        "pixel_format",
        "How the pixel under the pointer is read out: hex, decimal, or mapped.",
    ),
    (
        "log_counts",
        "The histogram's bars as tall as the logarithm of their counts.",
    ),
];

impl Config {
    /// The configuration as a file: every setting at its default, commented
    /// out, under a line saying what it does; then every key's name at its
    /// chords, and every gesture's slot at its behavior. Uncommenting a line
    /// and changing its value is the whole of editing it, and a line left
    /// commented follows the default wherever it goes.
    pub fn template() -> String {
        let mut text = format!(
            "# {PROGRAM}'s configuration: how the window opens, and what the keys and\n\
             # the mouse do. Each setting is shown at its default, commented out;\n\
             # take the # off a line to change it. --histogram, --info and\n\
             # --no-minimap win over what is set here.\n"
        );
        let config = Self::default();
        for (name, words) in SETTINGS {
            // Which key hides them, as the keys below have it.
            let key = config.keys.spelled("interface.toggle");
            let words = match name {
                "show_ui" if !key.is_empty() => {
                    format!("The panels around the picture; {key} hides and shows them.")
                }
                _ => words.to_string(),
            };
            text.push_str(&format!("\n# {words}\n# {name} = {}\n", config.value(name)));
        }
        text.push_str(&config.keys.template());
        text.push_str(&config.gestures.template());
        text
    }

    /// The value of the setting `name`, as the file writes it.
    fn value(&self, name: &str) -> String {
        match name {
            "show_ui" => self.show_ui.to_string(),
            "show_minimap" => self.show_minimap.to_string(),
            "show_filmstrip" => self.show_filmstrip.to_string(),
            "show_histogram" => self.show_histogram.to_string(),
            "show_info" => self.show_info.to_string(),
            "pixel_format" => self.pixel_format.label().to_ascii_lowercase(),
            "log_counts" => self.log_counts.to_string(),
            _ => unreachable!("`{name}` is not in SETTINGS"),
        }
    }

    /// The configuration file, read, with what it could not use said on the
    /// terminal. No file, or no directory to look for one in, is every
    /// default.
    pub fn load() -> Self {
        let Some(path) = config_path() else {
            return Self::default();
        };
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Self::default(),
            Err(error) => {
                eprintln!("{PROGRAM}: reading {}: {error}", shown_path(&path));
                return Self::default();
            }
        };
        let (config, problems) = Self::parse(&text);
        for (line, problem) in problems {
            eprintln!("{PROGRAM}: {} line {line}: {problem}", shown_path(&path));
        }
        config
    }

    /// `text` as a configuration, and each line it could not use, by
    /// number, with what was wrong with it. A setting written twice is
    /// taken from the later line; so is a chord two lines give two names in
    /// one context, which is said.
    fn parse(text: &str) -> (Self, Vec<(usize, String)>) {
        let mut config = Self::default();
        let mut problems = Vec::new();
        // Which line set each key's name, for saying which a later line
        // took a chord from.
        let mut set_on: Vec<(&'static str, usize)> = Vec::new();
        for (number, line) in lines(text) {
            let Some((name, value)) = line else {
                problems.push((number, "expected `name = value`".to_string()));
                continue;
            };
            if let Some(key) = name.strip_prefix("keys.") {
                config.parse_keys(key, value, number, &mut set_on, &mut problems);
                continue;
            }
            if let Some(slot) = name.strip_prefix("gesture.") {
                if let Err(problem) = config.parse_gesture(slot, value) {
                    problems.push((number, problem));
                }
                continue;
            }
            let flag = match name {
                "show_ui" => Some(&mut config.show_ui),
                "show_minimap" => Some(&mut config.show_minimap),
                "show_filmstrip" => Some(&mut config.show_filmstrip),
                "show_histogram" => Some(&mut config.show_histogram),
                "show_info" => Some(&mut config.show_info),
                "log_counts" => Some(&mut config.log_counts),
                "pixel_format" => {
                    match PixelFormat::parse(value) {
                        Some(format) => config.pixel_format = format,
                        None => problems.push((
                            number,
                            format!("unknown pixel_format `{value}`: hex, decimal, or mapped"),
                        )),
                    }
                    None
                }
                _ => {
                    problems.push((number, format!("unknown setting `{name}`")));
                    None
                }
            };
            if let Some(flag) = flag {
                match value {
                    "true" => *flag = true,
                    "false" => *flag = false,
                    _ => problems.push((number, format!("{name} is true or false, not `{value}`"))),
                }
            }
        }
        (config, problems)
    }

    /// A `keys.` line: `name` bound to the chords in `value`, whitespace
    /// between them. A chord that does not read is said and left out, the
    /// rest still bound; a chord taken from a name an earlier line set is
    /// said with both lines.
    fn parse_keys(
        &mut self,
        name: &str,
        value: &str,
        number: usize,
        set_on: &mut Vec<(&'static str, usize)>,
        problems: &mut Vec<(usize, String)>,
    ) {
        let mut chords = Vec::new();
        let mut tokens = Vec::new();
        for token in value.split_whitespace() {
            match Chord::read(token) {
                Ok(chord) => {
                    chords.push(chord);
                    tokens.push(token);
                }
                Err(problem) => problems.push((number, problem)),
            }
        }
        let before = self.keys.clone();
        match self.keys.bind(name, chords.clone()) {
            Ok(displaced) => {
                for other in displaced {
                    let Some((_, line)) = set_on.iter().find(|(each, _)| *each == other) else {
                        continue;
                    };
                    let taken: Vec<&str> = chords
                        .iter()
                        .zip(&tokens)
                        .filter(|(chord, _)| before.chords_of(other).contains(chord))
                        .map(|(_, token)| *token)
                        .collect();
                    problems.push((
                        number,
                        format!(
                            "`{}` was {other} on line {line}; {name} takes it",
                            taken.join(" ")
                        ),
                    ));
                }
                if let Some(static_name) = self.keys.name_of(name) {
                    set_on.retain(|(each, _)| *each != static_name);
                    set_on.push((static_name, number));
                }
            }
            Err(problem) => problems.push((number, problem)),
        }
    }

    /// A `gesture.` line: the slot `name` given the behavior `value`.
    fn parse_gesture(&mut self, name: &str, value: &str) -> Result<(), String> {
        let slot = Slot::read(name)?;
        let behavior = Behavior::read(&slot, value)?;
        if let Behavior::Click(key) = &behavior
            && self.keys.action_named(key).is_none()
        {
            return Err(format!("unknown key name `{key}` for a click"));
        }
        self.gestures.set(slot, behavior);
        Ok(())
    }
}

/// What the program keeps for itself from one run to the next.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct State {
    /// The width each of the file list's thumbnails is fitted into, in
    /// logical pixels, which the panel's own width is made from.
    pub filmstrip_width: f32,
    /// One of [`loupe::MAGNIFICATIONS`].
    pub loupe_magnification: f32,
}

impl Default for State {
    fn default() -> Self {
        Self {
            filmstrip_width: filmstrip::SLOT_DEFAULT,
            loupe_magnification: loupe::DEFAULT_MAGNIFICATION,
        }
    }
}

impl State {
    /// `text` as a state file. A value out of range is the default rather
    /// than the nearest one in range: a width past either end is a file
    /// someone else wrote, not a drag.
    fn parse(text: &str) -> Self {
        let mut state = Self::default();
        for (_, line) in lines(text) {
            let Some((name, value)) = line else {
                continue;
            };
            let Ok(number) = value.parse::<f32>() else {
                continue;
            };
            match name {
                "filmstrip_width"
                    if (filmstrip::SLOT_MIN..=filmstrip::SLOT_MAX).contains(&number) =>
                {
                    state.filmstrip_width = number;
                }
                "loupe_magnification" if loupe::MAGNIFICATIONS.contains(&number) => {
                    state.loupe_magnification = number;
                }
                _ => {}
            }
        }
        state
    }

    fn render(&self) -> String {
        format!(
            "# Kept by {PROGRAM} between runs, and written when its window closes.\n\
             # Deleting this file starts it afresh.\n\
             filmstrip_width = {}\n\
             loupe_magnification = {}\n",
            self.filmstrip_width, self.loupe_magnification,
        )
    }
}

/// The state file, where it is and what it said when the program started:
/// what is written back is compared with that, so a run that changed
/// nothing writes nothing.
pub struct StateFile {
    path: Option<PathBuf>,
    loaded: State,
}

impl StateFile {
    /// The state file, read. No file, or one that cannot be read, is every
    /// default.
    pub fn load() -> Self {
        let path = state_path();
        let loaded = path
            .as_deref()
            .and_then(|path| fs::read_to_string(path).ok())
            .map(|text| State::parse(&text))
            .unwrap_or_default();
        Self { path, loaded }
    }

    /// No file at all: every default, and nothing ever written. What the
    /// tests start from, so that none of them reads or writes the user's.
    #[cfg(test)]
    pub fn none() -> Self {
        Self {
            path: None,
            loaded: State::default(),
        }
    }

    pub fn state(&self) -> State {
        self.loaded
    }

    /// Writes `state` back where it was read from, where it differs from
    /// what was read. A failure is said on the terminal and is otherwise
    /// nothing: the window is closing, and the next run starts from the
    /// defaults.
    pub fn save(&self, state: State) {
        let Some(path) = &self.path else {
            return;
        };
        if state == self.loaded {
            return;
        }
        if let Err(error) = write(path, &state.render()) {
            eprintln!("{PROGRAM}: {error:#}");
        }
    }
}

fn config_path() -> Option<PathBuf> {
    Some(xdg::config_home()?.join(PROGRAM).join("config"))
}

fn state_path() -> Option<PathBuf> {
    Some(xdg::state_home()?.join(PROGRAM).join("state"))
}

/// The lines of `text` that say something, numbered from one, each as its
/// name and value with the space around them taken off and a value's
/// quotes taken off too, or `None` for a line with no `=` in it. Blank
/// lines and comments are left out.
fn lines(text: &str) -> impl Iterator<Item = (usize, Option<(&str, &str)>)> {
    text.lines().enumerate().filter_map(|(index, line)| {
        let line = line
            .split_once('#')
            .map_or(line, |(before, _)| before)
            .trim();
        if line.is_empty() {
            return None;
        }
        let pair = line.split_once('=').map(|(name, value)| {
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .unwrap_or(value);
            (name.trim(), value)
        });
        Some((index + 1, pair))
    })
}

/// `contents` written to `path` by way of a temporary name beside it, so
/// that the program leaving part way through leaves the old file whole.
fn write(path: &Path, contents: &str) -> Result<()> {
    let dir = path.parent().context("the state file has a directory")?;
    fs::create_dir_all(dir).with_context(|| format!("creating {}", shown_path(dir)))?;
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let written = fs::File::create(&temporary)
        .and_then(|mut file| file.write_all(contents.as_bytes()))
        .with_context(|| format!("writing {}", shown_path(&temporary)))
        .and_then(|()| {
            fs::rename(&temporary, path)
                .with_context(|| format!("moving the state to {}", shown_path(path)))
        });
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_configuration_is_every_default() {
        assert_eq!(Config::parse(""), (Config::default(), Vec::new()));
    }

    #[test]
    fn a_configuration_sets_what_it_names() {
        let (config, problems) = Config::parse(
            "# the window as I like it\n\
             show_ui = false\n\
             \n\
             show_minimap=false   # it gets in the way\n\
             show_histogram = true\n\
             pixel_format = \"Decimal\"\n\
             log_counts = true\n",
        );
        assert_eq!(problems, Vec::new());
        assert_eq!(
            config,
            Config {
                show_ui: false,
                show_minimap: false,
                show_histogram: true,
                pixel_format: PixelFormat::Decimal,
                log_counts: true,
                ..Config::default()
            }
        );
    }

    #[test]
    fn a_line_the_configuration_cannot_use_is_skipped_and_said() {
        let (config, problems) = Config::parse(
            "show_info = yes\n\
             pixel_format = octal\n\
             show_grid = true\n\
             show_filmstrip\n\
             show_histogram = true\n",
        );
        assert_eq!(
            config,
            Config {
                show_histogram: true,
                ..Config::default()
            }
        );
        let lines: Vec<usize> = problems.iter().map(|(line, _)| *line).collect();
        assert_eq!(lines, [1, 2, 3, 4]);
    }

    /// The template, as printed, sets nothing; with every line uncommented
    /// it sets every default, and says nothing is wrong with it.
    #[test]
    fn the_template_is_every_default() {
        let template = Config::template();
        assert_eq!(Config::parse(&template), (Config::default(), Vec::new()));
        let uncommented: String = template
            .lines()
            .map(|line| line.strip_prefix("# ").unwrap_or(line))
            .filter(|line| line.contains(" = "))
            .map(|line| format!("{line}\n"))
            .collect();
        // Every setting, every key's name and every gesture's slot.
        let keys = uncommented
            .lines()
            .filter(|line| line.starts_with("keys."))
            .count();
        let gestures = uncommented
            .lines()
            .filter(|line| line.starts_with("gesture."))
            .count();
        assert_eq!(keys, 90, "{uncommented}");
        assert_eq!(gestures, 9, "{uncommented}");
        assert_eq!(
            uncommented.lines().count(),
            SETTINGS.len() + keys + gestures,
            "{uncommented}"
        );
        assert_eq!(Config::parse(&uncommented), (Config::default(), Vec::new()));
        // The key that hides the panels is named as the keys have it.
        assert!(template.contains("# The panels around the picture; ` hides and shows them.\n"));
    }

    /// A `keys.` line binds its name to its chords, taking them from any
    /// other name that held them; an empty one unbinds; a chord taken from
    /// a name an earlier line set is said, with both lines; and an unknown
    /// name or a chord that does not read is a problem like any other.
    #[test]
    fn a_keys_line_binds_its_name() {
        let (config, problems) = Config::parse(
            "keys.files.undo = ctrl+e\n\
             keys.region.move.left = h\n\
             keys.interface.dismiss =\n\
             keys.nope = a\n\
             keys.files.rename = f2 shift+.\n\
             keys.interface.grid = ctrl+e\n",
        );
        assert_eq!(config.keys.spelled("files.undo"), "");
        assert_eq!(config.keys.spelled("interface.grid"), "Ctrl+E");
        assert_eq!(config.keys.spelled("files.export"), "");
        assert_eq!(config.keys.spelled("region.move.left"), "h");
        assert_eq!(config.keys.spelled("interface.dismiss"), "");
        assert_eq!(config.keys.spelled("files.rename"), "F2");
        let lines: Vec<usize> = problems.iter().map(|(line, _)| *line).collect();
        assert_eq!(lines, [4, 5, 6], "{problems:?}");
        assert_eq!(
            problems[2].1,
            "`ctrl+e` was files.undo on line 1; interface.grid takes it"
        );
    }

    /// A `gesture.` line puts one behavior in one slot; a word its kind does
    /// not take, a surface there is none of and a click naming no key are
    /// problems.
    #[test]
    fn a_gesture_line_sets_its_slot() {
        use crate::gestures::{Button, HoldAction, Mods, Surface};
        let (config, problems) = Config::parse(
            "gesture.image.middle.hold = loupe\n\
             gesture.image.wheel = none\n\
             gesture.image.left.drag = loupe\n\
             gesture.desk.left.drag = pan\n\
             gesture.image.middle.click = files.nope\n\
             gesture.image.middle.click = interface.grid\n",
        );
        assert_eq!(
            config
                .gestures
                .hold(Surface::Image, Mods::empty(), Button::Middle),
            Some(HoldAction::Loupe)
        );
        assert_eq!(
            config.gestures.wheel(Surface::Image, Mods::empty(), None),
            None
        );
        assert_eq!(
            config
                .gestures
                .click(Surface::Image, Mods::empty(), Button::Middle),
            Some("interface.grid")
        );
        let lines: Vec<usize> = problems.iter().map(|(line, _)| *line).collect();
        assert_eq!(lines, [3, 4, 5], "{problems:?}");
    }

    /// Every field of the configuration is in [`SETTINGS`], written and read
    /// back: a configuration unlike the default in every field survives the
    /// round, which it would not with a field the template leaves out.
    #[test]
    fn every_setting_is_written_and_read_back() {
        let defaults = Config::default();
        let changed = Config {
            show_ui: !defaults.show_ui,
            show_minimap: !defaults.show_minimap,
            show_filmstrip: !defaults.show_filmstrip,
            show_histogram: !defaults.show_histogram,
            show_info: !defaults.show_info,
            pixel_format: defaults.pixel_format.next(),
            log_counts: !defaults.log_counts,
            keys: defaults.keys.clone(),
            gestures: defaults.gestures.clone(),
        };
        let text: String = SETTINGS
            .iter()
            .map(|(name, _)| format!("{name} = {}\n", changed.value(name)))
            .collect();
        assert_eq!(Config::parse(&text), (changed, Vec::new()));
    }

    #[test]
    fn the_state_reads_back_what_it_writes() {
        let state = State {
            filmstrip_width: 212.0,
            loupe_magnification: 8.0,
        };
        assert_eq!(State::parse(&state.render()), state);
    }

    #[test]
    fn a_state_out_of_range_is_the_default() {
        let state = State::parse(
            "filmstrip_width = 100000\n\
             loupe_magnification = 3\n\
             garbage\n",
        );
        assert_eq!(state, State::default());
    }

    #[test]
    fn the_state_is_written_whole_and_replaced() {
        let dir = std::env::temp_dir().join(format!("gamut-state-{}", std::process::id()));
        let path = dir.join("nested").join("state");
        write(&path, "one\n").expect("the temporary directory is writable");
        write(&path, "two\n").expect("the temporary directory is writable");
        assert_eq!(fs::read_to_string(&path).unwrap(), "two\n");
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
        let _ = fs::remove_dir_all(&dir);
    }
}
