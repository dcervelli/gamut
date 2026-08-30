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

use crate::image::display::{AutoWindow, Colormap, Display, Startup};
use crate::image::stats::{BINS, COLOUR};
use crate::image::{DecodedImage, Stats, decode};
use crate::loader::{Decoded, Loader, Opened, Ready, Reload, Request};
use crate::render::{
    Backdrop, Blend, Color, Corner, HdrPreference, Popup, PopupGrid, Rect, Renderer, UiFrame,
};
use crate::timing;
use crate::view::{Fit, Placement, Upscale, View, Viewport};
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

/// Height of the top and bottom panels.
const BAR_HEIGHT: f32 = 30.0;
/// Width of the left and right panels. Wide enough for a square button and
/// nothing else, which is the point: they hold tools, not content.
const SIDE_WIDTH: f32 = 50.0;
/// The square buttons that live in the side panels.
const BUTTON_SIZE: f32 = 34.0;
/// The zoom readout at the right of the bottom bar, which is also the button
/// that opens the zoom menu. Wide enough for the longest reading it takes.
const ZOOM_BUTTON: [f32; 2] = [58.0, 22.0];
/// One cell of a popup menu, and the room around them. A cell is a little
/// wider than it is tall because the widest thing in one is "1600%".
const MENU_CELL: [f32; 2] = [56.0, 34.0];
const MENU_GAP: f32 = 6.0;
const MENU_PADDING: f32 = 8.0;
/// The corner radius of a popup's panel, and of the cells inside it.
const MENU_RADIUS: f32 = 8.0;
const CELL_RADIUS: f32 = 5.0;
const TEXT_SIZE: f32 = 13.0;
const PADDING: f32 = 12.0;
/// The largest the minimap's thumbnail may be. It keeps the image's own
/// shape inside this, so a panorama gets a wide short one and a portrait a
/// narrow tall one.
const MINIMAP_SIZE: [f32; 2] = [168.0, 132.0];
/// Below this on either side there is no room for a map worth reading, and
/// the minimap stays off rather than shrinking to a smudge.
const MINIMAP_MIN: f32 = 48.0;
/// The frame drawn in a fit cell of the zoom menu, which the arrows point out
/// to the edges of.
const FIT_ICON: [f32; 2] = [28.0, 22.0];
/// The gap between the histogram panel's edge and its plot.
const HISTOGRAM_INSET: f32 = 10.0;
/// Wide enough that a bin is exactly one logical pixel, which is what keeps
/// the bars evenly spaced instead of some of them landing astride a pixel
/// boundary and coming out fatter than their neighbours.
const HISTOGRAM_SIZE: [f32; 2] = [BINS as f32 + 2.0 * HISTOGRAM_INSET, 130.0];

/// The panels are opaque, not a tint over the image: the image is fitted
/// inside them rather than passing behind them, so there is nothing back
/// there to show through. The same neutral fills the window behind the image,
/// so the two never read as separate surfaces.
const BAR_BACKGROUND: Color = Color::rgb(18, 18, 22);
/// The hairline along a panel's inner edge, and the other square of the
/// checkerboard behind the image. Far enough off the panel colour to place
/// the edge, close enough that neither the line nor the checks become
/// something the eye keeps going back to.
const BORDER: Color = Color::rgb(38, 38, 46);
/// The hairline's width, in logical pixels.
const BORDER_WIDTH: f32 = 1.0;
/// Side of one checkerboard square, in logical pixels. Small enough to read
/// as a texture behind the image rather than as a pattern competing with it.
const CHECKER_SQUARE: f32 = 8.0;
const PANEL_BACKGROUND: Color = Color::rgba(12, 12, 16, 214);
/// A popup's panel, which is more nearly opaque than the panels that float
/// over the image permanently: a menu is what is being read while it is open,
/// and the picture coming through it competes with the choices on it. Not
/// quite opaque, so that it still reads as lying over the image rather than
/// as another piece of the chrome.
const MENU_BACKGROUND: Color = Color::rgba(12, 12, 16, 246);
const BUTTON_IDLE: Color = Color::rgba(255, 255, 255, 20);
const BUTTON_HOVER: Color = Color::rgba(255, 255, 255, 45);
const TEXT_PRIMARY: Color = Color::rgb(238, 238, 238);
const TEXT_DIM: Color = Color::rgb(150, 152, 160);
const ACCENT: Color = Color::rgb(120, 180, 255);
/// The minimap's border, and the wash over the part of the image that is not
/// on screen. Both go over a thumbnail drawn by the image layer, so they are
/// the only things in the interface that have to stay translucent.
const MINIMAP_EDGE: Color = Color::rgba(255, 255, 255, 70);
const MINIMAP_DIM: Color = Color::rgba(6, 6, 10, 150);
// Histogram ink. The colour planes screen over one another, so overlaps come
// out as the additive mix — red over green reads yellow, all three neutral —
// the way a photo editor's RGB histogram does. Luminance goes underneath in
// the neutral the rest of the panel uses.
const HISTOGRAM_LUMA: Color = Color::rgba(150, 152, 160, 200);
/// What the luminance plane drops to once colour planes are drawn over it.
const HISTOGRAM_LUMA_UNDER: u8 = 110;
// Dimmer than they look on their own: screening all three has to land on a
// neutral grey rather than blowing out to white.
const HISTOGRAM_PLANES: [Color; COLOUR] = [
    Color::rgb(184, 44, 44),
    Color::rgb(44, 170, 52),
    Color::rgb(52, 100, 186),
];

/// Something in the interface the pointer can be over and press: a toggle in
/// a side panel, the zoom readout in the bottom bar, or a cell of the menu
/// that readout opens. One value rather than a flag each, so that
/// hit-testing, hover and drawing all go through the same test.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Widget {
    Minimap,
    Histogram,
    Zoom,
    /// A cell of whichever menu is open. Which menu that is is the
    /// application's `menu`, so a cell needs only its place in the grid.
    Cell(usize),
}

/// A popup the interface can have open, and so what it is a menu of.
///
/// One at a time: a second would have to say which of the two a press outside
/// dismisses. Adding another is a variant, the two matches below, and the
/// code that draws its cells — where the panel goes, what a press lands on
/// and how it is dismissed are the same for every menu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Menu {
    Zoom,
}

