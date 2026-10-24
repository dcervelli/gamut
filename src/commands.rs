//! The commands the configuration file defines: what each one is, read from
//! its `command.<name>.*` lines, and running one — a line of the shell, run
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

use crate::ui::commands::{Kind, Needs};
use crate::{PROGRAM, xdg};

/// One command, as the configuration file defines it.
#[derive(Clone, PartialEq, Debug)]
pub struct Command {
    /// What follows `command.` on its lines, and what its key is bound by:
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
    /// What its dialog asks for, in the order the file first names each.
    pub inputs: Vec<Input>,
}

impl Command {
    /// Whether pressing it opens its dialog rather than running it.
    pub fn asks(&self) -> bool {
        self.confirm || !self.inputs.is_empty()
    }

    /// The name its key is bound by.
    pub fn key_name(&self) -> String {
        format!("command.{}", self.name)
    }
}

/// One input of a command's dialog.
#[derive(Clone, PartialEq, Debug)]
pub struct Input {
    /// What follows `input.`, and what the script reads it as, upper case:
    /// `GAMUT_INPUT_<NAME>`.
    pub name: String,
    pub label: String,
    pub kind: Kind,
}

/// Whether `name` is a command's name: lower-case letters, digits, `_` and
/// `-`.
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

/// A command as it is read, before the file is over.
struct Partial {
    name: String,
    /// The line it first appears on, which a missing `run` is said on.
    first: usize,
    label: Option<String>,
    run: Option<String>,
    needs: Needs,
    confirm: bool,
    shown: bool,
    inputs: Vec<PartialInput>,
}

struct PartialInput {
    name: String,
    first: usize,
    label: Option<String>,
    kind: Option<Kind>,
    /// Whether a kind was written and refused, which has been said once.
    refused: bool,
}

/// The commands that `lines` define — each the line's number, its name with
/// `command.` taken off, and its value — in the order each is first named,
/// and each line that could not be used, with what was wrong with it. A
/// field written twice is taken from the later line, as a setting is.
pub fn parse(lines: &[(usize, &str, &str)]) -> (Vec<Command>, Vec<(usize, String)>) {
    let mut partials: Vec<Partial> = Vec::new();
    let mut problems = Vec::new();
    for &(number, rest, value) in lines {
        let Some((name, field)) = rest.split_once('.') else {
            problems.push((
                number,
                format!(
                    "command.{rest} names no field: label, run, needs, confirm, shown or input"
                ),
            ));
            continue;
        };
        if !command_name(name) {
            problems.push((
                number,
                format!("`{name}` is not a command's name: a-z, 0-9, _ and - only"),
            ));
            continue;
        }
        let at = match partials.iter().position(|partial| partial.name == name) {
            Some(at) => at,
            None => {
                partials.push(Partial {
                    name: name.to_string(),
                    first: number,
                    label: None,
                    run: None,
                    needs: Needs::default(),
                    confirm: false,
                    shown: false,
                    inputs: Vec::new(),
                });
                partials.len() - 1
            }
        };
        let partial = &mut partials[at];
        let flag = |value: &str| match value {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(format!(
                "command.{name}.{field} is true or false, not `{value}`"
            )),
        };
        let outcome = match field {
            "label" if value.is_empty() => Err(format!("command.{name}.label is empty")),
            "label" => {
                partial.label = Some(value.to_string());
                Ok(())
            }
            "run" if value.is_empty() => Err(format!("command.{name}.run is empty")),
            "run" => {
                partial.run = Some(value.to_string());
                Ok(())
            }
            "needs" => Needs::read(value)
                .map(|needs| partial.needs = needs)
                .map_err(|problem| format!("command.{name} {problem}")),
            "confirm" => flag(value).map(|on| partial.confirm = on),
            "shown" => flag(value).map(|on| partial.shown = on),
            input => match input.strip_prefix("input.") {
                None => Err(format!(
                    "unknown field `{input}` of command.{name}: label, run, needs, confirm, \
                     shown or input"
                )),
                Some(input) => read_input(partial, number, name, input, value),
            },
        };
        if let Err(problem) = outcome {
            problems.push((number, problem));
        }
    }
    let mut commands = Vec::new();
    for partial in partials {
        let Some(run) = partial.run else {
            problems.push((
                partial.first,
                format!("command.{} has no run line", partial.name),
            ));
            continue;
        };
        let mut inputs = Vec::new();
        for input in partial.inputs {
            match input.kind {
                Some(kind) => inputs.push(Input {
                    label: input.label.unwrap_or_else(|| input.name.clone()),
                    name: input.name,
                    kind,
                }),
                None if input.refused => {}
                None => problems.push((
                    input.first,
                    format!(
                        "command.{}.input.{} has no kind: string, number, checkbox, radio or \
                         slider",
                        partial.name, input.name
                    ),
                )),
            }
        }
        commands.push(Command {
            label: partial.label.unwrap_or_else(|| partial.name.clone()),
            name: partial.name,
            run,
            needs: partial.needs,
            confirm: partial.confirm,
            shown: partial.shown,
            inputs,
        });
    }
    problems.sort_by_key(|(number, _)| *number);
    (commands, problems)
}

