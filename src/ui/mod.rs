//! Building each frame's interface with egui: the chrome and everything on
//! it, laid out in logical pixels from what is on screen.
//!
//! Nothing here touches the GPU or the window. The state it needs is passed
//! in — what is on screen, which panels are showing, what this frame's
//! geometry is — and what was pressed comes back as commands, so that a
//! frame can be built and driven with no application behind it.

pub mod chooser;
pub mod chrome;
pub mod control;
pub mod empty;
pub mod export;
pub mod filmstrip;
pub mod fonts;
pub mod help;
pub mod info;
pub mod loupe;
pub mod menu;
pub mod minimap;
pub mod rename;
mod slider;
pub mod toast;
pub mod tooltip;
pub mod transport;

mod grid;
pub mod histogram;
mod icon;
pub mod panel;
pub mod pixel;
mod rect;
pub mod region;
mod status;
pub mod style;

#[cfg(test)]
mod driven;

use std::sync::Arc;

use crate::image::display::{Display, Headroom};
use crate::image::exif::Exif;
use crate::image::orient::Turn;
use crate::image::region::{Grip, Region};
use crate::image::sequence::Sequence;
use crate::image::stats::BINS;
use crate::image::{DecodedImage, Sample, Stats};
use egui::Sense;
pub use transport::Transport;

use crate::render::Backdrop;
use crate::theme::Theme;
use crate::view::{View, Viewport};

pub use control::{Command, Control, Grab, Naming, Selection};
pub use info::FileFacts;
pub use pixel::PixelFormat;
pub use rect::Rect;
pub use status::explain_state;
pub use toast::Toast;
pub use tooltip::{Tip, Tooltip};

/// A file's thumbnail, in the copies the screen holds, smallest first, each
/// half the next: what the chooser and the file list draw from, each taking
/// the copy its slot calls for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thumb {
    pub copies: [egui::load::SizedTexture; 3],
}

impl Thumb {
    /// The copy to draw across `side` device pixels: the smallest at least
    /// that large, which is never shrunk to less than half itself, or the
    /// largest where none is — a small picture's copies are all its own
    /// size, and a slot wider than the largest enlarges it.
    pub fn for_side(&self, side: f32) -> egui::load::SizedTexture {
        let copies = &self.copies;
        *copies
            .iter()
            .find(|copy| copy.size.max_elem() >= side)
            .unwrap_or(&copies[copies.len() - 1])
    }
}

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

/// The gap between the row of steps and count and the name it belongs to.
/// Tighter than the gap between two unrelated things in a bar, the two being
/// one line about one file.
const COUNTER_GAP: f32 = 8.0;

/// The gap between two neighbors: what floats over the content area from the
/// edge of that area, and one thing in a bar from the next.
///
/// Not what a bar is inset by at its ends — that is
/// [`chrome::BAR_PADDING`], which is tighter, so that the bars
/// and the side panels share one line down each edge of the window.
const PADDING: f32 = 12.0;

/// The gap between a popup menu and the button it hangs off: half of
/// [`PADDING`], close enough that the menu reads as the button's own.
const MENU_OFFSET: f32 = PADDING / 2.0;

/// The gap between a floating panel's edge and what is on it.
const PANEL_INSET: f32 = 10.0;

/// How wide the panels that float over the content area are. The histogram
/// fixes it: wide enough that a bin is exactly one logical pixel, which is
/// what keeps its bars evenly spaced instead of some of them landing astride
/// a pixel boundary and coming out fatter than their neighbors. The
/// information panel takes the same width so that the two line up down the
/// right of the window, whether or not either has anything else on it.
const PANEL_WIDTH: f32 = histogram::TOOLBAR_WIDTH + BINS as f32 + 2.0 * PANEL_INSET;

use style::PANEL_RADIUS;

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
    /// What else the file holds: the frames of an animation, or its pages.
    pub sequence: Sequence,
    /// Which page `image` is, where the file has pages; zero otherwise.
    pub page: usize,
    /// The lift the picture is drawn through, where it has a gain map: the
    /// table at the weight the surface's room asks for, which is what every
    /// readout of a pixel reads it through too. `None` where it has none.
    pub lift: Option<Arc<crate::image::gain_map::Table>>,
    /// How far the picture has been turned on screen. `image` stays as the
    /// file holds it; everything read off `Current` other than `image`
    /// itself is in the turned picture's coordinates — see [`Turn`].
    pub turn: Turn,
}

