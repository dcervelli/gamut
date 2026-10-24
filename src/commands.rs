//! The commands the configuration file defines: what each one is, read from
//! its `[command.<name>]` table, and running one — a line of the shell, run
//! on a thread of its own with the window's state in its environment, its
//! output captured, and what it asks the window to do read back from a file
//! it was given.
//!
//! The values go in the environment rather than into the line: a file's
//! metadata is anyone's to write, and a camera name spliced into a shell
//! line would be a command. The asks come back through a file named by
//! `GAMUT_RESULT` rather than on the output, for the same reason the other
//! way: a script that prints what it read from a file — exiftool's tags, a
//! caption, text found in the picture — would otherwise be printing
//! whatever the file's author wanted the window to do. The output is only
//! ever shown. What the window does with each, and the run in flight, are
//! `app/commands.rs`'s.

use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::DirBuilderExt as _;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command as Process, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::settings::document::{self, Lines, Table, Value, shown};
use crate::ui::commands::{KINDS, Kind, Needs};
use crate::{PROGRAM, xdg};

/// One command, as the configuration file defines it.
#[derive(Clone, PartialEq, Debug)]
pub struct Command {
    /// What its table is named, `[command.<name>]`, and what its key is bound by:
    /// `keys.command.<name>`.
    pub name: String,
    /// What the menu, the help and the window's messages call it.
    pub label: String,
    /// The line of the shell it runs.
    pub run: String,
    pub needs: Needs,
    /// Whether it asks before it runs even with nothing to ask for.
    pub confirm: bool,
    /// Whether the picture as shown is written out for it to read.
    pub shown: bool,
    /// What becomes of what it writes to `GAMUT_OUTPUT`.
    pub output: Output,
    /// The name its new file is offered under, `{stem}` standing for the
    /// file's own: `name = "{stem}-small.jpg"`. Only for an output that
    /// makes a new file; where none is given, the dialog makes one up.
    pub new_name: Option<String>,
    /// What its dialog asks for, in the order the file lists them.
    pub inputs: Vec<Input>,
}

/// What becomes of what a command writes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Output {
    /// Nothing: the command writes wherever it likes, if anywhere, and
    /// says `open` for the window to show it.
    #[default]
    Own,
    /// `"replace"`: what it writes to `GAMUT_OUTPUT` takes the place of the
    /// file it ran on.
    Replace,
    /// `"new"`: what it writes to `GAMUT_OUTPUT` becomes a new file under
    /// the name its dialog asks for.
    New,
    /// `"choose"`: either, as its dialog is told.
    Choose,
}

impl Output {
    /// What the configuration file writes for each, `Own` being left out.
    const WORDS: [(&str, Output); 3] = [
        ("replace", Output::Replace),
        ("new", Output::New),
        ("choose", Output::Choose),
    ];

    /// Whether it can make a new file, whose name its dialog asks for.
    pub fn makes(self) -> bool {
        matches!(self, Output::New | Output::Choose)
    }

    /// Whether it can take the place of the file it ran on.
    pub fn replaces(self) -> bool {
        matches!(self, Output::Replace | Output::Choose)
    }
}

/// What a command's `name` stands for the file's own name without its
/// extension by.
pub const STEM: &str = "{stem}";

/// What is wrong with `name` as the name a command's new file is offered
/// under, if anything: a name of a file in a folder, holding no `{…}` but
/// [`STEM`].
fn name_problem(name: &str) -> Option<String> {
    if name.contains('/') || name.chars().any(char::is_control) {
        return Some("is a file's name, with no / and no control characters".into());
    }
    if name == "." || name == ".." {
        return Some(format!("{name} is not a file's name"));
    }
    let rest = name.replace(STEM, "");
    (rest.contains('{') || rest.contains('}'))
        .then(|| format!("holds a {{ or }} that is not {STEM}, the one name it can hold"))
}

impl Command {
    /// Whether pressing it opens its dialog rather than running it: it has
    /// something to ask, or a name for a new file to have typed.
    pub fn asks(&self) -> bool {
        self.confirm || !self.inputs.is_empty() || self.output.makes()
    }

    /// The name its key is bound by.
    pub fn key_name(&self) -> String {
        format!("command.{}", self.name)
    }
}

/// One input of a command's dialog.
#[derive(Clone, PartialEq, Debug)]
pub struct Input {
    /// Its `name`, and what the script reads it as, upper case:
    /// `GAMUT_INPUT_<NAME>`.
    pub name: String,
    pub label: String,
    pub kind: Kind,
}

/// Whether `name` is a command's name: lower-case letters, digits, `_` and
/// `-`, which TOML takes as a bare key.
fn command_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// Whether `name` is an input's name: as a command's, without the `-`,
/// which a variable of the shell cannot hold.
fn input_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// What a command's table takes.
const FIELDS: &str = "label, run, needs, confirm, shown, output, name or input";

/// A problem, and the line it is said on.
type Problem = (usize, String);

/// The commands the file's `command` table defines, in the order the file
/// writes them, and each thing in it that could not be used, by line. A
/// command with no `run` is left out, and so is an input that does not
/// read; the rest of each is taken.
pub fn parse(table: &Table, lines: &Lines) -> (Vec<Command>, Vec<Problem>) {
    let mut commands = Vec::new();
    let mut problems = Vec::new();
    for (key, value) in table.iter() {
        let name: &str = key.get_ref();
        let line = lines.of(key.span());
        if !command_name(name) {
            problems.push((
                line,
                format!("`{name}` is not a command's name: a-z, 0-9, _ and - only"),
            ));
            continue;
        }
        let Value::Table(fields) = value.get_ref() else {
            problems.push((
                line,
                format!(
                    "command.{name} is a table of its fields, [command.{name}], not {}",
                    shown(value.get_ref())
                ),
            ));
            continue;
        };
        if let Some(command) = read_command(name, line, fields, lines, &mut problems) {
            commands.push(command);
        }
    }
    problems.sort_by_key(|(line, _)| *line);
    (commands, problems)
}

