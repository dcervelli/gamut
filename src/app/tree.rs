//! A directory named — on the command line, in the desktop's dialog, or
//! the folder beside a single file — as the application holds it.
//!
//! The reading and the watching are the walker thread's — see
//! [`super::walker`]; what is here is what arrives from it, taken into the
//! list between reads as every change to the list is, since a read in
//! flight is aimed at an index. The first images a walk finds are opened
//! onto the moment they land, where nothing is on screen or the directory
//! was chosen in the window, so that the picture is up while the walk goes
//! on; the rest join the list in batches, put in the list's order as they
//! do, and never move the screen.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::time::Instant;

use super::App;
use super::files::SLOW_READ;
use super::input::Effect;
use super::walker::{Ask, Event, Walker};
use crate::loader::Source;
use crate::ui::toast::{Level, Toast};

/// Where a directory was named, which is where a walk that comes to
/// nothing is said: the terminal for the command line, which is answered
/// by leaving when nothing else is coming, and the window for a choice
/// made in it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Origin {
    CommandLine,
    Window,
}

/// The walks as the application holds them.
pub(super) struct Trees {
    pub(super) walker: Walker,
    /// What the thread has said that a read in flight kept from the list.
    landed: VecDeque<Event>,
    /// The walks still going, and where each was named.
    roots: HashMap<PathBuf, Origin>,
    /// When a walk started with nothing on the list to open the window
    /// on: the window waits up to [`SLOW_READ`] after it for the first
    /// image found, to open at its size.
    opening: Option<Instant>,
    /// Whether the next images found are to be gone to: a folder chosen
    /// in the window is opened onto, as a file chosen is, rather than
    /// quietly added to the end of the list behind the picture up.
    go_to_found: bool,
    /// When the walks in flight began, and how many images the walking had
    /// found by then: what the toast saying they are still going counts
    /// from.
    since: Option<(Instant, usize)>,
    /// Whether the folder dialog's box for subfolders was last left
    /// ticked, which is how it is put up the next time.
    pub(super) include_subfolders: bool,
}

impl Trees {
    pub(super) fn new(walker: Walker, include_subfolders: bool) -> Self {
        Self {
            walker,
            landed: VecDeque::new(),
            roots: HashMap::new(),
            opening: None,
            go_to_found: false,
            since: None,
            include_subfolders,
        }
    }
}

impl App {
    /// Has `root` walked for its images — the directory alone, or with
    /// `recursive` its whole subtree — and watched while the program runs.
    pub(super) fn start_tree(&mut self, root: PathBuf, recursive: bool, origin: Origin) {
        if self.trees.roots.contains_key(&root) {
            return;
        }
        if self.trees.roots.is_empty() {
            self.trees.since = Some((Instant::now(), self.trees.walker.progress().found()));
        }
        if self.files.len() == 0 && self.current.is_none() {
            self.trees.opening = Some(Instant::now());
        }
        if origin == Origin::Window {
            self.trees.go_to_found = true;
        }
        self.trees.roots.insert(root.clone(), origin);
        self.trees.walker.ask(Ask::Walk {
            root,
            recursive,
            sort: self.filmstrip.order().sort,
        });
    }

    /// Whether a walk is still going: what keeps an empty window from
    /// offering its buttons, and a command line that has shown nothing yet
    /// from being given up on.
    pub(super) fn walking(&self) -> bool {
        !self.trees.roots.is_empty()
    }

    /// Takes in what the walker said, which is acted on at the first chance.
    pub(super) fn tree_event(&mut self, event: Event) -> Effect {
        self.trees.landed.push_back(event);
        self.settle_trees()
    }

    /// Acts on everything the walker has said. Between reads only, as any
    /// change to the list is: a read in flight is aimed at an index.
    pub(super) fn settle_trees(&mut self) -> Effect {
        if self.trees.landed.is_empty() || !self.files.is_idle() {
            return Effect::Nothing;
        }
        // In the order they were said, and only for as long as nothing is
        // being read: the first images found may have just been asked for,
        // and what follows them waits for the read to be over.
        let mut effect = Effect::Nothing;
        while self.files.is_idle()
            && let Some(event) = self.trees.landed.pop_front()
        {
            effect = effect.also(self.take_tree_event(event));
        }
        // The command line named nothing but directories with nothing in
        // them: nothing is coming, and the window is not worth keeping.
        if self.showed_nothing() && self.files.is_idle() && !self.walking() {
            return Effect::Quit;
        }
        effect
    }

