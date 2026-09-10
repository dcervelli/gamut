//! Building each frame's interface with egui: the chrome and everything on
//! it, laid out in logical pixels from what is on screen.
//!
//! Nothing here touches the GPU or the window. The state it needs is passed
//! in — what is on screen, which panels are showing, what this frame's
//! geometry is — and what was pressed comes back as commands, so that a
//! frame can be built and driven with no application behind it.

pub mod chrome;
pub mod control;
pub mod fonts;
pub mod info;
pub mod menu;
pub mod minimap;
pub mod toast;
pub mod tooltip;

mod grid;
pub mod histogram;
mod icon;
pub mod pixel;
mod rect;
mod status;
pub mod style;

#[cfg(test)]
mod driven;

use std::sync::Arc;

use crate::image::display::{Display, Headroom};
use crate::image::exif::Exif;
use crate::image::stats::BINS;
use crate::image::{DecodedImage, Stats};
use egui::Sense;

use crate::render::Backdrop;
use crate::theme::Theme;
use crate::view::{View, Viewport};

pub use control::{Command, Control, Naming};
pub use info::FileFacts;
pub use pixel::PixelFormat;
pub use rect::Rect;
pub use status::explain_state;
pub use toast::Toast;
pub use tooltip::{Tip, Tooltip};

use chrome::Pass;

const TEXT_SIZE: f32 = 13.0;

/// Trackpad pixels that add up to one notch of the wheel. Wheels report whole
/// lines and need no conversion; a trackpad reports the scroll it would have
/// done, and this is what turns that into the same zoom increment.
const WHEEL_PIXELS_PER_STEP: f32 = 50.0;

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
    /// How the bottom bar writes out the value of the pixel under the
    /// pointer. Here rather than with the display's own settings because it
    /// is about the reading and not about the rendering: nothing on screen
    /// changes with it but the words in the bar.
    pub pixel_format: PixelFormat,
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
    /// Whether a drag would move the picture: a fitted image has nowhere to
    /// go, and the closed hand is a promise that dragging will move
    /// something.
    pub can_pan: bool,
    /// What else can open the file on screen, in the order the menu under the
    /// open button lists them and named as their desktop entries name them.
    /// Empty where nothing offers, which draws that button dead.
    ///
    /// The names alone: what pressing one actually runs is the
    /// application's, and an item comes back as its place in this list — see
    /// [`crate::openers`].
    pub openers: Vec<String>,
    /// The message about what was just done, while one is up. Copied out of
    /// the application's [`toast::Toasts`]: what a frame draws is what had
    /// settled when it was asked for.
    pub toast: Option<Toast>,
}

/// One pass of the interface: the chrome and everything on it, laid out in
/// `ui` — the whole window — from what is on screen. What was pressed comes
/// back as commands for the application to act on; nothing here acts on it.
pub fn show(
    ui: &mut egui::Ui,
    input: &FrameInput,
    panels: &Panels,
    current: Option<&Current>,
    view: &View,
    theme: &Theme,
    namer: &dyn Naming,
) -> Vec<Command> {
    let mut pass = Pass {
        input,
        panels,
        current,
        view,
        theme,
        namer,
        commands: Vec::new(),
    };
    if panels.show_ui {
        pass.bars(ui);
    }
    pass.picture(ui);
    if let Some(current) = current {
        let content = chrome::content_area(input.logical, panels.show_ui);
        pass.overlays(ui, current, content);
    }
    pass.commands
}

impl Pass<'_> {
    /// The picture: what the panels leave in the middle, which is where a
    /// drag pans and the wheel zooms. Laid out as the one thing under
    /// everything that floats, so that a panel over it takes the pointer
    /// from it — egui's layers are what the pointer is routed by.
    ///
    /// The hand is on the view: a drag goes exactly where it is put, and is
    /// handed back as far as it went. A wheel's notch is a step asked for by
    /// name, and a trackpad's scroll is the hand again; which of the two it
    /// was goes with the steps, and the application decides what to animate.
    fn picture(&mut self, ui: &mut egui::Ui) {
        let response = egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                ui.allocate_rect(ui.max_rect(), Sense::CLICK | Sense::DRAG)
            })
            .inner;
        let scale = self.input.scale;
        if response.dragged_by(egui::PointerButton::Primary) {
            let delta = response.drag_delta();
            if delta != egui::Vec2::ZERO {
                self.commands
                    .push(Command::Drag([delta.x * scale, delta.y * scale]));
            }
            // The closed hand is a promise that dragging will move
            // something, so a fitted image — which has nowhere to go — does
            // not make it.
            if self.input.can_pan {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
        }
        self.commands
            .push(Command::OverImage(response.contains_pointer()));
        if response.contains_pointer() {
            let wheel: Vec<Command> = ui.input(|input| {
                input
                    .events
                    .iter()
                    .filter_map(|event| match event {
                        egui::Event::MouseWheel { unit, delta, .. } => Some(match unit {
                            egui::MouseWheelUnit::Point => Command::Wheel {
                                steps: delta.y / WHEEL_PIXELS_PER_STEP,
                                notched: false,
                            },
                            egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => {
                                Command::Wheel {
                                    steps: delta.y,
                                    notched: true,
                                }
                            }
                        }),
                        _ => None,
                    })
                    .collect()
            });
            self.commands.extend(wheel);
        }
    }

    /// What floats over the picture, in the order it is stacked: the grid
    /// under everything, then the minimap, then the message about what was
    /// just done — over the panels rather than among them, and there whether
    /// or not the bars are, since what it says does not stop being true
    /// because they are away.
    fn overlays(&mut self, ui: &mut egui::Ui, current: &Current, content: Rect) {
        let zoom = self.view.zoom(current.size(), self.input.viewport);
        // Under the floating panels, which are read against the image and
        // would be harder to read over a grid as well. The minimap's
        // thumbnail is not one of them — the image layer draws it, below the
        // whole interface — so the grid is told to leave its rectangle alone.
        if self.panels.show_grid {
            let thumbnail = self
                .input
                .minimap_on_screen
                .then(|| minimap::thumbnail(content, current.size()))
                .flatten();
            grid::paint(
                ui.painter(),
                self.view.placement(current.size(), self.input.viewport),
                self.input.scale,
                content,
                thumbnail,
                grid::step(zoom, self.input.scale),
                self.theme,
            );
        }
        if self.input.minimap_on_screen {
            minimap::show(self, ui, current, content);
        }
        let room = room(content, self.panels);
        if self.panels.show_histogram && room.histogram {
            histogram::show(self, ui, current, content);
        }
        if self.panels.show_info && room.info {
            info::show(self, ui, current, content);
        }
        if let Some(message) = &self.input.toast {
            toast::show(self, ui, message, content);
        }
    }
}

