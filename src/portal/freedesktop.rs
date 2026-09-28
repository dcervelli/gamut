//! The desktop's own file dialog on Linux, through the file chooser portal.
//!
//! A program on Wayland has no dialog of its own to put up: the desktop's
//! is the one the user knows, with their bookmarks and their recent places
//! in it, and `xdg-desktop-portal` is how any program asks for it. The ask
//! is one method — `org.freedesktop.portal.FileChooser.OpenFile` — and the
//! answer comes back as a signal on a request object once the user has
//! chosen, which may be minutes later. [`choose`] does the whole exchange
//! on the thread it is called on, blocking; [`choose_on_thread`] is the
//! form the window uses, which hands the answer back through the event
//! loop.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, bail};

use super::{Deliver, Pick, Picked};
use crate::dbus::{Connection, Value};
use crate::uri;

const PORTAL_NAME: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const FILE_CHOOSER: &str = "org.freedesktop.portal.FileChooser";
const REQUEST: &str = "org.freedesktop.portal.Request";

/// Numbers the requests, so that each has a handle token of its own.
static REQUESTS: AtomicU64 = AtomicU64::new(0);

/// Puts up the dialog and waits for the answer.
///
/// `Ok(None)` is the dialog dismissed; `Ok(Some(paths))` is what was
/// chosen, as local paths. The parent window is left unnamed: naming one
/// takes an exported handle from the compositor that the window toolkit
/// does not hand out, and a dialog with no parent is one the compositor
/// places itself.
///
/// `folder` is where the dialog starts, where there is somewhere to start:
/// the folder of the picture on screen, or of the last one shown.
pub fn choose(pick: Pick, folder: Option<&Path>) -> Result<Option<Vec<PathBuf>>> {
    let mut bus = Connection::session()?;
    let token = format!(
        "{}_{}_{}",
        crate::PROGRAM,
        std::process::id(),
        REQUESTS.fetch_add(1, Ordering::Relaxed)
    );
    // Where the answer will be signaled from, worked out ahead of the
    // call and subscribed to before it: the portal can answer before it
    // has returned the handle, and a signal nobody was waiting for is a
    // signal lost.
    let sender = bus.unique_name().trim_start_matches(':').replace('.', "_");
    let expected = format!("{PORTAL_PATH}/request/{sender}/{token}");
    bus.add_match(&format!(
        "type='signal',interface='{REQUEST}',member='Response',path='{expected}'"
    ))?;

    let mut options = vec![
        ("handle_token", Value::Str(token)),
        ("accept_label", Value::Str("_Open".to_string())),
    ];
    match pick {
        Pick::Files => {
            let images = image_filter();
            options.push(("multiple", Value::Bool(true)));
            options.push((
                "filters",
                Value::Array {
                    element: "(sa(us))".to_string(),
                    items: vec![images.clone(), every_file_filter()],
                },
            ));
            options.push(("current_filter", images));
        }
        Pick::Folder => options.push(("directory", Value::Bool(true))),
    }
    // A byte string the portal wants with its terminating zero, the path
    // being bytes rather than text.
    if let Some(folder) = folder {
        let bytes = folder.as_os_str().as_encoded_bytes().iter().copied();
        options.push((
            "current_folder",
            Value::Array {
                element: "y".to_string(),
                items: bytes.chain([0]).map(Value::Byte).collect(),
            },
        ));
    }
    let reply = bus
        .call(
            PORTAL_NAME,
            PORTAL_PATH,
            FILE_CHOOSER,
            "OpenFile",
            &[
                Value::Str(String::new()),
                Value::Str(pick.title().to_string()),
                Value::dict(options),
            ],
        )
        .context("asking the desktop for its file dialog")?;
    // An older portal names the request itself rather than honoring the
    // token; the answer is waited for on either path.
    let handle = reply
        .first()
        .and_then(Value::as_str)
        .map(str::to_string)
        .context("the portal returned no request handle")?;
    if handle != expected {
        bus.add_match(&format!(
            "type='signal',interface='{REQUEST}',member='Response',path='{handle}'"
        ))?;
    }
    let response = bus.wait_signal(&[&expected, &handle], REQUEST, "Response")?;
    read_response(&response)
}

