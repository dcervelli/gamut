//! The clock an animation plays by: which frame should be on screen now,
//! and when the next one is due.
//!
//! The frame due is worked out from when the current one began showing and
//! how long the file says it lasts, never by counting redraws: a redraw that
//! comes late finds the clock has moved on past the frame it missed, and the
//! picture catches up to where it should be rather than slipping behind by
//! a frame every time the loop was busy. What the clock will not do is run
//! ahead of the decoder. A frame that is due but not yet decoded holds the
//! clock where it is, and gets its whole delay from the moment it arrives —
//! a slow decode makes a slow animation, never a jumpy one.
//!
//! Pure: it is told the time and what is decoded, and answers with a frame
//! and a deadline. Nothing here reads a file or draws.

use std::time::{Duration, Instant};

use crate::image::sequence::Loops;

/// What one tick of the clock found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tick {
    /// Whether the frame that should be on screen is a different one than
    /// before the tick.
    pub changed: bool,
    /// When the next frame is due, for the loop to sleep until. `None`
    /// while paused, finished, or waiting on the decoder.
    pub deadline: Option<Instant>,
}

pub struct Playback {
    /// The frame that should be on screen.
    head: usize,
    count: usize,
    loops: Loops,
    /// Passes still to play, counting the one under way. `None` for ever.
    passes_left: Option<u32>,
    /// When `head` began showing. `None` while paused or finished.
    since: Option<Instant>,
    /// Playing, but `head` is not decoded yet: the clock holds until it is.
    stalled: bool,
    finished: bool,
}

impl Playback {
    /// A clock at the first frame, running if `playing`.
    pub fn new(count: usize, loops: Loops, playing: bool, now: Instant) -> Self {
        Self {
            head: 0,
            count: count.max(1),
            loops,
            passes_left: passes(loops),
            since: playing.then_some(now),
            stalled: false,
            finished: false,
        }
    }

