//! The desktop's trash: moving a file into it, and moving one back out. A
//! file moved there shows up in the file manager's Trash beside everything
//! else the user has thrown away, and can be restored or emptied from there.
//! How that is done is the platform's — `freedesktop.rs` on Linux and
//! `macos.rs` on a Mac, which keeps the freedesktop layout for the tests.

// On a Mac, only a trash in a directory of the tests' own is laid out this
// way, so the half that finds the user's trash is never called there.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod freedesktop;
#[cfg(target_os = "linux")]
pub use freedesktop::{Entry, Trash, restore};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{Entry, Trash, restore};

/// Why a file was not replaced by another.
#[derive(Debug)]
pub enum Unreplaced {
    /// Nothing changed: the file is where it was, as it was.
    Untouched(anyhow::Error),
    /// The file went to the trash, and what was to take its place did not
    /// arrive, nor could the file be brought back: it is in the trash. Only
    /// Finder's trash, which moves a file rather than keeping a copy of it,
    /// can leave one there.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    InTrash(anyhow::Error),
}

/// Why a restore did not happen.
#[derive(Debug)]
pub enum Refused {
    /// The trash no longer holds the file: it was emptied, or restored
    /// from elsewhere.
    Gone,
    /// Something else now stands where the file came from, and a restore
    /// does not overwrite.
    Taken,
    Failed(anyhow::Error),
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::Gone => write!(f, "the trash no longer holds it"),
            Refused::Taken => write!(f, "something else is there now"),
            Refused::Failed(error) => write!(f, "{error:#}"),
        }
    }
}
