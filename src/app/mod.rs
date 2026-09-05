//! Window lifecycle, key handling, and building each frame's interface.

mod files;
pub mod input;
mod kept;
mod window;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{Window, WindowId};

use crate::image::decode;
use crate::image::display::{Display, Headroom, Startup};
use crate::loader::{Decoded, Loader, Opened, Ready, Reload, Request};
use crate::monitor::{Mode, Monitors};
use crate::motion::Motion;
use crate::render::{HdrPreference, Placement, Rect, Renderer, Scene, Upscale};
use crate::theme::{self, Theme};
use crate::timing;
use crate::ui::chrome::{Chrome, content_area, image_viewport};
use crate::ui::layers::{Hit, Shown};
use crate::ui::toast::{self, Level, Toasts};
use crate::ui::{self, Current, FileFacts, FrameInput, Panels, Reading, Tooltips};
use crate::view::{View, Viewport};
use crate::watch::{self, Watch};

use files::{Announce, Files};
use input::{Effect, Pointer};
use kept::{Kept, Settings};
use window::{file_label, initial_window_size, loading_title, window_title};

/// What wakes the event loop from another thread.
pub enum UserEvent {
    /// A file the loader has finished with. Boxed: it carries the pixels,
    /// and the other variant carries nothing.
    Decoded(Box<Decoded>),
    /// A monitor's mode was learned, or changed.
    Monitor,
}

/// What the command line asked for, beyond which files to show.
pub struct Options {
    pub overrides: decode::Overrides,
    pub startup: Startup,
    pub hdr: HdrPreference,
    pub histogram: bool,
    pub info: bool,
    pub minimap: bool,
    pub upscale: Upscale,
    /// What `--size` asked the window to open at, in logical pixels.
    pub size: Option<[u32; 2]>,
}

/// What a copy prepared on a thread of its own did: took the selection, or
/// failed with this much to say about it.
type CopyOutcome = Result<(), String>;

pub struct App {
    files: Files,
    current: Option<Current>,
    startup: Startup,
    /// What was asked of the surface: by `--output`, and by every press of
    /// the switch since. Read against the monitor's mode in
    /// [`App::surface_hdr`] and [`App::headroom`].
    hdr: HdrPreference,
    /// The compositor's word on the monitors, where it gives one.
    monitors: Option<Monitors>,
    /// The mode of the monitor the window is on, as last read: `None` until
    /// the window has landed on one, and for good where nothing says.
    monitor: Option<Mode>,
    /// Where the view is going: the pan and zoom every key and press act
    /// on. What is on screen is [`App::shown_view`], which is this once it
    /// has arrived.
    view: View,
    /// The move the view is in the middle of, from where it was shown when
    /// the last animated change was asked for to wherever `view` now says.
    /// `None` once it has landed, and while nothing is moving.
    motion: Option<Motion>,
    /// What each file that has been on screen was left in, so that stepping
    /// back to one puts it back rather than opening it afresh: the view, and
    /// everything the display is doing to it.
    kept: Kept,
    /// The file on screen, watched for writes by anything else.
    watch: Watch,
    /// The paths as the command line gave them, and a watch on each directory
    /// among them. A directory is a place to look rather than a fixed list:
    /// images appearing in it or disappearing from it while the window is open
    /// join or leave the walk, noticed on the same cadence as a write to the
    /// file on screen. Empty — and so costing nothing — when every path named
    /// was a file.
    named: Vec<PathBuf>,
    directories: Vec<Watch>,
    /// The colors everything is drawn in, and the palette file they came
    /// from, watched on the same cadence as the image: Omarchy rewrites it
    /// wholesale when the desktop's theme changes, and the window should
    /// follow rather than stay in the theme it opened under.
    theme: Theme,
    theme_watch: Watch,
    /// When to look at it next.
    next_poll: Instant,
    /// What the header said the first file's size was, so that the window can
    /// open at the right shape before the pixels arrive. Only ever consulted
    /// while `current` is empty, and `None` for a format whose header would
    /// not say.
    header_size: Option<[f32; 2]>,
    /// The size `--size` asked the window to open at, in logical pixels, if it
    /// asked for one. Read once, when the window is made; every size after
    /// that is the compositor's to give.
    asked_size: Option<[u32; 2]>,
    /// The thread that reads files.
    ///
    /// Declared before the renderer on purpose: fields are dropped in the
    /// order they are written, and the loader has been given a handle on the
    /// GPU device. Shutting the thread down first means the last reference to
    /// that device is the renderer's, and so the device is destroyed here on
    /// the thread that made it. See [`Loader::drop`] for what goes wrong when
    /// the thread is still running as the process leaves `main`.
    loader: Loader,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    pointer: Pointer,
    panels: Panels,
    /// When the label naming what the pointer is resting on opens and closes.
    /// Apart from the panels because it is the one thing on screen that
    /// depends on how long something has been true: `panels.tooltip` is what
    /// it has settled on, and this is the clock behind it.
    tooltips: Tooltips,
    /// The message about what was just done, and when it takes itself off.
    /// The other thing on screen that time alone changes.
    toasts: Toasts,
    /// How the copies being prepared on threads of their own turned out. A
    /// copy of the picture has to walk every pixel before it can say whether
    /// it worked, and the thread doing that has no business touching the
    /// interface — so it sends the outcome here, and the loop picks it up on
    /// the same cadence it looks at the file, the palette and the clipboard
    /// on. `Err` carries the one line the window shows; the whole chain has
    /// already gone to the terminal.
    copied: (mpsc::Sender<CopyOutcome>, mpsc::Receiver<CopyOutcome>),
    /// Copies of the picture still being prepared, joined before the loop
    /// leaves. A copy is often followed straight away by `q`, and a thread
    /// that has not yet handed its bytes over dies with the process — the
    /// copy would go missing for no reason the user could see.
    copying: Vec<JoinHandle<()>>,
    /// Counts copies asked for, so that one still being prepared can tell it
    /// has been superseded. Copying the picture takes long enough on a large
    /// image for a second press to arrive while the first is still working,
    /// and the clipboard should end up holding the one asked for last rather
    /// than whichever finished last. Shared with the threads doing the work.
    copies: Arc<AtomicU64>,
    /// Whether the window has already said how to bring the interface back.
    /// The message goes up the first time the bars are hidden and not again:
    /// with them gone there is nothing on screen that could say it, and a
    /// message every time would be in the way of the picture that was just
    /// asked for. Here rather than in [`Panels`] because it is what has
    /// happened, not what is on screen.
    said_how_to_restore: bool,
    /// Set if the last render failed, so we report it once rather than every frame.
    reported_error: bool,
}

impl App {
    /// `size` is what the header of `files[index]` said, where it would say:
    /// enough to open the window at the right shape before the pixels exist.
    /// The file itself is asked for here, so that it is being read while the
    /// window and the GPU are still being set up.
    ///
    /// `named` is the command line's own list, `files` before any directory in
    /// it was replaced by the images inside. Kept so that those directories
    /// can be looked at again and the list built from them anew.
    pub fn new(
        files: Vec<PathBuf>,
        named: Vec<PathBuf>,
        index: usize,
        size: Option<[f32; 2]>,
        options: Options,
        loader: Loader,
        monitors: Option<Monitors>,
    ) -> Self {
        let Options {
            overrides,
            startup,
            hdr,
            histogram,
            info,
            minimap,
            upscale,
            size: asked_size,
        } = options;
        let watch = Watch::new(&files[index]);
        let directories = named
            .iter()
            .filter(|path| path.is_dir())
            .map(|path| Watch::new(path))
            .collect();
        let theme_watch = theme::watch();
        let mut view = View::new();
        view.set_upscale(upscale);
        let mut app = Self {
            files: Files::new(files, index, overrides),
            current: None,
            header_size: size,
            asked_size,
            startup,
            hdr,
            monitors,
            monitor: None,
            view,
            motion: None,
            kept: Kept::default(),
            watch,
            named,
            directories,
            theme: Theme::detect(),
            theme_watch,
            next_poll: Instant::now() + watch::INTERVAL,
            loader,
            window: None,
            renderer: None,
            pointer: Pointer::default(),
            tooltips: Tooltips::default(),
            toasts: Toasts::default(),
            copied: mpsc::channel(),
            copying: Vec::new(),
            copies: Arc::new(AtomicU64::new(0)),
            panels: Panels {
                show_ui: true,
                show_histogram: histogram,
                show_luma: true,
                show_planes: true,
                log_counts: false,
                show_info: info,
                info_scroll: 0.0,
                show_minimap: minimap,
                show_grid: false,
                paste: false,
                pixel_format: ui::PixelFormat::default(),
                hover: None,
                info_hover: None,
                state_hover: false,
                menu: None,
            },
            said_how_to_restore: false,
            reported_error: false,
        };
        let request = app.files.open_first();
        app.send(request);
        app
    }

    /// Whether anything ever reached the screen. False only when every file
    /// named on the command line failed to decode.
    pub fn showed_nothing(&self) -> bool {
        self.current.is_none()
    }

    /// The size to open the window at: the image's, once there is one, and
    /// otherwise whatever the header claimed on the way past.
    fn opening_size(&self) -> Option<[f32; 2]> {
        self.current
            .as_ref()
            .map(Current::size)
            .or(self.header_size)
    }

