//! Window lifecycle, key handling, and building each frame's interface.

mod chooser;
mod edits;
mod files;
mod gui;
pub mod input;
mod kept;
mod playback;
mod window;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::{KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{Window, WindowId};

use crate::image::DecodedImage;
use crate::image::decode;
use crate::image::display::{Display, Headroom, Startup};
use crate::image::region::{Grip, Region};
use crate::image::sequence::Sequence;
use crate::loader::{Decoded, Loader, Opened, Ready, Reload, Request, Source};
use crate::monitor::{Mode, Monitors};
use crate::motion::Motion;
use crate::openers::{self, Opener};
use crate::player::{self, Player};
use crate::portal::{self, Pick, Picked};
use crate::render::{GpuImage, HdrPreference, Placement, Renderer, Scene, Upscale};
use crate::theme::{self, Theme};
use crate::thumbnailer::{Delivered, Facts, Thumb, Thumbnailer};
use crate::timing;
use crate::trash::Trash;
use crate::ui::chrome::{content_area, image_viewport};
use crate::ui::toast::{self, Level, Toasts};
use crate::ui::tooltip::Hdr;
use crate::ui::{self, Current, FileFacts, FrameInput, Panels, Reading, Rect, Selection};
use crate::view::{View, Viewport};
use crate::watch::{self, Watch};

use chooser::{Chooser, Thumbs};
use edits::{Edit, Renaming};
use files::{Announce, Files};
use gui::Gui;
use input::{Effect, Framing, Grabbing, Pointer};
use kept::{Kept, Left, Settings};
use playback::Playback;
use window::{file_label, initial_window_size, loading_title, window_title};

/// What wakes the event loop from another thread.
pub enum UserEvent {
    /// A file the loader has finished with. Boxed: it carries the pixels,
    /// and the other variant carries nothing.
    Decoded(Box<Decoded>),
    /// A monitor's mode was learned, or changed.
    Monitor,
    /// A player's cache changed: a frame arrived, or the decoder gave up.
    Frame(player::Event),
    /// The thumbnail thread has something to say about a file. Boxed for
    /// the variant that carries pixels.
    Thumbnail(Box<Delivered>),
    /// The desktop's file dialog has come down: what was chosen in it, if
    /// anything.
    Picked(Picked),
}

/// The other threads, and how each reaches the loop: made in `main` from
/// the event loop, which is the only place a proxy to it can come from.
pub struct Threads {
    pub loader: Loader,
    /// How a player wakes the loop; one is started per animated file.
    pub wake: player::Wake,
    pub monitors: Option<Monitors>,
    /// The thread making the chooser's thumbnails, over the whole session.
    pub thumbnailer: Thumbnailer,
    /// How the desktop's file dialog hands its answer back.
    pub picker: portal::Deliver,
}

/// The file the command line asked for first, for the window to open on:
/// where it stands in the list, where its bytes come from, and the size
/// its header claims. `None` for a program opened on nothing, whose window
/// opens on the buttons that give it something.
pub struct Opening {
    /// Which file of the list: the first whose header could be read, which
    /// is not necessarily the first named.
    pub index: usize,
    /// Where its bytes come from: the clipboard for `--paste`, whose file
    /// is the empty one reserved for it until the loader has fetched the
    /// picture into it.
    pub source: Source,
    /// What its header said its size was, where it would say: enough to
    /// open the window at the right shape before the pixels exist.
    pub size: Option<[f32; 2]>,
}

/// What the command line asked for, beyond which files to show.
pub struct Options {
    pub overrides: decode::Overrides,
    pub startup: Startup,
    pub hdr: HdrPreference,
    pub histogram: bool,
    pub info: bool,
    pub minimap: bool,
    pub upscale: Upscale,
    /// What `--size` asked the window to open at, in logical pixels.
    pub size: Option<[u32; 2]>,
    /// Whether an animation opens stopped on its first frame.
    pub paused: bool,
}

/// What a copy prepared on a thread of its own did: took the selection, and
/// this is what to say about it, or failed with this much to say about it.
type CopyOutcome = Result<&'static str, String>;

pub struct App {
    files: Files,
    current: Option<Current>,
    startup: Startup,
    /// What was asked of the surface: by `--output`, and by every press of
    /// the switch since. Read against the monitor's mode in
    /// [`App::surface_hdr`] and [`App::headroom`].
    hdr: HdrPreference,
    /// The compositor's word on the monitors, where it gives one.
    monitors: Option<Monitors>,
    /// The mode of the monitor the window is on, as last read: `None` until
    /// the window has landed on one, and for good where nothing says.
    monitor: Option<Mode>,
    /// Where the view is going: the pan and zoom every key and press act
    /// on. What is on screen is [`App::shown_view`], which is this once it
    /// has arrived.
    view: View,
    /// The move the view is in the middle of, from where it was shown when
    /// the last animated change was asked for to wherever `view` now says.
    /// `None` once it has landed, and while nothing is moving.
    motion: Option<Motion>,
    /// What each file that has been on screen was left in, so that stepping
    /// back to one puts it back rather than opening it afresh: the view, and
    /// everything the display is doing to it.
    kept: Kept,
    /// The file on screen, watched for writes by anything else.
    watch: Watch,
    /// What else on the desktop can open that file, read as it goes up: the
    /// menu under the open button, and — by being empty or not — whether that
    /// button answers at all.
    ///
    /// Once per file rather than once per frame. It is a handful of small
    /// files to read and a millisecond or two to read them, which is nothing
    /// beside decoding the picture and far too much to do sixty times a
    /// second. A program installed while the window is open therefore joins
    /// the menu at the next file rather than the next frame, which is as
    /// close to the moment as anything short of watching the whole desktop
    /// for changes could get.
    openers: Vec<Opener>,
    /// The paths as the command line gave them, and a watch on each directory
    /// among them. A directory is a place to look rather than a fixed list:
    /// images appearing in it or disappearing from it while the window is open
    /// join or leave the walk, noticed on the same cadence as a write to the
    /// file on screen. Empty — and so costing nothing — when every path named
    /// was a file.
    named: Vec<PathBuf>,
    directories: Vec<Watch>,
    /// The colors everything is drawn in, and the palette file they came
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
    /// The size `--size` asked the window to open at, in logical pixels, if it
    /// asked for one. Read once, when the window is made; every size after
    /// that is the compositor's to give.
    asked_size: Option<[u32; 2]>,
    /// The thread that reads files.
    ///
    /// Declared before the renderer on purpose: fields are dropped in the
    /// order they are written, and the loader has been given a handle on the
    /// GPU device. Shutting the thread down first means the last reference to
    /// that device is the renderer's, and so the device is destroyed here on
    /// the thread that made it. See [`Loader::drop`] for what goes wrong when
    /// the thread is still running as the process leaves `main`.
    loader: Loader,
    /// The thread decoding the frames of the animation on screen, and the
    /// cache it fills. `None` for a still or a paged file. Before the
    /// renderer for the loader's reason: it has no handle on the device,
    /// but a thread still decoding as the process leaves `main` is a thread
    /// to have joined.
    player: Option<Player>,
    /// The clock the animation on screen plays by. Beside `player`: one
    /// without the other is never the case.
    playback: Option<Playback>,
    /// The thread making thumbnails of every file on the list, for the
    /// chooser. Told to stop rather than joined — see its own account.
    thumbnailer: Thumbnailer,
    /// The chooser's state: the query, which files fit it, what is known
    /// about each. Whether the popup is open is egui's — see
    /// [`App::chooser_open`].
    chooser: Chooser,
    /// The thumbnails the screen holds, as egui textures.
    thumbs: Thumbs,
    /// Thumbnails that arrived before there was a context to make textures
    /// in, taken up at the first frame.
    pending_thumbs: Vec<(PathBuf, Thumb)>,
    /// Which frame the texture and `current` hold. `None` for the file's
    /// own decode, which is what a still is and what an animation opens as.
    uploaded: Option<usize>,
    /// How a player wakes the loop: what `main` made from the loop's proxy.
    wake: player::Wake,
    /// Numbers the players, so that news from one dropped with its file is
    /// told from the one now playing.
    players: u64,
    /// Whether the player's failure, if it failed, has been reported.
    player_failed: bool,
    /// Whether an animation opens stopped on its first frame: `--paused`.
    open_paused: bool,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    /// The toolkit's context and its adapter to the window, made with them.
    gui: Option<Gui>,
    pointer: Pointer,
    panels: Panels,
    /// The region on the picture: off, asked for, or drawn. What the arrows
    /// move, `Space` fits and `Ctrl+C` copies while one is on screen.
    selection: Selection,
    /// The current handle of the region: the one the arrows move, and the
    /// one lit apart from the others. The middle — the whole region — until
    /// a handle is clicked or dragged, and again for every new region.
    handle: Grip,
    /// The hold a drag on the picture has on it, from the press to the
    /// release, while the drag is the region's rather than the view's.
    grabbing: Option<Grabbing>,
    /// What `Space` frames next while a region is up: the region at either
    /// fit, then the picture at either. Its own rather than the view's,
    /// since a fit of the region is not a fit the view keeps.
    framing: Framing,
    /// The box being dragged out to zoom to, with `Space` held, while the
    /// drag is under way: painted over the picture, and what the view goes
    /// to when the drag lets go.
    zoom_box: Option<Region>,
    /// The message about what was just done, and when it takes itself off.
    /// The one thing on screen that time alone changes.
    toasts: Toasts,
    /// How the copies being prepared on threads of their own turned out. A
    /// copy of the picture has to walk every pixel before it can say whether
    /// it worked, and the thread doing that has no business touching the
    /// interface — so it sends the outcome here, and the loop picks it up on
    /// the same cadence it looks at the file, the palette and the clipboard
    /// on. `Err` carries the one line the window shows; the whole chain has
    /// already gone to the terminal.
    copied: (mpsc::Sender<CopyOutcome>, mpsc::Receiver<CopyOutcome>),
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
    /// Where a deleted file goes: the desktop's trash, where the file
    /// manager shows it. `None` where there is no way to find it — no home
    /// directory — in which case a deletion is refused rather than done
    /// some other way.
    trash: Option<Trash>,
    /// What has been done to files on disk this session, last first, for
    /// undo — see [`edits`].
    edits: Vec<Edit>,
    /// The rename dialog, while it is up.
    renaming: Option<Renaming>,
    /// How the desktop's file dialog hands its answer back: what `main`
    /// made from the loop's proxy.
    picker: portal::Deliver,
    /// Whether that dialog is up. One at a time: the buttons that open it
    /// are drawn dead while it is, and the key does nothing.
    picking: bool,
    /// Whether the list is still the one the command line gave, with
    /// nothing opened from the window since. A command line whose every
    /// file fails to decode is a command line to answer by leaving, with a
    /// failing status; a choice made in the window that fails is answered
    /// in the window, which stays up for the next choice.
    from_command_line: bool,
    /// Whether the window has already said how to bring the interface back.
    /// The message goes up the first time the bars are hidden and not again:
    /// with them gone there is nothing on screen that could say it, and a
    /// message every time would be in the way of the picture that was just
    /// asked for. Here rather than in [`Panels`] because it is what has
    /// happened, not what is on screen.
    said_how_to_restore: bool,
    /// Set if the last render failed, so we report it once rather than every frame.
    reported_error: bool,
}

impl App {
    /// `opening` is the file to open on, if the command line named any: it
    /// is asked for here, so that it is being read while the window and the
    /// GPU are still being set up. With `None` the list is empty and the
    /// window opens on nothing but the buttons that give it something.
    ///
    /// `named` is the command line's own list, `files` before any directory in
    /// it was replaced by the images inside. Kept so that those directories
    /// can be looked at again and the list built from them anew.
    pub fn new(
        files: Vec<PathBuf>,
        named: Vec<PathBuf>,
        opening: Option<Opening>,
        options: Options,
        threads: Threads,
    ) -> Self {
        let Threads {
            loader,
            wake,
            monitors,
            thumbnailer,
            picker,
        } = threads;
        let Options {
            overrides,
            startup,
            hdr,
            histogram,
            info,
            minimap,
            upscale,
            size: asked_size,
            paused,
        } = options;
        let (index, source, size) = match opening {
            Some(Opening {
                index,
                source,
                size,
            }) => (index, Some(source), size),
            None => (0, None, None),
        };
        let watch = match files.get(index) {
            Some(path) => Watch::new(path),
            None => Watch::idle(),
        };
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
            asked_size,
            startup,
            hdr,
            monitors,
            monitor: None,
            view,
            motion: None,
            kept: Kept::default(),
            watch,
            openers: Vec::new(),
            named,
            directories,
            theme: Theme::detect(),
            theme_watch,
            next_poll: Instant::now() + watch::INTERVAL,
            loader,
            player: None,
            playback: None,
            thumbnailer,
            chooser: Chooser::default(),
            thumbs: Thumbs::default(),
            pending_thumbs: Vec::new(),
            uploaded: None,
            wake,
            players: 0,
            player_failed: false,
            open_paused: paused,
            window: None,
            renderer: None,
            gui: None,
            pointer: Pointer::default(),
            selection: Selection::Off,
            handle: Grip::Middle,
            grabbing: None,
            framing: Framing::FIRST,
            zoom_box: None,
            toasts: Toasts::default(),
            copied: mpsc::channel(),
            copying: Vec::new(),
            copies: Arc::new(AtomicU64::new(0)),
            panels: Panels {
                show_ui: true,
                show_histogram: histogram,
                show_luma: true,
                show_planes: true,
                log_counts: false,
                mark_clipped: false,
                show_info: info,
                show_minimap: minimap,
                show_grid: false,
                paste: false,
                pixel_format: ui::PixelFormat::default(),
            },
            trash: Trash::detect(),
            edits: Vec::new(),
            renaming: None,
            picker,
            picking: false,
            from_command_line: source.is_some(),
            said_how_to_restore: false,
            reported_error: false,
        };
        if let Some(source) = source {
            let request = app.files.open_first(source);
            app.send(request);
        }
        // The whole list, from the start: the cache fills while the first
        // file is being looked at, and the chooser then has thumbnails the
        // moment it opens.
        app.thumbnailer.enqueue(app.files.paths().to_vec());
        app
    }

