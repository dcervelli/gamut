//! Where a thing floating over the picture goes, and the area it is drawn
//! in: the one placement every panel is fitted by, and the one opening
//! every panel makes.

use super::{PADDING, Rect};

/// Where on the content a panel stands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    /// The top-right corner, inside the padding.
    TopRight,
    /// Across the top, centered.
    TopCenter,
    /// The middle of the content.
    Center,
}

/// Where a panel goes on `content`, or `None` where the content has no room
/// for it: the panel wants at most `most` and at least `least`, and stands
/// at `place`, inside the padding everything floating over the picture
/// keeps. Rounded to whole logical pixels, so that what is laid out in it
/// starts on a pixel edge.
///
/// A panel refused rather than shrunk is what keeps every panel readable:
/// one drawn smaller than it was laid out for would cover the picture it is
/// about and say nothing for it.
pub fn fit(content: Rect, most: [f32; 2], least: [f32; 2], place: Place) -> Option<Rect> {
    let room = [
        content.width - 2.0 * PADDING,
        content.height - 2.0 * PADDING,
    ];
    let size = [most[0].min(room[0]), most[1].min(room[1])];
    if size[0] < least[0] || size[1] < least[1] {
        return None;
    }
    let x = match place {
        Place::TopRight => content.right() - PADDING - size[0],
        Place::TopCenter | Place::Center => content.x + (content.width - size[0]) / 2.0,
    };
    let y = match place {
        Place::TopRight | Place::TopCenter => content.y + PADDING,
        Place::Center => content.y + (content.height - size[1]) / 2.0,
    };
    Some(Rect::new(
        x.round(),
        y.round(),
        size[0].round(),
        size[1].round(),
    ))
}

/// The area a panel is drawn in: at `panel`, at `order` in the stack, and
/// taking the pointer from the picture under it — what lands on a panel
/// belongs to it rather than to what it is floating over.
pub fn area(name: &'static str, panel: Rect, order: egui::Order) -> egui::Area {
    egui::Area::new(egui::Id::new(name))
        .order(order)
        .fixed_pos(egui::pos2(panel.x, panel.y))
        .interactable(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTENT: Rect = Rect {
        x: 100.0,
        y: 50.0,
        width: 800.0,
        height: 600.0,
    };

    /// A panel of a fixed size stands where it is placed, inside the
    /// padding, and is refused rather than shrunk where the content cannot
    /// hold it.
    #[test]
    fn a_fixed_panel_is_placed_whole_or_not_at_all() {
        let size = [300.0, 200.0];
        let corner = fit(CONTENT, size, size, Place::TopRight).unwrap();
        assert_eq!(
            (corner.x, corner.y, corner.width, corner.height),
            (900.0 - PADDING - 300.0, 50.0 + PADDING, 300.0, 200.0)
        );
        let middle = fit(CONTENT, size, size, Place::Center).unwrap();
        assert_eq!((middle.x, middle.y), (350.0, 250.0));
        let top = fit(CONTENT, size, size, Place::TopCenter).unwrap();
        assert_eq!((top.x, top.y), (350.0, 50.0 + PADDING));

        let narrow = Rect::new(0.0, 0.0, 300.0 + 2.0 * PADDING - 1.0, 600.0);
        assert_eq!(fit(narrow, size, size, Place::TopRight), None);
        let just = Rect::new(0.0, 0.0, 300.0 + 2.0 * PADDING, 600.0);
        assert!(fit(just, size, size, Place::TopRight).is_some());
    }

    /// A panel that can give takes the room there is, down to the least it
    /// can be read at, and is rounded to whole pixels.
    #[test]
    fn a_panel_that_can_give_takes_the_room_and_no_less_than_it_needs() {
        let fitted = fit(CONTENT, [2000.0, 2000.0], [100.0, 100.0], Place::Center).unwrap();
        assert_eq!(
            (fitted.width, fitted.height),
            (800.0 - 2.0 * PADDING, 600.0 - 2.0 * PADDING)
        );
        assert_eq!((fitted.x, fitted.y), (100.0 + PADDING, 50.0 + PADDING));
        let short = Rect::new(0.0, 0.0, 800.0, 100.0 + 2.0 * PADDING - 1.0);
        assert_eq!(
            fit(short, [2000.0, 2000.0], [100.0, 100.0], Place::Center),
            None
        );
        let odd = Rect::new(0.3, 0.3, 500.7, 400.1);
        let fitted = fit(odd, [2000.0, 2000.0], [1.0, 1.0], Place::Center).unwrap();
        for value in [fitted.x, fitted.y, fitted.width, fitted.height] {
            assert_eq!(value, value.round());
        }
    }
}