    /// Whether the surface should be the HDR one. The monitor decides where
    /// the compositor says what it is in: one in HDR mode gets the HDR
    /// surface, which costs the compositor nothing and gives the picture the
    /// room, and one in SDR mode gets the SDR surface — unless `--output
    /// hdr` asked for the other regardless, which is the one route left that
    /// asks a compositor to switch a monitor over. Where nothing says what
    /// the monitor is, what was asked for is all there is to go on.
    fn surface_hdr(&self) -> bool {
        match self.monitor {
            Some(Mode::Hdr) => true,
            Some(Mode::Sdr) | None => self.hdr == HdrPreference::On,
        }
    }

    /// Whether the picture is going out with room above SDR white, which is
    /// half of what a tone map defaults from and half of what every readout
    /// of a value says. It takes three things: a surface with the room, a
    /// monitor not known to be in SDR mode — a compositor maps an HDR surface
    /// down for one that is, and the room is not there however the surface
    /// was made — and the switch not having turned it off. Before there is a
    /// window the answer is the SDR one, and [`App::adopt_headroom`] asks
    /// again whenever any of the three moves.
    fn headroom(&self) -> Headroom {
        let surface = self
            .renderer
            .as_ref()
            .is_some_and(|renderer| renderer.output().is_hdr);
        if surface && self.monitor != Some(Mode::Sdr) && self.hdr != HdrPreference::Off {
            Headroom::Above
        } else {
            Headroom::None
        }
    }

    /// Whether the switch has anything to switch: an HDR color space is
    /// offered for the window, and the monitor is in HDR mode — or nothing
    /// can say what it is in. Where the compositor can say and has not yet,
    /// which is the moment before the window has landed on a monitor, the
    /// answer is no: most monitors are SDR, and a switch that lit for a
    /// frame and then died would be the switch having been wrong.
    fn hdr_available(&self) -> bool {
        self.renderer.as_ref().is_some_and(Renderer::hdr_available)
            && (self.monitors.is_none() || self.monitor == Some(Mode::Hdr))
    }

    /// Puts the surface where [`App::surface_hdr`] says and the curve where
    /// the headroom that leaves says, and reports whether the picture
    /// changed. Called whenever an input to either moves: the window landing
    /// on a monitor, the monitor changing mode, the switch being pressed.
    /// Which surface it is goes to stderr when it changes, the way the choice
    /// at start-up does, since the bar has room for one word and the
    /// surface's name is several.
    fn sync_output(&mut self) -> bool {
        let before = self.headroom();
        let wanted = self.surface_hdr();
        let mut changed = false;
        if let Some(renderer) = &mut self.renderer
            && renderer.output().is_hdr != wanted
            && renderer.set_hdr(wanted)
        {
            eprintln!("gamut: {} output", renderer.output().label);
            changed = true;
        }
        if self.headroom() != before {
            self.adopt_headroom();
            changed = true;
        }
        changed
    }

    /// Reads which monitor the window is on and what the compositor says it
    /// is in, and follows a change. Cheap enough to ask after every batch of
    /// events, which is how a window carried to another monitor is noticed:
    /// nothing else says. Returns whether anything on screen changed — the
    /// picture, or only the switch, which a monitor's mode lights or kills.
    fn sync_monitor(&mut self) -> bool {
        let Some(monitors) = &self.monitors else {
            return false;
        };
        let name = self
            .window
            .as_ref()
            .and_then(|window| window.current_monitor())
            .and_then(|monitor| monitor.name());
        let mode = name.as_deref().and_then(|name| monitors.mode(name));
        if mode == self.monitor {
            return false;
        }
        // Worth a line, since it is what lights the switch or kills it.
        if let (Some(name), Some(mode)) = (&name, mode) {
            let mode = match mode {
                Mode::Hdr => "HDR",
                Mode::Sdr => "SDR",
            };
            eprintln!("gamut: monitor {name} is in {mode} mode");
        }
        self.monitor = mode;
        self.sync_output();
        true
    }

    /// Re-derives the tone map for whatever is on screen, for the moment the
    /// output is settled and its headroom is known at last, and for every
    /// switch of it after.
    ///
    /// The file named on the command line is decoded before the window opens,
    /// so its display state is worked out against an SDR surface whatever the
    /// surface turns out to be. A curve asked for on the command line is left
    /// alone: that is a choice rather than a default.
    fn adopt_headroom(&mut self) {
        if self.startup.tone_map.is_some() {
            return;
        }
        let headroom = self.headroom();
        if let Some(current) = &mut self.current {
            current.display.adopt(headroom, &current.stats);
        }
    }

    /// Switches the room above white on or off. Returns whether anything
    /// changed.
    ///
    /// On a monitor in HDR mode the surface stays the HDR one either way and
    /// the compositor clips at white instead, so that the switch never asks
    /// the compositor for anything it might answer with a modeset. Where
    /// nothing says what the monitor is, the switch moves the surface
    /// itself, as the only lever there is. On a monitor in SDR mode there is
    /// no room to switch to, and the press is refused with a word on why.
    ///
    /// The curve follows the headroom, as it does when the window first
    /// opens: the switch chooses the curve the room wants for what is on
    /// screen, and `t` changes it afterwards.
    pub(super) fn toggle_hdr(&mut self) -> bool {
        if self.renderer.is_none() {
            return false;
        }
        if self.monitor == Some(Mode::Sdr) {
            eprintln!(
                "gamut: this monitor is in SDR mode, so there is no room above white to switch to"
            );
            return false;
        }
        if self.monitors.is_some() && self.monitor.is_none() {
            eprintln!("gamut: the compositor has not yet said which monitor this is on");
            return false;
        }
        if !self.hdr_available() {
            eprintln!("gamut: no HDR color space is offered for this window");
            return false;
        }
        self.hdr = if self.headroom() == Headroom::Above {
            HdrPreference::Off
        } else {
            HdrPreference::On
        };
        self.sync_output()
    }

    fn image_size(&self) -> [f32; 2] {
        self.current
            .as_ref()
            .map(Current::size)
            .unwrap_or([1.0, 1.0])
    }

    fn window_size(&self) -> [f32; 2] {
        self.renderer
            .as_ref()
            .map(Renderer::size)
            .unwrap_or([1.0, 1.0])
    }

    fn scale_factor(&self) -> f32 {
        self.window
            .as_ref()
            .map(|window| window.scale_factor() as f32)
            .unwrap_or(1.0)
    }

    /// The window in the logical pixels the interface is laid out in. Events
    /// and the surface are both in physical ones.
    fn logical_size(&self) -> [f32; 2] {
        let scale = self.scale_factor();
        let physical = self.window_size();
        [physical[0] / scale, physical[1] / scale]
    }

    /// Where the panels are this frame. Cheap enough to derive on demand, and
    /// deriving it means there is no cached layout to fall out of step with
    /// the window.
    fn chrome(&self) -> Chrome {
        Chrome::new(self.logical_size())
    }

    /// Where the info panel is, when it is on screen. The one thing floating
    /// over the image that takes the pointer for itself, so the pointer has
    /// to be able to ask where it is.
    fn info_panel(&self) -> Option<Rect> {
        if !self.panels.show_info || self.current.is_none() {
            return None;
        }
        ui::info::panel(self.content(), self.panels.show_histogram)
    }

    /// Whether the content area has room for each of the two panels that
    /// float over it. The frame builder works this out for itself; it is
    /// worked out here as well for the presses and the tooltips, which have
    /// to answer between frames.
    pub(super) fn room(&self) -> ui::Room {
        ui::room(self.content(), &self.panels)
    }

    /// What the panels leave free for the image and for whatever floats over
    /// it, in the logical pixels those are laid out in. The frame builder
    /// works this out for itself; it is worked out here as well for the hit
    /// tests, which have to answer between frames.
    fn content(&self) -> Rect {
        content_area(self.logical_size(), self.panels.show_ui)
    }

    /// What the top bar says about a read that is taking its time, which is
    /// part of the name it sets: worked out here rather than in the frame
    /// builder because the pointer is answered against that name between
    /// frames and has to see the same words.
    fn reading(&self) -> Option<Reading> {
        self.files
            .pending()
            // With nothing on screen there is no flicker to guard against and
            // nothing else to say, so the wait is worth naming immediately.
            .filter(|pending| pending.announced || self.current.is_none())
            .map(|pending| {
                if self.current.is_some() && pending.index == self.files.index() {
                    Reading::Again
                } else {
                    Reading::File(file_label(self.files.path(pending.index)))
                }
            })
    }

