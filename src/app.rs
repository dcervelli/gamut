//! Window lifecycle, key handling, and building each frame's interface.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Cursor, CursorIcon, Window, WindowId};

use crate::image::display::{AutoWindow, Colormap, Display, Startup};
use crate::image::stats::{BINS, ChannelStats};
use crate::image::{DecodedImage, Stats, decode};
use crate::render::{Color, HdrPreference, Rect, Renderer, UiFrame};
use crate::view::{Upscale, View, Viewport};
use crate::watch::{self, Watch};

/// Window pixels moved per arrow-key press.
const PAN_STEP: f32 = 64.0;
/// Trackpad pixels that add up to one notch of the wheel. Wheels report whole
/// lines and need no conversion; a trackpad reports the scroll it would have
/// done, and this is what turns that into the same zoom increment.
const WHEEL_PIXELS_PER_STEP: f32 = 50.0;
/// Fraction of the monitor a freshly opened window may occupy.
const MAX_WINDOW_FRACTION: f64 = 0.85;

/// Height of the top and bottom panels.
const BAR_HEIGHT: f32 = 30.0;
/// Width of the left and right panels. Wide enough for a square button and
/// nothing else, which is the point: they hold tools, not content.
const SIDE_WIDTH: f32 = 50.0;
/// The square buttons that live in the side panels.
const BUTTON_SIZE: f32 = 34.0;
const TEXT_SIZE: f32 = 13.0;
const PADDING: f32 = 12.0;
/// The gap between the histogram panel's edge and its plot.
const HISTOGRAM_INSET: f32 = 10.0;
/// Wide enough that a bin is exactly one logical pixel, which is what keeps
/// the bars evenly spaced instead of some of them landing astride a pixel
/// boundary and coming out fatter than their neighbours.
const HISTOGRAM_SIZE: [f32; 2] = [BINS as f32 + 2.0 * HISTOGRAM_INSET, 130.0];

/// The panels are opaque, not a tint over the image: the image is fitted
/// inside them rather than passing behind them, so there is nothing back
/// there to show through.
const BAR_BACKGROUND: Color = Color::rgb(18, 18, 22);
const PANEL_BACKGROUND: Color = Color::rgba(12, 12, 16, 214);
const BUTTON_IDLE: Color = Color::rgba(255, 255, 255, 20);
const BUTTON_HOVER: Color = Color::rgba(255, 255, 255, 45);
const TEXT_PRIMARY: Color = Color::rgb(238, 238, 238);
const TEXT_DIM: Color = Color::rgb(150, 152, 160);
const ACCENT: Color = Color::rgb(120, 180, 255);
// Histogram ink. The colour planes are translucent so that overlapping bars
// read as a blend rather than as whichever happened to be drawn last, and the
// luminance plane sits under them in the neutral the rest of the panel uses.
const HISTOGRAM_LUMA: Color = Color::rgba(150, 152, 160, 200);
/// Red, green and blue, in the order the channels are stored.
const COLOUR_PLANES: usize = 3;
const HISTOGRAM_PLANES: [Color; COLOUR_PLANES] = [
    Color::rgba(255, 96, 88, 170),
    Color::rgba(88, 220, 120, 170),
    Color::rgba(96, 150, 255, 170),
];

/// The window chrome: four panels, and the widgets sitting in them.
///
/// Top and bottom span the full width; left and right are nested between
/// them, so the corners belong to the horizontal bars and the vertical ones
/// never have to reason about where a bar ends.
///
/// Laid out from the window size alone, so the frame builder and the click
/// handler agree on where everything is without either owning it.
#[derive(Clone, Copy)]
struct Chrome {
    top: Rect,
    bottom: Rect,
    left: Rect,
    right: Rect,
    /// The histogram toggle, at the top of the right panel.
    histogram_button: Rect,
}