/// One command, from its table; `line` is where it is first named, which a
/// missing `run` is said on.
fn read_command(
    name: &str,
    line: usize,
    fields: &Table,
    lines: &Lines,
    problems: &mut Vec<Problem>,
) -> Option<Command> {
    let mut command = Command {
        name: name.to_string(),
        label: name.to_string(),
        run: String::new(),
        needs: Needs::default(),
        confirm: false,
        shown: false,
        output: Output::Own,
        new_name: None,
        inputs: Vec::new(),
    };
    let mut run = None;
    let mut output_at = line;
    let mut name_at = line;
    for (key, value) in fields.iter() {
        let field: &str = key.get_ref();
        let at = lines.of(value.span());
        let value = value.get_ref();
        let words = |value: &Value| match document::text(value) {
            Some("") => Err(format!("command.{name}.{field} is empty")),
            Some(words) => Ok(words.to_string()),
            None => Err(format!(
                "command.{name}.{field} is a string, not {}",
                shown(value)
            )),
        };
        let flag = |value: &Value| {
            document::flag(value).ok_or_else(|| {
                format!(
                    "command.{name}.{field} is true or false, not {}",
                    shown(value)
                )
            })
        };
        let outcome = match field {
            "label" => words(value).map(|label| command.label = label),
            "run" => words(value).map(|line| run = Some(line)),
            "needs" => words(value).and_then(|word| {
                Needs::read(&word)
                    .map(|needs| command.needs = needs)
                    .map_err(|problem| format!("command.{name} {problem}"))
            }),
            "confirm" => flag(value).map(|on| command.confirm = on),
            "shown" => flag(value).map(|on| command.shown = on),
            "output" => words(value).and_then(|word| {
                output_at = at;
                match Output::WORDS.iter().find(|(each, _)| *each == word) {
                    Some((_, output)) => {
                        command.output = *output;
                        Ok(())
                    }
                    None => Err(format!(
                        "command.{name}.output is \"replace\", \"new\" or \"choose\", \
                         or left out, not \"{word}\""
                    )),
                }
            }),
            "name" => words(value).and_then(|given| {
                name_at = at;
                match name_problem(&given) {
                    Some(problem) => Err(format!("command.{name}.name {problem}")),
                    None => {
                        command.new_name = Some(given);
                        Ok(())
                    }
                }
            }),
            "input" => {
                command.inputs = read_inputs(name, at, value, lines, problems);
                Ok(())
            }
            // Every line after a `[command.…]` header is that command's,
            // so a setting written below one lands here.
            other if crate::settings::top_level(other) => Err(format!(
                "command.{name} has no field `{other}`: every line after \
                 [command.{name}] is the command's, so a setting, a key or a \
                 gesture goes above the first [command]"
            )),
            other => Err(format!(
                "unknown field `{other}` of command.{name}: {FIELDS}"
            )),
        };
        if let Err(problem) = outcome {
            problems.push((at, problem));
        }
    }
    let Some(run) = run else {
        problems.push((line, format!("command.{name} has no run")));
        return None;
    };
    // What it replaces is the file it ran on, so it needs one.
    if command.output.replaces() && command.needs == Needs::Nothing {
        let (instead, output) = match command.output {
            Output::Choose => ("it makes a new file instead", Output::New),
            _ => ("it runs without replacing", Output::Own),
        };
        problems.push((
            output_at,
            format!(
                "command.{name} replaces the file it runs on, so it cannot need none; \
                 {instead}"
            ),
        ));
        command.output = output;
    }
    if command.new_name.is_some() && !command.output.makes() {
        problems.push((
            name_at,
            format!(
                "command.{name}.name is the name of the new file that output \"new\" \
                 or \"choose\" makes"
            ),
        ));
        command.new_name = None;
    }
    if command.needs == Needs::Nothing
        && command
            .new_name
            .as_deref()
            .is_some_and(|given| given.contains(STEM))
    {
        problems.push((
            name_at,
            format!(
                "command.{name}.name holds {STEM}, the file's own name, \
                 and command.{name} needs none"
            ),
        ));
        command.new_name = None;
    }
    Some(Command { run, ..command })
}

/// A command's inputs: an array of tables, written `[[command.<name>.input]]`
/// or `input = [{ … }, …]`.
fn read_inputs(
    command: &str,
    line: usize,
    value: &Value,
    lines: &Lines,
    problems: &mut Vec<Problem>,
) -> Vec<Input> {
    let Value::Array(items) = value else {
        problems.push((
            line,
            format!(
                "command.{command}.input is a list of inputs, each \
                 [[command.{command}.input]], not {}",
                shown(value)
            ),
        ));
        return Vec::new();
    };
    let mut inputs: Vec<Input> = Vec::new();
    for item in items.iter() {
        let at = lines.of(item.span());
        let Value::Table(fields) = item.get_ref() else {
            problems.push((
                at,
                format!(
                    "an input of command.{command} is a table, not {}",
                    shown(item.get_ref())
                ),
            ));
            continue;
        };
        match read_input(command, at, fields, lines) {
            Ok(input) if inputs.iter().any(|each| each.name == input.name) => problems.push((
                at,
                format!("command.{command} has two inputs named `{}`", input.name),
            )),
            Ok(input) => inputs.push(input),
            Err(said) => problems.extend(said),
        }
    }
    inputs
}

