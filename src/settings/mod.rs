//! What the program starts from beyond the command line: the configuration
//! the user writes, and the state it keeps for itself between runs.
//!
//! The two are different files because they are written by different hands.
//! The configuration, `$XDG_CONFIG_HOME/gamut/config.toml`, is the user's:
//! how the window should open every time. The program only reads it, and a
//! toggle pressed in the window leaves it alone. The state,
//! `$XDG_STATE_HOME/gamut/state`, is the program's: what was left where it
//! was set by hand — the file list's width where it was dragged, the
//! loupe's magnification where the wheel left it, how large the interface
//! was made, the order its menu put the list in, how the pointer's readout
//! was last written — written when the window closes, and nothing lost when
//! it is deleted.
//!
//! The configuration is TOML — see [`document`] for how it is read. Its
//! names are its settings, and beside them every key's dotted name under
//! `keys.`, every gesture's slot under `gesture.`, and a table under
//! `command.` for each command. A missing file is every default, and none
//! is ever written for the user: a line written out on their behalf would
//! hold them to today's default after it changed. [`Config::template`] is
//! what `--print-config` prints instead — every setting at its default,
//! commented out — and what [`edit`] writes where it is asked to open a file
//! that is not there, which holds nobody to anything for the same reason.
//! The one file written for them is a configuration in the old format,
//! `config`, rewritten once as `config.toml` — see [`legacy`]. A line the
//! configuration cannot use is said on the terminal and in the window, and
//! skipped, the rest still taken.
//!
//! The state is lines of `name = value`, with `#` starting a comment, and
//! ours, so a line in it that does not read is only dropped.

pub(crate) mod document;
mod legacy;

use std::fs;
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::app::input::Action;
use crate::app::keymap::{Chord, Keymap};
use crate::commands::{self, Command, Program};
use crate::gestures::{Behavior, Gestures, Slot};
use crate::image::geo::Corners;
use crate::ui::{CoordinateFormat, GeographicFormat, PixelFormat};
use crate::ui::{attitude, filmstrip, loupe, scale};
use crate::{PROGRAM, shown_path, xdg};
use document::{Lines, Value, shown};

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
    /// Whether a single file named on the command line steps on through
    /// the other images in its folder.
    pub browse_folder: bool,
    /// How many images walking the directories named may add to the list
    /// in a session before it stops: a walk started at the top of a disk
    /// by mistake would otherwise read headers and make thumbnails for
    /// hours.
    pub max_files: usize,
    /// The web address the `Location` section's map button opens, with
    /// `{lat}` and `{lng}` standing for the coordinates in signed degrees.
    pub open_map_link: String,
    /// The web address the `Georeference` section's box button opens, with
    /// `{lat_ul}`, `{lng_ul}` and the same for `ur`, `lr` and `ll` standing
    /// for the raster's corners, in signed degrees.
    pub open_map_bounds_link: String,
    /// The program the info panel's Tags tab runs: a name looked for in
    /// `PATH` and the places packages put it, or a path.
    pub exiftool: String,
    pub keys: Keymap,
    pub gestures: Gestures,
    /// The commands its `command.` lines define, in the order each is first
    /// named, and then the built-in ones it leaves in; each also a name of
    /// `keys`, `command.<name>`.
    pub commands: Vec<Command>,
    /// The programs its `program.` tables describe, and then the built-in
    /// ones none of them replaces.
    pub programs: Vec<Program>,
}

impl Config {
    /// Makes `commands` the configuration's, each a name of `keys`.
    fn adopt_commands(&mut self, commands: Vec<Command>, programs: Vec<Program>) {
        self.keys.clear_extras();
        for (index, command) in commands.iter().enumerate() {
            self.keys
                .add_extra(&command.key_name(), &command.label, Action::Command(index));
        }
        self.commands = commands;
        self.programs = programs;
    }
}

impl Default for Config {
    fn default() -> Self {
        let mut config = Self {
            show_ui: true,
            show_minimap: true,
            show_filmstrip: true,
            show_histogram: false,
            show_info: false,
            browse_folder: true,
            max_files: MAX_FILES,
            open_map_link: OPEN_MAP_LINK.to_string(),
            open_map_bounds_link: OPEN_MAP_BOUNDS_LINK.to_string(),
            exiftool: "exiftool".to_string(),
            keys: Keymap::default(),
            gestures: Gestures::default(),
            commands: Vec::new(),
            programs: Vec::new(),
        };
        let builtins = commands::builtins();
        config.adopt_commands(builtins.commands.clone(), builtins.programs.clone());
        config
    }
}

