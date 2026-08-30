//! Building each frame's interface: a display list of panels, widgets and
//! text, laid out in logical pixels and handed to the renderer's UI layer.
//!
//! Nothing here touches the GPU or the window. The state it needs is passed
//! in — what is on screen, which panels are showing, what this frame's
//! geometry is — so that a frame can be built against anything that can
//! measure text.

pub mod chrome;
pub mod menu;
pub mod minimap;

mod buttons;
mod grid;
mod histogram;
mod pixel;
mod status;

use crate::image::display::Display;
use crate::image::{DecodedImage, Stats};
use crate::render::{Backdrop, Rect, TextMeasure, UiFrame};
use crate::theme::Theme;
use crate::view::{View, Viewport};

use chrome::Chrome;
pub use menu::Menu;

const TEXT_SIZE: f32 = 13.0;

const PADDING: f32 = 12.0;

/// Side of one checkerboard square, in logical pixels. Small enough to read
/// as a texture behind the image rather than as a pattern competing with it.
const CHECKER_SQUARE: f32 = 8.0;

/// Something in the interface the pointer can be over and press: a toggle in
/// a side panel, the zoom readout in the bottom bar, or a cell of the menu
/// that readout opens. One value rather than a flag each, so that
/// hit-testing, hover and drawing all go through the same test.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Widget {
    Minimap,
    Histogram,
    Grid,
    Zoom,
    /// A cell of whichever menu is open. Which menu that is is
    /// [`Panels::menu`], so a cell needs only its place in the grid.
    Cell(usize),
}

/// The image on screen, with everything derived from it.
pub struct Current {
    pub image: DecodedImage,
    pub stats: Stats,
    pub display: Display,
    pub label: String,
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
    /// The menu popped up over the interface, if any. It takes every press
    /// while it is open: one on a cell chooses, one anywhere else dismisses
    /// it.
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
    /// The output's label when it is an HDR surface, which is worth a word in
    /// the bar; `None` on an ordinary one.
    pub hdr_output: Option<&'static str>,
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
    let mut frame = UiFrame::new();
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
                    [PADDING, text_baseline(chrome.top)],
                    TEXT_SIZE,
                    theme.text_dim,
                    (chrome.top.width - PADDING * 2.0).max(1.0),
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
        histogram::draw(&mut frame, current, content, theme);
    }
    if input.minimap_on_screen {
        minimap::draw(&mut frame, current, view, input, content, theme);
    }
    if !panels.show_ui {
        return frame;
    }

    for panel in chrome.panels() {
        frame.rect(panel, theme.bar_background);
    }
    for border in chrome.borders() {
        frame.rect(border, theme.border);
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
    let facts = status::fit_segments(text, &facts, (top.width / 2.0 - PADDING * 2.0).max(1.0));
    let facts_width = text.measure_text(&facts, TEXT_SIZE)[0];
    // Clear of the grid toggle at the end of the bar, the way the state
    // readout in the bottom bar keeps clear of the zoom button.
    let grid_button = chrome.grid_button(panels.show_grid);
    let facts_x = (grid_button.x - PADDING - facts_width).max(PADDING);

    frame.text_clipped(
        [PADDING, top_baseline],
        TEXT_SIZE,
        theme.text_primary,
        (facts_x - PADDING * 2.0).max(1.0),
        status::top_label(&current.label, input.reading.as_ref()),
    );
    frame.text([facts_x, top_baseline], TEXT_SIZE, theme.text_dim, facts);

    let spacing = panels.show_grid.then(|| grid::label(grid_step));
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

    // Bottom panel: what is happening to the image. The pointer comes and
    // goes on its own, and the rest changes as the view is worked.
    let bar = chrome.bottom;
    let baseline = text_baseline(bar);

    buttons::zoom_button(
        &mut frame,
        text,
        chrome.zoom_button,
        zoom,
        panels.menu == Some(Menu::Zoom),
        panels.hover == Some(Widget::Zoom),
        theme,
    );

    let mut right = status::describe_state(current, view, input);
    if let Some(label) = input.hdr_output {
        right = format!("{label}   \u{00b7}   {right}");
    }
    let right_width = text.measure_text(&right, TEXT_SIZE)[0];
    let right_x = (chrome.zoom_button.x - PADDING - right_width).max(PADDING);

    if let Some(at) = input.pointer {
        pixel::draw(
            &mut frame,
            text,
            current,
            at,
            bar,
            (right_x - PADDING).max(PADDING),
            theme,
        );
    }
    frame.text([right_x, baseline], TEXT_SIZE, theme.text_dim, right);

    // Last, so that it lies over the panels and over anything floating in the
    // content area: a popup is the thing being looked at while it is open.
    if let Some(open) = panels.menu
        && let Some(popup) = chrome.popup(open)
    {
        menu::draw(&mut frame, text, &popup, view.fit(), zoom, panels, theme);
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
fn text_baseline(bar: Rect) -> f32 {
    bar.y + (bar.height - TEXT_SIZE * 1.3) / 2.0
}