/// One input, from its table at `line`: its name, its words and its kind,
/// and what that kind takes — or everything wrong with it.
fn read_input(
    command: &str,
    line: usize,
    fields: &Table,
    lines: &Lines,
) -> Result<Input, Vec<Problem>> {
    let field = |key: &str| {
        fields
            .get(key)
            .map(|value| (value.get_ref(), lines.of(value.span())))
    };
    let Some((named, at)) = field("name") else {
        return Err(vec![(
            line,
            format!("an input of command.{command} has no name"),
        )]);
    };
    let Some(name) = document::text(named).filter(|name| input_name(name)) else {
        return Err(vec![(
            at,
            format!(
                "{} is not an input's name: a-z, 0-9 and _ only",
                shown(named)
            ),
        )]);
    };
    let said = |problem: String| format!("command.{command} input `{name}`: {problem}");
    let mut problems = Vec::new();
    let label = match field("label") {
        None => name.to_string(),
        Some((value, at)) => match document::text(value) {
            Some(label) if !label.is_empty() => label.to_string(),
            _ => {
                problems.push((at, said(format!("label is words, not {}", shown(value)))));
                name.to_string()
            }
        },
    };
    let kinds = format!(
        "{} or {}",
        KINDS[..KINDS.len() - 1].join(", "),
        KINDS[KINDS.len() - 1]
    );
    let Some((kind, at)) = field("kind") else {
        problems.push((line, said(format!("no kind: {kinds}"))));
        return Err(problems);
    };
    let Some(word) = document::text(kind).filter(|word| KINDS.contains(word)) else {
        problems.push((
            at,
            said(format!("{} is not a kind of input: {kinds}", shown(kind))),
        ));
        return Err(problems);
    };
    let takes: &[&str] = match word {
        "number" => &["min", "max", "default"],
        "radio" => &["choices", "default"],
        "slider" => &["min", "max", "default", "step"],
        "string" => &["default", "lines"],
        _ => &["default"],
    };
    for (key, value) in fields.iter() {
        let key: &str = key.get_ref();
        if !["name", "label", "kind"].contains(&key) && !takes.contains(&key) {
            problems.push((
                lines.of(value.span()),
                said(format!("a {word} takes {}, not {key}", takes.join(", "))),
            ));
        }
    }
    match read_kind(word, &field, at) {
        Ok(kind) if problems.is_empty() => Ok(Input {
            name: name.to_string(),
            label,
            kind,
        }),
        Ok(_) => Err(problems),
        Err((at, problem)) => {
            problems.push((at, said(problem)));
            Err(problems)
        }
    }
}

/// The kind `word` names, from the fields `field` finds; `line` is the
/// kind's, which a problem with the whole is said on.
fn read_kind<'v>(
    word: &str,
    field: &dyn Fn(&str) -> Option<(&'v Value<'v>, usize)>,
    line: usize,
) -> Result<Kind, Problem> {
    let number = |key: &str| match field(key) {
        None => Ok(None),
        Some((value, at)) => document::number(value)
            .map(Some)
            .ok_or_else(|| (at, format!("{key} is a number, not {}", shown(value)))),
    };
    let words = |key: &str| match field(key) {
        None => Ok(None),
        Some((value, at)) => document::text(value)
            .map(Some)
            .ok_or_else(|| (at, format!("{key} is a string, not {}", shown(value)))),
    };
    let whole = |made: Result<Kind, String>| made.map_err(|problem| (line, problem));
    match word {
        "string" => whole(Kind::text(words("default")?, number("lines")?)),
        "number" => {
            let range = match (number("min")?, number("max")?) {
                (Some(low), Some(high)) => Some((low, high)),
                (None, None) => None,
                _ => return Err((line, "min and max go together".into())),
            };
            whole(Kind::number(range, number("default")?))
        }
        "checkbox" => match field("default") {
            None => Ok(Kind::Checkbox { default: false }),
            Some((value, at)) => document::flag(value)
                .map(|default| Kind::Checkbox { default })
                .ok_or_else(|| {
                    (
                        at,
                        format!("default is true or false, not {}", shown(value)),
                    )
                }),
        },
        "radio" => {
            let Some((value, at)) = field("choices") else {
                return Err((line, "a radio needs its choices".into()));
            };
            let choices = match value {
                Value::Array(items) => items
                    .iter()
                    .map(|item| document::text(item.get_ref()).map(str::to_string))
                    .collect::<Option<Vec<String>>>(),
                _ => None,
            }
            .ok_or_else(|| {
                (
                    at,
                    "choices is a list of strings: [\"png\", \"jpg\"]".into(),
                )
            })?;
            whole(Kind::radio(choices, words("default")?))
        }
        _ => {
            let (Some(low), Some(high)) = (number("min")?, number("max")?) else {
                return Err((line, "a slider needs a min and a max".into()));
            };
            whole(Kind::slider(low, high, number("default")?, number("step")?))
        }
    }
}

/// The commented example the template ends with: every field, and an input
/// of each kind. Indented after the `#`, so that it is read as an example
/// rather than as a setting left at its default.
pub const TEMPLATE: &str = "\n\
# Commands: a line of the shell, run with the file on screen and what is
# marked on it in its environment, from the foot of the file menu or by a
# key bound to keys.command.<name>. Each is a table, after every setting,
# key and gesture, since every line after a [table] belongs to it. Every
# field but run may be left out; needs is file, region, pointer, location,
# georeference or none. An input is a string, number, checkbox, radio or
# slider; a string with lines above 1 is a box of that many lines. A script asks the window to show an image it made, or to say
# something, with open, message and error lines in the file $GAMUT_RESULT
# names: the name, an equals sign, and the rest of the line, unquoted. What
# it prints is shown, never obeyed. With output set to \"replace\", it writes
# its version of the file to $GAMUT_OUTPUT, which then takes the file's
# place: the original goes to the trash, and undo brings it back. With
# \"new\", what it writes there becomes a new file, under the name its dialog
# asks for, which name offers, {stem} being the file's own; with \"choose\",
# the dialog asks which of the two.
#   [command.redact]
#   label = \"Redact region\"
#   run = \"~/bin/redact.sh\"
#   needs = \"region\"
#   confirm = false
#   shown = false
#   input = [
#     { name = \"blur\", label = \"Blur radius\", kind = \"slider\", min = 1, max = 50, default = 10 },
#     { name = \"note\", kind = \"string\", default = \"Redacted\", lines = 1 },
#     { name = \"count\", kind = \"number\", min = 1, max = 10, default = 3 },
#     { name = \"solid\", kind = \"checkbox\", default = true },
#     { name = \"format\", kind = \"radio\", choices = [\"png\", \"jpg\", \"webp\"], default = \"jpg\" },
#   ]
";
/// One thing the result file asks the window to do.
#[derive(Clone, PartialEq, Debug)]
pub enum Directive {
    /// Show this image, which the script made.
    Open(String),
    Message(String),
    Error(String),
}

