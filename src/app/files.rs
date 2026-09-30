//! The list of files, which of them is on screen, and the read in flight.
//!
//! A state machine and nothing else: it hands back the [`Request`] each move
//! calls for and never sends one, so it needs no loader, no window and no
//! disk, and can be driven through a whole walk in a test.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::image::decode;
use crate::image::decode::Rendering;
use crate::loader::{Reload, Request, Source};

/// How long a file may take to open before the window says so — the toast,
/// the thumbnail standing in for it, the panels waiting for it. Long enough
/// that the ordinary case — a file that opens within a few frames — never
/// flickers any of that into the window and out again.
pub(super) const SLOW_READ: Duration = Duration::from_millis(150);

/// What [`Files::announce_slow_read`] found: a read to say something about now,
/// one to look at again at a given moment, or nothing worth a word.
#[derive(PartialEq, Eq, Debug)]
pub(super) enum Announce {
    Now,
    Waiting(Instant),
    Nothing,
}

/// A read that has been asked for and not yet answered.
pub(super) struct Pending {
    /// Which request it is, so that a reply arriving after the user has moved
    /// on can be recognized and dropped.
    pub(super) generation: u64,
    pub(super) index: usize,
    /// Set when the request came from `]` or `[`, so that a file that will not
    /// decode can be stepped over rather than stopping the walk.
    pub(super) step: Option<Step>,
    /// Which page of the file, where one was asked for by number.
    pub(super) page: Option<usize>,
    pub(super) since: Instant,
    /// When the wait began to be mentioned, if it has. Latched so that the
    /// wait is announced once rather than on every frame it spans, and handed
    /// on to a read that replaces this one, so that the toast saying so stays
    /// up through a walk rather than going and coming back with each step.
    pub(super) announced: Option<Instant>,
}

/// A walk through the file list, carried along so that it can continue past a
/// file that fails to decode.
#[derive(Clone, Copy)]
pub(super) struct Step {
    forward: bool,
    /// How many further files this walk may ask for if the one in flight
    /// fails. Counted down rather than up because the two walks start with
    /// different budgets: stepping has the rest of the list to try, while the
    /// walk that opens the first file has the whole of it.
    remaining: usize,
}

/// The list can be empty: the program opened on nothing, and is waiting
/// for the window to be handed something. Then there is no file on screen
/// for [`Files::shown_path`] to name and no index worth reading, and the
/// first thing that fills the list — [`Files::append`] for what the
/// desktop's dialog chose, [`Files::adopt`] for a paste — is what starts
/// the first read.
pub(super) struct Files {
    paths: Vec<PathBuf>,
    /// Where each path stands in `paths` — the first place, should a path
    /// stand twice — built again by [`Files::reindex`] whenever the list
    /// changes, so that asking where a file is costs nothing per file.
    places: HashMap<PathBuf, usize>,
    /// Files written while the program was running — a picture pasted from
    /// the clipboard — which live where pictures are kept rather than
    /// wherever we were told to look. No directory named on the command line
    /// accounts for one, so [`Files::relist`] has to put them back itself.
    adopted: Vec<PathBuf>,
    /// The file on screen.
    index: usize,
    /// The file on screen once it has been moved to the trash, while it is
    /// still on screen. It stays on the list until another file has taken
    /// the screen from it — the picture is still what is being looked at,
    /// and the list still has to say which file that is — and leaves the
    /// list in [`Files::shown`], the moment it does. `None` otherwise.
    leaving: Option<PathBuf>,
    /// Files taken off the list this session with nothing done to them on
    /// disk. A directory read again would list each of them as before, and
    /// [`Files::relist`] leaves them out; opening one by name again is what
    /// puts it back.
    hidden: HashSet<PathBuf>,
    overrides: decode::Overrides,
    /// Numbers the requests. Only the newest one's reply is acted on.
    generation: u64,
    pending: Option<Pending>,
}

impl Files {
    /// `index` is the file to open first: the first one whose header could be
    /// read, which is not necessarily the first one named. An empty list is
    /// a program opened on nothing, and `index` is then nothing either.
    pub(super) fn new(paths: Vec<PathBuf>, index: usize, overrides: decode::Overrides) -> Self {
        let mut files = Self {
            paths,
            places: HashMap::new(),
            adopted: Vec::new(),
            index,
            leaving: None,
            hidden: HashSet::new(),
            overrides,
            generation: 0,
            pending: None,
        };
        files.reindex();
        files
    }

