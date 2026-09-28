//! The folder beside a single file: what a command line naming one file
//! and nothing else steps on through, as a desktop's viewer does when a
//! file is opened from the file manager.
//!
//! Not read until it is wanted — a step past the file, the chooser, the
//! file list — so that one file opened is one file on screen, with none of
//! the list's furniture around it, until the user asks to go further. Then
//! read on a thread of its own, since a folder can be large or far away,
//! with what the order in force needs of each file read alongside the
//! names: a step waits for the whole folder and lands on the file that
//! really comes next, whatever the list is sorted by.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Instant, SystemTime};

use anyhow::Result;

use super::App;
use super::files::SLOW_READ;
use super::input::{self, Effect};
use crate::image::decode;
use crate::ui::Control;
use crate::ui::filmstrip::Sort;
use crate::ui::toast::{Level, Toast};
use crate::watch::Watch;

/// What the folder read is for: what it does once it is in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Then {
    /// A step, forward or back.
    Step(bool),
    /// The chooser, opened over the list.
    Chooser,
    /// The file list, put up.
    Filmstrip,
}

/// Where the folder beside the list stands.
#[derive(Debug, Default)]
pub(super) enum Folder {
    /// No folder to read: several things were named, or a directory, or
    /// the setting is off, or it has been read.
    #[default]
    Closed,
    /// The folder `file` is in, not read yet.
    Unread { file: PathBuf },
    /// `dir` being read on its thread, for `then`; `since` is when it was
    /// asked for, which the wait is said from.
    Reading {
        dir: PathBuf,
        then: Then,
        since: Instant,
        progress: Arc<Progress>,
    },
}

impl Folder {
    /// Whether there is a folder that could still be read.
    pub(super) fn unread(&self) -> bool {
        matches!(self, Folder::Unread { .. })
    }
}

/// How far a read has got, shared with its thread: how many files it has
/// looked at, of how many. `total` is zero until the folder has been
/// listed, and stays zero for an order that needs nothing but the names.
#[derive(Debug, Default)]
pub(super) struct Progress {
    done: AtomicUsize,
    total: AtomicUsize,
}

impl Progress {
    /// Files looked at, of how many; `None` while it is not known how many
    /// there are to look at.
    pub(super) fn counted(&self) -> Option<(usize, usize)> {
        let total = self.total.load(Ordering::Relaxed);
        (total > 0).then(|| (self.done.load(Ordering::Relaxed), total))
    }
}

/// What the order knows of a file before its header is read by the
/// thumbnail thread: only what the order in force asked for, the rest
/// `None`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Glimpse {
    pub format: Option<&'static str>,
    pub bytes: Option<u64>,
    pub size: Option<(u32, u32)>,
    pub modified: Option<SystemTime>,
}

/// A folder read: which file it was read for, the folder as that file's
/// name has it, and the images in it with a glimpse of each. `images` is
/// the error where the folder would not list.
pub struct Listed {
    pub(super) file: PathBuf,
    pub(super) dir: PathBuf,
    pub(super) images: Result<Vec<PathBuf>>,
    pub(super) glimpses: Vec<(PathBuf, Glimpse)>,
}

/// What was glimpsed of each file, by its path.
pub type Glimpses = std::collections::HashMap<PathBuf, Glimpse>;

/// How a finished read reaches the event loop.
pub type Deliver = Arc<dyn Fn(Listed) + Send + Sync>;

/// Reads the folder `file` is in on a thread of its own, glimpsing each
/// image for what `sort` needs, and hands it to `deliver`. Its progress is
/// the one handed back.
pub(super) fn read_on_thread(file: PathBuf, sort: Sort, deliver: Deliver) -> Arc<Progress> {
    let progress = Arc::new(Progress::default());
    let counting = Arc::clone(&progress);
    let spawned = std::thread::Builder::new()
        .name("folder".into())
        .spawn(move || deliver(read(&file, sort, &counting)));
    if let Err(error) = spawned {
        eprintln!("{}: reading the folder: {error}", crate::PROGRAM);
    }
    progress
}

/// The read itself, on whichever thread it is called on.
pub(super) fn read(file: &Path, sort: Sort, progress: &Progress) -> Listed {
    let dir = crate::listing::folder_of(file);
    let images = crate::listing::images_in(&dir);
    let glimpses = match &images {
        Ok(images) => glimpse_all(images, sort, progress),
        Err(_) => Vec::new(),
    };
    Listed {
        file: file.to_path_buf(),
        dir,
        images,
        glimpses,
    }
}

