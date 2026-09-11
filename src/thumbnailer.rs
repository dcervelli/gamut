//! The thread that makes the chooser's thumbnails, and what it hands back.
//!
//! One thread, one file at a time, at the lowest priority the kernel will
//! give it: the thumbnails are for a list the user may never open, and the
//! decoder on the loader thread is for the picture they are waiting on.
//! It starts at start-up over the whole session, so that the cache is
//! filling while the first file is still being looked at, and the chooser
//! moves whatever is on its screen to the front of the queue.
//!
//! What it makes goes into the desktop's own cache — see [`crate::thumbnail`]
//! — and what it hands back is a small copy of that for the screen, so
//! a second run of the program, or the file manager, reads the same files
//! back without decoding anything.
//!
//! Replies arrive as winit user events, like the loader's: the loop is
//! asleep between frames, and a proxy wakes it. Unlike the loader the thread
//! is not joined on the way out. It holds no handle on the GPU, which was
//! the loader's reason to join; a decode part way through would hold the
//! quit up for a picture nobody asked to see; and the cache is written to a
//! temporary name and renamed, so the process leaving mid-write leaves
//! nothing under a thumbnail's name. It is told to stop, and left to notice.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use anyhow::{Context, Result, anyhow};

use crate::image::decode::{self, Overrides};
use crate::image::display::{Display, Headroom, Startup};
use crate::image::sequence::Sequence;
use crate::image::{Channels, Region, Stats, encode, resample};
use crate::loader::guard;
use crate::thumbnail::{self, Dirs, Key, Lookup};

/// The longest side of the copy handed to the screen, which is what the
/// chooser draws: a quarter of the cache's, and at most 64 KiB of RGBA.
pub const DISPLAY_SIDE: u32 = 128;

/// What a file's pixels may come to, decoded, before the thread declines to
/// hold them: the same figure the player keeps its cache under, being the
/// same question — what a background thread may hold on a machine that is
/// also showing a picture. Reckoned at the worst case, four channels of
/// floats, from the size the header claims.
pub const MAX_THUMBNAIL_DECODE_BYTES: u64 = 1 << 30;

/// Something the thread has to say about one file.
pub struct Delivered {
    pub path: PathBuf,
    pub news: News,
}

pub enum News {
    /// What the header said, before any pixel work: enough for the row to
    /// say what kind of file it is and how large.
    Facts(Facts),
    /// The thumbnail, for the screen.
    Thumb(Thumb),
    /// No thumbnail is coming: the file could not be read, or the cache
    /// already says this version could not read it.
    Failed,
}

/// What a file's header says about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Facts {
    /// `None` for a format whose header does not say.
    pub size: Option<(u32, u32)>,
    pub sequence: Sequence,
}

/// The small copy for the screen: straight alpha, at most [`DISPLAY_SIDE`]
/// a side.
pub struct Thumb {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The handle the event loop keeps. Dropping it tells the thread to stop
/// and does not wait — see the module's own account of why.
pub struct Thumbnailer {
    asks: Option<Sender<Ask>>,
    canceled: Arc<AtomicBool>,
}

enum Ask {
    /// Files to thumbnail in due course, at the back of the queue; ones
    /// already seen are left where they are.
    Enqueue(Vec<PathBuf>),
    /// Files wanted now, moved — or put back — at the front of the queue
    /// in this order. A file already done is done again, which for one
    /// whose thumbnail the screen let go of is a read of the cache and
    /// never a second decode.
    Prioritize(Vec<PathBuf>),
}

impl Thumbnailer {
    /// `deliver` is where each thing the thread has to say goes, called on
    /// the thread; it answers `false` once nobody is listening, which stops
    /// the thread.
    pub fn new(
        overrides: Overrides,
        deliver: impl FnMut(Delivered) -> bool + Send + 'static,
    ) -> Self {
        let (asks, incoming) = mpsc::channel();
        let canceled = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&canceled);
        // A thread that cannot be spawned is a session with no thumbnails,
        // which is a session that still shows pictures.
        let spawned = thread::Builder::new()
            .name("gamut thumbnailer".into())
            .spawn(move || run(incoming, overrides, deliver, &flag));
        if let Err(error) = spawned {
            eprintln!("gamut: no thumbnail thread: {error}");
        }
        Self {
            asks: Some(asks),
            canceled,
        }
    }