    /// Builds `places` from `paths` again: called by everything that
    /// changes the list, once it has.
    fn reindex(&mut self) {
        self.places.clear();
        for (index, path) in self.paths.iter().enumerate() {
            self.places.entry(path.clone()).or_insert(index);
        }
    }

    pub(super) fn len(&self) -> usize {
        self.paths.len()
    }

    /// Which file is on screen. Meaningless on an empty list.
    pub(super) fn index(&self) -> usize {
        self.index
    }

    pub(super) fn path(&self, index: usize) -> &Path {
        &self.paths[index]
    }

    /// The file on screen — or the one being asked for first, before
    /// anything is — and `None` on an empty list.
    pub(super) fn shown_path(&self) -> Option<&Path> {
        self.paths.get(self.index).map(PathBuf::as_path)
    }

    /// The file last asked for: the one being read, or the one on screen
    /// where nothing is. What the list's own readouts follow — the count,
    /// the file list's highlight, the name — so that they move with the key
    /// rather than waiting on the decode, and go back to the file on screen
    /// by themselves when a read fails.
    pub(super) fn target(&self) -> usize {
        self.pending
            .as_ref()
            .map_or(self.index, |pending| pending.index)
    }

    /// The path of [`Files::target`], and `None` on an empty list.
    pub(super) fn target_path(&self) -> Option<&Path> {
        self.paths.get(self.target()).map(PathBuf::as_path)
    }

    /// The whole list, in the order it is walked.
    pub(super) fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// Where `path` stands in the list, if it is on it.
    pub(super) fn position(&self, path: &Path) -> Option<usize> {
        self.places.get(path).copied()
    }

    /// Straight to the file at `index`: what the chooser asks for. Not a
    /// walk, since one particular file was named; and not held back by a
    /// read in flight, as `adopt` is not — a pick wins over whatever step
    /// was on its way.
    pub(super) fn go_to(&mut self, index: usize) -> Request {
        self.request(index, Reload::Fresh, None, Source::Disk)
    }

    pub(super) fn overrides(&self) -> decode::Overrides {
        self.overrides
    }

    /// The read that has been asked for and not yet answered, if any.
    pub(super) fn pending(&self) -> Option<&Pending> {
        self.pending.as_ref()
    }

    /// Whether nothing is being read.
    pub(super) fn is_idle(&self) -> bool {
        self.pending.is_none()
    }

    /// The opening request. As a walk, so that a file which passes the header
    /// check and then fails to decode is stepped over exactly as `]` would
    /// step over it. Nothing is on screen yet, so every file in the list is a
    /// candidate.
    ///
    /// `source` is where the first file's bytes come from. `--paste` puts a
    /// file at the head of the list that the clipboard is still to fill, and
    /// it is kept as a paste made later would be — through a relist, which no
    /// named directory would otherwise put it back into. A paste that will
    /// not arrive is walked past like a file that will not decode: the rest
    /// of the list was asked for too.
    pub(super) fn open_first(&mut self, source: Source) -> Request {
        if let Source::Clipboard(_) = source {
            self.adopted.push(self.paths[self.index].clone());
        }
        let remaining = self.paths.len() - 1;
        self.request(
            self.index,
            Reload::Fresh,
            Some(Step {
                forward: true,
                remaining,
            }),
            source,
        )
    }

    /// Makes `index` the file the opening request asks for, before anything
    /// has been asked: the list having been put in order under it.
    pub(super) fn start_at(&mut self, index: usize) {
        debug_assert!(self.is_idle(), "nothing asked for yet");
        self.index = index;
    }

    /// Moves to the next or previous file. `None` when there is nowhere to go.
    ///
    /// From wherever the last request was aimed rather than from what is on
    /// screen, so that holding `]` walks the list instead of asking for the
    /// same neighbor over and over while a slow file opens. Only the last of
    /// those requests is decoded; the ones passed over are files the user has
    /// already scrolled past.
    pub(super) fn step(&mut self, forward: bool) -> Option<Request> {
        if self.paths.len() < 2 {
            return None;
        }
        let from = self
            .pending
            .as_ref()
            .map_or(self.index, |pending| pending.index);
        let next = self.neighbor(from, forward);
        Some(self.request(
            next,
            Reload::Fresh,
            Some(Step {
                forward,
                // Everything but the file being asked for and the one already
                // on screen.
                remaining: self.paths.len() - 2,
            }),
            Source::Disk,
        ))
    }

