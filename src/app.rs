//! Window lifecycle, key handling, and building each frame's interface.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Cursor, CursorIcon, Window, WindowId};

use crate::image::decode;
use crate::image::display::{Display, Startup};
use crate::loader::{Decoded, Loader, Opened, Ready, Reload, Request};
use crate::render::{HdrPreference, Placement, Renderer, Scene, Upscale};
use crate::theme::{self, Theme};
use crate::timing;
use crate::ui::chrome::{BAR_HEIGHT, Chrome, SIDE_WIDTH, image_viewport};
use crate::ui::{self, Current, FrameInput, Panels, Reading, Widget};
use crate::view::{View, Viewport};
use crate::watch::{self, Watch};

/// Window pixels moved per arrow-key press.
const PAN_STEP: f32 = 64.0;
/// Trackpad pixels that add up to one notch of the wheel. Wheels report whole
/// lines and need no conversion; a trackpad reports the scroll it would have
/// done, and this is what turns that into the same zoom increment.
const WHEEL_PIXELS_PER_STEP: f32 = 50.0;
/// Fraction of the monitor a freshly opened window may occupy.
const MAX_WINDOW_FRACTION: f64 = 0.85;
/// What a window opens at when the first file's header will not say how large
/// its image is. Every format read here does say, so this is a fallback for a
/// decoder added later without a header probe rather than a size anything
/// reaches today.
const DEFAULT_IMAGE: [f32; 2] = [960.0, 640.0];
/// How long a file may take to open before the bar says so. Long enough that
/// the ordinary case — a file that opens between two frames — never flickers a
/// word into the interface and out again.
const SLOW_READ: Duration = Duration::from_millis(120);

/// What the command line asked for, beyond which files to show.
pub struct Options {
    pub overrides: decode::Overrides,
    pub startup: Startup,
    pub hdr: HdrPreference,
    pub histogram: bool,
    pub minimap: bool,
    pub upscale: Upscale,
}

/// What [`App::announce_slow_read`] found: a read to say something about now,
/// one to look at again at a given moment, or nothing worth a word.
#[derive(PartialEq, Eq, Debug)]
enum Announce {
    Now,
    Waiting(Instant),
    Nothing,
}

/// A read that has been asked for and not yet answered.
struct Pending {
    /// Which request it is, so that a reply arriving after the user has moved
    /// on can be recognised and dropped.
    generation: u64,
    index: usize,
    /// Set when the request came from `n` or `p`, so that a file that will not
    /// decode can be stepped over rather than stopping the walk.
    step: Option<Step>,
    since: Instant,
    /// Whether the bar has been told to mention it. Latched so that the wait
    /// is announced once rather than on every frame it spans.
    announced: bool,
}

/// A walk through the file list, carried along so that it can continue past a
/// file that fails to decode.
#[derive(Clone, Copy)]
struct Step {
    forward: bool,
    /// How many further files this walk may ask for if the one in flight
    /// fails. Counted down rather than up because the two walks start with
    /// different budgets: stepping has the rest of the list to try, while the
    /// walk that opens the first file has the whole of it.
    remaining: usize,
}

pub struct App {
    files: Vec<PathBuf>,
    index: usize,
    current: Option<Current>,
    overrides: decode::Overrides,
    startup: Startup,
    hdr: HdrPreference,
    view: View,
    /// The file on screen, watched for writes by anything else.
    watch: Watch,
    /// The colours everything is drawn in, and the palette file they came
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
    modifiers: ModifiersState,
    /// Physical window pixels, and the point a wheel zoom works about.
    cursor: Option<[f32; 2]>,
    /// Whether the left button is down, which is what a drag is.
    dragging: bool,
    /// Where the pointer was when the drag last moved the view. Held apart
    /// from `cursor` because a press can arrive before any motion has told us
    /// where the pointer is, and because leaving the window clears `cursor`
    /// without ending a drag the pointer grab is still delivering.
    drag_from: Option<[f32; 2]>,
    panels: Panels,
    /// Set if the last render failed, so we report it once rather than every frame.
    reported_error: bool,
    /// Numbers the requests. Only the newest one's reply is acted on.
    generation: u64,
    pending: Option<Pending>,
}