    pub fn enqueue(&self, paths: Vec<PathBuf>) {
        self.send(Ask::Enqueue(paths));
    }

    pub fn prioritize(&self, paths: Vec<PathBuf>) {
        self.send(Ask::Prioritize(paths));
    }

    fn send(&self, ask: Ask) {
        // A closed channel is a thread that has gone, which nobody is
        // waiting on.
        if let Some(asks) = &self.asks {
            let _ = asks.send(ask);
        }
    }

    /// A thumbnailer that makes nothing, for tests that drive the
    /// application with no threads behind it.
    #[cfg(test)]
    pub fn detached() -> Self {
        let (asks, _) = mpsc::channel();
        Self {
            asks: Some(asks),
            canceled: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Drop for Thumbnailer {
    fn drop(&mut self) {
        self.canceled.store(true, Ordering::Relaxed);
        self.asks = None;
    }
}

/// Lowers the calling thread's priority, CPU and I/O both, as far as an
/// unprivileged process can: nice 10, and the best-effort I/O class at its
/// lowest level. Both are per-thread on Linux, which is what makes this
/// worth doing here rather than for the process. Best effort: a kernel
/// that refuses leaves the thread where it was, and the thumbnails still
/// come.
fn lower_priority() {
    const NICE: libc::c_int = 10;
    const IOPRIO_WHO_PROCESS: libc::c_int = 1;
    const IOPRIO_CLASS_BE: libc::c_int = 2;
    const IOPRIO_CLASS_SHIFT: libc::c_int = 13;
    const LOWEST_LEVEL: libc::c_int = 7;
    // SAFETY: two system calls with constant arguments, acting on the
    // calling thread — `0` names it for both — and reading no memory.
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS as _, 0, NICE);
        libc::syscall(
            libc::SYS_ioprio_set,
            IOPRIO_WHO_PROCESS,
            0,
            (IOPRIO_CLASS_BE << IOPRIO_CLASS_SHIFT) | LOWEST_LEVEL,
        );
    }
}

/// The queue: paths in the order they are to be done, and every path that
/// has ever been put in it, so that a file enqueued twice is done once.
#[derive(Default)]
struct Queue {
    waiting: VecDeque<PathBuf>,
    seen: HashSet<PathBuf>,
}

impl Queue {
    fn take(&mut self, ask: Ask) {
        match ask {
            Ask::Enqueue(paths) => {
                for path in paths {
                    if self.seen.insert(path.clone()) {
                        self.waiting.push_back(path);
                    }
                }
            }
            Ask::Prioritize(paths) => {
                // Back to front, so that the first asked for ends up first.
                for path in paths.into_iter().rev() {
                    self.waiting.retain(|waiting| *waiting != path);
                    self.seen.insert(path.clone());
                    self.waiting.push_front(path);
                }
            }
        }
    }
}

fn run(
    incoming: Receiver<Ask>,
    overrides: Overrides,
    mut deliver: impl FnMut(Delivered) -> bool,
    canceled: &AtomicBool,
) {
    // Rayon's global pool first, while this thread is still at the
    // process's own priority. The pool is built by whichever thread first
    // uses it — a JPEG XL decode here, or one on the loader — and its
    // threads keep that thread's nice value for the life of the process, so
    // built from here after the drop below every JPEG XL the loader decoded
    // would run at the thumbnailer's priority. Already built is fine, and
    // is what the error says.
    let _ = rayon_core::ThreadPoolBuilder::new()
        .thread_name(|index| format!("gamut rayon {index}"))
        .build_global();
    lower_priority();
    let dirs = Dirs::detect();
    let mut queue = Queue::default();
    loop {
        // Block for one ask when there is nothing to do, and take everything
        // else already waiting before starting on a file, so that what the
        // chooser has just brought to the front is what is done next.
        if queue.waiting.is_empty() {
            let Ok(first) = incoming.recv() else {
                return;
            };
            queue.take(first);
        }
        while let Ok(next) = incoming.try_recv() {
            queue.take(next);
        }
        if canceled.load(Ordering::Relaxed) {
            return;
        }
        let Some(path) = queue.waiting.pop_front() else {
            continue;
        };
        let mut deliver_news = |news| {
            deliver(Delivered {
                path: path.clone(),
                news,
            })
        };
        if !thumbnail_one(&path, dirs.as_ref(), overrides, canceled, &mut deliver_news) {
            return;
        }
    }
}

/// What the file system says about the file: its modification time in
/// seconds, and its size in bytes.
fn stat(path: &Path) -> Result<(u64, u64)> {
    let metadata = std::fs::metadata(path).with_context(|| format!("{}", path.display()))?;
    let modified = metadata
        .modified()
        .context("no modification time")?
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0);
    Ok((modified, metadata.len()))
}