    /// Re-reads the file on screen. `None` while a read is already in
    /// flight: a file being written continuously would otherwise stack up a
    /// decode every interval, and the reply already on its way carries a
    /// watch taken later than this one anyway.
    pub(super) fn reload(&mut self) -> Option<Request> {
        if self.pending.is_some() || self.paths.is_empty() {
            return None;
        }
        Some(self.request(self.index, Reload::InPlace, None, Source::Disk))
    }

    /// Asks for another page of the file on screen. `None` while a read is
    /// in flight, as for a reload: a key held down would otherwise stack up
    /// a decode per repeat, each aimed at a page the next has moved past.
    pub(super) fn page(&mut self, page: usize) -> Option<Request> {
        if self.pending.is_some() || self.paths.is_empty() {
            return None;
        }
        let mut request = self.request(self.index, Reload::Page, None, Source::Disk);
        request.page = Some(page);
        if let Some(pending) = &mut self.pending {
            pending.page = Some(page);
        }
        Some(request)
    }

    /// Asks for the file on screen again in its other rendering — which one
    /// is `App::send`'s to fill in. `None` while a read is in flight, as for
    /// a reload: that read may be a step, which this would otherwise cancel.
    /// The reply to it says which rendering it was asked for, and
    /// `App::deliver` asks again if the preference has moved since.
    pub(super) fn rerender(&mut self) -> Option<Request> {
        if self.pending.is_some() || self.paths.is_empty() {
            return None;
        }
        Some(self.request(self.index, Reload::Rendering, None, Source::Disk))
    }

    /// Takes in a file that did not exist when the list was made — a picture
    /// pasted from the clipboard, whose bytes the loader fetches on its way
    /// to reading it — and asks for it.
    ///
    /// It goes in beside the file on screen rather than at the end of the
    /// list: the list is what `]` and `[` walk, and what was just pasted
    /// belongs next to where the user is rather than past every file they
    /// have not looked at yet.
    ///
    /// Not a walk. A file that will not decode is stepped over when the user
    /// was going somewhere, but a paste is one particular picture that was
    /// asked for, and wandering off to a neighbor instead would answer a
    /// question nobody put.
    pub(super) fn adopt(&mut self, path: PathBuf, source: Source) -> Request {
        // Beside the file on screen, or at the head of a list with nothing
        // on it yet.
        let at = match self.paths.is_empty() {
            true => 0,
            false => self.index + 1,
        };
        self.hidden.remove(&path);
        self.paths.insert(at, path.clone());
        self.adopted.push(path);
        self.reindex();
        self.request(at, Reload::Fresh, None, source)
    }

    /// Adds `paths` to the end of the list — what the desktop's dialog
    /// chose, joining whatever was named before it — and asks for the
    /// first of the newcomers as a walk over them, so that one that will
    /// not decode is stepped over as it is at start-up. A path already on
    /// the list is not added again; where none is new, the first of them
    /// is asked for by name instead, unless it is the file on screen, in
    /// which case there is nothing to do and the answer is `None`.
    ///
    /// At the end rather than beside the file on screen, as a paste goes:
    /// a paste is one picture that belongs next to where the user is, and
    /// this is a set of files that keeps the order it was chosen in.
    pub(super) fn append(&mut self, paths: Vec<PathBuf>) -> Option<Request> {
        let first = paths.first()?.clone();
        let fresh: Vec<PathBuf> = paths
            .into_iter()
            .filter(|path| !self.places.contains_key(path))
            .collect();
        if fresh.is_empty() {
            let at = self.position(&first)?;
            return (self.shown_path() != Some(first.as_path())).then(|| self.go_to(at));
        }
        let at = self.paths.len();
        let remaining = fresh.len() - 1;
        for path in &fresh {
            self.hidden.remove(path);
        }
        self.paths.extend(fresh);
        self.reindex();
        Some(self.request(
            at,
            Reload::Fresh,
            Some(Step {
                forward: true,
                remaining,
            }),
            Source::Disk,
        ))
    }

