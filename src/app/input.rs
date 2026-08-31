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
use crate::ui::{Current, Menu, Widget};

/// Window pixels moved per arrow-key press.
const PAN_STEP: f32 = 64.0;

/// How far one notch of the wheel scrolls a panel that has more to show than
/// fits, in logical pixels: about three lines of it.
const WHEEL_SCROLL_STEP: f32 = 48.0;

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
    ToggleInfo,
    ToggleMinimap,
    ToggleGrid,
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
    /// Put the path of the file on screen on the system clipboard.
    CopyPath,
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

/// The modifiers a binding is held with.
///
/// Ctrl, Alt and Super must be held exactly as written: a chord this table
/// does not bind belongs to the window manager, and acting on `Super+0` as
/// well would move the view behind its back. Shift is asked for and not
/// forbidden, because a binding that differs by case says so with the
/// character itself — the Shift that turned `e` into `E` must not read as a
/// chord.
pub type Mods = ModifiersState;

/// Held with nothing.
const PLAIN: Mods = Mods::empty();
const CTRL_SHIFT: Mods = Mods::CONTROL.union(Mods::SHIFT);

/// Whether the modifiers `held` are the ones a binding asked for.
fn satisfies(required: Mods, held: Mods) -> bool {
    held.control_key() == required.control_key()
        && held.alt_key() == required.alt_key()
        && held.super_key() == required.super_key()
        && (!required.shift_key() || held.shift_key())
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
    /// What is held down with the keys below.
    pub mods: Mods,
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
        mods: PLAIN,
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
        mods: PLAIN,
        shown: "+, =",
        help: "Zoom in",
        keys: &[(Char("+"), ZoomIn), (Char("="), ZoomIn)],
    },
    Binding {
        section: Section::View,
        mods: PLAIN,
        shown: "-, _",
        help: "Zoom out",
        keys: &[(Char("-"), ZoomOut), (Char("_"), ZoomOut)],
    },
    Binding {
        section: Section::View,
        mods: PLAIN,
        shown: "Wheel",
        help: "Zoom about the pointer",
        keys: &[],
    },
    Binding {
        section: Section::View,
        mods: PLAIN,
        shown: "0",
        help: "Actual size (100%)",
        keys: &[(Char("0"), ActualSize)],
    },
    Binding {
        section: Section::View,
        mods: PLAIN,
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
        mods: PLAIN,
        shown: "f",
        help: "Cycle fit / fit width / fit height",
        keys: &[(Char("f"), CycleFit), (Char("F"), CycleFit)],
    },
    Binding {
        section: Section::View,
        mods: PLAIN,
        shown: "u",
        help: "Cycle the filter used above 100%: nearest, bicubic",
        keys: &[(Char("u"), CycleUpscale), (Char("U"), CycleUpscale)],
    },
    Binding {
        section: Section::View,
        mods: PLAIN,
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
        section: Section::View,
        mods: CTRL_SHIFT,
        shown: "Ctrl+Shift+C",
        help: "Copy the path of the file on screen to the clipboard",
        keys: &[(Char("C"), CopyPath), (Char("c"), CopyPath)],
    },
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "e, E",
        help: "Exposure down / up, half a stop",
        keys: &[(Char("e"), Exposure(-0.5)), (Char("E"), Exposure(0.5))],
    },
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "a",
        help: "Cycle the automatic window: unit, min/max, 99.8%",
        keys: &[(Char("a"), CycleAutoWindow), (Char("A"), CycleAutoWindow)],
    },
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "[, ]",
        help: "Slide the window down / up",
        keys: &[
            (Char("["), ShiftWindow(-0.05)),
            (Char("]"), ShiftWindow(0.05)),
        ],
    },
    Binding {
        section: Section::Display,
        mods: PLAIN,
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
        mods: PLAIN,
        shown: "t",
        help: "Cycle tone mapping: clip, reinhard, neutral",
        keys: &[(Char("t"), CycleToneMap), (Char("T"), CycleToneMap)],
    },
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "c",
        help: "Cycle false colour for single-channel images",
        keys: &[(Char("c"), CycleColormap), (Char("C"), CycleColormap)],
    },
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "r",
        help: "Reset display settings",
        keys: &[(Char("r"), ResetDisplay), (Char("R"), ResetDisplay)],
    },
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "h",
        help: "Toggle the histogram",
        keys: &[(Char("h"), ToggleHistogram), (Char("H"), ToggleHistogram)],
    },
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "i",
        help: "Toggle the file information panel",
        keys: &[(Char("i"), ToggleInfo), (Char("I"), ToggleInfo)],
    },
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "m",
        help: "Toggle the minimap",
        keys: &[(Char("m"), ToggleMinimap), (Char("M"), ToggleMinimap)],
    },
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "g",
        help: "Toggle the grid over the image",
        keys: &[(Char("g"), ToggleGrid), (Char("G"), ToggleGrid)],
    },
    Binding {
        section: Section::Display,
        mods: PLAIN,
        shown: "`",
        help: "Toggle the interface panels",
        keys: &[(Char("`"), ToggleInterface), (Char("~"), ToggleInterface)],
    },
];

