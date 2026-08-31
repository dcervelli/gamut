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

use std::io::Write as _;
use std::process::{Child, Command, Stdio};

use anyhow::{Context, Result};
use wl_clipboard_rs::copy::{MimeType, Options, Source};

/// The argument the serving process is started with. Not a command-line
/// option: it is how this program re-runs itself, and `--help` does not
/// mention it.
pub const SERVE_ARGUMENT: &str = "--serve-clipboard";

/// Puts `text` on the clipboard, and returns the process that will keep it
/// there. The caller holds on to the handle only to reap it once it exits;
/// dropping it leaves the process running, which is the point.
pub fn copy_text(text: &str) -> Result<Child> {
    let program = std::env::current_exe().context("finding this program's own path")?;
    // Down the pipe rather than in an argument: a path can be longer than the
    // argument list allows, and need not be valid UTF-8 to be spelled out on
    // a command line. Standard error is left as it is, so that a failure on
    // the far side is reported where every other message goes.
    let mut child = Command::new(program)
        .arg(SERVE_ARGUMENT)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .context("starting the process that holds the clipboard")?;
    let mut stdin = child.stdin.take().expect("stdin was asked for as a pipe");
    let written = stdin
        .write_all(text.as_bytes())
        .and_then(|()| stdin.flush())
        .context("handing the text to the clipboard process");
    // Closed here rather than at the end of the call: the far side reads to
    // end of file before it offers anything, so it would otherwise wait for
    // a pipe that is still open on this side.
    drop(stdin);
    written?;
    Ok(child)
}

/// The [`SERVE_ARGUMENT`] half: takes the text on standard input, offers it
/// as the selection, and stays to answer pastes. Returns when the selection
/// has been taken over by somebody else.
pub fn serve() -> Result<()> {
    let mut options = Options::new();
    // In the foreground because there is nothing else for this process to do,
    // and because `prepare_copy` requires it. A path is copied as it is: a
    // trailing newline is part of a filename where a file has one.
    options.foreground(true).trim_newline(false);
    options
        .prepare_copy(Source::StdIn, MimeType::Text)
        .context("offering the text to the compositor")?
        .serve()
        .context("serving the clipboard")?;
    Ok(())
}
