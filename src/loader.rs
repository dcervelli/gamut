//! Reading files off the event loop.
//!
//! One thread, one file at a time, newest request wins. Everything that costs
//! time in proportion to the pixel count happens there — the decode, the
//! statistics scan, the repack into a texture format and the copy to the GPU —
//! so that the event loop is free to pan, zoom and draw while a file opens.
//! A pasted picture is fetched here too: it is one more thing a read may have
//! to wait on somebody else for, and the waiting belongs off the event loop
//! with the rest.
//!
//! Replies come back as winit user events rather than through a channel the
//! event loop would have to poll: the loop is asleep almost all of the time,
//! and a proxy wakes it the moment an image is ready instead of leaving it to
//! be noticed at the next file-watch tick.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use anyhow::{Result, anyhow};

use crate::clipboard;
use crate::image::decode::{self, Overrides};
use crate::image::exif::Exif;
use crate::image::sequence::Sequence;
use crate::image::{DecodedImage, Stats};
use crate::render::{GpuImage, Upload};
use crate::timing;
use crate::watch::Watch;

/// Why a file is being read, which decides what survives the reading.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Reload {
    /// A different file: display settings start over, and so does the view
    /// unless the new file happens to be the same size as the old one.
    Fresh,
    /// The file already on screen, changed on disk. The user is presumably
    /// looking at something in particular, so what they set up stays.
    InPlace,
    /// The file already on screen, another of the pictures it holds: an
    /// entry of an ICO, a directory of a TIFF. What they set up stays, as
    /// for a file changed on disk.
    Page,
}

/// Where a read's bytes come from.
pub enum Source {
    /// The file itself, which is already on disk.
    Disk,
    /// The clipboard, under this MIME type. The selection is written into the
    /// file first and then read back like any other, so a pasted picture is a
    /// file the user keeps rather than pixels that exist only while the
    /// window is open.
    ///
    /// Here rather than on the event loop because fetching it means waiting
    /// on whichever program holds the selection, and how long that takes is
    /// theirs to decide.
    Clipboard(String),
}

/// One file to read.
pub struct Request {
    /// Which request this is. Replies carry it back so the event loop can tell
    /// the answer it is waiting for from one it has already stepped past.
    pub generation: u64,
    pub index: usize,
    pub path: PathBuf,
    pub overrides: Overrides,
    pub mode: Reload,
    pub source: Source,
    /// Which of the file's pictures, where it holds several. `None` is the
    /// one the decoder shows first.
    pub page: Option<usize>,
}

/// A finished read, whether or not it produced an image.
pub struct Decoded {
    pub generation: u64,
    pub file: Opened,
    pub outcome: Result<Ready>,
}

/// Which file a reply is about, and what it looked like when we opened it.
pub struct Opened {
    pub index: usize,
    pub path: PathBuf,
    pub mode: Reload,
    /// Taken immediately before the file was read rather than after: a write
    /// that lands while we are decoding then shows up as another change,
    /// instead of being recorded as the version we are holding.
    pub watch: Watch,
}

/// An image ready to go on screen, with the work already done.
pub struct Ready {
    pub image: DecodedImage,
    pub stats: Stats,
    /// What the file says about the photograph, for the info panel. Read
    /// here rather than on the event loop because it is one more parse of a
    /// file whoever wrote it chose the bytes of, and this is the thread with
    /// the guard around it.
    pub exif: Exif,
    /// The texture, when the window was open in time to give us somewhere to
    /// put it. `None` only for a file requested before the renderer existed,
    /// which the event loop then uploads itself.
    pub gpu: Option<GpuImage>,
    /// What else the file holds: frames to play, or pages to step through.
    pub sequence: Sequence,
    /// Which page `image` is, where the file has pages; zero otherwise.
    pub page: usize,
}

/// The handle the event loop keeps. Dropping it stops the thread and waits
/// for it, which the process cannot leave `main` without: see [`Loader::drop`].
pub struct Loader {
    /// Taken on the way out. The thread returns only once its end of the
    /// channel fails, so shutting down means dropping this before waiting.
    commands: Option<Sender<Command>>,
    thread: Option<JoinHandle<()>>,
    /// Asks a read under way to stop between stages rather than see itself
    /// out, so that quitting does not wait on work nobody will look at.
    canceled: Arc<AtomicBool>,
}