/// One setting of the configuration file: its name, the words the template
/// gives it, and the field it is, read and written as the file has it.
struct Setting {
    name: &'static str,
    words: &'static str,
    get: fn(&Config) -> Plain,
    /// Sets the field from the file's value, or says what is wrong with the
    /// value — after the setting's name, which [`Config::parse`] puts in
    /// front.
    set: fn(&mut Config, &Value) -> Result<(), String>,
}

/// A setting's value, of one of the three shapes a setting has.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Plain {
    Flag(bool),
    Whole(usize),
    Text(String),
}

impl Plain {
    /// As the file writes it.
    pub(crate) fn written(&self) -> String {
        match self {
            Plain::Flag(on) => on.to_string(),
            Plain::Whole(number) => number.to_string(),
            Plain::Text(text) => document::string(text),
        }
    }
}

/// Every setting the configuration file takes, in the order the template
/// lists them. [`Config::template`] writes each one's value, and
/// [`Config::parse`] reads it back.
const SETTINGS: [Setting; 10] = [
    Setting {
        name: "show_ui",
        words: "The panels around the picture.",
        get: |config| Plain::Flag(config.show_ui),
        set: |config, value| flag(&mut config.show_ui, value),
    },
    Setting {
        name: "show_minimap",
        words: "The minimap, while the picture is larger than the window.",
        get: |config| Plain::Flag(config.show_minimap),
        set: |config, value| flag(&mut config.show_minimap, value),
    },
    Setting {
        name: "show_filmstrip",
        words: "The file list down the left, while there is more than one file.",
        get: |config| Plain::Flag(config.show_filmstrip),
        set: |config, value| flag(&mut config.show_filmstrip, value),
    },
    Setting {
        name: "show_histogram",
        words: "The histogram, in the side panel; it wins over show_info where both are on.",
        get: |config| Plain::Flag(config.show_histogram),
        set: |config, value| flag(&mut config.show_histogram, value),
    },
    Setting {
        name: "show_info",
        words: "The file information, in the side panel.",
        get: |config| Plain::Flag(config.show_info),
        set: |config, value| flag(&mut config.show_info, value),
    },
    Setting {
        name: "browse_folder",
        words: "A single file opened alone steps on through the other images in its folder.",
        get: |config| Plain::Flag(config.browse_folder),
        set: |config, value| flag(&mut config.browse_folder, value),
    },
    Setting {
        name: "max_files",
        words: "How many images a folder and its subfolders may add to the list before the search for more stops.",
        get: |config| Plain::Whole(config.max_files),
        set: |config, value| match document::whole(value).map(usize::try_from) {
            Some(Ok(count)) if count > 0 => {
                config.max_files = count;
                Ok(())
            }
            _ => Err(format!(
                "is a whole number above zero, not {}",
                shown(value)
            )),
        },
    },
    Setting {
        name: "open_map_link",
        words: "The web page the map buttons in the info panel's Location and Georeference sections open: {lat} and {lng} are where the picture was taken, or the middle of what it covers, in degrees.",
        get: |config| Plain::Text(config.open_map_link.clone()),
        set: |config, value| {
            let value = text(value)?;
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
        name: "open_map_bounds_link",
        words: "The web page the corners button in the info panel's Georeference section opens: {lat_ul} and {lng_ul}, {lat_ur} and {lng_ur}, {lat_lr} and {lng_lr}, and {lat_ll} and {lng_ll} are the picture's upper left, upper right, lower right and lower left corners, in degrees.",
        get: |config| Plain::Text(config.open_map_bounds_link.clone()),
        set: |config, value| {
            let value = text(value)?;
            // As with the map link: one with no latitude or no longitude in
            // it would open the same place for every picture. Not every
            // corner, since a box can be named by two of them.
            let has = |names: &[&str]| names.iter().any(|name| value.contains(name));
            if !(has(&CORNERS[..4]) && has(&CORNERS[4..])) {
                return Err("needs a corner's {lat_..} and {lng_..} in it".to_string());
            }
            config.open_map_bounds_link = value.to_string();
            Ok(())
        },
    },
    Setting {
        name: "exiftool",
        words: "The program the info panel's Raw Data tab reads every tag with: a name looked for on the PATH, or a path.",
        get: |config| Plain::Text(config.exiftool.clone()),
        set: |config, value| {
            let value = text(value)?;
            if value.is_empty() {
                return Err("needs a program name or a path".to_string());
            }
            config.exiftool = value.to_string();
            Ok(())
        },
    },
];

/// How many images a walk may find unless the configuration says otherwise.
pub const MAX_FILES: usize = 10_000;

/// A setting that is on or off, as the file writes one.
fn flag(field: &mut bool, value: &Value) -> Result<(), String> {
    *field =
        document::flag(value).ok_or_else(|| format!("is true or false, not {}", shown(value)))?;
    Ok(())
}

/// A setting that is words, as the file writes them.
fn text<'a>(value: &'a Value) -> Result<&'a str, String> {
    document::text(value).ok_or_else(|| format!("is a string, in quotes, not {}", shown(value)))
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