impl Current {
    /// The picture's size on screen, turned.
    pub fn size(&self) -> [f32; 2] {
        let [width, height] = self.pixels();
        [width as f32, height as f32]
    }

    /// The same in whole pixels, which is what a region and the pointer's
    /// coordinate are measured in.
    pub fn pixels(&self) -> [u32; 2] {
        self.turn.size([self.image.width, self.image.height])
    }

    /// The pixel shown at `(x, y)` of the turned picture, read through the
    /// lift as the screen shows it. `None` outside the picture.
    pub fn sample(&self, x: u32, y: u32) -> Option<Sample> {
        let [width, height] = self.pixels();
        if x >= width || y >= height {
            return None;
        }
        let [x, y] = self
            .turn
            .stored([x, y], [self.image.width, self.image.height]);
        self.image.sample(x, y, self.lift.as_deref())
    }
}

/// The interface's panels: whether each is showing, and which toggle the
/// pointer is over.
#[derive(Clone, Copy, Debug)]
pub struct Panels {
    /// Whether the four panels are on screen. They are opaque and the image
    /// is fitted inside them, so hiding them gives it the whole window.
    pub show_ui: bool,
    /// Whether the file list is up down the left of the picture — see
    /// [`filmstrip`]. Whether it is actually on screen also asks whether
    /// there is a list to show: one file is no list, and the toggle for it
    /// is not drawn.
    pub show_filmstrip: bool,
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
    /// Whether the pixels the window has taken to black or to white are
    /// painted in the warning colors. A way of looking at the picture, like
    /// the grid, rather than a setting of it: it stays on from one file to
    /// the next, and no reset of the display touches it.
    pub mark_clipped: bool,
    /// Whether the minimap is switched on. Whether it is actually on screen
    /// also asks whether there is anything off screen for it to point out —
    /// see [`FrameInput::minimap_on_screen`].
    pub show_minimap: bool,
    /// Whether the grid is laid over the image. How far apart its lines are
    /// is not held: it follows the zoom, and is worked out afresh each frame
    /// by [`grid::step`].
    pub show_grid: bool,
    /// Whether the loupe follows the pointer over the image. The secondary
    /// button held on the picture puts it up as well, whatever this says;
    /// where it is on any one frame is [`FrameInput::loupe`].
    pub show_loupe: bool,
    /// How much larger the loupe's glass shows what its eye rings: one of
    /// [`loupe::MAGNIFICATIONS`], which the wheel steps through while the
    /// secondary button holds the loupe up. The glass stays one size and
    /// the eye shrinks as this grows.
    pub loupe_magnification: f32,
    /// Whether the clipboard is holding a picture this program could show,
    /// which is whether the paste button is on screen at all: a button that
    /// did nothing when pressed would be worse than no button.
    ///
    /// Looked at on the same cadence as the file and the palette, from a
    /// thread of its own, since nothing tells us when a selection changes
    /// — see `clipboard::watch` and `App::clipboard_changed`. It is what
    /// was true at the last look, so a press asks the clipboard again
    /// rather than acting on it.
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
    /// The loupe, while it is up: its toggle is on or the secondary button
    /// is held on the picture, and the pointer is on a pixel of it. Where
    /// its two circles go, worked out by the application from the same
    /// pointer the pixel readout reads, so that the rings drawn here and
    /// the glass the image layer draws cannot disagree.
    pub loupe: Option<loupe::Loupe>,
    /// Whether the secondary button is down on the picture, which is
    /// holding the loupe up whatever its toggle says: the toggle is lit for
    /// it, so that the button reads as the state it is showing.
    pub secondary: bool,
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
    /// The region on the picture: off, asked for, or drawn. What the button
    /// for it is lit by, what is painted over the picture, and what a drag
    /// on the picture means.
    pub selection: Selection,
    /// The region's current handle: the one the arrows move, drawn apart
    /// from the others. Meaningless without a region on screen.
    pub handle: Grip,
    /// The hold a drag under way has on the region, while it is the
    /// region's drag rather than the view's: said back to the interface so
    /// that the frames of one drag all go the same way.
    pub grabbing: Option<Grab>,
    /// Whether the pointer is on the region — over it, or on one of its
    /// handles, or dragging it — which is what its words are written for.
    /// Asked of the application rather than worked out here so that a
    /// pointer over a panel covering the region does not count, which is
    /// the same reading [`FrameInput::pointer`] is made from.
    pub over_region: bool,
    /// Whether a drag on the picture draws a box to zoom to — `Space` is
    /// held — rather than panning or taking hold of the region.
    pub box_zoom: bool,
    /// Whether a drag from inside the region moves it — `Shift` is held —
    /// rather than panning the picture under it.
    pub move_region: bool,
    /// The box being dragged out to zoom to, while a drag is drawing one:
    /// painted over the picture, and gone when the drag lets go.
    pub zoom_box: Option<Region>,
    /// The transport bar's state, for a file of frames or pages, and
    /// `None` for a still — which is what decides whether the bar is there.
    pub transport: Option<Transport>,
    /// The file list, on every frame it is up, and `None` while it is
    /// not — which is what decides whether the panel is there, and so
    /// where the picture starts.
    pub filmstrip: Option<filmstrip::Input>,
    /// The file chooser, on every frame it is open, and `None` while it is
    /// not. Whether it is open is egui's to say — see
    /// [`chooser::id`] — and this has to be handed over on every frame it
    /// is, since a popup not drawn for a frame is a popup egui has closed.
    pub chooser: Option<chooser::Input>,
    /// The rename dialog, on every frame it is up, and `None` while it is
    /// not. Its open state is the application's — see `App::renaming` —
    /// which is why it is a modal rather than a popup: nothing egui does
    /// on its own can close it.
    pub rename: Option<rename::Input>,
    /// The export dialog, the same way.
    pub export: Option<export::Input>,
    /// Whether there is nothing on screen and nothing on its way: the
    /// window opened on nothing, or everything it was handed failed. What
    /// puts the buttons for opening something in the middle of the content
    /// area — see [`empty`]. Not merely `current` being `None`, which is
    /// also the moment before the first file arrives.
    pub empty: bool,
    /// Whether the desktop's file dialog is up, which draws the buttons that
    /// put it up dead.
    pub picking: bool,
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
    let parts = chrome::Parts {
        transport: input.transport.is_some(),
        filmstrip: input.filmstrip.as_ref().map(|strip| strip.slot),
    };
    let content = chrome::content_area(input.logical, panels.show_ui, parts);
    let mut pass = Pass {
        input,
        panels,
        current,
        view,
        theme,
        namer,
        grid: icon::Grid::new(input.scale),
        content,
        room: room(content, panels),
        file_list: None,
        commands: Vec::new(),
    };
    if panels.show_ui {
        pass.bars(ui);
    } else {
        // The one part of the chrome that stays when the rest goes.
        pass.file_list(ui);
    }
    pass.picture(ui);
    // After the picture, which it overhangs while the panels are hidden,
    // so that the edge is the grip's and not the picture's drag.
    if let Some(strip) = &input.filmstrip {
        filmstrip::grip(&mut pass, ui, strip.slot);
    }
    match current {
        Some(current) => pass.overlays(ui, current),
        // Nothing to lay over: the buttons that would give the window
        // something, where the picture would be, and the message about
        // what was just done — a failure to open, most likely — under
        // them, as it goes under the panels when there is a picture.
        None => {
            if input.empty {
                empty::show(&mut pass, ui);
            }
            if let Some(message) = &input.toast {
                toast::show(&mut pass, ui, message);
            }
        }
    }
    // Over everything, and whether or not there is a picture yet: the
    // chooser is about the list, and the list is there before the first
    // file has been read.
    if let Some(chooser) = &input.chooser {
        chooser::show(&mut pass, ui, chooser);
    }
    // And the keys, which are the same whatever is on screen.
    help::show(&mut pass, ui);
    // Over all of it: a rename is a question, and nothing else answers
    // until it has.
    if let Some(rename) = &input.rename {
        rename::show(&mut pass, ui, rename);
    }
    if let Some(export) = &input.export {
        export::show(&mut pass, ui, export);
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
        // The region's share of the gestures first, and the zoom box's: a
        // drag that is either's is not the view's.
        let grabbed = self.region_gestures(ui, &response);
        if grabbed.is_none() && response.dragged_by(egui::PointerButton::Primary) {
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
        // The secondary button, from the press on rather than from the
        // toolkit's decision that the press became a drag: the loupe comes
        // up the moment the button goes down. Where the pointer is goes
        // with it, since the application's own pointer stands still while
        // the toolkit holds the button.
        let held = response.is_pointer_button_down_on()
            && ui.input(|input| input.pointer.secondary_down());
        let at = held
            .then(|| response.interact_pointer_pos())
            .flatten()
            .map(|pos| [pos.x * scale, pos.y * scale]);
        self.commands.push(Command::Secondary(at));
        let dragging = response
            .dragged_by(egui::PointerButton::Primary)
            .then(|| response.interact_pointer_pos())
            .flatten()
            .map(|pos| [pos.x * scale, pos.y * scale]);
        self.commands.push(Command::Dragging(dragging));
        if response.contains_pointer() {
            // With the secondary button down the wheel is the loupe's: the
            // hand holding it up is the hand that sets how much it shows.
            let wheel: Vec<Command> = ui.input(|input| {
                input
                    .events
                    .iter()
                    .filter_map(|event| match event {
                        egui::Event::MouseWheel { unit, delta, .. } => {
                            let (steps, notched) = match unit {
                                egui::MouseWheelUnit::Point => {
                                    (delta.y / WHEEL_PIXELS_PER_STEP, false)
                                }
                                egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => {
                                    (delta.y, true)
                                }
                            };
                            Some(if held {
                                Command::Magnify(steps)
                            } else {
                                Command::Wheel { steps, notched }
                            })
                        }
                        _ => None,
                    })
                    .collect()
            });
            self.commands.extend(wheel);
        }
    }

    /// The region's reading of the picture's response, and the hold a drag
    /// has on it, if any: `Some` while the drag is the region's — or the
    /// zoom box's — in which case the view does not pan.
    ///
    /// A drag is decided where the button went down — `press_origin`, not
    /// the pointer's position on the frame the toolkit called it a drag,
    /// which is already some points away. With `Space` held it draws a box
    /// to zoom to, whatever the selection: the key is held for exactly
    /// that, and a region under the press does not take it. Otherwise what
    /// it is depends on the selection: with one asked for, any drag draws a
    /// new region; with one on screen, a drag from a handle takes hold of
    /// that, and a drag from inside it takes hold of the whole only with
    /// `Shift` held — anywhere else, and inside without the key, it is the
    /// view's, as it always was, so the picture stays navigable under a
    /// region that covers it. The hand's place goes back in image pixels
    /// each frame, through the same placement the bar's readout uses, since
    /// the application's own pointer stands still while the toolkit holds a
    /// drag. A click on a handle — a press that never became a drag — makes
    /// it the current one, as a drag on it does. Which handle the pointer
    /// rests on is said every pass a region is up, for the words the region
    /// wears while it is.
    fn region_gestures(&mut self, ui: &egui::Ui, response: &egui::Response) -> Option<Grab> {
        let scale = self.input.scale;
        let placement = self
            .view
            .placement(self.current?.size(), self.input.viewport);
        let image_point = |pos: egui::Pos2| placement.image_point([pos.x * scale, pos.y * scale]);
        let grid = self.grid;
        let handles = self.input.selection.region().map(|region| {
            let rect = region::rect(region, placement, scale);
            (rect, region::handles(rect, grid))
        });
        let grip_under = |pos: egui::Pos2| {
            handles
                .as_ref()
                .and_then(|(rect, handles)| region::grip_at(*rect, handles, [pos.x, pos.y]))
        };
        // The inside is a hold only while the key says so; a handle always is.
        let holds = |grip: Grip| grip != Grip::Inside || self.input.move_region;

        let mut grabbed = self.input.grabbing;
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(origin) = ui.input(|input| input.pointer.press_origin())
        {
            let grab = match self.input.selection {
                _ if self.input.box_zoom => Some(Grab::Zoom),
                Selection::Armed => Some(Grab::New),
                Selection::Shown(_) => grip_under(origin)
                    .filter(|grip| holds(*grip))
                    .map(Grab::Handle),
                Selection::Off => None,
            };
            if let Some(grab) = grab {
                self.commands.push(Command::Grab {
                    grab,
                    at: image_point(origin),
                });
                grabbed = Some(grab);
            }
        }
        // The click's own place: `press_origin` is gone by the time the
        // button is up, and a click has by definition not moved far from it.
        if response.clicked_by(egui::PointerButton::Primary)
            && let Some(pos) = response.interact_pointer_pos()
            && let Some(grip) = grip_under(pos)
            && grip != Grip::Inside
        {
            self.commands.push(Command::Handle(grip));
        }
        if let Some(grab) = grabbed {
            if response.dragged_by(egui::PointerButton::Primary)
                && let Some(pos) = response.interact_pointer_pos()
            {
                self.commands.push(Command::Pull(image_point(pos)));
            }
            // Over when the toolkit is no longer dragging, however that came
            // about: the button up, or Escape taking the drag off it.
            if !response.dragged() {
                self.commands.push(Command::Release);
                grabbed = None;
            } else {
                ui.ctx().set_cursor_icon(region::cursor(grab));
            }
        }
        if handles.is_some() {
            let over = response.hover_pos().and_then(grip_under);
            self.commands.push(Command::OverGrip(over));
            if grabbed.is_none()
                && let Some(grip) = over
                && holds(grip)
            {
                ui.ctx().set_cursor_icon(region::cursor(Grab::Handle(grip)));
            }
        }
        // What the next drag would draw, while the pointer is on the
        // picture with nothing in hand: the box if the key is held, else
        // the region asked for.
        if grabbed.is_none() && response.contains_pointer() {
            if self.input.box_zoom {
                ui.ctx().set_cursor_icon(region::cursor(Grab::Zoom));
            } else if self.input.selection == Selection::Armed {
                ui.ctx().set_cursor_icon(region::cursor(Grab::New));
            }
        }
        grabbed
    }

    /// What floats over the picture, in the order it is stacked: the grid
    /// under everything, then the region marked out on the picture, then
    /// the minimap, then the message about what was just done — over the
    /// panels rather than among them, and there whether or not the bars
    /// are, since what it says does not stop being true because they are
    /// away.
    fn overlays(&mut self, ui: &mut egui::Ui, current: &Current) {
        let content = self.content;
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
            // Nor the loupe's glass: it is the image layer's as well, and
            // the grid's spacing is the view's, not the glass's.
            let glass = self
                .input
                .loupe
                .map(|loupe| (loupe.glass, loupe::GLASS_RADIUS));
            grid::paint(
                ui.painter(),
                self.view.placement(current.size(), self.input.viewport),
                self.input.scale,
                content,
                grid::Clear {
                    minimap: thumbnail,
                    glass,
                },
                grid::step(zoom, self.input.scale),
                self.theme,
            );
        }
        // Under the panels, like the grid: the region marks up the picture,
        // and a panel over the picture is over the region too. The box
        // being dragged out to zoom to goes over the region, being the
        // newer of the two marks and the one under the hand.
        region::show(self, ui);
        region::show_zoom_box(self, ui);
        // Over the region: the loupe is under the hand, and the newer mark.
        loupe::show(self, ui);
        if self.input.minimap_on_screen {
            minimap::show(self, ui);
        }
        if self.panels.show_histogram && self.room.histogram {
            histogram::show(self, ui);
        }
        if self.panels.show_info && self.room.info {
            info::show(self, ui);
        }
        if let Some(message) = &self.input.toast {
            toast::show(self, ui, message);
        }
    }
}