impl App {
    /// `size` is what the header of `files[index]` said, where it would say:
    /// enough to open the window at the right shape before the pixels exist.
    /// The file itself is asked for here, so that it is being read while the
    /// window and the GPU are still being set up.
    pub fn new(
        files: Vec<PathBuf>,
        index: usize,
        size: Option<[f32; 2]>,
        options: Options,
        loader: Loader,
    ) -> Self {
        let Options {
            overrides,
            startup,
            hdr,
            histogram,
            minimap,
            upscale,
        } = options;
        let watch = Watch::new(&files[index]);
        let theme_watch = theme::watch();
        let mut view = View::new();
        view.set_upscale(upscale);
        let count = files.len();
        let mut app = Self {
            files,
            index,
            current: None,
            header_size: size,
            overrides,
            startup,
            hdr,
            view,
            watch,
            theme: Theme::detect(),
            theme_watch,
            next_poll: Instant::now() + watch::INTERVAL,
            loader,
            window: None,
            renderer: None,
            modifiers: ModifiersState::empty(),
            cursor: None,
            dragging: false,
            drag_from: None,
            panels: Panels {
                show_ui: true,
                show_histogram: histogram,
                show_minimap: minimap,
                hover: None,
            },
            reported_error: false,
            generation: 0,
            pending: None,
        };
        // As a walk, so that a file which passes the header check and then
        // fails to decode is stepped over exactly as `n` would step over it.
        // Nothing is on screen yet, so every file in the list is a candidate.
        app.request(
            index,
            Reload::Fresh,
            Some(Step {
                forward: true,
                remaining: count - 1,
            }),
        );
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

    /// Where the panels are this frame. Cheap enough to derive on demand, and
    /// deriving it means there is no cached layout to fall out of step with
    /// the window.
    fn chrome(&self) -> Chrome {
        let scale = self.scale_factor();
        let physical = self.window_size();
        Chrome::new([physical[0] / scale, physical[1] / scale])
    }

    /// Where the image is drawn, in physical pixels: what the panels leave in
    /// the middle, or the whole window when they are hidden. Derived rather
    /// than stored, so toggling the interface re-fits a fitted image without
    /// anything having to remember to.
    fn viewport(&self) -> Viewport {
        image_viewport(self.window_size(), self.scale_factor(), self.panels.show_ui)
    }

    /// The pointer in logical pixels, which is what the interface is laid out
    /// in. Events arrive in physical ones.
    fn logical_cursor(&self) -> Option<[f32; 2]> {
        let scale = self.scale_factor();
        self.cursor
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
        let cursor = self.cursor?;
        let viewport = self.viewport();
        if !viewport.contains(cursor) {
            return None;
        }
        let image = self.current.as_ref()?.size();
        let point = self.view.placement(image, viewport).image_point(cursor);
        if point[0] < 0.0 || point[1] < 0.0 || point[0] >= image[0] || point[1] >= image[1] {
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
        self.panels.show_minimap && self.view.can_pan(self.image_size(), self.viewport())
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
    fn poll_file(&mut self) {
        // Not while a read is already in flight. A file being written
        // continuously would otherwise stack up a decode every interval, and
        // the reply already on its way carries a watch taken later than this
        // one anyway.
        if self.pending.is_none() && self.watch.poll() {
            self.request(self.index, Reload::InPlace, None);
        }
    }

    /// Notices that the desktop's theme has changed. Returns whether the
    /// window owes a redraw, which it does only when the new palette actually
    /// resolves to different colours.
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
            Some(_) => window_title(&self.files[self.index]),
            None => {
                let index = self.pending.as_ref().map_or(self.index, |p| p.index);
                loading_title(&self.files[index])
            }
        }
    }

    /// Asks the loader for `files[index]`.
    ///
    /// Nothing changes on screen here. The image already up stays where it is,
    /// still pannable and zoomable, until the reply arrives at
    /// [`App::user_event`] — which is the whole point of the exercise, and the
    /// reason everything the interface says about the image goes on describing
    /// the one being shown rather than the one being fetched.
    fn request(&mut self, index: usize, mode: Reload, step: Option<Step>) {
        self.generation += 1;
        self.pending = Some(Pending {
            generation: self.generation,
            index,
            step,
            since: Instant::now(),
            announced: false,
        });
        self.loader.request(Request {
            generation: self.generation,
            index,
            path: self.files[index].clone(),
            overrides: self.overrides,
            mode,
        });
        // With nothing on screen the title is the only thing naming the file,
        // so it follows the request rather than the pixels — including when a
        // walk moves on past one that would not decode.
        if self.current.is_none()
            && let Some(window) = &self.window
        {
            window.set_title(&self.title());
        }
    }

    /// Puts a finished read on screen. Returns `false` if the upload failed,
    /// which leaves the current image where it is.
    fn apply(&mut self, file: Opened, ready: Ready) -> bool {
        let Ready { image, stats, gpu } = ready;
        let size = [image.width as f32, image.height as f32];
        // Images of the same size are almost always a set to be compared —
        // frames of a sequence, or one exposure against another — and there
        // the point is that the same detail stays under the same pixels, so
        // the pan and zoom carry over. A file of another size is a new
        // picture, so it is fitted afresh.
        let same_size = self
            .current
            .as_ref()
            .is_some_and(|current| current.size() == size);
        // Re-reading the same file keeps the user where they were, since they
        // are watching one spot for the change: same exposure and tone map,
        // with only an automatic window re-derived from the new pixels.
        // Stepping to a different file is a different picture, and gets the
        // exposure its own pixels ask for.
        let in_place = file.mode == Reload::InPlace && same_size;
        let display = match self.current.as_ref().filter(|_| in_place) {
            Some(current) => {
                let mut display = current.display.clone();
                display.refresh_auto(&stats);
                display
            }
            None => Display::for_image_with(&image, &stats, self.startup),
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
                        eprintln!("image-view: {error:#}");
                        return false;
                    }
                },
            };
            if let Some(note) = renderer.install_image(uploaded) {
                eprintln!("image-view: {note}");
            }
            stored = renderer.image_format_label();
        }

        self.index = file.index;
        self.watch = file.watch;
        if !same_size {
            self.view.reset();
        }
        self.current = Some(Current {
            image,
            stats,
            display,
            label: file_label(&file.path),
            stored,
        });
        if let Some(window) = &self.window {
            window.set_title(&window_title(&file.path));
        }
        true
    }

    /// Moves to the next or previous file.
    ///
    /// From wherever the last request was aimed rather than from what is on
    /// screen, so that holding `n` walks the list instead of asking for the
    /// same neighbour over and over while a slow file opens. Only the last of
    /// those requests is decoded; the ones passed over are files the user has
    /// already scrolled past.
    fn step(&mut self, forward: bool) {
        if self.files.len() < 2 {
            return;
        }
        let from = self
            .pending
            .as_ref()
            .map_or(self.index, |pending| pending.index);
        let next = self.neighbour(from, forward);
        self.request(
            next,
            Reload::Fresh,
            Some(Step {
                forward,
                // Everything but the file being asked for and the one already
                // on screen.
                remaining: self.files.len() - 2,
            }),
        );
    }

    /// Carries a walk on past a file that would not decode, so that one bad
    /// file cannot trap navigation. Gives up once it has tried them all.
    fn step_again(&mut self, from: usize, step: Step) {
        if step.remaining == 0 {
            return;
        }
        let next = self.neighbour(from, step.forward);
        self.request(
            next,
            Reload::Fresh,
            Some(Step {
                forward: step.forward,
                remaining: step.remaining - 1,
            }),
        );
    }

    fn neighbour(&self, index: usize, forward: bool) -> usize {
        let count = self.files.len();
        if forward {
            (index + 1) % count
        } else {
            (index + count - 1) % count
        }
    }

    /// Decides whether a read still in flight has been going long enough to
    /// earn a word in the bar. Latches, so that a wait is announced once
    /// rather than on every frame it spans.
    fn announce_slow_read(&mut self, now: Instant) -> Announce {
        let Some(pending) = &mut self.pending else {
            return Announce::Nothing;
        };
        let due = pending.since + SLOW_READ;
        if now < due {
            Announce::Waiting(due)
        } else if pending.announced {
            Announce::Nothing
        } else {
            pending.announced = true;
            Announce::Now
        }
    }

    /// Takes in a file the loader has finished with. Held apart from the
    /// handler that receives it, since nothing here needs the event loop.
    fn deliver(&mut self, decoded: Decoded) {
        // Anything but the newest request is a file the user has stepped past
        // while it was being read. Its pixels are correct and unwanted.
        let Some(pending) = self
            .pending
            .take_if(|pending| pending.generation == decoded.generation)
        else {
            return;
        };

        let index = decoded.file.index;
        // A file that will not go on screen is a file to step over, whether it
        // was the decode or the upload that would not have it.
        let failed = match decoded.outcome {
            Ok(ready) => !self.apply(decoded.file, ready),
            Err(error) => {
                eprintln!("image-view: {error:#}");
                true
            }
        };
        if failed && let Some(step) = pending.step {
            self.step_again(index, step);
        }

        // Owed either way: on success for the new image, and on failure
        // because the bar may have been saying that a read was under way.
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    /// Returns `true` if the key changed anything on screen.
    fn handle_key(&mut self, event_loop: &ActiveEventLoop, key: &Key) -> bool {
        // Chords belong to the window manager, not to us: a compositor binding
        // such as Super+0 still delivers its key here, and acting on it would
        // move the view behind the user's back.
        if self.modifiers.control_key() || self.modifiers.alt_key() || self.modifiers.super_key() {
            return false;
        }

        let image = self.image_size();
        let viewport = self.viewport();

        match key {
            Key::Named(NamedKey::Escape) => {
                event_loop.exit();
                return false;
            }
            Key::Named(NamedKey::ArrowLeft) => self.view.pan_by(-PAN_STEP, 0.0, image, viewport),
            Key::Named(NamedKey::ArrowRight) => self.view.pan_by(PAN_STEP, 0.0, image, viewport),
            Key::Named(NamedKey::ArrowUp) => self.view.pan_by(0.0, -PAN_STEP, image, viewport),
            Key::Named(NamedKey::ArrowDown) => self.view.pan_by(0.0, PAN_STEP, image, viewport),
            // Nothing to draw yet: the file is only being asked for, and what
            // is on screen stays until it arrives.
            Key::Named(NamedKey::PageDown) => {
                self.step(true);
                return false;
            }
            Key::Named(NamedKey::PageUp) => {
                self.step(false);
                return false;
            }
            Key::Character(text) => {
                return self.handle_character(event_loop, text, image, viewport);
            }
            _ => return false,
        }
        true
    }

    fn handle_character(
        &mut self,
        event_loop: &ActiveEventLoop,
        text: &str,
        image: [f32; 2],
        viewport: Viewport,
    ) -> bool {
        // Everything below the view controls needs an image to act on.
        match text {
            "q" | "Q" => {
                event_loop.exit();
                return false;
            }
            "+" | "=" => {
                self.view.zoom_in(image, viewport);
                return true;
            }
            "-" | "_" => {
                self.view.zoom_out(image, viewport);
                return true;
            }
            "0" => {
                self.view.actual_size(image, viewport);
                return true;
            }
            "f" | "F" => {
                self.view.cycle_fit();
                return true;
            }
            "n" | "N" => {
                self.step(true);
                return false;
            }
            "p" | "P" => {
                self.step(false);
                return false;
            }
            "`" | "~" => {
                self.panels.show_ui = !self.panels.show_ui;
                // A fitted image re-fits on the next frame: the viewport it is
                // measured against is the one the panels leave, and they have
                // just come or gone.
                return true;
            }
            "h" | "H" => {
                self.panels.toggle(Widget::Histogram);
                return true;
            }
            "m" | "M" => {
                self.panels.toggle(Widget::Minimap);
                return true;
            }
            "u" | "U" => {
                self.view.cycle_upscale();
                return true;
            }
            _ => {}
        }

        let Some(current) = &mut self.current else {
            return false;
        };
        match text {
            "e" => current.display.adjust_exposure(-0.5),
            "E" => current.display.adjust_exposure(0.5),
            "a" | "A" => current.display.cycle_auto(&current.stats),
            "t" | "T" => current.display.cycle_tone_map(),
            "c" | "C" => {
                if !current.image.is_gray() {
                    return false;
                }
                current.display.cycle_colormap();
            }
            "[" => current.display.shift_window(-0.05),
            "]" => current.display.shift_window(0.05),
            "," | "<" => current.display.adjust_contrast(0.8),
            "." | ">" => current.display.adjust_contrast(1.25),
            "r" | "R" => current
                .display
                .reset(&current.stats, &current.image, self.startup),
            _ => return false,
        }
        true
    }

    /// Starts or ends a drag of the image with the left button. The pointer
    /// keeps its grab until the button comes back up, so a drag that leaves
    /// the window goes on working.
    ///
    /// A press does not need to know where the pointer is: the first motion
    /// after it establishes the point the drag measures from. Waiting for that
    /// costs nothing, and a press can genuinely arrive with no position yet —
    /// the pointer entering the window and clicking without moving.
    /// Returns `true` if the click changed anything on screen, which a press
    /// on a widget does and a press on the image does not.
    fn handle_button(&mut self, state: ElementState, button: MouseButton) -> bool {
        if button != MouseButton::Left {
            return false;
        }

        // The chrome gets first refusal. A press that lands on a panel is
        // aimed at the interface, so it neither reaches a widget's neighbour
        // nor starts a drag of the image underneath.
        if state == ElementState::Pressed
            && self.panels.show_ui
            && let Some(point) = self.logical_cursor()
        {
            let chrome = self.chrome();
            if let Some(widget) = chrome.widget_at(point) {
                self.panels.toggle(widget);
                return true;
            }
            if chrome.contains(point) {
                return false;
            }
        }

        self.dragging = state == ElementState::Pressed;
        self.drag_from = if self.dragging { self.cursor } else { None };

        if let Some(window) = &self.window {
            // The closed hand is a promise that dragging will move something,
            // so a fitted image — which has nowhere to go — does not make it.
            let icon = if self.dragging && self.view.can_pan(self.image_size(), self.viewport()) {
                CursorIcon::Grabbing
            } else {
                CursorIcon::Default
            };
            window.set_cursor(Cursor::Icon(icon));
        }
        false
    }

    /// Follows the pointer. Returns `true` if the frame is now out of date —
    /// because a drag moved the view, or because the bar is reporting a pixel
    /// the pointer has since left.
    fn handle_motion(&mut self, position: [f32; 2]) -> bool {
        let was_over = self.pointer_pixel();
        self.cursor = Some(position);
        let moved_pixel = self.panels.show_ui && self.pointer_pixel() != was_over;
        if !self.dragging {
            // Nothing else to do out here, so this is where the button's
            // highlight gets to follow the pointer.
            return self.update_hover() || moved_pixel;
        }
        let Some(from) = self.drag_from.replace(position) else {
            // First motion of this drag: nothing to measure from yet.
            return false;
        };
        // The image follows the pointer, so the viewport moves the other way.
        let (dx, dy) = (from[0] - position[0], from[1] - position[1]);
        if dx == 0.0 && dy == 0.0 {
            return false;
        }
        self.view.pan_by(dx, dy, self.image_size(), self.viewport());
        true
    }

    /// Re-tests the pointer against the widgets. Returns `true` if the
    /// highlight moved, and so if the frame is now out of date.
    fn update_hover(&mut self) -> bool {
        let hover = self
            .logical_cursor()
            .filter(|_| self.panels.show_ui)
            .and_then(|point| self.chrome().widget_at(point));
        let changed = hover != self.panels.hover;
        self.panels.hover = hover;
        changed
    }

    /// Returns `true` if the wheel changed anything on screen.
    fn handle_wheel(&mut self, delta: MouseScrollDelta) -> bool {
        // Same reasoning as `handle_key`: Ctrl+wheel and friends belong to the
        // compositor, and acting on them as well would zoom behind its back.
        if self.modifiers.control_key() || self.modifiers.alt_key() || self.modifiers.super_key() {
            return false;
        }

        let steps = match delta {
            MouseScrollDelta::LineDelta(_, lines) => lines,
            MouseScrollDelta::PixelDelta(pixels) => pixels.y as f32 / WHEEL_PIXELS_PER_STEP,
        };
        // A trackpad emits a long tail of all but motionless events at the end
        // of a gesture, which would leave the view drifting after the finger
        // has stopped.
        if !steps.is_finite() || steps.abs() < 1e-3 {
            return false;
        }

        let viewport = self.viewport();
        let anchor = self.cursor.unwrap_or([
            viewport.x + viewport.width / 2.0,
            viewport.y + viewport.height / 2.0,
        ]);
        self.view
            .zoom_steps_at(steps, anchor, self.image_size(), viewport);
        true
    }

    fn redraw(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        if self.renderer.is_none() {
            return;
        }

        let scale = window.scale_factor() as f32;
        let physical = self.window_size();
        let logical = [physical[0] / scale, physical[1] / scale];
        let viewport = self.viewport();
        let placement = self.view.placement(self.image_size(), viewport);

        let pointer = self.pointer_pixel();
        let thumbnail = self.minimap_placement(logical, scale);
        let minimap = self.minimap_on_screen();
        let reading = self
            .pending
            .as_ref()
            // With nothing on screen there is no flicker to guard against and
            // nothing else to say, so the wait is worth naming immediately.
            .filter(|pending| pending.announced || self.current.is_none())
            .map(|pending| {
                if self.current.is_some() && pending.index == self.index {
                    Reading::Again
                } else {
                    Reading::File(file_label(&self.files[pending.index]))
                }
            });
        let hdr_output = {
            let output = self.renderer.as_ref().expect("checked above").output();
            output.is_hdr.then_some(output.label)
        };
        let input = FrameInput {
            logical,
            scale,
            viewport,
            pointer,
            minimap_on_screen: minimap,
            reading,
            index: self.index,
            count: self.files.len(),
            hdr_output,
        };

        // Split borrow: the frame builder needs the renderer's font metrics
        // while reading the rest of the application state.
        let renderer = self.renderer.as_mut().expect("checked above");
        let frame = ui::build_frame(
            renderer,
            &input,
            &self.panels,
            self.current.as_ref(),
            &self.view,
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
        };
        match renderer.render(scene) {
            Ok(()) => self.reported_error = false,
            Err(error) => {
                if !self.reported_error {
                    eprintln!("image-view: {error:#}");
                    self.reported_error = true;
                }
            }
        }
    }
}

