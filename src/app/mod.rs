//! Window lifecycle, key handling, and building each frame's interface.

mod files;
pub mod input;
mod window;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::thread::JoinHandle;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{Window, WindowId};

use crate::image::decode;
use crate::image::display::{Display, Startup};
use crate::loader::{Decoded, Loader, Opened, Ready, Reload, Request};
use crate::render::{HdrPreference, Placement, Rect, Renderer, Scene, Upscale};
use crate::theme::{self, Theme};
use crate::timing;
use crate::ui::chrome::{Chrome, content_area, image_viewport};
use crate::ui::layers::{Hit, Shown};
use crate::ui::{self, Current, FileFacts, FrameInput, Panels, Reading, Tooltips};
use crate::view::{View, Viewport};
use crate::watch::{self, Watch};

use files::{Announce, Files};
use input::{Effect, Pointer};
use window::{file_label, initial_window_size, loading_title, window_title};

/// What the command line asked for, beyond which files to show.
pub struct Options {
    pub overrides: decode::Overrides,
    pub startup: Startup,
    pub hdr: HdrPreference,
    pub histogram: bool,
    pub info: bool,
    pub minimap: bool,
    pub upscale: Upscale,
}

pub struct App {
    files: Files,
    current: Option<Current>,
    startup: Startup,
    hdr: HdrPreference,
    view: View,
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
    pointer: Pointer,
    panels: Panels,
    /// When the label naming what the pointer is resting on opens and closes.
    /// Apart from the panels because it is the one thing on screen that
    /// depends on how long something has been true: `panels.tooltip` is what
    /// it has settled on, and this is the clock behind it.
    tooltips: Tooltips,
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
    ) -> Self {
        let Options {
            overrides,
            startup,
            hdr,
            histogram,
            info,
            minimap,
            upscale,
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
            startup,
            hdr,
            view,
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
                hover: None,
                info_hover: None,
                menu: None,
            },
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
        let limit = chrome.zoom_button(self.grid_spacing().as_deref()).x;
        let (index, count) = (self.files.index(), self.files.len());
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
        ui::bar_tip(renderer, point, bar, limit, &about)
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
        let zoom = self.view.zoom(current.size(), self.viewport());
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
        let point = self.view.placement(image, viewport).image_point(cursor);
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
        if !same_size {
            self.view.reset();
        }
        // A different picture is a different column of words about it, and it
        // is read from the top.
        if !in_place {
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

        let scale = window.scale_factor() as f32;
        let physical = self.window_size();
        let logical = [physical[0] / scale, physical[1] / scale];
        let viewport = self.viewport();
        let placement = self.view.placement(self.image_size(), viewport);

        let pointer = self.pointer_pixel();
        let cursor = self.logical_cursor();
        let thumbnail = self.minimap_placement(logical, scale);
        let minimap = self.minimap_on_screen();
        let reading = self.reading();
        let tooltip = self.tooltip();
        let hdr_output = {
            let output = self.renderer.as_ref().expect("checked above").output();
            output.is_hdr.then_some(output.label)
        };
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
            hdr_output,
            tooltip,
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
                    eprintln!("gamut: {}", crate::escape_controls(&format!("{error:#}")));
                    self.reported_error = true;
                }
            }
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
            // All of them, always: each has a watch that only advances when
            // it is polled.
            let vanished = self.poll_file();
            let relisted = self.poll_directories();
            let retinted = self.poll_theme();
            let offered = self.poll_clipboard();
            if (vanished || relisted || retinted || offered)
                && let Some(window) = &self.window
            {
                window.request_redraw();
            }
        }

        // The pointer resting on a button long enough to be told what it is:
        // the one thing on screen that happens because time passed rather
        // than because anything arrived.
        if self.tooltips.tick(now)
            && let Some(window) = &self.window
        {
            window.request_redraw();
        }

        // Sleep until the next thing with a time on it: the file check, the
        // moment a read that is still going becomes worth mentioning, or the
        // moment a tooltip is due. A read that finishes first wakes us
        // through the proxy instead.
        let mut deadline = self.next_poll;
        if let Some(due) = self.tooltips.deadline() {
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

    /// A file the loader has finished with.
    fn user_event(&mut self, event_loop: &ActiveEventLoop, decoded: Decoded) {
        self.deliver(decoded);
        // Nothing ever reached the screen and nothing else is coming: every
        // file named on the command line failed to decode. Stop, rather than
        // sit in an empty window with nothing on the way.
        if self.current.is_none() && self.files.is_idle() {
            event_loop.exit();
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let size = initial_window_size(event_loop, self.opening_size());
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

        if self.hdr == HdrPreference::On {
            let output = renderer.output();
            eprintln!(
                "gamut: {} \u{2192} {} output{}",
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
        };
        let size = decode::probe(&paths[0]).expect("we just wrote it");
        App::new(
            paths,
            named,
            0,
            size.map(|(w, h)| [w as f32, h as f32]),
            options,
            Loader::detached(),
        )
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