impl Menu {
    fn items(self) -> usize {
        match self {
            Menu::Zoom => ZOOM_CHOICES.len(),
        }
    }

    fn grid(self) -> PopupGrid {
        let columns = match self {
            // Eight percentages and three fits: two full rows of powers of
            // two, and the fits along the bottom.
            Menu::Zoom => 4,
        };
        PopupGrid {
            cell: MENU_CELL,
            columns,
            gap: MENU_GAP,
            padding: MENU_PADDING,
            margin: PADDING,
            radius: MENU_RADIUS,
        }
    }
}

/// What the zoom menu offers. The order is the order the cells are laid out
/// in, left to right and top to bottom.
const ZOOM_CHOICES: [ZoomChoice; 11] = [
    ZoomChoice::Scale(0.10),
    ZoomChoice::Scale(0.25),
    ZoomChoice::Scale(0.50),
    ZoomChoice::Scale(1.0),
    ZoomChoice::Scale(2.0),
    ZoomChoice::Scale(4.0),
    ZoomChoice::Scale(8.0),
    ZoomChoice::Scale(16.0),
    ZoomChoice::Fit(Fit::Whole),
    ZoomChoice::Fit(Fit::Width),
    ZoomChoice::Fit(Fit::Height),
];

/// One cell of the zoom menu: a zoom to go to, or a fit to hand the view back
/// to.
#[derive(Clone, Copy, PartialEq, Debug)]
enum ZoomChoice {
    Scale(f32),
    Fit(Fit),
}

impl ZoomChoice {
    /// Whether this is what the view is already doing, which is what lights
    /// the cell. A fit is only itself; a scale counts as matched when it is
    /// the zoom on screen and the view is not in a fit that happens to have
    /// landed there, since pressing it would then mean something.
    fn active(self, view: &View, image: [f32; 2], viewport: Viewport) -> bool {
        match self {
            ZoomChoice::Scale(scale) => {
                view.fit().is_none() && (view.zoom(image, viewport) - scale).abs() < scale * 1e-3
            }
            ZoomChoice::Fit(fit) => view.fit() == Some(fit),
        }
    }

