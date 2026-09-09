//! The toolkit's side of the window: egui's context and the winit adapter
//! that feeds it, and the two things the loop has to know from it — when it
//! wants painting again, and whether the pointer was on the picture.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use winit::event::WindowEvent;
use winit::window::Window;

use crate::theme::Theme;
use crate::ui;

pub(super) struct Gui {
    pub ctx: egui::Context,
    state: egui_winit::State,
    /// When egui last asked to be painted again, for the loop to sleep
    /// until: a hover fade, a tooltip's delay. `None` while nothing is
    /// due, which is nearly always.
    repaint_due: Option<Instant>,
}

impl Gui {
    pub fn new(window: &Arc<Window>, theme: &Theme, max_texture_side: u32) -> Result<Self> {
        let ctx = egui::Context::default();
        ctx.set_fonts(ui::fonts::system()?);
        ui::style::apply(&ctx, theme);
        // A repaint asked for from outside a frame — there are none yet, but
        // a widget's animation could — wakes the loop the way any event does.
        let handle = window.clone();
        ctx.set_request_repaint_callback(move |_| handle.request_redraw());
        let state = egui_winit::State::new(
            ctx.clone(),
            egui::ViewportId::ROOT,
            &**window,
            Some(window.scale_factor() as f32),
            window.theme(),
            Some(max_texture_side as usize),
        );
        Ok(Self {
            ctx,
            state,
            repaint_due: None,
        })
    }

    /// Hands `event` to egui. Returns whether egui took it for itself: a
    /// press on one of its widgets, a key into one of its fields.
    pub fn on_event(&mut self, window: &Window, event: &WindowEvent) -> egui_winit::EventResponse {
        self.state.on_window_event(window, event)
    }

    /// Runs one pass of the interface and returns what it drew, for the
    /// renderer, with what egui changed about its textures, having applied
    /// whatever egui asked of the window — the cursor icon, mostly.
    pub fn run(
        &mut self,
        window: &Window,
        show: impl FnMut(&mut egui::Ui),
    ) -> (Painted, egui::TexturesDelta) {
        let raw = self.state.take_egui_input(window);
        let full = self.ctx.run_ui(raw, show);
        self.state
            .handle_platform_output(window, full.platform_output);
        let delay = full
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|viewport| viewport.repaint_delay)
            .unwrap_or(Duration::MAX);
        self.schedule(delay, window);
        let primitives = self.ctx.tessellate(full.shapes, full.pixels_per_point);
        (
            Painted {
                primitives,
                pixels_per_point: full.pixels_per_point,
            },
            full.textures_delta,
        )
    }

    /// Takes in how soon egui wants painting again: now, at some moment, or
    /// not until something happens.
    fn schedule(&mut self, delay: Duration, window: &Window) {
        self.repaint_due = if delay == Duration::ZERO {
            window.request_redraw();
            None
        } else if delay == Duration::MAX {
            None
        } else {
            Some(Instant::now() + delay)
        };
    }

    /// When egui next wants painting, for the loop's deadline.
    pub fn deadline(&self) -> Option<Instant> {
        self.repaint_due
    }

    /// Whether the moment egui asked for has come. Clears it: a repaint is
    /// owed once.
    pub fn due(&mut self, now: Instant) -> bool {
        match self.repaint_due {
            Some(due) if now >= due => {
                self.repaint_due = None;
                true
            }
            _ => false,
        }
    }

    /// Puts `theme` on the interface, for when the desktop's changes.
    pub fn retint(&self, theme: &Theme) {
        ui::style::apply(&self.ctx, theme);
    }
}

/// What one pass of the interface came to, in the form the renderer draws.
pub use crate::render::UiPaint as Painted;
