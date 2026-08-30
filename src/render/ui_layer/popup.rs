//! Popups: a floating panel of cells, laid out as a grid.
//!
//! Geometry and hit-testing, not a widget. What a cell has in it — a word, a
//! number, an icon — is the caller's, and so is what pressing one does; what
//! lives here is the part every popup shares, so that the code that opens
//! one, the code that draws it and the code that answers a click are all
//! reading the same rectangles.

use super::{Color, Rect, UiFrame};

/// Which corner of the area a popup is anchored to. It grows inwards from
/// there, so the corner it is pinned to is the one that stays put as the
/// window is resized.
///
/// All four are here although the only popup written so far anchors to one of
/// them: a corner the caller cannot ask for is a trap for the next popup, and
/// the arithmetic for it is one line.
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// How a popup's cells are sized and spaced, in logical pixels.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PopupGrid {
    /// One cell. Uniform across the grid: a set of choices reads as a set
    /// only while each is the same size as the one beside it.
    pub cell: [f32; 2],
    /// Cells to a row. The last row takes whatever is left over.
    pub columns: usize,
    /// Between neighbouring cells.
    pub gap: f32,
    /// Between the outermost cells and the panel's edge.
    pub padding: f32,
    /// Between the panel and the edge of the area it is anchored in.
    pub margin: f32,
    /// The panel's corner radius.
    pub radius: f32,
}

/// A popup placed in a window: where its panel is, and where each of its
/// cells landed.
pub struct Popup {
    grid: PopupGrid,
    panel: Rect,
    items: usize,
}

impl Popup {
    /// Places a popup of `items` cells in the corner `at` of `area`.
    ///
    /// `None` when `area` has no room for the whole of it, which is what
    /// keeps a popup off a window dragged down small: half a menu answers
    /// nothing, and shrinking the cells would only make them unreadable.
    pub fn new(items: usize, grid: PopupGrid, area: Rect, at: Corner) -> Option<Self> {
        if items == 0 || grid.columns == 0 {
            return None;
        }
        // A grid with fewer choices than it has room for is as wide as the
        // choices it actually holds, not as wide as the row it was allowed.
        let columns = grid.columns.min(items);
        let rows = items.div_ceil(grid.columns);
        let span = |count: usize, cell: f32| {
            count as f32 * cell + count.saturating_sub(1) as f32 * grid.gap + 2.0 * grid.padding
        };
        let width = span(columns, grid.cell[0]);
        let height = span(rows, grid.cell[1]);
        if width + 2.0 * grid.margin > area.width || height + 2.0 * grid.margin > area.height {
            return None;
        }

        let x = match at {
            Corner::TopLeft | Corner::BottomLeft => area.x + grid.margin,
            Corner::TopRight | Corner::BottomRight => area.right() - grid.margin - width,
        };
        let y = match at {
            Corner::TopLeft | Corner::TopRight => area.y + grid.margin,
            Corner::BottomLeft | Corner::BottomRight => area.bottom() - grid.margin - height,
        };

        Some(Self {
            // Whole logical pixels: everything inside is placed from this
            // corner, and a panel on a half pixel puts every label on one.
            panel: Rect::new(x.round(), y.round(), width, height),
            grid: PopupGrid { columns, ..grid },
            items,
        })
    }

    /// The panel the cells sit on. Nothing needs it to draw a popup — that
    /// is [`Popup::draw`] — but anything laying something out against a popup
    /// does.
    #[allow(dead_code)]
    pub fn panel(&self) -> Rect {
        self.panel
    }

    /// Where cell `index` sits. Out-of-range indices are laid out as though
    /// the grid ran on, which no caller should be asking for but which is
    /// cheaper than an option nobody would check.
    pub fn cell(&self, index: usize) -> Rect {
        let row = index / self.grid.columns;
        let column = index % self.grid.columns;
        Rect::new(
            self.panel.x + self.grid.padding + column as f32 * (self.grid.cell[0] + self.grid.gap),
            self.panel.y + self.grid.padding + row as f32 * (self.grid.cell[1] + self.grid.gap),
            self.grid.cell[0],
            self.grid.cell[1],
        )
    }