/// A `command.<name>.input.` line: `input` is what follows `input.`, its
/// name alone for the kind or with `.label` after it for its words.
fn read_input(
    partial: &mut Partial,
    number: usize,
    name: &str,
    input: &str,
    value: &str,
) -> Result<(), String> {
    let (input, label) = match input.split_once('.') {
        Some((input, "label")) => (input, true),
        Some((input, other)) => {
            return Err(format!(
                "unknown field `{other}` of command.{name}.input.{input}: only label"
            ));
        }
        None => (input, false),
    };
    if !input_name(input) {
        return Err(format!(
            "`{input}` is not an input's name: a-z, 0-9 and _ only"
        ));
    }
    let at = match partial.inputs.iter().position(|each| each.name == input) {
        Some(at) => at,
        None => {
            partial.inputs.push(PartialInput {
                name: input.to_string(),
                first: number,
                label: None,
                kind: None,
                refused: false,
            });
            partial.inputs.len() - 1
        }
    };
    let each = &mut partial.inputs[at];
    if label {
        each.label = Some(value.to_string());
        return Ok(());
    }
    match Kind::read(value) {
        Ok(kind) => {
            each.kind = Some(kind);
            Ok(())
        }
        Err(problem) => {
            each.refused = true;
            Err(format!("command.{name}.input.{input}: {problem}"))
        }
    }
}

/// The commented example the template ends with: every field, and an input
/// of each kind. Indented after the `#`, so that it is read as an example
/// rather than as a setting left at its default.
pub const TEMPLATE: &str = "\n\
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

/// One thing the result file asks the window to do.
#[derive(Clone, PartialEq, Debug)]
pub enum Directive {
    /// Show this image, which the script made.
    Open(String),
    Message(String),
    Error(String),
}

/// What the result file `text` asks for, line by line, and each line that
/// does not read, by number. It is read as the configuration file is:
/// `name = value`, `#` starting a comment.
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
    let base = format!("{}-{id}", std::process::id());
    let mut tries = 0;
    loop {
        let dir = match tries {
            0 => runs.join(&base),
            n => runs.join(format!("{base}-{n}")),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> (Vec<Command>, Vec<(usize, String)>) {
        let lines: Vec<(usize, &str, &str)> = crate::settings::lines(text)
            .filter_map(|(number, line)| {
                let (name, value) = line?;
                Some((number, name.strip_prefix("command.")?, value))
            })
            .collect();
        parse(&lines)
    }

    /// A command's lines may come in any order and be spread through the
    /// file; it is listed where it is first named, and its inputs where
    /// each is.
    #[test]
    fn a_command_block_is_read_in_order_of_first_appearance() {
        let (commands, problems) = parsed(
            "command.b.run = echo b\n\
             command.a.input.size = number 1 10 3\n\
             command.a.run = ~/bin/a.sh\n\
             command.a.label = Do A\n\
             command.a.needs = region\n\
             command.a.input.size.label = Size\n\
             command.a.input.on = checkbox true\n\
             command.b.confirm = true\n\
             command.b.shown = true\n",
        );
        assert_eq!(problems, Vec::new());
        let names: Vec<&str> = commands.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["b", "a"]);
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
        let b = &commands[0];
        assert_eq!((b.label.as_str(), b.needs), ("b", Needs::File));
        assert!(b.confirm && b.shown && b.asks());
    }

    /// Each line that does not read is said on its own line, and a command
    /// with no `run` on the line it first appeared on; the rest is taken.
    #[test]
    fn a_bad_command_line_is_said_on_its_line() {
        let (commands, problems) = parsed(
            "command.Bad.run = x\n\
             command.ok.colour = red\n\
             command.ok.run = echo ok\n\
             command.ok.input.my-size = number\n\
             command.ok.input.size = dial\n\
             command.norun.label = Nothing\n\
             command.ok.needs = everything\n\
             command.ok.confirm = maybe\n\
             command.ok.input.w.label = Width\n\
             command.alone = x\n",
        );
        let names: Vec<&str> = commands.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["ok"]);
        assert_eq!(commands[0].inputs, Vec::new());
        let lines: Vec<usize> = problems.iter().map(|(line, _)| *line).collect();
        assert_eq!(lines, [1, 2, 4, 5, 6, 7, 8, 9, 10], "{problems:?}");
        assert_eq!(problems[4].1, "command.norun has no run line");
    }

    /// A line of the shell that begins and ends with quotes of its own
    /// keeps them; a value wholly in one pair loses the pair.
    #[test]
    fn a_line_quoted_at_both_ends_keeps_its_quotes() {
        let (commands, problems) = parsed(
            "command.a.run = \"$HOME/bin/a\" \"$GAMUT_FILE\"\n\
             command.a.label = \"Quoted label\"\n",
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
            .filter_map(|line| line.strip_prefix("#   command."))
            .map(|line| format!("command.{line}\n"))
            .collect();
        let (commands, problems) = parsed(&example);
        assert_eq!(problems, Vec::new());
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].inputs.len(), 5);
    }

    /// The result file is read as the configuration is; a line it cannot
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
