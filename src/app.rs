//! Window lifecycle, key handling, and building each frame's interface.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Cursor, CursorIcon, Window, WindowId};

use crate::image::display::{AutoWindow, Colormap, Display, Startup};
use crate::image::{DecodedImage, Stats, decode};
use crate::render::{Color, HdrPreference, Rect, Renderer, UiFrame};
use crate::view::{Upscale, View};

/// Window pixels moved per arrow-key press.
const PAN_STEP: f32 = 64.0;
/// Trackpad pixels that add up to one notch of the wheel. Wheels report whole
/// lines and need no conversion; a trackpad reports the scroll it would have
/// done, and this is what turns that into the same zoom increment.
const WHEEL_PIXELS_PER_STEP: f32 = 50.0;
/// Fraction of the monitor a freshly opened window may occupy.
const MAX_WINDOW_FRACTION: f64 = 0.85;

const BAR_HEIGHT: f32 = 30.0;
const TEXT_SIZE: f32 = 13.0;
const PADDING: f32 = 12.0;
const HISTOGRAM_SIZE: [f32; 2] = [320.0, 130.0];

const BAR_BACKGROUND: Color = Color::rgba(0, 0, 0, 160);
const PANEL_BACKGROUND: Color = Color::rgba(12, 12, 16, 214);
const TEXT_PRIMARY: Color = Color::rgb(238, 238, 238);
const TEXT_DIM: Color = Color::rgb(150, 152, 160);
const ACCENT: Color = Color::rgb(120, 180, 255);

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

pub struct App {
    files: Vec<PathBuf>,
    index: usize,
    current: Option<Current>,
    overrides: decode::Overrides,
    startup: Startup,
    hdr: HdrPreference,
    view: View,
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
    show_overlay: bool,
    show_histogram: bool,
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
            window: None,
            renderer: None,
            modifiers: ModifiersState::empty(),
            cursor: None,
            dragging: false,
            drag_from: None,
            show_overlay: true,
            show_histogram: histogram,
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

