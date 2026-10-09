//! What the program starts from beyond the command line: the configuration
//! the user writes, and the state it keeps for itself between runs.
//!
//! The two are different files because they are written by different hands.
//! The configuration, `$XDG_CONFIG_HOME/gamut/config`, is the user's: how
//! the window should open every time. The program only reads it, and a
//! toggle pressed in the window leaves it alone. The state,
//! `$XDG_STATE_HOME/gamut/state`, is the program's: what was left where it
//! was set by hand — the file list's width where it was dragged, the
//! loupe's magnification where the wheel left it, how large the interface
//! was made, the order its menu put
//! the list in, how the pointer's readout was last written — written when
//! the window
//! closes, and nothing lost when it is deleted.
//!
//! Both are lines of `name = value`, with `#` starting a comment. The
//! configuration's names are its settings, and beside them every key's
//! dotted name under `keys.` and every gesture's slot under `gesture.`. A missing
//! file is every default, and none is ever written for the user: a line
//! written out on their behalf would hold them to today's default after it
//! changed. [`Config::template`] is what `--print-config` prints instead —
//! every setting at its default, commented out — and what [`edit`] writes
//! where it is asked to open a file that is not there, which holds nobody
//! to anything for the same reason. A line the configuration cannot use is said on
//! the terminal and in the window, and skipped, the rest still taken; the state file is ours,
//! so a line in it that does not read is only dropped.

use std::fs;
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::app::keymap::{Chord, Keymap};
use crate::gestures::{Behavior, Gestures, Slot};
use crate::ui::{CoordinateFormat, GeographicFormat, PixelFormat};
use crate::ui::{filmstrip, loupe, scale};
use crate::{PROGRAM, shown_path, xdg};

/// The configuration: which panels the window opens with, and what the keys
/// and the mouse do. A
/// command-line flag for the same thing wins over it.
#[derive(Clone, PartialEq, Debug)]
pub struct Config {
    pub show_ui: bool,
    pub show_minimap: bool,
    pub show_filmstrip: bool,
    pub show_histogram: bool,
    pub show_info: bool,
    pub log_counts: bool,
    /// Whether a single file named on the command line steps on through
    /// the other images in its folder.
    pub browse_folder: bool,
    /// The web address the `Location` section's map button opens, with
    /// `{lat}` and `{lng}` standing for the coordinates in signed degrees.
    pub open_map_link: String,
    /// The program the info panel's Tags tab runs: a name looked for in
    /// `PATH` and the places packages put it, or a path.
    pub exiftool: String,
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
            log_counts: false,
            browse_folder: true,
            open_map_link: OPEN_MAP_LINK.to_string(),
            exiftool: "exiftool".to_string(),
            keys: Keymap::default(),
            gestures: Gestures::default(),
        }
    }
}

/// One setting of the configuration file: its name, the words the template
/// gives it, and the field it is, read and written as the file has it.
struct Setting {
    name: &'static str,
    words: &'static str,
    /// The value as the file writes it.
    get: fn(&Config) -> String,
    /// Sets the field from the file's value, or says what is wrong with the
    /// value — after the setting's name, which [`Config::parse`] puts in
    /// front.
    set: fn(&mut Config, &str) -> Result<(), String>,
}