    /// Which of the top bar's own runs of words the pointer is on, if any.
    ///
    /// Asked with the fonts the bar is drawn in, as the information panel's
    /// rows are: a run of words ends where the face it is set in says, and
    /// the pointer has to be answered against what was actually drawn.
    pub(super) fn bar_tip(&mut self) -> Option<ui::Tip> {
        if !self.panels.show_ui {
            return None;
        }
        let point = self.logical_cursor()?;
        let chrome = self.chrome();
        // Asked on every motion, including the ones over the picture, so the
        // cheap question comes before the layout it would otherwise pay for.
        if !chrome.top.contains(point) {
            return None;
        }
        let bar = chrome.top;
        let limit = chrome.zoom_button().x;
        let (index, count) = (self.files.index(), self.files.len());
        // Past the pair of step buttons, which are there on exactly the terms
        // the count beside them is — see `Chrome::step_buttons`.
        let start = chrome.bar_text_x(count > 1);
        let deleted = self.watch.missing();
        let reading = self.reading();
        // Split borrow: the measurement needs the renderer while it reads the
        // file the bar is about.
        let (Some(renderer), Some(current)) = (self.renderer.as_mut(), self.current.as_ref())
        else {
            return None;
        };
        let about = ui::BarText {
            current,
            reading: reading.as_ref(),
            index,
            count,
            deleted,
        };
        ui::bar_tip(renderer, point, bar, start, limit, &about)
    }

    /// Whether the pointer is on the bottom bar's words about what is being
    /// done to the picture — which name themselves, and open the histogram
    /// panel when pressed.
    ///
    /// Asked afresh rather than read off [`Panels::state_hover`] wherever it
    /// decides anything: a press can arrive before the pointer has moved
    /// since the words last changed, and what is stored there is only what
    /// was true at the last motion.
    pub(super) fn state_hover(&mut self) -> bool {
        if !self.panels.show_ui {
            return false;
        }
        let Some(point) = self.logical_cursor() else {
            return false;
        };
        let chrome = self.chrome();
        // The cheap question first: this is asked on every motion, including
        // the ones over the picture.
        if !chrome.bottom.contains(point) {
            return false;
        }
        let bar = chrome.bottom;
        let limit = chrome.state_limit();
        let headroom = self.headroom();
        // Split borrow, as in `bar_tip`.
        let (Some(renderer), Some(current)) = (self.renderer.as_mut(), self.current.as_ref())
        else {
            return false;
        };
        ui::state_hover(renderer, point, bar, limit, current, headroom)
    }

    /// The image on screen, as the layers need to know it. `None` before the
    /// first decode, when the panels that describe an image are not drawn.
    fn shown(&self) -> Option<Shown> {
        let current = self.current.as_ref()?;
        Some(Shown {
            size: current.size(),
            gray: current.image.channels().is_gray(),
            minimap: self.minimap_on_screen(),
        })
    }

    /// Which layer of the interface the pointer is on: the one question every
    /// pointer handler asks, so that the highlight, the press, the wheel and
    /// the bar's readout cannot disagree about what is under it.
    ///
    /// `None` before the pointer has said where it is — a press can genuinely
    /// arrive first — and once it has left the window.
    pub(super) fn pointer_hit(&self) -> Option<Hit> {
        let point = self.logical_cursor()?;
        Some(ui::layers::hit(
            point,
            &self.panels,
            self.logical_size(),
            self.shown(),
            self.grid_spacing().as_deref(),
            self.files.len() > 1,
            self.toasts.showing(),
        ))
    }

    /// What the grid toggle is reading out, and so how much of the top bar it
    /// is taking: how far apart its lines are at the zoom on screen, or
    /// `None` with the grid off or nothing to lay one over.
    ///
    /// Worked out from the same zoom the frame builder works it out from,
    /// rather than remembered from the frame it drew: it is the width of a
    /// button the pointer has to be answered against, and a width kept
    /// between the two of them is a width that can fall out of step.
    pub(super) fn grid_spacing(&self) -> Option<String> {
        let current = self.current.as_ref()?;
        let zoom = self.shown_view().zoom(current.size(), self.viewport());
        ui::grid_spacing(self.panels.show_grid, zoom, self.scale_factor())
    }

    /// Whether the menu that is open has the pointer, rather than the layer
    /// the pointer happens to be over.
    ///
    /// A menu takes the pointer for as long as it is open, as menus do
    /// everywhere: a press anywhere off it dismisses it instead of reaching
    /// what it landed on, the wheel is spent on it, and nothing behind it
    /// lights up under the pointer. What the pointer is *over* is unaffected,
    /// which is why the bar goes on reading out the pixel under it.
    pub(super) fn menu_has_pointer(&self, hit: Option<Hit>) -> bool {
        self.panels.menu.is_some() && !hit.is_some_and(Hit::is_menu)
    }

    /// Which bin of the histogram the panel is marking, or `None` when it is
    /// marking none.
    ///
    /// What the panel draws its rule and its readout from, and so what a
    /// motion compares before and after to decide whether the frame on screen
    /// has gone out of date. It answers for the pointer over the plot and for
    /// the pixel under it alike, so either one moving on is caught here.
    pub(super) fn histogram_mark(&self) -> Option<usize> {
        if !self.panels.show_histogram {
            return None;
        }
        let current = self.current.as_ref()?;
        ui::histogram::marked(
            current,
            self.content(),
            self.logical_cursor(),
            self.pointer_pixel(),
        )
    }

    /// Where the info panel is when the pointer is on it, and so when what it
    /// does next belongs to the panel rather than to the image behind it.
    ///
    /// Asked of the layers rather than of the panel's own rectangle, so that
    /// a menu drawn over the panel keeps the pointer it is covering.
    pub(super) fn pointer_over_info(&self) -> Option<Rect> {
        if self.pointer_hit()? != Hit::Info {
            return None;
        }
        self.info_panel()
    }

    /// How far the column in `panel` may still be scrolled, measured with the
    /// fonts the frame will draw it with. Zero when it all fits, and when
    /// there is nothing on screen to describe.
    pub(super) fn info_overflow(&mut self, panel: Rect) -> f32 {
        // Split borrow: the measurement needs the renderer while it reads the
        // image the column is about.
        let (Some(renderer), Some(current)) = (self.renderer.as_mut(), self.current.as_ref())
        else {
            return 0.0;
        };
        ui::info::max_scroll(renderer, current, panel)
    }

    /// What clicking where the pointer is would copy out of the info panel,
    /// and so which of its copy buttons is showing. `None` when the pointer
    /// is somewhere else, or on a part of the panel that copies nothing.
    pub(super) fn info_copyable(&mut self) -> Option<ui::info::Copyable> {
        let panel = self.pointer_over_info()?;
        let point = self.logical_cursor()?;
        let scroll = self.panels.info_scroll;
        // Split borrow, as in `info_overflow`.
        let (Some(renderer), Some(current)) = (self.renderer.as_mut(), self.current.as_ref())
        else {
            return None;
        };
        ui::info::copyable_at(renderer, current, panel, scroll, point)
    }

    /// Raises the message at the foot of the window, in place of whatever was
    /// up. Handlers say it and return `Effect::Redraw`; nothing here asks the
    /// window for a frame.
    ///
    /// Measured against the fonts the frame will set it in, once and here,
    /// which is why it needs the renderer at all: nothing about the message
    /// changes while it is up, so the frame builder and the pointer both
    /// place it from that one number. No renderer means no window to show it
    /// on, and nothing has been asked for yet.
    pub(super) fn toast(&mut self, message: impl Into<String>, level: Level) {
        if let Some(renderer) = self.renderer.as_mut() {
            self.toasts.show(
                renderer,
                Instant::now(),
                message.into(),
                level,
                toast::LINGER,
            );
        }
    }

    /// Says what the copies prepared on their own threads did. Returns
    /// whether anything was said, and so whether a redraw is owed.
    ///
    /// Taken up on the file check's cadence rather than the moment the thread
    /// finishes: a copy of a large picture takes far longer than the wait
    /// itself, and a quarter of a second either way on a message about it is
    /// not a difference anyone can see.
    fn poll_copies(&mut self) -> bool {
        let outcomes: Vec<CopyOutcome> = self.copied.1.try_iter().collect();
        let said = !outcomes.is_empty();
        for outcome in outcomes {
            match outcome {
                Ok(()) => self.toast("Copied image.", Level::Message),
                Err(error) => self.toast(error, Level::Error),
            }
        }
        said
    }

    /// How far the column in `panel` moves for each logical pixel a drag of
    /// its scrollbar travels, measured with the fonts the frame will draw it
    /// with. One where there is nothing on screen to describe.
    pub(super) fn info_scroll_per_drag(&mut self, panel: Rect) -> f32 {
        // Split borrow, as in `info_overflow`.
        let (Some(renderer), Some(current)) = (self.renderer.as_mut(), self.current.as_ref())
        else {
            return 1.0;
        };
        ui::info::scroll_per_drag(renderer, current, panel)
    }

    /// Moves the info panel's column by `by` logical pixels, clamped to what
    /// there is left to scroll. Returns whether it moved, and so whether the
    /// frame is now out of date.
    pub(super) fn scroll_info_by(&mut self, panel: Rect, by: f32) -> bool {
        if !by.is_finite() || by == 0.0 {
            return false;
        }
        let limit = self.info_overflow(panel);
        let scrolled = (self.panels.info_scroll + by).clamp(0.0, limit);
        let moved = scrolled != self.panels.info_scroll;
        self.panels.info_scroll = scrolled;
        moved
    }

