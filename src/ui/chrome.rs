//! The window chrome: four panels, the controls sitting in them, and what
//! they leave in the middle for the image.
//!
//! The geometry is derived from the window size alone and nothing else, so
//! that the image can be fitted into what the panels leave *before* the
//! interface is laid out: a fit that waited on the toolkit would be a frame
//! behind the window. egui's panels are then given exactly those sizes, and
//! what they hold is laid out inside them.

use egui::{
    Align, Button, CornerRadius, Layout, Rect as Area, Response, RichText, Sense, Ui, Vec2,
    WidgetInfo, WidgetType, pos2, vec2,
};

use crate::view::{Fit, Viewport};

use super::Rect;

use super::control::{Command, Control, Naming};
use super::icon::{self, Mark};
use super::style::TOGGLE_RADIUS;
use super::tooltip::Tip;
use super::{Current, FrameInput, PADDING, Panels, Reading, menu, pixel, status};
use crate::image::display::Headroom;
use crate::theme::Theme;
use crate::view::View;

/// Height of the top and bottom panels.
pub(crate) const BAR_HEIGHT: f32 = 30.0;

/// Width of the left and right panels. The bars' own height: they hold a
/// column of toggles and nothing else, so their width is a button's, and
/// making the four panels the same thickness leaves the picture centered in a
/// frame of even weight.
pub(crate) const SIDE_WIDTH: f32 = BAR_HEIGHT;

/// The square buttons that live in the side panels.
pub(super) const BUTTON_SIZE: f32 = 22.0;
/// The gap between two buttons, whether stacked down a panel or side by side
/// in a bar.
pub(super) const BUTTON_GAP: f32 = 8.0;
/// And the gap between two set against each other instead: a hairline of the
/// bar showing between them, and nothing more.
///
/// The pair that steps through the list is parted by this rather than by
/// [`BUTTON_GAP`], and squared off where they meet. Two buttons that go the
/// two ways of one thing are one control, and a control is not read as one
/// thing with a button's own width of bar down the middle of it.
pub(super) const STEP_SEAM: f32 = 1.0;

/// The margin at the ends of the bars: how far the first thing in one is
/// from the edge of the window.
///
/// The inset that centers a toggle across a side panel, and derived from it
/// rather than merely equal to it, so the two cannot drift apart as either is
/// retuned. That makes the button at the end of a bar and the column of
/// toggles below it share one line down the edge of the window — the whole
/// reason the bars are not inset by [`PADDING`] like the panels that float
/// over the picture.
pub(super) const BAR_PADDING: f32 = (SIDE_WIDTH - BUTTON_SIZE) / 2.0;
/// The zoom readout in the top bar, which is also the button that opens the
/// zoom menu. Wide enough for the longest reading it takes, and fixed so
/// that it does not move as the zoom changes what it reads.
const ZOOM_BUTTON: [f32; 2] = [58.0, 22.0];
/// The gap between the spacing the grid toggle reads out and the mark it
/// belongs to.
const READING_GAP: f32 = 4.0;
/// The room the grid toggle keeps after its reading.
const READING_PAD: f32 = 6.0;
/// The surface switch at the right of the bottom bar: the one word it wears,
/// with the room a button's label keeps around itself.
const OUTPUT_BUTTON: [f32; 2] = [42.0, 22.0];

/// Width of the hairline along a panel's inner edge, in logical pixels. What
/// it is drawn in is the theme's `border`.
const BORDER_WIDTH: f32 = 1.0;

/// The room set aside for a toggle's mark: what [`icon::square`] is given to
/// size a square out of, not the size it comes back with. The side panels
/// are a bar's thickness wide and the buttons fill them, so what this is set
/// against is legibility at that size rather than the button — three logical
/// pixels of air is enough to keep a mark off the button's rounded corners,
/// and every pixel beyond that is one the mark does not have.
pub(super) const ICON_SIDE: f32 = BUTTON_SIZE - 6.0;

/// The window chrome: the four panels, and the fifth a file of frames or
/// pages brings with it.
///
/// Top and bottom span the full width; left and right are nested between
/// them, so the corners belong to the horizontal bars and the vertical ones
/// never have to reason about where a bar ends. The transport bar, where
/// there is one, is a second bar above the bottom one and spans the width as
/// the bars do: the strips end above it.
#[derive(Clone, Copy)]
pub struct Chrome {
    pub top: Rect,
    pub bottom: Rect,
    pub left: Rect,
    pub right: Rect,
    /// The bar of playback controls, above the bottom bar, for an animation
    /// or a file of pages. `None` for a still.
    pub transport: Option<Rect>,
}

