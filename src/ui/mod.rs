//! Building each frame's interface: a display list of panels, widgets and
//! text, laid out in logical pixels and handed to the renderer's UI layer.
//!
//! Nothing here touches the GPU or the window. The state it needs is passed
//! in — what is on screen, which panels are showing, what this frame's
//! geometry is — so that a frame can be built against anything that can
//! measure text.

pub mod chrome;
pub mod info;
pub mod layers;
pub mod menu;
pub mod minimap;

mod buttons;
mod grid;
pub mod histogram;
mod icon;
mod pixel;
mod status;

use std::sync::Arc;

use crate::image::display::Display;
use crate::image::exif::Exif;
use crate::image::stats::BINS;
use crate::image::{DecodedImage, Stats};
use crate::render::{Backdrop, Rect, TextMeasure, UiFrame};
use crate::theme::Theme;
use crate::view::{View, Viewport};

use chrome::{BAR_PADDING, Chrome};
pub use info::FileFacts;
pub use menu::Menu;

const TEXT_SIZE: f32 = 13.0;

/// What stands between a value and what the display makes of it, in every
/// readout that shows one turning into the other.
///
/// Not an arrow, though an arrow is what it means. The interface's face has
/// no U+2192 of its own, so a run holding one is broken in two and the arrow
/// comes from whichever fallback the font stack offers — a monospace face
/// here, whose arrow is drawn small and low in the em and sits visibly under
/// the line it was set on. A guillemet is Latin-1, so every face the
/// interface could be set in has one, drawn on the same line as the words
/// either side of it.
pub(super) const BECOMES: &str = "\u{00bb}";

/// The gap between the file count and the name it belongs to. Tighter than
/// the gap between two unrelated things in a bar, the two being one line
/// about one file.
const COUNTER_GAP: f32 = 8.0;

/// The gap between two neighbours: what floats over the content area from the
/// edge of that area, and one thing in a bar from the next.
///
/// Not what a bar is inset by at its ends — that is
/// [`chrome::BAR_PADDING`], which is tighter, so that the bars
/// and the side panels share one line down each edge of the window.
const PADDING: f32 = 12.0;

/// The gap between a floating panel's edge and what is on it.
const PANEL_INSET: f32 = 10.0;

/// How wide the panels that float over the content area are. The histogram
/// fixes it: wide enough that a bin is exactly one logical pixel, which is
/// what keeps its bars evenly spaced instead of some of them landing astride
/// a pixel boundary and coming out fatter than their neighbours. The
/// information panel takes the same width so that the two line up down the
/// right of the window, whether or not either has anything else on it.
const PANEL_WIDTH: f32 = histogram::TOOLBAR_WIDTH + BINS as f32 + 2.0 * PANEL_INSET;

/// The corner radius of a floating panel.
const PANEL_RADIUS: f32 = 6.0;

/// Side of one checkerboard square, in logical pixels. Small enough to read
/// as a texture behind the image rather than as a pattern competing with it.
const CHECKER_SQUARE: f32 = 8.0;

/// Something in the interface the pointer can be over and press: a toggle in
/// a side panel, one of the two buttons at the end of the top bar, or a cell
/// of the menu the zoom readout opens. One value rather than a flag each, so that
/// hit-testing, hover and drawing all go through the same test.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Widget {
    Minimap,
    Histogram,
    Info,
    Grid,
    Zoom,
    /// A cell of whichever menu is open. Which menu that is is
    /// [`Panels::menu`], so a cell needs only its place in the grid.
    Cell(usize),
    /// The two plane toggles, the switch between a linear and a logarithmic
    /// count axis, and the button that puts the rendering back, down the left
    /// of the histogram panel.
    Luma,
    Planes,
    Log,
    Reset,
    /// One of the false colours offered under that panel's ramp, by its place
    /// in [`crate::image::display::Colormap::ALL`].
    Ramp(usize),
}

/// The image on screen, with everything derived from it.
pub struct Current {
    /// Shared rather than owned: copying the picture to the clipboard walks
    /// every pixel on a thread of its own, and handing that thread the image
    /// must not mean duplicating however many hundred megabytes it is.
    pub image: Arc<DecodedImage>,
    pub stats: Stats,
    pub display: Display,
    pub label: String,
    /// What the file it came from says about itself, for the info panel.
    pub file: FileFacts,
    /// And what its metadata says about the photograph, if it carries any.
    pub exif: Exif,
    /// What the GPU actually stored it as, which is not always what we asked.
    pub stored: Option<String>,
}