    /// Where the image is drawn, in physical pixels: what the panels leave in
    /// the middle, or the whole window when they are hidden. Derived rather
    /// than stored, so toggling the interface re-fits a fitted image without
    /// anything having to remember to.
    fn viewport(&self) -> Viewport {
        image_viewport(self.window_size(), self.scale_factor(), self.panels.show_ui)
    }

    /// The view as it is on screen at `now`: `view` itself once it has
    /// arrived, and somewhere along the way to it while a move is in flight.
    /// Everything that reads the picture — the frame, the pixel under the
    /// pointer, the grid's spacing, the minimap's marker — reads this, so
    /// that they agree with one another about what is on screen mid-move.
    fn view_at(&self, now: Instant) -> View {
        match &self.motion {
            Some(motion) => {
                let (image, viewport) = (self.image_size(), self.viewport());
                let to = self.view.position(image, viewport);
                self.view.at(motion.position(to, now))
            }
            None => self.view,
        }
    }

    pub(super) fn shown_view(&self) -> View {
        self.view_at(Instant::now())
    }

    /// Makes `change` to the view, and puts it on screen as a move over
    /// [`crate::motion::DURATION`] rather than in one jump. The move starts
    /// from where the view is shown at this instant, which part way through
    /// an earlier move is part way along it: that move is dropped, and this
    /// one has the whole time to get from there to where `change` leaves the
    /// view.
    ///
    /// For a change asked for by name — a key, a notch of the wheel, a
    /// choice from the menu. A change the hand is on — a drag, a single
    /// pixel's step, a trackpad's scroll — goes to `view` directly and lands
    /// at once, or, if a move is in flight, at the end of it: the move is
    /// left running, and finds the view moved when it looks.
    pub(super) fn animate(&mut self, change: impl FnOnce(&mut View, [f32; 2], Viewport)) {
        let (image, viewport) = (self.image_size(), self.viewport());
        let now = Instant::now();
        let from = self.view_at(now).position(image, viewport);
        change(&mut self.view, image, viewport);
        let to = self.view.position(image, viewport);
        // Nowhere to go — a fit already fitted, an edge already reached, a
        // filter changed — is not a move, and owes no frames.
        self.motion = (to != from).then(|| Motion::new(from, now));
    }

    /// The pointer in logical pixels, which is what the interface is laid out
    /// in. Events arrive in physical ones.
    fn logical_cursor(&self) -> Option<[f32; 2]> {
        let scale = self.scale_factor();
        self.pointer
            .cursor
            .map(|cursor| [cursor[0] / scale, cursor[1] / scale])
    }

    /// The image pixel under the pointer, or `None` when there is not one:
    /// the pointer is outside the window, over a panel, or past the edge of
    /// an image that does not fill the viewport it sits in.
    ///
    /// Tested against the viewport as well as the image because a zoomed-in
    /// image runs on underneath the panels, where it is not drawn and so has
    /// no pixel to report.
    fn pointer_pixel(&self) -> Option<[u32; 2]> {
        // Anything drawn over the picture takes the pointer rather than
        // letting it through: the bar would otherwise read out a pixel nobody
        // can see, under the panel that is covering it, and the histogram's
        // own mark would follow the pointer across its ramp and its buttons
        // to whatever happened to be behind them.
        if !self.pointer_hit()?.is_image() {
            return None;
        }
        let cursor = self.pointer.cursor?;
        let viewport = self.viewport();
        if !viewport.contains(cursor) {
            return None;
        }
        let image = self.current.as_ref()?.size();
        let point = self
            .shown_view()
            .placement(image, viewport)
            .image_point(cursor);
        // Written as a positive range test rather than four negated bounds so
        // that a NaN coordinate is rejected: every `<`/`>=` comparison is
        // false for NaN, so the old form let a NaN through to read as pixel
        // (0, 0) — a coordinate the readout must never invent.
        let inside = (0.0..image[0]).contains(&point[0]) && (0.0..image[1]).contains(&point[1]);
        if !inside {
            return None;
        }
        Some([point[0] as u32, point[1] as u32])
    }

    /// Whether the minimap is on screen, which takes the toggle and a view
    /// that has something to point out. A view holding the whole image is
    /// already its own map, so the widget would be a second copy of what the
    /// window is showing, over the corner of it; it goes away instead, and
    /// comes back on the zoom that first cuts something off. The toggle keeps
    /// its state through that, so the button stays lit and the minimap
    /// returns without being asked for again.
    fn minimap_on_screen(&self) -> bool {
        // Panning and the minimap answer the same question: whether any of the
        // image is off screen. Pan is clamped to the image, so a view with
        // nowhere to go is one showing all of it.
        self.panels.show_minimap
            && self
                .shown_view()
                .can_pan(self.image_size(), self.viewport())
    }

    /// Where the minimap's thumbnail goes, in physical pixels: the whole
    /// image, drawn small in the corner the interface will then mark up.
    ///
    /// It is the image layer that draws it, from the same texture as the view
    /// itself, so this is a placement like any other and everything that
    /// applies to the image — the window, the colormap, the tone map — comes
    /// with it for nothing.
    fn minimap_placement(&self, logical: [f32; 2], scale: f32) -> Option<Placement> {
        if !self.minimap_on_screen() {
            return None;
        }
        let image = self.current.as_ref()?.size();
        ui::minimap::placement(
            logical,
            scale,
            self.panels.show_ui,
            image,
            self.view.upscale(),
        )
    }

    /// Re-reads the file on screen if something else has written to it, which
    /// is what makes this usable next to whatever produced the image.
    ///
    /// Returns whether the window owes a redraw, which it does when the file
    /// has gone or come back: the picture is untouched either way, and the bar
    /// is the only thing that changes.
    fn poll_file(&mut self) -> bool {
        // Not while a read is already in flight. A file being written
        // continuously would otherwise stack up a decode every interval, and
        // the reply already on its way carries a watch taken later than this
        // one anyway.
        if !self.files.is_idle() {
            return false;
        }
        let was_missing = self.watch.missing();
        if self.watch.poll()
            && let Some(request) = self.files.reload()
        {
            self.send(request);
        }
        self.watch.missing() != was_missing
    }

    /// Notices images arriving in or leaving a directory that was named on the
    /// command line, and builds the list from it again. Returns whether the
    /// window owes a redraw, which it does only when the list really changed —
    /// the bar counts the files and says which of them is on screen.
    fn poll_directories(&mut self) -> bool {
        // Between reads only: rebuilding moves the file on screen to a new
        // index, and a reply on its way is aimed at the old one. Nothing is
        // lost by waiting, since a watch not polled is a watch that has not
        // seen the change yet and will see it at a later look.
        if self.directories.is_empty() || !self.files.is_idle() {
            return false;
        }
        // Every one of them is polled, not just as far as the first that
        // fires: each has its own idea of what has settled to keep up to date.
        let changed = self.directories.iter_mut().fold(false, |changed, watch| {
            let fired = watch.poll();
            fired || changed
        });
        changed && self.files.relist(crate::listing::relist(&self.named))
    }

    /// Notices a picture arriving on the clipboard or leaving it, which is
    /// what puts the paste button on screen and takes it off again. Returns
    /// whether the answer changed, and so whether the window owes a redraw.
    ///
    /// Asked rather than waited for, as everything else on this tick is:
    /// nothing tells a program that the selection has changed, and one look
    /// costs about as much as the handful of `stat`s beside it. Only while
    /// the interface is on screen, since the button is the only thing that
    /// depends on the answer — `` ` `` therefore stops the looking as well as
    /// hiding the button.
    fn poll_clipboard(&mut self) -> bool {
        let offered =
            self.panels.show_ui && matches!(crate::clipboard::offered_image(), Ok(Some(_)));
        if offered == self.panels.paste {
            return false;
        }
        self.panels.paste = offered;
        // A button that has just appeared under a pointer that has not moved
        // should light up, and one that has just gone should not leave the
        // highlight behind it. Motion is what usually asks this question, and
        // this is the one thing that can change the answer without any.
        self.update_hover();
        true
    }

    /// Notices that the desktop's theme has changed. Returns whether the
    /// window owes a redraw, which it does only when the new palette actually
    /// resolves to different colors.
    fn poll_theme(&mut self) -> bool {
        if !self.theme_watch.poll() {
            return false;
        }
        let theme = Theme::detect();
        let changed = theme != self.theme;
        self.theme = theme;
        changed
    }

    /// What the window is called: the image on screen, or the file being read
    /// while there is nothing on screen to name.
    fn title(&self) -> String {
        match &self.current {
            Some(_) => window_title(self.files.shown_path()),
            None => {
                let index = self
                    .files
                    .pending()
                    .map_or(self.files.index(), |pending| pending.index);
                loading_title(self.files.path(index))
            }
        }
    }

    /// Sends a request to the loader.
    ///
    /// Nothing changes on screen here. The image already up stays where it is,
    /// still pannable and zoomable, until the reply arrives at
    /// [`App::user_event`] — which is the whole point of the exercise, and the
    /// reason everything the interface says about the image goes on describing
    /// the one being shown rather than the one being fetched.
    fn send(&mut self, request: Request) {
        self.loader.request(request);
        // With nothing on screen the title is the only thing naming the file,
        // so it follows the request rather than the pixels — including when a
        // walk moves on past one that would not decode.
        if self.current.is_none()
            && let Some(window) = &self.window
        {
            window.set_title(&self.title());
        }
    }

