//! Noticing that the file on screen has changed underneath us.
//!
//! Polling rather than inotify and its per-platform cousins, deliberately: it
//! is one `stat` every quarter second, it needs no dependency and no
//! background thread, it works on a network mount, and it sees the case that
//! trips naive file watching — an editor that saves by writing a temporary
//! file and renaming it over the original, which leaves any watch on the
//! original inode looking at a file nobody will write to again.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// How often the file is checked. Fast enough that a save feels immediate,
/// slow enough that the process is asleep almost all of the time.
pub const INTERVAL: Duration = Duration::from_millis(250);

/// What the file looked like to `stat`. Not its contents: a write that leaves
/// both the length and the timestamp alone goes unnoticed, which in practice
/// means a write that did not happen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Signature {
    modified: Option<SystemTime>,
    len: u64,
}

/// One file, and enough of its history to tell a finished write from one
/// still in progress.
pub struct Watch {
    path: PathBuf,
    /// The file as it was when we last read it.
    loaded: Option<Signature>,
    /// The file as it was at the previous poll. A change is acted on only
    /// once it has stayed put for a whole interval, so that a half-written
    /// file is not decoded while the writer is still going. Missing means
    /// the file was not there, which is a state a poll can pass through
    /// while a file is being replaced.
    settling: Option<Signature>,
}

impl Watch {
    /// Starts watching `path`, taking it to be what is already on screen.
    pub fn new(path: &Path) -> Self {
        let signature = signature(path);
        Self {
            path: path.to_path_buf(),
            loaded: signature,
            settling: signature,
        }
    }

    /// Returns `true` when the file has changed and then stopped changing,
    /// and so is worth reading again. Costs one `stat`.
    pub fn poll(&mut self) -> bool {
        let seen = signature(&self.path);
        self.advance(seen)
    }

    /// The decision itself, kept apart from the filesystem so it can be
    /// tested a poll at a time.
    fn advance(&mut self, seen: Option<Signature>) -> bool {
        if seen != self.settling {
            // The first sighting of a change, or the middle of a long one.
            self.settling = seen;
            return false;
        }
        // A file that has gone away leaves the last good image on screen:
        // there is nothing to read, and the delete is usually half of a
        // replacement whose other half is along in a moment.
        if seen.is_none() || seen == self.loaded {
            return false;
        }
        self.loaded = seen;
        true
    }
}

fn signature(path: &Path) -> Option<Signature> {
    let metadata = fs::metadata(path).ok()?;
    Some(Signature {
        modified: metadata.modified().ok(),
        len: metadata.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(len: u64) -> Option<Signature> {
        Some(Signature {
            modified: None,
            len,
        })
    }

    fn watching(loaded: Option<Signature>) -> Watch {
        Watch {
            path: PathBuf::from("unused"),
            loaded,
            settling: loaded,
        }
    }

    #[test]
    fn an_unchanged_file_is_never_reloaded() {
        let mut watch = watching(sig(10));
        assert!(!watch.advance(sig(10)));
        assert!(!watch.advance(sig(10)));
    }

    #[test]
    fn a_change_is_reloaded_once_it_settles() {
        let mut watch = watching(sig(10));
        // Seen changed, but it could still be growing.
        assert!(!watch.advance(sig(20)));
        assert!(watch.advance(sig(20)));
        // And not again afterwards.
        assert!(!watch.advance(sig(20)));
    }

    #[test]
    fn a_write_in_progress_waits_for_the_last_of_it() {
        let mut watch = watching(sig(10));
        assert!(!watch.advance(sig(15)));
        assert!(!watch.advance(sig(30)));
        assert!(!watch.advance(sig(64)));
        assert!(watch.advance(sig(64)));
    }

    #[test]
    fn a_missing_file_leaves_what_is_on_screen() {
        let mut watch = watching(sig(10));
        assert!(!watch.advance(None));
        assert!(!watch.advance(None));
        // Replaced rather than deleted: the new file loads.
        assert!(!watch.advance(sig(20)));
        assert!(watch.advance(sig(20)));
    }

    #[test]
    fn a_file_restored_byte_for_byte_is_not_reloaded() {
        let mut watch = watching(sig(10));
        assert!(!watch.advance(None));
        assert!(!watch.advance(sig(10)));
        assert!(!watch.advance(sig(10)));
    }

    #[test]
    fn a_real_file_is_watched() {
        let path = std::env::temp_dir().join(format!("image-view-watch-{}", std::process::id()));
        fs::write(&path, b"first").expect("the temporary directory is writable");
        let mut watch = Watch::new(&path);
        assert!(!watch.poll());

        fs::write(&path, b"second write").expect("the temporary directory is writable");
        assert!(!watch.poll(), "the change has not settled yet");
        assert!(watch.poll());
        assert!(!watch.poll());

        fs::remove_file(&path).expect("we just wrote it");
    }
}
