//! Popups: a floating panel of cells, laid out in named sections.
//!
//! Geometry and hit-testing, not a widget. What a cell has in it — a word, a
//! number, an icon — is the caller's, and so is what pressing one does; what
//! lives here is the part every popup shares, so that the code that opens
//! one, the code that draws it and the code that answers a click are all
//! reading the same rectangles.
//!
//! Sections are the popup's, though, rather than the caller's: a heading
//! belongs to the layout in a way a cell's contents do not, and a panel whose
//! sections were drawn by one piece of code and measured by another would be
//! a panel that could disagree with itself about where a row starts.

use super::{Color, Rect, UiFrame};

/// One section of a popup: a heading, and the cells standing under it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PopupSection {
    /// The name written above the cells, which is what parts one group of
    /// choices from the next.
    pub title: &'static str,
    /// How many cells the section holds.
    pub items: usize,
    /// How many of them go on a row.
    pub columns: usize,
    /// How wide one of this section's cells is.
    ///
    /// Its own, rather than a share of the panel: a cell is sized by what
    /// goes in it, and a row of three that has been stretched to the width of
    /// a row of four is three cells too big for what they hold. A section
    /// whose contents need the room — words rather than numbers — asks for it
    /// here, and a section that does not keeps the ordinary cell and leaves
    /// the rest of the row empty.
    pub cell_width: f32,
}

/// How a popup's cells are sized and spaced, in logical pixels.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PopupGrid {
    /// How tall one row is, in every section. Uniform where the widths are
    /// not: cells of a height read as one panel, and a row that stood taller
    /// than the row above it would read as a different kind of control.
    pub cell_height: f32,
    /// Between neighboring cells.
    pub gap: f32,
    /// Between the outermost cells and the panel's edge.
    pub padding: f32,
    /// Between the panel and the thing it hangs from, and the least it may
    /// come to the edge of the area it is placed in.
    pub margin: f32,
    /// The panel's corner radius.
    pub radius: f32,
    /// The line a section's name is set on, and the space between that name
    /// and the first row under it.
    pub heading: f32,
    pub heading_gap: f32,
    /// Between one section's last row and the next section's name. Wider than
    /// `heading_gap`, so that a name reads as belonging to what is under it
    /// rather than to what is above.
    pub section_gap: f32,
}

/// One section as it was laid out.
struct Placed {
    title: &'static str,
    /// Where this section's cells start in the popup's flat numbering, which
    /// is what a press comes back as.
    first: usize,
    items: usize,
    columns: usize,
    cell: [f32; 2],
    /// The top-left of the first cell of the first row.
    origin: [f32; 2],
    heading: Rect,
}

/// A popup placed in a window: where its panel is, where each section's name
/// goes, and where each of its cells landed.
pub struct Popup {
    grid: PopupGrid,
    panel: Rect,
    sections: Vec<Placed>,
    items: usize,
}