impl Current {
    pub fn size(&self) -> [f32; 2] {
        [self.image.width as f32, self.image.height as f32]
    }
}

/// What the top bar says about a read that is taking its time.
pub enum Reading {
    /// Another file, on its way in.
    File(String),
    /// The file already on screen, being read again after something wrote to
    /// it. There is no new name to show, only the fact that we are busy.
    Again,
}

/// The interface's panels: whether each is showing, and which toggle the
/// pointer is over.
#[derive(Clone, Copy, Debug)]
pub struct Panels {
    /// Whether the four panels are on screen. They are opaque and the image
    /// is fitted inside them, so hiding them gives it the whole window.
    pub show_ui: bool,
    pub show_histogram: bool,
    pub show_info: bool,
    /// How far the info panel's column has been scrolled, in logical pixels.
    /// Kept here rather than in the panel because the panel is rebuilt every
    /// frame, and clamped where it is used: what it may run to depends on how
    /// tall the text comes out in the window as it is now.
    pub info_scroll: f32,
    /// Which of the histogram's planes are drawn. Both can be off: the panel
    /// still has its response curve and its ramp to read, and a toggle that
    /// refuses to switch off is a toggle that owes an explanation.
    pub show_luma: bool,
    pub show_planes: bool,
    /// Whether the plot's bars are as tall as the logarithm of their counts
    /// rather than as tall as the counts themselves. Off by default, which is
    /// what a photograph wants; on, a measurement's one dominating bin — a
    /// masked sea, the surround of a scan — stops flattening everything else.
    pub log_counts: bool,
    /// Whether the minimap is switched on. Whether it is actually on screen
    /// also asks whether there is anything off screen for it to point out —
    /// see [`FrameInput::minimap_on_screen`].
    pub show_minimap: bool,
    /// Whether the grid is laid over the image. How far apart its lines are
    /// is not held: it follows the zoom, and is worked out afresh each frame
    /// by [`grid::step`].
    pub show_grid: bool,
    /// Which widget the pointer is over. Held rather than recomputed while
    /// drawing so that motion knows when the highlight has changed and a
    /// redraw is actually owed.
    pub hover: Option<Widget>,
    /// And which part of the info panel's column, for the same reason. Held
    /// apart from `hover` because the column is not one of the chrome's
    /// widgets: it moves as the panel scrolls, and it is there whether or not
    /// the bars are.
    pub info_hover: Option<info::Copyable>,
    /// The menu popped up over the interface, if any. It is drawn over
    /// everything and takes the pointer while it is open: a press on a cell
    /// chooses, one anywhere else dismisses it, and the wheel is spent on it
    /// — see [`layers`].
    pub menu: Option<Menu>,
}

/// What this frame looks like, beyond the image and the panels: the values
/// the application derives per frame from its window, pointer and loader.
pub struct FrameInput {
    /// Window size in logical pixels, which is what the interface lays out in.
    pub logical: [f32; 2],
    /// Physical pixels to the logical one, for the few places that have to
    /// land on the device's grid rather than on the layout's.
    pub scale: f32,
    /// Where the image is drawn, which is what zoom is measured against.
    pub viewport: Viewport,
    /// The image pixel under the pointer, when it is over one.
    pub pointer: Option<[u32; 2]>,
    /// And where the pointer itself is, in the logical pixels the interface
    /// is laid out in, for the widgets that read it against their own
    /// geometry rather than against the image. `None` once it has left the
    /// window, which is what takes those readouts back off the screen.
    pub cursor: Option<[f32; 2]>,
    /// Whether the minimap is on screen, which takes the toggle and a view
    /// with part of the image off it.
    pub minimap_on_screen: bool,
    /// The read in progress, once it has taken long enough to be worth saying.
    /// Kept apart from the label rather than replacing it: everything else in
    /// the interface describes the image on screen, and so must that.
    pub reading: Option<Reading>,
    /// Which file is on screen, out of how many.
    pub index: usize,
    pub count: usize,
    /// Whether the file the picture came from is no longer there. The picture
    /// stays up — its pixels are as good as they ever were, and there is
    /// nothing to put in its place — so the bar is where this is said.
    pub deleted: bool,
    /// The output's label when it is an HDR surface, which is worth a word in
    /// the bar; `None` on an ordinary one.
    pub hdr_output: Option<&'static str>,
}

/// A stand-in for the renderer's fonts, for the tests that lay something out
/// without a GPU: every glyph one `size` square, so that a test can say how
/// much room a string has in whole characters.
#[cfg(test)]
pub(super) struct Monospace;

