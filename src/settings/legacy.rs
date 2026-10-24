//! The configuration file as it was before it was TOML — `config`, lines of
//! `name = value` with `#` starting a comment, and a command's input in a
//! grammar of its own — rewritten as TOML, once, as `config.toml`.
//!
//! Line by line, so that what the user wrote survives where they wrote it:
//! a comment and a blank line are kept as they are, and a setting is written
//! as TOML writes it, in the same place — a setting commented out too, so
//! that taking its `#` off still works. Only the commands move, to the foot
//! of the file, since a TOML table takes every line after it. The passages
//! of the template whose words the new format made wrong are put back as
//! the template says them now, where the file holds them as they were
//! printed. A line that did not read before is kept as it was, and does not
//! read now either, which is said as it was.

use std::path::Path;

use super::{Config, Plain, RETIRED, SETTINGS, on, pair, uncommented};
use crate::PROGRAM;
use crate::commands;
use crate::settings::document::{key, string};
use crate::ui::commands::{Kind, decimal};

/// A file in the old format, rewritten.
pub(super) struct Rewritten {
    pub text: String,
    /// The old file's line each line of the new one came from, from one.
    origins: Vec<usize>,
}

impl Rewritten {
    /// The old file's line that the new one's `line` came from.
    pub fn origin(&self, line: usize) -> usize {
        line.checked_sub(1)
            .and_then(|index| self.origins.get(index))
            .copied()
            .unwrap_or(1)
    }
}

/// The new file being written: its lines, each with the old line it came
/// from.
#[derive(Default)]
struct Out {
    lines: Vec<(String, usize)>,
}

impl Out {
    fn push(&mut self, line: impl Into<String>, from: usize) {
        self.lines.push((line.into(), from));
    }

    /// Every line of `block`, each said to come from `from`.
    fn block(&mut self, block: &str, from: usize) {
        for line in block.lines() {
            self.push(line, from);
        }
    }
}

/// One command's lines, in the order the file had them.
struct Pending<'a> {
    name: &'a str,
    first: usize,
    /// Each line's field — what follows the name — its value, its number
    /// and the comment after it.
    fields: Vec<(&'a str, &'a str, usize, &'a str)>,
}

/// `old`, read from `path`, as TOML.
pub(super) fn rewrite(old: &str, path: &Path) -> Rewritten {
    let mut out = Out::default();
    out.push(
        format!(
            "# Rewritten by {PROGRAM} from {}, which it no longer reads.",
            crate::shown_path(path)
        ),
        1,
    );
    out.push("", 1);
    let lines: Vec<&str> = old.lines().collect();
    let passages = passages();
    // Where a name is set twice the later line wins, as it did; TOML takes
    // a name once, so the earlier is left commented out.
    let mut last: Vec<(&str, usize)> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if let Some((name, _)) = pair(uncommented(line).0.trim())
            && !name.starts_with("command.")
        {
            last.retain(|(each, _)| *each != name);
            last.push((name, index));
        }
    }
    let mut commands: Vec<Pending> = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let number = index + 1;
        if let Some((was, now)) = passages
            .iter()
            .find(|(was, _)| lines[index..].starts_with(&was.lines().collect::<Vec<_>>()))
        {
            out.block(now, number);
            index += was.lines().count();
            continue;
        }
        let line = lines[index];
        index += 1;
        let (code, comment) = uncommented(line);
        let code = code.trim();
        if code.is_empty() {
            out.push(commented(line).unwrap_or_else(|| line.to_string()), number);
            continue;
        }
        let Some((name, value)) = pair(code) else {
            out.push(line, number);
            continue;
        };
        if let Some(rest) = name.strip_prefix("command.")
            && let Some((command, field)) = rest.split_once('.')
        {
            match commands.iter_mut().find(|each| each.name == command) {
                Some(pending) => pending.fields.push((field, value, number, comment)),
                None => commands.push(Pending {
                    name: command,
                    first: number,
                    fields: vec![(field, value, number, comment)],
                }),
            }
            continue;
        }
        let written = assignment(name, value);
        match last.contains(&(name, number - 1)) {
            true => out.push(format!("{written}{comment}"), number),
            false => out.push(format!("# {written}{comment}"), number),
        }
    }
    for pending in &commands {
        command(&mut out, pending);
    }
    let mut text = String::new();
    let mut origins = Vec::new();
    for (line, from) in out.lines {
        text.push_str(&line);
        text.push('\n');
        origins.push(from);
    }
    Rewritten { text, origins }
}