impl Popup {
    /// Places a popup of `sections` under `anchor` — the button that opens
    /// it — with its right edge in line with the anchor's, kept within
    /// `area`.
    ///
    /// Hung from the button rather than pinned to a corner of the window: a
    /// menu that appears somewhere other than under the thing pressed makes
    /// the reader look for it. `area` is only the bound it may not leave, so
    /// a menu is free to lie over whatever its button's neighbors are.
    ///
    /// The panel is cut for the widest row any section asks for; every
    /// section then lays its own cells out from the same left edge, at its
    /// own width, and a short row simply stops early. A section with nothing
    /// in it is left out rather than drawn as a heading over a blank row.
    ///
    /// `None` when `area` has no room for the whole of it, which is what
    /// keeps a popup off a window dragged down small: half a menu answers
    /// nothing, and shrinking the cells would only make them unreadable.
    pub fn below(
        sections: &[PopupSection],
        grid: PopupGrid,
        anchor: Rect,
        area: Rect,
    ) -> Option<Self> {
        let wanted: Vec<&PopupSection> = sections
            .iter()
            .filter(|section| section.items > 0 && section.columns > 0)
            .collect();
        let row_width = |section: &PopupSection| {
            section.columns as f32 * section.cell_width
                + section.columns.saturating_sub(1) as f32 * grid.gap
        };
        // The panel is as wide as the widest row it has to hold, and no
        // wider. Every other section starts at the same left edge and stops
        // where its own cells stop.
        let interior = wanted
            .iter()
            .map(|section| row_width(section))
            .fold(f32::NAN, f32::max);
        if !interior.is_finite() {
            return None;
        }
        let rows = |section: &PopupSection| section.items.div_ceil(section.columns);
        let section_height = |section: &PopupSection| {
            let rows = rows(section) as f32;
            grid.heading + grid.heading_gap + rows * grid.cell_height + (rows - 1.0) * grid.gap
        };

        let width = interior + 2.0 * grid.padding;
        let height = 2.0 * grid.padding
            + wanted
                .iter()
                .map(|section| section_height(section))
                .sum::<f32>()
            + wanted.len().saturating_sub(1) as f32 * grid.section_gap;

        let bounds = area.inset(grid.margin, grid.margin);
        if width > bounds.width || height > bounds.height {
            return None;
        }

        // Justified with the anchor, then slid back inside the bound; a menu
        // hanging off a button near the edge stays whole rather than running
        // off the window.
        let x = (anchor.right() - width).clamp(bounds.x, bounds.right() - width);
        let y = (anchor.bottom() + grid.margin).clamp(bounds.y, bounds.bottom() - height);
        // Whole logical pixels: everything inside is placed from this corner,
        // and a panel on a half pixel puts every label on one.
        let panel = Rect::new(x.round(), y.round(), width, height);

        let mut placed = Vec::with_capacity(wanted.len());
        let mut first = 0;
        let mut top = panel.y + grid.padding;
        for section in wanted {
            let cell = [section.cell_width, grid.cell_height];
            placed.push(Placed {
                title: section.title,
                first,
                items: section.items,
                columns: section.columns,
                cell,
                origin: [
                    panel.x + grid.padding,
                    top + grid.heading + grid.heading_gap,
                ],
                heading: Rect::new(panel.x + grid.padding, top, interior, grid.heading),
            });
            first += section.items;
            top += section_height(section) + grid.section_gap;
        }

        Some(Self {
            grid,
            panel,
            sections: placed,
            items: first,
        })
    }

    /// The panel the cells sit on. Nothing needs it to draw a popup — that
    /// is [`Popup::draw`] — but anything laying something out against a popup
    /// does.
    #[allow(dead_code)]
    pub fn panel(&self) -> Rect {
        self.panel
    }

    /// Each section's name and the line it is set on, in the order they were
    /// asked for. The words themselves are the caller's to draw: only the
    /// caller knows what ink a heading wants.
    pub fn headings(&self) -> impl Iterator<Item = (&'static str, Rect)> + '_ {
        self.sections
            .iter()
            .map(|section| (section.title, section.heading))
    }

    /// Where cell `index` sits, counting through the sections in order.
    ///
    /// The empty rectangle for an index past the end, which no caller should
    /// be asking for: a cell that is nowhere contains nothing, so a press
    /// cannot land on one by accident.
    pub fn cell(&self, index: usize) -> Rect {
        let Some(section) = self
            .sections
            .iter()
            .find(|section| index >= section.first && index < section.first + section.items)
        else {
            return Rect::new(self.panel.x, self.panel.y, 0.0, 0.0);
        };
        let place = index - section.first;
        let row = place / section.columns;
        let column = place % section.columns;
        Rect::new(
            section.origin[0] + column as f32 * (section.cell[0] + self.grid.gap),
            section.origin[1] + row as f32 * (section.cell[1] + self.grid.gap),
            section.cell[0],
            section.cell[1],
        )
    }

    /// Every cell with its index, in the order they were asked for: what
    /// drawing iterates over.
    pub fn cells(&self) -> impl Iterator<Item = (usize, Rect)> + '_ {
        (0..self.items).map(|index| (index, self.cell(index)))
    }

    /// Which cell a point lands on, if any. The gaps between cells and the
    /// lines the headings are set on belong to no cell, which is why this and
    /// [`Popup::contains`] are separate questions: a press in a gap is inside
    /// the popup without choosing anything.
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