/// What the grid toggle reads out while the grid is on: how far apart its
/// lines are at `zoom`, on a display of `scale` physical pixels to the
/// logical one. `None` while it is off, there being no spacing in force then.
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
    histogram::SIZE[1] + info::INFO_MIN_HEIGHT + 3.0 * PADDING,
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
    /// The help popup, which needs what the information panel needs.
    pub help: bool,
}

/// What `content` has room for, with `panels` saying which of the two is
/// asked for — the histogram's take counting against the information panel
/// only where the histogram is on screen, see [`histogram_shown`]. The
/// histogram is one height for every file, so the file has no say.
pub fn room(content: Rect, panels: &Panels) -> Room {
    Room {
        histogram: histogram::panel(content).is_some(),
        info: info::panel(content, histogram_shown(content, panels)).is_some(),
        help: help::panel(content).is_some(),
    }
}

/// Where the histogram is on screen, or `None` where it is not: its toggle
/// is off, or the window has no room for it. What the information column
/// starts below.
fn histogram_shown(content: Rect, panels: &Panels) -> Option<Rect> {
    panels
        .show_histogram
        .then(|| histogram::panel(content))
        .flatten()
}

/// The hairline drawn across a column: above every section of the
/// information panel but the first and under its header, and under the
/// help popup's headings, which is the same kind of thing. The same width
/// as the hairline along a panel's edge, being the same kind of thing.
const RULE_WIDTH: f32 = 1.0;

