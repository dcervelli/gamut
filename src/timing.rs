//! Where the time goes, printed to stdout while we are working on it.
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
/// them: a viewer should be silent on stdout, not narrate every file it opens
/// — and the names it would print are attacker-chosen.
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
pub fn decoded(path: &Path, elapsed: Duration) {
    if enabled() {
        let name = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy();
        report(&format!("decode {}", crate::escape_controls(&name)), elapsed);
    }
}

/// One image encoded as a PNG for the clipboard. Measured over the encoder
/// alone, the walk that maps the pixels being reported beside it: this is the
/// half the compression setting decides, and the other is the half every
/// display setting does.
pub fn encoded_png(width: u32, height: u32, bytes: usize, elapsed: Duration) {
    if enabled() {
        report(
            &format!("encode png {width}x{height} \u{2192} {bytes} bytes"),
            elapsed,
        );
    }
}

/// The same image walked through the display pipeline on the way to that PNG:
/// one `sample` and one `map` per pixel, which is where the time in a copy
/// actually goes.
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
    println!("[timing] {event}: {:.2} ms", elapsed.as_secs_f64() * 1e3);
}