impl Chrome {
    /// `size` is the window in logical pixels; `transport` is whether the
    /// file on screen has frames or pages to step through.
    pub fn new(size: [f32; 2], transport: bool) -> Self {
        // An equal share of the window each at the very smallest, so that a
        // window dragged down to nothing shrinks the panels rather than
        // letting the opposite pair pass through each other.
        let bars = if transport { 3.0 } else { 2.0 };
        let bar = BAR_HEIGHT.min(size[1] / bars);
        let side = SIDE_WIDTH.min(size[0] / 2.0);
        let middle = (size[1] - bars * bar).max(0.0);

        Self {
            top: Rect::new(0.0, 0.0, size[0], bar),
            left: Rect::new(0.0, bar, side, middle),
            right: Rect::new(size[0] - side, bar, side, middle),
            transport: transport.then(|| Rect::new(0.0, size[1] - 2.0 * bar, size[0], bar)),
            bottom: Rect::new(0.0, size[1] - bar, size[0], bar),
        }
    }

    /// What the panels leave in the middle: the image is drawn in it, and
    /// anything that floats over the image has to fit in it.
    pub fn content(&self) -> Rect {
        let floor = self
            .transport
            .map_or(self.bottom.y, |transport| transport.y);
        Rect::new(
            self.left.right(),
            self.top.bottom(),
            (self.right.x - self.left.right()).max(0.0),
            (floor - self.top.bottom()).max(0.0),
        )
    }
}

/// What the interface leaves for the image, in logical pixels: the middle
/// when the panels are showing, the whole window when they are not.
/// `transport` is whether the file on screen brings the fifth bar with it.
///
/// With the panels hidden a floating panel still sits in the corner of the
/// window rather than where the panels that are not there would have put it.
pub fn content_area(logical: [f32; 2], show_ui: bool, transport: bool) -> Rect {
    if show_ui {
        Chrome::new(logical, transport).content()
    } else {
        Rect::new(0.0, 0.0, logical[0], logical[1])
    }
}

/// Where the image is drawn, in physical pixels, for a window of `size`
/// physical pixels at `scale`.
///
/// The panels are opaque, so with them on screen the image belongs in what
/// they leave in the middle; with them off it has the window. Nothing caches
/// this, which is why toggling the interface re-fits a fitted image on the
/// very next frame.
pub fn image_viewport(size: [f32; 2], scale: f32, show_ui: bool, transport: bool) -> Viewport {
    if !show_ui {
        return Viewport::whole(size);
    }
    let content = Chrome::new([size[0] / scale, size[1] / scale], transport).content();
    Viewport::new(
        content.x * scale,
        content.y * scale,
        content.width * scale,
        content.height * scale,
    )
}

/// One pass of the interface: what it is drawn from, and what it asks for.
pub(super) struct Pass<'a> {
    pub input: &'a FrameInput,
    pub panels: &'a Panels,
    pub current: Option<&'a Current>,
    pub view: &'a View,
    pub theme: &'a Theme,
    pub namer: &'a dyn Naming,
    pub commands: Vec<Command>,
}

/// Which of a button's corners are turned: all four for one standing on its
/// own, only the outer ones for each end of a row set together, and none for
/// one in the middle of such a row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Corners {
    All,
    Leading,
    Middle,
    Trailing,
}

impl Corners {
    fn radius(self) -> CornerRadius {
        let r = TOGGLE_RADIUS as u8;
        match self {
            Corners::All => CornerRadius::same(r),
            Corners::Leading => CornerRadius {
                nw: r,
                sw: r,
                ne: 0,
                se: 0,
            },
            Corners::Middle => CornerRadius::ZERO,
            Corners::Trailing => CornerRadius {
                nw: 0,
                sw: 0,
                ne: r,
                se: r,
            },
        }
    }
}

