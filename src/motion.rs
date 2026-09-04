//! A pan or zoom on its way: where the view was when it was asked to move,
//! and how far along it is.
//!
//! Where it is going is the [`View`](crate::view::View) itself, which holds
//! the settled state and is asked afresh on every frame. So a move is defined
//! by its start alone, and its end can change under it — the window resized,
//! a panel come or gone, another key pressed — without anything to reconcile:
//! the view on screen is always on the straight line from where the move
//! began to wherever the view now says. A second move begun before the first
//! lands starts from wherever the first had got to, and has [`DURATION`] all
//! over again to finish. The line is straight in space-scale coordinates,
//! which [`Position`] explains.

use std::time::{Duration, Instant};

use crate::view::Position;

/// How long a move takes, from wherever the view was to wherever it is going.
/// Short enough to be over before the next key repeat has to restart it
/// from part way, and long enough to be a movement rather than a cut.
pub const DURATION: Duration = Duration::from_millis(200);

pub struct Motion {
    from: Position,
    started: Instant,
}

impl Motion {
    pub fn new(from: Position, now: Instant) -> Self {
        Self { from, started: now }
    }

    /// How far along the move is at `now`, from zero to one: eased out, so
    /// it leaves quickly and settles rather than braking. A key held down
    /// restarts the move from part way at every repeat, and a curve that
    /// began slowly would never get out of its slow beginning.
    fn progress(&self, now: Instant) -> f32 {
        let elapsed = now.saturating_duration_since(self.started).as_secs_f32();
        let t = (elapsed / DURATION.as_secs_f32()).clamp(0.0, 1.0);
        1.0 - (1.0 - t).powi(3)
    }

    /// Where the view is at `now`, on its way to `to`.
    pub fn position(&self, to: Position, now: Instant) -> Position {
        Position::between(self.from, to, self.progress(now))
    }

    /// Whether the move has landed, and so whether the frames it was asking
    /// for can stop.
    pub fn done(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.started) >= DURATION
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FROM: Position = Position {
        u: [0.0, 100.0],
        v: 1.0,
    };
    const TO: Position = Position {
        u: [400.0, -300.0],
        v: 3.0,
    };

    #[test]
    fn a_move_starts_where_it_was_and_lands_where_it_is_going() {
        let start = Instant::now();
        let motion = Motion::new(FROM, start);
        assert_eq!(motion.position(TO, start), FROM);
        assert!(!motion.done(start));
        // A clock that has somehow gone backwards is still at the start.
        assert_eq!(motion.position(TO, start - DURATION), FROM);

        let end = start + DURATION;
        assert_eq!(motion.position(TO, end), TO);
        assert!(motion.done(end));
        assert_eq!(motion.position(TO, end + DURATION), TO);
    }

    /// Eased out: most of the distance goes in the first half of the time,
    /// and the move never turns back.
    #[test]
    fn a_move_leaves_quickly_and_settles() {
        let start = Instant::now();
        let motion = Motion::new(FROM, start);
        let half = motion.position(TO, start + DURATION / 2);
        assert!(half.v > (FROM.v + TO.v) / 2.0);
        assert!(half.v < TO.v);

        let mut last = FROM.v;
        for step in 1..=20 {
            let at = motion.position(TO, start + DURATION * step / 20);
            assert!(at.v >= last, "{at:?} after {last}");
            last = at.v;
        }
    }

    /// The end is asked for every time rather than remembered, so a move
    /// whose destination changes part way bends towards the new one from
    /// where it is rather than finishing the old one first.
    #[test]
    fn a_move_follows_a_destination_that_changes() {
        let start = Instant::now();
        let motion = Motion::new(FROM, start);
        let now = start + DURATION / 2;
        let towards_first = motion.position(TO, now);
        let elsewhere = Position {
            u: [-400.0, 300.0],
            v: 0.5,
        };
        let towards_second = motion.position(elsewhere, now);
        assert!(towards_first.v > FROM.v);
        assert!(towards_second.v < FROM.v);
    }
}