/// Every setting the configuration file takes, in the order the template
/// lists them. [`Config::template`] writes each one's value, and
/// [`Config::parse`] reads it back.
const SETTINGS: [Setting; 9] = [
    Setting {
        name: "show_ui",
        words: "The panels around the picture.",
        get: |config| config.show_ui.to_string(),
        set: |config, value| flag(&mut config.show_ui, value),
    },
    Setting {
        name: "show_minimap",
        words: "The minimap, while the picture is larger than the window.",
        get: |config| config.show_minimap.to_string(),
        set: |config, value| flag(&mut config.show_minimap, value),
    },
    Setting {
        name: "show_filmstrip",
        words: "The file list down the left, while there is more than one file.",
        get: |config| config.show_filmstrip.to_string(),
        set: |config, value| flag(&mut config.show_filmstrip, value),
    },
    Setting {
        name: "show_histogram",
        words: "The histogram panel.",
        get: |config| config.show_histogram.to_string(),
        set: |config, value| flag(&mut config.show_histogram, value),
    },
    Setting {
        name: "show_info",
        words: "The file information panel.",
        get: |config| config.show_info.to_string(),
        set: |config, value| flag(&mut config.show_info, value),
    },
    Setting {
        name: "log_counts",
        words: "The histogram's bars as tall as the logarithm of their counts.",
        get: |config| config.log_counts.to_string(),
        set: |config, value| flag(&mut config.log_counts, value),
    },
    Setting {
        name: "browse_folder",
        words: "A single file opened alone steps on through the other images in its folder.",
        get: |config| config.browse_folder.to_string(),
        set: |config, value| flag(&mut config.browse_folder, value),
    },
    Setting {
        name: "open_map_link",
        words: "The web page the map button in the info panel's Location section opens: {lat} and {lng} are where the picture was taken, in degrees.",
        get: |config| config.open_map_link.clone(),
        set: |config, value| {
            // A link without both would open the same place for every
            // picture, or nowhere.
            if !(value.contains("{lat}") && value.contains("{lng}")) {
                return Err("needs {lat} and {lng} in it".to_string());
            }
            config.open_map_link = value.to_string();
            Ok(())
        },
    },
    Setting {
        name: "exiftool",
        words: "The program the info panel's Raw Data tab reads every tag with: a name looked for on the PATH, or a path.",
        get: |config| config.exiftool.clone(),
        set: |config, value| {
            if value.is_empty() {
                return Err("needs a program name or a path".to_string());
            }
            config.exiftool = value.to_string();
            Ok(())
        },
    },
];

/// A setting that is on or off, as the file writes one.
fn flag(field: &mut bool, value: &str) -> Result<(), String> {
    *field = match value {
        "true" => true,
        "false" => false,
        _ => return Err(format!("is true or false, not `{value}`")),
    };
    Ok(())
}

/// Where the map button goes unless the configuration says otherwise.
pub const OPEN_MAP_LINK: &str = "https://geojson.io/#data=data:application/json,%7B%22type%22%3A%22Feature%22%2C%22properties%22%3A%7B%7D%2C%22geometry%22%3A%7B%22type%22%3A%22Point%22%2C%22coordinates%22%3A%5B{lng}%2C{lat}%5D%7D%7D";

/// `link` with the coordinates in it: `{lat}` the latitude and `{lng}` the
/// longitude, each in signed degrees to six places, which is a tenth of a
/// meter on the ground.
pub fn map_link(link: &str, [latitude, longitude]: [f64; 2]) -> String {
    link.replace("{lat}", &format!("{latitude:.6}"))
        .replace("{lng}", &format!("{longitude:.6}"))
}

/// Settings the configuration file once took, which the state file keeps
/// now: how the pointer's pixel and place are written, chosen by hand from
/// the readout's menu and remembered where they were left, like everything
/// else set there. A line naming one is passed over.
const RETIRED: [&str; 3] = ["pixel_format", "coordinate_format", "geographic_format"];

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
             # take the # off a line to change it. --histogram, --info,\n\
             # --no-minimap and --alone win over what is set here.\n"
        );
        let config = Self::default();
        for setting in &SETTINGS {
            // Which key hides them, as the keys below have it.
            let key = config.keys.spelled("interface.toggle");
            let words = match setting.name {
                "show_ui" if !key.is_empty() => {
                    format!("The panels around the picture; {key} hides and shows them.")
                }
                _ => setting.words.to_string(),
            };
            text.push_str(&format!(
                "\n# {words}\n# {} = {}\n",
                setting.name,
                (setting.get)(&config)
            ));
        }
        text.push_str(&config.keys.template());
        text.push_str(&config.gestures.template());
        text
    }

    /// The configuration file, read, and a word for the window about what
    /// it could not use, each thing of which is said on the terminal too.
    /// No file, or no directory to look for one in, is every default, and
    /// nothing to say.
    pub fn load() -> (Self, Option<String>) {
        let Some(path) = config_path() else {
            return (Self::default(), None);
        };
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return (Self::default(), None),
            Err(error) => {
                eprintln!("{PROGRAM}: reading {}: {error}", shown_path(&path));
                let said = format!("The configuration could not be read: {error}");
                return (Self::default(), Some(said));
            }
        };
        let (config, problems) = Self::parse(&text);
        for (line, problem) in &problems {
            eprintln!("{PROGRAM}: {} line {line}: {problem}", shown_path(&path));
        }
        (config, complaint(&problems))
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
            match SETTINGS.iter().find(|setting| setting.name == name) {
                Some(setting) => {
                    if let Err(problem) = (setting.set)(&mut config, value) {
                        problems.push((number, format!("{name} {problem}")));
                    }
                }
                // Remembered in the state file now, as the readout's menu
                // leaves them; a line written for an older version is
                // passed over rather than called a mistake.
                None if RETIRED.contains(&name) => {}
                None => problems.push((number, format!("unknown setting `{name}`"))),
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
    /// How large the interface is drawn, on top of the monitor's own scale.
    pub ui_scale: f32,
    /// What the file list was last sorted by, and which way.
    pub order: filmstrip::Order,
    /// Whether a raw opens as the camera's JPEG of it rather than as the
    /// picture developed from its sensor counts.
    pub camera_jpeg: bool,
    /// How the pointer's pixel was last written.
    pub pixel_format: PixelFormat,
    /// How its place was last written, in a georeferenced file.
    pub coordinate_format: CoordinateFormat,
    /// And a latitude and longitude, where it was one.
    pub geographic_format: GeographicFormat,
}

