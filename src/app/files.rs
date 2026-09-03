//! The list of files, which of them is on screen, and the read in flight.
//!
//! A state machine and nothing else: it hands back the [`Request`] each move
//! calls for and never sends one, so it needs no loader, no window and no
//! disk, and can be driven through a whole walk in a test.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::image::decode;
use crate::loader::{Reload, Request};

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
    /// on can be recognised and dropped.
    pub(super) generation: u64,
    pub(super) index: usize,
    /// Set when the request came from `]` or `[`, so that a file that will not
    /// decode can be stepped over rather than stopping the walk.
    pub(super) step: Option<Step>,
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

pub(super) struct Files {
    paths: Vec<PathBuf>,
    /// The file on screen.
    index: usize,
    overrides: decode::Overrides,
    /// Numbers the requests. Only the newest one's reply is acted on.
    generation: u64,
    pending: Option<Pending>,
}

impl Files {
    /// `index` is the file to open first: the first one whose header could be
    /// read, which is not necessarily the first one named.
    pub(super) fn new(paths: Vec<PathBuf>, index: usize, overrides: decode::Overrides) -> Self {
        Self {
            paths,
            index,
            overrides,
            generation: 0,
            pending: None,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.paths.len()
    }

    /// Which file is on screen.
    pub(super) fn index(&self) -> usize {
        self.index
    }

    pub(super) fn path(&self, index: usize) -> &Path {
        &self.paths[index]
    }

    pub(super) fn shown_path(&self) -> &Path {
        &self.paths[self.index]
    }

    #[cfg(test)]
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
    pub(super) fn open_first(&mut self) -> Request {
        let remaining = self.paths.len() - 1;
        self.request(
            self.index,
            Reload::Fresh,
            Some(Step {
                forward: true,
                remaining,
            }),
        )
    }

    /// Moves to the next or previous file. `None` when there is nowhere to go.
    ///
    /// From wherever the last request was aimed rather than from what is on
    /// screen, so that holding `]` walks the list instead of asking for the
    /// same neighbour over and over while a slow file opens. Only the last of
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
        let next = self.neighbour(from, forward);
        Some(self.request(
            next,
            Reload::Fresh,
            Some(Step {
                forward,
                // Everything but the file being asked for and the one already
                // on screen.
                remaining: self.paths.len() - 2,
            }),
        ))
    }

    /// Re-reads the file on screen. `None` while a read is already in
    /// flight: a file being written continuously would otherwise stack up a
    /// decode every interval, and the reply already on its way carries a
    /// watch taken later than this one anyway.
    pub(super) fn reload(&mut self) -> Option<Request> {
        if self.pending.is_some() {
            return None;
        }
        Some(self.request(self.index, Reload::InPlace, None))
    }

