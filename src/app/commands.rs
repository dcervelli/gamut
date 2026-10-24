//! The configuration file's commands as the application holds them: the
//! set in force, the one run in flight, the dialog while it is up, what
//! each file's last run printed, and the values each dialog was last left
//! with. What a command is and how it is run are `commands.rs`'s; what a
//! frame draws from this, `ui/commands.rs`'s and `ui/output.rs`'s.
//!
//! One run at a time: every command is dead while one runs, and the
//! message that says it is running carries the cross that stops it, so a
//! script that hangs costs a press rather than the session.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use super::App;
use super::edits::Replacing;
use super::files::SLOW_READ;
use super::input::Effect;
use crate::commands::{self, Command, Directive, Finished, Launch, Status, Stopper};
use crate::image::auxiliary::Showing;
use crate::image::exif;
use crate::image::orient::{self, Orientation};
use crate::image::region::Region;
use crate::image::sequence::Sequence;
use crate::loader::Source;
use crate::ui::commands::{Item, Kind, Slot, Value};
use crate::ui::output::{self, Ended, Line, Run};
use crate::ui::toast::{Level, Toast};
use crate::ui::{self, Control};

/// How many files' last runs are kept for the Output tab; the oldest goes.
const OUTPUTS_KEPT: usize = commands::RUNS_KEPT;

/// The commands and everything about them that changes as the session
/// goes.
pub(super) struct Commands {
    /// The set in force: the configuration file's, as last read.
    pub(super) set: Vec<Command>,
    running: Option<Running>,
    /// The dialog, while it is up.
    pub(super) asking: Option<Asking>,
    /// The values each command's dialog was last let go with, by name,
    /// for it to open with next time. This session's only: what is typed
    /// into a dialog may be a secret, and nothing here is written down.
    remembered: HashMap<String, Vec<Value>>,
    /// Each file's last run, by the file it ran on — `None` for one run
    /// with nothing open — and the order they were kept in.
    outputs: HashMap<Option<PathBuf>, Arc<Run>>,
    /// Where each of those runs left its output on disk.
    logs: HashMap<Option<PathBuf>, PathBuf>,
    order: VecDeque<Option<PathBuf>>,
    /// Numbers the runs.
    runs: u64,
    /// How a run's answer reaches the loop.
    delivered: commands::Deliver,
}

/// The run in flight.
struct Running {
    id: u64,
    label: String,
    /// Where it runs, which a relative `open` is read against.
    cwd: PathBuf,
    about: Option<PathBuf>,
    started: Instant,
    stopper: Stopper,
    /// Whether the window ended it.
    stopped: bool,
    /// Where its output is to take the file's place: the file, and where
    /// the output goes.
    replacing: Option<Replacing>,
}

/// The dialog while it is up: which command it is for, and what each
/// input holds.
pub(super) struct Asking {
    index: usize,
    values: Vec<Value>,
    /// Whether this is its first frame, when its first field takes the
    /// keyboard.
    opened: bool,
}

impl Commands {
    pub(super) fn new(set: Vec<Command>, delivered: commands::Deliver) -> Self {
        Self {
            set,
            running: None,
            asking: None,
            remembered: HashMap::new(),
            outputs: HashMap::new(),
            logs: HashMap::new(),
            order: VecDeque::new(),
            runs: 0,
            delivered,
        }
    }

    /// Whether a run is in flight.
    pub(super) fn running(&self) -> bool {
        self.running.is_some()
    }

    /// The message a run in flight keeps up once it has gone on long enough
    /// to say, with the cross that stops it.
    pub(super) fn toast(&self, now: Instant) -> Option<Toast> {
        let running = self.running.as_ref()?;
        let raised = running.started + SLOW_READ;
        (now >= raised).then(|| {
            Toast::stoppable(
                format!("Running {}\u{2026}", running.label),
                raised,
                Control::StopCommand,
            )
        })
    }

    /// When that message is due, for the loop to wake for it.
    pub(super) fn due(&self, now: Instant) -> Option<Instant> {
        let raised = self.running.as_ref()?.started + SLOW_READ;
        (now < raised).then_some(raised)
    }

    /// What the Output tab shows for `file`.
    pub(super) fn output_for(&self, file: Option<&Path>) -> output::State {
        let key = file.map(Path::to_path_buf);
        if let Some(running) = &self.running
            && running.about == key
        {
            return output::State::Running(running.label.clone());
        }
        match self.outputs.get(&key) {
            Some(run) => output::State::Shown(Arc::clone(run)),
            None => output::State::Nothing,
        }
    }