    /// Loads and displays `files[index]`. Returns `false` if it could not be
    /// decoded, leaving the current image on screen.
    fn show(&mut self, index: usize) -> bool {
        let path = &self.files[index];
        let image = match decode::load(path, self.overrides) {
            Ok(image) => image,
            Err(error) => {
                eprintln!("image-view: {error:#}");
                return false;
            }
        };

        let stats = Stats::scan(&image);
        let display = Display::for_image_with(&image, &stats, self.startup);
        self.index = index;
        self.view.reset();

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
            if self.show(index) {
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
        let window = self.window_size();

        match key {
            Key::Named(NamedKey::Escape) => {
                event_loop.exit();
                return false;
            }
            Key::Named(NamedKey::ArrowLeft) => self.view.pan_by(-PAN_STEP, 0.0, image, window),
            Key::Named(NamedKey::ArrowRight) => self.view.pan_by(PAN_STEP, 0.0, image, window),
            Key::Named(NamedKey::ArrowUp) => self.view.pan_by(0.0, -PAN_STEP, image, window),
            Key::Named(NamedKey::ArrowDown) => self.view.pan_by(0.0, PAN_STEP, image, window),
            Key::Named(NamedKey::PageDown) => self.step(true),
            Key::Named(NamedKey::PageUp) => self.step(false),
            Key::Character(text) => return self.handle_character(event_loop, text, image, window),
            _ => return false,
        }
        true
    }

    fn handle_character(
        &mut self,
        event_loop: &ActiveEventLoop,
        text: &str,
        image: [f32; 2],
        window: [f32; 2],
    ) -> bool {
        // Everything below the view controls needs an image to act on.
        match text {
            "q" | "Q" => {
                event_loop.exit();
                return false;
            }
            "+" | "=" => {
                self.view.zoom_in(image, window);
                return true;
            }
            "-" | "_" => {
                self.view.zoom_out(image, window);
                return true;
            }
            "0" => {
                self.view.actual_size(image, window);
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
            "i" | "I" => {
                self.show_overlay = !self.show_overlay;
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
    fn handle_button(&mut self, state: ElementState, button: MouseButton) {
        if button != MouseButton::Left {
            return;
        }
        self.dragging = state == ElementState::Pressed;
        self.drag_from = if self.dragging { self.cursor } else { None };

        if let Some(window) = &self.window {
            // The closed hand is a promise that dragging will move something,
            // so a fitted image — which has nowhere to go — does not make it.
            let icon = if self.dragging && self.view.can_pan(self.image_size(), self.window_size())
            {
                CursorIcon::Grabbing
            } else {
                CursorIcon::Default
            };
            window.set_cursor(Cursor::Icon(icon));
        }
    }

    /// Follows the pointer. Returns `true` if a drag moved the view.
    fn handle_motion(&mut self, position: [f32; 2]) -> bool {
        self.cursor = Some(position);
        if !self.dragging {
            return false;
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
            .pan_by(dx, dy, self.image_size(), self.window_size());
        true
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

        let window = self.window_size();
        let anchor = self.cursor.unwrap_or([window[0] / 2.0, window[1] / 2.0]);
        self.view
            .zoom_steps_at(steps, anchor, self.image_size(), window);
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
        let placement = self.view.placement(self.image_size(), physical);

        // Split borrow: the frame builder needs the renderer's font metrics
        // while reading the rest of the application state.
        let renderer = self.renderer.as_mut().expect("checked above");
        let frame = build_ui(
            renderer,
            Layout {
                logical,
                physical,
                index: self.index,
                file_count: self.files.len(),
                show_overlay: self.show_overlay,
                show_histogram: self.show_histogram,
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
            WindowEvent::CursorLeft { .. } => self.cursor = None,
            WindowEvent::MouseInput { state, button, .. } => self.handle_button(state, button),
            // A drag the window did not see end — the button came up over
            // another window, say — would otherwise resume on the next motion.
            WindowEvent::Focused(false) => {
                self.handle_button(ElementState::Released, MouseButton::Left);
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
    /// Window size in physical pixels, which is what zoom is measured against.
    physical: [f32; 2],
    index: usize,
    file_count: usize,
    show_overlay: bool,
    show_histogram: bool,
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
    let mut frame = UiFrame::new(size);
    let Some(current) = current else {
        return frame;
    };

    if layout.show_histogram {
        draw_histogram(&mut frame, current, layout.show_overlay);
    }
    if !layout.show_overlay {
        return frame;
    }

    let bar = Rect::new(0.0, size[1] - BAR_HEIGHT, size[0], BAR_HEIGHT);
    frame.rect(bar, BAR_BACKGROUND);

    let baseline = bar.y + (BAR_HEIGHT - TEXT_SIZE * 1.3) / 2.0;

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
            current.label.clone(),
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
    let zoom = view.zoom(current.size(), layout.physical);
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

fn draw_histogram(frame: &mut UiFrame, current: &Current, above_bar: bool) {
    let size = frame.size();
    let bottom = size[1] - if above_bar { BAR_HEIGHT } else { 0.0 } - PADDING;
    let panel = Rect::new(
        size[0] - HISTOGRAM_SIZE[0] - PADDING,
        bottom - HISTOGRAM_SIZE[1],
        HISTOGRAM_SIZE[0],
        HISTOGRAM_SIZE[1],
    );
    frame.rounded_rect(panel, 6.0, PANEL_BACKGROUND);

    let plot = panel.inset(10.0, 10.0);
    let label_height = TEXT_SIZE * 1.4;
    let bars = Rect::new(
        plot.x,
        plot.y + label_height,
        plot.width,
        plot.height - label_height,
    );

    frame.text(
        [plot.x, plot.y],
        TEXT_SIZE * 0.85,
        TEXT_DIM,
        format!(
            "{:.4}  \u{2013}  {:.4}",
            current.stats.min, current.stats.max
        ),
    );

    let peak = current
        .stats
        .histogram
        .iter()
        .copied()
        .max()
        .unwrap_or(1)
        .max(1) as f32;
    let bin_width = bars.width / crate::image::stats::BINS as f32;
    for (index, count) in current.stats.histogram.iter().enumerate() {
        if *count == 0 {
            continue;
        }
        // Log scale: a linear one is all noise once a single bin dominates.
        let height = ((*count as f32).ln_1p() / peak.ln_1p()) * bars.height;
        frame.rect(
            Rect::new(
                bars.x + index as f32 * bin_width,
                bars.bottom() - height,
                bin_width.max(1.0),
                height,
            ),
            TEXT_DIM.with_alpha(200),
        );
    }

    // Where the display window sits within the observed range.
    let span = current.stats.max - current.stats.min;
    if span > 0.0 {
        for value in [current.display.low, current.display.high] {
            let position = ((value - current.stats.min) / span).clamp(0.0, 1.0);
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
fn initial_window_size(event_loop: &ActiveEventLoop, image: [f32; 2]) -> PhysicalSize<u32> {
    let (mut width, mut height) = (image[0] as f64, image[1] as f64);

    if let Some(monitor) = event_loop
        .primary_monitor()
        .or_else(|| event_loop.available_monitors().next())
    {
        let available = monitor.size();
        let max_width = available.width as f64 * MAX_WINDOW_FRACTION;
        let max_height = available.height as f64 * MAX_WINDOW_FRACTION;
        if max_width > 1.0 && max_height > 1.0 {
            let shrink = (max_width / width).min(max_height / height).min(1.0);
            width *= shrink;
            height *= shrink;
        }
    }

    PhysicalSize::new(
        (width.round() as u32).max(320),
        (height.round() as u32).max(240),
    )
}
