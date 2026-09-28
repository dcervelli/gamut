//! The clipboard on Wayland.
//!
//! Wayland has no clipboard of its own. The selection is a promise by the
//! window that made it to hand the bytes over when someone later asks for
//! them, so it lasts exactly as long as that window does — copying and then
//! quitting would leave the paste with nobody to answer it.
//!
//! So the promise is made by somebody else: a second copy of this program,
//! started with [`SERVE_ARGUMENT`], which takes the content on its standard
//! input, owns the selection and answers pastes until the compositor cancels
//! it — which is when something else copies. It is not waited for in line,
//! and it outlives the window that asked for it. This is what `wl-copy` does,
//! for the same reason.
//!
//! Reading is the other way round and needs no such trick: the selection
//! belongs to somebody else, who answers down a pipe.

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{Read as _, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use wl_clipboard_rs::copy::{MimeType, Options, Source};
use wl_clipboard_rs::paste::{self, ClipboardType, Error as PasteError, Seat};

use super::{IMAGE_TYPES, MAX_PASTE_BYTES, Offer};

/// The argument the serving process is started with, followed by the MIME
/// type to offer. Not a command-line option: it is how this program re-runs
/// itself, and `--help` does not mention it.
pub const SERVE_ARGUMENT: &str = "--serve-clipboard";

/// Puts `content` on the clipboard under `mime_type`.
///
/// Returns as soon as the content has been handed over, the process that
/// holds it being left to run. Nobody is waiting on it in line — a thread of
/// its own does that, and does nothing else — so this costs a spawn and a
/// write, whatever is being copied and however long it stays copied.
pub fn copy(content: &[u8], mime_type: &str) -> Result<()> {
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

    // Somebody has to collect it when the compositor finally cancels the
    // selection, or every copy in a session leaves a zombie behind. A thread
    // parked in `wait` is the cheapest place to do that, and it goes away
    // with the process if the window is closed first — leaving the child to
    // be adopted and to go on serving, which is the whole point of it.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
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

/// What the clipboard is offering that this program could show, and `None`
/// when it is offering nothing of the sort — which covers an empty clipboard,
/// a clipboard holding words, and a seat that has no selection at all. Those
/// are answers rather than failures: the user pressed a key, and there was
/// nothing there.
///
/// A source is expected to offer conversions as well as whatever it actually
/// holds — a browser copying a PNG will offer JPEG and WebP alongside it, and
/// make them on demand — and there is no way to ask which is which. What
/// there is, is the order: a source that distinguishes them at all puts its
/// own first. So the first offer this can read wins, rather than a preference
/// of ours that would ask half the desktop to re-encode a picture it already
/// had.
pub fn offered_image() -> Result<Option<Offer>> {
    let offered = match paste::get_mime_types_ordered(ClipboardType::Regular, Seat::Unspecified) {
        Ok(offered) => offered,
        Err(PasteError::NoSeats | PasteError::ClipboardEmpty | PasteError::NoMimeType) => {
            return Ok(None);
        }
        Err(error) => return Err(error).context("asking what is on the clipboard"),
    };
    Ok(offered.into_iter().find_map(|mime| {
        let (_, extension) = IMAGE_TYPES
            .iter()
            .find(|(known, _)| known.eq_ignore_ascii_case(&mime))?;
        Some(Offer { mime, extension })
    }))
}

/// Writes the selection, as `mime_type`, into the file already made at
/// `path`. Returns how many bytes it held.
///
/// Streamed rather than held: a paste is written where it is going as it
/// arrives, so that a picture large enough to matter is never in memory twice
/// over. The far side is another program, which may take as long as it likes
/// about answering — this belongs on a thread with the rest of the reading.
pub fn receive(mime_type: &str, path: &Path) -> Result<u64> {
    let (reader, offered) = paste::get_contents(
        ClipboardType::Regular,
        Seat::Unspecified,
        paste::MimeType::Specific(mime_type),
    )
    .with_context(|| format!("asking the clipboard for {mime_type}"))?;
    // What was offered a moment ago and what is offered now need not be the
    // same thing: the type was chosen from one look at the selection and
    // asked for on another, and something else may have copied in between.
    if offered != mime_type {
        bail!("the clipboard now holds {offered} rather than {mime_type}");
    }

    let mut file = File::create(path)
        .with_context(|| format!("opening {} to write", crate::shown_path(path)))?;
    let written = std::io::copy(&mut reader.take(MAX_PASTE_BYTES), &mut file)
        .with_context(|| format!("writing {}", crate::shown_path(path)))?;
    if written == MAX_PASTE_BYTES {
        bail!(
            "the clipboard offered more than the {:.1} GB this build will hold",
            MAX_PASTE_BYTES as f64 / 1e9
        );
    }
    if written == 0 {
        bail!("the clipboard offered {mime_type} and then handed over nothing");
    }
    file.flush()
        .with_context(|| format!("writing {}", crate::shown_path(path)))?;
    Ok(written)
}
