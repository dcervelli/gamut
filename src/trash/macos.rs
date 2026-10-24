//! The desktop's trash on macOS: Finder's, through `NSFileManager`, which
//! moves a file into the Trash of the volume it is on and says where it went.
//! A file put there shows up in Finder's Trash beside everything else, and
//! Finder's Put Back works on it as on anything else thrown away.
//!
//! Taking it back out is a rename from where it went to where it came from,
//! refusing to replace anything, as on Linux. A process started from a
//! terminal may be refused a look inside `~/.Trash` without Full Disk
//! Access; a refusal is then reported as a failure, never as a file gone.
//!
//! A trash can also be a directory laid out as the freedesktop specification
//! lays one out, which is what the tests put files in, so that no test ever
//! touches the user's own.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use objc2::rc::autoreleasepool;
use objc2_foundation::{NSFileManager, NSURL};

use super::{Refused, Unreplaced, freedesktop};
use crate::no_replace::rename_no_replace;

/// Where files thrown away go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Trash {
    /// Finder's.
    Finder,
    /// A directory laid out as a freedesktop trash, for the tests.
    #[cfg_attr(not(test), allow(dead_code))]
    Directory(freedesktop::Trash),
}

/// One file in a trash, as it was put there: enough to take it out again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Where it came from, absolute: where a restore puts it back.
    pub original: PathBuf,
    held: Held,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Held {
    /// Where Finder's Trash put it.
    Finder(PathBuf),
    Directory(freedesktop::Entry),
}

impl Trash {
    /// Finder's Trash, which is always there to move things to.
    pub fn detect() -> Option<Self> {
        Some(Self::Finder)
    }

    /// A trash whose home directory is `home`, for the tests.
    #[cfg(test)]
    pub fn under(home: PathBuf) -> Self {
        Self::Directory(freedesktop::Trash::under(home))
    }

    /// Moves the file at `path` into the trash, and says exactly where it
    /// went.
    pub fn put(&self, path: &Path) -> Result<Entry> {
        match self {
            Self::Directory(trash) => {
                let entry = trash.put(path)?;
                Ok(Entry {
                    original: entry.original.clone(),
                    held: Held::Directory(entry),
                })
            }
            Self::Finder => {
                let original = std::path::absolute(path)
                    .with_context(|| format!("locating {}", crate::shown_path(path)))?;
                // The file has to be there to be thrown away; asked up front
                // so that the answer is the plain one rather than Cocoa's.
                fs::symlink_metadata(&original)
                    .with_context(|| format!("reading {}", crate::shown_path(&original)))?;
                let held = autoreleasepool(|_| {
                    let url = NSURL::from_file_path(&original)
                        .with_context(|| format!("naming {}", crate::shown_path(&original)))?;
                    let mut resulting = None;
                    NSFileManager::defaultManager()
                        .trashItemAtURL_resultingItemURL_error(&url, Some(&mut resulting))
                        .map_err(|error| anyhow!("{}", error.localizedDescription()))
                        .with_context(|| {
                            format!("moving {} to the Trash", crate::shown_path(&original))
                        })?;
                    resulting
                        .and_then(|url| url.to_file_path())
                        .context("the Trash did not say where the file went")
                })?;
                Ok(Entry {
                    original,
                    held: Held::Finder(held),
                })
            }
        }
    }

    /// Puts `with` where `path` is, the file that was there moved to the
    /// trash, and says which entry it became. Finder's trash only moves, so
    /// for the moment between the two nothing is at `path`; a failure
    /// after the move brings the file back out, and says when that failed
    /// too.
    pub fn replace(&self, path: &Path, with: &Path) -> Result<Entry, Unreplaced> {
        if let Self::Directory(trash) = self {
            let entry = trash.replace(path, with)?;
            return Ok(Entry {
                original: entry.original.clone(),
                held: Held::Directory(entry),
            });
        }
        let entry = self.put(path).map_err(Unreplaced::Untouched)?;
        match rename_no_replace(with, &entry.original) {
            Ok(()) => Ok(entry),
            Err(error) => {
                let error = anyhow!(error).context(format!(
                    "moving the new {} into place",
                    crate::shown_path(path)
                ));
                match restore(&entry) {
                    Ok(()) => Err(Unreplaced::Untouched(error)),
                    Err(_) => Err(Unreplaced::InTrash(error)),
                }
            }
        }
    }

    /// Puts `entry` back where it came from over whatever is there now,
    /// which goes to the trash in its turn: the undoing of
    /// [`Trash::replace`]. Refused before anything moves when the trash no
    /// longer holds the entry; when the entry will not come back, what was
    /// there is brought back out in its place.
    pub fn restore_over(&self, entry: &Entry) -> Result<(), Refused> {
        let held = match (self, &entry.held) {
            (Self::Directory(trash), Held::Directory(inner)) => return trash.restore_over(inner),
            (_, Held::Directory(inner)) => &inner.held,
            (_, Held::Finder(held)) => held,
        };
        match fs::symlink_metadata(held) {
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => return Err(Refused::Gone),
            Err(error) => {
                return Err(Refused::Failed(anyhow!(error).context(format!(
                    "looking in the Trash for {}; Finder's Put Back can restore it",
                    crate::shown_path(&entry.original)
                ))));
            }
        }
        let moved = match fs::symlink_metadata(&entry.original) {
            Ok(_) => Some(self.put(&entry.original).map_err(Refused::Failed)?),
            Err(_) => None,
        };
        restore(entry).inspect_err(|_| {
            if let Some(moved) = &moved {
                let _ = restore(moved);
            }
        })
    }
}

/// Takes `entry` back out of its trash, to exactly where it came from,
/// refusing to replace anything that has arrived there since.
pub fn restore(entry: &Entry) -> Result<(), Refused> {
    let held = match &entry.held {
        Held::Directory(entry) => return freedesktop::restore(entry),
        Held::Finder(held) => held,
    };
    match fs::symlink_metadata(held) {
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => return Err(Refused::Gone),
        Err(error) => {
            return Err(Refused::Failed(anyhow!(error).context(format!(
                "looking in the Trash for {}; Finder's Put Back can restore it",
                crate::shown_path(&entry.original)
            ))));
        }
    }
    if let Some(parent) = entry.original.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        return Err(Refused::Failed(
            anyhow!(error).context(format!("making {}", crate::shown_path(parent))),
        ));
    }
    match rename_no_replace(held, &entry.original) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => Err(Refused::Taken),
        // A file on another volume went to that volume's own Trash, so the
        // way back is a rename too; anything else is reported as it is.
        Err(error) => Err(Refused::Failed(anyhow!(error).context(format!(
            "moving {} back to {}",
            held.display(),
            crate::shown_path(&entry.original)
        )))),
    }
}