    /// Lays the panel down. The cells and the headings go on top of it and
    /// are the caller's to draw, since only the caller knows what they say.
    pub fn draw(&self, frame: &mut UiFrame, background: Color) {
        frame.rounded_rect(self.panel, self.grid.radius, background);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 900.0,
        height: 640.0,
    };

    /// The button the menu hangs from, near the right of the area.
    const ANCHOR: Rect = Rect {
        x: 800.0,
        y: 4.0,
        width: 60.0,
        height: 22.0,
    };

    /// Three sections in the shape the zoom menu uses them: a wide one of
    /// numbers, a row of icons, and a pair wearing words.
    const SECTIONS: [PopupSection; 3] = [
        PopupSection {
            title: "Zoom",
            items: 8,
            columns: 4,
            cell_width: 50.0,
        },
        PopupSection {
            title: "Fit",
            items: 3,
            columns: 3,
            cell_width: 50.0,
        },
        PopupSection {
            title: "Up-scaling",
            items: 2,
            columns: 2,
            cell_width: 70.0,
        },
    ];

    fn grid() -> PopupGrid {
        PopupGrid {
            cell_height: 30.0,
            gap: 5.0,
            padding: 10.0,
            margin: 12.0,
            radius: 8.0,
            heading: 16.0,
            heading_gap: 4.0,
            section_gap: 12.0,
        }
    }

    fn interior() -> f32 {
        4.0 * 50.0 + 3.0 * 5.0
    }

    #[test]
    fn the_panel_wraps_the_sections_and_hangs_from_the_anchor() {
        let popup = Popup::below(&SECTIONS, grid(), ANCHOR, AREA).expect("room enough");
        let panel = popup.panel();

        // As wide as the widest row, which is the four-column one.
        assert_eq!(panel.width, interior() + 2.0 * 10.0);
        // Two rows, then one, then one — each under a heading, and the three
        // parted by the section gap.
        let section = |rows: f32| 16.0 + 4.0 + rows * 30.0 + (rows - 1.0) * 5.0;
        assert_eq!(
            panel.height,
            2.0 * 10.0 + section(2.0) + section(1.0) + section(1.0) + 2.0 * 12.0
        );
        // Right edges in line, and clear of the button by the margin.
        assert_eq!(panel.right(), ANCHOR.right());
        assert_eq!(panel.y, ANCHOR.bottom() + 12.0);

        assert_eq!(popup.cells().count(), 13);
        for (_, cell) in popup.cells() {
            assert!(
                cell.x >= panel.x && cell.right() <= panel.right() + 0.01,
                "{cell:?}"
            );
            assert!(
                cell.y >= panel.y && cell.bottom() <= panel.bottom(),
                "{cell:?}"
            );
        }
    }

    /// Each section keeps its own cell width and starts at the same left
    /// edge, so a short row stops early rather than being stretched across
    /// the panel. The panel is cut for the widest row and no wider.
    #[test]
    fn a_section_keeps_its_own_cell_width_and_does_not_fill_the_row() {
        let popup = Popup::below(&SECTIONS, grid(), ANCHOR, AREA).expect("room enough");
        let panel = popup.panel();

        // Every section starts flush left, under the one above it.
        for first in [0, 8, 11] {
            assert_eq!(popup.cell(first).x, panel.x + 10.0, "section at {first}");
        }

        // The widest row is the eight-cell one, and it is what the panel was
        // cut for: only that section reaches the far padding.
        assert_eq!(popup.cell(3).right(), panel.right() - 10.0);
        assert!(popup.cell(10).right() < panel.right() - 10.0, "the fits");
        assert!(popup.cell(12).right() < panel.right() - 10.0, "the words");

        // The cells are the widths their sections asked for, and the two that
        // share one are equal rather than merely similar.
        assert_eq!(popup.cell(0).width, 50.0);
        assert_eq!(popup.cell(8).width, 50.0);
        assert_eq!(popup.cell(11).width, 70.0);

        // One height throughout, whatever the widths.
        for (_, cell) in popup.cells() {
            assert_eq!(cell.height, 30.0, "{cell:?}");
        }
    }

