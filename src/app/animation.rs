//! The animation on screen: the thread decoding its frames ahead of the
//! clock, the clock itself, and which frame the texture holds. One
//! without the others is never the case, which is what this type says.
//! `player` is the thread and its cache, `playback` the pure clock; here is
//! what the application does with the pair.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use crate::image::decode::Overrides;
use crate::image::sequence::Loops;
use crate::player::{self, Frame, Player};
use crate::ui;

use super::kept::Left;
use super::playback::Playback;

/// What the clock's tick came to: whether the frame on screen is owed a
/// change, when the next is due, and the error the decoder hit if it hit
/// one — said once, the first time it is seen.
pub(super) struct Ticked {
    pub changed: bool,
    pub deadline: Option<Instant>,
    pub error: Option<String>,
}

pub(super) struct Animation {
    /// The thread decoding the frames, and the cache it fills.
    player: Player,
    /// The clock the frames play by.
    playback: Playback,
    /// Which frame the texture and the picture on screen hold. `None` for
    /// the file's own decode, which is what an animation opens as.
    uploaded: Option<usize>,
    /// Whether the player's failure, if it failed, has been reported.
    failed: bool,
}

impl Animation {
    /// Starts decoding `path`, told to hold `count` frames, with its clock
    /// running from the first frame — or from where the file was `left`,
    /// playing if it was playing — and told to wake the loop through
    /// `wake` with news under `generation`, the number that tells its news
    /// from a player dropped with its file.
    #[allow(
        clippy::too_many_arguments,
        reason = "each is one fact about the start; there is no struct they are all fields of"
    )]
    pub fn start(
        path: &Path,
        overrides: Overrides,
        count: usize,
        loops: Loops,
        left: Option<Left>,
        playing: bool,
        generation: u64,
        wake: player::Wake,
        now: Instant,
    ) -> Self {
        let mut playback = Playback::new(count, loops, playing, now);
        if let Some(Left::Frame { frame, paused }) = left {
            playback.seek(frame);
            if !paused {
                playback.toggle(now);
            }
        }
        let player = Player::new(
            generation,
            path.to_path_buf(),
            overrides,
            count,
            move |event| wake(event),
        );
        player.head(playback.head());
        Self {
            player,
            playback,
            uploaded: None,
            failed: false,
        }
    }

    /// Whether news under `generation` is this animation's.
    pub fn is(&self, generation: u64) -> bool {
        self.player.generation == generation
    }

    #[cfg(test)]
    pub fn head(&self) -> usize {
        self.playback.head()
    }

    #[cfg(test)]
    pub fn count(&self) -> usize {
        self.playback.count()
    }

    /// Whether the clock is running.
    pub fn playing(&self) -> bool {
        self.playback.playing()
    }

    /// Which frame the texture holds — see the field.
    #[cfg(test)]
    pub fn uploaded(&self) -> Option<usize> {
        self.uploaded
    }

    /// The frame whose pixels are on screen, counted from zero: the one the
    /// texture holds, which the clock's head can be ahead of while the
    /// player catches up. Before the first is written it is the file's own
    /// decode, its first frame.
    pub fn on_screen(&self) -> usize {
        self.uploaded.unwrap_or(0)
    }

    /// Where the file is being left: the frame that is up, and whether it
    /// was stopped there.
    pub fn left(&self) -> Left {
        Left::Frame {
            frame: self.playback.head(),
            paused: !self.playback.playing(),
        }
    }

    /// What the transport bar shows.
    pub fn transport(&self) -> ui::Transport {
        ui::Transport {
            index: self.playback.head(),
            count: self.playback.count(),
            kind: ui::transport::Kind::Animation {
                playing: self.playback.playing(),
                delays: self.player.read(|cache| cache.delays().to_vec()),
            },
        }
    }

    /// The frame the clock says should be up, where it is not already and
    /// the player has it. Where the player has not got to it yet, it is
    /// told where the head is and asked to wake us when it has; the frame
    /// already up stays until then. The caller puts the frame on screen and
    /// then says so with [`Animation::shown`].
    pub fn due_frame(&mut self) -> Option<(usize, Arc<Frame>)> {
        let head = self.playback.head();
        if self.uploaded == Some(head) {
            return None;
        }
        self.player.head(head);
        let frame = self.player.read(|cache| cache.frame(head))?;
        Some((head, frame))
    }

    /// The frame `head` is on screen now.
    pub fn shown(&mut self, head: usize) {
        self.uploaded = Some(head);
    }

    /// Moves the clock on to `now`, against what the player has decoded so
    /// far.
    pub fn tick(&mut self, now: Instant) -> Ticked {
        let (delays, count, error) = self.player.read(|cache| {
            (
                cache.delays().to_vec(),
                cache.count(),
                cache.error().map(str::to_string),
            )
        });
        self.playback.shrink(count);
        let tick = self.playback.tick(now, &delays);
        let error = error.filter(|_| !self.failed);
        if error.is_some() {
            self.failed = true;
        }
        Ticked {
            changed: tick.changed || error.is_some(),
            deadline: tick.deadline,
            error,
        }
    }

    /// One frame on or back, stopped there.
    pub fn step(&mut self, by: isize) {
        self.playback.step(by);
    }

    /// Plays a stopped animation, or stops a playing one.
    pub fn toggle(&mut self, now: Instant) {
        self.playback.toggle(now);
    }

    /// Straight to `frame`, stopped there.
    pub fn seek(&mut self, frame: usize) {
        self.playback.seek(frame);
    }

    /// Reads from the player's cache, for a test that waits on it.
    #[cfg(test)]
    pub fn read<T>(&self, read: impl FnOnce(&player::Cache) -> T) -> T {
        self.player.read(read)
    }
}
