//! The region marked out on the picture, as the application holds it: what
//! it is in, which handle the arrows move, the hold a drag has on it, what
//! `Space` frames next, and the box being dragged out to zoom to. Every
//! change to the rectangle itself is `image::region`'s; where it is drawn
//! and which handle the pointer is on are `ui::region`'s. This is the
//! state between the two, and the gestures that move it, with no picture
//! and no window: a drag is a hold and a pull in image pixels.

use crate::image::orient::Turn;
use crate::image::region::{Grip, Region};
use crate::ui::{Grab, Selection};
use crate::view::Fit;

/// What `Space` frames next while a region is up: the region at either fit,
/// then the picture at either and at actual size, and round again. The
/// picture on its own has its three stops to cycle through; with a region
/// up there are two things to frame, and the region — the thing being
/// worked on — comes first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Framing {
    Region(Fit),
    Picture(Fit),
    /// The picture at 100%, as `1` shows it.
    Actual,
}

impl Framing {
    /// Where the cycle starts, and where a change to the region puts it
    /// back: whatever was framed before, the region that was just drawn or
    /// moved is what the next press should show.
    pub(super) const FIRST: Framing = Framing::Region(Fit::Whole);

    pub(super) fn next(self) -> Framing {
        match self {
            Framing::Region(Fit::Whole) => Framing::Region(Fit::Fill),
            Framing::Region(Fit::Fill) => Framing::Picture(Fit::Whole),
            Framing::Picture(Fit::Whole) => Framing::Picture(Fit::Fill),
            Framing::Picture(Fit::Fill) => Framing::Actual,
            Framing::Actual => Framing::Region(Fit::Whole),
        }
    }
}

/// The hold a drag on the picture has on the region, from the press to the
/// release: what was taken hold of, the region as it was when it was, and
/// where the press was in image pixels. Each frame of the drag remakes the
/// region from these and the hand's place, rather than from the frame
/// before, so nothing accumulates.
#[derive(Clone, Copy, Debug)]
struct Grabbing {
    grab: Grab,
    origin: Option<Region>,
    from: [f32; 2],
}

/// The region and everything about the hand on it.
#[derive(Clone, Copy, Debug)]
pub(super) struct Marking {
    /// The region on the picture: off, asked for, or drawn. What the arrows
    /// move, `Space` fits and `Ctrl+C` copies while one is on screen.
    pub selection: Selection,
    /// The current handle of the region: the one the arrows move, and the
    /// one lit apart from the others. The middle — the whole region — until
    /// a handle is clicked or dragged, and again for every new region.
    pub handle: Grip,
    /// The hold a drag on the picture has on it, from the press to the
    /// release, while the drag is the region's rather than the view's.
    grabbing: Option<Grabbing>,
    /// What `Space` frames next while a region is up. Its own rather than
    /// the view's, since a fit of the region is not a fit the view keeps.
    pub framing: Framing,
    /// The box being dragged out to zoom to, with `Space` held, while the
    /// drag is under way: painted over the picture, and what the view goes
    /// to when the drag lets go.
    zoom_box: Option<Region>,
    /// The handle of the region the pointer was resting on at the last
    /// pass, if any — said back by the frame, and what the region's words
    /// are written for.
    pub grip: Option<Grip>,
}

impl Default for Marking {
    fn default() -> Self {
        Self {
            selection: Selection::Off,
            handle: Grip::Middle,
            grabbing: None,
            framing: Framing::FIRST,
            zoom_box: None,
            grip: None,
        }
    }
}

impl Marking {
    /// Puts `region` on screen. A region drawn or moved is the region the
    /// next press of `Space` should show, wherever the cycle had got to.
    pub fn select(&mut self, region: Region) {
        self.selection = Selection::Shown(region);
        self.framing = Framing::FIRST;
    }

    /// The picture, `shown` pixels across and down, has been turned by
    /// `turn` more: the region follows the pixels it marked out. The handle
    /// goes back to the middle rather than being turned with it — an edge
    /// that was the top is the right one now, and the arrows that move it
    /// would move it the other way — and a drag under way lets go.
    pub fn turned(&mut self, turn: Turn, shown: [u32; 2]) {
        if let Selection::Shown(region) = self.selection {
            self.selection = Selection::Shown(region.turned(turn, shown));
        }
        self.handle = Grip::Middle;
        self.grabbing = None;
        self.zoom_box = None;
        self.grip = None;
    }