/// A hairline across a column, on the device's grid: what parts a panel's
/// header from its rows, and one section from the next.
fn rule(pass: &chrome::Pass, ui: &mut egui::Ui, width: f32) {
    let edge = pass.grid.line_width(RULE_WIDTH);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, edge), egui::Sense::HOVER);
    ui.painter().rect_filled(
        egui::Rect::from_min_size(rect.min, egui::vec2(width, edge)),
        0.0,
        pass.theme.border,
    );
}

/// A rectangle drawn as four edges, so that what is behind it — the
/// thumbnail under the minimap's border, the picture inside a region — stays
/// visible. Four snapped lines, so that an outline is the same weight as
/// itself wherever on the device's grid it lands, and the two down the sides
/// stop where the two across meet them, so a translucent color is not laid
/// twice at the corners.
fn outline(
    painter: &egui::Painter,
    grid: icon::Grid,
    rect: Rect,
    thickness: f32,
    color: egui::Color32,
) {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }
    let edge = grid.line_width(thickness);
    let middle = (rect.height - 2.0 * edge).max(0.0);
    let fill = |piece: Rect| {
        let x = grid.snap(piece.x);
        let y = grid.snap(piece.y);
        painter.rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(x, y),
                egui::vec2(
                    (grid.snap(piece.right()) - x).max(edge),
                    (grid.snap(piece.bottom()) - y).max(edge),
                ),
            ),
            0.0,
            color,
        );
    };
    fill(Rect::new(rect.x, rect.y, rect.width, edge));
    fill(Rect::new(rect.x, rect.bottom() - edge, rect.width, edge));
    fill(Rect::new(rect.x, rect.y + edge, edge, middle));
    fill(Rect::new(rect.right() - edge, rect.y + edge, edge, middle));
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

    /// The copy drawn is the smallest that covers the device pixels it is
    /// drawn across, so none is shrunk to less than half itself; past the
    /// largest, the largest; and a small picture, all of whose copies are
    /// its own size, is drawn from the first.
    #[test]
    fn a_thumbnail_is_drawn_from_the_smallest_copy_that_covers_it() {
        let copy = |id: u64, side: f32| egui::load::SizedTexture {
            id: egui::TextureId::User(id),
            size: egui::vec2(side, side / 2.0),
        };
        let thumb = Thumb {
            copies: [copy(0, 128.0), copy(1, 256.0), copy(2, 512.0)],
        };
        assert_eq!(thumb.for_side(72.0), thumb.copies[0]);
        assert_eq!(thumb.for_side(128.0), thumb.copies[0]);
        assert_eq!(thumb.for_side(129.0), thumb.copies[1]);
        assert_eq!(thumb.for_side(384.0), thumb.copies[2]);
        assert_eq!(thumb.for_side(768.0), thumb.copies[2]);
        let tiny = Thumb {
            copies: [copy(0, 40.0), copy(1, 40.0), copy(2, 40.0)],
        };
        assert_eq!(tiny.for_side(384.0), tiny.copies[2]);
        assert_eq!(tiny.for_side(20.0), tiny.copies[0]);
    }

    /// [`PANELS_ROOM`] is a sum of the constants the two panels are laid out
    /// from, and this is what holds it to what they do with them: a content
    /// area that size has room for both at once, and one a pixel smaller in
    /// either direction does not.
    #[test]
    fn the_panels_room_is_room_for_both() {
        let panels = Panels {
            show_ui: true,
            show_filmstrip: false,
            show_histogram: true,
            show_info: true,
            show_luma: true,
            show_planes: true,
            log_counts: false,
            mark_clipped: false,
            show_minimap: true,
            show_grid: false,
            show_loupe: false,
            loupe_magnification: loupe::DEFAULT_MAGNIFICATION,
            paste: false,
            pixel_format: PixelFormat::default(),
        };
        let area = |width, height| room(Rect::new(0.0, 0.0, width, height), &panels);

        assert_eq!(
            area(PANELS_ROOM[0], PANELS_ROOM[1]),
            Room {
                histogram: true,
                info: true,
                help: true,
            }
        );
        assert!(!area(PANELS_ROOM[0] - 1.0, PANELS_ROOM[1]).histogram);
        assert!(!area(PANELS_ROOM[0], PANELS_ROOM[1] - 1.0).info);
    }
}