    /// A heading stands over its own section and takes no press.
    #[test]
    fn each_section_is_named_above_the_cells_it_covers() {
        let popup = Popup::below(&SECTIONS, grid(), ANCHOR, AREA).expect("room enough");
        let headings: Vec<_> = popup.headings().collect();
        assert_eq!(
            headings.iter().map(|(title, _)| *title).collect::<Vec<_>>(),
            ["Zoom", "Fit", "Up-scaling"]
        );

        for ((_, heading), first) in headings.iter().zip([0, 8, 11]) {
            let cell = popup.cell(first);
            assert!(heading.bottom() <= cell.y, "{heading:?} over {cell:?}");
            assert_eq!(heading.x, cell.x);
            // On the panel, but not a choice.
            let middle = [heading.x + 4.0, heading.y + heading.height / 2.0];
            assert!(popup.contains(middle));
            assert_eq!(popup.item_at(middle), None);
        }
    }

    /// A button hard against an edge would justify the panel off the window,
    /// so the panel slides back inside and stops at the margin.
    #[test]
    fn a_panel_that_would_hang_off_the_area_is_slid_back_inside_it() {
        let edge = Rect::new(AREA.right() - 20.0, 4.0, 20.0, 22.0);
        let popup = Popup::below(&SECTIONS, grid(), edge, AREA).expect("room enough");
        assert_eq!(popup.panel().right(), AREA.right() - 12.0);

        // And one near the bottom is lifted rather than run off it.
        let low = Rect::new(800.0, AREA.bottom() - 30.0, 60.0, 22.0);
        let popup = Popup::below(&SECTIONS, grid(), low, AREA).expect("room enough");
        assert_eq!(popup.panel().bottom(), AREA.bottom() - 12.0);
    }

    #[test]
    fn a_point_finds_the_cell_it_is_over_and_nothing_in_the_gaps() {
        let popup = Popup::below(&SECTIONS, grid(), ANCHOR, AREA).expect("room enough");

        for (index, cell) in popup.cells() {
            let middle = [cell.x + cell.width / 2.0, cell.y + cell.height / 2.0];
            assert_eq!(popup.item_at(middle), Some(index));
        }

        // The gap between the first two cells is inside the popup but is not
        // a choice, and neither is an index past the last section.
        let first = popup.cell(0);
        let gap = [first.right() + 2.0, first.y + 5.0];
        assert!(popup.contains(gap));
        assert_eq!(popup.item_at(gap), None);
        let missing = popup.cell(13);
        assert_eq!(missing.width, 0.0);
        assert_eq!(popup.item_at([missing.x + 1.0, missing.y + 1.0]), None);

        assert!(!popup.contains([AREA.x + 1.0, AREA.y + 1.0]));
        assert_eq!(popup.item_at([AREA.x + 1.0, AREA.y + 1.0]), None);
    }

    /// A section with nothing in it is not a heading over a blank row: it is
    /// left out, and the numbering closes up behind it.
    #[test]
    fn an_empty_section_is_left_out_altogether() {
        let sections = [
            SECTIONS[0],
            PopupSection {
                title: "Nothing",
                items: 0,
                columns: 2,
                cell_width: 70.0,
            },
            SECTIONS[2],
        ];
        let popup = Popup::below(&sections, grid(), ANCHOR, AREA).expect("room enough");

        assert_eq!(
            popup.headings().map(|(title, _)| title).collect::<Vec<_>>(),
            ["Zoom", "Up-scaling"]
        );
        assert_eq!(popup.cells().count(), 10);
        // The up-scaling cells are 8 and 9 now, not 11 and 12.
        assert_eq!(popup.cell(8).width, 70.0);

        assert!(Popup::below(&[], grid(), ANCHOR, AREA).is_none());
    }

    /// A window too small for the whole grid gets no popup rather than a
    /// clipped one.
    #[test]
    fn a_cramped_area_has_no_room_for_one() {
        let tiny = Rect::new(0.0, 0.0, 120.0, 400.0);
        assert!(Popup::below(&SECTIONS, grid(), ANCHOR, tiny).is_none());
        let short = Rect::new(0.0, 0.0, 400.0, 60.0);
        assert!(Popup::below(&SECTIONS, grid(), ANCHOR, short).is_none());
    }
}