impl ApplicationHandler<Decoded> for App {
    /// Look at the file, then sleep until it is time to look again rather than
    /// until the next event: nothing tells us about a write, so we go and ask.
    ///
    /// The deadline is a fixed cadence rather than an interval from here, so
    /// that a stream of events — a drag, a resize — cannot keep pushing the
    /// next look out of reach.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if now >= self.next_poll {
            self.next_poll = now + watch::INTERVAL;
            self.poll_file();
            if self.poll_theme()
                && let Some(window) = &self.window
            {
                window.request_redraw();
            }
        }

        // Sleep until the next thing with a time on it: the file check, or the
        // moment a read that is still going becomes worth mentioning. A read
        // that finishes first wakes us through the proxy instead.
        let mut deadline = self.next_poll;
        match self.announce_slow_read(now) {
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

    /// A file the loader has finished with.
    fn user_event(&mut self, event_loop: &ActiveEventLoop, decoded: Decoded) {
        self.deliver(decoded);
        // Nothing ever reached the screen and nothing else is coming: every
        // file named on the command line failed to decode. Stop, rather than
        // sit in an empty window with nothing on the way.
        if self.current.is_none() && self.pending.is_none() {
            event_loop.exit();
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let size = initial_window_size(event_loop, self.opening_size());
        let attributes = Window::default_attributes()
            .with_title(self.title())
            .with_inner_size(size);

        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                eprintln!("image-view: could not open a window: {error}");
                event_loop.exit();
                return;
            }
        };
        timing::window_open();

        let mut renderer = match Renderer::new(window.clone(), self.hdr) {
            Ok(renderer) => renderer,
            Err(error) => {
                eprintln!("image-view: {error:#}");
                event_loop.exit();
                return;
            }
        };

        if let Some(current) = &mut self.current {
            match renderer.set_image(&current.image) {
                Ok(note) => {
                    if let Some(note) = note {
                        eprintln!("image-view: {note}");
                    }
                    current.stored = renderer.image_format_label();
                }
                Err(error) => {
                    eprintln!("image-view: {error:#}");
                    event_loop.exit();
                    return;
                }
            }
        }

        if self.hdr == HdrPreference::On {
            let output = renderer.output();
            eprintln!(
                "image-view: {} \u{2192} {} output{}",
                renderer.adapter_name(),
                output.label,
                if output.is_hdr {
                    ""
                } else {
                    " (no HDR colour space offered for this surface)"
                }
            );
        }

        // From here on the loader uploads as well as decodes, so that
        // stepping to the next file costs the event loop nothing but the swap.
        self.loader.attach(renderer.uploader());
        self.renderer = Some(renderer);
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::CursorMoved { position, .. } => {
                if self.handle_motion([position.x as f32, position.y as f32])
                    && let Some(window) = &self.window
                {
                    window.request_redraw();
                }
            }
            WindowEvent::CursorLeft { .. } => {
                let was_over = self.pointer_pixel().is_some();
                self.cursor = None;
                if (self.update_hover() || was_over)
                    && let Some(window) = &self.window
                {
                    window.request_redraw();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if self.handle_button(state, button)
                    && let Some(window) = &self.window
                {
                    window.request_redraw();
                }
            }
            // A drag the window did not see end — the button came up over
            // another window, say — would otherwise resume on the next motion.
            WindowEvent::Focused(false) => {
                let _ = self.handle_button(ElementState::Released, MouseButton::Left);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if self.handle_wheel(delta)
                    && let Some(window) = &self.window
                {
                    window.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key,
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                if self.handle_key(event_loop, &logical_key)
                    && let Some(window) = &self.window
                {
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn window_title(path: &Path) -> String {
    format!("{} — image-view", file_label(path))
}

/// Before there is anything to look at, the title carries the file being read.
/// Titling an empty window with a file it is not yet showing would be saying
/// something untrue, and the title is the only place the name can go.
fn loading_title(path: &Path) -> String {
    format!("loading {} — image-view", file_label(path))
}

/// Open at the image's own size, shrunk to fit comfortably on the monitor.
///
/// The panels take their room out of the image rather than lying over it, so
/// the window asks for the image *plus* the chrome around it — otherwise a
/// picture that used to open at 100% would open slightly reduced. The monitor
/// fraction still applies to the image itself.
fn initial_window_size(event_loop: &ActiveEventLoop, image: Option<[f32; 2]>) -> PhysicalSize<u32> {
    let monitor = event_loop
        .primary_monitor()
        .or_else(|| event_loop.available_monitors().next());
    let scale = monitor
        .as_ref()
        .map_or(1.0, |monitor| monitor.scale_factor());
    let chrome = [
        2.0 * SIDE_WIDTH as f64 * scale,
        2.0 * BAR_HEIGHT as f64 * scale,
    ];

    // Only a file whose header would not say how large it is arrives here
    // with nothing, and then a plain rectangle is the best that can be done.
    let image = image.unwrap_or(DEFAULT_IMAGE);
    let (mut width, mut height) = (image[0] as f64, image[1] as f64);

    if let Some(monitor) = monitor {
        let available = monitor.size();
        let max_width = available.width as f64 * MAX_WINDOW_FRACTION - chrome[0];
        let max_height = available.height as f64 * MAX_WINDOW_FRACTION - chrome[1];
        if max_width > 1.0 && max_height > 1.0 {
            let shrink = (max_width / width).min(max_height / height).min(1.0);
            width *= shrink;
            height *= shrink;
        }
    }

    PhysicalSize::new(
        ((width + chrome[0]).round() as u32).max(320),
        ((height + chrome[1]).round() as u32).max(240),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::Stats;

    const WINDOW: [f32; 2] = [1000.0, 700.0];
    /// The same window with nothing taken out of it, for the tests that are
    /// about stepping between files rather than about where the panels are.
    const VIEWPORT: Viewport = Viewport::whole(WINDOW);

    /// A grey PNG of the given size, written where the test can step onto it.
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
        let dir = std::env::temp_dir().join(format!("image-view-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the temporary directory is writable");
        let paths: Vec<PathBuf> = files
            .iter()
            .map(|&(name, width, height)| write_png(&dir, name, width, height))
            .collect();
        let options = Options {
            overrides: decode::Overrides::default(),
            startup: Startup::default(),
            hdr: HdrPreference::default(),
            histogram: false,
            minimap: false,
            upscale: Upscale::default(),
        };
        let size = decode::probe(&paths[0]).expect("we just wrote it");
        let app = App::new(
            paths,
            0,
            size.map(|(w, h)| [w as f32, h as f32]),
            options,
            Loader::detached(),
        );
        (app, dir)
    }

    /// As [`opening`], with the application's own opening request answered:
    /// the state the tests about later behaviour want to start from.
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
        let pending = app.pending.as_ref().expect("a request is in flight");
        let (generation, index) = (pending.generation, pending.index);
        let path = app.files[index].clone();
        let watch = Watch::new(&path);
        let outcome = decode::load(&path, app.overrides).map(|image| Ready {
            stats: Stats::scan(&image),
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

    /// Stepping between frames of the same size is a comparison — the same
    /// detail has to stay under the same pixels, or there is nothing to
    /// compare.
    #[test]
    fn stepping_to_an_image_of_the_same_size_keeps_the_view() {
        let (mut app, dir) = app_over("same", &[("a.png", 64, 48), ("b.png", 64, 48)]);
        app.view.actual_size(app.image_size(), VIEWPORT);
        app.view.zoom_in(app.image_size(), VIEWPORT);
        let zoom = app.view.zoom(app.image_size(), VIEWPORT);

        app.step(true);
        // Nothing has moved yet: the file has only been asked for.
        assert_eq!(app.index, 0);

        answer(&mut app, Reload::Fresh);
        assert_eq!(app.index, 1);
        assert_eq!(app.view.mode_label(), "free");
        assert_eq!(app.view.zoom(app.image_size(), VIEWPORT), zoom);

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// Holding `n` through a directory asks for each file in turn without
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
            .pending
            .as_ref()
            .expect("a request is in flight")
            .generation;
        app.step(true);
        assert_eq!(app.pending.as_ref().map(|pending| pending.index), Some(2));

        // The first file arrives late, after the user has moved past it.
        let path = app.files[1].clone();
        let image = decode::load(&path, app.overrides).expect("we just wrote it");
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
                image,
                gpu: None,
            }),
        });
        assert_eq!(app.index, 0, "an overtaken file must not reach the screen");

        // The one actually waited for still lands.
        answer(&mut app, Reload::Fresh);
        assert_eq!(app.index, 2);

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// A file whose header reads cleanly and whose pixels do not gets past
    /// the check that happens before the window opens. Opening asks for the
    /// first file as a walk for exactly that reason, so start-up steps over it
    /// as `n` would step over it later.
    #[test]
    fn a_first_file_that_will_not_decode_is_stepped_over() {
        let (mut app, dir) = opening(
            "first-broken",
            &[("a.png", 64, 48), ("b.png", 32, 32), ("c.png", 16, 16)],
        );
        corrupt(&app.files[0]);

        assert_eq!(app.pending.as_ref().map(|p| p.index), Some(0));
        answer(&mut app, Reload::Fresh);
        assert!(app.current.is_none(), "nothing can be shown yet");
        assert_eq!(
            app.pending.as_ref().map(|p| p.index),
            Some(1),
            "the walk carries on to the next file"
        );

        answer(&mut app, Reload::Fresh);
        assert_eq!(app.index, 1);
        assert!(!app.showed_nothing());

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// When none of them decode there is nothing to look at, and the caller
    /// needs to know so it can leave with a failing status rather than sit in
    /// an empty window.
    #[test]
    fn nothing_decoding_at_all_is_reported_as_having_shown_nothing() {
        let (mut app, dir) = opening("all-broken", &[("a.png", 64, 48), ("b.png", 32, 32)]);
        for path in &app.files {
            corrupt(path);
        }

        for _ in 0..app.files.len() {
            if app.pending.is_none() {
                break;
            }
            answer(&mut app, Reload::Fresh);
        }
        assert!(app.pending.is_none(), "the walk has to stop asking");
        assert!(app.showed_nothing());

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// While nothing is on screen the title names the file being read, and it
    /// follows the walk rather than staying on a file that would not open.
    #[test]
    fn the_title_names_the_file_being_read_until_there_is_one_to_show() {
        let (mut app, dir) = opening("title", &[("a.png", 64, 48), ("b.png", 32, 32)]);
        corrupt(&app.files[0]);

        assert_eq!(app.title(), "loading a.png — image-view");
        answer(&mut app, Reload::Fresh);
        assert_eq!(app.title(), "loading b.png — image-view");
        answer(&mut app, Reload::Fresh);
        assert_eq!(app.title(), "b.png — image-view");

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
        std::fs::write(&app.files[1], b"not a png at all").expect("the file is writable");

        app.step(true);
        answer(&mut app, Reload::Fresh);
        assert_eq!(app.index, 0, "the broken file cannot be shown");
        assert_eq!(
            app.pending.as_ref().map(|pending| pending.index),
            Some(2),
            "and the walk carries on past it"
        );

        answer(&mut app, Reload::Fresh);
        assert_eq!(app.index, 2);

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
        for path in &app.files[1..] {
            std::fs::write(path, b"not a png at all").expect("the file is writable");
        }

        app.step(true);
        for _ in 0..app.files.len() {
            if app.pending.is_none() {
                break;
            }
            answer(&mut app, Reload::Fresh);
        }
        assert!(app.pending.is_none(), "the walk has to stop asking");
        assert_eq!(app.index, 0);

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// A file that opens between two frames must not flicker a word into the
    /// interface and out again; one that keeps the user waiting has to say so,
    /// and say it once.
    #[test]
    fn only_a_read_that_keeps_the_user_waiting_is_announced() {
        let (mut app, dir) = app_over("slow", &[("a.png", 64, 48), ("b.png", 32, 32)]);
        let start = Instant::now();

        assert_eq!(app.announce_slow_read(start), Announce::Nothing);

        app.step(true);
        let since = app.pending.as_ref().expect("a request is in flight").since;
        assert_eq!(
            app.announce_slow_read(since),
            Announce::Waiting(since + SLOW_READ),
            "a read that has just started is not worth mentioning yet"
        );

        assert_eq!(app.announce_slow_read(since + SLOW_READ), Announce::Now);
        assert_eq!(
            app.announce_slow_read(since + SLOW_READ * 2),
            Announce::Nothing,
            "and having been said once it is not said again"
        );

        answer(&mut app, Reload::Fresh);
        assert_eq!(
            app.announce_slow_read(since + SLOW_READ * 2),
            Announce::Nothing
        );

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// A file of another size is another picture, and gets the opening view.
    #[test]
    fn stepping_to_an_image_of_another_size_fits_it() {
        let (mut app, dir) = app_over("other", &[("a.png", 64, 48), ("b.png", 32, 32)]);
        app.view.actual_size(app.image_size(), VIEWPORT);
        app.view.zoom_in(app.image_size(), VIEWPORT);

        app.step(true);
        answer(&mut app, Reload::Fresh);
        assert_eq!(app.index, 1);
        assert_eq!(app.view.mode_label(), "fit");

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }
}