/// As [`choose`], on a thread of its own, the answer handed to `deliver`
/// when it comes. Not joined: a dialog left up is up for as long as the
/// user leaves it, and the window has nothing to wait on it for.
pub fn choose_on_thread(pick: Pick, folder: Option<PathBuf>, deliver: Deliver) {
    std::thread::Builder::new()
        .name("file dialog".into())
        .spawn(move || {
            deliver(Picked {
                outcome: choose(pick, folder.as_deref()),
            });
        })
        .expect("a thread can be spawned");
}

/// What a `Response` signal's body says: the code and, on success, the
/// URIs chosen.
fn read_response(body: &[Value]) -> Result<Option<Vec<PathBuf>>> {
    let code = match body.first() {
        Some(Value::U32(code)) => *code,
        _ => bail!("the portal's response carried no code"),
    };
    match code {
        0 => {}
        1 => return Ok(None),
        _ => bail!("the desktop's file dialog failed"),
    }
    let results = body
        .get(1)
        .context("the portal's response carried no results")?;
    let uris = results
        .get("uris")
        .and_then(Value::as_array)
        .context("the portal's response named no files")?;
    let paths = uris
        .iter()
        .map(|uri| {
            uri.as_str()
                .context("a URI that was not a string")
                .and_then(uri::path_from_uri)
        })
        .collect::<Result<Vec<PathBuf>>>()?;
    if paths.is_empty() {
        return Ok(None);
    }
    Ok(Some(paths))
}

/// The filter that shows only the formats this program reads: one glob
/// per extension in each case, since a pattern matches by its letters and
/// a file may be named in either.
fn image_filter() -> Value {
    let mut patterns = Vec::new();
    for extension in crate::image::decode::supported_extensions() {
        patterns.push(pattern(&format!("*.{extension}")));
        patterns.push(pattern(&format!("*.{}", extension.to_ascii_uppercase())));
    }
    filter("Images", patterns)
}

/// The filter that shows everything, for a file named without its
/// extension, which the decoders sniff for what it holds.
fn every_file_filter() -> Value {
    filter("All files", vec![pattern("*")])
}

/// A filter as the portal spells one: its name, and its patterns.
fn filter(name: &str, patterns: Vec<Value>) -> Value {
    Value::Struct(vec![
        Value::Str(name.to_string()),
        Value::Array {
            element: "(us)".to_string(),
            items: patterns,
        },
    ])
}

/// One glob pattern of a filter: type 0 is a glob, 1 a MIME type.
fn pattern(glob: &str) -> Value {
    Value::Struct(vec![Value::U32(0), Value::Str(glob.to_string())])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The response's code decides: chosen, dismissed, or failed.
    #[test]
    fn a_response_is_read_by_its_code() {
        let results = |uris: Vec<&str>| {
            Value::dict(vec![(
                "uris",
                Value::Array {
                    element: "s".into(),
                    items: uris.into_iter().map(|u| Value::Str(u.into())).collect(),
                },
            )])
        };
        assert_eq!(
            read_response(&[
                Value::U32(0),
                results(vec!["file:///a.png", "file:///b.png"])
            ])
            .unwrap(),
            Some(vec![PathBuf::from("/a.png"), PathBuf::from("/b.png")])
        );
        assert_eq!(
            read_response(&[Value::U32(1), Value::dict(vec![])]).unwrap(),
            None
        );
        assert_eq!(
            read_response(&[Value::U32(0), results(vec![])]).unwrap(),
            None
        );
        assert!(read_response(&[Value::U32(2), Value::dict(vec![])]).is_err());
        assert!(read_response(&[Value::U32(0), Value::dict(vec![])]).is_err());
        assert!(read_response(&[]).is_err());
    }

    /// The images filter names every extension the decoders read, in both
    /// cases, and nothing else; the folder pick carries no filter at all.
    #[test]
    fn the_filter_follows_the_decoders() {
        let Value::Struct(fields) = image_filter() else {
            panic!("a filter is a struct");
        };
        assert_eq!(fields[0].as_str(), Some("Images"));
        let patterns: Vec<&str> = fields[1]
            .as_array()
            .unwrap()
            .iter()
            .map(|pattern| match pattern {
                Value::Struct(pair) => pair[1].as_str().unwrap(),
                _ => panic!("a pattern is a struct"),
            })
            .collect();
        let extensions = crate::image::decode::supported_extensions();
        assert_eq!(patterns.len(), 2 * extensions.len());
        assert!(patterns.contains(&"*.png"));
        assert!(patterns.contains(&"*.PNG"));
        assert!(patterns.contains(&"*.jxl"));
    }
}