/// `line`, a comment, as a setting commented out where that is what it is,
/// and otherwise `None`: a comment that is words is left as it is.
fn commented(line: &str) -> Option<String> {
    let inside = line.trim_start().strip_prefix('#')?;
    let inside = inside.strip_prefix(' ').unwrap_or(inside);
    // Indented, it is an example, as the old template wrote its commands.
    if inside.starts_with(char::is_whitespace) {
        return None;
    }
    let (code, comment) = uncommented(inside);
    let (name, value) = pair(code.trim())?;
    let known = SETTINGS.iter().any(|setting| setting.name == name)
        || RETIRED.contains(&name)
        || name.starts_with("keys.")
        || name.starts_with("gesture.");
    known.then(|| format!("# {}{comment}", assignment(name, value)))
}

/// Key names that held another name inside their own — `zoom.100` and
/// `zoom.100.toggle` — which TOML cannot hold side by side, since the one is
/// then both a value and a table of more; and what each is called now.
const RENAMED: [(&str, &str); 2] = [
    ("zoom.100.toggle", "zoom.toggle"),
    ("files.map.bounds", "files.map-bounds"),
];

/// A key's name as it is called now.
fn renamed(name: &str) -> &str {
    RENAMED
        .iter()
        .find(|(was, _)| *was == name)
        .map_or(name, |(_, now)| now)
}

/// `name = value` as TOML: a setting that is a flag or a number bare, where
/// the old value was one, and everything else in quotes. A value the
/// setting could not take is quoted as it was, and refused as it was. A
/// name that is no setting now — retired, or never one — is written bare
/// where its value reads as a flag or a whole number, as the setting it
/// was most likely was. A key renamed since is written as it is called
/// now, as a binding and as a click's.
fn assignment(name: &str, value: &str) -> String {
    let (name, value) = match (name.strip_prefix("keys."), name.starts_with("gesture.")) {
        (Some(key), _) => (format!("keys.{}", renamed(key)), value),
        (None, true) => (name.to_string(), renamed(value)),
        (None, false) => (name.to_string(), value),
    };
    let name = name.as_str();
    let shape = SETTINGS
        .iter()
        .find(|setting| setting.name == name)
        .map(|setting| (setting.get)(&Config::default()));
    let flag = || on(value).map(|on| on.to_string());
    let whole = || value.parse::<usize>().ok().map(|n| n.to_string());
    let written = match shape {
        Some(Plain::Flag(_)) => flag(),
        Some(Plain::Whole(_)) => whole(),
        Some(Plain::Text(_)) => None,
        None if name.starts_with("keys.") || name.starts_with("gesture.") => None,
        None => flag().or_else(whole),
    };
    let written = written.unwrap_or_else(|| string(value));
    format!("{} = {written}", key(name))
}