    /// The last run on `file`, where one is kept.
    pub(super) fn run_on(&self, file: Option<&Path>) -> Option<Arc<Run>> {
        self.outputs.get(&file.map(Path::to_path_buf)).cloned()
    }

    /// Where the last run on `file` left its output, where it was kept.
    pub(super) fn log_of(&self, file: Option<&Path>) -> Option<PathBuf> {
        self.logs.get(&file.map(Path::to_path_buf)).cloned()
    }

    /// Keeps `run` as `about`'s last, letting the oldest go past
    /// [`OUTPUTS_KEPT`].
    fn keep(&mut self, about: Option<PathBuf>, run: Run, log: Option<PathBuf>) {
        self.order.retain(|each| *each != about);
        self.order.push_back(about.clone());
        match log {
            Some(log) => self.logs.insert(about.clone(), log),
            None => self.logs.remove(&about),
        };
        self.outputs.insert(about, Arc::new(run));
        while self.order.len() > OUTPUTS_KEPT {
            if let Some(oldest) = self.order.pop_front() {
                self.outputs.remove(&oldest);
                self.logs.remove(&oldest);
            }
        }
    }

    /// The set read again: the dialog comes down, since the command it was
    /// asking for may have changed under it; the run in flight, the output
    /// kept and the values remembered stay.
    pub(super) fn replace(&mut self, set: Vec<Command>) {
        self.set = set;
        self.asking = None;
    }

    /// The window closing. A run that was to replace its file is ended,
    /// since nothing will be left to put its output in the file's place, and
    /// the directory made for that output taken away; any other run is
    /// left to finish on its own.
    pub(super) fn abandon(&mut self) {
        if let Some(running) = &self.running
            && let Some(replacing) = &running.replacing
        {
            running.stopper.stop();
            let _ = std::fs::remove_dir_all(&replacing.dir);
        }
    }
}

#[cfg(test)]
impl Commands {
    /// Has each run's answer handed to `deliver`: what a test waits on in
    /// place of the event loop.
    pub(super) fn deliver_to(&mut self, deliver: commands::Deliver) {
        self.delivered = deliver;
    }
}

/// `path` with a leading `~/` read as the home directory, as the shell
/// would.
fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), crate::xdg::home()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ if path == "~" => crate::xdg::home().unwrap_or_else(|| PathBuf::from(path)),
        _ => PathBuf::from(path),
    }
}

/// `path` absolute, with its `.` and `..` read away, as the script may
/// write it: for telling whether two spellings name the same place without
/// asking the disk, where the one may already be gone.
fn lexically(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut read = PathBuf::new();
    for part in absolute.components() {
        match part {
            std::path::Component::ParentDir => {
                read.pop();
            }
            std::path::Component::CurDir => {}
            other => read.push(other),
        }
    }
    read
}

/// A value fit for the environment: a NUL cannot be in one, and a
/// control character from a file's metadata has no business reaching a
/// script that may print it to a terminal.
fn clean(value: &str) -> String {
    crate::escape_controls(value)
}

/// A number for the environment: as few places as say it.
fn number(value: f64) -> String {
    ui::commands::decimal(value)
}

/// `moment`, read on a clock showing `wall`, as ISO 8601 with the offset
/// the two are apart by.
fn iso_8601(moment: SystemTime, wall: crate::clock::Civil) -> String {
    let epoch = match moment.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(since) => since.as_secs() as i64,
        Err(before) => -(before.duration().as_secs() as i64),
    };
    let offset = crate::clock::seconds(wall) - epoch;
    let sign = if offset < 0 { '-' } else { '+' };
    let offset = offset.unsigned_abs();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{sign}{:02}:{:02}",
        wall.year,
        wall.month,
        wall.day,
        wall.hour,
        wall.minute,
        wall.second,
        offset / 3600,
        offset % 3600 / 60,
    )
}

/// The fields of view a 35 mm-equivalent focal length gives a picture of
/// `width` by `height`, horizontal, vertical and diagonal, in degrees: the
/// full frame's diagonal shared out by the picture's own aspect.
pub(super) fn fields_of_view(focal: f64, width: f64, height: f64) -> [f64; 3] {
    let diagonal = crate::image::attitude::FULL_FRAME_DIAGONAL;
    let aspect = width / height;
    let across = diagonal * aspect / aspect.hypot(1.0);
    let down = diagonal / aspect.hypot(1.0);
    [across, down, diagonal].map(|side| (2.0 * (side / (2.0 * focal)).atan()).to_degrees())
}