/// Where the box button goes unless the configuration says otherwise: the
/// picture's four corners as a GeoJSON polygon, its ring run from the lower
/// left through the lower right, which is counterclockwise on a map with
/// north up, as RFC 7946 has an outer ring run.
pub const OPEN_MAP_BOUNDS_LINK: &str = "https://geojson.io/#data=data:application/json,%7B%22type%22%3A%22Feature%22%2C%22properties%22%3A%7B%7D%2C%22geometry%22%3A%7B%22type%22%3A%22Polygon%22%2C%22coordinates%22%3A%5B%5B%5B{lng_ll}%2C{lat_ll}%5D%2C%5B{lng_lr}%2C{lat_lr}%5D%2C%5B{lng_ur}%2C{lat_ur}%5D%2C%5B{lng_ul}%2C{lat_ul}%5D%2C%5B{lng_ll}%2C{lat_ll}%5D%5D%5D%7D%7D";

/// The names a box's link puts the corners in by: the four latitudes, then
/// the four longitudes.
const CORNERS: [&str; 8] = [
    "{lat_ul}", "{lat_ur}", "{lat_lr}", "{lat_ll}", "{lng_ul}", "{lng_ur}", "{lng_lr}", "{lng_ll}",
];

/// `link` with the picture's corners in it, each in signed degrees to six
/// places, as [`map_link`] writes a point's.
pub fn map_bounds_link(link: &str, corners: Corners) -> String {
    let Corners {
        upper_left,
        upper_right,
        lower_right,
        lower_left,
    } = corners;
    [upper_left, upper_right, lower_right, lower_left]
        .into_iter()
        .zip(["ul", "ur", "lr", "ll"])
        .fold(link.to_string(), |link, ([latitude, longitude], corner)| {
            link.replace(&format!("{{lat_{corner}}}"), &format!("{latitude:.6}"))
                .replace(&format!("{{lng_{corner}}}"), &format!("{longitude:.6}"))
        })
}

/// Settings the configuration file once took: how the pointer's pixel and
/// place are written, which the state file keeps now, chosen by hand from
/// the readout's menu and remembered where they were left, like everything
/// else set there; and the histogram's logarithmic counts, which its one
/// scale down the column has taken the place of. A line naming one is
/// passed over.
const RETIRED: [&str; 4] = [
    "pixel_format",
    "coordinate_format",
    "geographic_format",
    "log_counts",
];

/// Key names the table once had: the histogram's logarithmic counts, which
/// its one scale down the column has taken the place of. A `keys.` line
/// naming one is passed over.
const RETIRED_KEYS: [&str; 1] = ["interface.log-counts"];