/// One command as a table, and an array of tables for its inputs, at the
/// foot of the file.
fn command(out: &mut Out, pending: &Pending) {
    let name = key(pending.name);
    out.push("", pending.first);
    out.push(format!("[command.{name}]"), pending.first);
    // Each field once, from its last line, in the order first written.
    let mut inputs: Vec<&str> = Vec::new();
    let mut fields: Vec<&str> = Vec::new();
    for &(field, ..) in &pending.fields {
        match field.strip_prefix("input.") {
            Some(input) => {
                let input = input.split_once('.').map_or(input, |(input, _)| input);
                if !inputs.contains(&input) {
                    inputs.push(input);
                }
            }
            None if !fields.contains(&field) => fields.push(field),
            None => {}
        }
    }
    let last = |field: &str| {
        pending
            .fields
            .iter()
            .rev()
            .find(|(each, ..)| *each == field)
            .copied()
    };
    for field in fields {
        let Some((_, value, number, comment)) = last(field) else {
            continue;
        };
        let written = match field {
            "confirm" | "shown" => on(value).map(|on| on.to_string()),
            _ => None,
        }
        .unwrap_or_else(|| string(value));
        out.push(format!("{} = {written}{comment}", key(field)), number);
    }
    for input in inputs {
        let mut parts: Vec<&str> = Vec::new();
        for &(field, ..) in &pending.fields {
            if let Some(rest) = field.strip_prefix("input.")
                && let Some(part) = match rest.split_once('.') {
                    Some((each, part)) if each == input => Some(part),
                    None if rest == input => Some(""),
                    _ => None,
                }
                && !parts.contains(&part)
            {
                parts.push(part);
            }
        }
        let from = pending
            .fields
            .iter()
            .find(|(field, ..)| {
                field
                    .strip_prefix("input.")
                    .is_some_and(|rest| rest == input || rest.starts_with(&format!("{input}.")))
            })
            .map_or(pending.first, |&(_, _, number, _)| number);
        out.push("", from);
        out.push(format!("[[command.{name}.input]]"), from);
        out.push(format!("name = {}", string(input)), from);
        for part in parts {
            let field = match part {
                "" => format!("input.{input}"),
                part => format!("input.{input}.{part}"),
            };
            let Some((_, value, number, comment)) = last(&field) else {
                continue;
            };
            match part {
                "" => match read_kind(value) {
                    Ok(kind) => {
                        for line in kind_fields(&kind) {
                            out.push(line, number);
                        }
                    }
                    Err(problem) => out.push(
                        format!("# Not rewritten, since {problem}: {field} = {value}"),
                        number,
                    ),
                },
                part => out.push(
                    format!("{} = {}{comment}", key(part), string(value)),
                    number,
                ),
            }
        }
    }
}