impl Pass<'_> {
    pub fn press(&mut self, control: Control) {
        self.commands.push(Command::Press(control));
    }

    /// The four panels, and everything on them; and the transport bar for a
    /// file that has one.
    pub fn bars(&mut self, ui: &mut Ui) {
        let fill: egui::Color32 = self.theme.bar_background.into();
        let frame = egui::Frame::NONE.fill(fill);

        let top = egui::Panel::top("top")
            .exact_size(BAR_HEIGHT)
            .resizable(false)
            .show_separator_line(false)
            .frame(frame)
            .show(ui, |ui| self.top_bar(ui));
        self.hairline(ui, top.response.rect, Edge::Bottom);
        let bottom = egui::Panel::bottom("bottom")
            .exact_size(BAR_HEIGHT)
            .resizable(false)
            .show_separator_line(false)
            .frame(frame)
            .show(ui, |ui| self.bottom_bar(ui));
        self.hairline(ui, bottom.response.rect, Edge::Top);
        // After the bottom bar, so that it nests above it; before the
        // strips, so that they end above it, as `Chrome` lays them out.
        if let Some(transport) = self.input.transport.clone() {
            let bar = egui::Panel::bottom("transport")
                .exact_size(BAR_HEIGHT)
                .resizable(false)
                .show_separator_line(false)
                .frame(frame)
                .show(ui, |ui| super::transport::show(self, ui, &transport));
            self.hairline(ui, bar.response.rect, Edge::Top);
        }
        let left = egui::Panel::left("left")
            .exact_size(SIDE_WIDTH)
            .resizable(false)
            .show_separator_line(false)
            .frame(frame)
            .show(ui, |ui| self.left_strip(ui));
        self.hairline(ui, left.response.rect, Edge::Right);
        let right = egui::Panel::right("right")
            .exact_size(SIDE_WIDTH)
            .resizable(false)
            .show_separator_line(false)
            .frame(frame)
            .show(ui, |ui| self.right_strip(ui));
        self.hairline(ui, right.response.rect, Edge::Left);
    }

    /// The hairline along a panel's inner edge, on the device's own grid so
    /// that it is one pixel wide wherever it lands.
    fn hairline(&self, ui: &Ui, panel: Area, edge: Edge) {
        let ppp = ui.pixels_per_point();
        let width = icon::Grid::new(ppp).line_width(BORDER_WIDTH);
        let snap = |v: f32| icon::Grid::new(ppp).snap(v);
        let line = match edge {
            Edge::Bottom => Area::from_min_size(
                pos2(panel.min.x, snap(panel.max.y - width)),
                vec2(panel.width(), width),
            ),
            Edge::Top => Area::from_min_size(
                pos2(panel.min.x, snap(panel.min.y)),
                vec2(panel.width(), width),
            ),
            Edge::Right => Area::from_min_size(
                pos2(snap(panel.max.x - width), panel.min.y),
                vec2(width, panel.height()),
            ),
            Edge::Left => Area::from_min_size(
                pos2(snap(panel.min.x), panel.min.y),
                vec2(width, panel.height()),
            ),
        };
        ui.painter().rect_filled(line, 0.0, self.theme.border);
    }

    /// The top bar: what the image is. Everything here is a property of the
    /// file, so it is written once when the image opens and does not move
    /// again while it is on screen.
    fn top_bar(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        let dim: egui::Color32 = self.theme.text_dim.into();
        let Some(current) = self.current else {
            // Nothing has been decoded yet. The bars still go down, so that
            // the window reads as the application waiting rather than as a
            // hole, with the file being read where the image's own name will
            // go.
            ui.horizontal_centered(|ui| {
                ui.add_space(BAR_PADDING);
                if let Some(Reading::File(name)) = &self.input.reading {
                    ui.add(
                        egui::Label::new(RichText::new(format!("loading {name}")).color(dim))
                            .truncate(),
                    );
                }
            });
            return;
        };
        let zoom = self.view.zoom(current.size(), self.input.viewport);
        let fills = Fit::Fill.axis(current.size(), self.input.viewport);

        ui.horizontal_centered(|ui| {
            // The end of the bar first, so that the words get what is left.
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(BAR_PADDING);
                let maximize = self.icon_button(
                    ui,
                    icon::MAXIMIZE_2,
                    Control::Maximize,
                    false,
                    true,
                    Corners::All,
                );
                if maximize.clicked() {
                    self.press(Control::Maximize);
                }
                ui.add_space(BUTTON_GAP);
                self.zoom_readout(ui, zoom, fills);
                ui.add_space(PADDING);

                let facts = status::facts(current, |text| measure(ui, text));
                let room = (ui.max_rect().width() / 2.0 - BAR_PADDING * 2.0).max(1.0);
                let facts = status::fit_segments(|text| measure(ui, text), &facts, room);
                ui.add(egui::Label::new(RichText::new(facts).color(dim)).truncate());
                ui.add_space(PADDING);

                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.add_space(BAR_PADDING);
                    status::top_words(self, ui, current);
                });
            });
        });
    }

    /// The zoom readout: what the view is doing now, and one press from a
    /// menu of what it could be doing instead. Lit while that menu is open,
    /// the way a toggle is lit while it is on.
    fn zoom_readout(&mut self, ui: &mut Ui, zoom: f32, fills: crate::view::Axis) {
        let id = egui::Id::new("zoom menu");
        let open = egui::Popup::is_id_open(ui.ctx(), id);
        let response = ui.add_sized(ZOOM_BUTTON, Button::new(menu::percent(zoom)).selected(open));
        response.widget_info(|| {
            WidgetInfo::selected(WidgetType::Button, true, open, Control::Zoom.label())
        });
        let response = self.tooltip(response, Tip::Control(Control::Zoom), true);
        egui::Popup::from_toggle_button_response(&response)
            .id(id)
            .align(egui::RectAlign::BOTTOM_END)
            .gap(PADDING)
            .show(|ui| menu::zoom_cells(self, ui, zoom, fills));
    }

    /// The bottom bar: what is happening to the image. The pointer comes and
    /// goes on its own, and the rest changes as the view is worked.
    fn bottom_bar(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        let Some(current) = self.current else {
            return;
        };
        let zoom = self.view.zoom(current.size(), self.input.viewport);
        let spacing = super::grid_spacing(self.panels.show_grid, zoom, self.input.scale);

        ui.horizontal_centered(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // The surface switch ends the bar, where the button that hides
                // the interface ends the top one.
                ui.add_space(BAR_PADDING);
                self.output_switch(ui);
                ui.add_space(PADDING);
                // What is being done to the picture, up against that switch —
                // and nothing at all where nothing is being done.
                status::state_words(self, ui, current);

                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.add_space(BAR_PADDING);
                    self.grid_toggle(ui, spacing.as_deref());
                    ui.add_space(BUTTON_GAP);
                    self.pixel_dot(ui);
                    ui.add_space(pixel::GAP);
                    pixel::show(self, ui, current);
                });
            });
        });
    }

    /// The headroom switch: one word, lit while the picture is going out
    /// with room above white. Drawn dead rather than left out where there is
    /// nothing to switch to: a control that is sometimes there is a control
    /// that has to be found again.
    fn output_switch(&mut self, ui: &mut Ui) {
        let on = self.input.headroom == Headroom::Above;
        let available = self.input.hdr_available;
        let response = ui.add_enabled_ui(available, |ui| {
            ui.add_sized(OUTPUT_BUTTON, Button::new("HDR").selected(on))
        });
        let response = response.inner;
        let response = self.tooltip(response, Tip::Control(Control::Output), available);
        if response.clicked() {
            self.press(Control::Output);
        }
    }

    /// The grid toggle: the icon always, and — while the grid is on — how far
    /// apart its lines are, written after the mark it qualifies. The mark
    /// stays in the first button's width of the toggle, whether or not there
    /// is a reading after it, so it is in the same place from one press to
    /// the next; the button grows rightwards to make room for the reading.
    fn grid_toggle(&mut self, ui: &mut Ui, spacing: Option<&str>) {
        let font = egui::TextStyle::Button.resolve(ui.style());
        // No ink of its own: the reading is drawn in the button's, which is
        // handed to the painter below. A color set here would be baked into
        // the galley and would win over that one.
        let reading = spacing.map(|spacing| {
            ui.ctx().fonts_mut(|fonts| {
                fonts.layout_no_wrap(
                    spacing.to_string(),
                    font.clone(),
                    egui::Color32::PLACEHOLDER,
                )
            })
        });
        let width = match &reading {
            Some(galley) => BUTTON_SIZE + READING_GAP + galley.size().x + READING_PAD,
            None => BUTTON_SIZE,
        };
        let (rect, response) = ui.allocate_exact_size(vec2(width, BUTTON_SIZE), Sense::CLICK);
        let (background, ink) = self.button_ink(spacing.is_some(), &response, true);
        ui.painter().rect_filled(rect, TOGGLE_RADIUS, background);
        let mark = Area::from_min_size(rect.min, Vec2::splat(BUTTON_SIZE));
        icon::paint(
            ui.painter(),
            icon::GRID_3X3,
            icon::square(icon::Grid::new(ui.pixels_per_point()), mark, ICON_SIDE),
            ink,
            background,
        );
        if let Some(galley) = reading {
            let at = pos2(
                mark.max.x + READING_GAP,
                rect.center().y - galley.size().y / 2.0,
            );
            ui.painter().galley(at, galley, ink);
        }
        response.widget_info(|| {
            WidgetInfo::selected(
                WidgetType::Button,
                true,
                spacing.is_some(),
                Control::Grid.label(),
            )
        });
        let response = self.tooltip(response, Tip::Control(Control::Grid), true);
        if response.clicked() {
            self.press(Control::Grid);
        }
    }

    /// The dot at the head of the pixel readout, which opens the menu of ways
    /// to write a pixel's value. Lit while that menu is open.
    fn pixel_dot(&mut self, ui: &mut Ui) {
        let id = egui::Id::new("pixel menu");
        let open = egui::Popup::is_id_open(ui.ctx(), id);
        let response = self.icon_button(
            ui,
            icon::CIRCLE_DOT,
            Control::PixelFormat,
            open,
            true,
            Corners::All,
        );
        // From the bottom bar, so it stands over its button rather than
        // hanging off the foot of the window.
        egui::Popup::from_toggle_button_response(&response)
            .id(id)
            .align(egui::RectAlign::TOP_START)
            .gap(PADDING)
            .show(|ui| menu::pixel_cells(self, ui));
    }

    /// The button that opens the menu of everything else that can open this
    /// file. Lit while that menu is open, as the copy button above it is.
    ///
    /// Drawn dead where nothing offers to open it — an unusual format, or a
    /// desktop with nothing installed that reads this one — rather than left
    /// out: a button that comes and goes with the file on screen is a button
    /// that has to be found again, and the label on the dead one says why it
    /// is dead where a missing one could say nothing at all. The paste button
    /// below is the other way round for the other reason: what it does is not
    /// about the file at all, and there is nothing for it to explain.
    fn open_button(&mut self, ui: &mut Ui) {
        let id = egui::Id::new("open menu");
        let open = egui::Popup::is_id_open(ui.ctx(), id);
        let enabled = !self.input.openers.is_empty();
        let button = self.icon_button(
            ui,
            icon::EXTERNAL_LINK,
            Control::OpenWith,
            open,
            enabled,
            Corners::All,
        );
        egui::Popup::menu(&button)
            .id(id)
            .align(egui::RectAlign::RIGHT_START)
            .gap(PADDING)
            .show(|ui| menu::open_items(self, ui));
    }

    /// The left strip: the copy button, the open button under it, the region
    /// button under that, the paste button under that while the clipboard
    /// holds a picture, and the minimap toggle up from the foot — in the
    /// corner the minimap itself goes in, and clear of the column coming
    /// down. A window too short for both ends drops the toggle at the foot
    /// rather than standing it on the column.
    ///
    /// The two menu buttons are together at the head of the column because
    /// they are the same gesture: this file, handed to something else. The
    /// region button sits above the paste button rather than after it so
    /// that it stays put as the paste button comes and goes.
    fn left_strip(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        let height = ui.available_height();
        ui.vertical_centered(|ui| {
            ui.add_space(BAR_PADDING);
            let id = egui::Id::new("copy menu");
            let open = egui::Popup::is_id_open(ui.ctx(), id);
            let copy = self.icon_button(ui, icon::COPY, Control::Copy, open, true, Corners::All);
            // From a button in a column, which has its neighbors above and
            // below it and its room to the side.
            egui::Popup::menu(&copy)
                .id(id)
                .align(egui::RectAlign::RIGHT_START)
                .gap(PADDING)
                .show(|ui| menu::copy_items(self, ui));
            ui.add_space(BUTTON_GAP);
            self.open_button(ui);
            ui.add_space(BUTTON_GAP);
            let region = self.icon_button(
                ui,
                icon::CROP,
                Control::Region,
                self.input.selection.is_on(),
                true,
                Corners::All,
            );
            if region.clicked() {
                self.press(Control::Region);
            }
            if self.panels.paste {
                ui.add_space(BUTTON_GAP);
                let paste = self.icon_button(
                    ui,
                    icon::CLIPBOARD,
                    Control::Paste,
                    false,
                    true,
                    Corners::All,
                );
                if paste.clicked() {
                    self.press(Control::Paste);
                }
            }
        });
        // The room the column above keeps, whether or not the paste button
        // is on screen, so the toggle at the foot stays put as the clipboard
        // changes.
        let taken = BAR_PADDING + 4.0 * (BUTTON_SIZE + BUTTON_GAP);
        if taken + BUTTON_SIZE + BAR_PADDING > height {
            return;
        }
        ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
            ui.add_space(BAR_PADDING);
            let minimap = self.icon_button(
                ui,
                icon::SQUARE_SQUARE,
                Control::Minimap,
                self.panels.show_minimap,
                true,
                Corners::All,
            );
            if minimap.clicked() {
                self.press(Control::Minimap);
            }
        });
    }

    /// The right strip: the histogram toggle above the information toggle,
    /// the order the two panels they open are stacked in. Each is dead where
    /// the window has no room for what it opens.
    fn right_strip(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        let room = super::room(
            content_area(
                self.input.logical,
                self.panels.show_ui,
                self.input.transport.is_some(),
            ),
            self.panels,
        );
        ui.vertical_centered(|ui| {
            ui.add_space(BAR_PADDING);
            let histogram = self.icon_button(
                ui,
                icon::CHART_AREA,
                Control::Histogram,
                self.panels.show_histogram,
                room.histogram,
                Corners::All,
            );
            if histogram.clicked() {
                self.press(Control::Histogram);
            }
            ui.add_space(BUTTON_GAP);
            let info = self.icon_button(
                ui,
                icon::INFO,
                Control::Info,
                self.panels.show_info,
                room.info,
                Corners::All,
            );
            if info.clicked() {
                self.press(Control::Info);
            }
        });
    }

    /// A button's background and ink. Active outranks hover: what is on says
    /// more than what the pointer happens to be over. A dead control keeps
    /// the idle ground and enough of the ink to read the mark on it, but not
    /// enough to read as a button that would answer.
    pub fn button_ink(
        &self,
        active: bool,
        response: &Response,
        enabled: bool,
    ) -> (egui::Color32, egui::Color32) {
        let theme = self.theme;
        if !enabled {
            return (
                theme.button_idle.into(),
                theme
                    .text_dim
                    .with_alpha(super::style::DEAD_BUTTON_INK)
                    .into(),
            );
        }
        match (active, response.hovered()) {
            (true, _) => (
                theme
                    .accent
                    .with_alpha(super::style::ACTIVE_BUTTON_WASH)
                    .into(),
                theme.accent.into(),
            ),
            (false, true) => (theme.button_hover.into(), theme.text_primary.into()),
            (false, false) => (theme.button_idle.into(), theme.text_dim.into()),
        }
    }

    /// One square toggle wearing a mark: the button, the corners it is
    /// turned at, and the icon on it. `on` lights it; `enabled` is whether a
    /// press would do anything, and a toggle that would not is drawn dead
    /// and names its reason rather than itself.
    ///
    /// The press is not taken here: the caller reads it off the response,
    /// so that one button can open a menu where another sends a command.
    pub fn icon_button(
        &mut self,
        ui: &mut Ui,
        marks: &[Mark],
        control: Control,
        on: bool,
        enabled: bool,
        corners: Corners,
    ) -> Response {
        // A dead toggle senses nothing but the pointer resting on it: the
        // press is refused, and the label says why.
        let sense = if enabled { Sense::CLICK } else { Sense::HOVER };
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(BUTTON_SIZE), sense);
        let (background, ink) = self.button_ink(on, &response, enabled);
        ui.painter().rect_filled(rect, corners.radius(), background);
        icon::paint(
            ui.painter(),
            marks,
            icon::square(icon::Grid::new(ui.pixels_per_point()), rect, ICON_SIDE),
            ink,
            background,
        );
        response
            .widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, on, control.label()));
        self.tooltip(response, Tip::Control(control), enabled)
    }

    /// Hangs the tooltip for `tip` off `response`: what the thing is called,
    /// and under it the keys that do the same job — or, for a dead control,
    /// why it is dead.
    pub fn tooltip(&self, response: Response, tip: Tip, enabled: bool) -> Response {
        let Some(tooltip) = self.namer.tooltip(tip) else {
            return response;
        };
        let theme = self.theme;
        let show = move |ui: &mut Ui| super::tooltip::show(ui, &tooltip, theme);
        if enabled {
            response.on_hover_ui(show)
        } else {
            response.on_disabled_hover_ui(show)
        }
    }
}