/// Does one file, delivering what it learns as it goes. Returns `false`
/// when the loop has gone or the thread has been told to stop.
///
/// Each stage runs under the loader's own panic guard: the decoders parse
/// bytes chosen by whoever wrote the file, and a panic in one must become
/// a failed thumbnail rather than a thread that makes no more.
fn thumbnail_one(
    path: &Path,
    dirs: Option<&Dirs>,
    overrides: Overrides,
    canceled: &AtomicBool,
    deliver: &mut impl FnMut(News) -> bool,
) -> bool {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let facts = guard("reading the header", || {
        Ok(Facts {
            size: decode::probe(&absolute)?,
            sequence: decode::sequence(&absolute)?,
        })
    });
    let stat = stat(&absolute);
    let (facts, (mtime, bytes)) = match (facts, stat) {
        (Ok(facts), Ok(stat)) => (facts, stat),
        (Err(error), _) | (_, Err(error)) => {
            report(path, &error);
            return deliver(News::Failed);
        }
    };
    if !deliver(News::Facts(facts)) || canceled.load(Ordering::Relaxed) {
        return false;
    }
    // A file inside the cache is never thumbnailed, the specification says,
    // and there is no cache to read or write without a home to keep it in.
    let Some(dirs) = dirs.filter(|dirs| !dirs.holds(&absolute)) else {
        return deliver(News::Failed);
    };
    let key = thumbnail::key(&absolute);
    let news = match thumbnail::lookup(dirs, &key, mtime) {
        Lookup::Fresh(png) => match guard("reading the cached thumbnail", || read_png(&png)) {
            Ok(thumb) => News::Thumb(thumb),
            // A file in the cache that will not read is not this file's
            // failure: it is made again, over the top of it.
            Err(error) => {
                report(path, &error);
                make(
                    &absolute, dirs, &key, facts, mtime, bytes, overrides, canceled,
                )
            }
        },
        Lookup::Failed => News::Failed,
        Lookup::Missing => make(
            &absolute, dirs, &key, facts, mtime, bytes, overrides, canceled,
        ),
    };
    if canceled.load(Ordering::Relaxed) {
        return false;
    }
    deliver(news)
}

/// Makes the thumbnail of `path` and puts it in the cache, or records that
/// it could not; either way, what to say about it.
#[allow(
    clippy::too_many_arguments,
    reason = "one stage, given what the stages before it learned"
)]
fn make(
    path: &Path,
    dirs: &Dirs,
    key: &Key,
    facts: Facts,
    mtime: u64,
    bytes: u64,
    overrides: Overrides,
    canceled: &AtomicBool,
) -> News {
    // Refused before it is read: what the pixels would come to, at the
    // worst case a decoder produces, against what a background thread may
    // hold.
    let refused = facts.size.is_some_and(|(width, height)| {
        u64::from(width) * u64::from(height) * 16 > MAX_THUMBNAIL_DECODE_BYTES
    });
    let made = if refused {
        Err(anyhow!("too large to thumbnail in the background"))
    } else {
        guard("decoding", || decode::load(path, overrides)).and_then(|image| {
            let small = resample::downscale(&image, thumbnail::SIDE);
            drop(image);
            if canceled.load(Ordering::Relaxed) {
                return Err(anyhow!("stopped"));
            }
            // Windowed as the viewer would open it — a scene-referred
            // picture shown with `Display::default` is black — on the
            // small image, where the scan and the walk are cheap.
            let stats = guard("scanning", || Ok(Stats::scan(&small)))?;
            let display =
                Display::for_image_with(&small, &stats, Startup::default(), Headroom::None);
            let raster = encode::displayed_on(
                &small,
                &display,
                Region::whole([small.width, small.height]),
                1,
            );
            let chunks = thumbnail::text_chunks(key, mtime, bytes, facts.size);
            let png = encode::png_with_text(&raster, &chunks)?;
            thumbnail::write(dirs, &dirs.xlarge, key, &png)?;
            Ok(display_copy(
                raster.width,
                raster.height,
                raster.channels,
                &raster.data,
            ))
        })
    };
    match made {
        Ok(thumb) => News::Thumb(thumb),
        Err(error) => {
            if canceled.load(Ordering::Relaxed) {
                return News::Failed;
            }
            report(path, &error);
            if let Err(error) = thumbnail::write_failure(dirs, key, mtime) {
                report(path, &error);
            }
            News::Failed
        }
    }
}