    /// Every cell with its index, in the order they were asked for: what
    /// drawing iterates over.
    pub fn cells(&self) -> impl Iterator<Item = (usize, Rect)> + '_ {
        (0..self.items).map(|index| (index, self.cell(index)))
    }

    /// Which cell a point lands on, if any. The gaps between cells belong to
    /// no cell, which is why this and [`Popup::contains`] are separate
    /// questions: a press in a gap is inside the popup without choosing
    /// anything.
    pub fn item_at(&self, point: [f32; 2]) -> Option<usize> {
        if !self.contains(point) {
            return None;
        }
        self.cells()
            .find(|(_, cell)| cell.contains(point))
            .map(|(index, _)| index)
    }

    /// Whether a point is on the popup at all, which is what tells a press
    /// meant for the popup from the press that dismisses it.
    pub fn contains(&self, point: [f32; 2]) -> bool {
        self.panel.contains(point)
    }

    /// Lays the panel down. The cells go on top of it and are the caller's to
    /// draw, since only the caller knows what they say.
    pub fn draw(&self, frame: &mut UiFrame, background: Color) {
        frame.rounded_rect(self.panel, self.grid.radius, background);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect {
        x: 50.0,
        y: 30.0,
        width: 900.0,
        height: 640.0,
    };

    fn grid() -> PopupGrid {
        PopupGrid {
            cell: [50.0, 30.0],
            columns: 4,
            gap: 5.0,
            padding: 10.0,
            margin: 12.0,
            radius: 8.0,
        }
    }

    #[test]
    fn the_panel_wraps_the_cells_and_sits_in_the_corner_asked_for() {
        let popup = Popup::new(11, grid(), AREA, Corner::BottomRight).expect("room enough");
        let panel = popup.panel();

        // Four to a row and eleven of them is three rows, the last short.
        assert_eq!(panel.width, 4.0 * 50.0 + 3.0 * 5.0 + 2.0 * 10.0);
        assert_eq!(panel.height, 3.0 * 30.0 + 2.0 * 5.0 + 2.0 * 10.0);
        assert_eq!(panel.right(), AREA.right() - 12.0);
        assert_eq!(panel.bottom(), AREA.bottom() - 12.0);

        // Every cell is inside the panel, and the padding is even.
        for (_, cell) in popup.cells() {
            assert!(
                cell.x >= panel.x && cell.right() <= panel.right(),
                "{cell:?}"
            );
            assert!(
                cell.y >= panel.y && cell.bottom() <= panel.bottom(),
                "{cell:?}"
            );
        }
        let first = popup.cell(0);
        let last_of_row = popup.cell(3);
        assert_eq!(first.x - panel.x, panel.right() - last_of_row.right());
        assert_eq!(first.y - panel.y, 10.0);
    }

    #[test]
    fn each_corner_pins_the_panel_to_itself() {
        let of = |at| {
            Popup::new(8, grid(), AREA, at)
                .expect("room enough")
                .panel()
        };
        assert_eq!((of(Corner::TopLeft).x, of(Corner::TopLeft).y), (62.0, 42.0));
        assert_eq!(of(Corner::TopRight).right(), AREA.right() - 12.0);
        assert_eq!(of(Corner::BottomLeft).bottom(), AREA.bottom() - 12.0);
        assert_eq!(of(Corner::BottomLeft).x, 62.0);
    }

    #[test]
    fn a_point_finds_the_cell_it_is_over_and_nothing_in_the_gaps() {
        let popup = Popup::new(11, grid(), AREA, Corner::BottomRight).expect("room enough");

        for (index, cell) in popup.cells() {
            let middle = [cell.x + cell.width / 2.0, cell.y + cell.height / 2.0];
            assert_eq!(popup.item_at(middle), Some(index));
        }

        // The gap between the first two cells is inside the popup but is not
        // a choice, and the row the eleventh cell is missing from is neither.
        let first = popup.cell(0);
        let gap = [first.right() + 2.0, first.y + 5.0];
        assert!(popup.contains(gap));
        assert_eq!(popup.item_at(gap), None);
        let missing = popup.cell(11);
        assert_eq!(popup.item_at([missing.x + 1.0, missing.y + 1.0]), None);

        assert!(!popup.contains([AREA.x + 1.0, AREA.y + 1.0]));
        assert_eq!(popup.item_at([AREA.x + 1.0, AREA.y + 1.0]), None);
    }

    /// A window too small for the whole grid gets no popup rather than a
    /// clipped one.
    #[test]
    fn a_cramped_area_has_no_room_for_one() {
        let tiny = Rect::new(0.0, 0.0, 120.0, 400.0);
        assert!(Popup::new(11, grid(), tiny, Corner::BottomRight).is_none());
        let short = Rect::new(0.0, 0.0, 400.0, 60.0);
        assert!(Popup::new(11, grid(), short, Corner::BottomRight).is_none());
        assert!(Popup::new(0, grid(), AREA, Corner::BottomRight).is_none());
    }
}