    fn take_tree_event(&mut self, event: Event) -> Effect {
        match event {
            Event::Found { images, glimpses } => {
                self.glimpsed.extend(glimpses);
                // Walked because it was named, it is named: what was taken
                // off the list in it comes back, as a file named does.
                self.files.unhide(&images);
                let new = self.files.add(images);
                if new.is_empty() {
                    return Effect::Nothing;
                }
                self.list_grew(new.clone());
                let order = self.apply_order();
                if self.current.is_none() || std::mem::take(&mut self.trees.go_to_found) {
                    self.open_found(&new);
                }
                order.also(Effect::Redraw)
            }
            Event::Changed {
                added,
                glimpses,
                removed,
                modified,
            } => {
                self.glimpsed.extend(glimpses);
                // A file said to have gone that is there now was caught
                // between its leaving and its arriving again: replaced by a
                // program that removes a file before it writes the new one,
                // as a command's output takes a file's place on a Mac.
                let gone: HashSet<PathBuf> = removed
                    .into_iter()
                    .filter(|path| std::fs::symlink_metadata(path).is_err())
                    .collect();
                let left = self.files.remove(&gone);
                let new = self.files.add(added);
                let grew = !new.is_empty();
                if grew {
                    self.list_grew(new);
                }
                let rewritten = self.rewritten(modified);
                if !left && !grew && !rewritten {
                    return Effect::Nothing;
                }
                self.apply_order().also(Effect::Redraw)
            }
            Event::Done {
                root,
                found,
                capped,
            } => {
                let origin = self.trees.roots.remove(&root);
                self.walk_ended();
                if origin == Some(Origin::Window) && found == 0 {
                    self.trees.go_to_found = false;
                }
                if found == 0 && !capped {
                    self.said_empty(&root, origin);
                }
                Effect::Redraw
            }
            Event::Failed { root, error } => {
                let origin = self.trees.roots.remove(&root);
                self.walk_ended();
                crate::report(&error);
                if origin == Some(Origin::Window) {
                    let name = super::folder::name(&root);
                    self.toast(format!("Could not read {name}"), Level::Warning);
                }
                Effect::Redraw
            }
            Event::Capped { cap } => {
                self.toast(format!("Stopped at {cap} images"), Level::Warning);
                Effect::Redraw
            }
        }
    }

    /// Asks for the first in the list's order of `new`, the first images a
    /// walk has found. With nothing on screen it is asked for as the
    /// opening of a list is — as a walk past any that will not decode —
    /// and the window sized for it; with a picture up, it is gone to as a
    /// file chosen is, the picture staying until it arrives.
    fn open_found(&mut self, new: &[PathBuf]) {
        self.trees.go_to_found = false;
        let Some(first) = new
            .iter()
            .filter_map(|path| self.files.position(path))
            .min()
        else {
            return;
        };
        let request = match self.current {
            None => {
                self.files.start_at(first);
                self.files.open_first(Source::Disk)
            }
            Some(_) => self.files.go_to(first),
        };
        self.open_at(&request.path);
        let _ = self.send(request);
        self.trees.opening = None;
    }

    /// A walk is over. Once the last is, the window has nothing more to
    /// wait for, and the count starts again from the next.
    fn walk_ended(&mut self) {
        if self.trees.roots.is_empty() {
            self.trees.opening = None;
            self.trees.since = None;
        }
    }

    /// A directory that held no images: said where it was named.
    fn said_empty(&mut self, root: &std::path::Path, origin: Option<Origin>) {
        match origin {
            Some(Origin::Window) => {
                let name = super::folder::name(root);
                self.toast(format!("No images in {name}"), Level::Warning);
            }
            Some(Origin::CommandLine) => {
                eprintln!("gamut: no images in {}", crate::shown_path(root));
            }
            None => {}
        }
    }

    /// The list has grown by `new`, which the thumbnail thread is told of
    /// — the newcomers alone, rather than the whole list again — and which
    /// may have put the list out of its order.
    pub(super) fn list_grew(&mut self, new: Vec<PathBuf>) {
        self.filmstrip.mark_stale();
        self.thumbnailer.enqueue(new);
    }

    /// Files on the list written again since they arrived — read while
    /// their writer was still going, as often as not — which the thumbnail
    /// thread is asked for again, header and all, whatever it made of them
    /// before. Says whether any was on the list.
    pub(super) fn rewritten(&mut self, modified: Vec<PathBuf>) -> bool {
        let modified: Vec<PathBuf> = modified
            .into_iter()
            .filter(|path| self.files.position(path).is_some())
            .collect();
        if modified.is_empty() {
            return false;
        }
        for path in &modified {
            self.chooser.forget_failure(path);
        }
        // What it is sorted by may have changed with it.
        if self.filmstrip.order().reads_facts() {
            self.filmstrip.mark_stale();
        }
        self.thumbnailer.prioritize(modified);
        true
    }

    /// Tells the walker which folder the file on screen is in, which it
    /// then looks at every tick.
    pub(super) fn tell_walker_shown(&self) {
        self.trees.walker.ask(Ask::Shown(self.last_folder.clone()));
    }

    /// When the window stops waiting for a walk to find it something to
    /// open on.
    pub(super) fn tree_opening_due(&self) -> Option<Instant> {
        self.trees.opening.map(|since| since + SLOW_READ)
    }

    /// The toast about a walk still going, once it has taken as long as a
    /// file does before its wait is said, with how many images it has
    /// found so far.
    pub(super) fn finding_toast(&self) -> Option<Toast> {
        let (since, from) = self.trees.since?;
        let raised = since + SLOW_READ;
        if Instant::now() < raised {
            return None;
        }
        let found = self.trees.walker.progress().found().saturating_sub(from);
        let message = match found {
            0 => "Finding images\u{2026}".to_string(),
            found => format!("Finding images\u{2026} {found} so far"),
        };
        Some(Toast::waiting(message, raised))
    }

    /// When the toast about a walk goes up, while that is still to come.
    pub(super) fn finding_due(&self, now: Instant) -> Option<Instant> {
        let due = self.trees.since?.0 + SLOW_READ;
        (due > now).then_some(due)
    }

    /// A frame while the toast about a walk is up: the count in it moves on
    /// between one look and the next.
    pub(super) fn finding_counts(&self) -> bool {
        self.finding_toast().is_some()
    }
}