/// A thumbnail read back from the cache, reduced for the screen.
fn read_png(path: &Path) -> Result<Thumb> {
    let file = std::fs::File::open(path)?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let mut pixels = vec![0; reader.output_buffer_size().context("too large")?];
    let info = reader.next_frame(&mut pixels)?;
    pixels.truncate(info.buffer_size());
    let channels = match info.color_type {
        png::ColorType::Grayscale => Channels::Gray,
        png::ColorType::GrayscaleAlpha => Channels::GrayAlpha,
        png::ColorType::Rgb => Channels::Rgb,
        png::ColorType::Rgba => Channels::Rgba,
        png::ColorType::Indexed => return Err(anyhow!("a palette survived expansion")),
    };
    if info.bit_depth != png::BitDepth::Eight {
        return Err(anyhow!("{:?} survived stripping", info.bit_depth));
    }
    Ok(display_copy(info.width, info.height, channels, &pixels))
}

/// Eight-bit pixels of `channels`, fitted to [`DISPLAY_SIDE`] and widened
/// to the straight RGBA the screen takes.
fn display_copy(width: u32, height: u32, channels: Channels, data: &[u8]) -> Thumb {
    let (width, height, small) =
        resample::downscale_bytes(width, height, channels.count(), data, DISPLAY_SIDE);
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for pixel in small.chunks_exact(channels.count()) {
        match channels {
            Channels::Gray => rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], 255]),
            Channels::GrayAlpha => {
                rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]])
            }
            Channels::Rgb => rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]),
            Channels::Rgba => rgba.extend_from_slice(pixel),
        }
    }
    Thumb {
        width,
        height,
        rgba,
    }
}

