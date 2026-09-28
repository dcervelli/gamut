//! What else can open the file on screen, on macOS: the applications Launch
//! Services says can open it — the list Finder's "Open With" shows, the
//! default first — and starting one of them on it through the workspace,
//! which leaves the application to launchd rather than to this process.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSWorkspace, NSWorkspaceOpenConfiguration};
use objc2_foundation::{NSArray, NSBundle, NSFileManager, NSString, NSURL};

use super::shortened;
use crate::APP_ID;

/// An application Launch Services says can open a file of this kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opener {
    /// What Finder calls it, and what the menu wears. Control characters are
    /// taken out as it is read: it is somebody else's text on its way into
    /// this window.
    pub name: String,
    /// Where the application's bundle is.
    application: PathBuf,
}

/// The applications that can open `path`, the default first; this program
/// is never among them.
pub fn for_file(path: &Path) -> Vec<Opener> {
    autoreleasepool(|_| {
        let Some(url) = NSURL::from_file_path(path) else {
            return Vec::new();
        };
        let workspace = NSWorkspace::sharedWorkspace();
        let default = workspace.URLForApplicationToOpenURL(&url);
        let mut applications: Vec<PathBuf> = default
            .into_iter()
            .chain(workspace.URLsForApplicationsToOpenURL(&url).to_vec())
            .filter(|application| !is_this_program(application))
            .filter_map(|application| application.to_file_path())
            .collect();
        let mut seen = Vec::new();
        applications.retain(|application| {
            let first = !seen.contains(application);
            seen.push(application.clone());
            first
        });
        let files = NSFileManager::defaultManager();
        applications
            .into_iter()
            .map(|application| {
                let shown = files
                    .displayNameAtPath(&NSString::from_str(&application.to_string_lossy()))
                    .to_string();
                let name = shown.strip_suffix(".app").unwrap_or(&shown);
                Opener {
                    name: shortened(&crate::escape_controls(name)),
                    application,
                }
            })
            .collect()
    })
}

/// Whether the application at `url` is this program itself, by the identifier
/// its bundle carries.
fn is_this_program(url: &NSURL) -> bool {
    NSBundle::bundleWithURL(url)
        .and_then(|bundle| bundle.bundleIdentifier())
        .is_some_and(|identifier| identifier.to_string() == APP_ID)
}

/// Opens `path` in the application `opener` names. Returns once the request
/// is made; the application arrives in its own time.
pub fn open(opener: &Opener, path: &Path) -> Result<()> {
    autoreleasepool(|_| {
        let file = NSURL::from_file_path(path)
            .with_context(|| format!("naming {}", crate::shown_path(path)))?;
        let application = NSURL::from_directory_path(&opener.application)
            .with_context(|| format!("naming {}", opener.application.display()))?;
        NSWorkspace::sharedWorkspace()
            .openURLs_withApplicationAtURL_configuration_completionHandler(
                &NSArray::from_retained_slice(&[file]),
                &application,
                &NSWorkspaceOpenConfiguration::configuration(),
                None,
            );
        Ok(())
    })
}