    /// The step a deletion takes away from the file on screen: on to the
    /// next, or back to the previous from the last of the list — the walk
    /// that was being made, rather than a wrap round to the first. `None`
    /// with nowhere to go.
    pub(super) fn step_away(&mut self) -> Option<Request> {
        let forward = self.index + 1 < self.paths.len();
        self.step(forward)
    }

    /// Notes that the file on screen has been moved to the trash. It stays
    /// on the list until another file arrives — see [`Files::leaving`].
    pub(super) fn condemn(&mut self) {
        self.leaving = Some(self.paths[self.index].clone());
    }

    /// Takes the file on screen off the list, leaving it as it is on disk.
    /// It goes out through the door a trashed file does — it stays until
    /// another file has the screen — and is remembered, so that the
    /// directory being read again does not bring it back.
    pub(super) fn hide(&mut self) {
        self.hidden.insert(self.paths[self.index].clone());
        self.condemn();
    }

    /// Whether `path` was taken off the list with nothing done to it on
    /// disk — whether it has left yet or is still on screen.
    pub(super) fn is_hidden(&self, path: &Path) -> bool {
        self.hidden.contains(path)
    }

    /// Whether the file on screen is one that has been moved to the trash
    /// and not yet left the list.
    pub(super) fn is_condemned(&self, path: &Path) -> bool {
        self.leaving.as_deref() == Some(path)
    }

    /// The file that was on its way out is back: it stays on the list
    /// after all, and a rebuild keeps it as it keeps any other.
    pub(super) fn reprieve(&mut self) {
        if let Some(leaving) = self.leaving.take() {
            self.hidden.remove(&leaving);
        }
    }

    /// The file on screen leaves the list now, with nothing to take the
    /// screen from it: the last file deleted, after which the list is
    /// empty and the window shows nothing.
    pub(super) fn remove_shown(&mut self) {
        if let Some(path) = self.shown_path().map(Path::to_path_buf) {
            self.drop_path(&path);
        }
        self.leaving = None;
    }

    /// Whether `path` is a file taken in while the program ran, which a
    /// rebuild of the list keeps — what a file put back after a deletion
    /// has to be again.
    pub(super) fn is_adopted(&self, path: &Path) -> bool {
        self.adopted.iter().any(|held| held == path)
    }

    /// Puts a file back on the list that had left it — one restored from
    /// the trash — at `index`, or at the end where the list has grown
    /// shorter than that, and asks for it. `adopted` is whether a rebuild
    /// of the list should keep it, as it was kept before.
    pub(super) fn reinstate(&mut self, path: PathBuf, index: usize, adopted: bool) -> Request {
        let at = index.min(self.paths.len());
        // The file on screen moves along when the newcomer goes in ahead of
        // it — where there is one: an empty list has nothing on screen to
        // move, and the newcomer is what the list now holds.
        let shifts = !self.paths.is_empty() && at <= self.index;
        self.hidden.remove(&path);
        self.paths.insert(at, path.clone());
        self.reindex();
        if shifts {
            self.index += 1;
        }
        if let Some(pending) = &mut self.pending
            && at <= pending.index
        {
            pending.index += 1;
        }
        if adopted {
            self.adopted.push(path);
        }
        self.go_to(at)
    }

    /// The file at `index` is called `to` now. Its place in the list is
    /// kept: the list is rebuilt from the directory by name in its own
    /// time, where a directory is what was named, and a file named on the
    /// command line stays where the command line put it.
    pub(super) fn rename(&mut self, index: usize, to: PathBuf) {
        let from = std::mem::replace(&mut self.paths[index], to.clone());
        self.reindex();
        for held in &mut self.adopted {
            if *held == from {
                *held = to.clone();
            }
        }
        if self.leaving.as_deref() == Some(from.as_path()) {
            self.leaving = Some(to.clone());
        }
        if self.hidden.remove(&from) {
            self.hidden.insert(to);
        }
    }