    fn request(&mut self, index: usize, mode: Reload, step: Option<Step>) -> Request {
        self.generation += 1;
        self.pending = Some(Pending {
            generation: self.generation,
            index,
            step,
            since: Instant::now(),
            announced: false,
        });
        Request {
            generation: self.generation,
            index,
            path: self.paths[index].clone(),
            overrides: self.overrides,
            mode,
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
    /// The file on screen stays on the list wherever it has gone: its pixels
    /// are up and correct, and dropping the path they came from would leave
    /// the title, the bars and the information panel describing a file that is
    /// not the one being shown. It keeps its place among its neighbours too,
    /// so that `]` lands on whatever has taken its position rather than on a
    /// file the user has already seen.
    ///
    /// Between reads only: this moves the file on screen to a new index, and a
    /// request in flight is aimed at the old one.
    pub(super) fn relist(&mut self, mut paths: Vec<PathBuf>) -> bool {
        debug_assert!(self.is_idle(), "the list is rebuilt between reads");
        let shown = self.paths[self.index].clone();
        self.index = match paths.iter().position(|path| *path == shown) {
            Some(index) => index,
            None => {
                let at = self.place_for(&shown, &paths);
                paths.insert(at, shown);
                at
            }
        };
        let changed = paths != self.paths;
        self.paths = paths;
        changed
    }

    /// Where a file that is no longer in the directory goes back into the
    /// list: after everything that came before it, before everything that came
    /// after, which is what keeps `]` and `[` going the way the user was
    /// going.
    ///
    /// A path that was in the old list settles which side it is on by where it
    /// was. One that has only just appeared has no place there to go on, and
    /// settles it by its name — the order the directory itself was read in,
    /// which is the order the rest of the list is in.
    fn place_for(&self, shown: &Path, paths: &[PathBuf]) -> usize {
        let was: HashMap<&Path, usize> = self
            .paths
            .iter()
            .enumerate()
            .map(|(index, path)| (path.as_path(), index))
            .collect();
        paths
            .iter()
            .take_while(|path| match was.get(path.as_path()) {
                Some(&index) => index < self.index,
                None => path.as_path() < shown,
            })
            .count()
    }

    /// A reply has reached the screen.
    pub(super) fn shown(&mut self, index: usize) {
        self.index = index;
    }

    /// A reply would not go on screen. Carries a walk on past the file, so
    /// that one bad file cannot trap navigation; `None` once it has tried them
    /// all, or when the read was not part of a walk.
    pub(super) fn failed(&mut self, from: usize, step: Option<Step>) -> Option<Request> {
        let step = step?;
        if step.remaining == 0 {
            return None;
        }
        let next = self.neighbour(from, step.forward);
        Some(self.request(
            next,
            Reload::Fresh,
            Some(Step {
                forward: step.forward,
                remaining: step.remaining - 1,
            }),
        ))
    }

    fn neighbour(&self, index: usize, forward: bool) -> usize {
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

    /// A directory read again is a list read again: a file written into it
    /// joins the walk, and the one on screen goes on being the one on screen.
    #[test]
    fn a_file_that_has_appeared_joins_the_list() {
        let mut files = list(2);
        files.shown(1);
        assert!(files.relist(named(&["0.png", "1.png", "2.png"])));
        assert_eq!(files.len(), 3);
        assert_eq!(files.shown_path(), Path::new("1.png"));
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
    /// is read off the path they came from. It keeps its neighbours, so that
    /// `]` goes on to what has taken its place rather than back over a file
    /// already seen.
    #[test]
    fn the_file_on_screen_survives_being_deleted_under_us() {
        let mut files = list(4);
        files.shown(1);
        assert!(files.relist(named(&["0.png", "2.png"])));
        assert_eq!(files.shown_path(), Path::new("1.png"));
        assert_eq!(files.len(), 3, "the file on screen, and what is left");
        assert_eq!(files.step(true).map(|r| r.index), Some(2));
        assert_eq!(files.path(2), Path::new("2.png"));
    }

    /// The file on screen and the neighbours it would have stepped to, all
    /// gone at once. It keeps its place among whatever is left, so that `]`
    /// reaches the next survivor and `[` the last one before it — the walk
    /// carries on from where the user actually is, not from where the list
    /// happens to have closed up.
    #[test]
    fn a_file_deleted_with_its_neighbours_keeps_its_place_among_the_survivors() {
        let mut files = list(5);
        files.shown(2);
        assert!(files.relist(named(&["0.png", "4.png"])));
        assert_eq!(files.shown_path(), Path::new("2.png"));
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
        assert_eq!(files.shown_path(), Path::new("1.png"));
        assert_eq!(files.len(), 4);

        // One arrives before it, and it moves along with the rest.
        assert!(files.relist(named(&["0.png", "0a.png", "2.png", "3.png"])));
        assert_eq!(files.index(), 2);
        assert_eq!(files.shown_path(), Path::new("1.png"));

        // And the file itself comes back: it is an ordinary member again,
        // in the place the directory gives it rather than the place we kept.
        assert!(files.relist(named(&["0.png", "0a.png", "1.png", "2.png"])));
        assert_eq!(files.index(), 2);
        assert_eq!(files.len(), 4, "no phantom left behind beside it");
    }

    /// Emptying the directory altogether leaves the picture that is up, with
    /// nowhere to step to.
    #[test]
    fn an_emptied_directory_leaves_the_one_file_on_screen() {
        let mut files = list(2);
        assert!(files.relist(Vec::new()));
        assert_eq!(files.len(), 1);
        assert_eq!(files.shown_path(), Path::new("0.png"));
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
}