    fn apply(self, view: &mut View, image: [f32; 2], viewport: Viewport) {
        match self {
            ZoomChoice::Scale(scale) => view.set_zoom(scale, image, viewport),
            ZoomChoice::Fit(fit) => view.set_fit(fit),
        }
    }
}

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
    /// The minimap toggle, at the top of the left panel.
    minimap_button: Rect,
    /// The histogram toggle, at the top of the right panel.
    histogram_button: Rect,
    /// The zoom readout, at the right of the bottom bar. Fixed width rather
    /// than fitted to what it says, so that it neither moves as the zoom
    /// changes nor has to be measured to be pressed.
    zoom_button: Rect,
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

        let left = Rect::new(0.0, bar, side, middle);
        let right = Rect::new(size[0] - side, bar, side, middle);
        let bottom = Rect::new(0.0, size[1] - bar, size[0], bar);

        Self {
            top: Rect::new(0.0, 0.0, size[0], bar),
            minimap_button: top_button(left),
            histogram_button: top_button(right),
            zoom_button: bar_button(bottom, ZOOM_BUTTON),
            bottom,
            left,
            right,
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

    /// The hairline along each panel's inner edge: the bottom of the top
    /// panel, the right of the left one, and so on.
    ///
    /// Inside the panel rather than beside it, so that adding the line does
    /// not move the edge the image is fitted against.
    fn borders(&self) -> [Rect; 4] {
        let width = BORDER_WIDTH.min(self.top.height).min(self.left.width);
        [
            Rect::new(self.top.x, self.top.bottom() - width, self.top.width, width),
            Rect::new(self.bottom.x, self.bottom.y, self.bottom.width, width),
            Rect::new(
                self.left.right() - width,
                self.left.y,
                width,
                self.left.height,
            ),
            Rect::new(self.right.x, self.right.y, width, self.right.height),
        ]
    }

    /// Which of the widgets fixed to the panels a point lands on, if any.
    /// The cells of an open menu float above these and are tested first, by
    /// [`App::widget_at`].
    fn widget_at(&self, point: [f32; 2]) -> Option<Widget> {
        if self.minimap_button.contains(point) {
            Some(Widget::Minimap)
        } else if self.histogram_button.contains(point) {
            Some(Widget::Histogram)
        } else if self.zoom_button.contains(point) {
            Some(Widget::Zoom)
        } else {
            None
        }
    }

    /// Where `menu` goes when it is open: the lower right of the content
    /// area, over the image and just above the button that opens it.
    ///
    /// `None` when the window has no room for the whole grid, which is also
    /// what keeps the menu from being opened at all in a window that small.
    fn popup(&self, menu: Menu) -> Option<Popup> {
        Popup::new(
            menu.items(),
            menu.grid(),
            self.content(),
            Corner::BottomRight,
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

/// A square button at the top of a side panel. The same inset on all four
/// sides, so it reads as centred in the strip rather than merely fitted into
/// it — until the strip is shorter than that, at which point it goes flush to
/// the top.
fn top_button(panel: Rect) -> Rect {
    let size = BUTTON_SIZE.min(panel.width).min(panel.height);
    let inset = (panel.width - size) / 2.0;
    Rect::new(
        panel.x + inset,
        panel.y + inset.min(panel.height - size),
        size,
        size,
    )
}

/// A button at the right-hand end of a bar, centred across it. Clamped to the
/// bar, so a window dragged narrow shrinks the button rather than pushing it
/// out of the window.
fn bar_button(bar: Rect, size: [f32; 2]) -> Rect {
    let width = size[0].min(bar.width);
    let height = size[1].min(bar.height);
    Rect::new(
        (bar.right() - PADDING - width).max(bar.x),
        bar.y + (bar.height - height) / 2.0,
        width,
        height,
    )
}

/// What the command line asked for, beyond which files to show.
pub struct Options {
    pub overrides: decode::Overrides,
    pub startup: Startup,
    pub hdr: HdrPreference,
    pub histogram: bool,
    pub minimap: bool,
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

/// What [`App::announce_slow_read`] found: a read to say something about now,
/// one to look at again at a given moment, or nothing worth a word.
#[derive(PartialEq, Eq, Debug)]
enum Announce {
    Now,
    Waiting(Instant),
    Nothing,
}

/// What the top bar says about a read that is taking its time.
enum Reading {
    /// Another file, on its way in.
    File(String),
    /// The file already on screen, being read again after something wrote to
    /// it. There is no new name to show, only the fact that we are busy.
    Again,
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
    /// Whether the four panels are on screen. They are opaque and the image
    /// is fitted inside them, so hiding them gives it the whole window.
    show_ui: bool,
    show_histogram: bool,
    /// Whether the minimap is switched on: a thumbnail of the whole image,
    /// with the part of it the viewport is showing marked out. Whether it is
    /// actually on screen is `minimap_on_screen`, which also asks whether
    /// there is anything off screen for it to point out.
    show_minimap: bool,
    /// Which widget the pointer is over. Held rather than recomputed while
    /// drawing so that motion knows when the highlight has changed and a
    /// redraw is actually owed.
    hover: Option<Widget>,
    /// The menu popped up over the interface, if any. It takes every press
    /// while it is open: one on a cell chooses, one anywhere else dismisses
    /// it.
    menu: Option<Menu>,
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
            next_poll: Instant::now() + watch::INTERVAL,
            loader,
            window: None,
            renderer: None,
            modifiers: ModifiersState::empty(),
            cursor: None,
            dragging: false,
            drag_from: None,
            show_ui: true,
            show_histogram: histogram,
            show_minimap: minimap,
            hover: None,
            menu: None,
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
        image_viewport(self.window_size(), self.scale_factor(), self.show_ui)
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
        self.show_minimap && self.view.can_pan(self.image_size(), self.viewport())
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
        let rect = minimap_rect(content_area(logical, self.show_ui), image)?;
        Some(Placement {
            x: rect.x * scale,
            y: rect.y * scale,
            width: rect.width * scale,
            height: rect.height * scale,
            zoom: rect.width * scale / image[0].max(1.0),
            upscale: self.view.upscale(),
        })
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

        let mut format = None;
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
            format = renderer.image_format();
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
            format,
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
                // An open menu takes the key: dismissing a popup is what
                // Escape is for, and quitting out from under one is not what
                // was being asked for.
                if self.menu.take().is_some() {
                    self.update_hover();
                    return true;
                }
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
                self.show_ui = !self.show_ui;
                // The menu is part of the interface, and goes with it.
                self.menu = None;
                self.hover = None;
                // A fitted image re-fits on the next frame: the viewport it is
                // measured against is the one the panels leave, and they have
                // just come or gone.
                return true;
            }
            "h" | "H" => {
                self.press(Widget::Histogram);
                return true;
            }
            "m" | "M" => {
                self.press(Widget::Minimap);
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
            // An open menu comes before the chrome and before the image: a
            // press on a cell chooses and closes, one anywhere off the panel
            // closes and is spent doing exactly that, and one on the panel
            // but between cells lands on nothing at all.
            if let Some(menu) = self.menu {
                let popup = self.chrome().popup(menu);
                match popup.as_ref().and_then(|popup| popup.item_at(point)) {
                    Some(index) => self.press(Widget::Cell(index)),
                    None if popup.as_ref().is_none_or(|popup| !popup.contains(point)) => {
                        self.menu = None;
                    }
                    None => return false,
                }
                // The cell that had the highlight is no longer under the
                // pointer, or no longer there at all.
                self.update_hover();
                return true;
            }

            let chrome = self.chrome();
            if let Some(widget) = chrome.widget_at(point) {
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
        let moved_pixel = self.show_ui && self.pointer_pixel() != was_over;
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
            .filter(|_| self.show_ui)
            .and_then(|point| self.widget_at(point));
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    /// Which widget a point lands on. An open menu floats over the interface,
    /// so its cells are tested instead of what is underneath them — including
    /// the button that opened it, which a press dismisses the menu from
    /// rather than opening a second one.
    fn widget_at(&self, point: [f32; 2]) -> Option<Widget> {
        let chrome = self.chrome();
        match self.menu {
            Some(menu) => chrome
                .popup(menu)
                .and_then(|popup| popup.item_at(point))
                .map(Widget::Cell),
            None => chrome.widget_at(point),
        }
    }

    /// Acts on a press. The keys that stand in for the toggles come through
    /// here too, so that a key and a click cannot drift apart.
    fn press(&mut self, widget: Widget) {
        match widget {
            Widget::Minimap => self.show_minimap = !self.show_minimap,
            Widget::Histogram => self.show_histogram = !self.show_histogram,
            // Only ever opens one: the press that closes a menu is answered
            // by the menu itself, before the widgets underneath are asked.
            // A window with no room for the panel gets no menu rather than a
            // state nothing on screen accounts for.
            Widget::Zoom => {
                if self.current.is_some() && self.chrome().popup(Menu::Zoom).is_some() {
                    self.menu = Some(Menu::Zoom);
                }
            }
            Widget::Cell(index) => {
                if let Some(menu) = self.menu.take() {
                    self.choose(menu, index);
                }
            }
        }
    }

    /// Acts on cell `index` of `menu`. Out-of-range indices cannot arrive —
    /// the popup only hands back cells it laid out — but a menu that has
    /// nothing to say about a cell simply says nothing.
    fn choose(&mut self, menu: Menu, index: usize) {
        let (image, viewport) = (self.image_size(), self.viewport());
        match menu {
            Menu::Zoom => {
                if let Some(choice) = ZOOM_CHOICES.get(index) {
                    choice.apply(&mut self.view, image, viewport);
                }
            }
        }
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
        let pending = self
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

        // Split borrow: the frame builder needs the renderer's font metrics
        // while reading the rest of the application state.
        let renderer = self.renderer.as_mut().expect("checked above");
        let frame = build_ui(
            renderer,
            Layout {
                logical,
                scale,
                viewport,
                index: self.index,
                file_count: self.files.len(),
                show_ui: self.show_ui,
                show_histogram: self.show_histogram,
                show_minimap: self.show_minimap,
                minimap,
                hover: self.hover,
                menu: self.menu,
                pointer,
                pending,
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

        let backdrop = Backdrop {
            base: BAR_BACKGROUND,
            alternate: BORDER,
            square: CHECKER_SQUARE,
        };

        match renderer.render(placement, thumbnail, display, &frame, scale, backdrop) {
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

/// Everything the frame builder needs that is not the image itself.
struct Layout {
    /// Window size in logical pixels, which is what the UI lays out in.
    logical: [f32; 2],
    /// Physical pixels to the logical one, for the few places that have to
    /// land on the device's grid rather than on the layout's.
    scale: f32,
    /// Where the image is drawn, which is what zoom is measured against.
    viewport: Viewport,
    index: usize,
    file_count: usize,
    show_ui: bool,
    show_histogram: bool,
    /// Whether the minimap is switched on, which is what its button shows.
    show_minimap: bool,
    /// Whether the minimap is on screen, which also takes a view with part of
    /// the image off it.
    minimap: bool,
    hover: Option<Widget>,
    /// The menu popped up over the interface, if any.
    menu: Option<Menu>,
    /// The image pixel under the pointer, when it is over one.
    pointer: Option<[u32; 2]>,
    /// The read in progress, once it has taken long enough to be worth saying.
    /// Kept apart from the label rather than replacing it: everything else in
    /// the interface describes the image on screen, and so must that.
    pending: Option<Reading>,
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
    let chrome = Chrome::new(size);

    let Some(current) = current else {
        // Nothing has been decoded yet. The panels still go down, so that the
        // window reads as the application waiting rather than as a hole, with
        // the file being read where the image's own name will go.
        if layout.show_ui {
            for panel in [chrome.top, chrome.bottom, chrome.left, chrome.right] {
                frame.rect(panel, BAR_BACKGROUND);
            }
            if let Some(Reading::File(name)) = &layout.pending {
                frame.text_clipped(
                    [PADDING, text_baseline(chrome.top)],
                    TEXT_SIZE,
                    TEXT_DIM,
                    (chrome.top.width - PADDING * 2.0).max(1.0),
                    format!("loading {name}"),
                );
            }
        }
        return frame;
    };
    let content = content_area(size, layout.show_ui);

    if layout.show_histogram {
        draw_histogram(&mut frame, current, content);
    }
    if layout.minimap {
        draw_minimap(&mut frame, current, view, &layout, content);
    }
    if !layout.show_ui {
        return frame;
    }

    for panel in [chrome.top, chrome.bottom, chrome.left, chrome.right] {
        frame.rect(panel, BAR_BACKGROUND);
    }
    for border in chrome.borders() {
        frame.rect(border, BORDER);
    }

    // Top panel: what the image is. Everything here is a property of the
    // file, so it is written once when the image opens and does not move
    // again while it is on screen.
    let top = chrome.top;
    let top_baseline = text_baseline(top);

    // Least to most disposable, and dropped whole rather than clipped: half
    // of "18333 x 15667" is worse than none of it. Half the bar at most, so
    // that the name it is sharing the bar with keeps the other half.
    let facts = [
        format!("{} \u{00d7} {}", current.image.width, current.image.height),
        describe_pixels(current),
        current.image.color.label(),
    ];
    let facts = fit_segments(renderer, &facts, (top.width / 2.0 - PADDING * 2.0).max(1.0));
    let facts_width = renderer.measure_text(&facts, TEXT_SIZE)[0];
    let facts_x = (top.right() - PADDING - facts_width).max(PADDING);

    frame.text_clipped(
        [PADDING, top_baseline],
        TEXT_SIZE,
        TEXT_PRIMARY,
        (facts_x - PADDING * 2.0).max(1.0),
        top_label(&current.label, layout.pending.as_ref()),
    );
    frame.text([facts_x, top_baseline], TEXT_SIZE, TEXT_DIM, facts);

    draw_minimap_button(
        &mut frame,
        chrome.minimap_button,
        layout.show_minimap,
        layout.hover == Some(Widget::Minimap),
    );
    draw_histogram_button(
        &mut frame,
        chrome.histogram_button,
        layout.show_histogram,
        layout.hover == Some(Widget::Histogram),
    );

    // Bottom panel: what is happening to the image. The pointer comes and
    // goes on its own, and the rest changes as the view is worked.
    let bar = chrome.bottom;
    let baseline = text_baseline(bar);

    draw_zoom_button(
        &mut frame,
        renderer,
        chrome.zoom_button,
        view.zoom(current.size(), layout.viewport),
        layout.menu == Some(Menu::Zoom),
        layout.hover == Some(Widget::Zoom),
    );

    let mut right = describe_state(current, view, &layout);
    if renderer.output().is_hdr {
        right = format!("{}   \u{00b7}   {}", renderer.output().label, right);
    }
    let right_width = renderer.measure_text(&right, TEXT_SIZE)[0];
    let right_x = (chrome.zoom_button.x - PADDING - right_width).max(PADDING);

    if let Some([x, y]) = layout.pointer {
        frame.text_clipped(
            [PADDING, baseline],
            TEXT_SIZE,
            TEXT_PRIMARY,
            (right_x - PADDING * 2.0).max(1.0),
            format!("({x}, {y})"),
        );
    }
    frame.text([right_x, baseline], TEXT_SIZE, TEXT_DIM, right);

    // Last, so that it lies over the panels and over anything floating in the
    // content area: a popup is the thing being looked at while it is open.
    if let Some(menu) = layout.menu
        && let Some(popup) = chrome.popup(menu)
    {
        draw_menu(
            &mut frame,
            renderer,
            &popup,
            menu,
            view,
            current.size(),
            &layout,
        );
    }
    frame
}

/// Draws the open menu: its panel, and a cell for each choice in it.
///
/// The cells are drawn like the toggles in the side panels, and for the same
/// reason: each is a press, and a state it is either in or not.
fn draw_menu(
    frame: &mut UiFrame,
    renderer: &mut Renderer,
    popup: &Popup,
    menu: Menu,
    view: &View,
    image: [f32; 2],
    layout: &Layout,
) {
    popup.draw(frame, MENU_BACKGROUND);
    for (index, cell) in popup.cells() {
        let hover = layout.hover == Some(Widget::Cell(index));
        match menu {
            Menu::Zoom => {
                let choice = ZOOM_CHOICES[index];
                let (background, ink) =
                    button_ink(choice.active(view, image, layout.viewport), hover);
                frame.rounded_rect(cell, CELL_RADIUS, background);
                match choice {
                    ZoomChoice::Scale(scale) => {
                        centred_text(frame, renderer, cell, ink, &percent(scale))
                    }
                    ZoomChoice::Fit(fit) => draw_fit_icon(frame, cell, fit, ink),
                }
            }
        }
    }
}

/// The zoom readout, drawn as the button it is: what the view is doing now,
/// and one press from a menu of what it could be doing instead. Lit while
/// that menu is open, the way a toggle is lit while it is on.
fn draw_zoom_button(
    frame: &mut UiFrame,
    renderer: &mut Renderer,
    rect: Rect,
    zoom: f32,
    open: bool,
    hover: bool,
) {
    // As with the toggles: a window too narrow for the whole button gets no
    // button rather than a label spilling out of one.
    if rect.width < ZOOM_BUTTON[0] || rect.height < ZOOM_BUTTON[1] {
        return;
    }
    let (background, ink) = button_ink(open, hover);
    frame.rounded_rect(rect, CELL_RADIUS, background);
    centred_text(frame, renderer, rect, ink, &percent(zoom));
}

fn percent(zoom: f32) -> String {
    format!("{:.0}%", zoom * 100.0)
}

/// Draws `text` centred in `rect`, the way a button wears its label. Whole
/// logical pixels, since a glyph laid out on a half one is a blurred glyph.
fn centred_text(
    frame: &mut UiFrame,
    renderer: &mut Renderer,
    rect: Rect,
    color: Color,
    text: &str,
) {
    let width = renderer.measure_text(text, TEXT_SIZE)[0];
    frame.text(
        [
            (rect.x + (rect.width - width) / 2.0).round(),
            (rect.y + (rect.height - TEXT_SIZE * 1.3) / 2.0).round(),
        ],
        TEXT_SIZE,
        color,
        text,
    );
}

/// A box of `size` centred in `rect`: where an icon goes in a cell it is not
/// meant to fill.
fn centred(rect: Rect, size: [f32; 2]) -> Rect {
    Rect::new(
        (rect.x + (rect.width - size[0]) / 2.0).round(),
        (rect.y + (rect.height - size[1]) / 2.0).round(),
        size[0].min(rect.width),
        size[1].min(rect.height),
    )
}

/// The three fits, as the frame each of them fills and the directions it
/// fills it in: arrows out to left and right for a fit to the width, up and
/// down for one to the height, and both for the fit that takes in the whole
/// image.
fn draw_fit_icon(frame: &mut UiFrame, cell: Rect, fit: Fit, ink: Color) {
    let icon = centred(cell, FIT_ICON);
    outline(frame, icon, 1.5, ink);
    let inner = icon.inset(3.0, 3.0);
    if fit != Fit::Height {
        double_arrow(frame, inner, true, ink);
    }
    if fit != Fit::Width {
        double_arrow(frame, inner, false, ink);
    }
}

/// A double-headed arrow spanning `rect` along one axis and centred across
/// the other: a shaft with a triangle pointing out at each end.
fn double_arrow(frame: &mut UiFrame, rect: Rect, horizontal: bool, color: Color) {
    /// How far back from the point an arrowhead reaches, and how wide it is
    /// there.
    const HEAD: [f32; 2] = [5.0, 7.0];
    const SHAFT: f32 = 1.5;

    let (span, across) = if horizontal {
        (rect.width, rect.height)
    } else {
        (rect.height, rect.width)
    };
    // Two heads and nothing between them is still an arrow; less than that is
    // a smudge, and the cell is better left with just its frame.
    let head = HEAD[0].min(span / 2.0);
    if span <= 0.0 || across < HEAD[1] {
        return;
    }
    let middle = |low: f32, extent: f32, width: f32| low + (extent - width) / 2.0;

    if horizontal {
        let centre = rect.y + rect.height / 2.0;
        frame.rect(
            Rect::new(
                rect.x + head,
                middle(rect.y, rect.height, SHAFT),
                span - 2.0 * head,
                SHAFT,
            ),
            color,
        );
        for (point, back) in [(rect.x, rect.x + head), (rect.right(), rect.right() - head)] {
            frame.triangle(
                [
                    [point, centre],
                    [back, centre - HEAD[1] / 2.0],
                    [back, centre + HEAD[1] / 2.0],
                ],
                color,
            );
        }
    } else {
        let centre = rect.x + rect.width / 2.0;
        frame.rect(
            Rect::new(
                middle(rect.x, rect.width, SHAFT),
                rect.y + head,
                SHAFT,
                span - 2.0 * head,
            ),
            color,
        );
        for (point, back) in [
            (rect.y, rect.y + head),
            (rect.bottom(), rect.bottom() - head),
        ] {
            frame.triangle(
                [
                    [centre, point],
                    [centre - HEAD[1] / 2.0, back],
                    [centre + HEAD[1] / 2.0, back],
                ],
                color,
            );
        }
    }
}

/// What the interface leaves for the image, in logical pixels: the middle
/// when the panels are showing, the whole window when they are not.
///
/// With the panels hidden a floating panel still sits in the corner of the
/// window rather than where the panels that are not there would have put it.
/// The frame builder and the minimap's placement both lay out against this,
/// which is what keeps the thumbnail under the border drawn around it.
fn content_area(logical: [f32; 2], show_ui: bool) -> Rect {
    if show_ui {
        Chrome::new(logical).content()
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
    let (background, ink) = button_ink(active, hover);
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

/// The name of the image on screen, and after it whatever the loader is busy
/// with when that has taken long enough to notice.
///
/// One string, clipped as one piece: where there is no room for both, the file
/// you are actually looking at is the one worth keeping.
fn top_label(shown: &str, reading: Option<&Reading>) -> String {
    match reading {
        Some(Reading::File(next)) => format!("{shown}, loading {next}"),
        Some(Reading::Again) => format!("{shown}, reloading"),
        None => shown.to_string(),
    }
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
    // Not the percentage: that is the button at the end of the bar, and
    // saying it twice would only make the reader wonder which one to believe.
    let mut parts = vec![view.mode_label().to_string()];

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

/// The minimap toggle: the panel itself in miniature, a frame for the image
/// with the viewport sitting in a corner of it.
fn draw_minimap_button(frame: &mut UiFrame, rect: Rect, active: bool, hover: bool) {
    if rect.width < BUTTON_SIZE {
        return;
    }
    let (background, ink) = button_ink(active, hover);
    frame.rounded_rect(rect, 5.0, background);

    let icon = rect.inset(8.0, 10.0);
    outline(frame, icon, 1.5, ink);
    frame.rect(
        Rect::new(
            icon.x + 3.5,
            icon.y + 3.5,
            icon.width * 0.5,
            icon.height * 0.5,
        ),
        ink,
    );
}

/// A toggle's background and ink. Active outranks hover: what is on says more
/// than what the pointer happens to be over.
fn button_ink(active: bool, hover: bool) -> (Color, Color) {
    match (active, hover) {
        (true, _) => (ACCENT.with_alpha(64), ACCENT),
        (false, true) => (BUTTON_HOVER, TEXT_PRIMARY),
        (false, false) => (BUTTON_IDLE, TEXT_DIM),
    }
}

/// A rectangle drawn as four edges. What is behind an outline stays visible,
/// which is the whole point for anything laid over the minimap: the thumbnail
/// under it belongs to the image layer, and a filled quad would hide it.
fn outline(frame: &mut UiFrame, rect: Rect, thickness: f32, color: Color) {
    let edge = thickness.min(rect.width / 2.0).min(rect.height / 2.0);
    if edge <= 0.0 {
        return;
    }
    let middle = rect.height - 2.0 * edge;
    frame.rect(Rect::new(rect.x, rect.y, rect.width, edge), color);
    frame.rect(
        Rect::new(rect.x, rect.bottom() - edge, rect.width, edge),
        color,
    );
    frame.rect(Rect::new(rect.x, rect.y + edge, edge, middle), color);
    frame.rect(
        Rect::new(rect.right() - edge, rect.y + edge, edge, middle),
        color,
    );
}

/// Where the minimap's thumbnail goes: the image's own shape, fitted into the
/// top-left of `content` and never enlarged past life size, since a map of a
/// thirty-pixel image blown up to fill the box would be a map of nothing.
///
/// `None` when there is no room for one worth reading, which is what keeps it
/// off screen in a window dragged down small.
fn minimap_rect(content: Rect, image: [f32; 2]) -> Option<Rect> {
    if image[0] <= 0.0 || image[1] <= 0.0 {
        return None;
    }
    // A third of the content area at most, as well as the fixed cap: the
    // minimap is a guide to the image, and must not take the room the image
    // itself is being looked at in.
    let room = [
        MINIMAP_SIZE[0].min(content.width / 3.0),
        MINIMAP_SIZE[1].min(content.height / 3.0),
    ];
    if room[0] < MINIMAP_MIN || room[1] < MINIMAP_MIN {
        return None;
    }
    // Whole logical pixels, so the border sits on the thumbnail's edge rather
    // than half a pixel inside it.
    let scale = (room[0] / image[0]).min(room[1] / image[1]).min(1.0);
    let size = [
        (image[0] * scale).round().max(1.0),
        (image[1] * scale).round().max(1.0),
    ];
    Some(Rect::new(
        (content.x + PADDING).round(),
        (content.y + PADDING).round(),
        size[0],
        size[1],
    ))
}

/// The part of `rect` standing for what the viewport is showing.
///
/// The viewport's corners in image pixels, clamped to the image and scaled
/// into the thumbnail. Clamped because a view zoomed out sees past the
/// image's edges, and this marks out part of the image rather than part of
/// the window.
fn minimap_marker(rect: Rect, image: [f32; 2], placement: Placement, viewport: Viewport) -> Rect {
    let corner = |point: [f32; 2]| {
        let point = placement.image_point(point);
        [
            rect.x + (point[0] / image[0]).clamp(0.0, 1.0) * rect.width,
            rect.y + (point[1] / image[1]).clamp(0.0, 1.0) * rect.height,
        ]
    };
    let start = corner([viewport.x, viewport.y]);
    let end = corner([viewport.x + viewport.width, viewport.y + viewport.height]);
    Rect::new(start[0], start[1], end[0] - start[0], end[1] - start[1])
}

/// `rect` with its edges on whole physical pixels.
///
/// The quad shader feathers every edge over a pixel, which is what keeps the
/// interface's corners and thin lines smooth. Two feathered edges that meet
/// part-way through a pixel each cover part of it, and two translucent fills
/// covering a pixel between them do not add up to one covering all of it: the
/// join stays visible as a lighter line. The wash around the marker is four
/// quads meeting along the marker's edges, so those edges go on the grid and
/// the four pieces tile exactly.
fn snap_to_pixels(rect: Rect, scale: f32) -> Rect {
    let snap = |value: f32| (value * scale).round() / scale;
    let x = snap(rect.x);
    let y = snap(rect.y);
    // A marker smaller than a pixel — the view into a very large image — still
    // has to be somewhere on the map, so an edge never rounds onto the one
    // opposite it.
    let right = snap(rect.right()).max(x + 1.0 / scale);
    let bottom = snap(rect.bottom()).max(y + 1.0 / scale);
    Rect::new(x, y, right - x, bottom - y)
}

/// Draws the minimap over the thumbnail the image layer has already put in
/// the top-left of `content`: a border around the whole image, and the part
/// of it the viewport is showing left bright while the rest is washed over.
///
/// Nothing here is filled where the thumbnail shows through, and the frame
/// this draws into is composited over the image layer, so the two halves of
/// the widget meet on screen without either knowing about the other.
///
/// Only called with part of the image off screen — see `minimap_on_screen` —
/// so the marked-out part is always smaller than the thumbnail on at least
/// one axis, and there is always something to wash over.
fn draw_minimap(
    frame: &mut UiFrame,
    current: &Current,
    view: &View,
    layout: &Layout,
    content: Rect,
) {
    let image = current.size();
    let Some(rect) = minimap_rect(content, image) else {
        return;
    };
    outline(frame, rect, 1.0, MINIMAP_EDGE);

    let placement = view.placement(image, layout.viewport);
    let shown = snap_to_pixels(
        minimap_marker(rect, image, placement, layout.viewport),
        layout.scale,
    );

    for aside in [
        Rect::new(rect.x, rect.y, rect.width, shown.y - rect.y),
        Rect::new(
            rect.x,
            shown.bottom(),
            rect.width,
            rect.bottom() - shown.bottom(),
        ),
        Rect::new(rect.x, shown.y, shown.x - rect.x, shown.height),
        Rect::new(
            shown.right(),
            shown.y,
            rect.right() - shown.right(),
            shown.height,
        ),
    ] {
        if aside.width > 0.0 && aside.height > 0.0 {
            frame.rect(aside, MINIMAP_DIM);
        }
    }
    outline(frame, shown, 1.5, ACCENT);
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
    let plotted = &current.stats.plot;
    let luma = &plotted.luma;
    let colour: &[[u32; BINS]] = plotted.colour.as_ref().map_or(&[], |planes| planes);
    let (axis_min, axis_max) = (plotted.min, plotted.max);

    // The axis is in the file's own encoding; the label is not, since the
    // numbers everything else quotes are the decoded ones.
    let transfer = current.image.color.transfer;
    frame.text(
        [plot.x, plot.y],
        TEXT_SIZE * 0.85,
        TEXT_DIM,
        format!(
            "{:.4}  \u{2013}  {:.4}",
            transfer.to_linear(axis_min),
            transfer.to_linear(axis_max)
        ),
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
    // Strictly linear in the counts, the way a photo editor plots it: the
    // height of a bin is its share of the fullest one. A single dominating
    // bin — a nodata background, say — will flatten the rest, which is a
    // measurement-data problem to solve separately.
    let height_of = |count: u32| (count as f32 / peak) * bars.height;
    // One point per bin, at its centre, with the ends carried out to the
    // edges of the plot so the shape fills its width.
    let curve = |counts: &[u32; BINS]| -> Vec<[f32; 2]> {
        counts
            .iter()
            .enumerate()
            .map(|(index, &count)| {
                let x = match index {
                    0 => bars.x,
                    last if last == BINS - 1 => bars.right(),
                    _ => bars.x + (index as f32 + 0.5) * bin_width,
                };
                [x, bars.bottom() - height_of(count)]
            })
            .collect()
    };

    // Dimmed only when it is a backdrop; on a grey image it is the plot.
    let luma_ink = if colour.is_empty() {
        HISTOGRAM_LUMA
    } else {
        HISTOGRAM_LUMA.with_alpha(HISTOGRAM_LUMA_UNDER)
    };
    frame.area(&curve(luma), bars.bottom(), luma_ink, Blend::Over);
    for (counts, color) in colour.iter().zip(HISTOGRAM_PLANES) {
        frame.area(&curve(counts), bars.bottom(), color, Blend::Screen);
    }

    // Where the display window sits within the plotted range.
    let span = axis_max - axis_min;
    if span > 0.0 {
        for value in [current.display.low, current.display.high] {
            let encoded = transfer.to_encoded(value);
            let position = ((encoded - axis_min) / span).clamp(0.0, 1.0);
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
    fn the_minimap_toggle_sits_inside_the_left_panel() {
        let chrome = Chrome::new(WINDOW);
        let button = chrome.minimap_button;

        assert!(button.x >= chrome.left.x);
        assert!(button.right() <= chrome.left.right());
        assert!(button.y >= chrome.left.y);
        assert!(button.bottom() <= chrome.left.bottom());

        // The two toggles are the same button on opposite strips, and each
        // click lands on its own.
        assert_eq!(button.width, chrome.histogram_button.width);
        assert_eq!(button.y, chrome.histogram_button.y);
        assert_eq!(
            chrome.widget_at([button.x + 1.0, button.y + 1.0]),
            Some(Widget::Minimap)
        );
        assert_eq!(
            chrome.widget_at([
                chrome.histogram_button.x + 1.0,
                chrome.histogram_button.y + 1.0
            ]),
            Some(Widget::Histogram)
        );
        assert_eq!(chrome.widget_at([WINDOW[0] / 2.0, WINDOW[1] / 2.0]), None);
    }

    #[test]
    fn the_zoom_readout_is_a_button_at_the_end_of_the_bottom_bar() {
        let chrome = Chrome::new(WINDOW);
        let button = chrome.zoom_button;

        assert_eq!(button.width, ZOOM_BUTTON[0]);
        assert_eq!(button.right(), chrome.bottom.right() - PADDING);
        // Centred across the bar, and inside it.
        assert_eq!(
            button.y - chrome.bottom.y,
            chrome.bottom.bottom() - button.bottom()
        );
        assert!(button.y >= chrome.bottom.y && button.bottom() <= chrome.bottom.bottom());

        assert_eq!(
            chrome.widget_at([button.x + 1.0, button.y + 1.0]),
            Some(Widget::Zoom)
        );
        // The bar it sits in is still the interface, so a press beside it
        // does not reach the image behind.
        assert_eq!(chrome.widget_at([button.x - 2.0, button.y + 1.0]), None);
        assert!(chrome.contains([button.x - 2.0, button.y + 1.0]));
    }

    #[test]
    fn the_zoom_menu_pops_up_in_the_lower_right_of_the_content_area() {
        let chrome = Chrome::new(WINDOW);
        let content = chrome.content();
        let popup = chrome.popup(Menu::Zoom).expect("a window with room for it");

        assert_eq!(popup.cells().count(), ZOOM_CHOICES.len());
        // Over the image, clear of the panels: the menu is drawn on the frame
        // the image is in, and half of it under the bottom bar would be half
        // a menu.
        let panel = popup.panel();
        assert!(panel.x >= content.x && panel.right() <= content.right());
        assert!(panel.y >= content.y && panel.bottom() <= content.bottom());
        // In the corner nearest the button that opens it.
        assert_eq!(panel.right(), content.right() - PADDING);
        assert_eq!(panel.bottom(), content.bottom() - PADDING);

        // A window with no room for the whole of it gets no menu at all,
        // which is also what stops one being opened there.
        assert!(Chrome::new([220.0, 200.0]).popup(Menu::Zoom).is_none());
    }

    /// What a cell says it does is what pressing it does: the state each one
    /// puts the view in is the state that lights that cell and no other.
    #[test]
    fn every_zoom_choice_lands_on_itself() {
        let image = [900.0, 600.0];
        let viewport = Viewport::whole(WINDOW);

        for choice in ZOOM_CHOICES {
            let mut view = View::new();
            choice.apply(&mut view, image, viewport);
            assert!(choice.active(&view, image, viewport), "{choice:?}");

            for other in ZOOM_CHOICES {
                assert_eq!(
                    other.active(&view, image, viewport),
                    other == choice,
                    "{other:?} after {choice:?}"
                );
            }
            if let ZoomChoice::Scale(scale) = choice {
                assert!((view.zoom(image, viewport) - scale).abs() < 1e-4);
            }
        }
    }

    /// The button reads out the same zoom the cells are chosen from, so the
    /// two have to agree on how a zoom is written down.
    #[test]
    fn the_readout_is_written_the_way_the_menu_writes_it() {
        assert_eq!(percent(0.1), "10%");
        assert_eq!(percent(1.0), "100%");
        assert_eq!(percent(16.0), "1600%");
        let widest = ZOOM_CHOICES
            .iter()
            .filter_map(|choice| match choice {
                ZoomChoice::Scale(scale) => Some(percent(*scale).len()),
                ZoomChoice::Fit(_) => None,
            })
            .max();
        assert_eq!(widest, Some("1600%".len()));
    }

    /// The thumbnail is the image in miniature, so its shape is the image's
    /// and not the box it is fitted into.
    #[test]
    fn the_minimap_keeps_the_image_shape_and_never_enlarges_it() {
        let content = Chrome::new(WINDOW).content();

        let wide = minimap_rect(content, [4000.0, 1000.0]).expect("room in a 1000x700 window");
        assert!(wide.width <= MINIMAP_SIZE[0] && wide.height <= MINIMAP_SIZE[1]);
        assert!((wide.width / wide.height - 4.0).abs() < 0.1);

        let tall = minimap_rect(content, [1000.0, 4000.0]).expect("room in a 1000x700 window");
        assert!(tall.width <= MINIMAP_SIZE[0] && tall.height <= MINIMAP_SIZE[1]);
        assert!((tall.height / tall.width - 4.0).abs() < 0.1);

        // Life size at most: a tiny image gets a tiny map.
        assert_eq!(
            minimap_rect(content, [24.0, 18.0]),
            Some(Rect::new(
                (content.x + PADDING).round(),
                (content.y + PADDING).round(),
                24.0,
                18.0
            ))
        );

        // Top-left of the content area, and clear of its far edges.
        let rect = minimap_rect(content, [4000.0, 1000.0]).expect("room");
        assert!(rect.x >= content.x + PADDING - 0.5);
        assert!(rect.y >= content.y + PADDING - 0.5);
        assert!(rect.right() < content.right() && rect.bottom() < content.bottom());

        // And nothing at all when the window has no room to spare: a map
        // taking a third of a small content area would be in the way.
        assert_eq!(
            minimap_rect(Chrome::new([200.0, 160.0]).content(), [800.0, 600.0]),
            None
        );
    }

    /// What the marker is for: it says where you are, so it has to agree with
    /// the view it is drawn from.
    #[test]
    fn the_minimap_marker_follows_the_viewport() {
        let image = [800.0, 600.0];
        let viewport = Viewport::whole(WINDOW);
        let rect = Rect::new(100.0, 20.0, 160.0, 120.0);
        let close = |a: f32, b: f32| (a - b).abs() < 0.5;

        // Fitted, the whole image is on screen and the marker covers the map.
        let view = View::new();
        let marker = minimap_marker(rect, image, view.placement(image, viewport), viewport);
        assert_eq!(marker, rect);

        // At 1:1 in a window half the image's size, half of it in each
        // direction is on screen, and centred that is the middle of the map.
        let half = Viewport::whole([400.0, 300.0]);
        let mut view = View::new();
        view.actual_size(image, half);
        let marker = minimap_marker(rect, image, view.placement(image, half), half);
        assert!(close(marker.width, rect.width / 2.0), "{marker:?}");
        assert!(close(marker.height, rect.height / 2.0), "{marker:?}");
        assert!(close(
            marker.x + marker.width / 2.0,
            rect.x + rect.width / 2.0
        ));

        // Panned into the top-left corner it goes to the corner of the map,
        // and stops there rather than running off it.
        view.pan_by(-10_000.0, -10_000.0, image, half);
        let marker = minimap_marker(rect, image, view.placement(image, half), half);
        assert!(
            close(marker.x, rect.x) && close(marker.y, rect.y),
            "{marker:?}"
        );
        assert!(marker.right() <= rect.right() + 0.5 && marker.bottom() <= rect.bottom() + 0.5);
    }

    /// The wash around the marker is four quads meeting along its edges, and
    /// feathered edges only tile without a seam where they fall on the device
    /// grid.
    #[test]
    fn the_marker_lands_on_whole_physical_pixels() {
        for scale in [1.0, 1.5, 2.0] {
            let snapped = snap_to_pixels(Rect::new(10.3, 20.7, 40.4, 30.9), scale);
            for edge in [snapped.x, snapped.y, snapped.right(), snapped.bottom()] {
                let physical = edge * scale;
                assert!(
                    (physical - physical.round()).abs() < 1e-3,
                    "{edge} at scale {scale} is not on the grid"
                );
            }
            // Rounded to the nearest pixel rather than grown to cover one.
            assert!((snapped.x - 10.0).abs() <= 1.0 / scale);
        }

        // The view into a very large image marks out less than a pixel of the
        // map, and still has to be somewhere on it.
        let thin = snap_to_pixels(Rect::new(10.1, 20.1, 0.05, 0.05), 2.0);
        assert_eq!(thin.width, 0.5);
        assert_eq!(thin.height, 0.5);
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
            for button in [
                chrome.minimap_button,
                chrome.histogram_button,
                chrome.zoom_button,
            ] {
                assert!(
                    button.width >= 0.0 && button.height >= 0.0,
                    "{button:?} at {size:?}"
                );
                assert!(button.x >= 0.0 && button.y >= 0.0, "{button:?} at {size:?}");
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

    /// The bar names the image on screen first and always. A file on its way
    /// in is mentioned after it, never in place of it: captioning one picture
    /// with another's name is the one thing an image viewer must not do.
    #[test]
    fn the_bar_names_what_is_on_screen_before_what_is_coming() {
        assert_eq!(top_label("a.png", None), "a.png");
        assert_eq!(
            top_label("a.png", Some(&Reading::File("b.heic".into()))),
            "a.png, loading b.heic"
        );
        assert_eq!(
            top_label("a.png", Some(&Reading::Again)),
            "a.png, reloading",
            "a file being re-read has no new name to show, only the wait"
        );
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