    /// Moves to the next or previous file.
    fn step(&mut self, forward: bool) {
        if let Some(request) = self.files.step(forward) {
            self.send(request);
        }
    }

    /// Puts a finished read on screen. Returns `false` if the upload failed,
    /// which leaves the current image where it is.
    fn apply(&mut self, file: Opened, ready: Ready) -> bool {
        let Ready {
            image,
            stats,
            exif,
            gpu,
        } = ready;
        let size = [image.width as f32, image.height as f32];
        let same_size = self
            .current
            .as_ref()
            .is_some_and(|current| current.size() == size);
        // Re-reading the same file keeps the user where they were, since they
        // are watching one spot for the change: same exposure and tone map,
        // with only an automatic window re-derived from the new pixels. One
        // that has come back a different size is a new shape to fit, and is
        // treated as a new picture below.
        let in_place = file.mode == Reload::InPlace && same_size;
        // Whether this is a move between files at all. The file already on
        // screen being read again is not one, whatever it has become: it is
        // neither a departure to be put away nor a return to be restored.
        let stepping = self.current.is_some() && self.files.shown_path() != file.path;
        // The picture being stepped away from, kept as it stands so that
        // stepping back to it finds it as it was left.
        if let Some(current) = self.current.as_ref().filter(|_| stepping) {
            self.kept.keep(
                self.files.shown_path(),
                Settings {
                    view: self.view,
                    display: current.display.clone(),
                },
            );
        }
        // And what the file arriving left the last time it was on screen, if
        // it has been here. Its window is re-derived where it was automatic,
        // the file being free to have changed on disk since; one set by hand
        // is left exactly where it was put.
        let kept = stepping
            .then(|| self.kept.left(&file.path).cloned())
            .flatten();
        let display = match (self.current.as_ref().filter(|_| in_place), &kept) {
            (Some(current), _) => {
                let mut display = current.display.clone();
                display.refresh_auto(&stats);
                display
            }
            (None, Some(settings)) => {
                let mut display = settings.display.clone();
                display.refresh_auto(&stats);
                display
            }
            (None, None) => Display::for_image_with(&image, &stats, self.startup, self.headroom()),
        };

        let mut stored = None;
        if let Some(renderer) = &mut self.renderer {
            // Already across whenever the window was open when the read
            // started, which is every file but the one named on the command
            // line. The fallback covers only that gap.
            let uploaded = match gpu {
                Some(uploaded) => uploaded,
                None => match renderer.uploader().run(&image) {
                    Ok(uploaded) => uploaded,
                    Err(error) => {
                        eprintln!("gamut: {}", crate::escape_controls(&format!("{error:#}")));
                        return false;
                    }
                },
            };
            if let Some(note) = renderer.install_image(uploaded) {
                eprintln!("gamut: {note}");
            }
            stored = renderer.image_format_label();
        }

        self.files.shown(file.index);
        self.watch = file.watch;
        match &kept {
            // A picture of the same size as the one it is arriving beside is
            // almost always part of a set to be compared — frames of a
            // sequence, or one exposure against another — and there the point
            // is that the same detail stays under the same pixels. So the pan
            // and zoom carry over from the picture leaving the screen, ahead
            // of anything this file was left in itself: what the comparison
            // is being made at is where the eye already is, not where this
            // file happened to be the last time it was looked at.
            _ if same_size => {}
            // Back to a file of another size that has been here before:
            // exactly where it was left. The magnification filter is not part
            // of a view — it is a standing preference — so it stays as it is.
            Some(settings) => {
                let upscale = self.view.upscale();
                self.view = settings.view;
                self.view.set_upscale(upscale);
            }
            // A new shape, seen for the first time, so it is fitted afresh.
            None => self.view.reset(),
        }
        if !in_place {
            // A move under way was about the picture that has just left, and
            // there is nothing for it to carry the eye across any more.
            self.motion = None;
            // A different picture is a different column of words about it,
            // and it is read from the top.
            self.panels.info_scroll = 0.0;
        }
        self.current = Some(Current {
            image: Arc::new(image),
            stats,
            display,
            label: file_label(&file.path),
            file: file_facts(&file.path),
            exif,
            stored,
        });
        if let Some(window) = &self.window {
            window.set_title(&window_title(&file.path));
        }
        true
    }

    /// Takes in a file the loader has finished with. Held apart from the
    /// handler that receives it, since nothing here needs the event loop.
    fn deliver(&mut self, decoded: Decoded) {
        // Anything but the newest request is a file the user has stepped past
        // while it was being read. Its pixels are correct and unwanted.
        let Some(pending) = self.files.accept(decoded.generation) else {
            return;
        };

        let index = decoded.file.index;
        // A file that will not go on screen is a file to step over, whether it
        // was the decode or the upload that would not have it.
        let failed = match decoded.outcome {
            Ok(ready) => !self.apply(decoded.file, ready),
            Err(error) => {
                eprintln!("gamut: {}", crate::escape_controls(&format!("{error:#}")));
                true
            }
        };
        if failed && let Some(request) = self.files.failed(index, pending.step) {
            self.send(request);
        }

        // Owed either way: on success for the new image, and on failure
        // because the bar may have been saying that a read was under way.
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn redraw(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        if self.renderer.is_none() {
            return;
        }

        // A move that has landed is over: what is on screen is `view`
        // itself, and the frames it was asking for can stop.
        let now = Instant::now();
        if self.motion.as_ref().is_some_and(|motion| motion.done(now)) {
            self.motion = None;
        }
        let view = self.view_at(now);

        let scale = window.scale_factor() as f32;
        let physical = self.window_size();
        let logical = [physical[0] / scale, physical[1] / scale];
        let viewport = self.viewport();
        let placement = view.placement(self.image_size(), viewport);

        let pointer = self.pointer_pixel();
        let cursor = self.logical_cursor();
        let thumbnail = self.minimap_placement(logical, scale);
        let minimap = self.minimap_on_screen();
        let reading = self.reading();
        let tooltip = self.tooltip();
        let headroom = self.headroom();
        let hdr_available = self.hdr_available();
        let input = FrameInput {
            logical,
            scale,
            viewport,
            pointer,
            cursor,
            minimap_on_screen: minimap,
            reading,
            index: self.files.index(),
            count: self.files.len(),
            deleted: self.watch.missing(),
            headroom,
            hdr_available,
            tooltip,
            toast: self.toasts.showing().cloned(),
        };

        // Split borrow: the frame builder needs the renderer's font metrics
        // while reading the rest of the application state.
        let renderer = self.renderer.as_mut().expect("checked above");
        let frame = ui::build_frame(
            renderer,
            &input,
            &self.panels,
            self.current.as_ref(),
            &view,
            &self.theme,
        );

        let fallback = Display::default();
        let display = self
            .current
            .as_ref()
            .map(|current| &current.display)
            .unwrap_or(&fallback);

        let backdrop = ui::backdrop(&self.theme);

        let scene = Scene {
            placement,
            thumbnail,
            display,
            frame: &frame,
            scale,
            backdrop,
            headroom,
        };
        match renderer.render(scene) {
            Ok(()) => self.reported_error = false,
            Err(error) => {
                if !self.reported_error {
                    eprintln!("gamut: {}", crate::escape_controls(&format!("{error:#}")));
                    self.reported_error = true;
                }
            }
        }

        // A move still in flight owes the next frame. Asked for from here
        // rather than timed from the loop, so that it comes when the
        // compositor is ready for one and the move plays at the display's
        // own rate.
        if self.motion.is_some() {
            window.request_redraw();
        }
    }
}

/// What the file system says about the file on screen, for the info panel.
///
/// One look at it as the image goes up, rather than a look per frame: none of
/// this changes while the image is on screen, and a file being written to is
/// re-read whole anyway. Nothing is owed if it cannot be had — the file may
/// have been replaced between being read and being asked about.
fn file_facts(path: &std::path::Path) -> FileFacts {
    let metadata = std::fs::metadata(path).ok();
    FileFacts {
        path: path.display().to_string(),
        bytes: metadata.as_ref().map(|metadata| metadata.len()),
        modified: metadata.and_then(|metadata| metadata.modified().ok()),
        reader: decode::reader(path),
    }
}

impl ApplicationHandler<UserEvent> for App {
    /// Look at the file, then sleep until it is time to look again rather than
    /// until the next event: nothing tells us about a write, so we go and ask.
    ///
    /// The deadline is a fixed cadence rather than an interval from here, so
    /// that a stream of events — a drag, a resize — cannot keep pushing the
    /// next look out of reach.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.sync_monitor()
            && let Some(window) = &self.window
        {
            window.request_redraw();
        }

        let now = Instant::now();
        if now >= self.next_poll {
            self.next_poll = now + watch::INTERVAL;
            // All of them, always: each has a watch that only advances when
            // it is polled.
            let vanished = self.poll_file();
            let relisted = self.poll_directories();
            let retinted = self.poll_theme();
            let offered = self.poll_clipboard();
            let copied = self.poll_copies();
            if (vanished || relisted || retinted || offered || copied)
                && let Some(window) = &self.window
            {
                window.request_redraw();
            }
        }