/// What `sort` needs of each of `paths`, counted into `progress` as it
/// goes; nothing at all for a sort by name.
pub(super) fn glimpse_all(
    paths: &[PathBuf],
    sort: Sort,
    progress: &Progress,
) -> Vec<(PathBuf, Glimpse)> {
    if !sort.reads_facts() {
        return Vec::new();
    }
    progress.total.store(paths.len(), Ordering::Relaxed);
    paths
        .iter()
        .map(|path| {
            let glimpse = glimpse(path, sort);
            progress.done.fetch_add(1, Ordering::Relaxed);
            (path.clone(), glimpse)
        })
        .collect()
}

/// What `sort` needs of `path`, and nothing more: the file system's word
/// for a date or a size, the leading bytes for a type, the header for the
/// dimensions. The last two parse bytes chosen by whoever wrote the file,
/// so they run under the loader's guard, a panic being a glimpse of
/// nothing.
fn glimpse(path: &Path, sort: Sort) -> Glimpse {
    match sort {
        Sort::Name | Sort::Path => Glimpse::default(),
        Sort::Date | Sort::Size => {
            let metadata = std::fs::metadata(path).ok();
            Glimpse {
                bytes: metadata.as_ref().map(std::fs::Metadata::len),
                modified: metadata.and_then(|metadata| metadata.modified().ok()),
                ..Glimpse::default()
            }
        }
        Sort::Type => Glimpse {
            format: crate::loader::guard("reading the type", || Ok(decode::reader(path)))
                .ok()
                .flatten(),
            ..Glimpse::default()
        },
        Sort::Width | Sort::Height | Sort::Area => Glimpse {
            size: crate::loader::guard("reading the header", || decode::probe(path))
                .ok()
                .flatten(),
            ..Glimpse::default()
        },
    }
}

impl App {
    /// Opens what the desktop's file manager sent, as [`App::open_named`]
    /// does what the dialog chose; but one file sent to a window with
    /// nothing on its list has its folder beside it to step on through, as
    /// one named alone on the command line does.
    pub(super) fn open_sent(&mut self, named: Vec<PathBuf>) {
        let alone = match named.as_slice() {
            [file] if self.browse_folder && self.files.len() == 0 && !file.is_dir() => {
                Some(file.clone())
            }
            _ => None,
        };
        self.open_named(named);
        // Named only once it opened: a file that could not be listed
        // leaves the window as it was.
        if let Some(file) = alone
            && self.named.contains(&file)
        {
            self.folder = Folder::Unread { file };
        }
    }

    /// Reads the folder beside the one file, if there is one still to read,
    /// to do `then` once it is in; a second ask while it is being read
    /// changes what it is read for. Says whether it is being read, which
    /// is whether the caller has nothing more to do.
    pub(super) fn read_folder(&mut self, then: Then) -> bool {
        match &mut self.folder {
            Folder::Unread { file } => {
                let progress = read_on_thread(
                    file.clone(),
                    self.filmstrip.order().sort,
                    Arc::clone(&self.folder_delivered),
                );
                self.folder = Folder::Reading {
                    dir: crate::listing::folder_of(file),
                    then,
                    since: Instant::now(),
                    progress,
                };
                true
            }
            Folder::Reading { then: waiting, .. } => {
                *waiting = then;
                true
            }
            Folder::Closed => false,
        }
    }

    /// Takes in a folder read, which goes into the list at the first
    /// chance.
    pub(super) fn folder_read(&mut self, listed: Listed) -> Effect {
        self.listed = Some(listed);
        self.settle_folder()
    }

    /// Puts a folder read into the list and does what it was read for.
    /// Between reads only, as any rebuild is — a read in flight is aimed at
    /// an index — so under one it waits for the next chance.
    pub(super) fn settle_folder(&mut self) -> Effect {
        if self.listed.is_none() || !self.files.is_idle() {
            return Effect::Nothing;
        }
        let Some(Listed {
            file,
            dir,
            images,
            glimpses,
        }) = self.listed.take()
        else {
            return Effect::Nothing;
        };
        let then = match std::mem::take(&mut self.folder) {
            Folder::Reading { then, .. } => then,
            other => {
                self.folder = other;
                return Effect::Nothing;
            }
        };
        // The file went while its folder was being read, and with it the
        // list the folder was to join.
        if self.files.len() == 0 {
            return Effect::Nothing;
        }
        let images = match images {
            Ok(images) => images,
            // The why is on the terminal; the window names the folder,
            // which the error's outermost line only does in passing.
            Err(error) => {
                input::report(&error);
                self.toast(format!("Could not read {}", name(&dir)), Level::Warning);
                return Effect::Redraw;
            }
        };
        // The folder takes the file's place among the names, so that it is
        // watched and a rebuild reads it again. The file stays named beside
        // it where the folder does not list it: one read for what it holds
        // rather than what it is called.
        if let Some(at) = self.named.iter().position(|named| *named == file) {
            let mut instead = vec![dir.clone()];
            if !images.contains(&file) {
                instead.push(file);
            }
            self.named.splice(at..=at, instead);
        }
        self.directories
            .push(Watch::new(crate::listing::opened(&dir)));
        self.glimpsed.extend(glimpses);
        // The folder as it was just read rather than read again here: the
        // point of the thread was to keep the listing off this one.
        let mut list = Vec::new();
        for named in &self.named {
            if *named == dir {
                list.extend(images.iter().cloned());
            } else {
                list.extend(crate::listing::relist(std::slice::from_ref(named)));
            }
        }
        if self.files.relist(list) {
            self.list_changed();
        }
        let _ = self.apply_order();
        if self.files.len() < 2 {
            self.toast(format!("No other images in {}", name(&dir)), Level::Message);
            return Effect::Redraw;
        }
        match then {
            Then::Step(forward) => self.step(forward),
            Then::Chooser => self.press(Control::Chooser),
            Then::Filmstrip => {
                self.panels.show_filmstrip = true;
                self.filmstrip.reveal();
                Effect::Redraw
            }
        }
    }

