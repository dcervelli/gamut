//! Decoding an animation's frames ahead of the clock, on a thread of its
//! own, into a cache the event loop reads from.
//!
//! The thread owns the decoder's [`FrameSource`], which reads forward only,
//! and a [`Cache`] of the frames it has decoded, each scanned for its
//! statistics so that the histogram and the readout follow the frame on
//! screen. It is told where the play head is and works outward from there,
//! decoding the frame nearest ahead that is not yet held. A file whose frames
//! all fit the budget ends up wholly resident, playing or paused, so that
//! every later step or seek is answered from memory; a file that does not
//! fit keeps a window of frames around the head, letting the farthest go as
//! the head moves on, and a seek back behind the window sends the decoder
//! back to the start to read forward again. One shape covers both, and the
//! budget rather than the file decides which it is.
//!
//! The event loop is woken through a user event each time the cache changes,
//! the way the loader wakes it for a finished file, since the loop sleeps
//! until something is due and a frame it is waiting on is something due.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::image::decode::{self, MAX_SEQUENCE_BYTES, Overrides};
use crate::image::sequence::{FrameSource, MIN_DELAY};
use crate::image::{DecodedImage, Stats};
use crate::loader::guard;

/// One decoded frame with everything the interface reads off a picture.
pub struct Frame {
    /// Shared with the interface, which hands it to the renderer and to a
    /// copy's thread, the way `Current::image` is.
    pub image: Arc<DecodedImage>,
    pub stats: Stats,
    /// How long it is shown for, never under [`MIN_DELAY`].
    pub delay: Duration,
}

impl Frame {
    fn bytes(&self) -> u64 {
        self.image.samples.byte_len() as u64
    }
}

/// The thread's news for the event loop: something in the cache changed —
/// a frame arrived, the count came down, or the decoder failed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Event {
    /// Which player it is from, so that news from one abandoned with its
    /// file is told apart from the one now playing.
    pub generation: u64,
}

/// How a player wakes the event loop, cloned into each thread: what `main`
/// makes from the loop's proxy. Answers `false` once the loop has gone.
pub type Wake = Arc<dyn Fn(Event) -> bool + Send + Sync>;

/// The frames held, and what the thread knows about the ones it has not.
pub struct Cache {
    frames: BTreeMap<usize, Arc<Frame>>,
    /// Every frame's delay as far as decoding has reached, never let go of:
    /// the clock times frames the cache no longer holds by them, and the
    /// timeline is laid out in them.
    delays: Vec<Duration>,
    count: usize,
    bytes: u64,
    budget: u64,
    /// The play head as last told, which the eviction order is about.
    head: usize,
    /// Why decoding stopped, if it did.
    error: Option<String>,
}