    /// Takes `path` off the list, wherever it is, keeping the file on
    /// screen and a read in flight aimed where they were.
    fn drop_path(&mut self, path: &Path) {
        let Some(at) = self.position(path) else {
            return;
        };
        self.paths.remove(at);
        self.reindex();
        self.adopted.retain(|held| held != path);
        if at < self.index {
            self.index -= 1;
        }
        if let Some(pending) = &mut self.pending {
            if pending.index == at {
                // A read of the file that has gone: its reply is of nothing
                // on the list, and is dropped.
                self.pending = None;
            } else if at < pending.index {
                pending.index -= 1;
            }
        }
    }

    fn request(
        &mut self,
        index: usize,
        mode: Reload,
        step: Option<Step>,
        source: Source,
    ) -> Request {
        self.generation += 1;
        let announced = self.pending.as_ref().and_then(|pending| pending.announced);
        self.pending = Some(Pending {
            generation: self.generation,
            index,
            step,
            page: None,
            since: Instant::now(),
            announced,
        });
        Request {
            generation: self.generation,
            index,
            path: self.paths[index].clone(),
            overrides: self.overrides,
            mode,
            source,
            page: None,
            // Filled in by `App::send` from the viewer's preference, as the
            // page is from where the file was left.
            rendering: Rendering::Developed,
        }
    }

    /// Notes that the read in flight was aimed at a page by number: a file
    /// coming back to the page it was left on.
    pub(super) fn asked_for_page(&mut self, page: usize) {
        if let Some(pending) = &mut self.pending {
            pending.page = Some(page);
        }
    }

    /// Takes in a reply. Returns the request it answers, or `None` for one
    /// the user has stepped past while it was being read: its pixels are
    /// correct and unwanted.
    pub(super) fn accept(&mut self, generation: u64) -> Option<Pending> {
        self.pending
            .take_if(|pending| pending.generation == generation)
    }

    /// Takes in a list read again from the directories the command line
    /// named. Returns whether it differs from the one already held, which is
    /// what the bar's count and the file's place in it depend on.
    ///
    /// The list keeps the order it stands in: the rebuild says what is on
    /// it, not where. A directory is read in name order, and the list may
    /// have been sorted some other way since — by size, by type — with the
    /// files one sort cannot tell apart left in the order the sort before
    /// put them, so that sorts compose. Taking the rebuild's order would
    /// throw that away on every read. So every file the rebuild still lists
    /// stays where it is, and a newcomer goes in after the nearest file
    /// that comes before it in the rebuild, which for a list in name order
    /// is exactly where the directory has it.
    ///
    /// Two kinds of file survive a rebuild that does not mention them. The
    /// file on screen stays wherever it has gone: its pixels are up and
    /// correct, and dropping the path they came from would leave the title,
    /// the bars and the information panel describing a file that is not the
    /// one being shown. A file that was pasted stays because no directory
    /// named on the command line was ever going to list it — it was written
    /// where pictures are kept — and a rebuild is no reason for a picture the
    /// user made this session to fall out of the walk. Each keeps its place
    /// among its neighbors, so that `]` lands on whatever has taken its
    /// position rather than on a file already seen; a newcomer passes one
    /// of them by name, the order the directory itself was read in.
    ///
    /// A file taken off the list this session is left out however often
    /// the directory lists it.
    ///
    /// Between reads only: this moves the file on screen to a new index, and a
    /// request in flight is aimed at the old one.
    pub(super) fn relist(&mut self, mut paths: Vec<PathBuf>) -> bool {
        debug_assert!(self.is_idle(), "the list is rebuilt between reads");
        paths.retain(|path| !self.hidden.contains(path));
        // Nothing to keep from an empty list: it is what the rebuild says.
        if self.paths.is_empty() {
            let changed = !paths.is_empty();
            self.paths = paths;
            self.reindex();
            return changed;
        }
        let listed: HashSet<&Path> = paths.iter().map(PathBuf::as_path).collect();
        let mut merged: Vec<PathBuf> = self
            .paths
            .iter()
            .enumerate()
            .filter(|(index, path)| {
                *index == self.index
                    || self.adopted.contains(path)
                    || listed.contains(path.as_path())
            })
            .map(|(_, path)| path.clone())
            .collect();
        // Where the next newcomer goes: just after the last file of the
        // rebuild that the list already holds.
        let mut cursor = 0;
        for path in &paths {
            if let Some(at) = merged.iter().position(|held| held == path) {
                cursor = at + 1;
                continue;
            }
            while let Some(kept) = merged.get(cursor)
                && !listed.contains(kept.as_path())
                && kept < path
            {
                cursor += 1;
            }
            merged.insert(cursor, path.clone());
            cursor += 1;
        }

        let shown = &self.paths[self.index];
        self.index = merged
            .iter()
            .position(|path| path == shown)
            .expect("the file on screen was kept whatever the rebuild dropped");
        let changed = merged != self.paths;
        self.paths = merged;
        self.reindex();
        changed
    }