impl Chrome {
    /// `size` is the window in logical pixels.
    fn new(size: [f32; 2]) -> Self {
        // Half the window each at the very smallest, so that a window dragged
        // down to nothing shrinks the panels rather than letting the opposite
        // pair pass through each other.
        let bar = BAR_HEIGHT.min(size[1] / 2.0);
        let side = SIDE_WIDTH.min(size[0] / 2.0);
        let middle = (size[1] - 2.0 * bar).max(0.0);

        let right = Rect::new(size[0] - side, bar, side, middle);
        // The same inset on all four sides, so the button reads as centred in
        // the strip rather than merely fitted into it — until the strip is
        // shorter than that, at which point it goes flush to the top.
        let button = BUTTON_SIZE.min(right.width).min(right.height);
        let inset = (right.width - button) / 2.0;

        Self {
            top: Rect::new(0.0, 0.0, size[0], bar),
            bottom: Rect::new(0.0, size[1] - bar, size[0], bar),
            left: Rect::new(0.0, bar, side, middle),
            right,
            histogram_button: Rect::new(
                right.x + inset,
                right.y + inset.min(right.height - button),
                button,
                button,
            ),
        }
    }

    /// What the four panels leave in the middle: the image is drawn in it,
    /// and anything that floats over the image — the histogram, for now — has
    /// to fit in it.
    fn content(&self) -> Rect {
        Rect::new(
            self.left.right(),
            self.top.bottom(),
            (self.right.x - self.left.right()).max(0.0),
            (self.bottom.y - self.top.bottom()).max(0.0),
        )
    }

    /// Whether a click at `point` belongs to the interface rather than to the
    /// image behind it.
    fn contains(&self, point: [f32; 2]) -> bool {
        self.top.contains(point)
            || self.bottom.contains(point)
            || self.left.contains(point)
            || self.right.contains(point)
    }
}

/// What the command line asked for, beyond which files to show.
pub struct Options {
    pub overrides: decode::Overrides,
    pub startup: Startup,
    pub hdr: HdrPreference,
    pub histogram: bool,
    pub upscale: Upscale,
}

/// The image on screen, with everything derived from it.
struct Current {
    image: DecodedImage,
    stats: Stats,
    display: Display,
    label: String,
    /// What the GPU actually stored it as, which is not always what we asked.
    format: Option<wgpu::TextureFormat>,
}

impl Current {
    fn size(&self) -> [f32; 2] {
        [self.image.width as f32, self.image.height as f32]
    }
}

/// Why a file is being read, which decides what survives the reading.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reload {
    /// A different file: display settings start over, and so does the view
    /// unless the new file happens to be the same size as the old one.
    Fresh,
    /// The file already on screen, changed on disk. The user is presumably
    /// looking at something in particular, so what they set up stays.
    InPlace,
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
    /// When to look at it next.
    next_poll: Instant,
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
    /// Whether the four panels are on screen. They are opaque and the image
    /// is fitted inside them, so hiding them gives it the whole window.
    show_ui: bool,
    show_histogram: bool,
    /// Whether the pointer is over the histogram toggle. Held rather than
    /// recomputed while drawing so that motion knows when the highlight has
    /// changed and a redraw is actually owed.
    hover_histogram: bool,
    /// Set if the last render failed, so we report it once rather than every frame.
    reported_error: bool,
}

impl App {
    /// `first` is `files[index]`, already decoded before the window opened so
    /// that a bad path fails on the command line.
    pub fn new(files: Vec<PathBuf>, index: usize, first: DecodedImage, options: Options) -> Self {
        let Options {
            overrides,
            startup,
            hdr,
            histogram,
            upscale,
        } = options;
        let stats = Stats::scan(&first);
        let display = Display::for_image_with(&first, &stats, startup);
        let label = file_label(&files[index]);
        let watch = Watch::new(&files[index]);
        let mut view = View::new();
        view.set_upscale(upscale);
        Self {
            files,
            index,
            current: Some(Current {
                image: first,
                stats,
                display,
                label,
                format: None,
            }),
            overrides,
            startup,
            hdr,
            view,
            watch,
            next_poll: Instant::now() + watch::INTERVAL,
            window: None,
            renderer: None,
            modifiers: ModifiersState::empty(),
            cursor: None,
            dragging: false,
            drag_from: None,
            show_ui: true,
            show_histogram: histogram,
            hover_histogram: false,
            reported_error: false,
        }
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
        image_viewport(self.window_size(), self.scale_factor(), self.show_ui)
    }