/// What `key` held with `mods` asks for, if anything.
pub fn action_for(key: &Key, mods: Mods) -> Option<Action> {
    KEYS.iter()
        .filter(|binding| satisfies(binding.mods, mods))
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
    /// Set instead when the button went down on the info panel: the drag then
    /// belongs to that column and moves it, wherever the pointer goes while
    /// the button is held. Held apart from `dragging` so that a drag is one
    /// thing or the other for as long as it lasts, rather than changing what
    /// it moves the moment the pointer leaves the panel.
    pub(super) scrolling: bool,
    /// Where the pointer was when the drag last moved the view. Held apart
    /// from `cursor` because a press can arrive before any motion has told us
    /// where the pointer is, and because leaving the window clears `cursor`
    /// without ending a drag the pointer grab is still delivering.
    pub(super) drag_from: Option<[f32; 2]>,
}

impl Pointer {
    /// Whether a wheel event belongs to the window manager rather than to us:
    /// Ctrl with the wheel is a compositor gesture, and zooming on it as well
    /// would move the view behind the user's back. Keys answer the same
    /// question through [`satisfies`], which lets a chord this table does
    /// bind through.
    fn chorded(&self) -> bool {
        self.modifiers.control_key() || self.modifiers.alt_key() || self.modifiers.super_key()
    }
}

impl App {
    pub(super) fn handle_key(&mut self, key: &Key) -> Effect {
        match action_for(key, self.pointer.modifiers) {
            Some(action) => self.perform(action),
            None => Effect::Nothing,
        }
    }