        // The two things on screen that happen because time passed rather
        // than because anything arrived: the pointer resting on a button long
        // enough to be told what it is, and the message about what was just
        // done having been up long enough. The message taking itself off
        // takes a button off the screen with it, so the pointer is asked
        // again where it now is.
        let mut timed = self.tooltips.tick(now);
        if self.toasts.tick(now) {
            self.update_hover();
            timed = true;
        }
        if timed && let Some(window) = &self.window {
            window.request_redraw();
        }

        // Sleep until the next thing with a time on it: the file check, the
        // moment a read that is still going becomes worth mentioning, or the
        // moment a tooltip is due or a message has had its time. A read that finishes first wakes us
        // through the proxy instead.
        let mut deadline = self.next_poll;
        for due in [self.tooltips.deadline(), self.toasts.deadline()]
            .into_iter()
            .flatten()
        {
            deadline = deadline.min(due);
        }
        match self.files.announce_slow_read(now) {
            Announce::Waiting(due) => deadline = deadline.min(due),
            Announce::Now => {
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            Announce::Nothing => {}
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Decoded(decoded) => {
                self.deliver(*decoded);
                // Nothing ever reached the screen and nothing else is coming:
                // every file named on the command line failed to decode.
                // Stop, rather than sit in an empty window with nothing on
                // the way.
                if self.current.is_none() && self.files.is_idle() {
                    event_loop.exit();
                }
            }
            UserEvent::Monitor => {
                if self.sync_monitor()
                    && let Some(window) = &self.window
                {
                    window.request_redraw();
                }
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let size = initial_window_size(event_loop, self.opening_size(), self.asked_size);
        let attributes = window::with_app_id(
            Window::default_attributes()
                .with_title(self.title())
                .with_inner_size(size),
        );

        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                eprintln!("gamut: could not open a window: {error}");
                event_loop.exit();
                return;
            }
        };
        timing::window_open();

        let mut renderer = match Renderer::new(window.clone(), self.hdr) {
            Ok(renderer) => renderer,
            Err(error) => {
                eprintln!("gamut: {}", crate::escape_controls(&format!("{error:#}")));
                event_loop.exit();
                return;
            }
        };

        if let Some(current) = &mut self.current {
            match renderer.set_image(&current.image) {
                Ok(note) => {
                    if let Some(note) = note {
                        eprintln!("gamut: {note}");
                    }
                    current.stored = renderer.image_format_label();
                }
                Err(error) => {
                    eprintln!("gamut: {}", crate::escape_controls(&format!("{error:#}")));
                    event_loop.exit();
                    return;
                }
            }
        }

        // Asked for and not had is worth a line; asked for and had is worth
        // one too, since the switch's later lines say the same thing. A
        // surface that follows the monitor says so when it moves.
        if self.hdr == HdrPreference::On {
            let output = renderer.output();
            eprintln!(
                "gamut: {} \u{2192} {} output{}",
                renderer.adapter_name(),
                output.label,
                if output.is_hdr {
                    ""
                } else {
                    " (no HDR color space offered for this window)"
                }
            );
        }

        // From here on the loader uploads as well as decodes, so that
        // stepping to the next file costs the event loop nothing but the swap.
        self.loader.attach(renderer.uploader());
        self.renderer = Some(renderer);
        self.window = Some(window);
        // The surface exists at last, so whatever was decoded before the
        // window opened can find out what it is being drawn onto. Which
        // monitor it is on is not known until it has been shown, and the
        // surface follows it from `about_to_wait`.
        self.adopt_headroom();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let effect = match event {
            WindowEvent::CloseRequested => Effect::Quit,
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
                Effect::Redraw
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.pointer.modifiers = modifiers.state();
                Effect::Nothing
            }
            WindowEvent::CursorMoved { position, .. } => {
                Effect::redraw_if(self.handle_motion([position.x as f32, position.y as f32]))
            }
            WindowEvent::CursorLeft { .. } => {
                let was_over = self.pointer_pixel().is_some() || self.histogram_mark().is_some();
                self.pointer.cursor = None;
                Effect::redraw_if(self.update_hover() || was_over)
            }
            WindowEvent::MouseInput { state, button, .. } => {
                Effect::redraw_if(self.handle_button(state, button))
            }
            // A drag the window did not see end — the button came up over
            // another window, say — would otherwise resume on the next motion.
            //
            // Whatever the press had hold of on the info panel is dropped
            // rather than copied: losing the window is not a click, and a
            // copy is bound for somewhere else, where an unasked-for one
            // would be pasted in place of whatever the user had meant to keep.
            WindowEvent::Focused(false) => {
                self.pointer.copying = None;
                let _ = self.handle_button(ElementState::Released, MouseButton::Left);
                Effect::Nothing
            }
            WindowEvent::MouseWheel { delta, .. } => Effect::redraw_if(self.handle_wheel(delta)),
            WindowEvent::ScaleFactorChanged { .. } => Effect::Redraw,
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key,
                        physical_key,
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => self.handle_key(&logical_key, physical_key),
            WindowEvent::RedrawRequested => {
                self.redraw();
                Effect::Nothing
            }
            _ => Effect::Nothing,
        };
        match effect {
            Effect::Redraw => {
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            Effect::Quit => event_loop.exit(),
            Effect::Nothing => {}
        }
    }