/// What the result file `text` asks for, line by line, and each line that
/// does not read, by number. It is read as the state file is: lines of
/// `name = value`, `#` starting a comment, for a script to write with echo.
pub fn directives(text: &str) -> (Vec<Directive>, Vec<(usize, String)>) {
    let mut asked = Vec::new();
    let mut problems = Vec::new();
    for (number, line) in crate::settings::lines(text) {
        let Some((name, value)) = line else {
            problems.push((number, "expected `name = value`".to_string()));
            continue;
        };
        match name {
            "open" if value.is_empty() => problems.push((number, "open names no file".into())),
            "open" => asked.push(Directive::Open(value.to_string())),
            "message" => asked.push(Directive::Message(value.to_string())),
            "error" => asked.push(Directive::Error(value.to_string())),
            other => problems.push((number, format!("unknown `{other}`: open, message or error"))),
        }
    }
    (asked, problems)
}

/// How much of each of a run's streams is kept: the head, the rest read
/// and let go, so that a script that prints a great deal can neither hold
/// the window's memory nor stall on a full pipe.
pub const CAPTURE_CAP: usize = 1 << 20;

/// How many runs' directories are kept under the cache: their output,
/// their result file, for the Output tab's log after the fact.
pub const RUNS_KEPT: usize = 64;

/// A run asked for, everything it needs gathered on the loop so that the
/// thread reads nothing of the application.
pub struct Launch {
    /// Which run this is, for the answer to say.
    pub id: u64,
    pub label: String,
    /// The file it was run on, which its output is kept under.
    pub about: Option<PathBuf>,
    /// The line of the shell.
    pub line: String,
    /// Where it runs.
    pub cwd: PathBuf,
    /// The variables beyond the inherited environment.
    pub env: Vec<(String, String)>,
    /// The picture as shown, drawn and encoded as a PNG on the thread,
    /// where the command asks for it.
    pub shown: Option<Render>,
}

/// What draws the picture as shown and encodes it as a PNG, on the run's
/// thread.
pub type Render = Box<dyn FnOnce() -> Result<Vec<u8>, String> + Send>;

/// How a run ended.
#[derive(Clone, PartialEq, Debug)]
pub enum Status {
    /// The shell exited, with this code.
    Exited(i32),
    /// A signal ended it — the window's stop, or anything else's.
    Killed,
    /// It never ran, for this reason.
    Unstarted(String),
}

/// One of a run's streams, kept up to [`CAPTURE_CAP`].
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Captured {
    pub text: String,
    /// Whether more was said than was kept.
    pub cut: bool,
}

/// A run's answer, on its way to the loop.
#[derive(Clone, PartialEq, Debug)]
pub struct Finished {
    pub id: u64,
    pub label: String,
    pub about: Option<PathBuf>,
    /// Where its output and result file were left.
    pub dir: Option<PathBuf>,
    pub status: Status,
    pub stdout: Captured,
    pub stderr: Captured,
    /// What it wrote to the result file.
    pub result: String,
    pub took: Duration,
}

/// How an answer reaches the loop.
pub type Deliver = Arc<dyn Fn(Finished) + Send + Sync>;

/// A handle on a run in flight that can end it: the process group its
/// shell leads, while it runs.
#[derive(Clone, Default)]
pub struct Stopper(Arc<Mutex<Option<u32>>>);

impl Stopper {
    /// Ends the run, its shell and everything the shell started, with
    /// `SIGTERM` to the group. Nothing where the run is over or not begun.
    pub fn stop(&self) {
        let held = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(group) = *held
            && let Ok(group) = libc::pid_t::try_from(group)
        {
            // SAFETY: `kill` takes plain integers and touches no memory; a
            // group that has already gone answers ESRCH, which is ignored.
            unsafe {
                libc::kill(-group, libc::SIGTERM);
            }
        }
    }

    fn hold(&self, group: Option<u32>) {
        *self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = group;
    }
}

/// Where the runs' directories are kept: under the cache, and for the
/// tests under a directory of their own, so that they leave the user's
/// cache alone.
fn runs_dir() -> PathBuf {
    if cfg!(test) {
        return std::env::temp_dir().join(format!("{PROGRAM}-runs-{}", std::process::id()));
    }
    xdg::cache_home()
        .unwrap_or_else(std::env::temp_dir)
        .join(PROGRAM)
        .join("runs")
}

/// A directory for run `id` of this process, made fresh and readable by
/// its owner alone: the picture as shown is written there.
fn make_run_dir(id: u64) -> io::Result<PathBuf> {
    let runs = runs_dir();
    fs::create_dir_all(&runs)?;
    fresh_dir(&runs, &format!("{}-{id}", std::process::id()))
}

/// Where run `id` of a command whose output the window puts in place —
/// in the place of a file in `folder`, or as a new file there — has it
/// written: a directory of its own, made fresh in that folder — on its
/// filesystem, so that the output is put in place by a rename, and hidden,
/// so that nothing that lists the folder's images lists what is in it.
pub fn make_output_dir(folder: &Path, id: u64) -> io::Result<PathBuf> {
    fresh_dir(folder, &format!(".{PROGRAM}-{}-{id}", std::process::id()))
}