impl App {
    /// The menu's items: each command's label, whether it asks first, the
    /// control a press on it is, and whether that press would be taken.
    pub(super) fn command_items(&self, conditions: &super::input::Conditions) -> Vec<Item> {
        self.commands
            .set
            .iter()
            .enumerate()
            .map(|(index, command)| {
                let slot = Slot {
                    index,
                    needs: command.needs,
                };
                Item {
                    label: command.label.clone(),
                    asks: command.asks(),
                    slot,
                    enabled: ui::tooltip::disabled(
                        ui::Tip::Control(Control::RunCommand(slot)),
                        *conditions,
                    )
                    .is_none(),
                }
            })
            .collect()
    }

    /// A press on a command: its dialog put up where it asks first, and
    /// otherwise the command run. Refused where the frame it was drawn
    /// from is out of date — the file read again since — or another dialog
    /// is up.
    pub(super) fn press_command(&mut self, slot: Slot) -> Effect {
        let Some(command) = self.commands.set.get(slot.index) else {
            return Effect::Nothing;
        };
        if command.needs != slot.needs
            || self.renaming.is_some()
            || self.exporting.is_some()
            || self.commands.asking.is_some()
        {
            return Effect::Nothing;
        }
        let asks = command.asks();
        self.close_menus();
        if asks {
            self.open_command_dialog(slot.index);
            return Effect::Redraw;
        }
        self.start_command(slot.index, &[])
    }

    /// Puts up the dialog of the command at `index`, with the values it
    /// was last let go with where they still fit its inputs, and each
    /// input's default where they do not.
    fn open_command_dialog(&mut self, index: usize) {
        let Some(command) = self.commands.set.get(index) else {
            return;
        };
        let defaults: Vec<Value> = command
            .inputs
            .iter()
            .map(|input| input.kind.default_value())
            .collect();
        let values = match self.commands.remembered.get(&command.name) {
            Some(values)
                if values.len() == command.inputs.len()
                    && command
                        .inputs
                        .iter()
                        .zip(values)
                        .all(|(input, value)| input.kind.holds(value)) =>
            {
                values.clone()
            }
            _ => defaults,
        };
        self.commands.asking = Some(Asking {
            index,
            values,
            opened: true,
        });
    }

    /// An input of the dialog changed.
    pub(super) fn set_command_input(&mut self, input: usize, value: Value) -> Effect {
        let Some(asking) = self.commands.asking.as_mut() else {
            return Effect::Nothing;
        };
        let Some(command) = self.commands.set.get(asking.index) else {
            return Effect::Nothing;
        };
        let fits = command.inputs.get(input).is_some_and(|each| {
            matches!(
                (&each.kind, &value),
                (Kind::Text { .. }, Value::Text(_))
                    | (Kind::Number { .. }, Value::Number(_))
                    | (Kind::Checkbox { .. }, Value::Checkbox(_))
                    | (Kind::Radio { .. }, Value::Radio(_))
                    | (Kind::Slider { .. }, Value::Slider(_))
            ) && (matches!(value, Value::Text(_) | Value::Number(_)) || each.kind.holds(&value))
        });
        if !fits {
            return Effect::Nothing;
        }
        asking.values[input] = value;
        Effect::Redraw
    }

    /// Whether every input of the dialog holds something the command can
    /// be run with.
    fn command_allowed(&self) -> bool {
        let Some(asking) = &self.commands.asking else {
            return false;
        };
        let Some(command) = self.commands.set.get(asking.index) else {
            return false;
        };
        command
            .inputs
            .iter()
            .zip(&asking.values)
            .all(|(input, value)| input.kind.judge(value).is_none())
    }

    /// The dialog's OK: the values remembered for next time, the dialog
    /// down and the command run with them. Refused while an input holds
    /// something that will not do, or another command runs.
    pub(super) fn confirm_command(&mut self) -> Effect {
        if !self.command_allowed() || self.commands.running() {
            return Effect::Nothing;
        }
        // What it needs may have gone while it asked: the region taken off.
        let still = self.commands.asking.as_ref().and_then(|asking| {
            let needs = self.commands.set.get(asking.index)?.needs;
            let slot = Slot {
                index: asking.index,
                needs,
            };
            Some(!self.refuses(Control::RunCommand(slot)))
        });
        if still != Some(true) {
            return Effect::Nothing;
        }
        let Some(asking) = self.commands.asking.take() else {
            return Effect::Nothing;
        };
        let Some(command) = self.commands.set.get(asking.index) else {
            return Effect::Redraw;
        };
        let inputs: Vec<(String, String)> = command
            .inputs
            .iter()
            .zip(&asking.values)
            .map(|(input, value)| {
                (
                    format!("GAMUT_INPUT_{}", input.name.to_ascii_uppercase()),
                    input.kind.environment(value),
                )
            })
            .collect();
        self.commands
            .remembered
            .insert(command.name.clone(), asking.values);
        self.start_command(asking.index, &inputs)
            .also(Effect::Redraw)
    }

