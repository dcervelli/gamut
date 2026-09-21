//! The list of files, which of them is on screen, and the read in flight.
//!
//! A state machine and nothing else: it hands back the [`Request`] each move
//! calls for and never sends one, so it needs no loader, no window and no
//! disk, and can be driven through a whole walk in a test.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::image::decode;
use crate::loader::{Reload, Request, Source};

/// How long a file may take to open before the bar says so. Long enough that
/// the ordinary case — a file that opens between two frames — never flickers a
/// word into the interface and out again.
pub(super) const SLOW_READ: Duration = Duration::from_millis(120);

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
    /// Whether the bar has been told to mention it. Latched so that the wait
    /// is announced once rather than on every frame it spans.
    pub(super) announced: bool,
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
        Self {
            paths,
            adopted: Vec::new(),
            index,
            leaving: None,
            overrides,
            generation: 0,
            pending: None,
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

    /// The whole list, in the order it is walked.
    pub(super) fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// Where `path` stands in the list, if it is on it.
    pub(super) fn position(&self, path: &Path) -> Option<usize> {
        self.paths.iter().position(|held| held == path)
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
        self.paths.insert(at, path.clone());
        self.adopted.push(path);
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
            .filter(|path| !self.paths.contains(path))
            .collect();
        if fresh.is_empty() {
            let at = self.position(&first)?;
            return (self.shown_path() != Some(first.as_path())).then(|| self.go_to(at));
        }
        let at = self.paths.len();
        let remaining = fresh.len() - 1;
        self.paths.extend(fresh);
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

    /// Whether the file on screen is one that has been moved to the trash
    /// and not yet left the list.
    pub(super) fn is_condemned(&self, path: &Path) -> bool {
        self.leaving.as_deref() == Some(path)
    }

    /// The file that was moved to the trash is back: it stays on the list
    /// after all.
    pub(super) fn reprieve(&mut self) {
        self.leaving = None;
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
        self.paths.insert(at, path.clone());
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
        for held in &mut self.adopted {
            if *held == from {
                *held = to.clone();
            }
        }
        if self.leaving.as_deref() == Some(from.as_path()) {
            self.leaving = Some(to);
        }
    }

    /// Takes `path` off the list, wherever it is, keeping the file on
    /// screen and a read in flight aimed where they were.
    fn drop_path(&mut self, path: &Path) {
        let Some(at) = self.position(path) else {
            return;
        };
        self.paths.remove(at);
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
        self.pending = Some(Pending {
            generation: self.generation,
            index,
            step,
            page: None,
            since: Instant::now(),
            announced: false,
        });
        Request {
            generation: self.generation,
            index,
            path: self.paths[index].clone(),
            overrides: self.overrides,
            mode,
            source,
            page: None,
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
    /// Two kinds of file survive a rebuild that does not mention them. The
    /// file on screen stays wherever it has gone: its pixels are up and
    /// correct, and dropping the path they came from would leave the title,
    /// the bars and the information panel describing a file that is not the
    /// one being shown. A file that was pasted stays because no directory
    /// named on the command line was ever going to list it — it was written
    /// where pictures are kept — and a rebuild is no reason for a picture the
    /// user made this session to fall out of the walk.
    ///
    /// Each keeps its place among its neighbors, so that `]` lands on
    /// whatever has taken its position rather than on a file already seen.
    ///
    /// Between reads only: this moves the file on screen to a new index, and a
    /// request in flight is aimed at the old one.
    pub(super) fn relist(&mut self, mut paths: Vec<PathBuf>) -> bool {
        debug_assert!(self.is_idle(), "the list is rebuilt between reads");
        // Nothing to keep from an empty list: it is what the rebuild says.
        if self.paths.is_empty() {
            let changed = !paths.is_empty();
            self.paths = paths;
            return changed;
        }
        // In the order they stand in now, so that each is placed against a
        // list the ones before it are already back in.
        let keep: Vec<(usize, PathBuf)> = self
            .paths
            .iter()
            .enumerate()
            .filter(|(index, path)| *index == self.index || self.adopted.contains(path))
            .map(|(index, path)| (index, path.clone()))
            .collect();
        for (was, path) in keep {
            if paths.contains(&path) {
                continue;
            }
            let at = self.place_for(&path, was, &paths);
            paths.insert(at, path);
        }

        let shown = &self.paths[self.index];
        self.index = paths
            .iter()
            .position(|path| path == shown)
            .expect("the file on screen was put back if the rebuild had dropped it");
        let changed = paths != self.paths;
        self.paths = paths;
        changed
    }

    /// Where a file the rebuild did not list goes back into it: after
    /// everything that came before it, before everything that came after,
    /// which is what keeps `]` and `[` going the way the user was going.
    /// `was` is where it stood in the list as it is now.
    ///
    /// A path that was in the old list settles which side it is on by where it
    /// was. One that has only just appeared has no place there to go on, and
    /// settles it by its name — the order the directory itself was read in,
    /// which is the order the rest of the list is in.
    fn place_for(&self, missing: &Path, was: usize, paths: &[PathBuf]) -> usize {
        let previously: HashMap<&Path, usize> = self
            .paths
            .iter()
            .enumerate()
            .map(|(index, path)| (path.as_path(), index))
            .collect();
        paths
            .iter()
            .take_while(|path| match previously.get(path.as_path()) {
                Some(&index) => index < was,
                None => path.as_path() < missing,
            })
            .count()
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

    fn neighbor(&self, index: usize, forward: bool) -> usize {
        let count = self.paths.len();
        if forward {
            (index + 1) % count
        } else {
            (index + count - 1) % count
        }
    }

    /// Decides whether a read still in flight has been going long enough to
    /// earn a word in the bar. Latches, so that a wait is announced once
    /// rather than on every frame it spans.
    pub(super) fn announce_slow_read(&mut self, now: Instant) -> Announce {
        let Some(pending) = &mut self.pending else {
            return Announce::Nothing;
        };
        let due = pending.since + SLOW_READ;
        if now < due {
            Announce::Waiting(due)
        } else if pending.announced {
            Announce::Nothing
        } else {
            pending.announced = true;
            Announce::Now
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(count: usize) -> Files {
        let paths = (0..count)
            .map(|i| PathBuf::from(format!("{i}.png")))
            .collect();
        Files::new(paths, 0, decode::Overrides::default())
    }

    /// Holding `]` through a directory asks for each file in turn without
    /// waiting for the last, and only the file the user stopped on is shown.
    #[test]
    fn a_reply_the_user_has_stepped_past_is_not_accepted() {
        let mut files = list(3);
        let first = files.step(true).expect("three files to step through");
        let second = files.step(true).expect("three files to step through");
        assert_eq!((first.index, second.index), (1, 2));

        assert!(files.accept(first.generation).is_none());
        assert!(!files.is_idle(), "the newer request is still owed a reply");
        assert_eq!(files.accept(second.generation).map(|p| p.index), Some(2));
        assert!(files.is_idle());
    }

    #[test]
    fn stepping_wraps_and_is_not_offered_with_one_file() {
        let mut files = list(2);
        assert_eq!(files.step(false).map(|r| r.index), Some(1));
        assert_eq!(files.step(false).map(|r| r.index), Some(0));
        assert!(list(1).step(true).is_none());
    }

    /// A file that will not decode must not trap navigation, and the walk has
    /// to stop asking once it has been all the way round.
    #[test]
    fn a_walk_carries_on_past_a_failure_and_gives_up_after_the_last() {
        let mut files = list(3);
        let request = files.step(true).expect("three files");
        let pending = files
            .accept(request.generation)
            .expect("the reply we waited for");
        assert_eq!(pending.index, 1);

        let again = files.failed(1, pending.step).expect("one more file to try");
        assert_eq!(again.index, 2);
        let pending = files
            .accept(again.generation)
            .expect("the reply we waited for");
        assert!(
            files.failed(2, pending.step).is_none(),
            "every other file has been tried"
        );
        assert_eq!(files.index(), 0, "nothing new ever reached the screen");

        assert!(files.failed(0, None).is_none(), "a reload is not a walk");
    }

    fn named(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    /// A program opened on nothing has an empty list: nothing to step to,
    /// nothing to reload, nothing on screen to name — and the first thing
    /// handed to it, whether chosen or pasted, goes at the head.
    #[test]
    fn an_empty_list_names_nothing_and_takes_the_first_thing_it_is_given() {
        let mut files = list(0);
        assert_eq!(files.len(), 0);
        assert_eq!(files.shown_path(), None);
        assert!(files.step(true).is_none());
        assert!(files.reload().is_none());
        assert!(files.page(1).is_none());
        assert!(!files.relist(Vec::new()));
        assert!(files.is_idle());

        let pasted = files.adopt(PathBuf::from("pasted.png"), Source::Disk);
        assert_eq!(pasted.index, 0);
        assert_eq!(files.shown_path(), Some(Path::new("pasted.png")));
        assert!(files.is_adopted(Path::new("pasted.png")));

        // What the dialog chose joins the end of the list and is asked for
        // as a walk over the newcomers; the reply to the paste, still on
        // its way, is not a reply to it.
        let chosen = files
            .append(named(&["a.png", "b.png"]))
            .expect("something new");
        assert_eq!(chosen.index, 1);
        assert_ne!(chosen.generation, pasted.generation);
        assert!(files.accept(pasted.generation).is_none());
        let pending = files
            .accept(chosen.generation)
            .expect("the walk's own reply");
        assert!(
            pending.step.is_some(),
            "a walk, so a bad file is stepped over"
        );
        assert_eq!(files.len(), 3);
        assert!(files.is_adopted(Path::new("pasted.png")));
        let again = files.failed(1, pending.step).expect("on to b.png");
        assert_eq!(again.index, 2);
        let pending = files.accept(again.generation).expect("its reply");
        assert!(
            files.failed(2, pending.step).is_none(),
            "the walk is over the newcomers alone"
        );
        files.shown(2);

        // A file already on the list is not added again, and one that is
        // the only thing chosen is gone to rather than added.
        let again = files
            .append(named(&["a.png", "c.png"]))
            .expect("c.png is new");
        assert_eq!(again.index, 3);
        assert_eq!(files.len(), 4);
        files.accept(again.generation);
        let back = files.append(named(&["a.png"])).expect("a.png, by name");
        assert_eq!(back.index, 1);
        let pending = files.accept(back.generation).expect("its reply");
        assert!(pending.step.is_none(), "not a walk");
        files.shown(1);
        assert!(
            files.append(named(&["a.png"])).is_none(),
            "the file on screen is nothing to ask for"
        );
        assert_eq!(files.len(), 4);
    }

    /// A directory read again is a list read again: a file written into it
    /// joins the walk, and the one on screen goes on being the one on screen.
    #[test]
    fn a_file_that_has_appeared_joins_the_list() {
        let mut files = list(2);
        files.shown(1);
        assert!(files.relist(named(&["0.png", "1.png", "2.png"])));
        assert_eq!(files.len(), 3);
        assert_eq!(files.shown_path(), Some(Path::new("1.png")));
        assert_eq!(files.index(), 1);
        assert_eq!(files.step(true).map(|r| r.index), Some(2));

        let mut files = list(2);
        files.shown(1);
        assert!(files.relist(named(&["0.png", "0a.png", "1.png"])));
        assert_eq!(files.index(), 2, "a file arriving before it moves it along");
        assert!(
            !files.relist(named(&["0.png", "0a.png", "1.png"])),
            "a directory that has not changed changes nothing"
        );
    }

    /// The file on screen is never dropped from the list, whatever has become
    /// of it: its pixels are up, and everything the interface says about them
    /// is read off the path they came from. It keeps its neighbors, so that
    /// `]` goes on to what has taken its place rather than back over a file
    /// already seen.
    #[test]
    fn the_file_on_screen_survives_being_deleted_under_us() {
        let mut files = list(4);
        files.shown(1);
        assert!(files.relist(named(&["0.png", "2.png"])));
        assert_eq!(files.shown_path(), Some(Path::new("1.png")));
        assert_eq!(files.len(), 3, "the file on screen, and what is left");
        assert_eq!(files.step(true).map(|r| r.index), Some(2));
        assert_eq!(files.path(2), Path::new("2.png"));
    }

    /// The file on screen and the neighbors it would have stepped to, all
    /// gone at once. It keeps its place among whatever is left, so that `]`
    /// reaches the next survivor and `[` the last one before it — the walk
    /// carries on from where the user actually is, not from where the list
    /// happens to have closed up.
    #[test]
    fn a_file_deleted_with_its_neighbors_keeps_its_place_among_the_survivors() {
        let mut files = list(5);
        files.shown(2);
        assert!(files.relist(named(&["0.png", "4.png"])));
        assert_eq!(files.shown_path(), Some(Path::new("2.png")));
        assert_eq!(files.index(), 1);
        assert_eq!(files.path(0), Path::new("0.png"));
        assert_eq!(files.path(2), Path::new("4.png"));
        assert_eq!(files.step(true).map(|r| r.index), Some(2));

        // And with everything before it gone, it leads what is left.
        let mut files = list(3);
        files.shown(1);
        assert!(files.relist(named(&["2.png"])));
        assert_eq!(files.index(), 0);
        assert_eq!(files.path(1), Path::new("2.png"));
    }

    /// The list is read again every time the directory changes, so a file
    /// already deleted is passed over the missing path again and again. It
    /// stays where it was put, and the rebuilds around it go on as normal.
    #[test]
    fn a_file_already_gone_keeps_its_place_through_later_rebuilds() {
        let mut files = list(3);
        files.shown(1);
        assert!(
            !files.relist(named(&["0.png", "2.png"])),
            "the file on screen going back in leaves the list as it was, and \
             nothing in the bar reads any differently for it"
        );

        // A file arrives after it: the one on screen has not moved.
        assert!(files.relist(named(&["0.png", "2.png", "3.png"])));
        assert_eq!(files.index(), 1);
        assert_eq!(files.shown_path(), Some(Path::new("1.png")));
        assert_eq!(files.len(), 4);

        // One arrives before it, and it moves along with the rest.
        assert!(files.relist(named(&["0.png", "0a.png", "2.png", "3.png"])));
        assert_eq!(files.index(), 2);
        assert_eq!(files.shown_path(), Some(Path::new("1.png")));

        // And the file itself comes back: it is an ordinary member again,
        // in the place the directory gives it rather than the place we kept.
        assert!(files.relist(named(&["0.png", "0a.png", "1.png", "2.png"])));
        assert_eq!(files.index(), 2);
        assert_eq!(files.len(), 4, "no phantom left behind beside it");
    }

    /// A pasted picture goes in beside the file on screen and is asked for
    /// straight away, so that `[` goes back to where the user was and `]`
    /// carries on where they were going.
    #[test]
    fn a_pasted_file_joins_the_list_beside_the_one_on_screen() {
        let mut files = list(3);
        files.shown(1);
        let request = files.adopt(
            PathBuf::from("/pictures/pasted.png"),
            Source::Clipboard("image/png".into()),
        );
        assert_eq!(request.index, 2);
        assert!(matches!(request.source, Source::Clipboard(_)));
        assert_eq!(files.len(), 4);
        assert_eq!(files.path(2), Path::new("/pictures/pasted.png"));
        assert_eq!(files.index(), 1, "nothing is on screen until it is read");

        let pending = files
            .accept(request.generation)
            .expect("the reply we waited for");
        assert!(
            files.failed(2, pending.step).is_none(),
            "a paste asks for one picture rather than walking off to another"
        );
    }

    /// No directory named on the command line lists a pasted file — it was
    /// written where pictures are kept — so a rebuild would drop every one of
    /// them the moment the user stepped off it.
    #[test]
    fn a_pasted_file_survives_the_list_being_read_again() {
        let mut files = list(3);
        files.shown(1);
        let request = files.adopt(
            PathBuf::from("pasted.png"),
            Source::Clipboard("image/png".into()),
        );
        files.accept(request.generation);
        files.shown(request.index);
        assert_eq!(files.shown_path(), Some(Path::new("pasted.png")));

        // Stepped off it, and the directory changes underneath.
        let request = files.step(true).expect("somewhere to step");
        files.accept(request.generation);
        files.shown(request.index);
        assert!(files.relist(named(&["0.png", "1.png", "2.png", "3.png"])));
        assert_eq!(files.len(), 5);
        assert_eq!(
            files.path(2),
            Path::new("pasted.png"),
            "still between the file it was pasted beside and the next one"
        );
        assert_eq!(files.shown_path(), Some(Path::new("2.png")));
    }

    /// `--paste` puts the clipboard's picture at the head of the list, and it
    /// is a paste like any other from then on: fetched by the loader, kept
    /// through a relist that no named directory would put it back into, and
    /// walked past — since the rest of the list was asked for too — if it
    /// never arrives.
    #[test]
    fn a_paste_at_the_head_of_the_list_is_kept_like_any_other() {
        let mut files = Files::new(
            named(&["pasted.png", "0.png", "1.png"]),
            0,
            decode::Overrides::default(),
        );
        let request = files.open_first(Source::Clipboard("image/png".into()));
        assert_eq!(request.index, 0);
        assert!(matches!(request.source, Source::Clipboard(_)));

        let pending = files
            .accept(request.generation)
            .expect("the reply we waited for");
        let next = files
            .failed(0, pending.step)
            .expect("a paste that never arrived is walked past");
        assert_eq!(next.index, 1);
        assert!(matches!(next.source, Source::Disk));
        files.accept(next.generation);
        files.shown(1);

        assert!(files.relist(named(&["0.png", "1.png", "2.png"])));
        assert_eq!(files.path(0), Path::new("pasted.png"), "still at the head");
        assert_eq!(files.shown_path(), Some(Path::new("0.png")));
        assert_eq!(files.len(), 4);
    }

    /// Emptying the directory altogether leaves the picture that is up, with
    /// nowhere to step to.
    #[test]
    fn an_emptied_directory_leaves_the_one_file_on_screen() {
        let mut files = list(2);
        assert!(files.relist(Vec::new()));
        assert_eq!(files.len(), 1);
        assert_eq!(files.shown_path(), Some(Path::new("0.png")));
        assert!(files.step(true).is_none());
    }

    #[test]
    fn a_reload_waits_for_the_read_already_in_flight() {
        let mut files = list(2);
        assert!(files.reload().is_some());
        assert!(files.reload().is_none());
    }

    /// A file that opens between two frames must not flicker a word into the
    /// interface and out again; one that keeps the user waiting has to say so,
    /// and say it once.
    #[test]
    fn only_a_read_that_keeps_the_user_waiting_is_announced() {
        let mut files = list(2);
        assert_eq!(files.announce_slow_read(Instant::now()), Announce::Nothing);

        let request = files.step(true).expect("two files");
        let since = files.pending().expect("a request is in flight").since;
        assert_eq!(
            files.announce_slow_read(since),
            Announce::Waiting(since + SLOW_READ),
            "a read that has just started is not worth mentioning yet"
        );
        assert_eq!(files.announce_slow_read(since + SLOW_READ), Announce::Now);
        assert_eq!(
            files.announce_slow_read(since + SLOW_READ * 2),
            Announce::Nothing,
            "and having been said once it is not said again"
        );

        files.accept(request.generation);
        assert_eq!(
            files.announce_slow_read(since + SLOW_READ * 2),
            Announce::Nothing
        );
    }

    /// A file moved to the trash stays on the list, and on screen, until
    /// its neighbor has taken the screen from it, and leaves the list then
    /// — with the file on screen and a read in flight aimed where they
    /// were. From the last of the list the step away is back rather than
    /// round to the first: the walk goes on the way it was going.
    #[test]
    fn a_trashed_file_leaves_the_list_once_another_has_the_screen() {
        let mut files = list(4);
        files.shown(1);
        files.condemn();
        assert!(files.is_condemned(Path::new("1.png")));
        let request = files.step_away().expect("somewhere to go");
        assert_eq!(request.index, 2);
        assert_eq!(files.len(), 4, "still on the list while it is on screen");

        files.accept(request.generation);
        files.shown(2);
        assert_eq!(files.len(), 3);
        assert_eq!(files.shown_path(), Some(Path::new("2.png")));
        assert_eq!(files.index(), 1, "the file on screen moved up with it");
        assert!(!files.is_condemned(Path::new("1.png")));
        assert_eq!(
            files.paths(),
            &[
                PathBuf::from("0.png"),
                PathBuf::from("2.png"),
                PathBuf::from("3.png")
            ]
        );

        // From the end of the list, back.
        files.shown(2);
        files.condemn();
        let request = files.step_away().expect("somewhere to go");
        assert_eq!(request.index, 1);
        files.accept(request.generation);
        files.shown(1);
        assert_eq!(
            files.paths(),
            &[PathBuf::from("0.png"), PathBuf::from("2.png")]
        );
        assert_eq!(files.index(), 1);

        // The only file has nowhere to go: it leaves the list, which is
        // then empty, and comes back by being put back at the head.
        let mut alone = list(1);
        assert!(alone.step_away().is_none());
        alone.remove_shown();
        assert_eq!(alone.len(), 0);
        assert_eq!(alone.shown_path(), None);
        assert!(alone.is_idle());
        let back = alone.reinstate(PathBuf::from("0.png"), 0, false);
        assert_eq!(back.index, 0);
        assert_eq!(alone.len(), 1);
        alone.accept(back.generation);
        alone.shown(0);
        assert_eq!(alone.shown_path(), Some(Path::new("0.png")));
    }

    /// A neighbor that will not decode leaves the trashed file on screen,
    /// and on the list: what is on screen is still what the list has to
    /// name.
    #[test]
    fn a_trashed_file_stays_while_nothing_takes_the_screen() {
        let mut files = list(2);
        files.condemn();
        let request = files.step_away().expect("a neighbor");
        let pending = files.accept(request.generation).expect("ours");
        assert!(files.failed(pending.index, pending.step).is_none());
        files.shown(0);
        assert_eq!(files.len(), 2);
        assert!(files.is_condemned(Path::new("0.png")));
    }

    /// A file put back from the trash goes back where it stood — or at the
    /// end of a list that has grown shorter — and is asked for; the file on
    /// screen and a read in flight keep their places.
    #[test]
    fn a_reinstated_file_goes_back_where_it_was() {
        let mut files = list(3);
        files.shown(2);
        let request = files.reinstate(PathBuf::from("1b.png"), 1, false);
        assert_eq!(request.index, 1);
        assert_eq!(files.path(1), Path::new("1b.png"));
        assert_eq!(files.index(), 3, "the file on screen moved along");
        assert_eq!(files.pending().map(|pending| pending.index), Some(1));

        let request = files.reinstate(PathBuf::from("9.png"), 10, true);
        assert_eq!(request.index, 4, "past the end goes at the end");
        assert!(files.is_adopted(Path::new("9.png")));
        assert!(!files.is_adopted(Path::new("1b.png")));
        // A rebuild that does not list it keeps it, as it keeps a paste.
        files.accept(request.generation);
        files.shown(4);
        assert!(files.relist(named(&["0.png", "1.png", "2.png"])));
        assert_eq!(files.len(), 4);
        assert_eq!(files.shown_path(), Some(Path::new("9.png")));
    }

    /// A renamed file keeps its place on the list under its new name, and
    /// stays kept through a rebuild if it was a paste.
    #[test]
    fn a_renamed_file_keeps_its_place() {
        let mut files = list(3);
        let request = files.adopt(PathBuf::from("pasted.png"), Source::Disk);
        files.accept(request.generation);
        files.shown(1);
        files.rename(1, PathBuf::from("kept.png"));
        assert_eq!(files.shown_path(), Some(Path::new("kept.png")));
        assert_eq!(files.position(Path::new("pasted.png")), None);
        assert!(files.is_adopted(Path::new("kept.png")));
        files.shown(0);
        assert!(
            !files.relist(named(&["0.png", "1.png", "2.png"])),
            "put back where it was, the list is as it was"
        );
        assert_eq!(files.position(Path::new("kept.png")), Some(1));
    }

    /// A read in flight of the very file that has left the list is a read
    /// of nothing: its reply is dropped rather than shown under another
    /// file's index.
    #[test]
    fn a_read_of_a_file_that_left_the_list_is_dropped() {
        let mut files = list(3);
        files.shown(1);
        files.condemn();
        let reload = files.reload().expect("idle");
        // Another file arrives first — a pick from the chooser, say.
        let pick = files.go_to(2);
        files.accept(pick.generation);
        files.shown(2);
        assert_eq!(files.len(), 2);
        assert!(files.accept(reload.generation).is_none());
    }
}
