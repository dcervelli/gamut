//! A section of the column: which stage of the pipeline it is, how tall it
//! stands, where it goes, and the line it opens with.

use egui::{Align2, FontId, pos2};

use super::*;

/// One stage of the picture's way from the file to the screen, as a
/// section of the column. In the order the column lays them out, which is
/// the order the data flows in read back from the screen: what goes out at
/// the top, since that is the question the panel is most often opened to
/// answer, and the file at the foot.
///
/// A stage the picture does not pass through has no section — the gain map
/// on a picture with none — and one a later change adds goes in where it
/// stands in the flow, between the display and the file, at a height of its
/// own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    /// What goes out to the screen: the display's response to every value,
    /// clipped where the surface stops.
    Output,
    /// What the display starts from, and what it does to it: the window,
    /// the exposure and the curve, which are set here.
    Display,
    /// How far the gain map lifts the picture, where it has one.
    GainMap,
    /// The picture as the file stores it.
    File,
}

impl Section {
    /// Every section there can be, in the order the column lays them out.
    pub const ALL: [Section; 4] = [
        Section::Output,
        Section::Display,
        Section::GainMap,
        Section::File,
    ];

    /// What the section's header calls it.
    pub fn title(self) -> &'static str {
        match self {
            Section::Output => "Output",
            Section::Display => "Display",
            Section::GainMap => "Gain map",
            Section::File => "File",
        }
    }

    /// The mark beside its name, in the accent, as the information panel's
    /// sections wear theirs: a screen for what goes out, the sliders the
    /// display is set by, the sun the gain map is known by elsewhere in the
    /// window, and a sheet with a picture on it for the file.
    pub(in crate::ui) fn mark(self) -> &'static [icon::Mark] {
        match self {
            Section::Output => icon::MONITOR,
            Section::Display => icon::SLIDERS_HORIZONTAL,
            Section::GainMap => icon::SUN,
            Section::File => icon::FILE_IMAGE,
        }
    }

    /// Whether `current` passes through this stage.
    pub fn applies(self, current: &Current) -> bool {
        match self {
            Section::GainMap => current.picture_face().image.gain_map.is_some(),
            Section::Output | Section::Display | Section::File => true,
        }
    }

    /// How tall the section stands, for every file: its header, its plot,
    /// and what hangs under the plot. Fixed, so that one file's column is
    /// laid out where the last one's was, and a section is never shrunk to
    /// fit a short window — the column scrolls instead.
    pub const fn height(self) -> f32 {
        let head = LABEL_HEIGHT + HEAD_GAP + PLOT_INSET;
        let band = PLOT_INSET + RAMP_GAP + RAMP_HEIGHT;
        match self {
            // The plot and the band of what each output comes out as.
            Section::Output => head + PLOT_HEIGHT + band,
            // The plot, the band, and the three rows of settings under it,
            // which take the inset under the plot as their own: see
            // `display::ROWS_HEIGHT`.
            Section::Display => {
                head + PLOT_HEIGHT
                    + RAMP_GAP
                    + display::SWATCH_HEIGHT
                    + RAMP_GAP
                    + RAMP_HEIGHT
                    + display::ROWS_HEIGHT
            }
            // A shorter plot, and the band of how much of the lift is
            // applied.
            Section::GainMap => head + SHORT_PLOT_HEIGHT + band,
            // A shorter plot, and nothing under it.
            Section::File => head + SHORT_PLOT_HEIGHT + PLOT_INSET,
        }
    }

    /// The ground the bins stand on in the section laid out at `rect`: the
    /// whole width of the column, less the inset the ground is drawn out
    /// into, which leaves the bins a point each.
    pub fn plot(self, rect: Rect, gray: bool) -> Rect {
        let x = rect.x + PLOT_INSET;
        let y = rect.y + LABEL_HEIGHT + HEAD_GAP + PLOT_INSET;
        let width = rect.width - 2.0 * PLOT_INSET;
        match self {
            Section::Output => Rect::new(x, y, width, PLOT_HEIGHT),
            Section::Display => display::plot_area(rect, gray),
            Section::GainMap | Section::File => Rect::new(x, y, width, SHORT_PLOT_HEIGHT),
        }
    }
}

/// How tall the column is for `current`: every section it passes through,
/// and the gaps between them.
pub fn height(current: &Current) -> f32 {
    let sections: Vec<Section> = Section::ALL
        .into_iter()
        .filter(|section| section.applies(current))
        .collect();
    sections.iter().map(|section| section.height()).sum::<f32>()
        + (sections.len().saturating_sub(1)) as f32 * SECTION_GAP
}

/// Where each section `current` passes through goes in the column laid out
/// from the top of `column`: one under the other, each at its own height and
/// the column's width, whatever height `column` has.
pub fn layout(column: Rect, current: &Current) -> Vec<(Section, Rect)> {
    let mut y = column.y;
    Section::ALL
        .into_iter()
        .filter(|section| section.applies(current))
        .map(|section| {
            let rect = Rect::new(column.x, y, column.width, section.height());
            y += section.height() + SECTION_GAP;
            (section, rect)
        })
        .collect()
}

/// The section's header line, in the section laid out at `rect`.
pub fn header_line(rect: Rect) -> Rect {
    Rect::new(rect.x, rect.y, rect.width, LABEL_HEIGHT)
}

/// The square at the end of a section's header that its own button takes,
/// where it has one: the gain map's, which shows the map. Kept clear in
/// every section, so that the readouts end on one line down the column.
pub fn button(rect: Rect) -> Rect {
    let line = header_line(rect);
    Rect::new(
        line.right() - LABEL_HEIGHT,
        line.y,
        LABEL_HEIGHT,
        LABEL_HEIGHT,
    )
}

/// The line a section opens with, as the information panel's sections
/// open: the section's mark and name in the accent, the one thing on the
/// panel picked out in it; and on the right what the pointer is reading at
/// its stage — `readout` — set short of the square the section's own button
/// takes, [`button`].
pub fn header(pass: &Pass, ui: &egui::Ui, section: Section, rect: Rect, readout: Option<&str>) {
    let theme = pass.theme;
    let grid = pass.grid;
    let painter = ui.painter();
    let line = header_line(rect);
    let middle = line.y + line.height / 2.0;
    let mark = egui::Rect::from_min_size(pos2(line.x, line.y), egui::vec2(ICON_SIDE, line.height));
    icon::paint(
        painter,
        section.mark(),
        icon::square(grid, mark, ICON_SIDE),
        theme.accent.into(),
        theme.bar_background.into(),
    );
    painter.text(
        pos2(grid.snap(line.x + ICON_SIDE + MARK_GAP), middle),
        Align2::LEFT_CENTER,
        section.title(),
        FontId::proportional(TEXT_SIZE),
        theme.accent.into(),
    );
    if let Some(readout) = readout {
        painter.text(
            pos2(grid.snap(readout_end(line)), middle),
            Align2::RIGHT_CENTER,
            readout,
            FontId::proportional(ROW_TEXT),
            theme.text_primary.into(),
        );
    }
}

/// Where a header's readout ends: short of the square at the end of the
/// line that a section's own button takes.
pub fn readout_end(line: Rect) -> f32 {
    line.right() - LABEL_HEIGHT - CELL_GAP
}
