//! What the keyboard and the pointer do.
//!
//! Keys go through one table, [`KEYS`], which is also what `--help` prints:
//! a binding added here is documented by the same edit. Each key names an
//! [`Action`], and [`App::perform`] is the one place an action happens.

use winit::event::{ElementState, MouseButton, MouseScrollDelta};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Cursor, CursorIcon};

use super::App;
use crate::image::display::Startup;
use crate::ui::{Current, Widget};

/// Window pixels moved per arrow-key press.
const PAN_STEP: f32 = 64.0;

/// Trackpad pixels that add up to one notch of the wheel. Wheels report whole
/// lines and need no conversion; a trackpad reports the scroll it would have
/// done, and this is what turns that into the same zoom increment.
const WHEEL_PIXELS_PER_STEP: f32 = 50.0;

/// Something a key asks for.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    Quit,
    ZoomIn,
    ZoomOut,
    ActualSize,
    Pan(Direction),
    CycleFit,
    CycleUpscale,
    NextFile,
    PreviousFile,
    ToggleInterface,
    ToggleHistogram,
    ToggleMinimap,
    /// Exposure, by this many stops.
    Exposure(f32),
    CycleAutoWindow,
    /// Slide the window by this fraction of its width.
    ShiftWindow(f32),
    /// Scale the window's width by this factor.
    Contrast(f32),
    CycleToneMap,
    CycleColormap,
    ResetDisplay,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl Direction {
    /// One step in this direction, in window pixels.
    fn step(self) -> (f32, f32) {
        match self {
            Direction::Left => (-PAN_STEP, 0.0),
            Direction::Right => (PAN_STEP, 0.0),
            Direction::Up => (0.0, -PAN_STEP),
            Direction::Down => (0.0, PAN_STEP),
        }
    }
}

/// A key as `winit` reports it: the character it produced, or the name of
/// one that produces none.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyName {
    Char(&'static str),
    Named(NamedKey),
}

/// Which heading a binding is listed under in `--help`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    View,
    Display,
}

/// One line of `--help`, and the keys that do it. A line may bind several
/// keys to several actions (`n, p`), and a line may bind none (`Wheel`).
pub struct Binding {
    pub section: Section,
    /// The key column, as written for people: `q, Esc`, `Arrows`.
    pub shown: &'static str,
    pub help: &'static str,
    pub keys: &'static [(KeyName, Action)],
}

use Action::*;
use KeyName::{Char, Named};

/// Every key, in the order `--help` lists them.
pub const KEYS: &[Binding] = &[
    Binding {
        section: Section::View,
        shown: "q, Esc",
        help: "Quit",
        keys: &[
            (Char("q"), Quit),
            (Char("Q"), Quit),
            (Named(NamedKey::Escape), Quit),
        ],
    },
    Binding {
        section: Section::View,
        shown: "+, =",
        help: "Zoom in",
        keys: &[(Char("+"), ZoomIn), (Char("="), ZoomIn)],
    },
    Binding {
        section: Section::View,
        shown: "-, _",
        help: "Zoom out",
        keys: &[(Char("-"), ZoomOut), (Char("_"), ZoomOut)],
    },
    Binding {
        section: Section::View,
        shown: "Wheel",
        help: "Zoom about the pointer",
        keys: &[],
    },
    Binding {
        section: Section::View,
        shown: "0",
        help: "Actual size (100%)",
        keys: &[(Char("0"), ActualSize)],
    },
    Binding {
        section: Section::View,
        shown: "Arrows",
        help: "Pan",
        keys: &[
            (Named(NamedKey::ArrowLeft), Pan(Direction::Left)),
            (Named(NamedKey::ArrowRight), Pan(Direction::Right)),
            (Named(NamedKey::ArrowUp), Pan(Direction::Up)),
            (Named(NamedKey::ArrowDown), Pan(Direction::Down)),
        ],
    },
    Binding {
        section: Section::View,
        shown: "f",
        help: "Cycle fit / fit width / fit height",
        keys: &[(Char("f"), CycleFit), (Char("F"), CycleFit)],
    },
    Binding {
        section: Section::View,
        shown: "u",
        help: "Cycle the filter used above 100%: nearest, bicubic",
        keys: &[(Char("u"), CycleUpscale), (Char("U"), CycleUpscale)],
    },
    Binding {
        section: Section::View,
        shown: "n, p",
        help: "Next / previous file",
        keys: &[
            (Char("n"), NextFile),
            (Char("N"), NextFile),
            (Named(NamedKey::PageDown), NextFile),
            (Char("p"), PreviousFile),
            (Char("P"), PreviousFile),
            (Named(NamedKey::PageUp), PreviousFile),
        ],
    },
    Binding {
        section: Section::Display,
        shown: "e, E",
        help: "Exposure down / up, half a stop",
        keys: &[(Char("e"), Exposure(-0.5)), (Char("E"), Exposure(0.5))],
    },
    Binding {
        section: Section::Display,
        shown: "a",
        help: "Cycle the automatic window: unit, min/max, 99.8%",
        keys: &[(Char("a"), CycleAutoWindow), (Char("A"), CycleAutoWindow)],
    },
    Binding {
        section: Section::Display,
        shown: "[, ]",
        help: "Slide the window down / up",
        keys: &[
            (Char("["), ShiftWindow(-0.05)),
            (Char("]"), ShiftWindow(0.05)),
        ],
    },
    Binding {
        section: Section::Display,
        shown: ", .",
        help: "Narrow / widen the window",
        keys: &[
            (Char(","), Contrast(0.8)),
            (Char("<"), Contrast(0.8)),
            (Char("."), Contrast(1.25)),
            (Char(">"), Contrast(1.25)),
        ],
    },
    Binding {
        section: Section::Display,
        shown: "t",
        help: "Cycle tone mapping: clip, reinhard, neutral",
        keys: &[(Char("t"), CycleToneMap), (Char("T"), CycleToneMap)],
    },
    Binding {
        section: Section::Display,
        shown: "c",
        help: "Cycle false colour for single-channel images",
        keys: &[(Char("c"), CycleColormap), (Char("C"), CycleColormap)],
    },
    Binding {
        section: Section::Display,
        shown: "r",
        help: "Reset display settings",
        keys: &[(Char("r"), ResetDisplay), (Char("R"), ResetDisplay)],
    },
    Binding {
        section: Section::Display,
        shown: "h",
        help: "Toggle the histogram",
        keys: &[(Char("h"), ToggleHistogram), (Char("H"), ToggleHistogram)],
    },
    Binding {
        section: Section::Display,
        shown: "m",
        help: "Toggle the minimap",
        keys: &[(Char("m"), ToggleMinimap), (Char("M"), ToggleMinimap)],
    },
    Binding {
        section: Section::Display,
        shown: "`",
        help: "Toggle the interface panels",
        keys: &[(Char("`"), ToggleInterface), (Char("~"), ToggleInterface)],
    },
];