    /// The dialog's Cancel, and Escape.
    pub(super) fn cancel_command(&mut self) -> Effect {
        Effect::redraw_if(self.commands.asking.take().is_some())
    }

    /// What the dialog is drawn from this frame, while it is up.
    pub(super) fn command_input(&mut self) -> Option<ui::commands::Input> {
        let allowed = self.command_allowed();
        let busy = self.commands.running();
        let asking = self.commands.asking.as_mut()?;
        let command = self.commands.set.get(asking.index)?;
        let rows = command
            .inputs
            .iter()
            .zip(&asking.values)
            .map(|(input, value)| ui::commands::Row {
                label: input.label.clone(),
                kind: input.kind.clone(),
                value: value.clone(),
                refused: input.kind.judge(value),
            })
            .collect();
        Some(ui::commands::Input {
            title: command.label.clone(),
            rows,
            allowed: allowed && !busy,
            busy,
            opened: std::mem::take(&mut asking.opened),
        })
    }

    /// Runs the command at `index` with `inputs` in its environment beside
    /// everything the window knows, on a thread of its own.
    fn start_command(&mut self, index: usize, inputs: &[(String, String)]) -> Effect {
        if self.commands.running() {
            return Effect::Nothing;
        }
        let Some(command) = self.commands.set.get(index).cloned() else {
            return Effect::Nothing;
        };
        let about = self.shown_path().filter(|_| self.current.is_some());
        let cwd = about
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .filter(|dir| !dir.as_os_str().is_empty())
            .or_else(crate::xdg::home)
            .unwrap_or_else(|| PathBuf::from("/"));
        let mut env = self.command_environment();
        env.extend(
            inputs
                .iter()
                .map(|(name, value)| (name.clone(), value.replace('\0', ""))),
        );
        let shown = match (&self.current, command.shown) {
            (Some(current), true) => {
                let seen = current.seen();
                let pixels = current.pixels();
                env.push(("GAMUT_SHOWN_WIDTH".into(), pixels[0].to_string()));
                env.push(("GAMUT_SHOWN_HEIGHT".into(), pixels[1].to_string()));
                if let Some(region) = self.marking.selection.region() {
                    env.extend([
                        ("GAMUT_SHOWN_REGION_X".into(), region.x.to_string()),
                        ("GAMUT_SHOWN_REGION_Y".into(), region.y.to_string()),
                        ("GAMUT_SHOWN_REGION_WIDTH".into(), region.width.to_string()),
                        (
                            "GAMUT_SHOWN_REGION_HEIGHT".into(),
                            region.height.to_string(),
                        ),
                    ]);
                }
                let render: commands::Render = Box::new(move || {
                    let raster = seen.raster(Region::whole(pixels));
                    crate::image::encode::png(&raster).map_err(|error| format!("{error:#}"))
                });
                Some(render)
            }
            _ => None,
        };
        let id = self.commands.runs + 1;
        // The file as the list spells it, which what is done to it once the
        // run is over finds it by; the absolute path is the script's.
        let listed = self.files.shown_path().map(Path::to_path_buf);
        let replacing = match (command.replaces, &about, listed) {
            (true, Some(file), Some(listed)) => {
                match self.prepare_replacing(&command.label, file, listed, id) {
                    Some(replacing) => Some(replacing),
                    None => return Effect::Redraw,
                }
            }
            _ => None,
        };
        if let Some(replacing) = &replacing {
            env.push((
                "GAMUT_OUTPUT".into(),
                replacing.output.display().to_string(),
            ));
        }
        self.commands.runs = id;
        let stopper = Stopper::default();
        self.commands.running = Some(Running {
            id,
            label: command.label.clone(),
            cwd: cwd.clone(),
            about: about.clone(),
            started: Instant::now(),
            stopper: stopper.clone(),
            stopped: false,
            replacing,
        });
        commands::run_on_thread(
            Launch {
                id,
                label: command.label,
                about,
                line: command.run,
                cwd,
                env,
                shown,
            },
            stopper,
            Arc::clone(&self.commands.delivered),
        );
        Effect::Redraw
    }