impl Config {
    /// The configuration as a file: every setting at its default, commented
    /// out, under a line saying what it does; then every key's name at its
    /// chords, and every gesture's slot at its behavior. Uncommenting a line
    /// and changing its value is the whole of editing it, and a line left
    /// commented follows the default wherever it goes.
    pub fn template() -> String {
        let mut text = Self::header();
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
                (setting.get)(&config).written()
            ));
        }
        text.push_str(&config.keys.template());
        text.push_str(&config.gestures.template());
        text.push_str(commands::TEMPLATE);
        text.push_str(&commands::builtins_template());
        text
    }

    /// The words the template opens with.
    fn header() -> String {
        format!(
            "# {PROGRAM}'s configuration, in TOML: how the window opens, and what the\n\
             # keys and the mouse do. Each setting is shown at its default, commented\n\
             # out; take the # off a line to change it. Words go in quotes. --histogram,\n\
             # --info, --no-minimap and --alone win over what is set here.\n"
        )
    }

    /// The configuration file, read — where there is none yet but there is
    /// one in the old format, that one rewritten as TOML first — with what
    /// the window should say about it, each thing of which is said on the
    /// terminal too. No file, or no directory to look for one in, is every
    /// default, and nothing to say.
    pub fn load() -> Loaded {
        let Some(path) = config_path() else {
            return Loaded::from(Self::default());
        };
        match fs::read_to_string(&path) {
            Ok(text) => return Self::read(&text, &path, |line| line),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Loaded::unread(&path, &error),
        }
        let Some(old) = legacy_path() else {
            return Loaded::from(Self::default());
        };
        let text = match fs::read_to_string(&old) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Loaded::from(Self::default());
            }
            Err(error) => return Loaded::unread(&old, &error),
        };
        let rewritten = legacy::rewrite(&text, &old);
        match write_new(&path, &rewritten.text) {
            Ok(()) => {
                eprintln!(
                    "{PROGRAM}: rewrote {} as {}, which is read from now on; the old file is \
                     left as it was",
                    shown_path(&old),
                    shown_path(&path)
                );
                let loaded = Self::read(&rewritten.text, &path, |line| line);
                Loaded {
                    rewrite: Some(Rewrite::Written),
                    ..loaded
                }
            }
            Err(error) => {
                eprintln!(
                    "{PROGRAM}: {error:#}; reading {} as it would have been rewritten",
                    shown_path(&old)
                );
                // Said against the file the user has, line for line.
                let loaded = Self::read(&rewritten.text, &old, |line| rewritten.origin(line));
                Loaded {
                    rewrite: Some(Rewrite::Unwritten),
                    ..loaded
                }
            }
        }
    }

    /// `text` as a configuration, its problems said on the terminal against
    /// `path` at the line `origin` makes of each.
    fn read(text: &str, path: &Path, origin: impl Fn(usize) -> usize) -> Loaded {
        let (config, problems) = Self::parse(text);
        let problems: Vec<(usize, String)> = problems
            .into_iter()
            .map(|(line, problem)| (origin(line), problem))
            .collect();
        for (line, problem) in &problems {
            eprintln!("{PROGRAM}: {} line {line}: {problem}", shown_path(path));
        }
        Loaded {
            config,
            complaint: complaint(&problems),
            rewrite: None,
        }
    }

    /// `text` as a configuration, and each thing in it that could not be
    /// used, by line, with what was wrong with it. A chord two lines give two
    /// names in one context goes to the later, which is said.
    pub(crate) fn parse(text: &str) -> (Self, Vec<(usize, String)>) {
        let mut config = Self::default();
        let lines = Lines::new(text);
        let (clean, mut problems) = document::readable(text);
        let root = document::table(&clean);
        let root = root.get_ref();
        // The commands first, wherever their tables are, so that a `keys.`
        // or a gesture's line can name one the file defines further down;
        // and the programs with them, which say what each can write.
        let mut parsed = commands::Parsed::default();
        if let Some((key, value)) = root.get_key_value("command") {
            match value.get_ref() {
                Value::Table(table) => parsed = commands::parse(table, &lines),
                other => problems.push((
                    lines.of(key.span()),
                    format!("command is a table of commands, not {}", shown(other)),
                )),
            }
        }
        let mut programs = Vec::new();
        if let Some((key, value)) = root.get_key_value("program") {
            match value.get_ref() {
                Value::Table(table) => {
                    let (read, said) = commands::parse_programs(table, &lines);
                    programs = read;
                    problems.extend(said);
                }
                other => problems.push((
                    lines.of(key.span()),
                    format!("program is a table of programs, not {}", shown(other)),
                )),
            }
        }
        let (commands, programs, said) =
            commands::with_builtins(parsed, programs, commands::builtins());
        problems.extend(said);
        config.adopt_commands(commands, programs);
        // Which line set each key's name, for saying which a later line
        // took a chord from.
        let mut set_on: Vec<(String, usize)> = Vec::new();
        for (key, value) in root.iter() {
            let name: &str = key.get_ref();
            let line = lines.of(key.span());
            match (name, value.get_ref()) {
                ("command" | "program", _) => {}
                ("keys", Value::Table(table)) => {
                    for (name, value) in document::leaves(table) {
                        let line = lines.of(value.span());
                        config.parse_keys(&name, value.get_ref(), line, &mut set_on, &mut problems);
                    }
                }
                ("gesture", Value::Table(table)) => {
                    for (name, value) in document::leaves(table) {
                        if let Err(problem) = config.parse_gesture(&name, value.get_ref()) {
                            problems.push((lines.of(value.span()), problem));
                        }
                    }
                }
                ("keys" | "gesture", other) => problems.push((
                    line,
                    format!("{name} is a table, {name}.<name> = …, not {}", shown(other)),
                )),
                (_, value) => match SETTINGS.iter().find(|setting| setting.name == name) {
                    Some(setting) => {
                        if let Err(problem) = (setting.set)(&mut config, value) {
                            problems.push((line, format!("{name} {problem}")));
                        }
                    }
                    // Remembered in the state file now, as the readout's
                    // menu leaves them; a line written for an older version
                    // is passed over rather than called a mistake.
                    None if RETIRED.contains(&name) => {}
                    None => problems.push((line, format!("unknown setting `{name}`"))),
                },
            }
        }
        problems.sort_by_key(|(number, _)| *number);
        (config, problems)
    }

    /// A `keys.` line: `name` bound to the chords in `value` — a string with
    /// whitespace between them, or an array of them. A chord that does not
    /// read is said and left out, the rest still bound; a chord taken from a
    /// name an earlier line set is said with both lines.
    fn parse_keys(
        &mut self,
        name: &str,
        value: &Value,
        number: usize,
        set_on: &mut Vec<(String, usize)>,
        problems: &mut Vec<(usize, String)>,
    ) {
        if RETIRED_KEYS.contains(&name) {
            return;
        }
        let tokens: Vec<&str> = match value {
            Value::String(text) => text.split_whitespace().collect(),
            Value::Array(items) => match items
                .iter()
                .map(|item| document::text(item.get_ref()))
                .collect::<Option<Vec<&str>>>()
            {
                Some(tokens) => tokens,
                None => {
                    problems.push((number, format!("keys.{name} holds chords, in quotes")));
                    return;
                }
            },
            other => {
                problems.push((
                    number,
                    format!("keys.{name} is its chords in quotes, not {}", shown(other)),
                ));
                return;
            }
        };
        let mut chords = Vec::new();
        let mut read = Vec::new();
        for token in tokens {
            match Chord::read(token) {
                Ok(chord) => {
                    chords.push(chord);
                    read.push(token);
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
                        .zip(&read)
                        .filter(|(chord, _)| before.chords_of(&other).contains(chord))
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
                set_on.retain(|(each, _)| each != name);
                set_on.push((name.to_string(), number));
            }
            Err(problem) => problems.push((number, problem)),
        }
    }

    /// A `gesture.` line: the slot `name` given the behavior `value`.
    fn parse_gesture(&mut self, name: &str, value: &Value) -> Result<(), String> {
        let slot = Slot::read(name)?;
        let word = document::text(value).ok_or_else(|| {
            format!(
                "gesture.{name} is a behavior in quotes, not {}",
                shown(value)
            )
        })?;
        let behavior = Behavior::read(&slot, word)?;
        if let Behavior::Click(key) = &behavior
            && self.keys.action_named(key).is_none()
        {
            return Err(format!("unknown key name `{key}` for a click"));
        }
        self.gestures.set(slot, behavior);
        Ok(())
    }
}

