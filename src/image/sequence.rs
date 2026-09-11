//! What a file holds beyond the one image a decoder hands back first: the
//! frames of an animation, or the several pictures of a container that is a
//! folder rather than a film.
//!
//! An animation is read forward, one frame at a time, through a
//! [`FrameSource`] a decoder makes. Every frame arrives whole, at the size
//! of the canvas, with the file's disposal and blending already applied:
//! that is the decoder's business, since each format has its own rules, and
//! nothing past here needs to know them. A file of pages is not read
//! forward at all; each page is a decode of its own, asked for by number.

use std::num::NonZeroU32;
use std::time::Duration;

use anyhow::Result;

use super::DecodedImage;

/// How many times an animation plays.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Loops {
    Forever,
    Times(NonZeroU32),
}

impl Loops {
    /// The convention every animated format shares: zero means forever.
    pub fn from_count(count: u32) -> Self {
        match NonZeroU32::new(count) {
            Some(times) => Loops::Times(times),
            None => Loops::Forever,
        }
    }
}

/// What a file holds beyond the one image [`Decoder::decode`] returns.
///
/// [`Decoder::decode`]: super::decode::Decoder::decode
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sequence {
    /// One image, and that is all.
    Still,
    /// Frames on a clock. `count` is what the container says, or what a
    /// walk of it found; a frame that then fails to decode makes it one
    /// fewer, which the player allows for.
    Animation { count: usize, loops: Loops },
    /// Several images with no clock between them: the entries of an ICO, the
    /// directories of a TIFF. `default` is the one `decode` shows.
    Pages { count: usize, default: usize },
}

/// One frame of an animation as the decoder hands it over: whole, at the
/// canvas size, with the time it is shown for.
pub struct Frame {
    pub image: DecodedImage,
    pub delay: Duration,
}

/// A decoder's animation, read forward one frame at a time.
///
/// Not `Send`: the crates' frame iterators are not, so the thread that will
/// read the frames is the one that opens the source.
pub trait FrameSource {
    /// The next frame, or `None` past the last.
    fn next(&mut self) -> Result<Option<Frame>>;
    /// Back to the first frame, reopening whatever has to be reopened.
    fn rewind(&mut self) -> Result<()>;
}

/// The shortest a frame is shown for. A file may state zero, and a clock
/// given zero would spin; ten milliseconds is under any display's refresh,
/// so a frame stated faster than that still plays as fast as it can be seen.
pub const MIN_DELAY: Duration = Duration::from_millis(10);

/// A GIF's delay as browsers play it: a frame stated at a hundredth of a
/// second or less is shown for a tenth. Early encoders wrote 0 and 1
/// meaning "as fast as you like", and every browser settled on 100 ms for
/// them, so a file authored against that convention plays here at the
/// speed its author saw.
pub fn gif_delay(stated: Duration) -> Duration {
    if stated <= Duration::from_millis(10) {
        Duration::from_millis(100)
    } else {
        stated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gif_delay_under_a_hundredth_plays_at_a_tenth() {
        assert_eq!(gif_delay(Duration::ZERO), Duration::from_millis(100));
        assert_eq!(
            gif_delay(Duration::from_millis(10)),
            Duration::from_millis(100)
        );
        assert_eq!(
            gif_delay(Duration::from_millis(20)),
            Duration::from_millis(20)
        );
    }

    #[test]
    fn zero_loops_means_forever() {
        assert_eq!(Loops::from_count(0), Loops::Forever);
        assert_eq!(
            Loops::from_count(3),
            Loops::Times(NonZeroU32::new(3).unwrap())
        );
    }
}