#[cfg(test)]
impl TextMeasure for Monospace {
    fn measure_text(&mut self, text: &str, size: f32) -> [f32; 2] {
        [text.chars().count() as f32 * size, size]
    }

    /// Every glyph is a `size` square sitting on the top of its line here, so
    /// its middle is half a square down.
    fn cap_centre(&mut self, size: f32) -> f32 {
        size / 2.0
    }

    fn measure_mono(&mut self, text: &str, size: f32) -> [f32; 2] {
        self.measure_text(text, size)
    }

    fn measure_wrapped(&mut self, text: &str, size: f32, width: f32) -> [f32; 2] {
        // Broken between words, as glyphon breaks it, and at the same line
        // height the text layer sets its metrics to.
        let columns = (width / size).floor().max(1.0) as usize;
        let (mut lines, mut used) = (1usize, 0usize);
        for word in text.split_whitespace() {
            let length = word.chars().count();
            if used == 0 {
                used = length;
            } else if used + 1 + length <= columns {
                used += 1 + length;
            } else {
                lines += 1;
                used = length;
            }
        }
        [width, lines as f32 * size * 1.3]
    }
}

/// What the grid toggle reads out while the grid is on: how far apart its
/// lines are at `zoom`, on a display of `scale` physical pixels to the
/// logical one. `None` while it is off, there being no spacing in force then.
///
/// Worked out here rather than inside the toggle because it is also what says
/// how wide the toggle is, and the pointer has to be answered against the
/// width the frame was drawn at — see [`layers::hit`].
pub fn grid_spacing(show_grid: bool, zoom: f32, scale: f32) -> Option<String> {
    show_grid.then(|| grid::label(grid::step(zoom, scale)))
}

