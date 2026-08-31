//! Putting text on the system clipboard.
//!
//! Wayland has no clipboard of its own. The selection is a promise by the
//! window that made it to hand the bytes over when someone later asks for
//! them, so it lasts exactly as long as that window does — copying and then
//! quitting would leave the paste with nobody to answer it.
//!
//! So the promise is made by somebody else: a second copy of this program,
//! started with [`SERVE_ARGUMENT`], which takes the text on its standard
//! input, owns the selection and answers pastes until the compositor cancels
//! it — which is when something else copies. It is not waited for, and it
//! outlives the window that asked for it. This is what `wl-copy` does, for
//! the same reason.

use std::ffi::{OsStr, OsString};
use std::io::Write as _;
use std::os::unix::ffi::OsStrExt as _;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use anyhow::{Context, Result};
use wl_clipboard_rs::copy::{MimeType, Options, Source};

/// The argument the serving process is started with, followed by the MIME
/// type to offer. Not a command-line option: it is how this program re-runs
/// itself, and `--help` does not mention it.
pub const SERVE_ARGUMENT: &str = "--serve-clipboard";

/// Words. Offering this brings `text/plain;charset=utf-8`, `STRING`,
/// `UTF8_STRING` and `TEXT` with it, so that a reader gets the text under
/// whichever name it knows.
pub const TEXT: &str = "text/plain";

/// A file, named rather than spelled out: what a file manager, a browser or
/// another program's open dialog asks for when it wants the file itself
/// rather than the words. The plain-text names come with it, as they do from
/// `wl-copy`, so a paste into a text field still yields the URI.
pub const URI_LIST: &str = "text/uri-list";

/// The picture itself. Nothing text-like is offered alongside this one.
pub const PNG: &str = "image/png";

/// The `file:` URI naming `path`, which must be absolute.
///
/// Every byte outside RFC 3986's unreserved set is percent-encoded, the
/// separator apart. Encoding more than strictly necessary is always correct —
/// it decodes back to the same bytes — where guessing which sub-delimiters a
/// given reader tolerates unescaped is not. The bytes are the path's own, so
/// a name that is not valid UTF-8 survives the round trip.
pub fn file_uri(path: &Path) -> String {
    debug_assert!(path.is_absolute(), "a file URI needs an absolute path");
    let mut uri = String::from("file://");
    for &byte in path.as_os_str().as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                uri.push(char::from(byte));
            }
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

/// `path` as a whole `text/uri-list` body: one URI, and the CRLF that RFC
/// 2483 ends every line of one with.
pub fn uri_list(path: &Path) -> String {
    format!("{}\r\n", file_uri(path))
}

/// Puts `content` on the clipboard under `mime_type`, and returns the process
/// that will keep it there. The caller holds on to the handle only to reap it
/// once it exits; dropping it leaves the process running, which is the point.
pub fn copy(content: &[u8], mime_type: &str) -> Result<Child> {
    let program = std::env::current_exe().context("finding this program's own path")?;
    // Down the pipe rather than in an argument: what is copied runs from a
    // path to a whole PNG, is longer than the argument list allows well
    // before that, and need not be text at all. Standard error is left as it
    // is, so that a failure on the far side is reported where every other
    // message goes.
    let mut child = Command::new(program)
        .args([SERVE_ARGUMENT, mime_type])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .context("starting the process that holds the clipboard")?;
    let mut stdin = child.stdin.take().expect("stdin was asked for as a pipe");
    let written = stdin
        .write_all(content)
        .and_then(|()| stdin.flush())
        .context("handing the content to the clipboard process");
    // Closed here rather than at the end of the call: the far side reads to
    // end of file before it offers anything, so it would otherwise wait for
    // a pipe that is still open on this side.
    drop(stdin);
    written?;
    Ok(child)
}

/// The [`SERVE_ARGUMENT`] half: takes the content on standard input, offers
/// it as the selection under `mime_type`, and stays to answer pastes. Returns
/// when the selection has been taken over by somebody else.
pub fn serve(mime_type: Option<OsString>) -> Result<()> {
    let mime_type = mime_type
        .as_deref()
        .and_then(OsStr::to_str)
        .context("`--serve-clipboard` needs the MIME type to offer the content under")?
        .to_owned();
    let mut options = Options::new();
    // In the foreground because there is nothing else for this process to do,
    // and because `prepare_copy` requires it. Nothing is trimmed: a trailing
    // newline is part of a filename where a file has one, and a PNG is not
    // text to be tidied at all.
    options.foreground(true).trim_newline(false);
    options
        .prepare_copy(Source::StdIn, MimeType::Specific(mime_type))
        .context("offering the text to the compositor")?
        .serve()
        .context("serving the clipboard")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn a_plain_path_needs_no_escaping() {
        assert_eq!(
            file_uri(Path::new("/home/me/pictures/sunset_01.png")),
            "file:///home/me/pictures/sunset_01.png"
        );
    }

    /// Spaces, the reserved characters and anything above ASCII, all of which
    /// a reader would otherwise take for punctuation of the URI itself.
    #[test]
    fn everything_else_is_percent_encoded() {
        assert_eq!(
            file_uri(Path::new("/tmp/a b#c?d%e.png")),
            "file:///tmp/a%20b%23c%3Fd%25e.png"
        );
        assert_eq!(
            file_uri(Path::new("/tmp/caf\u{e9}.jpg")),
            "file:///tmp/caf%C3%A9.jpg"
        );
    }

    /// A name the filesystem allows and UTF-8 does not still names a file,
    /// and still has to reach the other program.
    #[test]
    fn a_name_that_is_not_utf8_survives() {
        let path = PathBuf::from(OsStr::from_bytes(b"/tmp/\xff.png"));
        assert_eq!(file_uri(&path), "file:///tmp/%FF.png");
    }

    #[test]
    fn a_uri_list_is_one_crlf_terminated_line() {
        assert_eq!(uri_list(Path::new("/tmp/a.png")), "file:///tmp/a.png\r\n");
    }
}