    /// Takes the region off, and the mode with it.
    pub fn clear(&mut self) {
        self.selection = Selection::Off;
        self.handle = Grip::Middle;
        self.grabbing = None;
        self.grip = None;
    }

    /// Whether the pointer is on the region, which is what its size and its
    /// coordinates are written for. The hand counts as being on it for as
    /// long as it has hold of it: a corner dragged to the edge of the image
    /// leaves the pointer off the region it is still resizing, and the size
    /// is exactly what is being watched then.
    pub fn over(&self) -> bool {
        self.selection.region().is_some() && (self.grip.is_some() || self.grabbing.is_some())
    }

    /// What the drag under way has hold of, for the frame to know which of
    /// the picture's gestures it is reading.
    pub fn grabbed(&self) -> Option<Grab> {
        self.grabbing.map(|grabbing| grabbing.grab)
    }

    /// The box being dragged out to zoom to, while one is.
    pub fn zoom_box(&self) -> Option<Region> {
        self.zoom_box
    }

    /// A drag on the picture has taken hold of the region — or of nothing
    /// yet, to draw one, or to draw a box to zoom to — at `at`, in image
    /// pixels. A handle taken hold of is the current one from then on, and
    /// a region drawn afresh starts over at the middle; a hold on the
    /// inside is a move and nothing more, and leaves the handle where it
    /// was.
    pub fn grab(&mut self, grab: Grab, at: [f32; 2]) {
        match grab {
            Grab::New => self.handle = Grip::Middle,
            Grab::Handle(Grip::Inside) | Grab::Zoom => {}
            Grab::Handle(grip) => self.handle = grip,
        }
        self.grabbing = Some(Grabbing {
            grab,
            origin: self.selection.region(),
            from: at,
        });
    }

    /// The hand is at `to`, in image pixels, on an image `image` pixels
    /// across: the region — or the box to zoom to — is what the hold makes
    /// of that. A new region, or a box, that has not yet enclosed a pixel —
    /// the hand still off the picture — leaves things as they were.
    pub fn pull(&mut self, to: [f32; 2], image: [u32; 2]) {
        let Some(Grabbing { grab, origin, from }) = self.grabbing else {
            return;
        };
        let region = match (grab, origin) {
            // The box is drawn as a new region is, but is not the
            // selection: it is kept apart, and taken by the release.
            (Grab::Zoom, _) => {
                if let Some(boxed) = Region::from_corners(from, to, image) {
                    self.zoom_box = Some(boxed);
                }
                return;
            }
            (Grab::New, _) => Region::from_corners(from, to, image),
            (Grab::Handle(Grip::Middle | Grip::Inside), Some(origin)) => {
                let by = |axis: usize| (to[axis] - from[axis]).round() as i64;
                Some(origin.moved_by(by(0), by(1), image))
            }
            (Grab::Handle(grip), Some(origin)) => Some(origin.pulled(grip, to, image)),
            (Grab::Handle(_), None) => None,
        };
        if let Some(region) = region {
            self.select(region);
        }
    }

    /// The button came up. A hold that never drew anything leaves the
    /// region asked for, so the next drag draws it. A box dragged out is
    /// handed back, for the view to go to.
    pub fn release(&mut self) -> Option<Region> {
        self.grabbing = None;
        self.zoom_box.take()
    }

