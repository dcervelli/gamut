//! The desktop's trash: moving a file into it, and moving one back out. A
//! file moved there shows up in the file manager's Trash beside everything
//! else the user has thrown away, and can be restored or emptied from there.
//! How that is done is the platform's — `freedesktop.rs` on Linux.

#[cfg(target_os = "linux")]
mod freedesktop;
#[cfg(target_os = "linux")]
pub use freedesktop::{Entry, Trash, restore};

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
