//! The desktop's own file dialog on macOS: `NSOpenPanel`, the dialog every
//! Mac program opens files with, its sidebar and recent places included.
//!
//! AppKit puts a panel up on the main thread only, which is the thread the
//! window asks from, so the panel is run there, modally, and its answer
//! handed to [`Deliver`] before this returns — the same way back into the
//! event loop the Linux dialog's answer takes from its thread.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use objc2::MainThreadMarker;
use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSModalResponseOK, NSOpenPanel};
use objc2_foundation::{NSArray, NSString, NSURL};
use objc2_uniform_type_identifiers::UTType;

use super::{Deliver, Pick, Picked};

/// Puts up the dialog and hands the answer to `deliver`: what the user
/// chose, or `None` for a dialog dismissed without choosing.
///
/// `folder` is where the dialog starts, where there is somewhere to start.
pub fn choose_on_thread(pick: Pick, folder: Option<PathBuf>, deliver: Deliver) {
    let outcome = choose(pick, folder.as_deref());
    deliver(Picked { outcome });
}

fn choose(pick: Pick, folder: Option<&Path>) -> Result<Option<Vec<PathBuf>>> {
    let main = MainThreadMarker::new().context("the file dialog is put up from the main thread")?;
    autoreleasepool(|_| {
        let panel = NSOpenPanel::openPanel(main);
        let files = pick == Pick::Files;
        panel.setCanChooseFiles(files);
        panel.setCanChooseDirectories(!files);
        panel.setAllowsMultipleSelection(files);
        panel.setMessage(Some(&NSString::from_str(pick.title())));
        panel.setPrompt(Some(&NSString::from_str("Open")));
        if files {
            // Only the formats this program reads, by the types their
            // extensions stand for; an extension the system has no type for
            // is left out of the filter rather than failing it.
            let types: Vec<_> = crate::image::decode::supported_extensions()
                .iter()
                .filter_map(|extension| {
                    UTType::typeWithFilenameExtension(&NSString::from_str(extension))
                })
                .collect();
            panel.setAllowedContentTypes(&NSArray::from_retained_slice(&types));
        }
        if let Some(folder) = folder.and_then(NSURL::from_directory_path) {
            panel.setDirectoryURL(Some(&folder));
        }
        if panel.runModal() != NSModalResponseOK {
            return Ok(None);
        }
        let paths: Vec<PathBuf> = panel
            .URLs()
            .iter()
            .filter_map(|url| url.to_file_path())
            .collect();
        Ok((!paths.is_empty()).then_some(paths))
    })
}