/// What `key` asks for, if anything.
pub fn action_for(key: &Key) -> Option<Action> {
    KEYS.iter()
        .flat_map(|binding| binding.keys)
        .find(|(name, _)| match (name, key) {
            (Char(text), Key::Character(typed)) => typed.as_str() == *text,
            (Named(name), Key::Named(pressed)) => name == pressed,
            _ => false,
        })
        .map(|(_, action)| *action)
}

/// What an event leaves the window owing.
#[must_use]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Effect {
    /// The frame is out of date.
    Redraw,
    Nothing,
    /// The user has asked to leave.
    Quit,
}

impl Effect {
    pub fn redraw_if(changed: bool) -> Self {
        if changed {
            Effect::Redraw
        } else {
            Effect::Nothing
        }
    }
}

/// Where the pointer is and what it is doing.
#[derive(Default)]
pub(super) struct Pointer {
    pub(super) modifiers: ModifiersState,
    /// Physical window pixels, and the point a wheel zoom works about.
    pub(super) cursor: Option<[f32; 2]>,
    /// Whether the left button is down, which is what a drag is.
    pub(super) dragging: bool,
    /// Where the pointer was when the drag last moved the view. Held apart
    /// from `cursor` because a press can arrive before any motion has told us
    /// where the pointer is, and because leaving the window clears `cursor`
    /// without ending a drag the pointer grab is still delivering.
    pub(super) drag_from: Option<[f32; 2]>,
}

impl Pointer {
    /// Whether a key or wheel event belongs to the window manager rather than
    /// to us: a compositor binding such as Super+0 still delivers its key
    /// here, and acting on it would move the view behind the user's back.
    fn chorded(&self) -> bool {
        self.modifiers.control_key() || self.modifiers.alt_key() || self.modifiers.super_key()
    }
}

impl App {
    pub(super) fn handle_key(&mut self, key: &Key) -> Effect {
        if self.pointer.chorded() {
            return Effect::Nothing;
        }
        match action_for(key) {
            Some(action) => self.perform(action),
            None => Effect::Nothing,
        }
    }