/// Builds one frame of interface.
pub fn build_frame(
    text: &mut dyn TextMeasure,
    input: &FrameInput,
    panels: &Panels,
    current: Option<&Current>,
    view: &View,
    theme: &Theme,
) -> UiFrame {
    let size = input.logical;
    let mut frame = UiFrame::new(input.scale);
    let chrome = Chrome::new(size);

    let Some(current) = current else {
        // Nothing has been decoded yet. The panels still go down, so that the
        // window reads as the application waiting rather than as a hole, with
        // the file being read where the image's own name will go.
        if panels.show_ui {
            for panel in chrome.panels() {
                frame.rect(panel, theme.bar_background);
            }
            if let Some(Reading::File(name)) = &input.reading {
                frame.text_clipped(
                    [BAR_PADDING, text_baseline(chrome.top)],
                    TEXT_SIZE,
                    theme.text_dim,
                    (chrome.top.width - BAR_PADDING * 2.0).max(1.0),
                    format!("loading {name}"),
                );
            }
        }
        return frame;
    };
    let content = chrome::content_area(size, panels.show_ui);
    let zoom = view.zoom(current.size(), input.viewport);
    let grid_step = grid::step(zoom, input.scale);

    // Under the floating panels, which are read against the image and would
    // be harder to read over a grid as well.
    if panels.show_grid {
        grid::draw(&mut frame, current, view, input, content, grid_step, theme);
    }
    if panels.show_histogram {
        histogram::draw(&mut frame, text, current, input, panels, content, theme);
    }
    if input.minimap_on_screen {
        minimap::draw(&mut frame, current, view, input, content, theme);
    }
    if panels.show_info {
        info::draw(&mut frame, text, current, panels, content, theme);
    }
    if !panels.show_ui {
        return frame;
    }

    for panel in chrome.panels() {
        frame.rect(panel, theme.bar_background);
    }
    for border in chrome.borders() {
        frame.hairline(border, theme.border);
    }

    // Top panel: what the image is. Everything here is a property of the
    // file, so it is written once when the image opens and does not move
    // again while it is on screen.
    let top = chrome.top;
    let top_baseline = text_baseline(top);

    // Least to most disposable, and dropped whole rather than clipped: half
    // of "18333 x 15667" is worse than none of it. Half the bar at most, so
    // that the name it is sharing the bar with keeps the other half.
    let facts = [
        format!("{} \u{00d7} {}", current.image.width, current.image.height),
        status::describe_pixels(current),
        current.image.color.label(),
    ];
    let facts = status::fit_segments(text, &facts, (top.width / 2.0 - BAR_PADDING * 2.0).max(1.0));
    let facts_width = text.measure_text(&facts, TEXT_SIZE)[0];
    // Clear of the two buttons at the end of the bar, the innermost of which
    // is the zoom readout.
    let spacing = grid_spacing(panels.show_grid, zoom, input.scale);
    let grid_button = chrome.grid_button(spacing.as_deref());
    let zoom_button = chrome.zoom_button(spacing.as_deref());
    let facts_x = (zoom_button.x - PADDING - facts_width).max(BAR_PADDING);

    // The count is a fact about the list, not part of the name, and is set
    // like the other facts in the bar: the name is the one thing here worth
    // picking out, and picking out two things picks out neither.
    let mut name_x = BAR_PADDING;
    if let Some(counter) = status::counter(input.index, input.count) {
        let width = text.measure_text(&counter, TEXT_SIZE)[0];
        frame.text_clipped(
            [BAR_PADDING, top_baseline],
            TEXT_SIZE,
            theme.text_dim,
            (facts_x - BAR_PADDING).max(1.0),
            counter,
        );
        name_x += width + COUNTER_GAP;
    }
    // In front of the name, on the side of the bar the name is read from, so
    // that it is seen before the file it is about rather than after it.
    if input.deleted {
        let width = text.measure_text(status::DELETED, TEXT_SIZE)[0];
        frame.text_clipped(
            [name_x, top_baseline],
            TEXT_SIZE,
            theme.warning,
            (facts_x - PADDING - name_x).max(1.0),
            status::DELETED.to_string(),
        );
        name_x += width + COUNTER_GAP;
    }
    frame.text_clipped_bold(
        [name_x, top_baseline],
        TEXT_SIZE,
        theme.text_bright,
        (facts_x - PADDING - name_x).max(1.0),
        status::top_label(&current.label, input.reading.as_ref()),
    );
    frame.text([facts_x, top_baseline], TEXT_SIZE, theme.text_dim, facts);

    buttons::zoom_button(
        &mut frame,
        text,
        zoom_button,
        zoom,
        panels.menu == Some(Menu::Zoom),
        panels.hover == Some(Widget::Zoom),
        theme,
    );

    buttons::grid_button(
        &mut frame,
        text,
        grid_button,
        spacing.as_deref(),
        panels.hover == Some(Widget::Grid),
        theme,
    );
    buttons::minimap_button(
        &mut frame,
        chrome.minimap_button,
        panels.show_minimap,
        panels.hover == Some(Widget::Minimap),
        theme,
    );
    buttons::histogram_button(
        &mut frame,
        chrome.histogram_button,
        panels.show_histogram,
        panels.hover == Some(Widget::Histogram),
        theme,
    );
    buttons::info_button(
        &mut frame,
        chrome.info_button,
        panels.show_info,
        panels.hover == Some(Widget::Info),
        theme,
    );

    // Bottom panel: what is happening to the image. The pointer comes and
    // goes on its own, and the rest changes as the view is worked.
    let bar = chrome.bottom;
    let baseline = text_baseline(bar);

    let right = status::describe_state(current, input);
    let right_width = text.measure_text(&right, TEXT_SIZE)[0];
    let right_x = (bar.right() - BAR_PADDING - right_width).max(BAR_PADDING);

    if let Some(at) = input.pointer {
        pixel::draw(
            &mut frame,
            text,
            current,
            at,
            bar,
            (right_x - PADDING).max(BAR_PADDING),
            theme,
        );
    }
    frame.text([right_x, baseline], TEXT_SIZE, theme.text_dim, right);

    // On the layer above everything else, so that it covers not only the
    // panels and what floats over the content area but the words on them: a
    // popup is the thing being looked at while it is open.
    if let Some(open) = panels.menu
        && let Some(popup) = chrome.popup(open, spacing.as_deref())
    {
        frame.over(|frame| menu::draw(frame, text, &popup, view, zoom, panels, theme));
    }
    frame
}

/// What the compositor paints behind the image: the panel colour, with the
/// theme's hairline as the other square of the checkerboard.
pub fn backdrop(theme: &Theme) -> Backdrop {
    Backdrop {
        base: theme.bar_background,
        alternate: theme.border,
        square: CHECKER_SQUARE,
    }
}

/// Where text has to start to sit centred in a bar of `BAR_HEIGHT`.
///
/// The line's own box, descenders and all, rather than the capitals that
/// [`buttons::centred_text`] levels a label by: what a bar carries is prose —
/// a file's name, a readout — where descenders are ordinary and the room
/// under the baseline is room the words actually use. A button's label is
/// levelled against the mark beside it instead, which is a different job.
fn text_baseline(bar: Rect) -> f32 {
    bar.y + (bar.height - TEXT_SIZE * 1.3) / 2.0
}