/// What the output is called in that directory: a name of the window's,
/// so that nothing typed reaches the command's arguments — a `%` or a
/// `[` is a pattern to ImageMagick — with `extension`, where there is one,
/// which a tool that writes many formats picks one by.
pub fn output_name(extension: Option<&str>) -> String {
    match extension {
        Some(extension) => format!("output.{extension}"),
        None => "output".to_string(),
    }
}

/// The name a command that makes a new file offers it under, before
/// anything is typed: its `name`, `{stem}` made the stem of `file`, the
/// file on screen; or the file's stem with the command's name after it and
/// its extension, and the command's name alone for one that needs nothing,
/// whose file is its own to say the format of. Counted up past whatever
/// `taken` says is in the folder already.
pub fn offered_name(
    command: &Command,
    file: Option<&Path>,
    taken: impl Fn(&str) -> bool,
) -> String {
    let part = |of: Option<&std::ffi::OsStr>| of.map(|part| part.to_string_lossy().into_owned());
    let stem = file.and_then(|file| part(file.file_stem()));
    let extension = file.and_then(|file| part(file.extension()));
    let made = match (&command.new_name, &stem) {
        (Some(name), _) => name.replace(STEM, stem.as_deref().unwrap_or_default()),
        (None, Some(stem)) if command.needs != Needs::Nothing => match &extension {
            Some(extension) => format!("{stem}-{}.{extension}", command.name),
            None => format!("{stem}-{}", command.name),
        },
        (None, _) => command.name.clone(),
    };
    if !taken(&made) {
        return made;
    }
    let made = Path::new(&made);
    let stem = part(made.file_stem()).unwrap_or_default();
    let extension = part(made.extension());
    (2..)
        .map(|count| match &extension {
            Some(extension) => format!("{stem}-{count}.{extension}"),
            None => format!("{stem}-{count}"),
        })
        .find(|name| !taken(name))
        .unwrap_or_default()
}

/// A directory under `parent` called `base`, or `base` with a number after
/// it where that is taken, made fresh and readable by its owner alone.
fn fresh_dir(parent: &Path, base: &str) -> io::Result<PathBuf> {
    let mut tries = 0;
    loop {
        let dir = match tries {
            0 => parent.join(base),
            n => parent.join(format!("{base}-{n}")),
        };
        match fs::DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => return Ok(dir),
            // Left by a run of an earlier process that had this one's id.
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists && tries < 100 => {
                tries += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

/// Takes away all but the newest [`RUNS_KEPT`] runs' directories.
fn prune_runs(runs: &Path) {
    let Ok(read) = fs::read_dir(runs) else {
        return;
    };
    let mut dirs: Vec<(std::time::SystemTime, PathBuf)> = read
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect();
    if dirs.len() <= RUNS_KEPT {
        return;
    }
    dirs.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    for (_, dir) in dirs.into_iter().skip(RUNS_KEPT) {
        let _ = fs::remove_dir_all(dir);
    }
}

/// Reads `stream` to its end, keeping the first [`CAPTURE_CAP`] bytes.
fn capture(mut stream: impl Read) -> Captured {
    let mut kept = Vec::new();
    let mut cut = false;
    let mut buffer = [0u8; 8192];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                let room = CAPTURE_CAP.saturating_sub(kept.len());
                kept.extend_from_slice(&buffer[..read.min(room)]);
                cut |= read > room;
            }
        }
    }
    Captured {
        text: String::from_utf8_lossy(&kept).into_owned(),
        cut,
    }
}

/// The directories a program is commonly installed in that a session
/// started from the Dock does not have on its `PATH`: Homebrew's, MacPorts'
/// and the user's own.
#[cfg(target_os = "macos")]
fn extra_path() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = ["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"]
        .into_iter()
        .map(PathBuf::from)
        .collect();
    dirs.extend(xdg::home().map(|home| home.join(".local/bin")));
    dirs
}

#[cfg(not(target_os = "macos"))]
fn extra_path() -> Vec<PathBuf> {
    Vec::new()
}

/// `PATH` with [`extra_path`]'s directories after it, each that is not on
/// it already; `None` where nothing is added.
fn widened_path() -> Option<std::ffi::OsString> {
    let current = std::env::var_os("PATH").unwrap_or_default();
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&current).collect();
    let before = dirs.len();
    for dir in extra_path() {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    (dirs.len() != before)
        .then(|| std::env::join_paths(dirs).ok())
        .flatten()
}

/// The status a run's shell ended with, as [`Status`] says it.
fn status_of(status: ExitStatus) -> Status {
    match status.code() {
        Some(code) => Status::Exited(code),
        None => Status::Killed,
    }
}

