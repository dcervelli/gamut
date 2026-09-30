//! A list about to be opened, put in the order the file list was left in
//! before the first of it is shown: the command line's, when the order
//! needs more than the names, and whatever the desktop's file dialog
//! chose.
//!
//! The first file shown is the first in that order, rather than the first
//! by name with the list sorted round it once it has arrived. What the
//! order needs of each file is read on a thread of its own, as the folder
//! beside a single file is ([`super::folder`]) — a `stat` for a date or a
//! size, the leading bytes for a type, the header for the dimensions — so
//! that a folder of large raws holds nothing up while it is read. An order
//! that needs only the names is put in place at once.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use super::App;
use super::files::SLOW_READ;
use super::folder::{self, Glimpse, Progress};
use super::input::Effect;
use crate::image::decode;
use crate::loader::Source;
use crate::ui::filmstrip::Sort;
use crate::ui::toast::Toast;

/// What an arranged list is for once it is in order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Arrive {
    /// The command line's list, already on the list: its first file opened.
    Open,
    /// Files chosen in the window, not on the list yet: added to its end,
    /// the first of them in the order opened.
    Append,
}

/// A list being put in order on its thread.
#[derive(Debug)]
pub(super) struct Arranging {
    job: u64,
    arrive: Arrive,
    sort: Sort,
    since: Instant,
    progress: Arc<Progress>,
}

/// A list read for its order: which job it answers, the files as they
/// were handed over, and what was read of each.
pub struct Arranged {
    job: u64,
    paths: Vec<PathBuf>,
    glimpses: Vec<(PathBuf, Glimpse)>,
}

/// How an arranged list reaches the event loop.
pub type Deliver = super::Deliver<Arranged>;

/// The lists being read for their order, as the application holds them:
/// `jobs` those in flight, `landed` those read and waiting for a read of a
/// file to finish, `deliver` how one comes back, `next_job` the count that
/// tells them apart, and `awaiting_window` whether the loop has asked for
/// the window and been kept waiting on the command line's order — the
/// window opens at the size of the first file in it if the order is in by
/// [`super::files::SLOW_READ`], and at the empty window's size if not.
pub(super) struct Arrangings {
    pub(super) jobs: Vec<Arranging>,
    pub(super) landed: Vec<Arranged>,
    pub(super) deliver: Deliver,
    pub(super) next_job: u64,
    pub(super) awaiting_window: bool,
}

/// Reads what `sort` needs of each of `paths`, on a thread of its own, and
/// hands it to `deliver`.
fn read_on_thread(job: u64, paths: Vec<PathBuf>, sort: Sort, deliver: Deliver) -> Arc<Progress> {
    let progress = Arc::new(Progress::default());
    let counting = Arc::clone(&progress);
    let spawned = std::thread::Builder::new()
        .name("arranging".into())
        .spawn(move || {
            let glimpses = folder::glimpse_all(&paths, sort, &counting);
            deliver(Arranged {
                job,
                paths,
                glimpses,
            });
        });
    if let Err(error) = spawned {
        eprintln!("{}: putting the list in order: {error}", crate::PROGRAM);
    }
    progress
}

/// The read [`read_on_thread`] makes, made here and now: a test has no
/// event loop for the thread to reach.
#[cfg(test)]
pub(super) fn read_now(job: u64, paths: Vec<PathBuf>, sort: Sort) -> Arranged {
    let glimpses = folder::glimpse_all(&paths, sort, &Progress::default());
    Arranged {
        job,
        paths,
        glimpses,
    }
}

impl App {
    /// Puts `paths` in the list's order and then does what `arrive` says
    /// with them: at once for an order of names, and once they have been
    /// read for any other.
    pub(super) fn arrange(&mut self, paths: Vec<PathBuf>, arrive: Arrive) {
        let sort = self.filmstrip.order().sort;
        if !sort.reads_facts() || paths.len() < 2 {
            self.arrived(arrive, paths);
            return;
        }
        self.arranging.next_job += 1;
        let progress = read_on_thread(
            self.arranging.next_job,
            paths,
            sort,
            Arc::clone(&self.arranging.deliver),
        );
        self.arranging.jobs.push(Arranging {
            job: self.arranging.next_job,
            arrive,
            sort,
            since: Instant::now(),
            progress,
        });
    }

    /// Takes in a list read for its order, which is acted on at the first
    /// chance.
    pub(super) fn arranged_read(&mut self, arranged: Arranged) -> Effect {
        self.arranging.landed.push(arranged);
        self.settle_arranged()
    }