    /// Does what a key asked for.
    pub(super) fn perform(&mut self, action: Action) -> Effect {
        let image = self.image_size();
        let viewport = self.viewport();
        match action {
            Quit => return Effect::Quit,
            ZoomIn => self.view.zoom_in(image, viewport),
            ZoomOut => self.view.zoom_out(image, viewport),
            ActualSize => self.view.actual_size(image, viewport),
            Pan(direction) => {
                let (dx, dy) = direction.step();
                self.view.pan_by(dx, dy, image, viewport);
            }
            CycleFit => self.view.cycle_fit(),
            CycleUpscale => self.view.cycle_upscale(),
            // Nothing to draw yet: the file is only being asked for, and what
            // is on screen stays until it arrives.
            NextFile => {
                self.step(true);
                return Effect::Nothing;
            }
            PreviousFile => {
                self.step(false);
                return Effect::Nothing;
            }
            // A fitted image re-fits on the next frame: the viewport it is
            // measured against is the one the panels leave, and they have
            // just come or gone.
            ToggleInterface => self.panels.show_ui = !self.panels.show_ui,
            ToggleHistogram => self.panels.toggle(Widget::Histogram),
            ToggleMinimap => self.panels.toggle(Widget::Minimap),
            Exposure(stops) => {
                return self.adjust(|current, _| {
                    current.display.adjust_exposure(stops);
                    true
                });
            }
            CycleAutoWindow => {
                return self.adjust(|current, _| {
                    current.display.cycle_auto(&current.stats);
                    true
                });
            }
            ShiftWindow(by) => {
                return self.adjust(|current, _| {
                    current.display.shift_window(by);
                    true
                });
            }
            Contrast(by) => {
                return self.adjust(|current, _| {
                    current.display.adjust_contrast(by);
                    true
                });
            }
            CycleToneMap => {
                return self.adjust(|current, _| {
                    current.display.cycle_tone_map();
                    true
                });
            }
            CycleColormap => {
                return self.adjust(|current, _| {
                    if !current.image.is_gray() {
                        return false;
                    }
                    current.display.cycle_colormap();
                    true
                });
            }
            ResetDisplay => {
                return self.adjust(|current, startup| {
                    current
                        .display
                        .reset(&current.stats, &current.image, startup);
                    true
                });
            }
        }
        Effect::Redraw
    }

    /// Runs `change` on the image on screen, if there is one. `change` says
    /// whether it changed anything, since some settings apply only to some
    /// images.
    fn adjust(&mut self, change: impl FnOnce(&mut Current, Startup) -> bool) -> Effect {
        let startup = self.startup;
        let Some(current) = &mut self.current else {
            return Effect::Nothing;
        };
        Effect::redraw_if(change(current, startup))
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
    pub(super) fn handle_button(&mut self, state: ElementState, button: MouseButton) -> bool {
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

        self.pointer.dragging = state == ElementState::Pressed;
        self.pointer.drag_from = if self.pointer.dragging {
            self.pointer.cursor
        } else {
            None
        };

        if let Some(window) = &self.window {
            // The closed hand is a promise that dragging will move something,
            // so a fitted image — which has nowhere to go — does not make it.
            let icon =
                if self.pointer.dragging && self.view.can_pan(self.image_size(), self.viewport()) {
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
    pub(super) fn handle_motion(&mut self, position: [f32; 2]) -> bool {
        let was_over = self.pointer_pixel();
        self.pointer.cursor = Some(position);
        let moved_pixel = self.panels.show_ui && self.pointer_pixel() != was_over;
        if !self.pointer.dragging {
            // Nothing else to do out here, so this is where the button's
            // highlight gets to follow the pointer.
            return self.update_hover() || moved_pixel;
        }
        let Some(from) = self.pointer.drag_from.replace(position) else {
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
    pub(super) fn update_hover(&mut self) -> bool {
        let hover = self
            .logical_cursor()
            .filter(|_| self.panels.show_ui)
            .and_then(|point| self.chrome().widget_at(point));
        let changed = hover != self.panels.hover;
        self.panels.hover = hover;
        changed
    }

    /// Returns `true` if the wheel changed anything on screen.
    pub(super) fn handle_wheel(&mut self, delta: MouseScrollDelta) -> bool {
        // Same reasoning as `handle_key`: Ctrl+wheel and friends belong to the
        // compositor, and acting on them as well would zoom behind its back.
        if self.pointer.chorded() {
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
        let anchor = self.pointer.cursor.unwrap_or([
            viewport.x + viewport.width / 2.0,
            viewport.y + viewport.height / 2.0,
        ]);
        self.view
            .zoom_steps_at(steps, anchor, self.image_size(), viewport);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A key bound twice would do whichever came first in the table, silently.
    #[test]
    fn no_key_is_bound_twice() {
        let mut seen: Vec<KeyName> = Vec::new();
        for (name, _) in KEYS.iter().flat_map(|binding| binding.keys) {
            assert!(!seen.contains(name), "{name:?} is bound more than once");
            seen.push(*name);
        }
    }

    #[test]
    fn keys_resolve_to_their_actions() {
        use winit::keyboard::SmolStr;
        assert_eq!(action_for(&Key::Character(SmolStr::new("q"))), Some(Quit));
        assert_eq!(action_for(&Key::Named(NamedKey::Escape)), Some(Quit));
        assert_eq!(action_for(&Key::Named(NamedKey::PageDown)), Some(NextFile));
        assert_eq!(
            action_for(&Key::Character(SmolStr::new("E"))),
            Some(Exposure(0.5))
        );
        assert_eq!(action_for(&Key::Character(SmolStr::new("z"))), None);
    }
}