/// Runs `launch` and waits for it: its directory made, the picture as
/// shown written there where it is asked for, the shell started in its
/// own process group with the environment and the result file's name, its
/// two streams read to their ends, and the result file read back. What the
/// tests call; the window calls [`run_on_thread`].
pub fn run(launch: Launch, stopper: &Stopper) -> Finished {
    let started = Instant::now();
    let Launch {
        id,
        label,
        about,
        line,
        cwd,
        env,
        shown,
    } = launch;
    let unstarted = |dir: Option<PathBuf>, reason: String| Finished {
        id,
        label: label.clone(),
        about: about.clone(),
        dir,
        status: Status::Unstarted(reason),
        stdout: Captured::default(),
        stderr: Captured::default(),
        result: String::new(),
        took: started.elapsed(),
    };
    let dir = match make_run_dir(id) {
        Ok(dir) => dir,
        Err(error) => return unstarted(None, format!("making a directory to run in: {error}")),
    };
    prune_runs(&runs_dir());
    let result_path = dir.join("result");
    if let Err(error) = fs::write(&result_path, b"") {
        return unstarted(Some(dir), format!("making the result file: {error}"));
    }
    let shown_path = dir.join("shown.png");
    let mut env = env;
    if let Some(render) = shown {
        match render().and_then(|png| fs::write(&shown_path, png).map_err(|e| e.to_string())) {
            Ok(()) => env.push(("GAMUT_SHOWN".into(), shown_path.display().to_string())),
            Err(error) => {
                return unstarted(Some(dir), format!("writing the picture as shown: {error}"));
            }
        }
    }
    env.push(("GAMUT_RESULT".into(), result_path.display().to_string()));
    let mut process = Process::new("/bin/sh");
    process
        .arg("-c")
        .arg(&line)
        .arg(PROGRAM)
        .current_dir(&cwd)
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    if let Some(path) = widened_path() {
        process.env("PATH", path);
    }
    let mut child = match process.spawn() {
        Ok(child) => child,
        Err(error) => return unstarted(Some(dir), error.to_string()),
    };
    stopper.hold(Some(child.id()));
    let stdout = child
        .stdout
        .take()
        .map(|out| std::thread::spawn(move || capture(out)));
    let stderr = child
        .stderr
        .take()
        .map(|err| std::thread::spawn(move || capture(err)));
    let status = child.wait();
    stopper.hold(None);
    let joined = |handle: Option<std::thread::JoinHandle<Captured>>| {
        handle
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default()
    };
    let stdout = joined(stdout);
    let stderr = joined(stderr);
    let status = match status {
        Ok(status) => status_of(status),
        Err(error) => Status::Unstarted(error.to_string()),
    };
    let _ = fs::remove_file(&shown_path);
    let _ = fs::write(dir.join("stdout"), &stdout.text);
    let _ = fs::write(dir.join("stderr"), &stderr.text);
    let mut result = String::new();
    if let Ok(file) = fs::File::open(&result_path) {
        let _ = file.take(CAPTURE_CAP as u64).read_to_string(&mut result);
    }
    Finished {
        id,
        label,
        about,
        dir: Some(dir),
        status,
        stdout,
        stderr,
        result,
        took: started.elapsed(),
    }
}

/// Runs `launch` on a thread of its own and hands the answer to `deliver`;
/// `stopper` ends it early. A thread that does not start is answered all
/// the same, so that the window does not wait on a run that never began.
/// Nothing waits for it as the window closes: a long job outlives it.
pub fn run_on_thread(launch: Launch, stopper: Stopper, deliver: Deliver) {
    let (id, label, about) = (launch.id, launch.label.clone(), launch.about.clone());
    let answer = Arc::clone(&deliver);
    let spawned = std::thread::Builder::new()
        .name("command".into())
        .spawn(move || deliver(run(launch, &stopper)));
    if let Err(error) = spawned {
        eprintln!("{PROGRAM}: starting a command: {error}");
        answer(Finished {
            id,
            label,
            about,
            dir: None,
            status: Status::Unstarted(error.to_string()),
            stdout: Captured::default(),
            stderr: Captured::default(),
            result: String::new(),
            took: Duration::ZERO,
        });
    }
}