/// What reading the configuration came to.
pub struct Loaded {
    pub config: Config,
    /// What the window says about what the file got wrong: the first thing,
    /// and how many more, the terminal having the whole of each.
    pub complaint: Option<String>,
    /// Whether a file in the old format was found in place of one in TOML.
    pub rewrite: Option<Rewrite>,
}

impl From<Config> for Loaded {
    fn from(config: Config) -> Self {
        Self {
            config,
            complaint: None,
            rewrite: None,
        }
    }
}

impl Loaded {
    /// Every default, the file at `path` being there and unreadable.
    fn unread(path: &Path, error: &std::io::Error) -> Self {
        eprintln!("{PROGRAM}: reading {}: {error}", shown_path(path));
        Self {
            complaint: Some(format!("The configuration could not be read: {error}")),
            ..Self::from(Config::default())
        }
    }
}

/// What became of a configuration file in the old format.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rewrite {
    /// Rewritten as TOML beside it, which is read from now on.
    Written,
    /// Read as it would have been rewritten, the new file not being
    /// writable; it is read so again each time.
    Unwritten,
}

/// Whether `name` is something the top of the file holds: a setting, a
/// retired one, or one of the tables of keys, gestures and commands. What
/// lets a command's table say that a line written below it belongs above.
pub(crate) fn top_level(name: &str) -> bool {
    matches!(name, "keys" | "gesture" | "command" | "program")
        || RETIRED.contains(&name)
        || SETTINGS.iter().any(|setting| setting.name == name)
}