    /// The toast about the folder being read, once it has been read for
    /// as long as a file is before its wait is said, with how far it has
    /// got where there is a count to give.
    pub(super) fn reading_folder(&self) -> Option<Toast> {
        let Folder::Reading {
            dir,
            since,
            progress,
            ..
        } = &self.folder
        else {
            return None;
        };
        let raised = *since + SLOW_READ;
        if Instant::now() < raised {
            return None;
        }
        let name = name(dir);
        let message = match progress.counted() {
            Some((done, total)) => format!("Reading {name}\u{2026} {done} of {total}"),
            None => format!("Reading {name}\u{2026}"),
        };
        Some(Toast::waiting(message, raised))
    }

    /// When the toast about the folder being read goes up, while that is
    /// still to come.
    pub(super) fn folder_due(&self, now: Instant) -> Option<Instant> {
        match &self.folder {
            Folder::Reading { since, .. } => Some(*since + SLOW_READ).filter(|due| *due > now),
            _ => None,
        }
    }

    /// Puts a folder read that came back under a read of a file into the
    /// list, and redraws the toast while the count in it moves on.
    pub(super) fn poll_folder(&mut self) -> Effect {
        let counting = matches!(
            &self.folder,
            Folder::Reading { progress, .. } if progress.counted().is_some()
        );
        self.settle_folder().also(Effect::redraw_if(
            counting && self.reading_folder().is_some(),
        ))
    }
}

/// What a folder is called in a message: its own name, found through the
/// working directory where it was named as `.` or not at all.
pub(super) fn name(dir: &Path) -> String {
    let opened = crate::listing::opened(dir);
    std::path::absolute(opened)
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| opened.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str, files: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gamut-folder-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for file in files {
            std::fs::write(dir.join(file), b"").unwrap();
        }
        dir
    }

    /// The images beside the file, in name order, the file among them
    /// under the same spelling it was named by; what is not an image left
    /// out.
    #[test]
    fn a_folder_lists_the_images_beside_the_file() {
        let dir = folder("lists", &["b.png", "a.jpg", "notes.txt", "c.png"]);
        let file = dir.join("b.png");
        let listed = read(&file, Sort::Name, &Progress::default());
        assert_eq!(listed.dir, dir);
        assert_eq!(
            listed.images.unwrap(),
            [dir.join("a.jpg"), dir.join("b.png"), dir.join("c.png")]
        );
        assert!(
            listed.glimpses.is_empty(),
            "a sort by name needs nothing more"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A sort by date or size reads what the file system says of each
    /// file, and counts them as it goes.
    #[test]
    fn a_sort_by_size_glimpses_every_file() {
        let dir = folder("sizes", &["a.png", "b.png"]);
        std::fs::write(dir.join("b.png"), b"four").unwrap();
        let progress = Progress::default();
        let listed = read(&dir.join("a.png"), Sort::Size, &progress);
        let bytes: Vec<_> = listed
            .glimpses
            .iter()
            .map(|(path, glimpse)| (path.clone(), glimpse.bytes))
            .collect();
        assert_eq!(
            bytes,
            [(dir.join("a.png"), Some(0)), (dir.join("b.png"), Some(4))]
        );
        assert_eq!(progress.counted(), Some((2, 2)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A bare name's folder is the current directory, and its images come
    /// back as bare names, so that the file named is found among them.
    #[test]
    fn a_bare_name_lists_as_bare_names() {
        assert_eq!(
            crate::listing::folder_of(Path::new("a.png")),
            PathBuf::new()
        );
        let listed = read(Path::new("Cargo.toml"), Sort::Name, &Progress::default());
        assert_eq!(listed.dir, PathBuf::new());
        assert!(
            listed
                .images
                .unwrap()
                .iter()
                .all(|path| path.parent() == Some(Path::new(""))),
            "every image under its bare name"
        );
    }
}