impl Default for State {
    fn default() -> Self {
        Self {
            filmstrip_width: filmstrip::SLOT_DEFAULT,
            loupe_magnification: loupe::DEFAULT_MAGNIFICATION,
            ui_scale: scale::DEFAULT,
            order: filmstrip::Order::default(),
            camera_jpeg: false,
            pixel_format: PixelFormat::default(),
            coordinate_format: CoordinateFormat::default(),
            geographic_format: GeographicFormat::default(),
        }
    }
}

/// One line of the state file: its name, and the field it is, written as
/// the file has it and read back from it.
struct Kept {
    name: &'static str,
    /// The value as the file writes it.
    get: fn(&State) -> String,
    /// Sets the field from the file's value, where the value is one it can
    /// be; a value that is not is left alone, the field keeping its default.
    set: fn(&mut State, &str),
}

/// Every line of the state file, in the order it is written.
const KEPT: [Kept; 9] = [
    Kept {
        name: "filmstrip_width",
        get: |state| state.filmstrip_width.to_string(),
        set: |state, value| {
            if let Ok(number) = value.parse::<f32>()
                && (filmstrip::SLOT_MIN..=filmstrip::SLOT_MAX).contains(&number)
            {
                state.filmstrip_width = number;
            }
        },
    },
    Kept {
        name: "loupe_magnification",
        get: |state| state.loupe_magnification.to_string(),
        set: |state, value| {
            if let Ok(number) = value.parse::<f32>()
                && loupe::MAGNIFICATIONS.contains(&number)
            {
                state.loupe_magnification = number;
            }
        },
    },
    Kept {
        name: "ui_scale",
        get: |state| state.ui_scale.to_string(),
        set: |state, value| {
            if let Ok(number) = value.parse::<f32>()
                && number.is_finite()
                && scale::RANGE.contains(&number)
            {
                state.ui_scale = number;
            }
        },
    },
    Kept {
        name: "sort",
        get: |state| state.order.sort.word().to_string(),
        set: |state, value| {
            if let Some(sort) = filmstrip::Sort::read(value) {
                state.order.sort = sort;
            }
        },
    },
    Kept {
        name: "sort_direction",
        get: |state| state.order.direction.word().to_string(),
        set: |state, value| {
            if let Some(direction) = filmstrip::Direction::read(value) {
                state.order.direction = direction;
            }
        },
    },
    Kept {
        name: "camera_jpeg",
        get: |state| state.camera_jpeg.to_string(),
        set: |state, value| {
            let _ = flag(&mut state.camera_jpeg, value);
        },
    },
    Kept {
        name: "pixel_format",
        get: |state| state.pixel_format.label().to_ascii_lowercase(),
        set: |state, value| {
            if let Some(format) = PixelFormat::parse(value) {
                state.pixel_format = format;
            }
        },
    },
    Kept {
        name: "coordinate_format",
        get: |state| state.coordinate_format.label().to_ascii_lowercase(),
        set: |state, value| {
            if let Some(format) = CoordinateFormat::parse(value) {
                state.coordinate_format = format;
            }
        },
    },
    Kept {
        name: "geographic_format",
        get: |state| state.geographic_format.label().to_ascii_lowercase(),
        set: |state, value| {
            if let Some(format) = GeographicFormat::parse(value) {
                state.geographic_format = format;
            }
        },
    },
];

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
            if let Some(kept) = KEPT.iter().find(|kept| kept.name == name) {
                (kept.set)(&mut state, value);
            }
        }
        state
    }

    fn render(&self) -> String {
        let mut text = format!(
            "# Kept by {PROGRAM} between runs, and written when its window closes.\n\
             # Deleting this file starts it afresh.\n"
        );
        for kept in &KEPT {
            text.push_str(&format!("{} = {}\n", kept.name, (kept.get)(self)));
        }
        text
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

    /// No file, and `state` as if one had been read: for a test that wants
    /// the program to start from something other than the defaults.
    #[cfg(test)]
    pub fn holding(state: State) -> Self {
        Self {
            path: None,
            loaded: state,
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

/// What the window says about the lines of the configuration it could not
/// use: the first of them, and how many more, the terminal having the whole
/// of each. Without the file's path, which the reader knows and the foot of
/// the window has no room for. `None` when every line was used.
fn complaint(problems: &[(usize, String)]) -> Option<String> {
    let ((line, problem), rest) = problems.split_first()?;
    let problem = crate::escape_controls(problem);
    Some(match rest.len() {
        0 => format!("Configuration line {line}: {problem}"),
        more => format!("Configuration line {line}: {problem} (and {more} more)"),
    })
}

/// Opens the configuration file in the user's editor — see
/// `openers::edit` — writing the template to it first where there is no
/// file: `Ctrl+,`, the button under the help popup's table, and the Mac's
/// Settings item. The keys, the gestures and the map link changed there are
/// read as the file is saved — see `App::reconfigure`.
pub fn edit() -> Result<()> {
    let path = config_path().context("no configuration directory: HOME is unset")?;
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory)
            .with_context(|| format!("creating {}", shown_path(directory)))?;
    }
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(mut file) => file
            .write_all(Config::template().as_bytes())
            .with_context(|| format!("writing {}", shown_path(&path)))?,
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(error).with_context(|| format!("creating {}", shown_path(&path)));
        }
    }
    crate::openers::edit(&path)
}