    /// What run `id` of `label`, which replaces `file` — `listed` on the
    /// list — is given to write its output to, and the file as it is now,
    /// which it has to still be once the run is over. `None`, with the
    /// reason said, where the file cannot be replaced: there is no trash to
    /// keep the original in, the file is a link — whose replacement would
    /// be a file in the link's place — or is not there, or its folder will
    /// not take a directory.
    fn prepare_replacing(
        &mut self,
        label: &str,
        file: &Path,
        listed: PathBuf,
        id: u64,
    ) -> Option<Replacing> {
        let name = super::edits::name_of(file);
        if self.trash.is_none() {
            self.toast(super::edits::NO_TRASH, Level::Error);
            return None;
        }
        let was = match std::fs::symlink_metadata(file) {
            Ok(link) if link.is_symlink() => {
                self.toast(
                    format!("{label} replaces the file it runs on, and {name} is a link."),
                    Level::Error,
                );
                return None;
            }
            _ => crate::watch::Signature::of(file),
        };
        let Some(was) = was else {
            self.toast(
                format!("{label} could not start: {name} is not there."),
                Level::Error,
            );
            return None;
        };
        let dir = match commands::make_output_dir(file, id) {
            Ok(dir) => dir,
            Err(error) => {
                self.toast(
                    format!(
                        "{label} failed to start: {}",
                        crate::escape_controls(&format!(
                            "making a directory beside {name}: {error}"
                        ))
                    ),
                    Level::Error,
                );
                return None;
            }
        };
        let output = dir.join(file.file_name().unwrap_or_default());
        Some(Replacing {
            file: listed,
            dir,
            output,
            was,
        })
    }

    /// The cross on the running message: the run ended, and everything it
    /// started.
    pub(super) fn stop_command(&mut self) -> Effect {
        let Some(running) = self.commands.running.as_mut() else {
            return Effect::Nothing;
        };
        running.stopped = true;
        running.stopper.stop();
        Effect::Redraw
    }

