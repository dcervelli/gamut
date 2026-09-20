//! Where the time goes, printed to stderr while we are working on it.
//!
//! Three numbers, all measured from `begin`, which `main` calls before it does
//! anything else:
//!
//! * how long it takes for a window to exist,
//! * how long each file takes to decode, reported every time one is read,
//! * how long it takes for the first frame carrying an image to be presented.
//!
//! Every measurement is wall clock on the thread that asks for it, so it
//! includes whatever else that thread was made to do on the way — which is the
//! point, since the thing being looked for is work on the main thread.

use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Set once, at the top of `main`. Not set at all under `cargo test`, where
/// there is no process start to speak of and the marks below stay quiet.
static START: OnceLock<Instant> = OnceLock::new();

/// Whether the timing marks are printed at all. Off unless `--timing` asks for
/// them: a viewer should not narrate every file it opens — and the names it
/// would print are attacker-chosen. On stderr, as diagnostics are, so that
/// whatever stdout is piped into is not handed them.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Turns the timing marks on. Called from argument parsing when `--timing` is
/// given, before any mark is reached.
pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
}

fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Cleared by the first presented frame that had an image in it.
static FIRST_FRAME: AtomicBool = AtomicBool::new(false);

/// Starts the clock every `startup → …` mark is measured against.
pub fn begin() {
    let _ = START.set(Instant::now());
}

/// Time since [`begin`], or `None` where it was never called.
fn since_start() -> Option<Duration> {
    START.get().map(Instant::elapsed)
}

/// The window exists and can be drawn into. This is the toolkit handing one
/// back, not the compositor putting it on screen — under Wayland nothing is
/// mapped until the first frame, which is what the mark below is for.
pub fn window_open() {
    if let Some(elapsed) = since_start() {
        report("startup → window open", elapsed);
    }
}

/// One file read and turned into pixels, however it was reached: the file the
/// command line named, a step to the next one, or a re-read after a write.
///
/// `elapsed` is the loader's work on it up to the point the pixels are
/// ready; `decoding` is the part of that spent inside the format's own
/// decoder. The line prints the two apart, with the difference as what this
/// program added around the decoder — reading the header, checking and
/// relabeling what came out, the statistics scan, the metadata — so that a
/// slow file can be told from a slow stage. The upload is not in it: that is
/// [`uploaded`]'s line, which is what lets the two files that are uploaded
/// differently be compared.
pub fn decoded(path: &Path, elapsed: Duration, decoding: Duration) {
    if enabled() {
        let ours = elapsed.saturating_sub(decoding);
        eprintln!(
            "[timing] decode {}: {} (decoder {}, gamut {})",
            name_of(path),
            ms(elapsed),
            ms(decoding),
            ms(ours),
        );
    }
}

/// One file's pixels repacked and copied to the GPU, on whichever thread did
/// it. That is the loader's for every file it read with a renderer to hand,
/// and the main one for the file named on the command line, which is
/// decoded while the window is still being made; kept out of [`decoded`]'s
/// line so that the two read the same whichever way the file came.
///
/// Measured over the copy into staging, which is where the time goes: the
/// bytes reach the texture at a later submit, on the frame that first draws
/// from them.
pub fn uploaded(path: &Path, elapsed: Duration) {
    if enabled() {
        eprintln!("[timing] upload {}: {}", name_of(path), ms(elapsed));
    }
}

/// The file's own name, safe to print: whoever named the file chose the
/// bytes, and a control character among them would write on the terminal.
fn name_of(path: &Path) -> String {
    let name = path
        .file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy();
    crate::escape_controls(&name)
}

/// One image encoded as a PNG for the clipboard. Measured over the encoder
/// alone, the walk that maps the pixels being reported beside it: this is the
/// half the compression setting decides, and the other is the half every
/// display setting does.
///
/// On the copy's own thread rather than the main one, so unlike the marks
/// above this is not time the window spent unable to answer. It is how long
/// the user waits before a paste has anything to give.
pub fn encoded_png(width: u32, height: u32, bytes: usize, elapsed: Duration) {
    if enabled() {
        report(
            &format!("encode png {width}x{height} \u{2192} {bytes} bytes"),
            elapsed,
        );
    }
}

/// The same image walked through the display pipeline on the way to that PNG:
/// one `sample` and one `map` per pixel, divided between as many threads as
/// the machine has. Reported apart from the encoding because the two are
/// answerable to different things — this one to the display state and the
/// image's size, that one to the compression setting.
pub fn mapped_image(width: u32, height: u32, elapsed: Duration) {
    if enabled() {
        report(&format!("map {width}x{height} for copy"), elapsed);
    }
}

/// The first frame with an image in it, handed to the presentation engine.
/// Called on every frame and prints on one, so the caller does not have to
/// carry the "has it happened yet" itself.
pub fn first_image_frame() {
    if FIRST_FRAME.swap(true, Ordering::Relaxed) {
        return;
    }
    if let Some(elapsed) = since_start() {
        report("startup → first image frame", elapsed);
    }
}

fn report(event: &str, elapsed: Duration) {
    if !enabled() {
        return;
    }
    eprintln!("[timing] {event}: {}", ms(elapsed));
}

fn ms(elapsed: Duration) -> String {
    format!("{:.2} ms", elapsed.as_secs_f64() * 1e3)
}
