//! The desktop's own file dialog: [`choose_on_thread`] puts it up and hands
//! the answer back through the event loop. How it is asked for is the
//! platform's — `freedesktop.rs` on Linux, through the portal.
//!
//! A dialog picks files or it picks a folder, never both in one — every
//! desktop's dialog is built that way — so [`Pick`] says which is being asked
//! for and the window offers the two as two buttons.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;

#[cfg(target_os = "linux")]
mod freedesktop;
#[cfg(target_os = "linux")]
pub use freedesktop::choose_on_thread;

/// What the dialog is to pick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pick {
    /// One or more image files, the dialog's list narrowed to the formats
    /// this program reads.
    Files,
    /// A folder, to stand for the images inside it as a directory on the
    /// command line does.
    Folder,
}

impl Pick {
    /// The dialog's title.
    pub(super) fn title(self) -> &'static str {
        match self {
            Pick::Files => "Open images",
            Pick::Folder => "Open a folder of images",
        }
    }
}

/// What came of a dialog: what the user chose, or `None` for a dialog
/// dismissed without choosing.
pub struct Picked {
    pub outcome: Result<Option<Vec<PathBuf>>>,
}

/// How a dialog's answer reaches the window: what `main` made from the
/// event loop's proxy.
pub type Deliver = Arc<dyn Fn(Picked) + Send + Sync>;