    /// Everything the window knows that a command may want, as the
    /// variables it is given: the file, the picture, what the file says of
    /// itself, the pointer, the region and where it all is on the ground.
    /// A variable is left out rather than set empty where there is nothing
    /// to say. Pixels are in the file's own grid, as its bytes lie, which
    /// is what a tool that reads the file sees.
    pub(super) fn command_environment(&self) -> Vec<(String, String)> {
        let mut env: Vec<(String, String)> = Vec::new();
        let mut set = |name: &str, value: String| env.push((format!("GAMUT_{name}"), value));
        set("VERSION", env!("CARGO_PKG_VERSION").to_string());
        set("COUNT", self.files.len().to_string());
        let Some(current) = &self.current else {
            return env;
        };
        if let Some(path) = self.shown_path() {
            set("FILE", path.display().to_string());
            if let Some(dir) = path.parent() {
                set("DIR", dir.display().to_string());
            }
            if let Some(name) = path.file_name() {
                set("NAME", name.to_string_lossy().into_owned());
            }
            if let Some(stem) = path.file_stem() {
                set("STEM", stem.to_string_lossy().into_owned());
            }
            if let Some(extension) = path.extension() {
                set("EXT", extension.to_string_lossy().into_owned());
            }
            set("INDEX", (self.files.index() + 1).to_string());
        }
        if let Some(reader) = current.file.reader {
            set("FORMAT", reader.to_string());
        }
        // The picture as decoded is upright, its orientation tag applied;
        // the file's grid is that tag undone, and what the window holds in
        // the turned picture is the user's turn undone first.
        let picture = current.picture().0;
        let upright = [picture.width, picture.height];
        let tag = current
            .exif
            .orientation
            .unwrap_or(Orientation::NoTransforms);
        let (width, height) = orient::size(upright[0], upright[1], tag);
        set("WIDTH", width.to_string());
        set("HEIGHT", height.to_string());
        set("ORIENTATION", tag.to_exif().to_string());
        set("TURN", (current.turn.quarters() * 90).to_string());
        match current.sequence {
            Sequence::Animation { .. } => {
                if let Some(animation) = &self.animation {
                    set("FRAME", (animation.on_screen() + 1).to_string());
                    set("FRAMES", animation.count().to_string());
                }
            }
            Sequence::Pages { count, .. } => {
                set("PAGE", (current.page + 1).to_string());
                set("PAGES", count.to_string());
            }
            Sequence::Still => {}
        }
        let exif = &current.exif;
        if let Some([latitude, longitude]) = exif.position {
            set("LAT", format!("{latitude:.6}"));
            set("LNG", format!("{longitude:.6}"));
        }
        if let Some(taken) = exif.taken {
            set("TAKEN", iso_8601(taken.moment, taken.wall));
        }
        let camera = exif
            .sections
            .iter()
            .filter(|section| section.group == exif::Group::Camera)
            .flat_map(|section| &section.entries)
            .find(|entry| entry.name == exif::CAMERA);
        if let Some(camera) = camera {
            set("CAMERA", clean(&camera.value));
        }
        if let Some(focal) = exif.focal_equivalent.filter(|focal| *focal > 0.0) {
            set("FOCAL_35MM", number((focal * 100.0).round() / 100.0));
            let [across, down, diagonal] =
                fields_of_view(focal, f64::from(upright[0]), f64::from(upright[1]));
            for (name, angle) in [("FOV_H", across), ("FOV_V", down), ("FOV_D", diagonal)] {
                set(name, format!("{angle:.2}"));
            }
        }
        if let Some(heading) = exif.heading {
            set("HEADING", format!("{heading:.2}"));
            let reference = if exif.magnetic { "magnetic" } else { "true" };
            set("HEADING_REF", reference.to_string());
        }
        // The pointer and the region are the picture's only while the
        // picture is what is shown: a map in its place has pixels of its
        // own.
        let on_picture = current.showing == Showing::Picture;
        let pointer = self
            .pointer_pixel()
            .filter(|_| on_picture)
            .map(|at| current.turn.stored(at, upright));
        if let Some(at) = pointer {
            let [x, y] = orient::raw(tag, at, upright);
            set("POINTER_X", x.to_string());
            set("POINTER_Y", y.to_string());
        }
        let region = self
            .marking
            .selection
            .region()
            .filter(|_| on_picture)
            .map(|region| region.stored(current.turn, upright));
        if let Some(region) = region {
            let raw = region.raw(tag, upright);
            set("REGION_X", raw.x.to_string());
            set("REGION_Y", raw.y.to_string());
            set("REGION_WIDTH", raw.width.to_string());
            set("REGION_HEIGHT", raw.height.to_string());
        }
        // Where it is on the ground, in the grid the georeference reads:
        // the picture as decoded, which a georeferenced file never turns.
        if let Some(geo) = &exif.georeference {
            let [w, h] = upright.map(f64::from);
            if let Some(code) = geo.code() {
                set("GEO_CRS", format!("EPSG:{code}"));
            }
            if let Some(unit) = geo.unit() {
                set("GEO_UNIT", unit.to_string());
            }
            let transform: Vec<String> = geo.transform().into_iter().map(number).collect();
            set("GEO_TRANSFORM", transform.join(" "));
            let mut place = |name: &str, at: [f64; 2]| {
                let [x, y] = geo.model_at(at);
                set(&format!("GEO_{name}_X"), number(x));
                set(&format!("GEO_{name}_Y"), number(y));
                if geo.offers_geographic()
                    && let Some([latitude, longitude]) = geo.latitude_longitude_at(at)
                {
                    set(&format!("GEO_{name}_LAT"), format!("{latitude:.8}"));
                    set(&format!("GEO_{name}_LNG"), format!("{longitude:.8}"));
                }
            };
            place("UL", [0.0, 0.0]);
            place("UR", [w, 0.0]);
            place("LR", [w, h]);
            place("LL", [0.0, h]);
            place("CENTER", [w / 2.0, h / 2.0]);
            if let Some([x, y]) = pointer {
                place("POINTER", [f64::from(x) + 0.5, f64::from(y) + 0.5]);
            }
            if let Some(region) = region {
                place("REGION_UL", [f64::from(region.x), f64::from(region.y)]);
                place(
                    "REGION_LR",
                    [
                        f64::from(region.x + region.width),
                        f64::from(region.y + region.height),
                    ],
                );
            }
        }
        env
    }