impl Cache {
    fn new(count: usize, budget: u64) -> Self {
        Self {
            frames: BTreeMap::new(),
            delays: Vec::new(),
            count: count.max(1),
            bytes: 0,
            budget,
            head: 0,
            error: None,
        }
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn delays(&self) -> &[Duration] {
        &self.delays
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn frame(&self, index: usize) -> Option<Arc<Frame>> {
        self.frames.get(&index).cloned()
    }

    /// Whether every frame is held, and so the thread has nothing to do
    /// until the head moves somewhere it is not.
    #[cfg(test)]
    pub fn complete(&self) -> bool {
        self.frames.len() >= self.count
    }

    /// How far ahead of the head `index` is, going round the end: the frame
    /// just behind the head is the farthest of all, since it comes round
    /// last.
    fn distance(&self, index: usize) -> usize {
        (index + self.count - self.head) % self.count
    }

    /// The frame the thread should decode next: the nearest ahead of the
    /// head that is not held. `None` when all are.
    fn wanted(&self) -> Option<usize> {
        (0..self.count)
            .filter(|index| !self.frames.contains_key(index))
            .min_by_key(|&index| self.distance(index))
    }

    /// Takes `frame` in as frame `index`, letting go of whatever is farthest
    /// from the head to make room. Answers `false` where there is no room to
    /// be made: everything held is nearer the head than this frame, so the
    /// window is full and this one will wait for the head to move. One frame
    /// is always held, however large.
    fn insert(&mut self, index: usize, frame: Arc<Frame>) -> bool {
        let bytes = frame.bytes();
        while !self.frames.is_empty() && self.bytes + bytes > self.budget {
            let farthest = *self
                .frames
                .keys()
                .max_by_key(|&&held| self.distance(held))
                .expect("not empty");
            if self.distance(farthest) <= self.distance(index) {
                return false;
            }
            if let Some(gone) = self.frames.remove(&farthest) {
                self.bytes -= gone.bytes();
            }
        }
        self.bytes += bytes;
        self.frames.insert(index, frame);
        true
    }

    /// Records frame `index`'s delay. Decoding runs forward from the first,
    /// so this is the next one or one already known.
    fn learned(&mut self, index: usize, delay: Duration) {
        if index == self.delays.len() {
            self.delays.push(delay);
        }
    }

    /// The source ran dry before the count the header gave: there are only
    /// this many.
    fn shrink(&mut self, count: usize) {
        self.count = count.clamp(1, self.count);
        self.head = self.head.min(self.count - 1);
        let over: Vec<usize> = self.frames.range(self.count..).map(|(&k, _)| k).collect();
        for index in over {
            if let Some(gone) = self.frames.remove(&index) {
                self.bytes -= gone.bytes();
            }
        }
    }
}

/// What the thread is told.
enum Command {
    /// The play head has moved here.
    Head(usize),
}

/// The handle the event loop keeps: one per animated file on screen.
/// Dropping it stops the thread and waits for it, as dropping the loader
/// does, and for the same reason it is declared before the renderer.
pub struct Player {
    commands: Option<Sender<Command>>,
    thread: Option<JoinHandle<()>>,
    shared: Arc<Shared>,
    pub generation: u64,
}

struct Shared {
    cache: Mutex<Cache>,
    canceled: AtomicBool,
}

impl Player {
    /// Starts decoding `path`, told to hold `count` frames. `deliver` is
    /// called on the thread with each change, and answers `false` once
    /// nobody is listening.
    pub fn new(
        generation: u64,
        path: PathBuf,
        overrides: Overrides,
        count: usize,
        deliver: impl FnMut(Event) -> bool + Send + 'static,
    ) -> Self {
        Self::with_budget(
            generation,
            path,
            overrides,
            count,
            MAX_SEQUENCE_BYTES,
            deliver,
        )
    }

    fn with_budget(
        generation: u64,
        path: PathBuf,
        overrides: Overrides,
        count: usize,
        budget: u64,
        deliver: impl FnMut(Event) -> bool + Send + 'static,
    ) -> Self {
        let (commands, incoming) = mpsc::channel();
        let shared = Arc::new(Shared {
            cache: Mutex::new(Cache::new(count, budget)),
            canceled: AtomicBool::new(false),
        });
        let theirs = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name("gamut player".into())
            .spawn(move || {
                let mut deliver = deliver;
                let mut deliver = move || deliver(Event { generation });
                match decode::frames(&path, overrides) {
                    Ok(source) => run(source, incoming, &theirs, &mut deliver),
                    Err(error) => {
                        lock(&theirs.cache).error = Some(format!("{error:#}"));
                        deliver();
                    }
                }
            })
            .expect("the player thread can be spawned");
        Self {
            commands: Some(commands),
            thread: Some(thread),
            shared,
            generation,
        }
    }

    /// Tells the thread where the play head is, which is what it decodes
    /// toward and keeps frames around.
    pub fn head(&self, index: usize) {
        if let Some(commands) = &self.commands {
            let _ = commands.send(Command::Head(index));
        }
    }

    /// Reads from the cache. Held briefly: the thread decodes outside the
    /// lock and takes it only to put a frame in.
    pub fn read<T>(&self, read: impl FnOnce(&Cache) -> T) -> T {
        read(&lock(&self.shared.cache))
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.shared.canceled.store(true, Ordering::Relaxed);
        self.commands = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn lock(cache: &Mutex<Cache>) -> std::sync::MutexGuard<'_, Cache> {
    cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The thread: decode toward the head until everything that fits is held,
/// then sleep until the head moves.
fn run(
    mut source: Box<dyn FrameSource>,
    incoming: Receiver<Command>,
    shared: &Shared,
    deliver: &mut impl FnMut() -> bool,
) {
    // The frame the source will hand over next.
    let mut position = 0usize;
    // Whether to wait for the head to move before decoding again: the
    // window is full, or everything is held.
    let mut wait = false;

    loop {
        if wait {
            // Block for one, then take the rest, so that a scrub that
            // moved the head many times is answered where it stopped.
            match incoming.recv() {
                Ok(command) => absorb(command, shared),
                Err(_) => return,
            }
            wait = false;
        }
        loop {
            match incoming.try_recv() {
                Ok(command) => absorb(command, shared),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        if shared.canceled.load(Ordering::Relaxed) {
            return;
        }

        let Some(target) = lock(&shared.cache).wanted() else {
            wait = true;
            continue;
        };
        if target < position {
            if let Err(error) = source.rewind() {
                lock(&shared.cache).error = Some(format!("{error:#}"));
                deliver();
                return;
            }
            position = 0;
        }

        // Forward to the target, one frame per pass so that a head moved
        // meanwhile is noticed between frames rather than after the lot.
        let index = position;
        match guard("decoding a frame", || source.next()) {
            Ok(Some(frame)) => {
                position += 1;
                let stats = Stats::scan(&frame.image);
                let frame = Arc::new(Frame {
                    image: Arc::new(frame.image),
                    stats,
                    delay: frame.delay.max(MIN_DELAY),
                });
                let mut cache = lock(&shared.cache);
                cache.learned(index, frame.delay);
                let held = cache.frames.contains_key(&index);
                let kept = !held && cache.insert(index, frame);
                if !kept && !held && index == target {
                    // The window is full: nothing farther from the head
                    // than this frame to let go of.
                    wait = true;
                }
                drop(cache);
                if kept && !deliver() {
                    return;
                }
            }
            Ok(None) => {
                // Fewer than the header said. What was found is the count
                // from here on, and the source goes back to the start.
                lock(&shared.cache).shrink(position.max(1));
                if !deliver() {
                    return;
                }
                if let Err(error) = source.rewind() {
                    lock(&shared.cache).error = Some(format!("{error:#}"));
                    deliver();
                    return;
                }
                position = 0;
                if index == 0 {
                    // Nothing at all came out: there is nothing to play.
                    return;
                }
            }
            Err(error) => {
                lock(&shared.cache).error = Some(format!("{error:#}"));
                deliver();
                return;
            }
        }
    }
}

fn absorb(command: Command, shared: &Shared) {
    match command {
        Command::Head(index) => {
            let mut cache = lock(&shared.cache);
            cache.head = index.min(cache.count - 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::image::sequence::Loops;
    use crate::image::{AlphaMode, Channels, ColorSpace, Samples};

    /// A frame of `bytes` bytes, for the arithmetic; the pixels are never
    /// looked at.
    fn frame(bytes: usize) -> Arc<Frame> {
        let image = DecodedImage::new(
            bytes as u32,
            1,
            Samples::U8 {
                channels: Channels::Gray,
                data: vec![0; bytes],
            },
            ColorSpace::SRGB,
            AlphaMode::Opaque,
        );
        let stats = Stats::scan(&image);
        Arc::new(Frame {
            image: Arc::new(image),
            stats,
            delay: MIN_DELAY,
        })
    }

    fn held(cache: &Cache) -> Vec<usize> {
        cache.frames.keys().copied().collect()
    }

    /// Under the budget every frame ends up held, in the order the head
    /// wants them, and then there is nothing left to want.
    #[test]
    fn a_small_sequence_becomes_wholly_resident() {
        let mut cache = Cache::new(4, 100);
        for index in 0..4 {
            assert_eq!(cache.wanted(), Some(index));
            assert!(cache.insert(index, frame(10)));
        }
        assert_eq!(cache.wanted(), None);
        assert!(cache.complete());
        assert_eq!(cache.bytes, 40);
    }

    /// Over the budget the cache is a window that slides with the head: the
    /// frame just behind it is the first to go, being the last to come round
    /// again.
    #[test]
    fn over_the_budget_the_window_follows_the_head() {
        let mut cache = Cache::new(10, 30);
        for index in 0..3 {
            assert!(cache.insert(index, frame(10)));
        }
        // Full: frame 3 is farther from the head than anything held.
        assert!(!cache.insert(3, frame(10)));
        assert_eq!(held(&cache), [0, 1, 2]);

        // The head moves on to 2. Frame 1, just behind it, is now the
        // farthest — nine frames away going round — and goes.
        cache.head = 2;
        assert_eq!(cache.wanted(), Some(3));
        assert!(cache.insert(3, frame(10)));
        assert_eq!(held(&cache), [0, 2, 3]);
    }

    /// A seek behind the window wants the head itself first, and makes room
    /// for it by letting go of the frames now farthest from it.
    #[test]
    fn a_seek_behind_the_window_is_served_first() {
        let mut cache = Cache::new(10, 30);
        cache.head = 5;
        for index in 5..8 {
            assert!(cache.insert(index, frame(10)));
        }
        cache.head = 1;
        assert_eq!(cache.wanted(), Some(1));
        assert!(cache.insert(1, frame(10)));
        // From a head at 1, frame 7 is the farthest of the three held and
        // is the one let go.
        assert_eq!(held(&cache), [1, 5, 6]);
    }

    /// One frame is always held, whatever the budget says.
    #[test]
    fn one_frame_is_always_held() {
        let mut cache = Cache::new(3, 5);
        assert!(cache.insert(0, frame(10)));
        assert!(!cache.insert(1, frame(10)));
        assert_eq!(held(&cache), [0]);
    }

    /// A count that came down drops the frames past it and keeps the head
    /// in range.
    #[test]
    fn a_short_count_drops_the_frames_past_it() {
        let mut cache = Cache::new(5, 100);
        for index in 0..5 {
            cache.insert(index, frame(10));
        }
        cache.head = 4;
        cache.shrink(3);
        assert_eq!(held(&cache), [0, 1, 2]);
        assert_eq!(cache.head, 2);
        assert_eq!(cache.bytes, 30);
    }

    /// Delays are learned in order and never let go of, so the clock can
    /// time a frame the window has passed.
    #[test]
    fn delays_are_kept_for_every_frame_seen() {
        let mut cache = Cache::new(3, 10);
        cache.learned(0, Duration::from_millis(50));
        cache.learned(1, Duration::from_millis(70));
        // Seen again after a rewind: nothing changes.
        cache.learned(0, Duration::from_millis(99));
        assert_eq!(
            cache.delays(),
            [Duration::from_millis(50), Duration::from_millis(70)]
        );
    }

    /// The thread decodes a short file to its end and stops, with every
    /// frame held and the event loop told each time.
    #[test]
    fn a_player_decodes_a_short_file_to_the_end() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_images/gif-animated.gif");
        let count = match decode::sequence(&path).unwrap() {
            crate::image::sequence::Sequence::Animation { count, loops } => {
                assert_eq!(loops, Loops::Forever);
                count
            }
            other => panic!("{other:?}"),
        };
        let (events, received) = mpsc::channel();
        let player = Player::new(7, path, Overrides::default(), count, move |event| {
            events.send(event).is_ok()
        });

        for _ in 0..count {
            let event = received
                .recv_timeout(Duration::from_secs(10))
                .expect("a frame arrives");
            assert_eq!(event.generation, 7);
        }
        player.read(|cache| {
            assert!(cache.complete());
            assert_eq!(cache.count(), 2);
            assert_eq!(cache.error(), None);
            assert_eq!(cache.delays(), [Duration::from_millis(100); 2]);
            let second = cache.frame(1).expect("the second frame is held");
            assert_eq!((second.image.width, second.image.height), (32, 24));
        });
    }

    /// A file that is not an animation leaves an error for the loop to
    /// report, rather than a thread that sits there.
    #[test]
    fn a_still_given_to_a_player_reports_an_error() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_images/png-rgb8.png");
        let (events, received) = mpsc::channel();
        let player = Player::new(1, path, Overrides::default(), 2, move |event| {
            events.send(event).is_ok()
        });
        received
            .recv_timeout(Duration::from_secs(10))
            .expect("the error is announced");
        player.read(|cache| assert!(cache.error().is_some()));
    }
}