/// Says on the terminal that a thumbnail could not be made, with the chain
/// of why. Nothing in the window says it: the row shows the placeholder,
/// and a message about every broken file in a directory would be a message
/// nobody asked for.
fn report(path: &Path, error: &anyhow::Error) {
    eprintln!(
        "gamut: no thumbnail for {}: {}",
        crate::shown_path(path),
        crate::escape_controls(&format!("{error:#}"))
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Enqueuing keeps order and drops repeats; prioritizing brings a file
    /// to the front, in the order asked, whether or not it was waiting.
    #[test]
    fn the_queue_keeps_order_and_puts_the_asked_for_first() {
        let path = |name: &str| PathBuf::from(name);
        let mut queue = Queue::default();
        queue.take(Ask::Enqueue(vec![
            path("a"),
            path("b"),
            path("c"),
            path("a"),
        ]));
        assert_eq!(queue.waiting, [path("a"), path("b"), path("c")]);

        queue.take(Ask::Prioritize(vec![path("c"), path("z")]));
        assert_eq!(queue.waiting, [path("c"), path("z"), path("a"), path("b")]);

        // Seen is seen: a file brought to the front is not enqueued again
        // at the back, but can be prioritized again.
        queue.take(Ask::Enqueue(vec![path("z")]));
        assert_eq!(queue.waiting.len(), 4);
        queue.waiting.clear();
        queue.take(Ask::Prioritize(vec![path("a")]));
        assert_eq!(queue.waiting, [path("a")]);
    }

    /// The display copy widens every layout to straight RGBA and never
    /// enlarges.
    #[test]
    fn the_display_copy_is_rgba_and_small() {
        let thumb = display_copy(2, 1, Channels::Gray, &[0, 255]);
        assert_eq!((thumb.width, thumb.height), (2, 1));
        assert_eq!(thumb.rgba, [0, 0, 0, 255, 255, 255, 255, 255]);
        let thumb = display_copy(1, 1, Channels::GrayAlpha, &[9, 3]);
        assert_eq!(thumb.rgba, [9, 9, 9, 3]);
        let thumb = display_copy(1, 1, Channels::Rgb, &[1, 2, 3]);
        assert_eq!(thumb.rgba, [1, 2, 3, 255]);
        let wide = display_copy(512, 256, Channels::Rgba, &[7; 512 * 256 * 4]);
        assert_eq!((wide.width, wide.height), (DISPLAY_SIDE, DISPLAY_SIDE / 2));
        assert_eq!(
            wide.rgba.len(),
            (DISPLAY_SIDE * DISPLAY_SIDE / 2 * 4) as usize
        );
    }

    /// The whole trip: a file thumbnailed into a cache directory of the
    /// test's own, found there on a second pass, and a broken file
    /// recorded as a failure.
    #[test]
    fn a_file_is_thumbnailed_into_the_cache_and_read_back() {
        let dir = std::env::temp_dir().join(format!("gamut-thumbnailer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dirs = Dirs::under(&dir.join("thumbnails"));
        let picture = dir.join("picture.png");
        let pixels = vec![200u8; 640 * 480 * 3];
        ::image::save_buffer(&picture, &pixels, 640, 480, ::image::ColorType::Rgb8).unwrap();

        let mut news = Vec::new();
        let done = thumbnail_one(
            &picture,
            Some(&dirs),
            Overrides::default(),
            &AtomicBool::new(false),
            &mut |item| {
                news.push(item);
                true
            },
        );
        assert!(done);
        assert_eq!(news.len(), 2);
        assert!(matches!(
            news[0],
            News::Facts(Facts {
                size: Some((640, 480)),
                sequence: Sequence::Still
            })
        ));
        let News::Thumb(thumb) = &news[1] else {
            panic!("a thumbnail");
        };
        assert_eq!((thumb.width, thumb.height), (128, 96));
        assert_eq!(&thumb.rgba[..4], &[200, 200, 200, 255]);

        // In the cache, at the cache's size, with the chunks.
        let key = thumbnail::key(&std::path::absolute(&picture).unwrap());
        let cached = dirs.xlarge.join(&key.name);
        let reader = png::Decoder::new(std::io::BufReader::new(
            std::fs::File::open(&cached).unwrap(),
        ))
        .read_info()
        .unwrap();
        assert_eq!((reader.info().width, reader.info().height), (512, 384));
        let chunks = &reader.info().uncompressed_latin1_text;
        assert!(
            chunks
                .iter()
                .any(|chunk| chunk.keyword == "Thumb::URI" && chunk.text == key.uri)
        );
        assert!(
            chunks
                .iter()
                .any(|chunk| chunk.keyword == "Software" && chunk.text == "gamut")
        );

        // The second pass reads it back rather than decoding again: the
        // cache file is left exactly as it was.
        let before = std::fs::metadata(&cached).unwrap().modified().unwrap();
        let mut again = Vec::new();
        thumbnail_one(
            &picture,
            Some(&dirs),
            Overrides::default(),
            &AtomicBool::new(false),
            &mut |item| {
                again.push(item);
                true
            },
        );
        assert!(matches!(again[1], News::Thumb(_)));
        assert_eq!(
            std::fs::metadata(&cached).unwrap().modified().unwrap(),
            before
        );

        // A file that will not decode is a failure, and is remembered as one.
        let broken = dir.join("broken.png");
        std::fs::write(&broken, &std::fs::read(&picture).unwrap()[..400]).unwrap();
        let mut failed = Vec::new();
        thumbnail_one(
            &broken,
            Some(&dirs),
            Overrides::default(),
            &AtomicBool::new(false),
            &mut |item| {
                failed.push(item);
                true
            },
        );
        assert!(matches!(failed.last(), Some(News::Failed)));
        let key = thumbnail::key(&std::path::absolute(&broken).unwrap());
        let (mtime, _) = stat(&broken).unwrap();
        assert_eq!(thumbnail::lookup(&dirs, &key, mtime), Lookup::Failed);

        std::fs::remove_dir_all(dir).unwrap();
    }
}