enum Command {
    Load(Request),
    /// The renderer exists; from here on the thread can upload as well as
    /// decode. Sent once, when the window opens.
    Attach(Upload),
}

impl Loader {
    /// `deliver` is where each finished file goes, called on the loader's
    /// thread; it answers `false` once nobody is listening, which stops the
    /// thread.
    pub fn new(deliver: impl FnMut(Decoded) -> bool + Send + 'static) -> Self {
        let (commands, incoming) = mpsc::channel();
        let canceled = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&canceled);
        let thread = thread::Builder::new()
            .name("gamut loader".into())
            .spawn(move || run(incoming, deliver, &flag))
            .expect("the loader thread can be spawned");
        Self {
            commands: Some(commands),
            thread: Some(thread),
            canceled,
        }
    }

    /// Asks for a file. Returns at once; the answer arrives as a user event.
    pub fn request(&self, request: Request) {
        self.send(Command::Load(request));
    }

    /// Hands over the GPU side, so that later reads arrive uploaded.
    pub fn attach(&self, upload: Upload) {
        self.send(Command::Attach(upload));
    }

    fn send(&self, command: Command) {
        // No channel means we are shutting down, and a closed one means the
        // thread has already gone. Nothing is waiting on a reply by then.
        if let Some(commands) = &self.commands {
            let _ = commands.send(command);
        }
    }

    /// A loader that answers nothing, for tests that hand replies to the
    /// application directly rather than round-tripping through a thread and an
    /// event loop they have no way to run.
    #[cfg(test)]
    pub fn detached() -> Self {
        let (commands, _) = mpsc::channel();
        Self {
            commands: Some(commands),
            thread: None,
            canceled: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Drop for Loader {
    /// Waits for the thread before letting the process go.
    ///
    /// The [`Upload`] handed over at start-up holds the wgpu device, so the
    /// thread can be the last owner of it. Detached, it would then be calling
    /// into the graphics driver to destroy the device at the same moment the
    /// main thread is running that driver's own `atexit` handlers on its way
    /// out of `main` — two threads dismantling the same global state, which
    /// the NVIDIA driver answers with a null dereference. Joining removes the
    /// race outright: nothing else runs while we wait, and by the time the
    /// process leaves `main` the thread is gone.
    fn drop(&mut self) {
        self.canceled.store(true, Ordering::Relaxed);
        // The channel goes first. The thread returns only when `recv` fails,
        // so joining while we still held the sender would wait for ever.
        self.commands = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(
    incoming: Receiver<Command>,
    mut deliver: impl FnMut(Decoded) -> bool,
    canceled: &AtomicBool,
) {
    let mut upload = None;
    let mut queued: Option<Request> = None;

    loop {
        // Block for one command, then take everything else already waiting.
        // A request overtaken while we were busy is a file the user has
        // stepped past, and decoding it would only delay the one they are
        // actually waiting for — so only the newest survives the drain.
        let Ok(first) = incoming.recv() else {
            return;
        };
        absorb(first, &mut queued, &mut upload);
        while let Ok(next) = incoming.try_recv() {
            absorb(next, &mut queued, &mut upload);
        }

        // Between commands rather than only at `recv`: a request sent just
        // before the sender was dropped is still sitting in the channel, and
        // reading it would hold up the quit for a file nobody will see.
        if canceled.load(Ordering::Relaxed) {
            return;
        }

        if let Some(request) = queued.take() {
            let Some(decoded) = read(request, upload.as_ref(), canceled) else {
                return;
            };
            // A closed loop means the window has gone; stop rather than
            // decode for nobody.
            if !deliver(decoded) {
                return;
            }
        }
    }
}

fn absorb(command: Command, queued: &mut Option<Request>, upload: &mut Option<Upload>) {
    match command {
        Command::Load(request) => *queued = Some(request),
        Command::Attach(handle) => *upload = Some(handle),
    }
}

/// Runs one fallible stage, turning a panic into an error rather than letting
/// it unwind the loader thread. A panic elsewhere is a bug and still aborts;
/// this is only for the decoders, which must survive a hostile file.
pub(crate) fn guard<T>(stage: &str, work: impl FnOnce() -> Result<T>) -> Result<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
        Ok(result) => result,
        Err(payload) => {
            let detail = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "no further detail".to_string());
            Err(anyhow!("panicked while {stage}: {detail}"))
        }
    }
}

/// Reads one file, or gives up and returns `None` if the application went
/// away while it was working.
///
/// Cancellation is checked between the stages rather than inside them: no
/// decoder here can be interrupted part way through, but the two stages after
/// the decode need never be started — and the upload in particular must not
/// reach for a device the main thread is on its way to destroying.
fn read(request: Request, upload: Option<&Upload>, canceled: &AtomicBool) -> Option<Decoded> {
    let Request {
        generation,
        index,
        path,
        overrides,
        mode,
        source,
        page,
    } = request;

    // A paste has to be fetched before there is a file to read at all. The
    // watch is taken after it, so that the file the bars describe is the one
    // that now exists rather than the empty name it was reserved under.
    let received = match &source {
        Source::Disk => Ok(()),
        Source::Clipboard(mime) => fetch(mime, &path),
    };
    let watch = Watch::new(&path);
    let started = Instant::now();
    // Each stage runs behind a panic guard. Decoders here run C and Rust
    // library code on bytes chosen by whoever wrote the file, and a panic in
    // one of them would otherwise unwind the whole thread: the loader would
    // then answer nothing ever again, and the window would sit in "loading"
    // for good. Caught, a panic becomes an ordinary decode failure, which the
    // event loop already knows how to step over.
    // What the file holds first, so that a page asked for by number is read
    // of a file known to have it, and the default page is known by name.
    let sequence = received.and_then(|()| guard("reading the header", || decode::sequence(&path)));
    let decoded = sequence.and_then(|sequence| {
        let shown = match (page, sequence) {
            (Some(page), _) => page,
            (None, Sequence::Pages { default, .. }) => default,
            (None, _) => 0,
        };
        let image = guard("decoding", || match page {
            Some(page) => decode::load_page(&path, overrides, page),
            None => decode::load(&path, overrides),
        })?;
        Ok((image, sequence, shown))
    });
    if canceled.load(Ordering::Relaxed) {
        return None;
    }

    let scanned = decoded.and_then(|(image, sequence, page)| {
        timing::decoded(&path, started.elapsed());
        let stats = guard("scanning", || Ok(Stats::scan(&image)))?;
        // A file with no metadata, or with metadata that will not parse, is
        // not a failure: the panel simply has less to say about it.
        let exif = guard("reading the metadata", || Ok(Exif::read(&path)))?;
        Ok((image, stats, exif, sequence, page))
    });
    if canceled.load(Ordering::Relaxed) {
        return None;
    }

    let outcome = scanned.and_then(|(image, stats, exif, sequence, page)| {
        let gpu = match upload {
            Some(upload) => Some(guard("uploading to the GPU", || upload.run(&image))?),
            None => None,
        };
        Ok(Ready {
            image,
            stats,
            exif,
            gpu,
            sequence,
            page,
        })
    });

    Some(Decoded {
        generation,
        file: Opened {
            index,
            path,
            mode,
            watch,
        },
        outcome,
    })
}

/// Writes the selection into the file reserved for it, and clears the name
/// again if it could not be filled.
///
/// The empty file was made to settle which paste owns the name (see
/// [`crate::pasted::reserve`]); nothing having been written into it, leaving
/// it behind would put a file nobody can open into the directory the user
/// keeps their pictures in — and into the walk beside it.
fn fetch(mime_type: &str, path: &Path) -> Result<()> {
    let received = clipboard::receive(mime_type, path);
    if received.is_err() {
        let _ = std::fs::remove_file(path);
    }
    received.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::guard;

    #[test]
    fn a_panic_in_a_stage_becomes_an_error_rather_than_unwinding() {
        let ok = guard("working", || Ok::<u8, anyhow::Error>(7));
        assert_eq!(ok.unwrap(), 7);

        // The default hook still prints the panic; the point is that the
        // thread survives it and hands back a description instead.
        let hushed = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let caught = guard::<()>("decoding", || panic!("bad file"));
        std::panic::set_hook(hushed);

        let message = format!("{:#}", caught.unwrap_err());
        assert!(message.contains("panicked while decoding"), "{message}");
        assert!(message.contains("bad file"), "{message}");
    }
}
