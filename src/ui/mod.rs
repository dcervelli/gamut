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
pub mod toast;
pub mod tooltip;

mod buttons;
mod grid;
pub mod histogram;
mod icon;
pub mod pixel;
mod status;

use std::sync::Arc;

use crate::image::display::{Display, Headroom};
use crate::image::exif::Exif;
use crate::image::stats::BINS;
use crate::image::{DecodedImage, Stats};
use crate::render::{Backdrop, Rect, TextMeasure, UiFrame};
use crate::theme::Theme;
use crate::view::{Fit, View, Viewport};

use chrome::{BAR_PADDING, Chrome};
pub use info::FileFacts;
pub use menu::Menu;
pub use pixel::PixelFormat;
pub use status::BarText;
pub use toast::Toast;
pub use tooltip::{Tip, Tooltip, Tooltips};

use tooltip::Tips;

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

/// The gap between two neighbors: what floats over the content area from the
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
/// a pixel boundary and coming out fatter than their neighbors. The
/// information panel takes the same width so that the two line up down the
/// right of the window, whether or not either has anything else on it.
const PANEL_WIDTH: f32 = histogram::TOOLBAR_WIDTH + BINS as f32 + 2.0 * PANEL_INSET;

/// The corner radius of a floating panel.
const PANEL_RADIUS: f32 = 6.0;

/// Side of one checkerboard square, in logical pixels. Small enough to read
/// as a texture behind the image rather than as a pattern competing with it.
const CHECKER_SQUARE: f32 = 8.0;