/// What the grid toggle reads out while the grid is on: how far apart its
/// lines are at `zoom`, on a display of `scale` physical pixels to the
/// logical one. `None` while it is off, there being no spacing in force then.
///
pub fn grid_spacing(show_grid: bool, zoom: f32, scale: f32) -> Option<String> {
    show_grid.then(|| grid::label(grid::step(zoom, scale)))
}

/// The content area both floating panels need at once: the strip they share
/// is [`PANEL_WIDTH`] wide, and down it go the histogram at its own fixed
/// height, the gap between the two, and the least column the information
/// panel will show — each inside the padding everything floating over the
/// image keeps.
///
/// What a window opens at least this large for, where the monitor has the
/// room to spare — see `app::window`. A window that opens smaller than its
/// own interface will fit in has two toggles dead in it from the start, for
/// no reason the viewer chose.
///
/// Derived rather than written down, and `the_panels_room_is_room_for_both`
/// holds it to what [`room`] actually answers.
pub const PANELS_ROOM: [f32; 2] = [
    PANEL_WIDTH + 2.0 * PADDING,
    histogram::HISTOGRAM_SIZE[1] + info::INFO_MIN_HEIGHT + 3.0 * PADDING,
];

/// Whether the content area has room for each of the two panels that float
/// over the top right of it.
///
/// Both are fixed at [`PANEL_WIDTH`], and the histogram is fixed in height as
/// well, so in a small enough window there is nothing to give and the panel
/// stays off rather than covering the picture it is about. Held together in
/// one answer because the two are stacked: the histogram takes the top of the
/// strip, and what it takes is height the information panel does not have.
///
/// Asked by the frame builder and by the application, which have to agree
/// about what is on screen: a toggle that quietly set something no one could
/// see would be worse than one that does nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Room {
    pub histogram: bool,
    pub info: bool,
}

/// What `content` has room for, with `panels` saying which of the two is
/// asked for — the histogram's take counting against the information panel
/// only where the histogram is on screen, which [`info::panel`] settles for
/// itself.
pub fn room(content: Rect, panels: &Panels) -> Room {
    Room {
        histogram: histogram::panel(content).is_some(),
        info: info::panel(content, panels.show_histogram).is_some(),
    }
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

/// `label` with its first letter capitalized: the labels are written as they
/// are read in the middle of a line, and a button wears a name, a sentence
/// starts with one.
pub(super) fn capitalized(label: &str) -> String {
    let mut letters = label.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().chain(letters).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [`PANELS_ROOM`] is a sum of the constants the two panels are laid out
    /// from, and this is what holds it to what they do with them: a content
    /// area that size has room for both at once, and one a pixel smaller in
    /// either direction does not.
    #[test]
    fn the_panels_room_is_room_for_both() {
        let panels = Panels {
            show_ui: true,
            show_histogram: true,
            show_info: true,
            show_luma: true,
            show_planes: true,
            log_counts: false,
            show_minimap: true,
            show_grid: false,
            paste: false,
            pixel_format: PixelFormat::default(),
        };
        let area = |width, height| room(Rect::new(0.0, 0.0, width, height), &panels);

        assert_eq!(
            area(PANELS_ROOM[0], PANELS_ROOM[1]),
            Room {
                histogram: true,
                info: true
            }
        );
        assert!(!area(PANELS_ROOM[0] - 1.0, PANELS_ROOM[1]).histogram);
        assert!(!area(PANELS_ROOM[0], PANELS_ROOM[1] - 1.0).info);
    }
}
