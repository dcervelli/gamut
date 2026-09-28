//! Putting things on the system clipboard, and taking a picture off it.
//!
//! What is copied and what a paste is taken as are the same everywhere, and
//! are here: the types, the table of pictures a paste may be, and the thread
//! that watches for one. How the clipboard is spoken to is the platform's —
//! `wayland.rs` on Linux.
//!
//! Reading always belongs to somebody else, who is asked for the selection
//! under one of the types they said they could produce. How long that takes
//! is therefore theirs to decide, which is why [`receive`] is called from the
//! loader thread rather than from the event loop — see [`crate::loader`].

use std::path::Path;
use std::time::Duration;

#[cfg(target_os = "linux")]
mod wayland;
#[cfg(target_os = "linux")]
pub use wayland::{SERVE_ARGUMENT, copy, offered_image, receive, serve};

/// Words. On Wayland, offering this brings `text/plain;charset=utf-8`,
/// `STRING`, `UTF8_STRING` and `TEXT` with it, so that a reader gets the
/// text under whichever name it knows.
pub const TEXT: &str = "text/plain";

/// A file, named rather than spelled out: what a file manager, a browser or
/// another program's open dialog asks for when it wants the file itself
/// rather than the words. The plain-text names come with it, as they do from
/// `wl-copy`, so a paste into a text field still yields the URI.
pub const URI_LIST: &str = "text/uri-list";

/// The picture itself. Nothing text-like is offered alongside this one.
pub const PNG: &str = "image/png";

/// `path` as a whole `text/uri-list` body: one URI, and the CRLF that RFC
/// 2483 ends every line of one with.
pub fn uri_list(path: &Path) -> String {
    format!("{}\r\n", crate::uri::file(path))
}

/// The image types this program will take a paste under, each with the
/// extension a file holding it is named by.
///
/// Every extension here is one the decoder registry reads, which is what
/// makes the list short: a type offered under a name we could not open again
/// is a type there is no point asking for. Nothing text-like belongs here
/// either — a copied filename is words about a picture, not a picture.
pub(super) const IMAGE_TYPES: &[(&str, &str)] = &[
    ("image/png", "png"),
    ("image/jpeg", "jpg"),
    ("image/jpg", "jpg"),
    ("image/webp", "webp"),
    ("image/tiff", "tif"),
    ("image/avif", "avif"),
    ("image/heic", "heic"),
    ("image/heif", "heic"),
    ("image/gif", "gif"),
    ("image/bmp", "bmp"),
    ("image/x-bmp", "bmp"),
    ("image/x-ms-bmp", "bmp"),
    ("image/x-icon", "ico"),
    ("image/vnd.microsoft.icon", "ico"),
    ("image/x-exr", "exr"),
    ("image/x-portable-pixmap", "ppm"),
    ("image/x-portable-anymap", "pnm"),
    ("image/vnd.radiance", "hdr"),
];

/// Watches the clipboard for a picture this program could show, from a
/// thread of its own, and calls `notify` with the answer each time it
/// changes — `true` when one has arrived, `false` when it has gone. Asked
/// every `interval` rather than waited for, since nothing tells a program
/// that the selection has changed; on a thread rather than on the loop,
/// since one look may be a round trip to another process, and one slow to
/// answer must not stall the window. The thread stops once `notify`
/// says nobody is listening, and is otherwise left to die with the process.
pub fn watch(interval: Duration, notify: impl Fn(bool) -> bool + Send + 'static) {
    let spawned = std::thread::Builder::new()
        .name("gamut clipboard".into())
        .spawn(move || {
            let mut offered = false;
            loop {
                let now = matches!(offered_image(), Ok(Some(_)));
                if now != offered {
                    offered = now;
                    if !notify(offered) {
                        return;
                    }
                }
                std::thread::sleep(interval);
            }
        });
    if let Err(error) = spawned {
        eprintln!("gamut: could not watch the clipboard: {error}");
    }
}

/// A picture the clipboard is offering: the MIME type to ask for it under,
/// and what a file holding it should be called.
pub struct Offer {
    pub mime: String,
    pub extension: &'static str,
}

/// The most a paste may write, which is the largest image this build could
/// display even in principle — 32768 x 32768 at 32 bits, the ceiling
/// `image::decode` holds every file to. No compressed stream that decodes to
/// less can be longer than that, so anything past it is not a picture we were
/// ever going to show, and stopping there is what keeps a source that never
/// stops writing from filling the disk.
pub(super) const MAX_PASTE_BYTES: u64 = 4 * 1024 * 1024 * 1024;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_uri_list_is_one_crlf_terminated_line() {
        assert_eq!(uri_list(Path::new("/tmp/a.png")), "file:///tmp/a.png\r\n");
    }

    /// A paste is written to a file and then opened again, so a type offered
    /// under an extension no decoder claims would be saved and never shown.
    #[test]
    fn every_type_a_paste_is_taken_under_can_be_read_back() {
        let readable = crate::image::decode::supported_extensions();
        for (mime, extension) in IMAGE_TYPES {
            assert!(
                readable.contains(extension),
                "{mime} is taken as .{extension}, which no decoder reads"
            );
            assert_eq!(
                *mime,
                mime.to_ascii_lowercase(),
                "types are compared without case, so the table is written in one"
            );
        }
    }

    /// The types a paste is taken under are the desktop's names for the
    /// same files: each is among what `media` says the desktop calls a
    /// file of that extension, so the two tables cannot drift apart.
    #[test]
    fn every_type_a_paste_is_taken_under_is_one_the_desktop_calls_it() {
        for (mime, extension) in IMAGE_TYPES {
            let known = crate::media::MIME_TYPES
                .iter()
                .find(|(known, _)| known == extension)
                .map(|(_, types)| *types)
                .unwrap_or_else(|| panic!("{extension} is not in media::MIME_TYPES"));
            assert!(
                known.contains(mime),
                "{mime} is not a name for .{extension}"
            );
        }
    }
}