    /// Acts on every list read for its order. Between reads only, as any
    /// change to the list is: a read in flight is aimed at an index.
    pub(super) fn settle_arranged(&mut self) -> Effect {
        if self.arranging.landed.is_empty() || !self.files.is_idle() {
            return Effect::Nothing;
        }
        for Arranged {
            job,
            paths,
            glimpses,
        } in std::mem::take(&mut self.arranging.landed)
        {
            let Some(at) = self.arranging.jobs.iter().position(|each| each.job == job) else {
                continue;
            };
            let arrive = self.arranging.jobs.remove(at).arrive;
            self.glimpsed.extend(glimpses);
            self.arrived(arrive, paths);
        }
        Effect::Redraw
    }

    /// `paths` in the list's order, as far as what is known of each can
    /// put them, and then opened or added as `arrive` says.
    fn arrived(&mut self, arrive: Arrive, paths: Vec<PathBuf>) {
        match arrive {
            Arrive::Open => {
                // The list is these files already, the one named first
                // standing ready to be asked for: put in order, and the
                // first in it asked for instead.
                let _ = self.apply_order();
                self.files.start_at(0);
                // `settle_arranged` owes the frame, for this and the list.
                let request = self.files.open_first(Source::Disk);
                self.open_at(&request.path);
                let _ = self.send(request);
            }
            Arrive::Append => {
                let paths = super::arranged(paths, self.filmstrip.order(), &self.glimpsed);
                if let Some(request) = self.files.append(paths) {
                    self.open_at(&request.path);
                    let _ = self.send(request);
                }
                self.list_changed();
            }
        }
    }

    /// Sizes the window for `first`, the file about to be asked for, while
    /// nothing is on screen: from the glimpse where the order read it, and
    /// the header otherwise, which is read in no time beside the picture. A
    /// window still to open opens at it — rather than at the size of the
    /// file whose header was read to make sure of the command line, or at
    /// the empty window's for files Finder sent — and an empty window is
    /// sized to it at once rather than once the picture arrives, which
    /// sizes it again only if it turns out another size.
    fn open_at(&mut self, first: &Path) {
        if self.current.is_some() || (self.shown.is_some() && !self.sizing.to_next) {
            return;
        }
        let size = self
            .glimpsed
            .get(first)
            .and_then(|glimpse| glimpse.size)
            .or_else(|| {
                crate::loader::guard("reading the header", || decode::probe(first))
                    .ok()
                    .flatten()
            })
            .map(|(width, height)| [width as f32, height as f32]);
        match self.shown {
            None => {
                self.sizing.header = size;
                self.sizing.to_next = true;
            }
            Some(_) => {
                if let Some(size) = size
                    && self.sizing.asked.is_none()
                {
                    self.size_window_to(size);
                }
            }
        }
        self.sizing.sized_for = size;
    }

    /// When the window stops waiting for the list it opens on to be put in
    /// order, while it is being: the moment the wait would be said.
    pub(super) fn window_due(&self) -> Option<Instant> {
        self.arranging
            .jobs
            .iter()
            .find(|each| self.opens_on(each))
            .map(|each| each.since + SLOW_READ)
    }

    /// Whether a list is being put in order before any of it is shown.
    pub(super) fn arranging_to_open(&self) -> bool {
        self.arranging.jobs.iter().any(|each| self.opens_on(each))
    }

    /// Whether the window opens on `arranging`: the command line's list, or
    /// files Finder sent before there was a window.
    fn opens_on(&self, arranging: &Arranging) -> bool {
        arranging.arrive == Arrive::Open || self.shown.is_none()
    }

    /// The toast about a list being put in order, once it has taken as long
    /// as a file does before its wait is said.
    pub(super) fn arranging_toast(&self) -> Option<Toast> {
        let arranging = self.arranging.jobs.first()?;
        let raised = arranging.since + SLOW_READ;
        if Instant::now() < raised {
            return None;
        }
        let by = arranging.sort.label().to_lowercase();
        let message = match arranging.progress.counted() {
            Some((done, total)) => format!("Sorting by {by}\u{2026} {done} of {total}"),
            None => format!("Sorting by {by}\u{2026}"),
        };
        Some(Toast::waiting(message, raised))
    }

    /// When the toast about a list being put in order goes up, while that
    /// is still to come.
    pub(super) fn arranging_due(&self, now: Instant) -> Option<Instant> {
        let due = self.arranging.jobs.first()?.since + SLOW_READ;
        (due > now).then_some(due)
    }

    /// A frame while the toast about a list being put in order is up: the
    /// count in it moves on between one look and the next.
    pub(super) fn arranging_counts(&self) -> bool {
        self.arranging_toast().is_some()
    }
}