    /// Puts up the desktop's file dialog, for files or for a folder, unless
    /// it is up already. What it answers arrives as [`UserEvent::Picked`].
    pub(super) fn pick(&mut self, pick: Pick) {
        if self.picking {
            return;
        }
        self.picking = true;
        self.close_menus();
        portal::choose_on_thread(pick, Arc::clone(&self.picker));
    }

    /// Takes in what the dialog answered.
    fn picked(&mut self, picked: Picked) {
        self.picking = false;
        match picked.outcome {
            Ok(Some(paths)) => self.open_named(paths),
            // Dismissed: nothing was asked for.
            Ok(None) => {}
            Err(error) => {
                input::report(&error);
                self.toast(input::briefly(&error), Level::Error);
            }
        }
    }

    /// Opens `named` as a command line naming them beside what was already
    /// named would have: a directory among them stands for the images
    /// inside it, and what they come to joins the end of the list. The
    /// first of the newcomers is asked for as a walk, so a file that will
    /// not decode is stepped over as it is at start-up; the picture on
    /// screen stays up until it arrives, as it does for a step.
    ///
    /// The names join `named` too — those not already there — so that a
    /// directory chosen is watched, and a rebuild of the list puts the
    /// newcomers back in the order they were chosen in.
    pub(super) fn open_named(&mut self, named: Vec<PathBuf>) {
        let files = match crate::listing::expand(named.clone()) {
            Ok(files) => files,
            Err(error) => {
                input::report(&error);
                self.toast(input::briefly(&error), Level::Warning);
                return;
            }
        };
        self.from_command_line = false;
        for path in named {
            if self.named.contains(&path) {
                continue;
            }
            if path.is_dir() {
                self.directories.push(Watch::new(&path));
            }
            self.named.push(path);
        }
        if let Some(request) = self.files.append(files) {
            self.send(request);
        }
        self.list_changed();
    }

    /// Keeps what the picture on screen was left in — its view, its
    /// display, and the frame or page it was on — under the file's path,
    /// so that stepping back to it puts it back.
    fn keep_shown(&mut self) {
        let (Some(current), Some(path)) = (&self.current, self.files.shown_path()) else {
            return;
        };
        let left = match (&self.playback, current.sequence) {
            (Some(playback), _) => Some(Left::Frame {
                frame: playback.head(),
                paused: !playback.playing(),
            }),
            (None, Sequence::Pages { .. }) => Some(Left::Page(current.page)),
            (None, _) => None,
        };
        self.kept.keep(
            path,
            Settings {
                view: self.view,
                display: current.display.clone(),
                left,
            },
        );
    }

    /// Whether the file chooser is up. Asked of egui, whose popup it is:
    /// `Esc` and a click outside close it there, and nothing here would
    /// know.
    pub(super) fn chooser_open(&self) -> bool {
        self.gui
            .as_ref()
            .is_some_and(|gui| egui::Popup::is_id_open(&gui.ctx, ui::chooser::id()))
    }

    /// The list has changed — a directory read again, a paste taken in —
    /// so the chooser reads it again, and the thread is told about any
    /// files new to it.
    pub(super) fn list_changed(&mut self) {
        self.chooser.relist(self.files.paths());
        self.thumbnailer.enqueue(self.files.paths().to_vec());
    }

    /// Takes in what the thumbnail thread had to say.
    fn take_thumbnail(&mut self, delivered: Delivered) {
        if let Some((path, thumb)) = self.chooser.take(delivered) {
            self.hold_thumb(path, thumb);
        }
    }