/// What the program keeps for itself from one run to the next.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct State {
    /// The width each of the file list's thumbnails is fitted into, in
    /// logical pixels, which the panel's own width is made from.
    pub filmstrip_width: f32,
    /// One of [`loupe::MAGNIFICATIONS`].
    pub loupe_magnification: f32,
    /// One of [`attitude::STEPS`]: how far apart the horizon grid's
    /// uprights are, in degrees round the camera.
    pub horizon_step: f32,
    /// How large the interface is drawn, on top of the monitor's own scale.
    pub ui_scale: f32,
    /// What the file list was last sorted by, and which way.
    pub order: filmstrip::Order,
    /// Whether a raw opens as the camera's JPEG of it rather than as the
    /// picture developed from its sensor counts.
    pub camera_jpeg: bool,
    /// Whether a raw whose profile says how it should look is shown as the
    /// profile renders it, rather than as developed.
    pub profile: bool,
    /// How the pointer's pixel was last written.
    pub pixel_format: PixelFormat,
    /// How its place was last written, in a georeferenced file.
    pub coordinate_format: CoordinateFormat,
    /// And a latitude and longitude, where it was one.
    pub geographic_format: GeographicFormat,
    /// Whether the folder dialog's box for subfolders was last left ticked.
    pub include_subfolders: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            filmstrip_width: filmstrip::SLOT_DEFAULT,
            loupe_magnification: loupe::DEFAULT_MAGNIFICATION,
            horizon_step: attitude::DEFAULT_STEP,
            ui_scale: scale::DEFAULT,
            order: filmstrip::Order::default(),
            camera_jpeg: false,
            profile: true,
            include_subfolders: false,
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
const KEPT: [Kept; 12] = [
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
        name: "horizon_step",
        get: |state| state.horizon_step.to_string(),
        set: |state, value| {
            if let Ok(number) = value.parse::<f32>()
                && attitude::STEPS.contains(&number)
            {
                state.horizon_step = number;
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
            if let Some(on) = on(value) {
                state.camera_jpeg = on;
            }
        },
    },
    Kept {
        name: "include_subfolders",
        get: |state| state.include_subfolders.to_string(),
        set: |state, value| {
            if let Some(on) = on(value) {
                state.include_subfolders = on;
            }
        },
    },
    Kept {
        name: "profile",
        get: |state| state.profile.to_string(),
        set: |state, value| {
            if let Some(on) = on(value) {
                state.profile = on;
            }
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
        get: |state| state.coordinate_format.word().to_string(),
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
    Some(xdg::config_home()?.join(PROGRAM).join("config.toml"))
}

/// Where the configuration file was before it was TOML, which is read only
/// to be rewritten — see [`legacy`].
fn legacy_path() -> Option<PathBuf> {
    Some(xdg::config_home()?.join(PROGRAM).join("config"))
}

fn state_path() -> Option<PathBuf> {
    Some(xdg::state_home()?.join(PROGRAM).join("state"))
}

/// The lines of `text` that say something, numbered from one, each as its
/// name and value with the space around them taken off and the quotes
/// around a whole value taken off too, or `None` for a line with no `=` in
/// it. Blank lines and comments are left out. How the state file and a
/// command's result file are read, and the configuration file was.
pub(crate) fn lines(text: &str) -> impl Iterator<Item = (usize, Option<(&str, &str)>)> {
    text.lines().enumerate().filter_map(|(index, line)| {
        let line = uncommented(line).0.trim();
        if line.is_empty() {
            return None;
        }
        Some((index + 1, pair(line)))
    })
}

/// `line` cut where its comment starts: what comes before, and the comment
/// with the space before it.
///
/// A comment starts at a `#` that begins the line or follows a space: a
/// web address holds `#` inside itself, as the default map link does, and
/// is not cut short there.
fn uncommented(line: &str) -> (&str, &str) {
    let comment = line.char_indices().find(|&(at, c)| {
        c == '#'
            && line[..at]
                .chars()
                .next_back()
                .is_none_or(char::is_whitespace)
    });
    let cut = comment.map_or(line.len(), |(at, _)| {
        at - (line[..at].len() - line[..at].trim_end().len())
    });
    line.split_at(cut)
}

/// A line with no comment as its name and value, or `None` where it has no
/// `=`.
fn pair(line: &str) -> Option<(&str, &str)> {
    line.split_once('=').map(|(name, value)| {
        let value = value.trim();
        // A pair around the whole value, and only that: a command's
        // line of the shell may begin and end with quotes of its own,
        // `"$HOME/bin/x" "$GAMUT_FILE"`, which are not a pair.
        let value = value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .filter(|inside| !inside.contains('"'))
            .unwrap_or(value);
        (name.trim(), value)
    })
}

/// A word of the state file that is on or off.
fn on(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
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

/// `contents` written whole to `path`, where nothing is there yet: by way of
/// a temporary name beside it, moved into place by a rename that refuses
/// rather than replace a file that arrived in the meantime — a second
/// window rewriting the same old file, say.
fn write_new(path: &Path, contents: &str) -> Result<()> {
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let written = fs::File::create(&temporary)
        .and_then(|mut file| file.write_all(contents.as_bytes()))
        .with_context(|| format!("writing {}", shown_path(&temporary)))
        .and_then(|()| {
            crate::no_replace::rename_no_replace(&temporary, path)
                .with_context(|| format!("writing {}", shown_path(path)))
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
             log_counts = true\n\
             keys.interface.log-counts = \"y\"\n",
        );
        // `log_counts` and its key are retired, and passed over rather than
        // called mistakes.
        assert_eq!(problems, Vec::new());
        assert_eq!(
            config,
            Config {
                show_ui: false,
                show_minimap: false,
                show_histogram: true,
                ..Config::default()
            }
        );
    }

    #[test]
    fn a_line_the_configuration_cannot_use_is_skipped_and_said() {
        let (config, problems) = Config::parse(
            "show_info = \"yes\"\n\
             show_ui = 3\n\
             show_grid = true\n\
             show_filmstrip\n\
             show_histogram = true\n\
             max_files = 0\n\
             exiftool = false\n",
        );
        assert_eq!(
            config,
            Config {
                show_histogram: true,
                ..Config::default()
            }
        );
        let lines: Vec<usize> = problems.iter().map(|(line, _)| *line).collect();
        assert_eq!(lines, [1, 2, 3, 4, 6, 7], "{problems:?}");
        assert_eq!(problems[0].1, "show_info is true or false, not \"yes\"");
    }

    /// The window is told the first line the configuration could not use,
    /// and how many more there were.
    #[test]
    fn the_window_is_told_the_first_problem_and_how_many_more() {
        let (_, problems) = Config::parse(
            "show_info = true\n\
             keys.zoom.fit = \"space Double-click\"\n\
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
            // An indented line is an example, not a setting at its default:
            // the commands' block.
            .filter(|line| !line.starts_with(char::is_whitespace))
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
        assert_eq!(keys, 107, "{uncommented}");
        // A Mac's three more: the scroll, the scroll with Command, and the
        // pinch.
        let slots = if cfg!(target_os = "macos") { 13 } else { 10 };
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
            "keys.files.undo = \"ctrl+e\"\n\
             keys.region.move.left = [\"h\"]\n\
             keys.interface.dismiss = \"\"\n\
             keys.nope = \"a\"\n\
             keys.files.rename = \"f2 shift+.\"\n\
             keys.interface.grid = \"ctrl+e\"\n\
             keys.interface.toggle = 3\n",
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
        assert_eq!(lines, [4, 5, 6, 7], "{problems:?}");
        assert_eq!(
            problems[2].1,
            "`ctrl+e` was files.undo on line 1; interface.grid takes it"
        );
    }

    /// A command's key may be bound before the file defines the command, a
    /// click may run one, and a name no command has is still refused.
    #[test]
    fn a_keys_line_binds_a_command_wherever_it_sits() {
        let (config, problems) = Config::parse(
            "keys.command.redact = \"ctrl+shift+r\"\n\
             gesture.image.middle.click = \"command.redact\"\n\
             keys.command.nope = \"a\"\n\
             \n\
             [command.redact]\n\
             label = \"Redact\"\n\
             run = \"~/bin/redact.sh\"\n",
        );
        let lines: Vec<usize> = problems.iter().map(|(line, _)| *line).collect();
        assert_eq!(lines, [3], "{problems:?}");
        // Its own, which replaces the built-in of that name, and then the
        // other built-ins.
        assert_eq!(config.commands[0].run, "~/bin/redact.sh");
        assert!(config.commands[1..].iter().all(|c| c.family() != "redact"));
        assert_eq!(
            config.keys.action_named("command.redact"),
            Some(Action::Command(0))
        );
        assert_eq!(
            config.keys.spelled("command.redact"),
            spelled_here("Ctrl+Shift+R")
        );
    }

    /// A `gesture.` line puts one behavior in one slot; a word its kind does
    /// not take, a surface there is none of and a click naming no key are
    /// problems.
    #[test]
    fn a_gesture_line_sets_its_slot() {
        use crate::gestures::{Button, HoldAction, Kind, Mods, Surface};
        let (config, problems) = Config::parse(
            "gesture.image.middle.hold = \"loupe\"\n\
             gesture.image.wheel = \"none\"\n\
             gesture.image.left.drag = \"loupe\"\n\
             gesture.desk.left.drag = \"pan\"\n\
             gesture.image.\"ctrl+middle\".click = \"files.nope\"\n\
             gesture.image.middle.click = \"interface.grid\"\n",
        );
        assert_eq!(
            config
                .gestures
                .hold(Surface::Image, Mods::empty(), Button::Middle),
            Some(HoldAction::Loupe)
        );
        assert_eq!(
            config
                .gestures
                .wheel(Surface::Image, Mods::empty(), None, false),
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
            browse_folder: !defaults.browse_folder,
            max_files: 50,
            open_map_link: "https://www.openstreetmap.org/?mlat={lat}&mlon={lng}".to_string(),
            open_map_bounds_link: "https://example.com/?bbox={lng_ll},{lat_ll},{lng_ur},{lat_ur}"
                .to_string(),
            exiftool: "/opt/exiftool/exiftool".to_string(),
            keys: defaults.keys.clone(),
            gestures: defaults.gestures.clone(),
            commands: defaults.commands.clone(),
            programs: defaults.programs.clone(),
        };
        let text: String = SETTINGS
            .iter()
            .map(|setting| format!("{} = {}\n", setting.name, (setting.get)(&changed).written()))
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
        let (config, problems) =
            Config::parse("open_map_link = \"https://example.com/?q={lat}\"\n");
        assert_eq!(config.open_map_link, OPEN_MAP_LINK);
        assert_eq!(problems.len(), 1, "{problems:?}");
        // The default's `#` is part of the address, not a comment; one
        // after it is.
        let (config, problems) = Config::parse(&format!(
            "open_map_link = \"{OPEN_MAP_LINK}\"  # the default\n"
        ));
        assert_eq!(config.open_map_link, OPEN_MAP_LINK);
        assert!(problems.is_empty(), "{problems:?}");
    }

    /// The box's address takes the picture's corners where it says; one
    /// with no latitude or no longitude in it is a problem, and the default
    /// stands.
    #[test]
    fn the_map_bounds_link_takes_the_corners() {
        let corners = Corners {
            upper_left: [47.809, 5.95],
            upper_right: [47.809, 6.05],
            lower_right: [47.609, 6.05],
            lower_left: [47.609, 5.95],
        };
        assert_eq!(
            map_bounds_link(OPEN_MAP_BOUNDS_LINK, corners),
            "https://geojson.io/#data=data:application/json,%7B%22type%22%3A%22Feature%22%2C%22properties%22%3A%7B%7D%2C%22geometry%22%3A%7B%22type%22%3A%22Polygon%22%2C%22coordinates%22%3A%5B%5B%5B5.950000%2C47.609000%5D%2C%5B6.050000%2C47.609000%5D%2C%5B6.050000%2C47.809000%5D%2C%5B5.950000%2C47.809000%5D%2C%5B5.950000%2C47.609000%5D%5D%5D%7D%7D"
        );
        // Two corners are enough to name a box by.
        let bbox = "https://example.com/?bbox={lng_ll},{lat_ll},{lng_ur},{lat_ur}";
        let (config, problems) = Config::parse(&format!("open_map_bounds_link = \"{bbox}\"\n"));
        assert_eq!(config.open_map_bounds_link, bbox);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(
            map_bounds_link(bbox, corners),
            "https://example.com/?bbox=5.950000,47.609000,6.050000,47.809000"
        );
        let (config, problems) =
            Config::parse("open_map_bounds_link = \"https://example.com/?{lat_ul},{lat_lr}\"\n");
        assert_eq!(config.open_map_bounds_link, OPEN_MAP_BOUNDS_LINK);
        assert_eq!(problems.len(), 1, "{problems:?}");
        let (config, problems) = Config::parse(&format!(
            "open_map_bounds_link = \"{OPEN_MAP_BOUNDS_LINK}\"  # the default\n"
        ));
        assert_eq!(config.open_map_bounds_link, OPEN_MAP_BOUNDS_LINK);
        assert!(problems.is_empty(), "{problems:?}");
    }

    /// exiftool is found by the name or path given, and a setting naming
    /// nothing is a problem, the default standing.
    /// Every command the settings page shows reads as the page says, so
    /// that what a reader copies from it works.
    #[test]
    fn the_documented_commands_read() {
        let page = include_str!("../../user-docs/SETTINGS.md");
        let blocks: Vec<&str> = page
            .split("```")
            .skip(1)
            .step_by(2)
            .filter(|block| block.contains("[command."))
            .collect();
        assert!(blocks.len() >= 10, "{}", blocks.len());
        for block in blocks {
            let (config, problems) = Config::parse(block);
            assert_eq!(problems, Vec::new(), "{block}");
            let builtins = &commands::builtins().commands;
            let own = config
                .commands
                .iter()
                .take_while(|c| !builtins.contains(c))
                .filter(|c| c.base.is_none());
            assert_eq!(own.count(), 1, "{block}");
        }
    }

    #[test]
    fn exiftool_needs_a_name() {
        let (config, problems) = Config::parse("exiftool = \"\"\n");
        assert_eq!(config.exiftool, "exiftool");
        assert_eq!(problems.len(), 1, "{problems:?}");
        let (config, problems) = Config::parse("exiftool = \"/usr/bin/vendor_perl/exiftool\"\n");
        assert_eq!(config.exiftool, "/usr/bin/vendor_perl/exiftool");
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn the_state_reads_back_what_it_writes() {
        let state = State {
            filmstrip_width: 212.0,
            loupe_magnification: 8.0,
            horizon_step: 15.0,
            ui_scale: 1.5,
            order: filmstrip::Order {
                sort: filmstrip::Sort::Modified,
                direction: filmstrip::Direction::Descending,
            },
            camera_jpeg: true,
            profile: false,
            include_subfolders: true,
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
            "pixel_format = \"decimal\"\n\
             coordinate_format = \"geographic\"\n\
             geographic_format = 3\n\
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
             horizon_step = 7\n\
             ui_scale = 9\n\
             sort = shoe size\n\
             sort_direction = sideways\n\
             camera_jpeg = maybe\n\
             include_subfolders = perhaps\n\
             profile = sometimes\n\
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