/// A kind as the lines of its table.
fn kind_fields(kind: &Kind) -> Vec<String> {
    let number = |name: &str, value: f64| format!("{name} = {}", decimal(value));
    match kind {
        Kind::Text { default, .. } if default.is_empty() => vec!["kind = \"string\"".into()],
        Kind::Text { default, .. } => vec![
            "kind = \"string\"".into(),
            format!("default = {}", string(default)),
        ],
        Kind::Number { range, default } => {
            let mut lines = vec!["kind = \"number\"".to_string()];
            if let Some((low, high)) = range {
                lines.push(number("min", *low));
                lines.push(number("max", *high));
            }
            lines.push(number("default", *default));
            lines
        }
        Kind::Checkbox { default } => {
            vec!["kind = \"checkbox\"".into(), format!("default = {default}")]
        }
        Kind::Radio { choices, default } => vec![
            "kind = \"radio\"".into(),
            format!(
                "choices = [{}]",
                choices
                    .iter()
                    .map(|choice| string(choice))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            format!("default = {}", string(&choices[*default])),
        ],
        Kind::Slider {
            low,
            high,
            default,
            step,
        } => vec![
            "kind = \"slider\"".into(),
            number("min", *low),
            number("max", *high),
            number("default", *default),
            number("step", *step),
        ],
    }
}

/// An input as the old file wrote it: its kind's word, and what that kind
/// takes after it, whitespace between — `slider 1 50 10`, `radio png jpg =
/// jpg`.
fn read_kind(value: &str) -> Result<Kind, String> {
    let value = value.trim();
    let (word, rest) = value
        .split_once(char::is_whitespace)
        .map_or((value, ""), |(word, rest)| (word, rest.trim()));
    let numbers = || {
        rest.split_whitespace()
            .map(|token| {
                token
                    .parse::<f64>()
                    .ok()
                    .filter(|number| number.is_finite())
                    .ok_or_else(|| format!("`{token}` is not a number"))
            })
            .collect::<Result<Vec<f64>, String>>()
    };
    match word {
        "string" => Ok(Kind::Text {
            default: rest.to_string(),
            lines: 1,
        }),
        "number" => match numbers()?[..] {
            [] => Kind::number(None, None),
            [default] => Kind::number(None, Some(default)),
            [low, high] => Kind::number(Some((low, high)), None),
            [low, high, default] => Kind::number(Some((low, high)), Some(default)),
            _ => Err("a number takes at most a least, a most and a default".into()),
        },
        "checkbox" => match rest {
            "" | "false" => Ok(Kind::Checkbox { default: false }),
            "true" => Ok(Kind::Checkbox { default: true }),
            _ => Err(format!("a checkbox is true or false, not `{rest}`")),
        },
        "radio" => {
            let (choices, default) = match rest.split_once('=') {
                Some((choices, default)) => (choices, Some(default.trim())),
                None => (rest, None),
            };
            let choices = choices.split_whitespace().map(str::to_string).collect();
            Kind::radio(choices, default)
        }
        "slider" => match numbers()?[..] {
            [low, high] => Kind::slider(low, high, None, None),
            [low, high, default] => Kind::slider(low, high, Some(default), None),
            [low, high, default, step] => Kind::slider(low, high, Some(default), Some(step)),
            _ => Err("a slider takes a least and a most, then a default and a step".into()),
        },
        "" => Err("an input needs a kind".into()),
        other => Err(format!("`{other}` is not a kind of input")),
    }
}

/// Each passage of the old template whose words are wrong now, as it was
/// printed, and the template's words for it now.
fn passages() -> Vec<(String, String)> {
    let header = Config::header();
    vec![
        (
            format!(
                "# {PROGRAM}'s configuration: how the window opens, and what the keys and\n\
                 # the mouse do. Each setting is shown at its default, commented out;\n\
                 # take the # off a line to change it. --histogram, --info,\n\
                 # --no-minimap and --alone win over what is set here.\n"
            ),
            header.clone(),
        ),
        (
            format!(
                "# {PROGRAM}'s configuration: how the window opens, and what the keys and\n\
                 # the mouse do. Each setting is shown at its default, commented out;\n\
                 # take the # off a line to change it. --histogram, --info and\n\
                 # --no-minimap win over what is set here.\n"
            ),
            header,
        ),
        (
            format!(
                "# Keys: each keys.<name> takes its chords, whitespace between them; with\n\
                 # none the name is unbound. A chord is modifiers joined by + and then a key:\n\
                 {}\
                 # (> rather than shift+., L for Shift+L), a digit of the number row by its\n\
                 # place (shift+2), or space, enter, esc, tab, backspace, delete, insert,\n\
                 # home, end, pageup, pagedown, left, right, up, down, f1 to f12. # starts\n\
                 # a comment, so the key that types it is written by its place: shift+3.\n",
                crate::app::keymap::MODIFIER_LINE
            ),
            crate::app::keymap::chords_prose(),
        ),
        (
            OLD_COMMANDS.to_string(),
            commands::TEMPLATE.trim_start_matches('\n').to_string(),
        ),
    ]
}

/// The commands' passage of the old template, as it was printed.
const OLD_COMMANDS: &str = "\
# Commands: a line of the shell, run with the file on screen and what is
# marked on it in its environment, from the foot of the file menu or by a
# key bound to keys.command.<name>. Every field but run may be left out;
# needs is file, region, pointer, location, georeference or none. A
# script asks the window to show an image it made, or to say something,
# with open, message and error lines, written as this file's are, in the
# file $GAMUT_RESULT names. What it prints is shown, never obeyed.
#   command.redact.label = Redact region
#   command.redact.run = ~/bin/redact.sh
#   command.redact.needs = region
#   command.redact.confirm = false
#   command.redact.shown = false
#   command.redact.input.blur = slider 1 50 10
#   command.redact.input.blur.label = Blur radius
#   command.redact.input.note = string Redacted
#   command.redact.input.count = number 1 10 3
#   command.redact.input.solid = checkbox true
#   command.redact.input.format = radio png jpg webp = jpg
#   keys.command.redact = ctrl+shift+r
";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::input::Action;
    use crate::gestures::{Button, Kind as Click, Mods, Surface, spelled_here};

    fn rewritten(old: &str) -> Rewritten {
        rewrite(old, Path::new("/home/someone/.config/gamut/config"))
    }

    /// What a file in the old format said survives the rewrite: each
    /// setting where it was, as TOML writes it, a comment kept, a command
    /// moved to the foot as a table, a key renamed since under its new name,
    /// and the later of two lines setting one thing winning, as it did.
    #[test]
    fn an_old_file_says_the_same_as_toml() {
        let old = "# the window as I like it\n\
                   show_ui = false   # less clutter\n\
                   # show_info = false\n\
                   max_files = 500\n\
                   max_files = 600\n\
                   pixel_format = decimal\n\
                   log_counts = true\n\
                   open_map_link = https://example.com/?q={lat},{lng}#map\n\
                   keys.zoom.100.toggle = t\n\
                   keys.interface.dismiss =\n\
                   keys.command.redact = ctrl+shift+r\n\
                   gesture.image.shift+middle.click = zoom.100.toggle\n\
                   command.redact.label = Redact region\n\
                   command.redact.run = \"$HOME/bin/redact\" \"$GAMUT_FILE\"\n\
                   command.redact.needs = region\n\
                   command.redact.input.style = radio black blur = blur\n\
                   command.redact.input.style.label = Style\n\
                   command.redact.input.size = slider 1 50 10\n\
                   command.redact.confirm = true\n\
                   command.other.run = true\n\
                   command.other.input.on = checkbox true\n\
                   command.other.input.note = string a note\n\
                   command.other.input.count = number 1 9\n";
        let new = rewritten(old);
        let (config, problems) = Config::parse(&new.text);
        assert_eq!(problems, Vec::new(), "{}", new.text);
        assert!(!config.show_ui);
        assert_eq!(config.max_files, 600);
        assert_eq!(
            config.open_map_link,
            "https://example.com/?q={lat},{lng}#map"
        );
        assert_eq!(config.keys.spelled("zoom.toggle"), "t");
        assert_eq!(config.keys.spelled("interface.dismiss"), "");
        assert_eq!(
            config.keys.spelled("command.redact"),
            spelled_here("Ctrl+Shift+R")
        );
        assert_eq!(
            config
                .gestures
                .click(Surface::Image, Mods::SHIFT, Button::Middle, Click::Click),
            Some("zoom.toggle")
        );
        assert_eq!(
            config.keys.action_named("command.other"),
            Some(Action::Command(1))
        );
        let redact = &config.commands[0];
        assert_eq!(redact.label, "Redact region");
        assert_eq!(redact.run, "\"$HOME/bin/redact\" \"$GAMUT_FILE\"");
        assert!(redact.confirm);
        let inputs: Vec<(&str, &str, Kind)> = redact
            .inputs
            .iter()
            .map(|input| {
                (
                    input.name.as_str(),
                    input.label.as_str(),
                    input.kind.clone(),
                )
            })
            .collect();
        assert_eq!(
            inputs,
            [
                (
                    "style",
                    "Style",
                    Kind::radio(vec!["black".into(), "blur".into()], Some("blur")).unwrap()
                ),
                (
                    "size",
                    "size",
                    Kind::slider(1.0, 50.0, Some(10.0), None).unwrap()
                ),
            ]
        );
        let other: Vec<Kind> = config.commands[1]
            .inputs
            .iter()
            .map(|input| input.kind.clone())
            .collect();
        assert_eq!(
            other,
            [
                Kind::Checkbox { default: true },
                Kind::Text {
                    default: "a note".into(),
                    lines: 1
                },
                Kind::number(Some((1.0, 9.0)), None).unwrap(),
            ]
        );
        // Where each thing was written.
        let lines: Vec<&str> = new.text.lines().collect();
        assert_eq!(lines[2], "# the window as I like it");
        assert_eq!(lines[3], "show_ui = false   # less clutter");
        assert_eq!(lines[4], "# show_info = false");
        assert_eq!(lines[5], "# max_files = 500");
        assert_eq!(lines[6], "max_files = 600");
        assert_eq!(lines[7], "pixel_format = \"decimal\"");
        assert_eq!(lines[8], "log_counts = true");
        assert_eq!(
            lines[13],
            "gesture.image.\"shift+middle\".click = \"zoom.toggle\""
        );
        assert_eq!(lines[15], "[command.redact]");
    }

    /// A line that did not read before does not now, and is said on the
    /// old file's line where the new file could not be written; a command
    /// input that does not read is left a comment saying why.
    #[test]
    fn what_did_not_read_still_does_not() {
        let old = "show_ui = false\n\
                   show_filmstrip\n\
                   show_info = maybe\n\
                   command.x.run = true\n\
                   command.x.input.size = slider 50 1\n";
        let new = rewritten(old);
        let (config, problems) = Config::parse(&new.text);
        assert!(!config.show_ui);
        let lines: Vec<usize> = problems.iter().map(|(line, _)| new.origin(*line)).collect();
        assert_eq!(lines, [2, 3, 5], "{problems:?}\n{}", new.text);
        assert!(
            new.text
                .contains("# Not rewritten, since 50 is not less than 1")
        );
    }

    /// The old template's passages that the new format made wrong are put
    /// back as the template has them now, and what is left reads as every
    /// default.
    #[test]
    fn the_old_templates_words_are_replaced() {
        let old = format!(
            "# {PROGRAM}'s configuration: how the window opens, and what the keys and\n\
             # the mouse do. Each setting is shown at its default, commented out;\n\
             # take the # off a line to change it. --histogram, --info,\n\
             # --no-minimap and --alone win over what is set here.\n\
             \n\
             # show_ui = true\n\
             \n\
             # Keys: each keys.<name> takes its chords, whitespace between them; with\n\
             # none the name is unbound. A chord is modifiers joined by + and then a key:\n\
             {}\
             # (> rather than shift+., L for Shift+L), a digit of the number row by its\n\
             # place (shift+2), or space, enter, esc, tab, backspace, delete, insert,\n\
             # home, end, pageup, pagedown, left, right, up, down, f1 to f12. # starts\n\
             # a comment, so the key that types it is written by its place: shift+3.\n\
             # A chord set here is taken from whichever other name held it. With a\n\
             # region selected the region's names are tried first, then the rest.\n\
             # keys.zoom.100 = 1 0\n\
             # keys.zoom.100.toggle =\n\
             # gesture.image.right+wheel = loupe-magnification\n\
             \n\
             {OLD_COMMANDS}",
            crate::app::keymap::MODIFIER_LINE
        );
        let new = rewritten(&old);
        assert!(new.text.contains(&Config::header()), "{}", new.text);
        assert!(new.text.contains(&crate::app::keymap::chords_prose()));
        assert!(
            new.text
                .contains(commands::TEMPLATE.trim_start_matches('\n'))
        );
        assert!(!new.text.contains("# starts\n"));
        assert!(new.text.contains("# keys.zoom.100 = \"1 0\"\n"));
        assert!(new.text.contains("# keys.zoom.toggle = \"\"\n"));
        assert!(
            new.text
                .contains("# gesture.image.\"right+wheel\" = \"loupe-magnification\"\n")
        );
        assert_eq!(Config::parse(&new.text), (Config::default(), Vec::new()));
        // And with every setting's # taken off, it still reads, as the
        // defaults.
        let uncommented: String = new
            .text
            .lines()
            .filter_map(|line| line.strip_prefix("# "))
            .filter(|line| line.contains(" = ") && !line.starts_with(char::is_whitespace))
            .map(|line| format!("{line}\n"))
            .collect();
        let (config, problems) = Config::parse(&uncommented);
        assert_eq!(problems, Vec::new(), "{uncommented}");
        assert_eq!(config, Config::default());
    }
}