    /// Puts a thumbnail in a texture for the screen to hold, or keeps it
    /// until there is a context to make one in.
    fn hold_thumb(&mut self, path: PathBuf, thumb: Thumb) {
        let Some(gui) = &self.gui else {
            self.pending_thumbs.push((path, thumb));
            return;
        };
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [thumb.width as usize, thumb.height as usize],
            &thumb.rgba,
        );
        let texture = gui.ctx.load_texture(
            path.display().to_string(),
            image,
            egui::TextureOptions::LINEAR,
        );
        self.thumbs.insert(path, texture);
    }

    /// Whether the command line asked for something and none of it ever
    /// reached the screen: every file it named failed to decode, and
    /// nothing was opened from the window in the meantime. A program
    /// opened on nothing and closed on nothing has not failed.
    pub fn showed_nothing(&self) -> bool {
        self.from_command_line && self.current.is_none()
    }

    /// Whether the window is showing nothing and waiting for nothing: the
    /// state that puts the buttons for opening something in the middle of
    /// it. Not while a read is in flight, since what is coming is a
    /// picture, and the buttons would be up for the length of a decode.
    fn is_empty(&self) -> bool {
        self.current.is_none() && self.files.is_idle()
    }

    /// The size to open the window at: the image's, once there is one, and
    /// otherwise whatever the header claimed on the way past.
    fn opening_size(&self) -> Option<[f32; 2]> {
        self.current
            .as_ref()
            .map(Current::size)
            .or(self.header_size)
    }

    /// Whether the surface should be the HDR one. The monitor decides where
    /// the compositor says what it is in: one in HDR mode gets the HDR
    /// surface, which costs the compositor nothing and gives the picture the
    /// room, and one in SDR mode gets the SDR surface — unless `--output
    /// hdr` asked for the other regardless, which is the one route left that
    /// asks a compositor to switch a monitor over. Where nothing says what
    /// the monitor is, what was asked for is all there is to go on.
    fn surface_hdr(&self) -> bool {
        match self.monitor {
            Some(Mode::Hdr) => true,
            Some(Mode::Sdr) | None => self.hdr == HdrPreference::On,
        }
    }

    /// Whether the picture is going out with room above SDR white, which is
    /// half of what a tone map defaults from and half of what every readout
    /// of a value says. It takes three things: a surface with the room, a
    /// monitor not known to be in SDR mode — a compositor maps an HDR surface
    /// down for one that is, and the room is not there however the surface
    /// was made — and the switch not having turned it off. Before there is a
    /// window the answer is the SDR one, and [`App::adopt_headroom`] asks
    /// again whenever any of the three moves.
    fn headroom(&self) -> Headroom {
        let surface = self
            .renderer
            .as_ref()
            .is_some_and(|renderer| renderer.output().is_hdr);
        if surface && self.monitor != Some(Mode::Sdr) && self.hdr != HdrPreference::Off {
            Headroom::Above
        } else {
            Headroom::None
        }
    }

    /// Whether the switch has anything to switch, and where it has not, which
    /// of the two reasons: an HDR color space has to be offered for the
    /// window, and the monitor has to be in HDR mode — or nothing able to say
    /// what it is in. Where the compositor can say and has not yet, which is
    /// the moment before the window has landed on a monitor, the answer is
    /// [`Hdr::NotInHdrMode`]: most monitors are SDR, and a switch that lit for
    /// a frame and then died would be the switch having been wrong.
    ///
    /// One answer for three readers — whether the button is drawn dead
    /// ([`App::hdr_available`]), whether it takes a press
    /// ([`App::toggle_hdr`]), and what its tooltip says instead of its name
    /// ([`ui::tooltip::disabled`]) — so a dead switch cannot come to give a
    /// reason it is not dead for.
    fn hdr_state(&self) -> Hdr {
        if !self.renderer.as_ref().is_some_and(Renderer::hdr_available) {
            return Hdr::Unsupported;
        }
        if self.monitors.as_ref().is_some_and(Monitors::speaks_modes)
            && self.monitor != Some(Mode::Hdr)
        {
            return Hdr::NotInHdrMode;
        }
        Hdr::Available
    }

    /// [`App::hdr_state`] read as the yes or no the button is drawn from.
    fn hdr_available(&self) -> bool {
        self.hdr_state() == Hdr::Available
    }

    /// Puts the surface where [`App::surface_hdr`] says and the curve where
    /// the headroom that leaves says, and reports whether the picture
    /// changed. Called whenever an input to either moves: the window landing
    /// on a monitor, the monitor changing mode, the switch being pressed.
    /// Which surface it is goes to stderr when it changes, the way the choice
    /// at start-up does, since the bar has room for one word and the
    /// surface's name is several.
    fn sync_output(&mut self) -> bool {
        let before = self.headroom();
        let wanted = self.surface_hdr();
        let mut changed = false;
        if let Some(renderer) = &mut self.renderer
            && renderer.output().is_hdr != wanted
            && renderer.set_hdr(wanted)
        {
            eprintln!("gamut: {} output", renderer.output().label);
            changed = true;
        }
        if self.headroom() != before {
            self.adopt_headroom();
            changed = true;
        }
        changed
    }

    /// Reads which monitor the window is on and what the compositor says it
    /// is in, and follows a change. Cheap enough to ask after every batch of
    /// events, which is how a window carried to another monitor is noticed:
    /// nothing else says. Returns whether anything on screen changed — the
    /// picture, or only the switch, which a monitor's mode lights or kills.
    fn sync_monitor(&mut self) -> bool {
        let Some(monitors) = &self.monitors else {
            return false;
        };
        let name = self
            .window
            .as_ref()
            .and_then(|window| window.current_monitor())
            .and_then(|monitor| monitor.name());
        let mode = name.as_deref().and_then(|name| monitors.mode(name));
        if mode == self.monitor {
            return false;
        }
        // Worth a line, since it is what lights the switch or kills it.
        if let (Some(name), Some(mode)) = (&name, mode) {
            let mode = match mode {
                Mode::Hdr => "HDR",
                Mode::Sdr => "SDR",
            };
            eprintln!("gamut: monitor {name} is in {mode} mode");
        }
        self.monitor = mode;
        self.sync_output();
        true
    }

    /// Re-derives the tone map for whatever is on screen, for the moment the
    /// output is settled and its headroom is known at last, and for every
    /// switch of it after.
    ///
    /// The file named on the command line is decoded before the window opens,
    /// so its display state is worked out against an SDR surface whatever the
    /// surface turns out to be. A curve asked for on the command line is left
    /// alone: that is a choice rather than a default.
    fn adopt_headroom(&mut self) {
        if self.startup.tone_map.is_some() {
            return;
        }
        let headroom = self.headroom();
        if let Some(current) = &mut self.current {
            current.display.adopt(headroom, &current.stats);
        }
    }

    /// Switches the room above white on or off. Returns whether anything
    /// changed.
    ///
    /// On a monitor in HDR mode the surface stays the HDR one either way and
    /// the compositor clips at white instead, so that the switch never asks
    /// the compositor for anything it might answer with a modeset. Where
    /// nothing says what the monitor is, the switch moves the surface
    /// itself, as the only lever there is. Where there is no room to switch
    /// to — a monitor in SDR mode, or no HDR color space for the window — the
    /// press does nothing at all: the button is drawn dead and its tooltip
    /// says which of the two it is, so a refusal said again in the terminal
    /// would only be said where it cannot be read. Switching the monitor over
    /// is `--output hdr`, at start-up, since that is the request a compositor
    /// answers with a modeset.
    ///
    /// The curve follows the headroom, as it does when the window first
    /// opens: the switch chooses the curve the room wants for what is on
    /// screen, and `t` changes it afterwards.
    pub(super) fn toggle_hdr(&mut self) -> bool {
        if !self.hdr_available() {
            return false;
        }
        self.hdr = if self.headroom() == Headroom::Above {
            HdrPreference::Off
        } else {
            HdrPreference::On
        };
        self.sync_output()
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

    /// Whether the content area has room for each of the two panels that
    /// float over it. The frame builder works this out for itself; it is
    /// worked out here as well for the presses and the tooltips, which have
    /// to answer between frames.
    pub(super) fn room(&self) -> ui::Room {
        ui::room(self.content(), &self.panels)
    }

    /// What the panels leave free for the image and for whatever floats over
    /// it, in the logical pixels those are laid out in. The frame builder
    /// works this out for itself; it is worked out here as well for the hit
    /// tests, which have to answer between frames.
    fn content(&self) -> Rect {
        content_area(
            self.logical_size(),
            self.panels.show_ui,
            self.has_transport(),
        )
    }

    /// Whether the file on screen brings the transport bar with it: an
    /// animation, or a file of pages.
    fn has_transport(&self) -> bool {
        self.current
            .as_ref()
            .is_some_and(|current| current.sequence != Sequence::Still)
    }

    /// What the transport bar shows, for a file that has one.
    fn transport(&self) -> Option<ui::Transport> {
        let current = self.current.as_ref()?;
        match (current.sequence, &self.playback, &self.player) {
            (Sequence::Animation { .. }, Some(playback), Some(player)) => Some(ui::Transport {
                index: playback.head(),
                count: playback.count(),
                kind: ui::transport::Kind::Animation {
                    playing: playback.playing(),
                    delays: player.read(|cache| cache.delays().to_vec()),
                },
            }),
            (Sequence::Pages { count, .. }, _, _) => Some(ui::Transport {
                index: current.page,
                count,
                kind: ui::transport::Kind::Pages,
            }),
            _ => None,
        }
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

    /// Raises the message at the foot of the window, in place of whatever was
    /// up. Handlers say it and return `Effect::Redraw`; nothing here asks the
    /// window for a frame.
    pub(super) fn toast(&mut self, message: impl Into<String>, level: Level) {
        self.toasts
            .show(Instant::now(), message.into(), level, toast::LINGER);
    }

    /// Raises a warning before the window opens: what the command line asked
    /// for and could not have, said where the reader will be looking.
    pub fn say(&mut self, message: &str) {
        self.toast(message, Level::Warning);
    }

    /// Says what the copies prepared on their own threads did. Returns
    /// whether anything was said, and so whether a redraw is owed.
    ///
    /// Taken up on the file check's cadence rather than the moment the thread
    /// finishes: a copy of a large picture takes far longer than the wait
    /// itself, and a quarter of a second either way on a message about it is
    /// not a difference anyone can see.
    fn poll_copies(&mut self) -> bool {
        let outcomes: Vec<CopyOutcome> = self.copied.1.try_iter().collect();
        let said = !outcomes.is_empty();
        for outcome in outcomes {
            match outcome {
                Ok(said) => self.toast(said, Level::Message),
                Err(error) => self.toast(error, Level::Error),
            }
        }
        said
    }

    /// Where the image is drawn, in physical pixels: what the panels leave in
    /// the middle, or the whole window when they are hidden. Derived rather
    /// than stored, so toggling the interface re-fits a fitted image without
    /// anything having to remember to.
    fn viewport(&self) -> Viewport {
        image_viewport(
            self.window_size(),
            self.scale_factor(),
            self.panels.show_ui,
            self.has_transport(),
        )
    }

    /// The view as it is on screen at `now`: `view` itself once it has
    /// arrived, and somewhere along the way to it while a move is in flight.
    /// Everything that reads the picture — the frame, the pixel under the
    /// pointer, the grid's spacing, the minimap's marker — reads this, so
    /// that they agree with one another about what is on screen mid-move.
    fn view_at(&self, now: Instant) -> View {
        match &self.motion {
            Some(motion) => {
                let (image, viewport) = (self.image_size(), self.viewport());
                let to = self.view.position(image, viewport);
                self.view.at(motion.position(to, now))
            }
            None => self.view,
        }
    }

    pub(super) fn shown_view(&self) -> View {
        self.view_at(Instant::now())
    }

    /// Makes `change` to the view, and puts it on screen as a move over
    /// [`crate::motion::DURATION`] rather than in one jump. The move starts
    /// from where the view is shown at this instant, which part way through
    /// an earlier move is part way along it: that move is dropped, and this
    /// one has the whole time to get from there to where `change` leaves the
    /// view.
    ///
    /// For a change asked for by name — a key, a notch of the wheel, a
    /// choice from the menu. A change the hand is on — a drag, a single
    /// pixel's step, a trackpad's scroll — goes to `view` directly and lands
    /// at once, or, if a move is in flight, at the end of it: the move is
    /// left running, and finds the view moved when it looks.
    pub(super) fn animate(&mut self, change: impl FnOnce(&mut View, [f32; 2], Viewport)) {
        let (image, viewport) = (self.image_size(), self.viewport());
        let now = Instant::now();
        let from = self.view_at(now).position(image, viewport);
        change(&mut self.view, image, viewport);
        let to = self.view.position(image, viewport);
        // Nowhere to go — a fit already fitted, an edge already reached, a
        // filter changed — is not a move, and owes no frames.
        self.motion = (to != from).then(|| Motion::new(from, now));
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
        // to whatever happened to be behind them. egui says which, from the
        // last pass — see `Pointer::over_image`.
        if !self.pointer.over_image {
            return None;
        }
        let cursor = self.pointer.cursor?;
        let viewport = self.viewport();
        if !viewport.contains(cursor) {
            return None;
        }
        let image = self.current.as_ref()?.size();
        let point = self
            .shown_view()
            .placement(image, viewport)
            .image_point(cursor);
        // A positive range test rather than four negated bounds, so that a
        // NaN coordinate is rejected: every `<`/`>=` comparison is false for
        // NaN, which would otherwise read as pixel (0, 0) — a coordinate the
        // readout must never invent.
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
        self.panels.show_minimap
            && self
                .shown_view()
                .can_pan(self.image_size(), self.viewport())
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
            self.has_transport(),
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
        let relisted = changed && self.files.relist(crate::listing::relist(&self.named));
        if relisted {
            self.list_changed();
        }
        relisted
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
        true
    }

    /// Notices that the desktop's theme has changed. Returns whether the
    /// window owes a redraw, which it does only when the new palette actually
    /// resolves to different colors.
    fn poll_theme(&mut self) -> bool {
        if !self.theme_watch.poll() {
            return false;
        }
        let theme = Theme::detect();
        let changed = theme != self.theme;
        self.theme = theme;
        if changed && let Some(gui) = &self.gui {
            gui.retint(&self.theme);
        }
        changed
    }

    /// What the window is called: the image on screen, the file being read
    /// while there is nothing on screen to name, or the program's own name
    /// while there is nothing at all.
    fn title(&self) -> String {
        match (&self.current, self.files.pending()) {
            (Some(_), _) => match self.files.shown_path() {
                Some(path) => window_title(path),
                None => crate::PROGRAM.to_string(),
            },
            (None, Some(pending)) => loading_title(self.files.path(pending.index)),
            (None, None) => match self.files.shown_path() {
                Some(path) => loading_title(path),
                None => crate::PROGRAM.to_string(),
            },
        }
    }

    /// Sends a request to the loader.
    ///
    /// Nothing changes on screen here. The image already up stays where it is,
    /// still pannable and zoomable, until the reply arrives at
    /// [`App::user_event`] — which is the whole point of the exercise, and the
    /// reason everything the interface says about the image goes on describing
    /// the one being shown rather than the one being fetched.
    fn send(&mut self, mut request: Request) {
        // A paged file comes back to the page it was left on, which has to
        // be asked for with the file: the page is what is decoded.
        if request.page.is_none()
            && request.mode == Reload::Fresh
            && let Some(Left::Page(page)) = self.kept.left(&request.path).and_then(|left| left.left)
        {
            request.page = Some(page);
            self.files.asked_for_page(page);
        }
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
            sequence,
            page,
        } = ready;
        let size = [image.width as f32, image.height as f32];
        let same_size = self
            .current
            .as_ref()
            .is_some_and(|current| current.size() == size);
        // Re-reading the same file keeps the user where they were, since they
        // are watching one spot for the change: same exposure and tone map,
        // with only an automatic window re-derived from the new pixels. One
        // that has come back a different size is a new shape to fit, and is
        // treated as a new picture below. Another page of the same file is
        // read the same way.
        let same_file = file.mode != Reload::Fresh;
        let in_place = same_file && same_size;
        // Whether this is a move between files at all. The file already on
        // screen being read again is not one, whatever it has become: it is
        // neither a departure to be put away nor a return to be restored.
        let stepping =
            self.current.is_some() && self.files.shown_path() != Some(file.path.as_path());
        // The picture being stepped away from, kept as it stands so that
        // stepping back to it finds it as it was left.
        if stepping {
            self.keep_shown();
        }
        // And what the file arriving left the last time it was on screen, if
        // it has been here — whether it is arriving beside a picture or
        // into an empty window, which is where the dialog's first choice
        // lands. Its window is re-derived where it was automatic,
        // the file being free to have changed on disk since; one set by hand
        // is left exactly where it was put.
        let kept = (!same_file)
            .then(|| self.kept.left(&file.path).cloned())
            .flatten();
        let display = match (self.current.as_ref().filter(|_| in_place), &kept) {
            (Some(current), _) => {
                let mut display = current.display.clone();
                display.refresh_auto(&stats);
                display
            }
            (None, Some(settings)) => {
                let mut display = settings.display.clone();
                display.refresh_auto(&stats);
                display
            }
            (None, None) => Display::for_image_with(&image, &stats, self.startup, self.headroom()),
        };

        let mut stored = None;
        if let Some(renderer) = &mut self.renderer {
            // Already across whenever the window was open when the read
            // started, which is every file but the one named on the command
            // line. The fallback covers only that gap.
            let uploaded = match gpu {
                Some(uploaded) => uploaded,
                None => match upload_here(renderer, &file.path, &image) {
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
        // What this file is, for the chooser's row about it, ahead of the
        // thumbnail thread reaching it. A thumbnail is asked for again for a
        // file changed on disk — the one in the cache is of the file as it
        // was, and its modification time no longer matches — and for one the
        // thread had given up on, which has just decoded here.
        let given_up = self.chooser.learn(
            &file.path,
            Facts {
                size: Some((image.width, image.height)),
                sequence,
                title: exif.title().map(str::to_string),
            },
        );
        if file.mode == Reload::InPlace || given_up {
            self.thumbnailer.prioritize(vec![file.path.clone()]);
        }
        // A region is of the picture it was drawn on. Stepping to another
        // file takes it off, and so does the file coming back a different
        // size, where the pixels it marked out are no longer the pixels.
        if stepping || !same_size {
            self.clear_region();
        }
        match &kept {
            // A picture of the same size as the one it is arriving beside is
            // almost always part of a set to be compared — frames of a
            // sequence, or one exposure against another — and there the point
            // is that the same detail stays under the same pixels. So the pan
            // and zoom carry over from the picture leaving the screen, ahead
            // of anything this file was left in itself: what the comparison
            // is being made at is where the eye already is, not where this
            // file happened to be the last time it was looked at.
            _ if same_size => {}
            // Back to a file of another size that has been here before:
            // exactly where it was left. The magnification filter is not part
            // of a view — it is a standing preference — so it stays as it is.
            Some(settings) => {
                let upscale = self.view.upscale();
                self.view = settings.view;
                self.view.set_upscale(upscale);
            }
            // A new shape, seen for the first time, so it is fitted afresh.
            None => self.view.reset(),
        }
        if !in_place {
            // A move under way was about the picture that has just left, and
            // there is nothing for it to carry the eye across any more.
            self.motion = None;
        }
        // And what else could open the file arriving, read here with the rest
        // of what the file itself says about it.
        if !same_file {
            self.openers = openers::for_file(&file.path);
        }
        self.current = Some(Current {
            image: Arc::new(image),
            stats,
            display,
            label: file_label(&file.path),
            file: file_facts(&file.path),
            exif,
            stored,
            sequence,
            page,
        });
        self.start_player(
            &file.path,
            sequence,
            kept.as_ref().and_then(|kept| kept.left),
        );
        if let Some(window) = &self.window {
            window.set_title(&window_title(&file.path));
        }
        true
    }

    /// Starts decoding the frames of an animation that has just gone up,
    /// with its clock, and stops whatever was playing before. A still or a
    /// paged file has neither.
    ///
    /// The file's own decode is what is on screen at this point — its first
    /// frame — and stays until the clock asks for another. A file that was
    /// left part way through comes back to that frame, playing if it was
    /// playing; every other animation opens playing from the start, unless
    /// `--paused` said otherwise.
    fn start_player(&mut self, path: &std::path::Path, sequence: Sequence, left: Option<Left>) {
        self.player = None;
        self.playback = None;
        self.uploaded = None;
        self.player_failed = false;
        let Sequence::Animation { count, loops } = sequence else {
            return;
        };
        let now = Instant::now();
        let mut playback = Playback::new(count, loops, !self.open_paused, now);
        if let Some(Left::Frame { frame, paused }) = left {
            playback.seek(frame);
            if !paused {
                playback.toggle(now);
            }
        }
        self.players += 1;
        let wake = Arc::clone(&self.wake);
        let player = Player::new(
            self.players,
            path.to_path_buf(),
            self.files.overrides(),
            count,
            move |event| wake(event),
        );
        player.head(playback.head());
        self.player = Some(player);
        self.playback = Some(playback);
    }

    /// Puts the frame the clock says should be up on screen, where it is
    /// not already and the player has it. Where the player has not got to
    /// it yet, it is told where the head is and asked to wake us when it
    /// has; the frame already up stays until then.
    ///
    /// The frame's pixels are written into the texture the last frame's
    /// occupy, and the interface's picture and statistics are swapped for
    /// the frame's, so that everything reading the picture — the histogram,
    /// the readout, a copy — reads the frame on screen. The display is left
    /// as it is: a window or an exposure is a setting, and a setting that
    /// changed under the eye with every frame would be a picture that
    /// pumped.
    fn show_due_frame(&mut self) {
        let (Some(playback), Some(player)) = (&self.playback, &self.player) else {
            return;
        };
        let head = playback.head();
        if self.uploaded == Some(head) {
            return;
        }
        player.head(head);
        let Some(frame) = player.read(|cache| cache.frame(head)) else {
            return;
        };
        if let Some(renderer) = &mut self.renderer {
            match renderer.refill_image(&frame.image) {
                Ok(Some(note)) => eprintln!("gamut: {note}"),
                Ok(None) => {}
                Err(error) => {
                    eprintln!("gamut: {}", crate::escape_controls(&format!("{error:#}")));
                    return;
                }
            }
        }
        if let Some(current) = &mut self.current {
            current.image = Arc::clone(&frame.image);
            current.stats = frame.stats.clone();
        }
        self.uploaded = Some(head);
    }

    /// Moves the animation's clock on to `now`. Returns whether the frame
    /// on screen is owed a change, and when the next one is due.
    fn tick_playback(&mut self, now: Instant) -> (bool, Option<Instant>) {
        let (Some(playback), Some(player)) = (&mut self.playback, &self.player) else {
            return (false, None);
        };
        let (delays, count, error) = player.read(|cache| {
            (
                cache.delays().to_vec(),
                cache.count(),
                cache.error().map(str::to_string),
            )
        });
        playback.shrink(count);
        let tick = playback.tick(now, &delays);
        let mut changed = tick.changed;
        if let Some(error) = error
            && !self.player_failed
        {
            self.player_failed = true;
            eprintln!("gamut: {}", crate::escape_controls(&error));
            self.toast("The animation could not be read to its end", Level::Error);
            changed = true;
        }
        (changed, tick.deadline)
    }

    /// One frame on or back through the animation, or one page on or back
    /// through a paged file: the same key for both, since a reader stepping
    /// through what a file holds does not care which kind it is.
    pub(super) fn step_frame(&mut self, by: isize) -> Effect {
        if let Some(playback) = &mut self.playback {
            playback.step(by);
            return Effect::Redraw;
        }
        let Some(current) = &self.current else {
            return Effect::Nothing;
        };
        let Sequence::Pages { count, .. } = current.sequence else {
            return Effect::Nothing;
        };
        let page = (current.page as isize + by).rem_euclid(count as isize) as usize;
        if let Some(request) = self.files.page(page) {
            self.send(request);
        }
        Effect::Nothing
    }

    /// Plays a stopped animation, or stops a playing one.
    pub(super) fn toggle_play(&mut self) -> Effect {
        let Some(playback) = &mut self.playback else {
            return Effect::Nothing;
        };
        playback.toggle(Instant::now());
        Effect::Redraw
    }

    /// Straight to frame `frame` of the animation, stopped there.
    pub(super) fn seek(&mut self, frame: usize) -> Effect {
        let Some(playback) = &mut self.playback else {
            return Effect::Nothing;
        };
        playback.seek(frame);
        Effect::Redraw
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
        let name = file_label(&decoded.file.path);
        // A file that will not go on screen is a file to step over, whether it
        // was the decode or the upload that would not have it.
        let failed = match decoded.outcome {
            Ok(ready) => match self.apply(decoded.file, ready) {
                true => None,
                false => Some(format!("Could not show {name}.")),
            },
            Err(error) => {
                input::report(&error);
                Some(format!("Could not read {name}: {}", input::briefly(&error)))
            }
        };
        if let Some(said) = failed {
            match self.files.failed(index, pending.step) {
                Some(request) => self.send(request),
                // The walk is over, or this was no walk, and the failure is
                // the last word: worth saying in the window where there is
                // a window left to say it in — with nothing on screen and
                // the list the command line's, the window is about to go.
                None => {
                    if !self.from_command_line || self.current.is_some() {
                        self.toast(said, Level::Error);
                    }
                }
            }
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

        // A move that has landed is over: what is on screen is `view`
        // itself, and the frames it was asking for can stop.
        let now = Instant::now();
        if self.motion.as_ref().is_some_and(|motion| motion.done(now)) {
            self.motion = None;
        }
        let view = self.view_at(now);
        self.show_due_frame();
        for (path, thumb) in std::mem::take(&mut self.pending_thumbs) {
            self.hold_thumb(path, thumb);
        }
        let chooser = self.chooser_open().then(|| {
            let shown = self.current.as_ref().and_then(|_| self.files.shown_path());
            self.chooser.input(&self.thumbs, shown)
        });
        let rename = self.rename_input();

        let scale = window.scale_factor() as f32;
        let physical = self.window_size();
        let logical = [physical[0] / scale, physical[1] / scale];
        let viewport = self.viewport();
        let placement = view.placement(self.image_size(), viewport);

        let pointer = self.pointer_pixel();
        let cursor = self.logical_cursor();
        let thumbnail = self.minimap_placement(logical, scale);
        let minimap = self.minimap_on_screen();
        let reading = self.reading();
        let headroom = self.headroom();
        let hdr_available = self.hdr_available();
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
            headroom,
            hdr_available,
            can_pan: self.view.can_pan(self.image_size(), viewport),
            openers: self
                .openers
                .iter()
                .map(|opener| opener.name.clone())
                .collect(),
            toast: self.toasts.showing().cloned(),
            selection: self.selection,
            handle: self.handle,
            grabbing: self.grabbing.as_ref().map(Grabbing::grab),
            over_region: self.over_region(),
            box_zoom: self.pointer.space != input::Space::Up,
            move_region: self.pointer.modifiers.shift_key(),
            zoom_box: self.zoom_box,
            transport: self.transport(),
            chooser,
            rename,
            empty: self.is_empty(),
            picking: self.picking,
        };

        let namer = self.namer();
        let Some(gui) = self.gui.as_mut() else {
            return;
        };
        let mut commands = Vec::new();
        let (painted, textures) = gui.run(&window, |ui| {
            commands = ui::show(
                ui,
                &input,
                &self.panels,
                self.current.as_ref(),
                &view,
                &self.theme,
                &namer,
            );
        });

        let fallback = Display::default();
        let display = self
            .current
            .as_ref()
            .map(|current| &current.display)
            .unwrap_or(&fallback);

        let backdrop = ui::backdrop(&self.theme);

        let renderer = self.renderer.as_mut().expect("checked above");
        let scene = Scene {
            placement,
            thumbnail,
            display,
            ui: &painted,
            scale,
            backdrop,
            headroom,
            mark_clipped: self.panels.mark_clipped,
        };
        match renderer.render(scene, textures) {
            Ok(()) => self.reported_error = false,
            Err(error) => {
                if !self.reported_error {
                    eprintln!("gamut: {}", crate::escape_controls(&format!("{error:#}")));
                    self.reported_error = true;
                }
            }
        }

        // What the interface asked for is done once the frame is off: it
        // was drawn from the state as it was, and the next frame shows what
        // the press did.
        let mut changed = false;
        for command in commands {
            changed |= self.act(command);
        }

        // A move still in flight owes the next frame. Asked for from here
        // rather than timed from the loop, so that it comes when the
        // compositor is ready for one and the move plays at the display's
        // own rate.
        if self.motion.is_some() || changed {
            window.request_redraw();
        }
    }
}

/// Uploads `image` on this thread, and reports the time the way the loader
/// does for the files it uploads itself, so that the two lines can be read
/// against each other. This is the path for a file the loader read before
/// it had a renderer to upload to: the one named on the command line.
fn upload_here(renderer: &Renderer, path: &Path, image: &DecodedImage) -> anyhow::Result<GpuImage> {
    let began = Instant::now();
    let uploaded = renderer.uploader().run(image)?;
    timing::uploaded(path, began.elapsed());
    Ok(uploaded)
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

impl ApplicationHandler<UserEvent> for App {
    /// Look at the file, then sleep until it is time to look again rather than
    /// until the next event: nothing tells us about a write, so we go and ask.
    ///
    /// The deadline is a fixed cadence rather than an interval from here, so
    /// that a stream of events — a drag, a resize — cannot keep pushing the
    /// next look out of reach.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.sync_monitor()
            && let Some(window) = &self.window
        {
            window.request_redraw();
        }

        let now = Instant::now();
        if now >= self.next_poll {
            self.next_poll = now + watch::INTERVAL;
            // All of them, always: each has a watch that only advances when
            // it is polled.
            let vanished = self.poll_file();
            let relisted = self.poll_directories();
            let retinted = self.poll_theme();
            let offered = self.poll_clipboard();
            let copied = self.poll_copies();
            if (vanished || relisted || retinted || offered || copied)
                && let Some(window) = &self.window
            {
                window.request_redraw();
            }
        }

        // The things on screen that happen because time passed rather than
        // because anything arrived: the message about what was just done
        // having been up long enough, and whatever egui is waiting on — a
        // tooltip's delay, a hover fading.
        let mut timed = self.toasts.tick(now);
        if self.gui.as_mut().is_some_and(|gui| gui.due(now)) {
            timed = true;
        }
        // And the animation's clock: the next frame being due.
        let (frame_due, next_frame) = self.tick_playback(now);
        timed |= frame_due;
        if timed && let Some(window) = &self.window {
            window.request_redraw();
        }

        // Sleep until the next thing with a time on it: the file check, the
        // moment a read that is still going becomes worth mentioning, the
        // moment a message has had its time, or the moment egui asked for. A
        // read that finishes first wakes us through the proxy instead.
        let mut deadline = self.next_poll;
        for due in [
            self.toasts.deadline(),
            self.gui.as_ref().and_then(Gui::deadline),
            next_frame,
        ]
        .into_iter()
        .flatten()
        {
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

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Decoded(decoded) => {
                self.deliver(*decoded);
                // Nothing ever reached the screen and nothing else is coming:
                // every file named on the command line failed to decode.
                // Stop, rather than sit in an empty window with nothing on
                // the way. A choice made in the window is answered in the
                // window instead, which stays up for the next choice.
                if self.showed_nothing() && self.files.is_idle() {
                    event_loop.exit();
                }
            }
            UserEvent::Picked(picked) => {
                self.picked(picked);
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            UserEvent::Monitor => {
                if self.sync_monitor()
                    && let Some(window) = &self.window
                {
                    window.request_redraw();
                }
            }
            // A frame from the player of a file already stepped past is news
            // about nothing on screen.
            UserEvent::Frame(event) => {
                if self
                    .player
                    .as_ref()
                    .is_some_and(|player| player.generation == event.generation)
                    && let Some(window) = &self.window
                {
                    window.request_redraw();
                }
            }
            // A frame only while the chooser is up: with it closed nothing
            // on screen shows a thumbnail, and the news is kept for when it
            // opens.
            UserEvent::Thumbnail(delivered) => {
                self.take_thumbnail(*delivered);
                if self.chooser_open()
                    && let Some(window) = &self.window
                {
                    window.request_redraw();
                }
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let size = initial_window_size(
            event_loop,
            self.monitors.as_ref(),
            self.opening_size(),
            self.asked_size,
        );
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

        // The file named on the command line, decoded while the window was
        // being made, and waiting here for somewhere to go. There is nothing
        // on screen yet for the wait to interrupt; everything opened
        // afterwards is uploaded on the loader's thread.
        if let (Some(current), Some(path)) = (&mut self.current, self.files.shown_path()) {
            match upload_here(&renderer, path, &current.image) {
                Ok(uploaded) => {
                    if let Some(note) = renderer.install_image(uploaded) {
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

        // Asked for and not had is worth a line; asked for and had is worth
        // one too, since the switch's later lines say the same thing. A
        // surface that follows the monitor says so when it moves.
        if self.hdr == HdrPreference::On {
            let output = renderer.output();
            eprintln!(
                "gamut: {} \u{2192} {} output{}",
                renderer.adapter_name(),
                output.label,
                if output.is_hdr {
                    ""
                } else {
                    " (no HDR color space offered for this window)"
                }
            );
        }

        let gui = match Gui::new(&window, &self.theme, renderer.max_texture_side()) {
            Ok(gui) => gui,
            Err(error) => {
                eprintln!("gamut: {}", crate::escape_controls(&format!("{error:#}")));
                event_loop.exit();
                return;
            }
        };

        // From here on the loader uploads as well as decodes, so that
        // stepping to the next file costs the event loop nothing but the swap.
        self.loader.attach(renderer.uploader());
        self.renderer = Some(renderer);
        self.gui = Some(gui);
        self.window = Some(window);
        // The surface exists at last, so whatever was decoded before the
        // window opened can find out what it is being drawn onto. Which
        // monitor it is on is not known until it has been shown, and the
        // surface follows it from `about_to_wait`.
        self.adopt_headroom();
        // A first frame, asked for outright. With a file on the way its
        // arrival asks for one; a window opened on nothing has nothing
        // coming, and its buttons are owed a frame all the same.
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        // egui sees every event first. What it takes for itself — a press on
        // one of its widgets — goes no further; what it merely wants painted
        // for, a pointer crossing one of them, is a redraw and nothing else.
        // Except for the redraw itself: egui answers `RedrawRequested` with
        // "repaint" too, meaning paint now, and a frame asked for on the
        // strength of that would be a frame asking for the next for ever.
        let response = match (&self.gui, &self.window) {
            (Some(_), Some(window)) => {
                let window = window.clone();
                let gui = self.gui.as_mut().expect("matched above");
                Some(gui.on_event(&window, &event))
            }
            _ => None,
        };
        if let Some(response) = &response
            && response.repaint
            && !matches!(event, WindowEvent::RedrawRequested)
            && let Some(window) = &self.window
        {
            window.request_redraw();
        }
        let consumed = response.is_some_and(|response| response.consumed);
        let effect = match event {
            _ if consumed && !matches!(event, WindowEvent::RedrawRequested) => Effect::Nothing,
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
            // Where the pointer is, for the readouts and for the wheel's
            // anchor: the press, the drag and the wheel themselves are egui's,
            // and come back from the frame as commands.
            WindowEvent::CursorMoved { position, .. } => {
                let was_over = self.pointer_pixel();
                self.pointer.cursor = Some([position.x as f32, position.y as f32]);
                Effect::redraw_if(self.pointer_pixel() != was_over)
            }
            WindowEvent::CursorLeft { .. } => {
                let was_over = self.pointer_pixel().is_some();
                self.pointer.cursor = None;
                Effect::redraw_if(was_over)
            }
            WindowEvent::ScaleFactorChanged { .. } => Effect::Redraw,
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key,
                        physical_key,
                        state,
                        ..
                    },
                ..
            } => self.handle_key(&logical_key, physical_key, state),
            // A key held as the focus goes is released somewhere else.
            WindowEvent::Focused(false) => {
                self.keys_lost();
                Effect::Nothing
            }
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
    use std::time::Duration;

    use super::*;
    use crate::image::{Stats, exif};
    use crate::view::Fit;

    const WINDOW: [f32; 2] = [1000.0, 700.0];
    /// The same window with nothing taken out of it, for the tests that are
    /// about stepping between files rather than about where the panels are.
    const VIEWPORT: Viewport = Viewport::whole(WINDOW);

    /// A gray PNG of the given size, written where the test can step onto it.
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
        let size = decode::probe(&paths[0]).expect("we just wrote it");
        App::new(
            paths,
            named,
            Some(Opening {
                index: 0,
                source: Source::Disk,
                size: size.map(|(w, h)| [w as f32, h as f32]),
            }),
            options(),
            threads(),
        )
    }

    /// The application as `gamut` alone opens it: no list, and nothing
    /// asked for.
    fn opened_on_nothing() -> App {
        App::new(Vec::new(), Vec::new(), None, options(), threads())
    }

    fn options() -> Options {
        Options {
            overrides: decode::Overrides::default(),
            startup: Startup::default(),
            hdr: HdrPreference::default(),
            histogram: false,
            info: false,
            minimap: false,
            upscale: Upscale::default(),
            size: None,
            paused: false,
        }
    }

    /// The other threads, each detached: a test has no event loop for them
    /// to reach.
    fn threads() -> Threads {
        Threads {
            loader: Loader::detached(),
            wake: Arc::new(|_| true),
            monitors: None,
            thumbnailer: Thumbnailer::detached(),
            picker: Arc::new(|_| {}),
        }
    }

    /// As [`opening`], with the application's own opening request answered:
    /// the state the tests about later behavior want to start from.
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
        let (generation, index, asked) = (pending.generation, pending.index, pending.page);
        let path = app.files.path(index).to_path_buf();
        let watch = Watch::new(&path);
        let sequence = decode::sequence(&path).expect("the header reads");
        let page = match (asked, sequence) {
            (Some(page), _) => page,
            (None, Sequence::Pages { default, .. }) => default,
            (None, _) => 0,
        };
        let decoded = decode::load_timed(&path, app.files.overrides(), asked);
        let outcome = decoded.map(|(image, _)| Ready {
            stats: Stats::scan(&image),
            exif: exif::Exif::read(&path),
            image,
            gpu: None,
            sequence,
            page,
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

    /// A row of the chooser asks for the file it names, wherever the list
    /// has put it, and a row naming the file already on screen asks for
    /// nothing; and a list read again is a list the chooser reads again.
    #[test]
    fn choosing_a_row_asks_for_its_file() {
        use crate::ui::Control;

        let (mut app, dir) = app_over(
            "choose",
            &[("a.png", 8, 8), ("b.png", 8, 8), ("c.png", 8, 8)],
        );
        assert!(app.files.is_idle());
        app.chooser.open(app.files.paths(), app.files.index());
        let _ = app.act(ui::Command::Press(Control::Choose(0)));
        assert!(
            app.files.is_idle(),
            "the file on screen is not asked for again"
        );

        let _ = app.act(ui::Command::Press(Control::Choose(2)));
        let pending = app.files.pending().expect("the third file is asked for");
        assert_eq!(pending.index, 2);
        assert_eq!(app.files.path(2).file_name().unwrap(), "c.png");
        answer(&mut app, Reload::Fresh);
        assert_eq!(app.files.index(), 2);

        // A row past the list asks for nothing rather than panicking.
        let _ = app.act(ui::Command::Press(Control::Choose(99)));
        assert!(app.files.is_idle());

        // The list rebuilt under the popup: the chooser sees the new file,
        // and a row still resolves to its file by name.
        write_png(&dir, "d.png", 8, 8);
        assert!(
            app.files
                .relist(crate::listing::relist(std::slice::from_ref(&dir)))
        );
        app.list_changed();
        let input = app.chooser.input(&app.thumbs, app.files.shown_path());
        assert_eq!(input.rows.len(), 4);
        assert_eq!(input.current, Some(2));
        assert_eq!(
            app.chooser
                .path_at(3)
                .map(|p| p.file_name().unwrap().to_owned())
                .as_deref(),
            Some(std::ffi::OsStr::new("d.png"))
        );

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// A program opened on nothing is empty rather than failed: it has no
    /// file to name, and every key about the file is a key that does
    /// nothing, rather than one that reaches for a file that is not there.
    #[test]
    fn opened_on_nothing_the_window_is_empty_and_the_file_keys_are_dead() {
        use crate::ui::Naming;
        use input::Action;

        let mut app = opened_on_nothing();
        assert!(app.is_empty());
        assert!(
            !app.showed_nothing(),
            "nothing was asked for, so nothing failed"
        );
        assert_eq!(app.title(), crate::PROGRAM);
        assert_eq!(app.files.len(), 0);
        assert!(app.reading().is_none());
        assert!(!app.poll_file());
        assert!(!app.poll_directories());

        for action in [
            Action::CopyName,
            Action::CopyPath,
            Action::CopyUri,
            Action::CopyImage,
            Action::CopyMetadata,
            Action::NextFile,
            Action::PreviousFile,
            Action::Rename,
            Action::Delete,
            Action::NextFrame,
            Action::TogglePlay,
            Action::ZoomIn,
            Action::CycleFit,
        ] {
            let _ = app.perform(action);
            assert!(app.is_empty(), "{action:?} changes nothing");
            assert!(app.renaming.is_none());
            assert!(app.files.is_idle());
        }
        let namer = app.namer();
        assert!(
            namer
                .tooltip(ui::Tip::Name)
                .is_some_and(|tip| tip.title == [""])
        );
        assert_eq!(
            namer
                .tooltip(ui::Tip::Control(ui::Control::Copy))
                .expect("a reason")
                .title,
            [ui::tooltip::NOTHING_OPEN]
        );
    }

    /// What the dialog chose is opened as a command line naming it beside
    /// the rest would be: a folder for the images in it, the newcomers
    /// joining the end of the list and the first of them asked for as a
    /// walk; the picture up stays until it arrives, and comes back as it
    /// was left. A choice that fails leaves the window as it was and says
    /// so, rather than leaving.
    #[test]
    fn what_the_dialog_chose_joins_the_list() {
        use input::Action;

        let (dir, paths) = written("chosen", &[("a.png", 16, 8), ("b.png", 8, 16)]);
        let mut app = opened_on_nothing();

        // Dismissed: nothing changes.
        app.picking = true;
        app.picked(Picked { outcome: Ok(None) });
        assert!(!app.picking);
        assert!(app.is_empty());
        assert!(app.toasts.showing().is_none());

        // The dialog could not be had: said, and the window stays empty.
        app.picked(Picked {
            outcome: Err(anyhow::anyhow!("no portal")),
        });
        assert!(app.is_empty());
        assert!(app.toasts.showing().is_some());
        app.toasts.dismiss();

        // One file: the list is that file, on its way.
        app.picked(Picked {
            outcome: Ok(Some(vec![paths[1].clone()])),
        });
        assert!(!app.is_empty(), "a read is in flight");
        assert_eq!(app.files.len(), 1);
        answer(&mut app, Reload::Fresh);
        assert!(app.current.is_some());
        assert_eq!(app.files.shown_path(), Some(paths[1].as_path()));
        assert!(!app.showed_nothing());
        assert!(!app.from_command_line);

        // A zoom to remember it by.
        let _ = app.perform(Action::ZoomTo(4.0));
        let zoomed = app.view.zoom(app.image_size(), app.viewport());
        let zoom = |app: &App| app.view.zoom(app.image_size(), app.viewport());

        // A folder chosen while a picture is up: the images in it join the
        // end of the list — the one already there not twice — the folder
        // is watched, the first newcomer is on its way, and the picture
        // stays until it arrives, kept as it was left.
        app.open_named(vec![dir.clone()]);
        assert!(app.current.is_some());
        assert_eq!(app.files.len(), 2);
        assert_eq!(app.files.path(1), paths[0]);
        assert_eq!(app.named, vec![paths[1].clone(), dir.clone()]);
        assert_eq!(app.directories.len(), 1, "the folder is watched");
        assert_eq!(app.files.pending().map(|pending| pending.index), Some(1));
        answer(&mut app, Reload::Fresh);
        assert_eq!(app.files.shown_path(), Some(paths[0].as_path()));
        assert!(app.kept.left(&paths[1]).is_some());
        assert_ne!(zoom(&app), zoomed, "a new shape, fitted afresh");

        // The first again, through the dialog: nothing new to add, so it
        // is gone to by name, and comes back as it was left.
        app.open_named(vec![paths[1].clone()]);
        assert_eq!(app.files.len(), 2);
        assert_eq!(app.named.len(), 2, "named once already");
        answer(&mut app, Reload::Fresh);
        assert_eq!(app.files.shown_path(), Some(paths[1].as_path()));
        assert_eq!(zoom(&app), zoomed);

        // The file on screen chosen: nothing to ask for.
        app.open_named(vec![paths[1].clone()]);
        assert!(app.files.is_idle());

        // A folder with no images in it is refused before anything moves.
        let empty = dir.join("empty");
        std::fs::create_dir_all(&empty).expect("the temporary directory is writable");
        app.open_named(vec![empty]);
        assert!(app.current.is_some());
        assert_eq!(app.files.len(), 2);
        assert!(app.toasts.showing().is_some());
        app.toasts.dismiss();

        // A file that will not read: it joins the list, the walk over it
        // fails, and the picture stays up with the reason under it.
        let broken = dir.join("broken.png");
        std::fs::write(&broken, b"not a png at all").expect("the file is writable");
        app.open_named(vec![broken]);
        assert_eq!(app.files.len(), 3);
        answer(&mut app, Reload::Fresh);
        assert!(app.current.is_some());
        assert_eq!(app.files.shown_path(), Some(paths[1].as_path()));
        assert!(
            app.toasts
                .showing()
                .is_some_and(|toast| toast.message.starts_with("Could not read broken.png")),
        );

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// The readings the pass makes on every frame owe a frame only when
    /// they change: said again unchanged, they must not ask for another,
    /// or an idle window would draw itself over and over.
    #[test]
    fn a_reading_said_again_unchanged_owes_no_frame() {
        let (mut app, dir) = app_over("readings", &[("a.png", 8, 8)]);
        assert!(app.act(ui::Command::OverImage(true)));
        assert!(!app.act(ui::Command::OverImage(true)));
        assert!(app.act(ui::Command::OverImage(false)));
        assert!(!app.act(ui::Command::OverGrip(None)));
        assert!(app.act(ui::Command::OverGrip(Some(
            crate::image::region::Grip::Inside
        ))));
        assert!(app.act(ui::Command::Press(ui::Control::Grid)));
        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// A key's pan is a move: the view is where it is going at once, and
    /// what is on screen gets there over [`crate::motion::DURATION`]. A
    /// single pixel's is not, and lands as it is pressed.
    #[test]
    fn a_keyboard_pan_moves_and_a_single_pixel_lands() {
        use input::{Action, Direction, PanStep};

        let (mut app, dir) = app_over("motion", &[("a.png", 64, 48)]);
        let (image, viewport) = (app.image_size(), app.viewport());
        app.view.set_zoom(4.0, image, viewport);
        let before = app.view.position(image, viewport);

        let _ = app.perform(Action::Pan(Direction::Right, PanStep::Coarse));
        assert!(app.motion.is_some());
        let target = app.view.position(image, viewport);
        assert!(target.u[0] > before.u[0]);
        // Just begun: on screen it has barely left where it was.
        let now = Instant::now();
        let shown = app.view_at(now).position(image, viewport);
        assert!(shown.u[0] < target.u[0]);
        // Landed, and where the view says.
        let landed = app.view_at(now + crate::motion::DURATION);
        assert_eq!(landed.position(image, viewport), target);

        app.motion = None;
        let _ = app.perform(Action::Pan(Direction::Left, PanStep::Fine));
        assert!(app.motion.is_none());

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// Escape puts away whatever is up, topmost first, and leaves only when
    /// there is nothing left to put away. `q` is not held up by a message:
    /// a copy is often followed straight away by it.
    #[test]
    fn escape_puts_things_away_before_it_quits() {
        use input::{Action, Effect};

        let (mut app, dir) = app_over("dismiss", &[("a.png", 64, 48)]);
        let raise = |app: &mut App| {
            app.toasts.show(
                Instant::now(),
                "Copied file path.".to_string(),
                Level::Message,
                toast::LINGER,
            );
        };

        // Each press takes off one thing. A menu would outrank the message,
        // but the menus are egui's and there is no window here to open one
        // in; `close_menus` answers for it.
        raise(&mut app);
        assert!(!app.close_menus());
        assert!(app.toasts.showing().is_some());

        assert_eq!(app.perform(Action::Dismiss), Effect::Redraw);
        assert!(app.toasts.showing().is_none());
        assert_eq!(app.perform(Action::Dismiss), Effect::Quit);

        // `q` leaves whether or not there is a message to read.
        raise(&mut app);
        assert_eq!(app.perform(Action::Quit), Effect::Quit);

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// `t` does nothing under a false color, where the curve does nothing:
    /// a curve changed there would only show once the ramp came off, from a
    /// press made long before. The ramp put back, the key is a key again.
    #[test]
    fn the_curve_key_is_dead_under_a_false_color() {
        use crate::image::display::{Colormap, Display, ToneMap};
        use crate::ui::{Command, Control};
        use input::Action;

        // A gray file, since a false color is a reading of one channel.
        let (dir, paths) = written("false-color", &[("gray.png", 8, 8)]);
        ::image::save_buffer(&paths[0], &[128u8; 64], 8, 8, ::image::ColorType::L8)
            .expect("the temporary directory is writable");
        let mut app = open(paths.clone(), paths);
        answer(&mut app, Reload::Fresh);
        fn display(app: &App) -> &Display {
            &app.current.as_ref().expect("a picture is up").display
        }
        assert!(
            app.current
                .as_ref()
                .is_some_and(|current| current.image.is_gray())
        );
        assert_eq!(display(&app).tone_map, ToneMap::None);

        assert_eq!(app.perform(Action::CycleColormap), Effect::Redraw);
        assert_eq!(display(&app).colormap, Colormap::Viridis);
        assert_eq!(app.perform(Action::CycleToneMap), Effect::Nothing);
        assert_eq!(display(&app).tone_map, ToneMap::None);
        let _ = app.act(Command::Press(Control::Curve(1)));
        assert_eq!(display(&app).tone_map, ToneMap::None);

        for _ in 1..Colormap::ALL.len() {
            let _ = app.perform(Action::CycleColormap);
        }
        assert_eq!(display(&app).colormap, Colormap::Gray);
        assert_eq!(app.perform(Action::CycleToneMap), Effect::Redraw);
        assert_eq!(display(&app).tone_map, ToneMap::Neutral);

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// A false color is a reading of one channel, and a color image's three
    /// are colors already: `r` refuses one, and so does the panel's swatch,
    /// or a press of it would put a map on the state that the picture, the
    /// bar and the readout all ignore.
    #[test]
    fn a_false_color_button_is_dead_on_a_color_image() {
        use crate::image::display::Colormap;
        use crate::ui::{Command, Control};
        use input::{Action, Effect};

        let (mut app, dir) = app_over("ramp", &[("a.png", 8, 8)]);
        assert!(
            app.current
                .as_ref()
                .is_some_and(|current| !current.image.is_gray())
        );
        let _ = app.act(Command::Press(Control::Ramp(1)));
        assert_eq!(
            app.current
                .as_ref()
                .expect("a picture is up")
                .display
                .colormap,
            Colormap::Gray
        );
        assert_eq!(app.perform(Action::CycleColormap), Effect::Nothing);

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// The keys step the handles, no further than the plot goes: on a
    /// graded file the plot is 0..1, so the black point cannot be stepped
    /// below 0 nor the white point above 1, and the exposure is left alone
    /// by both.
    #[test]
    fn the_window_keys_step_the_handles_within_the_plot() {
        use input::{Action, Effect};

        let (mut app, dir) = app_over("handles", &[("a.png", 8, 8)]);
        fn display(app: &App) -> &crate::image::display::Display {
            &app.current.as_ref().expect("a picture is up").display
        }
        assert_eq!(display(&app).displayed_bounds(), (0.0, 1.0));

        // Black is at the floor already, so a press downward is no press.
        assert_eq!(app.perform(Action::StepBlack(-0.05)), Effect::Nothing);
        assert_eq!(display(&app).displayed_bounds(), (0.0, 1.0));
        // A twentieth of the plot, which is on the file's sRGB curve.
        assert_eq!(app.perform(Action::StepBlack(0.05)), Effect::Redraw);
        let (black, white) = display(&app).displayed_bounds();
        assert!((crate::image::Transfer::Srgb.to_encoded(black) - 0.05).abs() < 1e-5);
        assert!((white - 1.0).abs() < 1e-6);

        // White is at the ceiling already, so a press upward is no press.
        assert_eq!(app.perform(Action::StepWhite(0.05)), Effect::Nothing);
        // And a press down moves it alone, a twentieth of the window along
        // the plot, the exposure untouched.
        assert_eq!(app.perform(Action::StepWhite(-0.05)), Effect::Redraw);
        assert_eq!(display(&app).exposure_stops, 0.0);
        let (still_black, white) = display(&app).displayed_bounds();
        assert_eq!(still_black, black);
        let encoded = crate::image::Transfer::Srgb.to_encoded(white);
        assert!((encoded - 0.9525).abs() < 1e-4, "{encoded}");

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// The window row is every file's: on a graded file, whose window opens
    /// at 0..1, *As stored* is what puts a hand-moved window back. It sets
    /// the rule and nothing else, so an exposure on top of the window stays.
    #[test]
    fn as_stored_puts_a_hand_moved_window_back_on_a_graded_file() {
        use crate::image::display::AutoWindow;
        use crate::ui::{Command, Control};
        use input::{Action, Effect};

        let (mut app, dir) = app_over("stored", &[("a.png", 8, 8)]);
        fn display(app: &App) -> &crate::image::display::Display {
            &app.current.as_ref().expect("a picture is up").display
        }
        assert_eq!(app.perform(Action::StepBlack(0.05)), Effect::Redraw);
        assert_eq!(app.perform(Action::StepWhite(-0.05)), Effect::Redraw);
        assert_eq!(app.perform(Action::Exposure(0.5)), Effect::Redraw);
        assert_eq!(display(&app).auto, AutoWindow::Manual);
        assert_ne!(display(&app).displayed_bounds(), (0.0, 1.0));

        let _ = app.act(Command::Press(Control::Window(0)));
        assert_eq!(display(&app).auto, AutoWindow::Off);
        assert_eq!(
            (display(&app).window_low, display(&app).window_high),
            (0.0, 1.0)
        );
        assert_eq!(
            display(&app).exposure_stops,
            0.5,
            "the rule, not the exposure"
        );

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// Escape is what brings the interface back, and it does that before it
    /// takes off the message that said so: a window that dismissed its own
    /// instructions and left the bars hidden would be disagreeing with what
    /// it had just told the reader. `q` still leaves from under it.
    #[test]
    fn escape_brings_the_interface_back_before_it_quits() {
        use input::{Action, Effect};

        let (mut app, dir) = app_over("restore", &[("a.png", 64, 48)]);
        assert!(app.panels.show_ui);

        // Hidden, and the window has said once how to get it back.
        let _ = app.perform(Action::ToggleInterface);
        assert!(!app.panels.show_ui);
        assert!(app.said_how_to_restore);

        // Escape brings it back rather than quitting out from under it.
        assert_eq!(app.perform(Action::Dismiss), Effect::Redraw);
        assert!(app.panels.show_ui);
        // And with it back, Escape is the quit it always was.
        assert_eq!(app.perform(Action::Dismiss), Effect::Quit);

        // The key that closes the floating panels on its way says it too:
        // what it hides is the same thing, by the same route.
        app.said_how_to_restore = false;
        let _ = app.perform(Action::ToggleInterfaceAndPanels);
        assert!(app.said_how_to_restore);
        // And `q` leaves from under a hidden interface, as it always did.
        assert_eq!(app.perform(Action::Quit), Effect::Quit);

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// A region on screen takes the keys that move the picture: the arrows
    /// move it a pixel — or its current handle, once one has been clicked or
    /// dragged — `Ctrl` with one grows it, `Space` fits it, and `Esc` takes
    /// it off after a message and before quitting. Stepping to another file
    /// takes it off as well.
    #[test]
    fn a_region_takes_the_keys_that_move_the_picture() {
        use crate::image::region::{Grip, Side};
        use input::{Action, Direction, Effect, PanStep};
        use ui::{Command, Grab};

        let (mut app, dir) = app_over("region", &[("a.png", 64, 48), ("b.png", 64, 48)]);
        assert_eq!(app.selection, Selection::Off);
        assert_eq!(app.perform(Action::ToggleRegion), Effect::Redraw);
        assert_eq!(app.selection, Selection::Armed);

        // Drawn as a drag draws it: from the press to wherever the hand is,
        // every pixel touched taken in.
        let draw = |app: &mut App| {
            app.act(Command::Grab {
                grab: Grab::New,
                at: [10.2, 5.5],
            });
            app.act(Command::Pull([20.9, 15.1]));
            app.act(Command::Release);
        };
        draw(&mut app);
        let region = Region {
            x: 10,
            y: 5,
            width: 11,
            height: 11,
        };
        assert_eq!(app.selection, Selection::Shown(region));
        assert!(app.grabbing.is_none());
        // The words are written while the pointer is on the region, and not
        // otherwise: no clock takes them off.
        assert!(!app.over_region());
        app.pointer.grip = Some(Grip::Inside);
        assert!(app.over_region());
        app.pointer.grip = None;

        // The arrows move the region and leave the view alone, at once: a
        // fresh region's current handle is its middle.
        assert_eq!(app.handle, Grip::Middle);
        let (image, viewport) = (app.image_size(), app.viewport());
        let view = app.view.position(image, viewport);
        let _ = app.perform(Action::Pan(Direction::Right, PanStep::Coarse));
        assert_eq!(app.selection, Selection::Shown(Region { x: 11, ..region }));
        assert_eq!(app.view.position(image, viewport), view);
        assert!(app.motion.is_none());

        // A handle clicked is the current one, and they move that instead.
        // The pointer resting on another handle does not come into it.
        app.act(Command::Handle(Grip::Edge(Side::Right)));
        app.pointer.grip = Some(Grip::Corner(Side::Left, Side::Top));
        assert_eq!(app.handle, Grip::Edge(Side::Right));
        let _ = app.perform(Action::Pan(Direction::Right, PanStep::Coarse));
        assert_eq!(
            app.selection,
            Selection::Shown(Region {
                x: 11,
                width: 12,
                ..region
            })
        );
        // An arrow along that edge moves the whole region instead.
        let _ = app.perform(Action::Pan(Direction::Down, PanStep::Coarse));
        assert_eq!(
            app.selection,
            Selection::Shown(Region {
                x: 11,
                y: 6,
                width: 12,
                height: 11
            })
        );
        app.pointer.grip = None;

        // A drag on a handle makes it current too, without moving it; a
        // move of the whole by its inside leaves the handle as it was; and
        // a hold on the middle handle brings the arrows back to the whole.
        app.act(Command::Grab {
            grab: Grab::Handle(Grip::Edge(Side::Top)),
            at: [16.0, 6.0],
        });
        app.act(Command::Release);
        assert_eq!(app.handle, Grip::Edge(Side::Top));
        app.act(Command::Grab {
            grab: Grab::Handle(Grip::Inside),
            at: [16.0, 10.0],
        });
        app.act(Command::Release);
        assert_eq!(app.handle, Grip::Edge(Side::Top));
        app.act(Command::Grab {
            grab: Grab::Handle(Grip::Middle),
            at: [16.0, 11.0],
        });
        app.act(Command::Release);
        assert_eq!(app.handle, Grip::Middle);
        let _ = app.perform(Action::Pan(Direction::Up, PanStep::Coarse));
        assert_eq!(
            app.selection,
            Selection::Shown(Region {
                x: 11,
                y: 5,
                width: 12,
                height: 11
            })
        );
        // Grown back for the steps below, which read from here.
        let _ = app.perform(Action::Pan(Direction::Down, PanStep::Coarse));

        // Shift with an arrow is not the region's: it pans the picture by
        // a pixel under it, as it does with no region up.
        let view = app.view.position(image, viewport);
        let _ = app.perform(Action::Pan(Direction::Down, PanStep::Fine));
        assert_eq!(
            app.selection,
            Selection::Shown(Region {
                x: 11,
                y: 6,
                width: 12,
                height: 11
            })
        );
        assert_ne!(app.view.position(image, viewport), view);

        // Ctrl grows it that way.
        let _ = app.perform(Action::Pan(Direction::Up, PanStep::Edge));
        assert_eq!(
            app.selection,
            Selection::Shown(Region {
                x: 11,
                y: 5,
                width: 12,
                height: 12
            })
        );
        // Ctrl+Shift shrinks it that way: Left brings the right edge in.
        let _ = app.perform(Action::ShrinkRegion(Direction::Left));
        assert_eq!(
            app.selection,
            Selection::Shown(Region {
                x: 11,
                y: 5,
                width: 11,
                height: 12
            })
        );

        // Space frames the region first and the picture after: the region
        // fitted and filled — a zoom of its own rather than a fit the view
        // keeps — then the picture's two fits and its actual size, and
        // round again.
        assert_eq!(app.view.fit(), Some(Fit::Whole));
        assert_eq!(app.framing, Framing::Region(Fit::Whole));
        let _ = app.perform(Action::CycleFit);
        assert_eq!(app.view.fit(), None);
        assert_eq!(app.framing, Framing::Region(Fit::Fill));
        let _ = app.perform(Action::CycleFit);
        assert_eq!(app.view.fit(), None);
        assert_eq!(app.framing, Framing::Picture(Fit::Whole));
        let _ = app.perform(Action::CycleFit);
        assert_eq!(app.view.fit(), Some(Fit::Whole));
        let _ = app.perform(Action::CycleFit);
        assert_eq!(app.view.fit(), Some(Fit::Fill));
        assert_eq!(app.framing, Framing::Actual);
        let _ = app.perform(Action::CycleFit);
        assert_eq!(app.view.fit(), None);
        assert_eq!(app.framing, Framing::Region(Fit::Whole));
        // A change to the region starts the cycle over at the region.
        let _ = app.perform(Action::CycleFit);
        assert_eq!(app.framing, Framing::Region(Fit::Fill));
        let _ = app.perform(Action::Pan(Direction::Left, PanStep::Coarse));
        assert_eq!(app.framing, Framing::Region(Fit::Whole));

        // Escape takes it off after the message, and before quitting.
        app.toast("Copied region.", Level::Message);
        assert_eq!(app.perform(Action::Dismiss), Effect::Redraw);
        assert!(app.toasts.showing().is_none());
        assert!(app.selection.is_on());
        assert_eq!(app.perform(Action::Dismiss), Effect::Redraw);
        assert_eq!(app.selection, Selection::Off);
        assert!(!app.over_region());
        assert_eq!(app.perform(Action::Dismiss), Effect::Quit);

        // The key with a region up takes it off too, and the arrows are the
        // view's again.
        let _ = app.perform(Action::ToggleRegion);
        draw(&mut app);
        assert!(app.selection.region().is_some());
        let _ = app.perform(Action::ToggleRegion);
        assert_eq!(app.selection, Selection::Off);
        let _ = app.perform(Action::Pan(Direction::Right, PanStep::Coarse));
        assert!(app.motion.is_some(), "a pan of the view is a move");

        // And a region is of the picture it was drawn on: stepping to
        // another file leaves it behind.
        app.motion = None;
        let _ = app.perform(Action::ToggleRegion);
        draw(&mut app);
        app.step(true);
        answer(&mut app, Reload::Fresh);
        assert_eq!(app.files.index(), 1);
        assert_eq!(app.selection, Selection::Off);

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// `Space` is answered on its way up, so that a drag while it is held
    /// can draw a box to zoom to without the view moving first: a tap fits,
    /// a hold with a box drawn under it does not, and the key's repeats are
    /// nothing at all.
    #[test]
    fn space_fits_on_its_way_up_unless_a_box_was_drawn_under_it() {
        use input::{Action, Effect, Space};
        use ui::{Command, Grab};
        use winit::event::ElementState::{Pressed, Released};
        use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};

        let (mut app, dir) = app_over("space", &[("a.png", 64, 48)]);
        let space = Key::Named(NamedKey::Space);
        let at = PhysicalKey::Code(KeyCode::Space);
        assert_eq!(app.view.fit(), Some(Fit::Whole));

        // A tap: nothing moves on the way down — a frame for the pointer,
        // no more — and the fit comes on the way up. Held, the key repeats,
        // and a repeat is the same press still going.
        assert_eq!(app.handle_key(&space, at, Pressed), Effect::Redraw);
        assert_eq!(app.view.fit(), Some(Fit::Whole));
        assert!(app.motion.is_none());
        assert_eq!(app.pointer.space, Space::Held { drawn: false });
        assert_eq!(app.handle_key(&space, at, Pressed), Effect::Nothing);
        assert_eq!(app.pointer.space, Space::Held { drawn: false });
        assert_eq!(app.handle_key(&space, at, Released), Effect::Redraw);
        assert_eq!(app.view.fit(), Some(Fit::Fill));
        assert_eq!(app.pointer.space, Space::Up);

        // Held with a box dragged out under it: the box is not a region,
        // the view goes to it as a move when the drag lets go, and letting
        // go of the key afterwards fits nothing.
        app.motion = None;
        let _ = app.handle_key(&space, at, Pressed);
        app.act(Command::Grab {
            grab: Grab::Zoom,
            at: [10.2, 5.5],
        });
        assert_eq!(app.pointer.space, Space::Held { drawn: true });
        app.act(Command::Pull([20.9, 15.1]));
        assert_eq!(
            app.zoom_box,
            Some(Region {
                x: 10,
                y: 5,
                width: 11,
                height: 11
            })
        );
        assert_eq!(app.selection, Selection::Off);
        app.act(Command::Release);
        assert_eq!(app.zoom_box, None);
        assert!(app.grabbing.is_none());
        assert!(app.motion.is_some(), "the zoom to the box is a move");
        assert_eq!(app.view.fit(), None);
        // Centered on the box — through whatever viewport the application
        // has without a window, which is what the move was made against.
        let (image, viewport) = (app.image_size(), app.viewport());
        let center = app.view.placement(image, viewport).image_point([
            viewport.x + viewport.width / 2.0,
            viewport.y + viewport.height / 2.0,
        ]);
        assert!(
            (center[0] - 15.5).abs() < 0.01 && (center[1] - 10.5).abs() < 0.01,
            "the box is centered: {center:?}"
        );
        assert_eq!(app.handle_key(&space, at, Released), Effect::Redraw);
        assert_eq!(app.view.fit(), None);
        assert_eq!(app.pointer.space, Space::Up);

        // Escape drops a box part way through: the toolkit takes the drag
        // off the hand on the same key, and the release that follows finds
        // nothing to zoom to.
        let _ = app.handle_key(&space, at, Pressed);
        app.act(Command::Grab {
            grab: Grab::Zoom,
            at: [1.0, 1.0],
        });
        app.act(Command::Pull([30.0, 30.0]));
        assert!(app.zoom_box.is_some());
        app.motion = None;
        let before = app.view.position(image, viewport);
        assert_eq!(app.perform(Action::Dismiss), Effect::Redraw);
        assert_eq!(app.zoom_box, None);
        assert!(app.grabbing.is_none());
        app.act(Command::Release);
        assert!(app.motion.is_none());
        assert_eq!(app.view.position(image, viewport), before);
        assert_eq!(app.handle_key(&space, at, Released), Effect::Redraw);
        assert_eq!(app.view.position(image, viewport), before);

        // The window losing the keyboard lets go of the key: its release
        // is going somewhere else.
        let _ = app.handle_key(&space, at, Pressed);
        assert_ne!(app.pointer.space, Space::Up);
        app.keys_lost();
        assert_eq!(app.pointer.space, Space::Up);

        std::fs::remove_dir_all(dir).expect("we just wrote it");
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

    /// A file coming back at the size of the one it is arriving beside is a
    /// comparison, and it is made where the eye is: the pan and zoom carry
    /// over from the picture leaving the screen, whatever this file was left
    /// in the last time it was looked at.
    #[test]
    fn a_same_size_neighbor_takes_the_view_it_arrives_beside() {
        let (mut app, dir) = app_over("compared", &[("a.png", 64, 48), ("b.png", 64, 48)]);

        // b.png is left at 4x, so it has a view of its own to be put back.
        app.step(true);
        answer(&mut app, Reload::Fresh);
        app.view.set_zoom(4.0, app.image_size(), VIEWPORT);

        // Back to a.png, and on to somewhere else in it.
        app.step(false);
        answer(&mut app, Reload::Fresh);
        app.view.set_zoom(2.0, app.image_size(), VIEWPORT);
        let zoom = app.view.zoom(app.image_size(), VIEWPORT);

        // And on to b.png again: at a.png's zoom, not the 4x it was left in.
        app.step(true);
        answer(&mut app, Reload::Fresh);
        assert_eq!(app.files.shown_path(), Some(dir.join("b.png").as_path()));
        assert_eq!(app.view.fit(), None);
        assert_eq!(app.view.zoom(app.image_size(), VIEWPORT), zoom);

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// Flipping between two pictures is how they are compared, so each of
    /// them has to come back as it was left: its own pan and zoom, its own
    /// window and exposure, its own false color.
    #[test]
    fn a_file_comes_back_as_it_was_left() {
        use crate::image::display::Colormap;

        let (mut app, dir) = app_over("kept", &[("a.png", 64, 48), ("b.png", 32, 16)]);
        app.view.set_zoom(4.0, app.image_size(), VIEWPORT);
        let zoom = app.view.zoom(app.image_size(), VIEWPORT);
        let display = app.current.as_mut().expect("a.png is on screen");
        display.display.adjust_exposure(2.0);
        display.display.cycle_colormap();
        let colormap = display.display.colormap;

        // Another size, so nothing carries over: b.png opens fitted and with
        // the display its own pixels ask for.
        app.step(true);
        answer(&mut app, Reload::Fresh);
        assert_eq!(app.view.fit(), Some(Fit::Whole));
        let display = &app.current.as_ref().expect("b.png is on screen").display;
        assert_eq!(display.exposure_stops, 0.0);
        assert_eq!(display.colormap, Colormap::Gray);

        // And back, to everything a.png was left in.
        app.step(false);
        answer(&mut app, Reload::Fresh);
        assert_eq!(app.files.shown_path(), Some(dir.join("a.png").as_path()));
        assert_eq!(app.view.fit(), None);
        assert_eq!(app.view.zoom(app.image_size(), VIEWPORT), zoom);
        let display = &app.current.as_ref().expect("a.png is on screen").display;
        assert_eq!(display.exposure_stops, 2.0);
        assert_eq!(display.colormap, colormap);

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// A file rewritten under the window is the same file being read again,
    /// not a return to it: one that comes back a different size is a new
    /// shape and is fitted afresh, rather than being put back into the view
    /// it was being looked at in.
    #[test]
    fn a_reload_is_not_a_return() {
        let (mut app, dir) = app_over("reloaded", &[("a.png", 64, 48)]);
        app.view.set_zoom(4.0, app.image_size(), VIEWPORT);
        assert_eq!(app.view.fit(), None);

        write_png(&dir, "a.png", 32, 16);
        let request = app.files.reload().expect("nothing else is being read");
        app.send(request);
        answer(&mut app, Reload::InPlace);

        assert_eq!(app.image_size(), [32.0, 16.0]);
        assert_eq!(app.view.fit(), Some(Fit::Whole), "a new shape to fit");

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
            Some(dir.join("a.png").as_path()),
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
        assert_eq!(app.files.shown_path(), Some(dir.join("a.png").as_path()));
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
                sequence: Sequence::Still,
                page: 0,
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

    /// A fixture from `test_images/`, opened on its own.
    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_images")
            .join(name)
    }

    /// Waits for the player to have every frame, which a two-frame fixture
    /// takes no time over.
    fn decoded_to_the_end(app: &App) {
        let player = app.player.as_ref().expect("an animation has a player");
        let started = Instant::now();
        while !player.read(|cache| cache.complete() || cache.error().is_some()) {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the player never finished"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        player.read(|cache| assert_eq!(cache.error(), None));
    }

    /// The pixel at `(x, y)` of the picture on screen, as bytes.
    fn shown_pixel(app: &App, x: u32, y: u32) -> Vec<u8> {
        let image = &app.current.as_ref().expect("a picture is up").image;
        let count = image.channels().count();
        let start = (y as usize * image.width as usize + x as usize) * count;
        match &image.samples {
            crate::image::Samples::U8 { data, .. } => data[start..start + count].to_vec(),
            other => panic!("{other:?}"),
        }
    }

    /// An animation arriving starts playing from its first frame, and the
    /// frame on screen follows the clock: the picture and its statistics
    /// are the frame's, so that everything reading them reads the frame.
    #[test]
    fn an_animation_plays_and_the_picture_follows_the_clock() {
        use input::Action::{NextFrame, PreviousFrame, TogglePlay};

        let path = fixture("gif-animated.gif");
        let mut app = open(vec![path.clone()], vec![path]);
        answer(&mut app, Reload::Fresh);

        let playback = app.playback.as_ref().expect("an animation has a clock");
        assert!(playback.playing());
        assert_eq!(playback.count(), 2);
        assert_eq!(app.uploaded, None, "the file's own decode is up first");
        let first = shown_pixel(&app, 8, 6);
        assert_eq!(&first[..3], [255, 0, 0], "red quadrant first");
        decoded_to_the_end(&app);

        // A tenth and a half later the second frame is due.
        let (changed, deadline) = app.tick_playback(Instant::now() + Duration::from_millis(150));
        assert!(changed);
        assert!(deadline.is_some(), "the next frame has a time");
        assert_eq!(app.playback.as_ref().unwrap().head(), 1);
        app.show_due_frame();
        assert_eq!(app.uploaded, Some(1));
        let second = shown_pixel(&app, 8, 6);
        assert_eq!(&second[..3], [255, 255, 255], "the pattern upside down");

        // A step pauses and moves; play resumes.
        assert_eq!(app.perform(NextFrame), Effect::Redraw);
        let playback = app.playback.as_ref().unwrap();
        assert!(!playback.playing());
        assert_eq!(playback.head(), 0);
        assert_eq!(app.perform(PreviousFrame), Effect::Redraw);
        assert_eq!(app.playback.as_ref().unwrap().head(), 1);
        assert_eq!(app.perform(TogglePlay), Effect::Redraw);
        assert!(app.playback.as_ref().unwrap().playing());
        app.show_due_frame();
        assert_eq!(app.uploaded, Some(1));
    }

    /// `--paused` opens an animation stopped, and a still has no clock for
    /// the keys to act on.
    #[test]
    fn paused_opens_stopped_and_a_still_has_no_clock() {
        use input::Action::TogglePlay;

        let path = fixture("webp-animated.webp");
        let mut app = open(vec![path.clone()], vec![path]);
        app.open_paused = true;
        answer(&mut app, Reload::Fresh);
        let playback = app.playback.as_ref().expect("an animation has a clock");
        assert!(!playback.playing());
        assert_eq!(
            app.tick_playback(Instant::now() + Duration::from_secs(1)),
            (false, None)
        );

        let path = fixture("png-rgb8.png");
        let mut app = open(vec![path.clone()], vec![path]);
        answer(&mut app, Reload::Fresh);
        assert!(app.playback.is_none() && app.player.is_none());
        assert_eq!(app.perform(TogglePlay), Effect::Nothing);
    }

    /// Stepping away from an animation and back finds it on the frame it
    /// was left on, stopped if it was stopped; a reload starts it over.
    #[test]
    fn an_animation_comes_back_to_the_frame_it_was_left_on() {
        use input::Action::NextFrame;

        let (dir, stills) = written("left-frame", &[("a.png", 32, 24)]);
        let animated = fixture("gif-animated.gif");
        let mut app = open(vec![animated.clone(), stills[0].clone()], vec![]);
        answer(&mut app, Reload::Fresh);
        let _ = app.perform(NextFrame);
        assert_eq!(app.playback.as_ref().unwrap().head(), 1);

        app.step(true);
        answer(&mut app, Reload::Fresh);
        assert!(app.playback.is_none(), "a still has no clock");
        app.step(true);
        answer(&mut app, Reload::Fresh);
        let playback = app.playback.as_ref().expect("back on the animation");
        assert_eq!(playback.head(), 1);
        assert!(
            !playback.playing(),
            "left stopped, so it comes back stopped"
        );

        let request = app.files.reload().expect("nothing is in flight");
        app.send(request);
        answer(&mut app, Reload::InPlace);
        let playback = app.playback.as_ref().expect("still an animation");
        assert_eq!(playback.head(), 0);
        assert!(playback.playing(), "read again, it starts over");

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// A paged file opens on its default page and steps through the rest,
    /// keeping the display and, at the same size, the view; a page request
    /// waits for the one in flight; and the page it was left on is the one
    /// it comes back to.
    #[test]
    fn a_paged_file_steps_through_its_pages() {
        use input::Action::{NextFrame, PreviousFrame};

        let (dir, stills) = written("pages", &[("a.png", 32, 24)]);
        let paged = fixture("tiff-pages.tif");
        let mut app = open(vec![paged.clone(), stills[0].clone()], vec![]);
        answer(&mut app, Reload::Fresh);
        let current = app.current.as_ref().unwrap();
        assert_eq!(current.page, 0);
        assert!(matches!(current.sequence, Sequence::Pages { count: 2, .. }));
        assert!(app.playback.is_none(), "pages have no clock");
        assert_eq!(&shown_pixel(&app, 8, 6)[..3], [255, 0, 0]);

        app.view.set_zoom(1.0, app.image_size(), VIEWPORT);
        app.view.zoom_in(app.image_size(), VIEWPORT);
        let zoom = app.view.zoom(app.image_size(), VIEWPORT);
        if let Some(current) = app.current.as_mut() {
            current.display.exposure_stops = 1.0;
        }

        assert_eq!(app.perform(NextFrame), Effect::Nothing);
        assert_eq!(
            app.files.pending().and_then(|pending| pending.page),
            Some(1)
        );
        // Held down: the second press waits for the first to land.
        assert_eq!(app.perform(NextFrame), Effect::Nothing);
        answer(&mut app, Reload::Page);
        let current = app.current.as_ref().unwrap();
        assert_eq!(current.page, 1);
        assert_eq!(
            &shown_pixel(&app, 8, 6)[..3],
            [255, 255, 255],
            "upside down"
        );
        assert_eq!(current.display.exposure_stops, 1.0, "the display stays");
        assert_eq!(
            app.view.zoom(app.image_size(), VIEWPORT),
            zoom,
            "the view stays at the same size"
        );

        // Round the end, back to the first.
        let _ = app.perform(NextFrame);
        answer(&mut app, Reload::Page);
        assert_eq!(app.current.as_ref().unwrap().page, 0);
        let _ = app.perform(PreviousFrame);
        answer(&mut app, Reload::Page);
        assert_eq!(app.current.as_ref().unwrap().page, 1);

        // Away and back: the page it was left on.
        app.step(true);
        answer(&mut app, Reload::Fresh);
        app.step(true);
        assert_eq!(
            app.files.pending().and_then(|pending| pending.page),
            Some(1)
        );
        answer(&mut app, Reload::Fresh);
        assert_eq!(app.current.as_ref().unwrap().page, 1);

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// What the window last said, for the tests about what a deletion or
    /// a rename says.
    fn said(app: &App) -> String {
        app.toasts
            .showing()
            .map(|toast| toast.message.clone())
            .unwrap_or_default()
    }

    /// The key that undoes, as the messages name it, is the key that
    /// undoes: a message naming a key that did something else would send
    /// the reader to the wrong key at the worst moment.
    #[test]
    fn the_messages_name_the_key_that_undoes() {
        use crate::app::input::{Action, KEYS, KeyName};
        let binding = KEYS
            .iter()
            .find(|binding| {
                binding
                    .keys
                    .iter()
                    .any(|(_, action)| *action == Action::Undo)
            })
            .expect("undo is bound");
        assert_eq!(binding.shown, "Ctrl+Z");
        assert!(binding.mods.control_key());
        assert!(
            binding
                .keys
                .iter()
                .any(|(key, _)| *key == KeyName::Char("z")),
            "the plain letter under Ctrl"
        );
    }

    /// A deletion moves the file to the trash and steps on; the file leaves
    /// the list once its neighbor is up; undo puts it back on disk and on
    /// the list, and shows it again.
    #[test]
    fn a_deleted_file_goes_to_the_trash_and_comes_back_on_undo() {
        use crate::app::input::Action;
        let (mut app, dir) = opening_directory(
            "trash-step",
            &[("a.png", 8, 8), ("b.png", 8, 8), ("c.png", 8, 8)],
        );
        answer(&mut app, Reload::Fresh);
        app.trash = Some(Trash::under(dir.join("Trash")));

        assert_eq!(app.perform(Action::Delete), Effect::Redraw);
        assert!(!dir.join("a.png").exists());
        assert!(dir.join("Trash/files/a.png").exists());
        assert!(dir.join("Trash/info/a.png.trashinfo").exists());
        assert_eq!(said(&app), "Trashed a.png. Ctrl+Z to undo.");
        assert!(app.watch.missing(), "the bar says so at once");
        assert_eq!(
            app.files.len(),
            3,
            "still on the list while it is on screen"
        );
        assert_eq!(
            app.files.pending().map(|pending| pending.index),
            Some(1),
            "the next file is asked for"
        );
        // Held down: nothing more happens until the neighbor is up.
        assert_eq!(app.perform(Action::Delete), Effect::Redraw);
        assert!(dir.join("b.png").exists());

        answer(&mut app, Reload::Fresh);
        assert_eq!(app.files.len(), 2);
        assert_eq!(app.files.shown_path(), Some(dir.join("b.png").as_path()));
        assert_eq!(app.files.index(), 0);
        assert!(app.conditions().undoable);

        assert_eq!(app.perform(Action::Undo), Effect::Redraw);
        assert!(dir.join("a.png").exists(), "back where it was");
        assert!(!dir.join("Trash/files/a.png").exists());
        assert_eq!(app.files.len(), 3);
        assert_eq!(app.files.path(0), dir.join("a.png"));
        assert_eq!(
            app.files.pending().map(|pending| pending.index),
            Some(0),
            "and shown again"
        );
        answer(&mut app, Reload::Fresh);
        assert_eq!(app.files.shown_path(), Some(dir.join("a.png").as_path()));
        assert!(!app.conditions().undoable);
        assert_eq!(app.perform(Action::Undo), Effect::Redraw);
        assert_eq!(said(&app), "Nothing to undo.");

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// The only file has nowhere to step to: it stays on screen, marked as
    /// gone, a second press does nothing, and undo puts it back where it
    /// stands.
    #[test]
    fn deleting_the_only_file_keeps_it_on_screen() {
        use crate::app::input::Action;
        let (mut app, dir) = app_over("trash-alone", &[("a.png", 8, 8)]);
        app.trash = Some(Trash::under(dir.join("Trash")));

        let _ = app.perform(Action::Delete);
        assert!(!dir.join("a.png").exists());
        assert!(app.files.is_idle());
        assert!(app.watch.missing());
        assert_eq!(app.files.len(), 1);
        assert!(app.files.is_condemned(&dir.join("a.png")));

        let _ = app.perform(Action::Delete);
        assert_eq!(said(&app), "Already in the trash.");
        assert_eq!(app.edits.len(), 1);

        let _ = app.perform(Action::Undo);
        assert!(dir.join("a.png").exists());
        assert!(!app.watch.missing());
        assert!(!app.files.is_condemned(&dir.join("a.png")));
        assert!(app.files.is_idle(), "still on screen: nothing to ask for");
        assert_eq!(app.files.len(), 1);

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// A file that has been emptied from the trash cannot come back, and
    /// the window says so rather than failing quietly.
    #[test]
    fn an_emptied_trash_has_nothing_to_put_back() {
        use crate::app::input::Action;
        let (mut app, dir) = app_over("trash-emptied", &[("a.png", 8, 8), ("b.png", 8, 8)]);
        app.trash = Some(Trash::under(dir.join("Trash")));
        let _ = app.perform(Action::Delete);
        answer(&mut app, Reload::Fresh);
        std::fs::remove_dir_all(dir.join("Trash")).expect("emptied");

        let _ = app.perform(Action::Undo);
        assert!(
            said(&app).contains("no longer in the trash"),
            "{}",
            said(&app)
        );
        assert!(app.files.is_idle());
        assert_eq!(app.files.len(), 1);
        assert!(!app.conditions().undoable, "the entry is spent");

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// The dialog judges the name as it is typed — taken, unchanged, an
    /// extension changing — and OK renames the file everywhere it is known
    /// by name; undo renames it back, and shows it if it had been left.
    #[test]
    fn a_rename_is_judged_as_typed_and_undone_by_name() {
        use crate::app::input::Action;
        use crate::ui::rename::{ExtensionChange, Verdict};
        let (mut app, dir) = app_over("rename", &[("a.png", 8, 8), ("b.png", 8, 8)]);

        let _ = app.perform(Action::Rename);
        let input = app.rename_input().expect("the dialog is up");
        assert_eq!(input.name, "a.png");
        assert_eq!(input.verdict, Verdict::Unchanged);
        assert!(input.opened);
        assert!(!app.rename_input().expect("still up").opened);

        assert!(app.act(ui::Command::Name("b.png".to_string())));
        assert_eq!(app.rename_input().expect("up").verdict, Verdict::Taken);
        assert!(app.act(ui::Command::Name("c.jpg".to_string())));
        assert_eq!(
            app.rename_input().expect("up").verdict,
            Verdict::Fine(Some(ExtensionChange {
                from: Some("png".to_string()),
                to: Some("jpg".to_string()),
            }))
        );

        // Cancel changes nothing.
        assert!(app.act(ui::Command::Press(ui::Control::CancelRename)));
        assert!(app.rename_input().is_none());
        assert!(dir.join("a.png").exists());

        let _ = app.perform(Action::Rename);
        assert!(app.act(ui::Command::Name("c.jpg".to_string())));
        assert!(app.act(ui::Command::Press(ui::Control::RenameTo)));
        assert!(app.rename_input().is_none());
        assert!(dir.join("c.jpg").exists() && !dir.join("a.png").exists());
        assert_eq!(app.files.shown_path(), Some(dir.join("c.jpg").as_path()));
        assert_eq!(
            app.current.as_ref().map(|current| current.label.as_str()),
            Some("c.jpg")
        );
        assert_eq!(said(&app), "Renamed a.png. Ctrl+Z to undo.");
        assert!(app.conditions().undoable);

        // Step away, then undo: the old name is back, and so is the file.
        app.step(true);
        answer(&mut app, Reload::Fresh);
        assert_eq!(app.files.shown_path(), Some(dir.join("b.png").as_path()));
        let _ = app.perform(Action::Undo);
        assert!(dir.join("a.png").exists() && !dir.join("c.jpg").exists());
        assert_eq!(app.files.path(0), dir.join("a.png"));
        assert_eq!(app.files.pending().map(|pending| pending.index), Some(0));
        answer(&mut app, Reload::Fresh);
        assert_eq!(app.files.shown_path(), Some(dir.join("a.png").as_path()));
        assert_eq!(
            app.current.as_ref().map(|current| current.label.as_str()),
            Some("a.png")
        );

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }

    /// A rename refuses to replace: a file made under the new name since
    /// the dialog judged it is left alone, and the window says so.
    #[test]
    fn a_rename_does_not_replace_a_file_that_has_arrived() {
        use crate::app::input::Action;
        let (mut app, dir) = app_over("rename-race", &[("a.png", 8, 8)]);
        let _ = app.perform(Action::Rename);
        assert!(app.act(ui::Command::Name("b.png".to_string())));
        write_png(&dir, "b.png", 4, 4);
        assert!(app.act(ui::Command::Press(ui::Control::RenameTo)));
        assert!(dir.join("a.png").exists());
        assert_eq!(app.files.shown_path(), Some(dir.join("a.png").as_path()));
        assert!(said(&app).contains("already there"), "{}", said(&app));
        assert!(!app.conditions().undoable);

        std::fs::remove_dir_all(dir).expect("we just wrote it");
    }
}
