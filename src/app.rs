//! Window lifecycle, key handling, and the glue between input and rendering.

use std::path::PathBuf;
use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

use crate::formats::{self, DecodedImage};
use crate::renderer::Renderer;
use crate::view::View;

/// Window pixels moved per arrow-key press.
const PAN_STEP: f32 = 64.0;
/// Fraction of the monitor a freshly opened window may occupy.
const MAX_WINDOW_FRACTION: f64 = 0.85;

struct Current {
    image: DecodedImage,
    label: String,
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
    view: View,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    modifiers: ModifiersState,
    /// Set if the last render failed, so we report it once rather than every frame.
    reported_error: bool,
}

impl App {
    /// `first` is the already-decoded `files[0]`, loaded before the window
    /// opened so that a bad path fails on the command line.
    pub fn new(files: Vec<PathBuf>, first: DecodedImage) -> Self {
        let label = file_label(&files[0]);
        Self {
            files,
            index: 0,
            current: Some(Current {
                image: first,
                label,
            }),
            view: View::new(),
            window: None,
            renderer: None,
            modifiers: ModifiersState::empty(),
            reported_error: false,
        }
    }

    fn status_line(&self) -> String {
        let Some(current) = &self.current else {
            return String::new();
        };
        let mut line = format!(
            "{}   {} \u{00d7} {}",
            current.label, current.image.width, current.image.height
        );
        if let Some(renderer) = &self.renderer {
            let zoom = self.view.zoom(current.size(), renderer.size());
            line.push_str(&format!(
                "   \u{00b7}   {:.0}%   \u{00b7}   {}",
                zoom * 100.0,
                self.view.mode_label()
            ));
        }
        if self.files.len() > 1 {
            line.push_str(&format!(
                "   \u{00b7}   [{}/{}]",
                self.index + 1,
                self.files.len()
            ));
        }
        line
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
        let image = match formats::load(path) {
            Ok(image) => image,
            Err(error) => {
                eprintln!("image-view: {error:#}");
                return false;
            }
        };

        self.index = index;
        self.current = Some(Current {
            image,
            label: file_label(path),
        });
        self.view.reset();
        if let (Some(renderer), Some(current)) = (&mut self.renderer, &self.current) {
            renderer.set_image(&current.image);
        }
        if let Some(window) = &self.window {
            window.set_title(&window_title(&self.files[index]));
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
            Key::Character(text) => match text.as_str() {
                "q" | "Q" => {
                    event_loop.exit();
                    return false;
                }
                "+" | "=" => self.view.zoom_in(image, window),
                "-" | "_" => self.view.zoom_out(image, window),
                "0" => self.view.actual_size(image, window),
                "f" | "F" => self.view.cycle_fit(),
                "n" | "N" => self.step(true),
                "p" | "P" => self.step(false),
                _ => return false,
            },
            _ => return false,
        }
        true
    }

    fn redraw(&mut self) {
        let (Some(_), Some(window)) = (&self.renderer, &self.window) else {
            return;
        };
        let image = self.image_size();
        let placement = self.view.placement(image, self.window_size());
        let status = self.status_line();
        let scale = window.scale_factor() as f32;

        let renderer = self.renderer.as_mut().expect("checked above");
        match renderer.render(placement, scale, &status) {
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

        let image = self.image_size();
        let size = initial_window_size(event_loop, image);
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

        let mut renderer = match Renderer::new(window.clone()) {
            Ok(renderer) => renderer,
            Err(error) => {
                eprintln!("image-view: {error:#}");
                event_loop.exit();
                return;
            }
        };
        if let Some(current) = &self.current {
            renderer.set_image(&current.image);
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

fn file_label(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn window_title(path: &std::path::Path) -> String {
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