    /// A run is over: what it asked for in its result file is done, what it
    /// printed kept for the Output tab, and how it went said.
    pub(super) fn command_finished(&mut self, finished: Finished) -> Effect {
        let running = match &self.commands.running {
            Some(running) if running.id == finished.id => self.commands.running.take(),
            _ => None,
        };
        let (stopped, cwd, replacing) = match running {
            Some(running) => (running.stopped, Some(running.cwd), running.replacing),
            None => (false, None, None),
        };
        let label = finished.label.clone();
        let mut lines: Vec<Line> = Vec::new();
        let mut push = |text: &str, error: bool| {
            for line in text.lines() {
                lines.push(Line {
                    text: crate::escape_controls(line),
                    error,
                });
            }
        };
        push(&finished.stdout.text, false);
        if finished.stdout.cut {
            push(
                &format!(
                    "\u{2026} more output, not kept past {} bytes",
                    commands::CAPTURE_CAP
                ),
                true,
            );
        }
        push(&finished.stderr.text, true);
        if finished.stderr.cut {
            push(
                &format!(
                    "\u{2026} more errors, not kept past {} bytes",
                    commands::CAPTURE_CAP
                ),
                true,
            );
        }
        let (asked, problems) = commands::directives(&finished.result);
        for (number, problem) in &problems {
            push(&format!("result line {number}: {problem}"), true);
        }
        let first_error = finished
            .stderr
            .text
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(crate::escape_controls);
        let ended = match &finished.status {
            Status::Exited(code) => Ended::Exited(*code),
            Status::Killed => Ended::Killed,
            Status::Unstarted(reason) => {
                push(&format!("did not start: {reason}"), true);
                Ended::Unstarted
            }
        };
        let failed = ended != Ended::Exited(0);
        let reveal = !finished.stdout.text.trim().is_empty()
            || (failed && !finished.stderr.text.trim().is_empty());
        let logged = finished.dir.is_some();
        // What of the result file could not be done, said in the Output tab
        // as well as by a message: the message has one slot, and what a
        // replacement says is said over it.
        let mut refused: Vec<String> = Vec::new();
        let mut effect = Effect::Redraw;
        // The file replaced before anything the result file asks is done,
        // so that an `open` of it finds the new one.
        let unreplaced = finished.status == Status::Exited(0)
            && replacing
                .as_ref()
                .is_some_and(|replacing| !self.replace_file(&label, replacing));
        match finished.status {
            Status::Unstarted(reason) => {
                self.toast(
                    format!(
                        "{label} failed to start: {}",
                        crate::escape_controls(&reason)
                    ),
                    Level::Error,
                );
            }
            Status::Killed if stopped => self.toast(format!("Stopped {label}."), Level::Message),
            Status::Killed => {
                self.toast(format!("{label} failed: ended by a signal"), Level::Error)
            }
            // The file left as it was, and why said: nothing the result
            // file asks for is done, since what it says was about a file
            // that did not arrive.
            Status::Exited(0) if unreplaced => {}
            Status::Exited(0) => {
                let mut said = false;
                for directive in asked {
                    said = true;
                    match directive {
                        Directive::Message(words) => {
                            self.toast(crate::escape_controls(&words), Level::Message);
                        }
                        Directive::Error(words) => {
                            self.toast(crate::escape_controls(&words), Level::Error);
                        }
                        Directive::Open(path) => {
                            match self.open_made(&path, cwd.as_deref(), replacing.as_ref()) {
                                Ok(opened) => effect = effect.also(opened),
                                Err(why) => {
                                    self.toast(format!("{label}: {why}"), Level::Error);
                                    refused.push(why);
                                }
                            }
                        }
                    }
                }
                if let Some((number, problem)) = problems.first() {
                    self.toast(
                        format!("{label}: result line {number}: {problem}"),
                        Level::Error,
                    );
                } else if !said {
                    self.toast(format!("{label} done."), Level::Message);
                }
                // Said last, over whatever the script said: the original
                // being in the trash, and undo bringing it back, is what
                // has to be read.
                if let Some(replacing) = &replacing {
                    self.said_replaced(&label, &replacing.file);
                }
            }
            Status::Exited(code) => {
                // A script that said what went wrong is taken at its word.
                let said = asked
                    .into_iter()
                    .rev()
                    .find_map(|directive| match directive {
                        Directive::Error(words) => Some(crate::escape_controls(&words)),
                        _ => None,
                    });
                let words = said.unwrap_or_else(|| {
                    let why = first_error.unwrap_or_else(|| format!("exit {code}"));
                    format!("{label} failed: {why}")
                });
                self.toast(words, Level::Error);
            }
        }
        if let Some(replacing) = replacing {
            let _ = std::fs::remove_dir_all(replacing.dir);
        }
        let reveal = reveal || !refused.is_empty();
        lines.extend(refused.into_iter().map(|why| Line {
            text: why,
            error: true,
        }));
        self.commands.keep(
            finished.about.clone(),
            Run {
                label,
                took: finished.took,
                ended,
                lines,
                logged,
            },
            finished.dir.map(|dir| dir.join("stdout")),
        );
        if reveal {
            effect = effect.also(self.reveal_info(ui::tags::Tab::Output));
        }
        effect
    }