/// Something in the interface the pointer can be over and press: a toggle in
/// a side panel, a button in one of the bars, or a cell of whichever menu is
/// open. One value rather than a flag each, so that hit-testing, hover and
/// drawing all go through the same test.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Widget {
    /// The two buttons at the head of the top bar, which step back and on
    /// through the file list. On screen only while there is more than one
    /// file — see [`chrome::Chrome::step_buttons`].
    Previous,
    Next,
    Minimap,
    /// The button that opens the menu of copies, at the top of the left
    /// strip.
    Copy,
    /// The button that pastes the picture on the clipboard. On screen only
    /// while there is one — see [`Panels::paste`].
    Paste,
    Histogram,
    Info,
    Grid,
    Zoom,
    /// The button at the end of the top bar that gives the picture the whole
    /// window. Not a toggle: what it hides includes the button itself, so
    /// there is no state for it to be showing and no press of it that puts
    /// the interface back — see [`crate::app::input::Action::Dismiss`].
    Maximize,
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
    /// One of the false colors offered under that panel's ramp, by its place
    /// in [`crate::image::display::Colormap::ALL`].
    Ramp(usize),
    /// The two steps of the exposure row under that ramp, a quarter of a stop
    /// each — see [`histogram::EV_STEP`].
    ExposureDown,
    ExposureUp,
    /// One of the windows the row below those offers, by its place in
    /// [`histogram::WINDOWS`]. They set a window rather than showing which
    /// one is in force: the line above them is what says that.
    Window(usize),
    /// The four nudges at the end of that line, which move the window the
    /// user has rather than putting them on a new one: along the axis either
    /// way, and narrower or wider about its own middle.
    WindowDown,
    WindowNarrow,
    WindowWiden,
    WindowUp,
    /// And one of the tone curves in the row below that, by its place in
    /// [`crate::image::display::ToneMap::ALL`]. These do show which is on,
    /// there being one curve at a time and a button for each of them.
    Curve(usize),
    /// The switch between the SDR and the HDR surface, at the end of the
    /// bottom bar: lit while the picture is going out with room above white.
    Output,
    /// The dot at the head of the pixel readout, at the other end of that
    /// bar, which opens the menu of ways to write a pixel's value.
    PixelFormat,
    /// The cross on the message at the foot of the content area, which takes
    /// it off. On screen only while there is a message — see
    /// [`FrameInput::toast`].
    Dismiss,
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
    /// Whether the clipboard is holding a picture this program could show,
    /// which is whether the paste button is on screen at all: a button that
    /// did nothing when pressed would be worse than no button.
    ///
    /// Looked at on the same cadence as the file and the palette, since
    /// nothing tells us when a selection changes — see
    /// `App::poll_clipboard`. It is what was true at the last look, so a
    /// press asks the clipboard again rather than acting on it.
    pub paste: bool,
    /// Which widget the pointer is over. Held rather than recomputed while
    /// drawing so that motion knows when the highlight has changed and a
    /// redraw is actually owed.
    pub hover: Option<Widget>,
    /// And which part of the info panel's column, for the same reason. Held
    /// apart from `hover` because the column is not one of the chrome's
    /// widgets: it moves as the panel scrolls, and it is there whether or not
    /// the bars are.
    pub info_hover: Option<info::Copyable>,
    /// How the bottom bar writes out the value of the pixel under the
    /// pointer. Here rather than with the display's own settings because it
    /// is about the reading and not about the rendering: nothing on screen
    /// changes with it but the words in the bar.
    pub pixel_format: PixelFormat,
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
    /// Whether the surface the picture is going out to has room above white,
    /// which is half of what every readout of a value has to say.
    pub headroom: Headroom,
    /// Whether the switch has anything to switch: the driver offers an HDR
    /// color space for this window, and the monitor is not known to be in
    /// SDR mode. The switch is drawn dead otherwise.
    pub hdr_available: bool,
    /// What the pointer has rested on long enough to be told about, and what
    /// to say about it. Composed by the application — most of a tooltip is
    /// the key that does the same job, and the keys are the application's —
    /// and settled by [`Tooltips`], which is where the timing lives.
    pub tooltip: Option<Tooltip>,
    /// The message about what was just done, while one is up. Copied out of
    /// the application's [`toast::Toasts`] the way the tooltip is composed
    /// there: what a frame draws is what had settled when it was asked for.
    pub toast: Option<Toast>,
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
    fn cap_center(&mut self, size: f32) -> f32 {
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

/// Which of the top bar's own runs of words the pointer is on, if any.
///
/// Beside [`layers::hit`] rather than in it because answering takes the fonts
/// the bar is set in: where a run of words ends depends on the face it is
/// drawn in, so this is asked separately, the way the information panel's
/// rows are.
///
/// `bar` is the top panel, `start` where the step buttons at its near end
/// leave off and `limit` where the buttons at the far end begin — the same
/// three the frame builder lays the words out between.
pub fn bar_tip(
    text: &mut dyn TextMeasure,
    point: [f32; 2],
    bar: Rect,
    start: f32,
    limit: f32,
    about: &BarText,
) -> Option<Tip> {
    if !bar.contains(point) {
        return None;
    }
    let words = status::top_bar(text, bar, start, limit, about);
    if words
        .counter
        .as_ref()
        .is_some_and(|counter| counter.strip(bar).contains(point))
    {
        return Some(Tip::Counter);
    }
    words.name.strip(bar).contains(point).then_some(Tip::Name)
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

    // Each thing says where it went as it is drawn, so that the tooltip
    // naming one hangs off the rectangle the frame actually used. Anything
    // else that wants a tooltip does the same: become a `Tip` the pointer can
    // be answered with, and offer its rectangle here.
    let mut tips = Tips::new(input.tooltip.as_ref());
    let zoom = view.zoom(current.size(), input.viewport);
    let grid_step = grid::step(zoom, input.scale);

    // Under the floating panels, which are read against the image and would
    // be harder to read over a grid as well. The minimap's thumbnail is not
    // one of them — the image layer draws it, below this frame — so the grid
    // is told to leave its rectangle alone.
    if panels.show_grid {
        let thumbnail = input
            .minimap_on_screen
            .then(|| minimap::thumbnail(content, current.size()))
            .flatten();
        grid::draw(
            &mut frame,
            view.placement(current.size(), input.viewport),
            input.scale,
            content,
            thumbnail,
            grid_step,
            theme,
        );
    }
    if panels.show_histogram {
        histogram::draw(&mut frame, text, current, input, panels, content, theme);
        histogram::offer_tips(&mut tips, content, current.image.is_gray());
    }
    if input.minimap_on_screen {
        minimap::draw(&mut frame, current, view, input, content, theme);
    }
    if panels.show_info {
        info::draw(&mut frame, text, current, panels, content, theme);
    }

    // Over those panels rather than among them, and drawn before the bars so
    // that hiding the chrome leaves it behind: what it says is about what was
    // just done, which does not stop being true because the bars are away.
    if let Some(message) = &input.toast
        && let Some(placed) = toast::place(message, content)
    {
        toast::draw(
            &mut frame,
            text,
            message,
            placed,
            panels.hover == Some(Widget::Dismiss),
            theme,
        );
        tips.offer(Tip::Widget(Widget::Dismiss), placed.close);
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

    // Clear of the two buttons at the end of the bar, the innermost of which
    // is the zoom readout.
    let spacing = grid_spacing(panels.show_grid, zoom, input.scale);
    let grid_button = chrome.grid_button(spacing.as_deref());
    let zoom_button = chrome.zoom_button(spacing.as_deref());

    // Laid out by `status`, which the pointer asks as well: what a tooltip
    // hangs from has to be where the words actually went.
    let bar_text = status::BarText {
        current,
        reading: input.reading.as_ref(),
        index: input.index,
        count: input.count,
        deleted: input.deleted,
    };
    // The pair that steps through the list, at the head of the bar in front
    // of the count they move through. Only with a list to step through: see
    // [`Chrome::step_buttons`].
    let steps = input.count > 1;
    if steps {
        let [previous, next] = chrome.step_buttons();
        for (rect, widget, forward) in [
            (previous, Widget::Previous, false),
            (next, Widget::Next, true),
        ] {
            buttons::step_button(
                &mut frame,
                rect,
                forward,
                panels.hover == Some(widget),
                theme,
            );
            tips.offer(Tip::Widget(widget), rect);
        }
    }

    let words = status::top_bar(
        text,
        top,
        chrome.bar_text_x(steps),
        zoom_button.x,
        &bar_text,
    );

    if let Some(counter) = &words.counter {
        frame.text_clipped(
            [counter.x, top_baseline],
            TEXT_SIZE,
            theme.text_dim,
            counter.room,
            counter.text.clone(),
        );
        tips.offer(Tip::Counter, counter.strip(top));
    }
    if let Some(deleted) = &words.deleted {
        frame.text_clipped(
            [deleted.x, top_baseline],
            TEXT_SIZE,
            theme.warning,
            deleted.room,
            deleted.text.clone(),
        );
    }
    frame.text_clipped_bold(
        [words.name.x, top_baseline],
        TEXT_SIZE,
        theme.text_bright,
        words.name.room,
        words.name.text.clone(),
    );
    tips.offer(Tip::Name, words.name.strip(top));
    frame.text(
        [words.facts.x, top_baseline],
        TEXT_SIZE,
        theme.text_dim,
        words.facts.text.clone(),
    );

    buttons::zoom_button(
        &mut frame,
        text,
        zoom_button,
        zoom,
        panels.menu == Some(Menu::Zoom),
        panels.hover == Some(Widget::Zoom),
        theme,
    );
    tips.offer(Tip::Widget(Widget::Zoom), zoom_button);

    buttons::grid_button(
        &mut frame,
        text,
        grid_button,
        spacing.as_deref(),
        panels.hover == Some(Widget::Grid),
        theme,
    );
    tips.offer(Tip::Widget(Widget::Grid), grid_button);
    buttons::maximize_button(
        &mut frame,
        chrome.maximize_button,
        panels.hover == Some(Widget::Maximize),
        theme,
    );
    tips.offer(Tip::Widget(Widget::Maximize), chrome.maximize_button);
    buttons::minimap_button(
        &mut frame,
        chrome.minimap_button,
        panels.show_minimap,
        panels.hover == Some(Widget::Minimap),
        theme,
    );
    tips.offer(Tip::Widget(Widget::Minimap), chrome.minimap_button);
    buttons::copy_button(
        &mut frame,
        chrome.copy_button,
        panels.menu == Some(Menu::Copy),
        panels.hover == Some(Widget::Copy),
        theme,
    );
    tips.offer(Tip::Widget(Widget::Copy), chrome.copy_button);
    if panels.paste {
        buttons::paste_button(
            &mut frame,
            chrome.paste_button,
            panels.hover == Some(Widget::Paste),
            theme,
        );
        tips.offer(Tip::Widget(Widget::Paste), chrome.paste_button);
    }
    buttons::histogram_button(
        &mut frame,
        chrome.histogram_button,
        panels.show_histogram,
        panels.hover == Some(Widget::Histogram),
        theme,
    );
    tips.offer(Tip::Widget(Widget::Histogram), chrome.histogram_button);
    buttons::info_button(
        &mut frame,
        chrome.info_button,
        panels.show_info,
        panels.hover == Some(Widget::Info),
        theme,
    );
    tips.offer(Tip::Widget(Widget::Info), chrome.info_button);

    // Bottom panel: what is happening to the image. The pointer comes and
    // goes on its own, and the rest changes as the view is worked.
    let bar = chrome.bottom;
    let baseline = text_baseline(bar);

    // The surface switch ends the bar, where the grid toggle ends the top
    // one: it is the one control of the display that is not a fact about the
    // picture, and the words about what is being done to the picture run up
    // to it.
    let output_button = chrome.output_button();
    buttons::output_button(
        &mut frame,
        text,
        output_button,
        input.headroom == Headroom::Above,
        input.hdr_available,
        panels.hover == Some(Widget::Output),
        theme,
    );
    tips.offer(Tip::Widget(Widget::Output), output_button);

    // The head of the readout, and the only part of it that is always there:
    // the pointer is over the bar rather than over a pixel while it is on its
    // way to this button.
    let pixel_button = chrome.pixel_button;
    buttons::pixel_button(
        &mut frame,
        pixel_button,
        panels.menu == Some(Menu::PixelFormat),
        panels.hover == Some(Widget::PixelFormat),
        theme,
    );
    tips.offer(Tip::Widget(Widget::PixelFormat), pixel_button);

    let right = status::describe_state(current, input);
    let right_width = text.measure_text(&right, TEXT_SIZE)[0];
    let right_x = (output_button.x - PADDING - right_width).max(BAR_PADDING);

    // The strip of the bar the readout has to itself: past the button at its
    // head, and stopping short of the words at the other end.
    let readout_x = pixel_button.right() + pixel::GAP;
    let readout = Rect::new(
        readout_x,
        bar.y,
        ((right_x - PADDING) - readout_x).max(0.0),
        bar.height,
    );
    pixel::draw(
        &mut frame,
        text,
        current,
        input,
        readout,
        panels.pixel_format,
        theme,
    );
    frame.text([right_x, baseline], TEXT_SIZE, theme.text_dim, right);

    // On the layer above everything else, so that it covers not only the
    // panels and what floats over the content area but the words on them: a
    // popup is the thing being looked at while it is open.
    if let Some(open) = panels.menu
        && let Some(popup) = chrome.popup(open, spacing.as_deref())
    {
        // Its cells are named like anything else, and are offered from here
        // rather than from inside the menu so that every tooltip in the frame
        // is collected in one place.
        for (index, cell) in popup.cells() {
            tips.offer(Tip::Widget(Widget::Cell(index)), cell);
        }
        let shown = menu::Shown {
            zoom,
            fills: Fit::Fill.axis(current.size(), input.viewport),
        };
        frame.over(|frame| menu::draw(frame, text, &popup, view, shown, panels, theme));
    }

    // On its own layer above even that: what a tooltip names can be on the
    // menu, and a label hidden by the thing it is about says nothing. It goes
    // in the content area, off the chrome the thing it names is part of.
    tips.draw(&mut frame, text, content, theme);
    frame
}

/// What the compositor paints behind the image: the panel color, with the
/// theme's hairline as the other square of the checkerboard.
pub fn backdrop(theme: &Theme) -> Backdrop {
    Backdrop {
        base: theme.bar_background,
        alternate: theme.border,
        square: CHECKER_SQUARE,
    }
}

/// Where text has to start to sit centered in a bar of `BAR_HEIGHT`.
///
/// The line's own box, descenders and all, rather than the capitals that
/// [`buttons::centered_text`] levels a label by: what a bar carries is prose —
/// a file's name, a readout — where descenders are ordinary and the room
/// under the baseline is room the words actually use. A button's label is
/// leveled against the mark beside it instead, which is a different job.
fn text_baseline(bar: Rect) -> f32 {
    bar.y + (bar.height - TEXT_SIZE * 1.3) / 2.0
}