    pub fn head(&self) -> usize {
        self.head
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn playing(&self) -> bool {
        self.since.is_some()
    }

    /// Whether the last pass has been played to its end. The last frame
    /// stays up, and play starts over.
    #[cfg(test)]
    pub fn finished(&self) -> bool {
        self.finished
    }

    /// Fewer frames than the header promised: one would not decode. The
    /// head is kept in range.
    pub fn shrink(&mut self, count: usize) {
        self.count = count.clamp(1, self.count);
        self.head = self.head.min(self.count - 1);
    }

    /// Moves the clock on to `now`. `delays[i]` is how long frame `i` is
    /// shown for, known for every frame decoded so far and no other; the
    /// decoder works forward from the first, so what is known is a prefix.
    pub fn tick(&mut self, now: Instant, delays: &[Duration]) -> Tick {
        let Some(mut since) = self.since else {
            return Tick {
                changed: false,
                deadline: None,
            };
        };
        let mut changed = false;
        loop {
            let Some(&delay) = delays.get(self.head) else {
                // Not decoded yet. The clock holds here, and the frame is
                // given its whole delay from the moment it arrives.
                self.stalled = true;
                self.since = Some(now);
                return Tick {
                    changed,
                    deadline: None,
                };
            };
            if self.stalled {
                self.stalled = false;
                since = now;
                changed = true;
            }
            let due = since + delay;
            if now < due {
                self.since = Some(since);
                return Tick {
                    changed,
                    deadline: Some(due),
                };
            }
            // Past due: on to the next frame, its delay counted from when
            // this one should have gone rather than from now, so that a
            // late tick does not stretch the frame it was late for.
            let next = self.head + 1;
            if next >= self.count {
                match self.passes_left {
                    Some(1) => {
                        self.finished = true;
                        self.since = None;
                        return Tick {
                            changed,
                            deadline: None,
                        };
                    }
                    Some(left) => self.passes_left = Some(left - 1),
                    None => {}
                }
                self.head = 0;
            } else {
                self.head = next;
            }
            since = due;
            changed = true;
        }
    }

    /// Play if paused, pause if playing. A finished animation plays again
    /// from the start.
    pub fn toggle(&mut self, now: Instant) {
        if self.since.is_some() {
            self.since = None;
            self.stalled = false;
            return;
        }
        if self.finished {
            self.finished = false;
            self.head = 0;
            self.passes_left = passes(self.loops);
        }
        self.since = Some(now);
    }

    /// One frame on or back, round the ends, and the clock stops: a frame
    /// asked for by hand is one to look at.
    pub fn step(&mut self, by: isize) {
        let count = self.count as isize;
        self.head = (self.head as isize + by).rem_euclid(count) as usize;
        self.pause();
    }

    /// Straight to `frame`, and the clock stops.
    pub fn seek(&mut self, frame: usize) {
        self.head = frame.min(self.count - 1);
        self.pause();
    }

    fn pause(&mut self) {
        self.since = None;
        self.stalled = false;
        self.finished = false;
    }
}

/// How many passes a loop count is, counting the first.
fn passes(loops: Loops) -> Option<u32> {
    match loops {
        Loops::Forever => None,
        Loops::Times(times) => Some(times.get()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::num::NonZeroU32;

    const TENTH: Duration = Duration::from_millis(100);

    fn delays(count: usize) -> Vec<Duration> {
        vec![TENTH; count]
    }

    /// A tick that comes late lands on the frame that should be showing,
    /// not the one after the frame that was.
    #[test]
    fn a_late_tick_catches_up() {
        let start = Instant::now();
        let mut clock = Playback::new(10, Loops::Forever, true, start);
        let delays = delays(10);

        let tick = clock.tick(start + Duration::from_millis(50), &delays);
        assert!(!tick.changed);
        assert_eq!(tick.deadline, Some(start + TENTH));

        let tick = clock.tick(start + Duration::from_millis(350), &delays);
        assert!(tick.changed);
        assert_eq!(clock.head(), 3);
        // The next frame is due when it always was, not a tenth from the
        // late tick.
        assert_eq!(tick.deadline, Some(start + 4 * TENTH));
    }

    /// The pass wraps round for ever, and a counted one stops on the last
    /// frame with the clock off.
    #[test]
    fn loops_wrap_and_counted_ones_finish() {
        let start = Instant::now();
        let delays = delays(3);

        let mut forever = Playback::new(3, Loops::Forever, true, start);
        forever.tick(start + 3 * TENTH, &delays);
        assert_eq!(forever.head(), 0);
        assert!(forever.playing());

        let twice = Loops::Times(NonZeroU32::new(2).unwrap());
        let mut counted = Playback::new(3, twice, true, start);
        counted.tick(start + 4 * TENTH, &delays);
        assert_eq!(counted.head(), 1);
        assert!(counted.playing());
        let tick = counted.tick(start + 6 * TENTH, &delays);
        assert_eq!(counted.head(), 2, "stopped on the last frame");
        assert!(counted.finished());
        assert!(!counted.playing());
        assert_eq!(tick.deadline, None);

        // Play again starts over.
        counted.toggle(start + 7 * TENTH);
        assert_eq!(counted.head(), 0);
        assert!(counted.playing() && !counted.finished());
    }

    /// A frame the decoder has not delivered holds the clock, and gets its
    /// whole delay once it has.
    #[test]
    fn the_clock_waits_for_the_decoder() {
        let start = Instant::now();
        let mut clock = Playback::new(4, Loops::Forever, true, start);
        let two = delays(2);

        let tick = clock.tick(start + 2 * TENTH + Duration::from_millis(50), &two);
        assert!(tick.changed);
        assert_eq!(clock.head(), 2);
        assert_eq!(tick.deadline, None, "nothing to wake for until it arrives");

        // It arrives a second later, and is shown for a full tenth from then.
        let arrived = start + Duration::from_secs(1);
        let tick = clock.tick(arrived, &delays(3));
        assert!(tick.changed);
        assert_eq!(clock.head(), 2);
        assert_eq!(tick.deadline, Some(arrived + TENTH));
        let tick = clock.tick(arrived + Duration::from_millis(50), &delays(3));
        assert_eq!(clock.head(), 2);
        assert!(!tick.changed);
    }

    /// Stepping and seeking pause, and stepping goes round the ends.
    #[test]
    fn a_step_or_a_seek_pauses() {
        let start = Instant::now();
        let mut clock = Playback::new(3, Loops::Forever, true, start);

        clock.step(-1);
        assert_eq!(clock.head(), 2);
        assert!(!clock.playing());
        assert_eq!(clock.tick(start + TENTH, &delays(3)).deadline, None);

        clock.step(1);
        assert_eq!(clock.head(), 0);

        clock.toggle(start);
        clock.seek(9);
        assert_eq!(clock.head(), 2, "clamped to the last frame");
        assert!(!clock.playing());
    }

    /// A count that turned out short keeps the head in range.
    #[test]
    fn a_short_count_keeps_the_head_in_range() {
        let start = Instant::now();
        let mut clock = Playback::new(5, Loops::Forever, false, start);
        clock.seek(4);
        clock.shrink(2);
        assert_eq!(clock.count(), 2);
        assert_eq!(clock.head(), 1);
    }
}