    /// The pointer in logical pixels, which is what the interface is laid out
    /// in. Events arrive in physical ones.
    fn logical_cursor(&self) -> Option<[f32; 2]> {
        let scale = self.scale_factor();
        self.cursor
            .map(|cursor| [cursor[0] / scale, cursor[1] / scale])
    }

    /// Re-reads the file on screen if something else has written to it, which
    /// is what makes this usable next to whatever produced the image. Returns
    /// `true` if the screen needs drawing again.
    fn poll_file(&mut self) -> bool {
        self.watch.poll() && self.load(self.index, Reload::InPlace)
    }

    /// Reads `files[index]` and puts it on screen. Returns `false` if it could
    /// not be decoded, leaving the current image where it is: a file caught
    /// mid-write is a failure we expect and one the next poll clears up.
    fn load(&mut self, index: usize, mode: Reload) -> bool {
        let path = &self.files[index];
        // Taken before the read rather than after it: a write that lands while
        // we are decoding then shows up as another change, instead of being
        // recorded as the version we are holding.
        let watch = Watch::new(path);
        let image = match decode::load(path, self.overrides) {
            Ok(image) => image,
            Err(error) => {
                eprintln!("image-view: {error:#}");
                return false;
            }
        };

        let stats = Stats::scan(&image);
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
        let in_place = mode == Reload::InPlace && same_size;
        let display = match self.current.as_ref().filter(|_| in_place) {
            Some(current) => {
                let mut display = current.display.clone();
                display.refresh_auto(&stats);
                display
            }
            None => Display::for_image_with(&image, &stats, self.startup),
        };
        self.index = index;
        self.watch = watch;
        if !same_size {
            self.view.reset();
        }

        let mut format = None;
        if let Some(renderer) = &mut self.renderer {
            match renderer.set_image(&image) {
                Ok(note) => {
                    if let Some(note) = note {
                        eprintln!("image-view: {note}");
                    }
                    format = renderer.image_format();
                }
                Err(error) => {
                    eprintln!("image-view: {error:#}");
                    return false;
                }
            }
        }

        self.current = Some(Current {
            image,
            stats,
            display,
            label: file_label(path),
            format,
        });
        if let Some(window) = &self.window {
            window.set_title(&window_title(path));
        }
        true
    }