/// Where the configuration file is, whether or not there is one.
pub fn config_path() -> Option<PathBuf> {
    Some(xdg::config_home()?.join(PROGRAM).join("config"))
}

fn state_path() -> Option<PathBuf> {
    Some(xdg::state_home()?.join(PROGRAM).join("state"))
}

/// The lines of `text` that say something, numbered from one, each as its
/// name and value with the space around them taken off and a value's
/// quotes taken off too, or `None` for a line with no `=` in it. Blank
/// lines and comments are left out.
///
/// A comment starts at a `#` that begins the line or follows a space: a
/// web address holds `#` inside itself, as the default map link does, and
/// is not cut short there.
fn lines(text: &str) -> impl Iterator<Item = (usize, Option<(&str, &str)>)> {
    text.lines().enumerate().filter_map(|(index, line)| {
        let comment = line.char_indices().find(|&(at, c)| {
            c == '#'
                && line[..at]
                    .chars()
                    .next_back()
                    .is_none_or(char::is_whitespace)
        });
        let line = comment.map_or(line, |(at, _)| &line[..at]).trim();
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
    use crate::gestures::spelled_here;

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
             log_counts = true\n",
        );
        assert_eq!(problems, Vec::new());
        assert_eq!(
            config,
            Config {
                show_ui: false,
                show_minimap: false,
                show_histogram: true,
                log_counts: true,
                ..Config::default()
            }
        );
    }

    #[test]
    fn a_line_the_configuration_cannot_use_is_skipped_and_said() {
        let (config, problems) = Config::parse(
            "show_info = yes\n\
             show_ui = maybe\n\
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

    /// The window is told the first line the configuration could not use,
    /// and how many more there were.
    #[test]
    fn the_window_is_told_the_first_problem_and_how_many_more() {
        let (_, problems) = Config::parse(
            "show_info = true\n\
             keys.zoom.fit = space Double-click\n\
             show_grid = true\n",
        );
        assert_eq!(
            complaint(&problems).as_deref(),
            Some("Configuration line 2: unknown key `Double-click` (and 1 more)")
        );
        assert_eq!(
            complaint(&problems[..1]).as_deref(),
            Some("Configuration line 2: unknown key `Double-click`")
        );
        assert_eq!(complaint(&[]), None);
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
            // A name with no chord by default is written with nothing after
            // its `=`.
            .filter(|line| line.contains(" = ") || line.ends_with(" ="))
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
        assert_eq!(keys, 103, "{uncommented}");
        // A Mac's two more: the wheel with Command, and the pinch.
        let slots = if cfg!(target_os = "macos") { 12 } else { 10 };
        assert_eq!(gestures, slots, "{uncommented}");
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
        assert_eq!(
            config.keys.spelled("interface.grid"),
            spelled_here("Ctrl+E")
        );
        assert!(
            !config
                .keys
                .spelled("files.export")
                .contains(&spelled_here("Ctrl+E"))
        );
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
        use crate::gestures::{Button, HoldAction, Kind, Mods, Surface};
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
                .click(Surface::Image, Mods::empty(), Button::Middle, Kind::Click),
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
            log_counts: !defaults.log_counts,
            browse_folder: !defaults.browse_folder,
            open_map_link: "https://www.openstreetmap.org/?mlat={lat}&mlon={lng}".to_string(),
            exiftool: "/opt/exiftool/exiftool".to_string(),
            keys: defaults.keys.clone(),
            gestures: defaults.gestures.clone(),
        };
        let text: String = SETTINGS
            .iter()
            .map(|setting| format!("{} = {}\n", setting.name, (setting.get)(&changed)))
            .collect();
        assert_eq!(Config::parse(&text), (changed, Vec::new()));
    }

    /// The map's address takes the coordinates where it says, signed; one
    /// that does not say where both go is a problem, and the default stands.
    #[test]
    fn the_map_link_takes_the_coordinates() {
        assert_eq!(
            map_link(OPEN_MAP_LINK, [-44.68202, 169.161956]),
            "https://geojson.io/#data=data:application/json,%7B%22type%22%3A%22Feature%22%2C%22properties%22%3A%7B%7D%2C%22geometry%22%3A%7B%22type%22%3A%22Point%22%2C%22coordinates%22%3A%5B169.161956%2C-44.682020%5D%7D%7D"
        );
        let (config, problems) = Config::parse("open_map_link = https://example.com/?q={lat}\n");
        assert_eq!(config.open_map_link, OPEN_MAP_LINK);
        assert_eq!(problems.len(), 1, "{problems:?}");
        // The default's `#` is part of the address, not a comment; one
        // after a space is.
        let (config, problems) =
            Config::parse(&format!("open_map_link = {OPEN_MAP_LINK}  # the default\n"));
        assert_eq!(config.open_map_link, OPEN_MAP_LINK);
        assert!(problems.is_empty(), "{problems:?}");
    }

    /// exiftool is found by the name or path given, and a setting naming
    /// nothing is a problem, the default standing.
    #[test]
    fn exiftool_needs_a_name() {
        let (config, problems) = Config::parse("exiftool = \n");
        assert_eq!(config.exiftool, "exiftool");
        assert_eq!(problems.len(), 1, "{problems:?}");
        let (config, problems) = Config::parse("exiftool = /usr/bin/vendor_perl/exiftool\n");
        assert_eq!(config.exiftool, "/usr/bin/vendor_perl/exiftool");
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn the_state_reads_back_what_it_writes() {
        let state = State {
            filmstrip_width: 212.0,
            loupe_magnification: 8.0,
            ui_scale: 1.5,
            order: filmstrip::Order {
                sort: filmstrip::Sort::Date,
                direction: filmstrip::Direction::Descending,
            },
            camera_jpeg: true,
            pixel_format: PixelFormat::Depth,
            coordinate_format: CoordinateFormat::Geographic,
            geographic_format: GeographicFormat::Dms,
        };
        assert_eq!(State::parse(&state.render()), state);
    }

    /// The readout's formats were settings of the configuration once, and
    /// are the state's now: a line an older configuration has for one is
    /// passed over, not reported, whatever it says.
    #[test]
    fn a_retired_setting_is_passed_over() {
        let (config, problems) = Config::parse(
            "pixel_format = decimal\n\
             coordinate_format = geographic\n\
             geographic_format = nonsense\n\
             show_info = true\n",
        );
        assert_eq!(problems, Vec::new());
        assert_eq!(
            config,
            Config {
                show_info: true,
                ..Config::default()
            }
        );
    }

    #[test]
    fn every_order_reads_back() {
        for sort in filmstrip::Sort::ALL {
            for direction in filmstrip::Direction::ALL {
                let state = State {
                    order: filmstrip::Order { sort, direction },
                    ..State::default()
                };
                assert_eq!(State::parse(&state.render()), state);
            }
        }
    }

    #[test]
    fn a_state_out_of_range_is_the_default() {
        let state = State::parse(
            "filmstrip_width = 100000\n\
             loupe_magnification = 3\n\
             ui_scale = 9\n\
             sort = shoe size\n\
             sort_direction = sideways\n\
             camera_jpeg = maybe\n\
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