    /// Puts the list in the order `places` gives — each entry the index, as
    /// the list stands, of the file that goes there — keeping the file on
    /// screen the file on screen. Returns whether anything moved.
    ///
    /// Between reads only, as [`Files::relist`] is: a request in flight is
    /// aimed at an index.
    pub(super) fn reorder(&mut self, places: &[usize]) -> bool {
        debug_assert!(self.is_idle(), "the list is reordered between reads");
        debug_assert_eq!(places.len(), self.paths.len(), "one place per file");
        if places
            .iter()
            .enumerate()
            .all(|(place, &index)| place == index)
        {
            return false;
        }
        let shown = self.shown_path().map(Path::to_path_buf);
        // Moved into their new places rather than copied: a permutation
        // takes each path once.
        let mut held: Vec<Option<PathBuf>> = std::mem::take(&mut self.paths)
            .into_iter()
            .map(Some)
            .collect();
        self.paths = places
            .iter()
            .map(|&index| {
                held[index]
                    .take()
                    .expect("a permutation names each place once")
            })
            .collect();
        self.reindex();
        if let Some(shown) = shown {
            self.index = self
                .position(&shown)
                .expect("every file is somewhere in a permutation of the list");
        }
        true
    }

    /// A reply has reached the screen. A file moved to the trash while it
    /// was on screen leaves the list here, the moment another file has
    /// taken the screen from it.
    pub(super) fn shown(&mut self, index: usize) {
        self.index = index;
        if let Some(leaving) = self.leaving.take() {
            if self.paths[index] == leaving {
                self.leaving = Some(leaving);
            } else {
                self.drop_path(&leaving);
            }
        }
    }

    /// A reply would not go on screen. Carries a walk on past the file, so
    /// that one bad file cannot trap navigation; `None` once it has tried them
    /// all, or when the read was not part of a walk.
    pub(super) fn failed(&mut self, from: usize, step: Option<Step>) -> Option<Request> {
        let step = step?;
        if step.remaining == 0 {
            return None;
        }
        let next = self.neighbor(from, step.forward);
        Some(self.request(
            next,
            Reload::Fresh,
            Some(Step {
                forward: step.forward,
                remaining: step.remaining - 1,
            }),
            Source::Disk,
        ))
    }

    /// The files either side of the one on screen, the next first: where a
    /// step goes from here. None on a list of one, and one on a list of two.
    pub(super) fn neighbors(&self) -> Vec<&Path> {
        if self.paths.len() < 2 {
            return Vec::new();
        }
        let mut near = vec![self.neighbor(self.index, true)];
        near.push(self.neighbor(self.index, false));
        near.dedup();
        near.into_iter().map(|index| self.path(index)).collect()
    }

    fn neighbor(&self, index: usize, forward: bool) -> usize {
        let count = self.paths.len();
        if forward {
            (index + 1) % count
        } else {
            (index + count - 1) % count
        }
    }

    /// Says the read in flight is slow from the start, without waiting
    /// [`SLOW_READ`] to find out: the last file of its kind was.
    pub(super) fn announce_now(&mut self, now: Instant) {
        if let Some(pending) = &mut self.pending {
            pending.announced.get_or_insert(now);
        }
    }

    /// Decides whether a read still in flight has been going long enough to
    /// earn a toast. Latches, so that a wait is announced once
    /// rather than on every frame it spans.
    pub(super) fn announce_slow_read(&mut self, now: Instant) -> Announce {
        let Some(pending) = &mut self.pending else {
            return Announce::Nothing;
        };
        let due = pending.since + SLOW_READ;
        if now < due && pending.announced.is_none() {
            Announce::Waiting(due)
        } else if pending.announced.is_some() {
            Announce::Nothing
        } else {
            pending.announced = Some(due);
            Announce::Now
        }
    }
}

#[cfg(test)]
mod tests;