    /// An `open` line: the image the script made, shown in the window — the
    /// file it is already on the list as, or a newcomer beside the file on
    /// screen, as an export joins it, where it is one the window reads.
    /// What is wrong with it otherwise, for the caller to say.
    ///
    /// `scratch` is where a run that replaced its file wrote, which is
    /// taken away once the run is over. Its output is the file it replaced
    /// by now, so an `open` of `GAMUT_OUTPUT` is an `open` of that; anything
    /// else in there is refused, since it would join the list only to be
    /// gone from under it a moment later.
    fn open_made(
        &mut self,
        path: &str,
        cwd: Option<&Path>,
        scratch: Option<&Replacing>,
    ) -> Result<Effect, String> {
        let path = expand_home(path);
        let path = match cwd {
            Some(cwd) if path.is_relative() => cwd.join(path),
            _ => path,
        };
        let name = super::edits::name_of(&path);
        let path = match scratch {
            Some(scratch) if lexically(&path) == lexically(&scratch.output) => scratch.file.clone(),
            Some(scratch) if lexically(&path).starts_with(lexically(&scratch.dir)) => {
                return Err(format!(
                    "{name} was left in the folder made for its output, \
                     which is taken away when it ends."
                ));
            }
            _ => path,
        };
        if !path.is_file() {
            return Err(format!("no file at {}", crate::shown_path(&path)));
        }
        let absolute = std::path::absolute(&path).unwrap_or_else(|_| path.clone());
        let listed = self.files.position(&path).or_else(|| {
            self.files
                .paths()
                .iter()
                .position(|each| std::path::absolute(each).is_ok_and(|each| each == absolute))
        });
        if listed.is_none() && crate::image::decode::probe(&path).is_err() {
            return Err(format!("{name} can't be read."));
        }
        self.from_command_line = false;
        let request = match listed {
            Some(index) => self.files.go_to(index),
            None => {
                let request = self.files.adopt(path, Source::Disk);
                self.list_changed();
                request
            }
        };
        Ok(self.send(request).also(Effect::Redraw))
    }

    /// The Output tab's copy: the whole of the last run's output on the
    /// file on screen.
    pub(super) fn copy_output(&mut self) -> Effect {
        let file = self.shown_path().filter(|_| self.current.is_some());
        let Some(run) = self.commands.run_on(file.as_deref()) else {
            return Effect::Nothing;
        };
        self.copy(run.text().as_bytes(), "text/plain", COPIED_OUTPUT);
        Effect::Redraw
    }

    /// A line of the Output tab, copied.
    pub(super) fn copy_output_line(&mut self, row: usize) -> Effect {
        let file = self.shown_path().filter(|_| self.current.is_some());
        let Some(run) = self.commands.run_on(file.as_deref()) else {
            return Effect::Nothing;
        };
        let Some(line) = run.lines.get(row) else {
            return Effect::Nothing;
        };
        self.copy(line.text.as_bytes(), "text/plain", COPIED_LINE);
        Effect::Redraw
    }

    /// The Output tab's log button: the last run's output, as it was
    /// written to disk, opened in the editor the configuration file opens
    /// in.
    pub(super) fn edit_output(&mut self) -> Effect {
        let file = self.shown_path().filter(|_| self.current.is_some());
        let Some(log) = self.commands.log_of(file.as_deref()) else {
            return Effect::Nothing;
        };
        if let Err(error) = crate::openers::edit(&log) {
            crate::report(&error);
            self.toast(super::input::briefly(&error), Level::Error);
            return Effect::Redraw;
        }
        Effect::Nothing
    }
}

/// What the toast says once the Output tab's output is copied, and a line
/// of it.
const COPIED_OUTPUT: &str = "Copied output.";
const COPIED_LINE: &str = "Copied line.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_picture_sees_more_across_than_down() {
        let [across, down, diagonal] = fields_of_view(24.0, 6000.0, 4000.0);
        assert!((across - 73.74).abs() < 0.05, "{across}");
        assert!((down - 53.13).abs() < 0.05, "{down}");
        assert!((diagonal - 84.06).abs() < 0.05, "{diagonal}");
    }

    #[test]
    fn a_moment_is_written_with_the_offset_of_its_clock() {
        let wall = crate::clock::Civil {
            year: 2024,
            month: 3,
            day: 9,
            hour: 14,
            minute: 5,
            second: 7,
        };
        let moment = crate::clock::moment(wall, Some(-7 * 3600)).unwrap();
        assert_eq!(iso_8601(moment, wall), "2024-03-09T14:05:07-07:00");
        let moment = crate::clock::moment(wall, Some(5 * 3600 + 1800)).unwrap();
        assert_eq!(iso_8601(moment, wall), "2024-03-09T14:05:07+05:30");
    }

    #[test]
    fn a_home_is_read_in_a_path() {
        assert_eq!(expand_home("/abs"), PathBuf::from("/abs"));
        if let Some(home) = crate::xdg::home() {
            assert_eq!(expand_home("~/x.png"), home.join("x.png"));
        }
    }
}