    /// Moves to the next or previous file, stepping over any that fail to
    /// decode so that one bad file cannot trap navigation.
    fn step(&mut self, forward: bool) {
        let count = self.files.len();
        if count < 2 {
            return;
        }
        let mut index = self.index;
        for _ in 0..count - 1 {
            index = if forward {
                (index + 1) % count
            } else {
                (index + count - 1) % count
            };
            if self.load(index, Reload::Fresh) {
                return;
            }
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
            Key::Named(NamedKey::PageDown) => self.step(true),
            Key::Named(NamedKey::PageUp) => self.step(false),
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
                return true;
            }
            "p" | "P" => {
                self.step(false);
                return true;
            }
            "`" | "~" => {
                self.show_ui = !self.show_ui;
                // A fitted image re-fits on the next frame: the viewport it is
                // measured against is the one the panels leave, and they have
                // just come or gone.
                return true;
            }
            "h" | "H" => {
                self.show_histogram = !self.show_histogram;
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
            && self.show_ui
            && let Some(point) = self.logical_cursor()
        {
            let chrome = self.chrome();
            if chrome.histogram_button.contains(point) {
                self.show_histogram = !self.show_histogram;
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
            let icon = if self.dragging && self.view.can_pan(self.image_size(), self.viewport())
            {
                CursorIcon::Grabbing
            } else {
                CursorIcon::Default
            };
            window.set_cursor(Cursor::Icon(icon));
        }
        false
    }

    /// Follows the pointer. Returns `true` if a drag moved the view.
    fn handle_motion(&mut self, position: [f32; 2]) -> bool {
        self.cursor = Some(position);
        if !self.dragging {
            // Nothing else to do out here, so this is where the button's
            // highlight gets to follow the pointer.
            return self.update_hover();
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
        self.view
            .pan_by(dx, dy, self.image_size(), self.viewport());
        true
    }

    /// Re-tests the pointer against the widgets. Returns `true` if the
    /// highlight moved, and so if the frame is now out of date.
    fn update_hover(&mut self) -> bool {
        let hover = self.show_ui
            && self
                .logical_cursor()
                .is_some_and(|point| self.chrome().histogram_button.contains(point));
        let changed = hover != self.hover_histogram;
        self.hover_histogram = hover;
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

        // Split borrow: the frame builder needs the renderer's font metrics
        // while reading the rest of the application state.
        let renderer = self.renderer.as_mut().expect("checked above");
        let frame = build_ui(
            renderer,
            Layout {
                logical,
                viewport,
                index: self.index,
                file_count: self.files.len(),
                show_ui: self.show_ui,
                show_histogram: self.show_histogram,
                hover_histogram: self.hover_histogram,
            },
            self.current.as_ref(),
            &self.view,
        );

        let fallback = Display::default();
        let display = self
            .current
            .as_ref()
            .map(|current| &current.display)
            .unwrap_or(&fallback);

        match renderer.render(placement, display, &frame, scale) {
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

impl ApplicationHandler for App {
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
            if self.poll_file()
                && let Some(window) = &self.window
            {
                window.request_redraw();
            }
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_poll));
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let size = initial_window_size(event_loop, self.image_size());
        let attributes = Window::default_attributes()
            .with_title(window_title(&self.files[self.index]))
            .with_inner_size(size);

        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                eprintln!("image-view: could not open a window: {error}");
                event_loop.exit();
                return;
            }
        };

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
                    current.format = renderer.image_format();
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
                self.cursor = None;
                if self.update_hover()
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

/// Everything the frame builder needs that is not the image itself.
struct Layout {
    /// Window size in logical pixels, which is what the UI lays out in.
    logical: [f32; 2],
    /// Where the image is drawn, which is what zoom is measured against.
    viewport: Viewport,
    index: usize,
    file_count: usize,
    show_ui: bool,
    show_histogram: bool,
    hover_histogram: bool,
}

/// Builds one frame of interface.
///
/// A free function taking exactly what it needs, rather than a method, so that
/// it can measure text through the renderer while reading application state.
fn build_ui(
    renderer: &mut Renderer,
    layout: Layout,
    current: Option<&Current>,
    view: &View,
) -> UiFrame {
    let size = layout.logical;
    let mut frame = UiFrame::new();
    let Some(current) = current else {
        return frame;
    };

    let chrome = Chrome::new(size);
    // With the panels hidden the whole window is the content area, so a
    // histogram on its own still sits in the corner rather than where the
    // panels that are not there would have put it.
    let content = if layout.show_ui {
        chrome.content()
    } else {
        Rect::new(0.0, 0.0, size[0], size[1])
    };

    if layout.show_histogram {
        draw_histogram(&mut frame, current, content);
    }
    if !layout.show_ui {
        return frame;
    }

    for panel in [chrome.top, chrome.bottom, chrome.left, chrome.right] {
        frame.rect(panel, BAR_BACKGROUND);
    }

    // Top panel: what is on screen.
    frame.text_clipped(
        [PADDING, text_baseline(chrome.top)],
        TEXT_SIZE,
        TEXT_PRIMARY,
        (size[0] - PADDING * 2.0).max(1.0),
        current.label.clone(),
    );

    draw_histogram_button(
        &mut frame,
        chrome.histogram_button,
        layout.show_histogram,
        layout.hover_histogram,
    );

    let bar = chrome.bottom;
    let baseline = text_baseline(bar);

    let mut right = describe_state(current, view, &layout);
    if renderer.output().is_hdr {
        right = format!("{}   \u{00b7}   {}", renderer.output().label, right);
    }
    let right_width = renderer.measure_text(&right, TEXT_SIZE)[0];
    let right_x = (bar.right() - PADDING - right_width).max(PADDING);

    // Least to most disposable. Rather than clip whatever happens to overflow
    // — which is how "18333 x 15667" becomes "18333" — drop whole facts from
    // the end until what is left fits.
    let left = fit_segments(
        renderer,
        &[
            format!("{} \u{00d7} {}", current.image.width, current.image.height),
            describe_pixels(current),
            current.image.color.label(),
        ],
        (right_x - PADDING * 2.0).max(1.0),
    );

    frame.text_clipped(
        [PADDING, baseline],
        TEXT_SIZE,
        TEXT_PRIMARY,
        (right_x - PADDING * 2.0).max(1.0),
        left,
    );
    frame.text([right_x, baseline], TEXT_SIZE, TEXT_DIM, right);
    frame
}

/// Where the image is drawn, in physical pixels, for a window of `size`
/// physical pixels at `scale`.
///
/// The panels are opaque, so with them on screen the image belongs in what
/// they leave in the middle; with them off it has the window. Nothing caches
/// this, which is why toggling the interface re-fits a fitted image on the
/// very next frame.
fn image_viewport(size: [f32; 2], scale: f32, show_ui: bool) -> Viewport {
    if !show_ui {
        return Viewport::whole(size);
    }
    let content = Chrome::new([size[0] / scale, size[1] / scale]).content();
    Viewport::new(
        content.x * scale,
        content.y * scale,
        content.width * scale,
        content.height * scale,
    )
}

/// Where text has to start to sit centred in a bar of `BAR_HEIGHT`.
fn text_baseline(bar: Rect) -> f32 {
    bar.y + (bar.height - TEXT_SIZE * 1.3) / 2.0
}

/// The histogram toggle: a miniature of what it shows, rather than a letter,
/// since the side panels are too narrow to label anything in words.
fn draw_histogram_button(frame: &mut UiFrame, rect: Rect, active: bool, hover: bool) {
    // A window too small to hold the button gets no button, rather than a
    // smear of sub-pixel bars.
    if rect.width < BUTTON_SIZE {
        return;
    }
    let (background, ink) = match (active, hover) {
        (true, _) => (ACCENT.with_alpha(64), ACCENT),
        (false, true) => (BUTTON_HOVER, TEXT_PRIMARY),
        (false, false) => (BUTTON_IDLE, TEXT_DIM),
    };
    frame.rounded_rect(rect, 5.0, background);

    const BARS: [f32; 4] = [0.45, 1.0, 0.7, 0.3];
    let plot = rect.inset(9.0, 9.0);
    let step = plot.width / BARS.len() as f32;
    for (index, fraction) in BARS.iter().enumerate() {
        let height = plot.height * fraction;
        frame.rect(
            Rect::new(
                plot.x + index as f32 * step,
                plot.bottom() - height,
                (step - 1.5).max(1.0),
                height,
            ),
            ink,
        );
    }
}

/// Joins as many leading segments as fit in `width`, keeping at least the
/// first however narrow the window gets.
fn fit_segments(renderer: &mut Renderer, segments: &[String], width: f32) -> String {
    const SEPARATOR: &str = "   \u{00b7}   ";
    let mut text = segments.first().cloned().unwrap_or_default();
    for segment in &segments[1..] {
        let candidate = format!("{text}{SEPARATOR}{segment}");
        if renderer.measure_text(&candidate, TEXT_SIZE)[0] > width {
            break;
        }
        text = candidate;
    }
    text
}

fn describe_pixels(current: &Current) -> String {
    let channels = match current.image.channels() {
        crate::image::Channels::Gray => "gray",
        crate::image::Channels::GrayAlpha => "gray+alpha",
        crate::image::Channels::Rgb => "rgb",
        crate::image::Channels::Rgba => "rgba",
    };
    let stored = current
        .format
        .map(|format| format!(" \u{2192} {format:?}"))
        .unwrap_or_default();
    format!(
        "{} {channels}{stored}",
        current.image.samples.component_name()
    )
}

fn describe_state(current: &Current, view: &View, layout: &Layout) -> String {
    let zoom = view.zoom(current.size(), layout.viewport);
    let mut parts = vec![
        format!("{:.0}%", zoom * 100.0),
        view.mode_label().to_string(),
    ];

    // Only while it is doing something. Below 1:1 the filter in use is the
    // area average, which is not a choice and so not worth a word in the bar.
    if zoom > 1.0 {
        parts.push(view.upscale().label().to_string());
    }
    if current.display.auto != AutoWindow::Off {
        parts.push(format!(
            "{} {}",
            current.display.auto.label(),
            format_window(current)
        ));
    }
    if current.display.exposure_stops != 0.0 {
        parts.push(format!("{:+.1} EV", current.display.exposure_stops));
    }
    if current.display.colormap != Colormap::Gray {
        parts.push(current.display.colormap.label().to_string());
    }
    if current.image.is_high_dynamic_range() {
        parts.push(current.display.tone_map.label().to_string());
    }
    if layout.file_count > 1 {
        parts.push(format!("[{}/{}]", layout.index + 1, layout.file_count));
    }
    parts.join("   \u{00b7}   ")
}

/// Window bounds in the units of the source file where that is meaningful.
/// Linear integer data reads back as counts, which is what measurement work
/// wants; anything with a curve on it stays in normalised units.
fn format_window(current: &Current) -> String {
    let scale = if current.image.color.transfer.is_linear() {
        current.image.samples.full_scale()
    } else {
        1.0
    };
    let low = current.display.low * scale;
    let high = current.display.high * scale;
    if scale > 1.0 {
        format!("{low:.0}\u{2013}{high:.0}")
    } else {
        format!("{low:.3}\u{2013}{high:.3}")
    }
}

/// Draws the histogram in the bottom-right of `content`, the area the panels
/// leave free.
///
/// Colour images get four planes — red, green, blue and luminance — over the
/// range their colour channels span; grey images keep the single luminance
/// plane over theirs.
fn draw_histogram(frame: &mut UiFrame, current: &Current, content: Rect) {
    // Rounded, so that the whole-pixel bin spacing starts on a pixel edge.
    let panel = Rect::new(
        (content.right() - HISTOGRAM_SIZE[0] - PADDING)
            .max(content.x + PADDING)
            .round(),
        (content.bottom() - HISTOGRAM_SIZE[1] - PADDING)
            .max(content.y + PADDING)
            .round(),
        HISTOGRAM_SIZE[0],
        HISTOGRAM_SIZE[1],
    );
    frame.rounded_rect(panel, 6.0, PANEL_BACKGROUND);

    let plot = panel.inset(HISTOGRAM_INSET, HISTOGRAM_INSET);
    let label_height = TEXT_SIZE * 1.4;
    let bars = Rect::new(
        plot.x,
        plot.y + label_height,
        plot.width,
        plot.height - label_height,
    );

    // Luminance always goes down first, underneath the colour planes: it
    // runs as tall as the tallest of them about as often as not, and painting
    // it on top swamps the colour the panel exists to show.
    let mut colour: &[[u32; BINS]] = &[];
    let (luma, axis_min, axis_max) = match &current.stats.channels {
        Some(channels) => {
            colour = &channels.bins[..COLOUR_PLANES];
            (
                &channels.bins[ChannelStats::LUMA],
                channels.min,
                channels.max,
            )
        }
        None => (
            &current.stats.histogram,
            current.stats.min,
            current.stats.max,
        ),
    };

    frame.text(
        [plot.x, plot.y],
        TEXT_SIZE * 0.85,
        TEXT_DIM,
        format!("{axis_min:.4}  \u{2013}  {axis_max:.4}"),
    );

    // One peak across every plane, so their heights stay comparable.
    let peak = colour
        .iter()
        .flatten()
        .chain(luma)
        .copied()
        .max()
        .unwrap_or(1)
        .max(1) as f32;
    let bin_width = bars.width / BINS as f32;
    // Log scale: a linear one is all noise once a single bin dominates.
    let height_of = |count: u32| ((count as f32).ln_1p() / peak.ln_1p()) * bars.height;
    let mut column: Vec<(f32, Color)> = Vec::with_capacity(COLOUR_PLANES);
    for index in 0..BINS {
        let bar = |height: f32| {
            Rect::new(
                bars.x + index as f32 * bin_width,
                bars.bottom() - height,
                bin_width.max(1.0),
                height,
            )
        };
        if luma[index] > 0 {
            frame.rect(bar(height_of(luma[index])), HISTOGRAM_LUMA);
        }
        column.clear();
        column.extend(
            colour
                .iter()
                .zip(HISTOGRAM_PLANES)
                .filter(|(counts, _)| counts[index] > 0)
                .map(|(counts, color)| (height_of(counts[index]), color)),
        );
        // Tallest first: bars are filled to the baseline, so a colour drawn
        // over a taller one would otherwise be lost behind it.
        column.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (height, color) in &column {
            frame.rect(bar(*height), *color);
        }
    }

    // Where the display window sits within the plotted range.
    let span = axis_max - axis_min;
    if span > 0.0 {
        for value in [current.display.low, current.display.high] {
            let position = ((value - axis_min) / span).clamp(0.0, 1.0);
            frame.rect(
                Rect::new(
                    bars.x + position * bars.width - 0.5,
                    bars.y,
                    1.5,
                    bars.height,
                ),
                ACCENT,
            );
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

/// Open at the image's own size, shrunk to fit comfortably on the monitor.
///
/// The panels take their room out of the image rather than lying over it, so
/// the window asks for the image *plus* the chrome around it — otherwise a
/// picture that used to open at 100% would open slightly reduced. The monitor
/// fraction still applies to the image itself.
fn initial_window_size(event_loop: &ActiveEventLoop, image: [f32; 2]) -> PhysicalSize<u32> {
    let monitor = event_loop
        .primary_monitor()
        .or_else(|| event_loop.available_monitors().next());
    let scale = monitor.as_ref().map_or(1.0, |monitor| monitor.scale_factor());
    let chrome = [2.0 * SIDE_WIDTH as f64 * scale, 2.0 * BAR_HEIGHT as f64 * scale];

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

    const WINDOW: [f32; 2] = [1000.0, 700.0];
    /// The same window with nothing taken out of it, for the tests that are
    /// about stepping between files rather than about where the panels are.
    const VIEWPORT: Viewport = Viewport::whole(WINDOW);

    #[test]
    fn the_side_panels_are_nested_between_the_bars() {
        let chrome = Chrome::new(WINDOW);

        // The bars own the full width, and so the corners.
        assert_eq!(chrome.top, Rect::new(0.0, 0.0, 1000.0, BAR_HEIGHT));
        assert_eq!(
            chrome.bottom,
            Rect::new(0.0, 700.0 - BAR_HEIGHT, 1000.0, BAR_HEIGHT)
        );

        // The sides start where the top ends and stop where the bottom begins.
        assert_eq!(chrome.left.y, chrome.top.bottom());
        assert_eq!(chrome.left.bottom(), chrome.bottom.y);
        assert_eq!(chrome.right.y, chrome.top.bottom());
        assert_eq!(chrome.right.bottom(), chrome.bottom.y);

        assert_eq!(chrome.left.x, 0.0);
        assert_eq!(chrome.left.width, SIDE_WIDTH);
        assert_eq!(chrome.right.right(), 1000.0);
        assert_eq!(chrome.right.width, SIDE_WIDTH);
    }

    #[test]
    fn the_content_area_is_what_the_four_leave_behind() {
        let content = Chrome::new(WINDOW).content();
        assert_eq!(
            content,
            Rect::new(
                SIDE_WIDTH,
                BAR_HEIGHT,
                1000.0 - 2.0 * SIDE_WIDTH,
                700.0 - 2.0 * BAR_HEIGHT
            )
        );
    }

    #[test]
    fn the_histogram_toggle_sits_inside_the_right_panel() {
        let chrome = Chrome::new(WINDOW);
        let button = chrome.histogram_button;

        assert!(button.x >= chrome.right.x);
        assert!(button.right() <= chrome.right.right());
        assert!(button.y >= chrome.right.y);
        assert!(button.bottom() <= chrome.right.bottom());

        // Centred in the strip rather than merely fitted into it.
        assert_eq!(
            button.x - chrome.right.x,
            chrome.right.right() - button.right()
        );

        assert!(chrome.contains([button.x + 1.0, button.y + 1.0]));
        assert!(!chrome.contains([chrome.right.x - 1.0, button.y + 1.0]));
    }

    #[test]
    fn a_window_smaller_than_its_own_chrome_stays_within_itself() {
        // Panels are laid out from the window size, so a window dragged down
        // to nothing must not produce rectangles that escape it or run
        // backwards — a negative width would be drawn as a flipped quad.
        for size in [[10.0, 10.0], [0.0, 0.0], [200.0, 20.0]] {
            let chrome = Chrome::new(size);
            for panel in [chrome.top, chrome.bottom, chrome.left, chrome.right] {
                assert!(
                    panel.width >= 0.0 && panel.height >= 0.0,
                    "{panel:?} at {size:?}"
                );
                assert!(panel.x >= 0.0 && panel.y >= 0.0, "{panel:?} at {size:?}");
                assert!(
                    panel.right() <= size[0] + f32::EPSILON,
                    "{panel:?} at {size:?}"
                );
                assert!(
                    panel.bottom() <= size[1] + f32::EPSILON,
                    "{panel:?} at {size:?}"
                );
            }
            let content = chrome.content();
            assert!(
                content.width >= 0.0 && content.height >= 0.0,
                "{content:?} at {size:?}"
            );
        }
    }

    /// The panels are opaque, so the image is fitted into what they leave —
    /// and gets the whole window back the moment they are hidden, without
    /// anything having to re-fit it by hand.
    #[test]
    fn the_image_is_fitted_between_the_panels_and_re_fitted_without_them() {
        // A 2x window, to catch a conversion that only holds at scale 1.
        let physical = [2000.0, 1400.0];
        let shown = image_viewport(physical, 2.0, true);
        assert_eq!(
            shown,
            Viewport::new(
                2.0 * SIDE_WIDTH,
                2.0 * BAR_HEIGHT,
                2000.0 - 4.0 * SIDE_WIDTH,
                1400.0 - 4.0 * BAR_HEIGHT,
            )
        );

        let hidden = image_viewport(physical, 2.0, false);
        assert_eq!(hidden, Viewport::whole(physical));

        let view = View::new();
        let image = [900.0, 600.0];
        assert_eq!(view.mode_label(), "fit");
        assert!(view.zoom(image, hidden) > view.zoom(image, shown));

        // Fitted between the panels means fitted *inside* them: the image is
        // centred on the content area, not on the window.
        let placement = view.placement(image, shown);
        assert!(placement.x >= shown.x - 0.5);
        assert!(placement.x + placement.width <= shown.x + shown.width + 0.5);
        assert!(placement.y >= shown.y - 0.5);
        assert!(placement.y + placement.height <= shown.y + shown.height + 0.5);
    }

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
    fn app_over(name: &str, files: &[(&str, u32, u32)]) -> (App, PathBuf) {
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
            upscale: Upscale::default(),
        };
        let first = decode::load(&paths[0], options.overrides).expect("we just wrote it");
        let app = App::new(paths, 0, first, options);
        (app, dir)
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
        assert_eq!(app.index, 1);
        assert_eq!(app.view.mode_label(), "free");
        assert_eq!(app.view.zoom(app.image_size(), VIEWPORT), zoom);

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// A file of another size is another picture, and gets the opening view.
    #[test]
    fn stepping_to_an_image_of_another_size_fits_it() {
        let (mut app, dir) = app_over("other", &[("a.png", 64, 48), ("b.png", 32, 32)]);
        app.view.actual_size(app.image_size(), VIEWPORT);
        app.view.zoom_in(app.image_size(), VIEWPORT);

        app.step(true);
        assert_eq!(app.index, 1);
        assert_eq!(app.view.mode_label(), "fit");

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }
}