/// The commands a whole file defines, as the configuration reads them.
#[cfg(test)]
pub fn parsed(text: &str) -> (Vec<Command>, Vec<Problem>) {
    let (clean, mut problems) = document::readable(text);
    let root = document::table(&clean);
    let lines = Lines::new(text);
    let Some(Value::Table(table)) = root.get_ref().get("command").map(|v| v.get_ref()) else {
        return (Vec::new(), problems);
    };
    let (commands, said) = parse(table, &lines);
    problems.extend(said);
    problems.sort_by_key(|(line, _)| *line);
    (commands, problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Commands are listed in the order the file writes them, and their
    /// inputs in the order each lists them, in either of TOML's ways of
    /// writing an array of tables.
    #[test]
    fn commands_and_inputs_are_read_in_the_files_order() {
        let (commands, problems) = parsed(
            "[command.b]\n\
             run = \"echo b\"\n\
             confirm = true\n\
             shown = true\n\
             \n\
             [command.a]\n\
             run = \"~/bin/a.sh\"\n\
             label = \"Do A\"\n\
             needs = \"region\"\n\
             \n\
             [[command.a.input]]\n\
             name = \"size\"\n\
             label = \"Size\"\n\
             kind = \"number\"\n\
             min = 1\n\
             max = 10\n\
             default = 3\n\
             \n\
             [[command.a.input]]\n\
             name = \"on\"\n\
             kind = \"checkbox\"\n\
             default = true\n\
             \n\
             [command.c]\n\
             run = \"true\"\n\
             input = [\n\
               { name = \"gain\", kind = \"slider\", min = 0, max = 1, default = 0.5, step = 0.1 },\n\
               { name = \"mode\", kind = \"radio\", choices = [\"fast\", \"slow\"], default = \"slow\" },\n\
               { name = \"note\", kind = \"string\", default = \"hello\" },\n\
             ]\n",
        );
        assert_eq!(problems, Vec::new());
        let names: Vec<&str> = commands.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["b", "a", "c"]);
        let a = &commands[1];
        assert_eq!(a.label, "Do A");
        assert_eq!(a.run, "~/bin/a.sh");
        assert_eq!(a.needs, Needs::Region);
        assert!(!a.confirm && !a.shown && a.asks());
        let inputs: Vec<(&str, &str)> = a
            .inputs
            .iter()
            .map(|i| (i.name.as_str(), i.label.as_str()))
            .collect();
        assert_eq!(inputs, [("size", "Size"), ("on", "on")]);
        assert_eq!(
            a.inputs[0].kind,
            Kind::Number {
                range: Some((1.0, 10.0)),
                default: 3.0
            }
        );
        assert_eq!(a.inputs[1].kind, Kind::Checkbox { default: true });
        let b = &commands[0];
        assert_eq!((b.label.as_str(), b.needs), ("b", Needs::File));
        assert!(b.confirm && b.shown && b.asks());
        let c = &commands[2];
        assert_eq!(
            c.inputs.iter().map(|i| i.kind.clone()).collect::<Vec<_>>(),
            [
                Kind::Slider {
                    low: 0.0,
                    high: 1.0,
                    default: 0.5,
                    step: 0.1
                },
                Kind::Radio {
                    choices: vec!["fast".into(), "slow".into()],
                    default: 1
                },
                Kind::Text {
                    default: "hello".into(),
                    lines: 1
                },
            ]
        );
    }

    /// Each thing that does not read is said on its own line, and a command
    /// with no `run` on the line it is named on; the rest is taken.
    #[test]
    fn a_bad_command_field_is_said_on_its_line() {
        let (commands, problems) = parsed(
            "[command.Bad]\n\
             run = \"x\"\n\
             [command.ok]\n\
             colour = \"red\"\n\
             run = \"echo ok\"\n\
             needs = \"everything\"\n\
             confirm = \"maybe\"\n\
             input = [\n\
               { name = \"my-size\", kind = \"number\" },\n\
               { name = \"size\", kind = \"dial\" },\n\
               { name = \"w\", label = \"Width\" },\n\
               { name = \"s\", kind = \"slider\", min = 5, max = 1 },\n\
               { name = \"t\", kind = \"string\", step = 2 },\n\
             ]\n\
             [command.norun]\n\
             label = \"Nothing\"\n",
        );
        let names: Vec<&str> = commands.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["ok"]);
        assert_eq!(commands[0].inputs, Vec::new());
        let lines: Vec<usize> = problems.iter().map(|(line, _)| *line).collect();
        assert_eq!(lines, [1, 4, 6, 7, 9, 10, 11, 12, 13, 15], "{problems:?}");
        assert_eq!(problems[9].1, "command.norun has no run");
        assert_eq!(
            problems[8].1,
            "command.ok input `t`: a string takes default, lines, not step"
        );
    }

    /// A string may take several lines, a whole number of them up to
    /// `MAX_LINES`; a default on several lines needs them.
    #[test]
    fn a_string_takes_its_lines() {
        let (commands, problems) = parsed(
            "[command.a]\n\
             run = \"true\"\n\
             input = [\n\
               { name = \"prompt\", kind = \"string\", lines = 4, default = \"one\\ntwo\" },\n\
               { name = \"none\", kind = \"string\", lines = 0 },\n\
               { name = \"half\", kind = \"string\", lines = 2.5 },\n\
               { name = \"split\", kind = \"string\", default = \"one\\ntwo\" },\n\
             ]\n",
        );
        assert_eq!(
            commands[0]
                .inputs
                .iter()
                .map(|i| i.kind.clone())
                .collect::<Vec<_>>(),
            [Kind::Text {
                default: "one\ntwo".into(),
                lines: 4
            }]
        );
        let said: Vec<&str> = problems
            .iter()
            .map(|(_, problem)| problem.as_str())
            .collect();
        assert_eq!(
            said,
            [
                "command.a input `none`: lines is a whole number from 1 to 12, not 0",
                "command.a input `half`: lines is a whole number from 1 to 12, not 2.5",
                "command.a input `split`: a default on several lines needs lines to say how many",
            ]
        );
    }

    /// A setting, a key or a gesture written below a command's table is
    /// read as the command's, and said as having been put there.
    #[test]
    fn a_setting_below_a_command_is_said_to_belong_above() {
        let (commands, problems) = parsed(
            "[command.redact]\n\
             run = \"~/bin/redact.sh\"\n\
             keys.command.redact = \"ctrl+shift+r\"\n\
             show_info = true\n",
        );
        assert_eq!(commands.len(), 1);
        let lines: Vec<usize> = problems.iter().map(|(line, _)| *line).collect();
        assert_eq!(lines, [3, 4], "{problems:?}");
        assert!(problems[0].1.contains("goes above the first [command]"));
    }

    /// An output is one of three words, and a command that needs nothing
    /// has no file to replace: it is taken without.
    #[test]
    fn a_command_replaces_its_file_where_it_has_one() {
        let (commands, problems) = parsed(
            "[command.strip]\n\
             run = \"strip.sh\"\n\
             output = \"replace\"\n\
             [command.odd]\n\
             run = \"odd.sh\"\n\
             output = \"beside\"\n\
             [command.none]\n\
             run = \"none.sh\"\n\
             needs = \"none\"\n\
             output = \"replace\"\n",
        );
        let replaces: Vec<(&str, Output)> = commands
            .iter()
            .map(|c| (c.name.as_str(), c.output))
            .collect();
        assert_eq!(
            replaces,
            [
                ("strip", Output::Replace),
                ("odd", Output::Own),
                ("none", Output::Own)
            ]
        );
        let lines: Vec<usize> = problems.iter().map(|(line, _)| *line).collect();
        assert_eq!(lines, [6, 10], "{problems:?}");
        assert!(problems[1].1.contains("cannot need none"), "{problems:?}");
    }

    /// A command that makes a new file may say its name, holding the
    /// file's own as `{stem}` where it has a file; a name anywhere else, or
    /// one that is not a name, is said and left out. One that may replace
    /// its file, needing none, makes a new one instead.
    #[test]
    fn a_new_files_name_is_read_where_one_is_made() {
        let (commands, problems) = parsed(
            "[command.mark]\n\
             run = \"mark.sh\"\n\
             output = \"choose\"\n\
             name = \"{stem}-marked.jpg\"\n\
             [command.make]\n\
             run = \"make.sh\"\n\
             needs = \"none\"\n\
             output = \"new\"\n\
             name = \"{stem}.png\"\n\
             [command.own]\n\
             run = \"own.sh\"\n\
             name = \"own.png\"\n\
             [command.bad]\n\
             run = \"bad.sh\"\n\
             output = \"new\"\n\
             name = \"a/{name}.png\"\n\
             [command.braces]\n\
             run = \"braces.sh\"\n\
             output = \"new\"\n\
             name = \"{when}.png\"\n\
             [command.either]\n\
             run = \"either.sh\"\n\
             needs = \"none\"\n\
             output = \"choose\"\n",
        );
        let read: Vec<(&str, Output, Option<&str>)> = commands
            .iter()
            .map(|c| (c.name.as_str(), c.output, c.new_name.as_deref()))
            .collect();
        assert_eq!(
            read,
            [
                ("mark", Output::Choose, Some("{stem}-marked.jpg")),
                ("make", Output::New, None),
                ("own", Output::Own, None),
                ("bad", Output::New, None),
                ("braces", Output::New, None),
                ("either", Output::New, None),
            ]
        );
        assert!(commands[0].asks() && commands[1].asks() && !commands[2].asks());
        let said: Vec<(usize, &str)> = problems
            .iter()
            .map(|(line, problem)| (*line, problem.as_str()))
            .collect();
        assert_eq!(
            said,
            [
                (
                    9,
                    "command.make.name holds {stem}, the file's own name, and command.make needs none"
                ),
                (
                    12,
                    "command.own.name is the name of the new file that output \"new\" or \"choose\" makes"
                ),
                (
                    16,
                    "command.bad.name is a file's name, with no / and no control characters"
                ),
                (
                    20,
                    "command.braces.name holds a { or } that is not {stem}, the one name it can hold"
                ),
                (
                    24,
                    "command.either replaces the file it runs on, so it cannot need none; it makes a new file instead"
                ),
            ]
        );
    }

    /// A line of the shell keeps every quote it was written with.
    #[test]
    fn a_line_of_the_shell_keeps_its_quotes() {
        let (commands, problems) = parsed(
            "[command.a]\n\
             run = '\"$HOME/bin/a\" \"$GAMUT_FILE\"'\n\
             label = \"Quoted label\"\n",
        );
        assert_eq!(problems, Vec::new());
        assert_eq!(commands[0].run, "\"$HOME/bin/a\" \"$GAMUT_FILE\"");
        assert_eq!(commands[0].label, "Quoted label");
    }

    /// The template's example is a whole command, read back as the file
    /// would read it with the `#` taken off.
    #[test]
    fn the_template_example_is_a_command() {
        let example: String = TEMPLATE
            .lines()
            .filter_map(|line| line.strip_prefix("#   "))
            .map(|line| format!("{line}\n"))
            .collect();
        let (commands, problems) = parsed(&example);
        assert_eq!(problems, Vec::new());
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].inputs.len(), 5);
    }

    /// The result file is lines of `name = value`; a line it cannot
    /// use is said by number.
    #[test]
    fn a_result_file_is_read_as_directives_and_an_unknown_name_is_said() {
        let (asked, problems) = directives(
            "# made by redact.sh\n\
             open = /tmp/a b.png\n\
             message = Redacted 3 faces\n\
             error = \"Quoted\"\n\
             text = nope\n\
             open =\n\
             just words\n",
        );
        assert_eq!(
            asked,
            [
                Directive::Open("/tmp/a b.png".into()),
                Directive::Message("Redacted 3 faces".into()),
                Directive::Error("Quoted".into()),
            ]
        );
        let lines: Vec<usize> = problems.iter().map(|(line, _)| *line).collect();
        assert_eq!(lines, [5, 6, 7]);
    }

    fn launch(line: &str, cwd: &Path) -> Launch {
        Launch {
            id: 0,
            label: "Test".into(),
            about: None,
            line: line.into(),
            cwd: cwd.to_path_buf(),
            env: vec![("GAMUT_INPUT_WORD".into(), "hello world".into())],
            shown: None,
        }
    }

    /// A run is the shell's: the environment is there, the output kept,
    /// the result file read, and the exit code said.
    #[test]
    fn a_run_keeps_its_output_and_reads_its_result_file() {
        let cwd = std::env::temp_dir();
        let finished = run(
            launch(
                "echo \"$GAMUT_INPUT_WORD from $0\"; echo oops >&2; \
                 echo 'message = done' >> \"$GAMUT_RESULT\"; exit 3",
                &cwd,
            ),
            &Stopper::default(),
        );
        assert_eq!(finished.status, Status::Exited(3));
        assert_eq!(finished.stdout.text, "hello world from gamut\n");
        assert_eq!(finished.stderr.text, "oops\n");
        assert_eq!(finished.result, "message = done\n");
        let dir = finished.dir.expect("a directory");
        assert_eq!(
            fs::read_to_string(dir.join("stdout")).unwrap(),
            "hello world from gamut\n"
        );
        let _ = fs::remove_dir_all(dir);
    }

    /// A run stopped is ended, everything its shell started with it.
    #[test]
    fn a_run_is_stopped_with_its_group() {
        let stopper = Stopper::default();
        let stopping = stopper.clone();
        let handle = std::thread::spawn(move || {
            run(launch("sleep 30 & wait", &std::env::temp_dir()), &stopping)
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        while stopper.0.lock().unwrap().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        stopper.stop();
        let finished = handle.join().unwrap();
        assert!(finished.took < Duration::from_secs(10));
        assert_eq!(finished.status, Status::Killed);
        if let Some(dir) = finished.dir {
            let _ = fs::remove_dir_all(dir);
        }
    }

    /// A line that cannot start is said, not waited on.
    #[test]
    fn a_run_in_a_missing_directory_does_not_start() {
        let finished = run(
            launch("true", Path::new("/nonexistent/gamut")),
            &Stopper::default(),
        );
        assert!(
            matches!(finished.status, Status::Unstarted(_)),
            "{finished:?}"
        );
        if let Some(dir) = finished.dir {
            let _ = fs::remove_dir_all(dir);
        }
    }

    /// A stream that says more than is kept keeps its head and says so.
    #[test]
    fn a_long_stream_is_cut() {
        let long = vec![b'x'; CAPTURE_CAP + 10];
        let captured = capture(&long[..]);
        assert!(captured.cut);
        assert_eq!(captured.text.len(), CAPTURE_CAP);
        assert!(!capture(&b"short"[..]).cut);
    }
}