/// Which edge of a panel faces the picture.
#[derive(Clone, Copy)]
enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

/// How wide `text` comes out in the bar's face, for laying words out
/// against what is left of a bar.
pub(super) fn measure(ui: &Ui, text: &str) -> f32 {
    let font = egui::TextStyle::Body.resolve(ui.style());
    ui.ctx().fonts_mut(|fonts| {
        fonts
            .layout_no_wrap(text.to_string(), font, egui::Color32::WHITE)
            .size()
            .x
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::{Fit, View};

    const WINDOW: [f32; 2] = [1000.0, 700.0];

    #[test]
    fn the_side_panels_are_nested_between_the_bars() {
        let chrome = Chrome::new(WINDOW, false);
        assert_eq!(chrome.transport, None);

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
        let content = Chrome::new(WINDOW, false).content();
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

    /// The transport bar is a bar's height taken off the bottom of the
    /// content area, and the strips end above it.
    #[test]
    fn the_transport_bar_takes_a_bar_off_the_bottom() {
        let chrome = Chrome::new(WINDOW, true);
        let transport = chrome.transport.expect("asked for");
        assert_eq!(
            transport,
            Rect::new(0.0, 700.0 - 2.0 * BAR_HEIGHT, 1000.0, BAR_HEIGHT)
        );
        assert_eq!(chrome.left.bottom(), transport.y);
        assert_eq!(chrome.right.bottom(), transport.y);
        assert_eq!(
            chrome.content(),
            Rect::new(
                SIDE_WIDTH,
                BAR_HEIGHT,
                1000.0 - 2.0 * SIDE_WIDTH,
                700.0 - 3.0 * BAR_HEIGHT
            )
        );
        assert_eq!(
            image_viewport([2000.0, 1400.0], 2.0, true, true).height,
            1400.0 - 6.0 * BAR_HEIGHT
        );
    }

    #[test]
    fn a_window_smaller_than_its_own_chrome_stays_within_itself() {
        // Panels are laid out from the window size, so a window dragged down
        // to nothing must not produce rectangles that escape it or run
        // backwards — a negative width would be drawn as a flipped quad.
        for size in [[10.0, 10.0], [0.0, 0.0], [200.0, 20.0]] {
            let chrome = Chrome::new(size, true);
            let transport = chrome.transport.expect("asked for");
            for panel in [
                chrome.top,
                chrome.bottom,
                chrome.left,
                chrome.right,
                transport,
            ] {
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

    #[test]
    fn the_image_is_fitted_between_the_panels_and_re_fitted_without_them() {
        // A 2x window, to catch a conversion that only holds at scale 1.
        let physical = [2000.0, 1400.0];
        let shown = image_viewport(physical, 2.0, true, false);
        assert_eq!(
            shown,
            Viewport::new(
                2.0 * SIDE_WIDTH,
                2.0 * BAR_HEIGHT,
                2000.0 - 4.0 * SIDE_WIDTH,
                1400.0 - 4.0 * BAR_HEIGHT,
            )
        );

        let hidden = image_viewport(physical, 2.0, false, false);
        assert_eq!(hidden, Viewport::whole(physical));

        let view = View::new();
        let image = [900.0, 600.0];
        assert_eq!(view.fit(), Some(Fit::Whole));
        assert!(view.zoom(image, hidden) > view.zoom(image, shown));

        // Fitted between the panels means fitted *inside* them: the image is
        // centered on the content area, not on the window.
        let placement = view.placement(image, shown);
        assert!(placement.x >= shown.x - 0.5);
        assert!(placement.x + placement.width <= shown.x + shown.width + 0.5);
        assert!(placement.y >= shown.y - 0.5);
        assert!(placement.y + placement.height <= shown.y + shown.height + 0.5);
    }
}