    /// The loop is done. A copy that is still being prepared gets to finish
    /// handing its bytes over first: the thread doing it would otherwise go
    /// down with the process, and the whole point of copying here is that it
    /// outlasts the window.
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        for thread in self.copying.drain(..) {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::image::{Stats, exif};
    use crate::view::Fit;

    const WINDOW: [f32; 2] = [1000.0, 700.0];
    /// The same window with nothing taken out of it, for the tests that are
    /// about stepping between files rather than about where the panels are.
    const VIEWPORT: Viewport = Viewport::whole(WINDOW);

    /// A gray PNG of the given size, written where the test can step onto it.
    fn write_png(dir: &Path, name: &str, width: u32, height: u32) -> PathBuf {
        let path = dir.join(name);
        let pixels = vec![128u8; (width * height * 3) as usize];
        ::image::save_buffer(&path, &pixels, width, height, ::image::ColorType::Rgb8)
            .expect("the temporary directory is writable");
        path
    }

    /// The files are written under a directory of their own so that the tests,
    /// which run alongside each other, cannot tread on each other's files.
    fn opening(name: &str, files: &[(&str, u32, u32)]) -> (App, PathBuf) {
        let (dir, paths) = written(name, files);
        (open(paths.clone(), paths), dir)
    }

    /// The same, opened the way `gamut some-dir/` opens it: the directory is
    /// what was named, and the files in it are only what it held at the time.
    fn opening_directory(name: &str, files: &[(&str, u32, u32)]) -> (App, PathBuf) {
        let (dir, paths) = written(name, files);
        (open(paths, vec![dir.clone()]), dir)
    }

    fn written(name: &str, files: &[(&str, u32, u32)]) -> (PathBuf, Vec<PathBuf>) {
        let dir = std::env::temp_dir().join(format!("gamut-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the temporary directory is writable");
        let paths = files
            .iter()
            .map(|&(name, width, height)| write_png(&dir, name, width, height))
            .collect();
        (dir, paths)
    }

    fn open(paths: Vec<PathBuf>, named: Vec<PathBuf>) -> App {
        let options = Options {
            overrides: decode::Overrides::default(),
            startup: Startup::default(),
            hdr: HdrPreference::default(),
            histogram: false,
            info: false,
            minimap: false,
            upscale: Upscale::default(),
            size: None,
        };
        let size = decode::probe(&paths[0]).expect("we just wrote it");
        App::new(
            paths,
            named,
            0,
            size.map(|(w, h)| [w as f32, h as f32]),
            options,
            Loader::detached(),
            None,
        )
    }

    /// As [`opening`], with the application's own opening request answered:
    /// the state the tests about later behavior want to start from.
    fn app_over(name: &str, files: &[(&str, u32, u32)]) -> (App, PathBuf) {
        let (mut app, dir) = opening(name, files);
        answer(&mut app, Reload::Fresh);
        (app, dir)
    }

    /// Cuts a file off part way through its pixel data: it still says what
    /// format it is and how large, so the header check passes, and only the
    /// decode fails. That is the case start-up cannot catch up front, and the
    /// reason the first file is asked for as a walk.
    ///
    /// Both halves are asserted here rather than assumed, so that a change in
    /// what the header check reads fails loudly instead of quietly leaving
    /// the tests below testing nothing.
    fn corrupt(path: &Path) {
        let length = std::fs::metadata(path).expect("we just wrote it").len();
        std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .expect("we just wrote it")
            .set_len(length / 2)
            .expect("the file is writable");
        assert!(
            decode::probe(path).is_ok_and(|size| size.is_some()),
            "the header has to survive, or this is not the case being tested"
        );
        assert!(
            decode::load(path, decode::Overrides::default()).is_err(),
            "the pixels have to be beyond saving, or this is not the case being tested"
        );
    }

    /// The round trip a request makes through the loader, made here instead:
    /// a test has no event loop to carry one. Reads whatever the application
    /// last asked for, under the generation it asked for it with, so that the
    /// staleness check sees exactly what it would in the running program.
    fn answer(app: &mut App, mode: Reload) {
        let pending = app.files.pending().expect("a request is in flight");
        let (generation, index) = (pending.generation, pending.index);
        let path = app.files.path(index).to_path_buf();
        let watch = Watch::new(&path);
        let outcome = decode::load(&path, app.files.overrides()).map(|image| Ready {
            stats: Stats::scan(&image),
            exif: exif::Exif::read(&path),
            image,
            gpu: None,
        });
        app.deliver(Decoded {
            generation,
            file: Opened {
                index,
                path,
                mode,
                watch,
            },
            outcome,
        });
    }

    /// A key's pan is a move: the view is where it is going at once, and
    /// what is on screen gets there over [`crate::motion::DURATION`]. A
    /// single pixel's is not, and lands as it is pressed.
    #[test]
    fn a_keyboard_pan_moves_and_a_single_pixel_lands() {
        use input::{Action, Direction, PanStep};

        let (mut app, dir) = app_over("motion", &[("a.png", 64, 48)]);
        let (image, viewport) = (app.image_size(), app.viewport());
        app.view.set_zoom(4.0, image, viewport);
        let before = app.view.position(image, viewport);

        let _ = app.perform(Action::Pan(Direction::Right, PanStep::Coarse));
        assert!(app.motion.is_some());
        let target = app.view.position(image, viewport);
        assert!(target.u[0] > before.u[0]);
        // Just begun: on screen it has barely left where it was.
        let now = Instant::now();
        let shown = app.view_at(now).position(image, viewport);
        assert!(shown.u[0] < target.u[0]);
        // Landed, and where the view says.
        let landed = app.view_at(now + crate::motion::DURATION);
        assert_eq!(landed.position(image, viewport), target);

        app.motion = None;
        let _ = app.perform(Action::Pan(Direction::Left, PanStep::Fine));
        assert!(app.motion.is_none());

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// Escape puts away whatever is up, topmost first, and leaves only when
    /// there is nothing left to put away. `q` is not held up by a message:
    /// a copy is often followed straight away by it.
    #[test]
    fn escape_puts_things_away_before_it_quits() {
        use input::{Action, Effect};

        let (mut app, dir) = app_over("dismiss", &[("a.png", 64, 48)]);
        let raise = |app: &mut App| {
            app.toasts.show(
                &mut crate::ui::Monospace,
                Instant::now(),
                "Copied file path.".to_string(),
                Level::Message,
                toast::LINGER,
            );
        };

        // The menu outranks the message, and each press takes off one thing.
        app.panels.menu = Some(ui::Menu::Zoom);
        raise(&mut app);
        assert_eq!(app.perform(Action::Dismiss), Effect::Redraw);
        assert_eq!(app.panels.menu, None);
        assert!(app.toasts.showing().is_some());

        assert_eq!(app.perform(Action::Dismiss), Effect::Redraw);
        assert!(app.toasts.showing().is_none());
        assert_eq!(app.perform(Action::Dismiss), Effect::Quit);

        // `q` leaves whether or not there is a message to read.
        raise(&mut app);
        assert_eq!(app.perform(Action::Quit), Effect::Quit);

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// Escape is what brings the interface back, and it does that before it
    /// takes off the message that said so: a window that dismissed its own
    /// instructions and left the bars hidden would be disagreeing with what
    /// it had just told the reader. `q` still leaves from under it.
    #[test]
    fn escape_brings_the_interface_back_before_it_quits() {
        use input::{Action, Effect};

        let (mut app, dir) = app_over("restore", &[("a.png", 64, 48)]);
        assert!(app.panels.show_ui);

        // Hidden, and the window has said once how to get it back.
        let _ = app.perform(Action::ToggleInterface);
        assert!(!app.panels.show_ui);
        assert!(app.said_how_to_restore);

        // Escape brings it back rather than quitting out from under it.
        assert_eq!(app.perform(Action::Dismiss), Effect::Redraw);
        assert!(app.panels.show_ui);
        // And with it back, Escape is the quit it always was.
        assert_eq!(app.perform(Action::Dismiss), Effect::Quit);

        // The key that closes the floating panels on its way says it too:
        // what it hides is the same thing, by the same route.
        app.said_how_to_restore = false;
        let _ = app.perform(Action::ToggleInterfaceAndPanels);
        assert!(app.said_how_to_restore);
        // And `q` leaves from under a hidden interface, as it always did.
        assert_eq!(app.perform(Action::Quit), Effect::Quit);

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// Stepping between frames of the same size is a comparison — the same
    /// detail has to stay under the same pixels, or there is nothing to
    /// compare.
    #[test]
    fn stepping_to_an_image_of_the_same_size_keeps_the_view() {
        let (mut app, dir) = app_over("same", &[("a.png", 64, 48), ("b.png", 64, 48)]);
        app.view.set_zoom(1.0, app.image_size(), VIEWPORT);
        app.view.zoom_in(app.image_size(), VIEWPORT);
        let zoom = app.view.zoom(app.image_size(), VIEWPORT);

        app.step(true);
        // Nothing has moved yet: the file has only been asked for.
        assert_eq!(app.files.index(), 0);

        answer(&mut app, Reload::Fresh);
        assert_eq!(app.files.index(), 1);
        assert_eq!(app.view.fit(), None);
        assert_eq!(app.view.zoom(app.image_size(), VIEWPORT), zoom);

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// A file coming back at the size of the one it is arriving beside is a
    /// comparison, and it is made where the eye is: the pan and zoom carry
    /// over from the picture leaving the screen, whatever this file was left
    /// in the last time it was looked at.
    #[test]
    fn a_same_size_neighbor_takes_the_view_it_arrives_beside() {
        let (mut app, dir) = app_over("compared", &[("a.png", 64, 48), ("b.png", 64, 48)]);

        // b.png is left at 4x, so it has a view of its own to be put back.
        app.step(true);
        answer(&mut app, Reload::Fresh);
        app.view.set_zoom(4.0, app.image_size(), VIEWPORT);

        // Back to a.png, and on to somewhere else in it.
        app.step(false);
        answer(&mut app, Reload::Fresh);
        app.view.set_zoom(2.0, app.image_size(), VIEWPORT);
        let zoom = app.view.zoom(app.image_size(), VIEWPORT);

        // And on to b.png again: at a.png's zoom, not the 4x it was left in.
        app.step(true);
        answer(&mut app, Reload::Fresh);
        assert_eq!(app.files.shown_path(), dir.join("b.png"));
        assert_eq!(app.view.fit(), None);
        assert_eq!(app.view.zoom(app.image_size(), VIEWPORT), zoom);

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// Flipping between two pictures is how they are compared, so each of
    /// them has to come back as it was left: its own pan and zoom, its own
    /// window and exposure, its own false color.
    #[test]
    fn a_file_comes_back_as_it_was_left() {
        use crate::image::display::Colormap;

        let (mut app, dir) = app_over("kept", &[("a.png", 64, 48), ("b.png", 32, 16)]);
        app.view.set_zoom(4.0, app.image_size(), VIEWPORT);
        let zoom = app.view.zoom(app.image_size(), VIEWPORT);
        let display = app.current.as_mut().expect("a.png is on screen");
        display.display.adjust_exposure(2.0);
        display.display.cycle_colormap();
        let colormap = display.display.colormap;

        // Another size, so nothing carries over: b.png opens fitted and with
        // the display its own pixels ask for.
        app.step(true);
        answer(&mut app, Reload::Fresh);
        assert_eq!(app.view.fit(), Some(Fit::Whole));
        let display = &app.current.as_ref().expect("b.png is on screen").display;
        assert_eq!(display.exposure_stops, 0.0);
        assert_eq!(display.colormap, Colormap::Gray);

        // And back, to everything a.png was left in.
        app.step(false);
        answer(&mut app, Reload::Fresh);
        assert_eq!(app.files.shown_path(), dir.join("a.png"));
        assert_eq!(app.view.fit(), None);
        assert_eq!(app.view.zoom(app.image_size(), VIEWPORT), zoom);
        let display = &app.current.as_ref().expect("a.png is on screen").display;
        assert_eq!(display.exposure_stops, 2.0);
        assert_eq!(display.colormap, colormap);

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// A file rewritten under the window is the same file being read again,
    /// not a return to it: one that comes back a different size is a new
    /// shape and is fitted afresh, rather than being put back into the view
    /// it was being looked at in.
    #[test]
    fn a_reload_is_not_a_return() {
        let (mut app, dir) = app_over("reloaded", &[("a.png", 64, 48)]);
        app.view.set_zoom(4.0, app.image_size(), VIEWPORT);
        assert_eq!(app.view.fit(), None);

        write_png(&dir, "a.png", 32, 16);
        let request = app.files.reload().expect("nothing else is being read");
        app.send(request);
        answer(&mut app, Reload::InPlace);

        assert_eq!(app.image_size(), [32.0, 16.0]);
        assert_eq!(app.view.fit(), Some(Fit::Whole), "a new shape to fit");

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// A directory named on the command line is a place to look, not a list
    /// fixed when the window opened: an image written into it joins the walk,
    /// and one taken out of it leaves.
    #[test]
    fn a_directory_is_read_again_when_what_is_in_it_changes() {
        let (mut app, dir) = opening_directory("relist", &[("a.png", 8, 8), ("b.png", 8, 8)]);
        answer(&mut app, Reload::Fresh);
        assert_eq!(app.files.len(), 2);
        assert!(!app.poll_directories(), "nothing has happened to it");

        write_png(&dir, "c.png", 8, 8);
        assert!(!app.poll_directories(), "the change has not settled yet");
        assert!(app.poll_directories());
        assert_eq!(app.files.len(), 3);
        assert_eq!(app.files.path(2), dir.join("c.png"));
        assert_eq!(
            app.files.shown_path(),
            dir.join("a.png"),
            "the picture on screen is undisturbed"
        );

        std::fs::remove_file(dir.join("b.png")).expect("we just wrote it");
        assert!(!app.poll_directories(), "the change has not settled yet");
        assert!(app.poll_directories());
        assert_eq!(app.files.len(), 2);
        assert_eq!(app.files.path(1), dir.join("c.png"));

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// Deleting the file being looked at does not take the picture off the
    /// screen — there is nothing to put in its place — so the bar says what
    /// has happened to it, and the walk goes on around it.
    #[test]
    fn a_deleted_file_stays_on_screen_and_is_marked() {
        let (mut app, dir) = opening_directory("deleted", &[("a.png", 8, 8), ("b.png", 8, 8)]);
        answer(&mut app, Reload::Fresh);
        assert!(!app.poll_file(), "nothing has happened to it");
        assert!(!app.watch.missing());

        std::fs::remove_file(dir.join("a.png")).expect("we just wrote it");
        assert!(!app.poll_file(), "one poll into a save is not a deletion");
        assert!(app.poll_file(), "the bar has something new to say");
        assert!(app.watch.missing());
        assert!(
            !app.poll_file(),
            "and having been said once it is not said again"
        );
        assert!(app.current.is_some(), "the picture is untouched");

        // The list still names it, and still steps around it. Rebuilding it
        // changes nothing: the file on screen goes back in where it was, so
        // the count in the bar and the walk are the same as they were.
        assert!(!app.poll_directories());
        assert!(!app.poll_directories());
        assert_eq!(app.files.len(), 2);
        assert_eq!(app.files.shown_path(), dir.join("a.png"));
        app.step(true);
        assert_eq!(
            app.files.pending().map(|pending| pending.index),
            Some(1),
            "`]` goes on to the file that is still there"
        );

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// The list is rebuilt between reads and not during one: a rebuild moves
    /// the file on screen to a new index, and the reply on its way is aimed at
    /// the old one. The change is not lost — the watch has not seen it yet.
    #[test]
    fn a_directory_is_not_rebuilt_under_a_read_in_flight() {
        let (mut app, dir) = opening_directory("mid-read", &[("a.png", 8, 8), ("b.png", 8, 8)]);
        answer(&mut app, Reload::Fresh);

        write_png(&dir, "c.png", 8, 8);
        app.step(true);
        assert!(!app.files.is_idle());
        for _ in 0..4 {
            assert!(!app.poll_directories(), "not while a read is in flight");
        }
        assert_eq!(app.files.len(), 2);

        answer(&mut app, Reload::Fresh);
        assert!(!app.poll_directories(), "the first look at the change");
        assert!(app.poll_directories());
        assert_eq!(app.files.len(), 3);

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// Holding `]` through a directory asks for each file in turn without
    /// waiting for the last, and only the file the user stopped on is shown.
    /// Anything else would be a picture they have already scrolled past.
    #[test]
    fn a_reply_the_user_has_stepped_past_is_dropped() {
        let (mut app, dir) = app_over(
            "stale",
            &[("a.png", 64, 48), ("b.png", 32, 32), ("c.png", 16, 16)],
        );

        app.step(true);
        let overtaken = app
            .files
            .pending()
            .expect("a request is in flight")
            .generation;
        app.step(true);
        assert_eq!(app.files.pending().map(|pending| pending.index), Some(2));

        // The first file arrives late, after the user has moved past it.
        let path = app.files.path(1).to_path_buf();
        let image = decode::load(&path, app.files.overrides()).expect("we just wrote it");
        app.deliver(Decoded {
            generation: overtaken,
            file: Opened {
                index: 1,
                watch: Watch::new(&path),
                path,
                mode: Reload::Fresh,
            },
            outcome: Ok(Ready {
                stats: Stats::scan(&image),
                exif: exif::Exif::default(),
                image,
                gpu: None,
            }),
        });
        assert_eq!(
            app.files.index(),
            0,
            "an overtaken file must not reach the screen"
        );

        // The one actually waited for still lands.
        answer(&mut app, Reload::Fresh);
        assert_eq!(app.files.index(), 2);

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// A file whose header reads cleanly and whose pixels do not gets past
    /// the check that happens before the window opens. Opening asks for the
    /// first file as a walk for exactly that reason, so start-up steps over it
    /// as `]` would step over it later.
    #[test]
    fn a_first_file_that_will_not_decode_is_stepped_over() {
        let (mut app, dir) = opening(
            "first-broken",
            &[("a.png", 64, 48), ("b.png", 32, 32), ("c.png", 16, 16)],
        );
        corrupt(app.files.path(0));

        assert_eq!(app.files.pending().map(|pending| pending.index), Some(0));
        answer(&mut app, Reload::Fresh);
        assert!(app.current.is_none(), "nothing can be shown yet");
        assert_eq!(
            app.files.pending().map(|pending| pending.index),
            Some(1),
            "the walk carries on to the next file"
        );

        answer(&mut app, Reload::Fresh);
        assert_eq!(app.files.index(), 1);
        assert!(!app.showed_nothing());

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// When none of them decode there is nothing to look at, and the caller
    /// needs to know so it can leave with a failing status rather than sit in
    /// an empty window.
    #[test]
    fn nothing_decoding_at_all_is_reported_as_having_shown_nothing() {
        let (mut app, dir) = opening("all-broken", &[("a.png", 64, 48), ("b.png", 32, 32)]);
        for index in 0..app.files.len() {
            corrupt(app.files.path(index));
        }

        for _ in 0..app.files.len() {
            if app.files.is_idle() {
                break;
            }
            answer(&mut app, Reload::Fresh);
        }
        assert!(app.files.is_idle(), "the walk has to stop asking");
        assert!(app.showed_nothing());

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// While nothing is on screen the title names the file being read, and it
    /// follows the walk rather than staying on a file that would not open.
    #[test]
    fn the_title_names_the_file_being_read_until_there_is_one_to_show() {
        let (mut app, dir) = opening("title", &[("a.png", 64, 48), ("b.png", 32, 32)]);
        corrupt(app.files.path(0));

        assert_eq!(app.title(), "loading a.png — gamut");
        answer(&mut app, Reload::Fresh);
        assert_eq!(app.title(), "loading b.png — gamut");
        answer(&mut app, Reload::Fresh);
        assert_eq!(app.title(), "b.png — gamut");

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// A file that will not decode must not trap navigation: the walk carries
    /// on in the direction it was going.
    #[test]
    fn a_file_that_will_not_decode_is_stepped_over() {
        let (mut app, dir) = app_over(
            "broken",
            &[("a.png", 64, 48), ("b.png", 32, 32), ("c.png", 16, 16)],
        );
        std::fs::write(app.files.path(1), b"not a png at all").expect("the file is writable");

        app.step(true);
        answer(&mut app, Reload::Fresh);
        assert_eq!(app.files.index(), 0, "the broken file cannot be shown");
        assert_eq!(
            app.files.pending().map(|pending| pending.index),
            Some(2),
            "and the walk carries on past it"
        );

        answer(&mut app, Reload::Fresh);
        assert_eq!(app.files.index(), 2);

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// And it gives up once it has been all the way round, rather than asking
    /// for files for ever when none of them will open.
    #[test]
    fn a_walk_through_files_that_all_fail_comes_to_a_stop() {
        let (mut app, dir) = app_over(
            "hopeless",
            &[("a.png", 64, 48), ("b.png", 32, 32), ("c.png", 16, 16)],
        );
        for index in 1..app.files.len() {
            std::fs::write(app.files.path(index), b"not a png at all")
                .expect("the file is writable");
        }

        app.step(true);
        for _ in 0..app.files.len() {
            if app.files.is_idle() {
                break;
            }
            answer(&mut app, Reload::Fresh);
        }
        assert!(app.files.is_idle(), "the walk has to stop asking");
        assert_eq!(app.files.index(), 0);

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// A file of another size is another picture, and gets the opening view.
    #[test]
    fn stepping_to_an_image_of_another_size_fits_it() {
        let (mut app, dir) = app_over("other", &[("a.png", 64, 48), ("b.png", 32, 32)]);
        app.view.set_zoom(1.0, app.image_size(), VIEWPORT);
        app.view.zoom_in(app.image_size(), VIEWPORT);

        app.step(true);
        answer(&mut app, Reload::Fresh);
        assert_eq!(app.files.index(), 1);
        assert_eq!(app.view.fit(), Some(Fit::Whole));

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }
}