    /// Drops the box being dragged out, and the hold with it, and says
    /// whether there was one: what `Esc` does before it takes the region
    /// off, so that the release the toolkit sends next finds nothing to
    /// zoom to.
    pub fn drop_box(&mut self) -> bool {
        if self.zoom_box.take().is_some() {
            self.grabbing = None;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::region::Side;

    const IMAGE: [u32; 2] = [40, 30];

    /// A drag from nothing draws a region, corner to corner, and the
    /// arrows then have the middle of it; the hand counts as on it while
    /// it has hold, and is off it once it lets go.
    #[test]
    fn a_drag_draws_a_region_and_holds_it_while_it_lasts() {
        let mut marking = Marking {
            selection: Selection::Armed,
            ..Marking::default()
        };
        marking.grab(Grab::New, [2.0, 3.0]);
        assert!(!marking.over(), "nothing drawn yet");
        marking.pull([10.0, 8.0], IMAGE);
        let drawn = marking.selection.region().expect("drawn");
        assert_eq!((drawn.x, drawn.y), (2, 3));
        assert!(drawn.width >= 7 && drawn.height >= 4, "{drawn:?}");
        assert!(marking.over(), "the hand has hold of it");
        assert_eq!(marking.handle, Grip::Middle);
        assert_eq!(marking.release(), None);
        assert!(!marking.over());
        assert_eq!(marking.selection.region(), Some(drawn));
    }

    /// A handle taken hold of is the current one, and pulling it moves that
    /// edge alone; a hold on the inside moves the whole and leaves the
    /// handle as it was.
    #[test]
    fn a_handle_pulled_is_the_current_one_and_the_inside_moves_the_whole() {
        let mut marking = Marking::default();
        let region = Region {
            x: 4,
            y: 4,
            width: 10,
            height: 10,
        };
        marking.select(region);
        marking.grab(Grab::Handle(Grip::Edge(Side::Right)), [14.0, 9.0]);
        assert_eq!(marking.handle, Grip::Edge(Side::Right));
        marking.pull([20.0, 9.0], IMAGE);
        let pulled = marking.selection.region().expect("still there");
        assert_eq!((pulled.x, pulled.y, pulled.height), (4, 4, 10));
        assert!(pulled.width > 10, "{pulled:?}");
        marking.release();

        marking.grab(Grab::Handle(Grip::Inside), [8.0, 8.0]);
        assert_eq!(marking.handle, Grip::Edge(Side::Right), "left as it was");
        marking.pull([11.0, 10.0], IMAGE);
        let moved = marking.selection.region().expect("still there");
        assert_eq!((moved.x, moved.y), (7, 6));
        assert_eq!((moved.width, moved.height), (pulled.width, pulled.height));
    }

    /// A box dragged out with `Space` held is not the region: it is kept
    /// apart, handed back by the release for the view to go to, and the
    /// region is left as it was. Dropped with `Esc`, the release finds
    /// nothing.
    #[test]
    fn a_zoom_box_is_kept_apart_and_taken_by_the_release() {
        let mut marking = Marking::default();
        let region = Region {
            x: 1,
            y: 1,
            width: 5,
            height: 5,
        };
        marking.select(region);
        marking.grab(Grab::Zoom, [10.0, 10.0]);
        marking.pull([20.0, 25.0], IMAGE);
        assert_eq!(marking.selection.region(), Some(region), "left alone");
        let boxed = marking.zoom_box().expect("a box is being drawn");
        assert_eq!((boxed.x, boxed.y), (10, 10));
        assert_eq!(marking.release(), Some(boxed));
        assert_eq!(marking.zoom_box(), None);

        marking.grab(Grab::Zoom, [10.0, 10.0]);
        marking.pull([20.0, 25.0], IMAGE);
        assert!(marking.drop_box());
        assert!(!marking.drop_box());
        assert_eq!(marking.release(), None);
    }

    /// Clearing takes everything off: the region, the hand, the handle and
    /// the pointer's place on it. And a region drawn or moved puts the
    /// framing back to the start.
    #[test]
    fn clearing_takes_everything_off_and_a_new_region_restarts_the_framing() {
        let mut marking = Marking::default();
        marking.select(Region {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
        });
        marking.framing = marking.framing.next();
        assert_ne!(marking.framing, Framing::FIRST);
        marking.handle = Grip::Edge(Side::Top);
        marking.grip = Some(Grip::Inside);
        marking.select(Region {
            x: 1,
            y: 1,
            width: 2,
            height: 2,
        });
        assert_eq!(marking.framing, Framing::FIRST);
        marking.clear();
        assert_eq!(marking.selection, Selection::Off);
        assert_eq!(marking.handle, Grip::Middle);
        assert_eq!(marking.grip, None);
        assert!(!marking.over());
    }
}