    /// Does what a key asked for.
    pub(super) fn perform(&mut self, action: Action) -> Effect {
        let image = self.image_size();
        let viewport = self.viewport();
        match action {
            // An open menu takes the key: dismissing a popup is what Escape
            // is for, and quitting out from under one is not what was being
            // asked for.
            Quit => {
                if self.panels.menu.take().is_some() {
                    self.update_hover();
                    return Effect::Redraw;
                }
                return Effect::Quit;
            }
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
            ToggleInterface => {
                self.panels.show_ui = !self.panels.show_ui;
                // The menu is part of the interface, and goes with it.
                self.panels.menu = None;
                self.panels.hover = None;
            }
            ToggleHistogram => self.press(Widget::Histogram),
            ToggleInfo => self.press(Widget::Info),
            ToggleMinimap => self.press(Widget::Minimap),
            ToggleGrid => self.press(Widget::Grid),
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
            // Nothing on screen changes; the copy is reported only when it
            // could not be made.
            CopyPath => {
                self.copy_path();
                return Effect::Nothing;
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

    /// Puts the path of the file on screen on the clipboard.
    ///
    /// The copy is served by a process of its own, so that it survives this
    /// window closing. Any earlier one that has since exited — the compositor
    /// cancels the last copy as soon as this one takes the selection — is
    /// reaped here, so that a session of copying does not leave a zombie
    /// behind each time.
    fn copy_path(&mut self) {
        self.clipboard
            .retain_mut(|held| !matches!(held.try_wait(), Ok(Some(_))));
        let path = self.files.shown_path().to_string_lossy().into_owned();
        match crate::clipboard::copy_text(&path) {
            Ok(child) => self.clipboard.push(child),
            Err(error) => eprintln!(
                "image-view: {}",
                crate::escape_controls(&format!("{error:#}"))
            ),
        }
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

        // The info panel floats over the image and inside the chrome, so it
        // is asked before either: the press starts a drag of the column
        // instead of one of the picture underneath. It can be on screen with
        // the chrome hidden, so this is outside the test for that below.
        if state == ElementState::Pressed
            && let Some(panel) = self.pointer_over_info()
        {
            self.pointer.scrolling = true;
            // As with a drag of the image: the first motion after the press
            // establishes the point the drag is measured from.
            self.pointer.drag_from = self.pointer.cursor;
            // The closed hand is a promise that dragging will move something,
            // so a column with nothing left to scroll does not make it.
            let icon = if self.info_overflow(panel) > 0.0 {
                CursorIcon::Grabbing
            } else {
                CursorIcon::Default
            };
            if let Some(window) = &self.window {
                window.set_cursor(Cursor::Icon(icon));
            }
            return false;
        }

        // The chrome gets first refusal. A press that lands on a panel is
        // aimed at the interface, so it neither reaches a widget's neighbour
        // nor starts a drag of the image underneath.
        if state == ElementState::Pressed
            && self.panels.show_ui
            && let Some(point) = self.logical_cursor()
        {
            // An open menu comes before the chrome and before the image: a
            // press on a cell chooses and closes, one anywhere off the panel
            // closes and is spent doing exactly that, and one on the panel
            // but between cells lands on nothing at all.
            if let Some(menu) = self.panels.menu {
                let popup = self.chrome().popup(menu, self.panels.show_grid);
                match popup.as_ref().and_then(|popup| popup.item_at(point)) {
                    Some(index) => self.press(Widget::Cell(index)),
                    None if popup.as_ref().is_none_or(|popup| !popup.contains(point)) => {
                        self.panels.menu = None;
                    }
                    None => return false,
                }
                // The cell that had the highlight is no longer under the
                // pointer, or no longer there at all.
                self.update_hover();
                return true;
            }

            let chrome = self.chrome();
            if let Some(widget) = chrome.widget_at(point, self.panels.show_grid) {
                self.press(widget);
                // The zoom readout keeps the pointer over it as it opens its
                // menu, and the highlight belongs to the menu from here on.
                self.update_hover();
                return true;
            }
            if chrome.contains(point) {
                return false;
            }
        }

        self.pointer.dragging = state == ElementState::Pressed;
        self.pointer.scrolling = false;
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
        if self.pointer.scrolling {
            let Some(from) = self.pointer.drag_from.replace(position) else {
                // First motion of this drag: nothing to measure from yet.
                return false;
            };
            let Some(panel) = self.info_panel() else {
                return false;
            };
            // The drag holds the scrollbar's thumb rather than the words:
            // dragging down runs down the column, as pulling the thumb down
            // would, and by as much as pulling it that far would move — which
            // for a long column is a good deal further than the pointer went.
            // Motion arrives in physical pixels and the column is laid out in
            // logical ones.
            let by =
                (position[1] - from[1]) / self.scale_factor() * self.info_scroll_per_drag(panel);
            // The readout in the bar is owed a redraw too, for a drag that
            // has carried the pointer off the panel and onto the image.
            return self.scroll_info_by(panel, by) || moved_pixel;
        }
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
            .and_then(|point| self.widget_at(point));
        let changed = hover != self.panels.hover;
        self.panels.hover = hover;
        changed
    }

    /// Which widget a point lands on. An open menu floats over the interface,
    /// so its cells are tested instead of what is underneath them — including
    /// the button that opened it, which a press dismisses the menu from
    /// rather than opening a second one.
    fn widget_at(&self, point: [f32; 2]) -> Option<Widget> {
        let chrome = self.chrome();
        match self.panels.menu {
            Some(menu) => chrome
                .popup(menu, self.panels.show_grid)
                .and_then(|popup| popup.item_at(point))
                .map(Widget::Cell),
            None => chrome.widget_at(point, self.panels.show_grid),
        }
    }

    /// Acts on a press. The keys that stand in for the toggles come through
    /// here too, so that a key and a click cannot drift apart.
    fn press(&mut self, widget: Widget) {
        match widget {
            Widget::Minimap => self.panels.show_minimap = !self.panels.show_minimap,
            Widget::Histogram => self.panels.show_histogram = !self.panels.show_histogram,
            Widget::Grid => self.panels.show_grid = !self.panels.show_grid,
            Widget::Info => self.panels.show_info = !self.panels.show_info,
            // Only ever opens one: the press that closes a menu is answered
            // by the menu itself, before the widgets underneath are asked.
            // A window with no room for the panel gets no menu rather than a
            // state nothing on screen accounts for.
            Widget::Zoom => {
                let chrome = self.chrome();
                if self.current.is_some()
                    && chrome.popup(Menu::Zoom, self.panels.show_grid).is_some()
                {
                    self.panels.menu = Some(Menu::Zoom);
                }
            }
            Widget::Cell(index) => {
                if let Some(menu) = self.panels.menu.take() {
                    let (image, viewport) = (self.image_size(), self.viewport());
                    menu.choose(index, &mut self.view, image, viewport);
                }
            }
        }
    }

    /// Scrolls the info panel with the wheel.
    ///
    /// `None` when the pointer is not over the panel, and the wheel is the
    /// image's to zoom with; `Some` when it is, whether or not the column
    /// actually moved — a panel with nothing left to scroll to has still
    /// taken the gesture, and must not hand a spin at its last line back to
    /// the image underneath.
    fn scroll_info(&mut self, delta: MouseScrollDelta) -> Option<bool> {
        let panel = self.pointer_over_info()?;

        // A wheel turned away from the reader moves the column up the panel,
        // which is the scroll running down the text.
        let by = match delta {
            MouseScrollDelta::LineDelta(_, lines) => -lines * WHEEL_SCROLL_STEP,
            MouseScrollDelta::PixelDelta(pixels) => -pixels.y as f32,
        };
        Some(self.scroll_info_by(panel, by))
    }

    /// Returns `true` if the wheel changed anything on screen.
    pub(super) fn handle_wheel(&mut self, delta: MouseScrollDelta) -> bool {
        // Same reasoning as `handle_key`: Ctrl+wheel and friends belong to the
        // compositor, and acting on them as well would zoom behind its back.
        if self.pointer.chorded() {
            return false;
        }

        // The info panel takes the wheel while the pointer is over it: a
        // column with more to say than fits is what a wheel is for, and the
        // image behind the panel is not what the gesture was aimed at.
        if let Some(scrolled) = self.scroll_info(delta) {
            return scrolled;
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

    /// A chord bound twice would do whichever came first in the table,
    /// silently. The same key under different modifiers is a different chord.
    #[test]
    fn no_chord_is_bound_twice() {
        let mut seen: Vec<(Mods, KeyName)> = Vec::new();
        for (mods, name) in KEYS
            .iter()
            .flat_map(|binding| binding.keys.iter().map(|(name, _)| (binding.mods, *name)))
        {
            assert!(
                !seen.contains(&(mods, name)),
                "{name:?} with {mods:?} is bound more than once"
            );
            seen.push((mods, name));
        }
    }

    #[test]
    fn keys_resolve_to_their_actions() {
        use winit::keyboard::SmolStr;
        let plain = |text: &str| action_for(&Key::Character(SmolStr::new(text)), PLAIN);
        assert_eq!(plain("q"), Some(Quit));
        assert_eq!(action_for(&Key::Named(NamedKey::Escape), PLAIN), Some(Quit));
        assert_eq!(
            action_for(&Key::Named(NamedKey::PageDown), PLAIN),
            Some(NextFile)
        );
        assert_eq!(plain("E"), Some(Exposure(0.5)));
        assert_eq!(plain("z"), None);
    }

    /// The modifiers pick the chord out from the plain key of the same name,
    /// and a chord nothing binds is still left to the window manager.
    #[test]
    fn modifiers_tell_chords_apart() {
        use winit::keyboard::SmolStr;
        let c = Key::Character(SmolStr::new("C"));
        assert_eq!(action_for(&c, PLAIN), Some(CycleColormap));
        assert_eq!(action_for(&c, Mods::SHIFT), Some(CycleColormap));
        assert_eq!(action_for(&c, CTRL_SHIFT), Some(CopyPath));
        assert_eq!(action_for(&c, Mods::CONTROL), None);
        assert_eq!(action_for(&c, CTRL_SHIFT | Mods::ALT), None);
        assert_eq!(
            action_for(&Key::Character(SmolStr::new("0")), Mods::SUPER),
            None
        );
    }
}
