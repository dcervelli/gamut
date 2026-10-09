//! Window lifecycle, key handling, and building each frame's interface.

mod animation;
pub mod arranging;
mod chooser;
mod copying;
mod edits;
mod exporting;
mod files;
mod filmstrip;
pub mod folder;
mod gui;
pub mod input;
mod kept;
pub mod keymap;
pub mod measuring;
#[cfg(target_os = "macos")]
mod menubar;
mod order;
mod playback;
mod region;
mod tags;
mod visited;
mod window;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::event::{KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

use crate::exiftool;
use crate::gestures::{Gestures, Surface};
use crate::image::DecodedImage;
use crate::image::auxiliary::{Auxiliary, Showing};
use crate::image::decode::{self, CameraJpeg, Rendering};
use crate::image::display::{Display, Headroom, Startup};
use crate::image::orient::Turn;
use crate::image::sequence::Sequence;
use crate::loader::{Decoded, Loader, Opened, Ready, Reload, Source};
use crate::monitor::{self, Mode, Monitors};
use crate::motion::Motion;
use crate::openers::{self, Opener};
use crate::player;
use crate::portal::{self, Pick, Picked};
use crate::render::{Draw, GpuImage, HdrPreference, Placement, Reduced, Renderer, Scene, Upscale};
use crate::settings::{Config, State, StateFile};
use crate::theme::{self, Theme};
use crate::thumbnailer::{Delivered, Facts, News, Thumb, Thumbnailer};
use crate::timing;
use crate::trash::Trash;
use crate::ui::chrome::{Parts, content_area, image_viewport};
use crate::ui::toast::{self, Level, Toasts};
use crate::ui::tooltip::Hdr;
use crate::ui::{self, Current, FileFacts, FrameInput, Panels, Rect, Toast};
use crate::view::{View, Viewport};
use crate::watch::{self, Watch};
use filmstrip::Filmstrip;
use folder::{Folder, Then};
use visited::Visited;

use animation::Animation;
use chooser::{Chooser, Thumbs};
use copying::{Copying, Done};
use edits::{Edit, Renaming};
use exporting::Exporting;
use files::{Announce, Asked, Files};
use gui::Gui;
use input::{Effect, Pointer};
use kept::{Kept, Left, Settings};
use measuring::{Measured, Measuring};
use region::Marking;
use tags::Tags;
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
    /// A picture this program could show has arrived on the clipboard, or
    /// the one there has gone — see `clipboard::watch`.
    Clipboard(bool),
    /// The folder beside a single file has been read — see
    /// [`folder`].
    Folder(folder::Listed),
    /// A list about to be opened has been read for its order — see
    /// [`arranging`].
    Arranged(arranging::Arranged),
    /// The picture has been measured through its lift — see [`measuring`].
    /// Boxed: it carries the histograms.
    Measured(Box<Measured>),
    /// exiftool has read a file for the info panel's Tags tab — see
    /// [`exiftool`]. Boxed: it carries every tag.
    Tags(Box<exiftool::Delivered>),
    /// The desktop has asked for these files to be opened: on a Mac, what
    /// Finder sends — see `finder`. Nothing sends it elsewhere, where the
    /// files arrive on the command line.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Opened(Vec<PathBuf>),
    /// Something was chosen in the Mac's menu bar with the pointer — see
    /// `menubar`.
    #[cfg(target_os = "macos")]
    Menu(crate::menubar::Chosen),
}

/// How a thread hands the loop a `T`: a function it calls from wherever it
/// is, which sends the `T` on as the [`UserEvent`] it stands for. Made by
/// [`Threads::new`] from the loop's proxy — see [`deliver`] — and by the
/// tests from nothing at all.
pub type Deliver<T> = Arc<dyn Fn(T) + Send + Sync>;

impl From<Decoded> for UserEvent {
    fn from(decoded: Decoded) -> Self {
        UserEvent::Decoded(Box::new(decoded))
    }
}

impl From<player::Event> for UserEvent {
    fn from(event: player::Event) -> Self {
        UserEvent::Frame(event)
    }
}

impl From<Delivered> for UserEvent {
    fn from(delivered: Delivered) -> Self {
        UserEvent::Thumbnail(Box::new(delivered))
    }
}

impl From<Picked> for UserEvent {
    fn from(picked: Picked) -> Self {
        UserEvent::Picked(picked)
    }
}

impl From<folder::Listed> for UserEvent {
    fn from(listed: folder::Listed) -> Self {
        UserEvent::Folder(listed)
    }
}

impl From<arranging::Arranged> for UserEvent {
    fn from(arranged: arranging::Arranged) -> Self {
        UserEvent::Arranged(arranged)
    }
}

impl From<Measured> for UserEvent {
    fn from(measured: Measured) -> Self {
        UserEvent::Measured(Box::new(measured))
    }
}

impl From<exiftool::Delivered> for UserEvent {
    fn from(delivered: exiftool::Delivered) -> Self {
        UserEvent::Tags(Box::new(delivered))
    }
}

/// A [`Deliver`] that sends each `T` to the loop as the user event it
/// stands for, whether or not the loop is still there to take it: for a
/// thread that finishes what it was asked and stops.
fn deliver<T: Into<UserEvent> + 'static>(proxy: EventLoopProxy<UserEvent>) -> Deliver<T> {
    Arc::new(move |sent| {
        let _ = proxy.send_event(sent.into());
    })
}

/// The same, saying whether the loop was still there to take it: what a
/// thread that runs for the session asks for, so that it can stop once the
/// window has gone.
fn waking<T: Into<UserEvent> + 'static>(
    proxy: EventLoopProxy<UserEvent>,
) -> impl Fn(T) -> bool + Send + Sync + 'static {
    move |sent| proxy.send_event(sent.into()).is_ok()
}

/// The other threads, and how each reaches the loop: made from the event
/// loop by [`Threads::new`], which is the only place a proxy to it can come
/// from, and by the tests from stubs.
pub struct Threads {
    pub loader: Loader,
    /// How a player wakes the loop; one is started per animated file.
    pub wake: player::Wake,
    pub monitors: Option<Monitors>,
    /// The thread making the chooser's thumbnails, over the whole session.
    pub thumbnailer: Thumbnailer,
    /// How the desktop's file dialog hands its answer back.
    pub picker: portal::Deliver,
    /// How the folder beside a single file comes back once it is read.
    pub folder: folder::Deliver,
    /// How a list read for its order comes back.
    pub arranged: arranging::Deliver,
    /// How the picture measured through its lift comes back.
    pub measured: measuring::Deliver,
    /// How exiftool's reading of a file comes back.
    pub tags: exiftool::Deliver,
}

impl Threads {
    /// Starts the threads that run for the session and makes the way back
    /// for those started later, each with a proxy of its own to the loop.
    /// The thread watching the clipboard is started here too; it reports
    /// through the loop and is never spoken to again, so it has no handle.
    /// A thread added is a variant of [`UserEvent`], a `From` for it, an
    /// arm of `user_event` and a field here.
    pub fn new(event_loop: &EventLoop<UserEvent>, overrides: decode::Overrides) -> Self {
        let proxy = || event_loop.create_proxy();
        let monitor = proxy();
        let clipboard = proxy();
        crate::clipboard::watch(watch::INTERVAL, move |offered| {
            clipboard.send_event(UserEvent::Clipboard(offered)).is_ok()
        });
        Self {
            loader: Loader::new(waking(proxy())),
            wake: Arc::new(waking(proxy())),
            monitors: monitor::watch(move || {
                let _ = monitor.send_event(UserEvent::Monitor);
            }),
            thumbnailer: Thumbnailer::new(overrides, waking(proxy())),
            picker: deliver(proxy()),
            folder: deliver(proxy()),
            arranged: deliver(proxy()),
            measured: deliver(proxy()),
            tags: deliver(proxy()),
        }
    }
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

/// `files` in `order`, as far as what is known of each — `glimpsed` — can
/// put them: files the order knows nothing of keep the order they stand in,
/// after those it does. What `main` puts the command line's list in when
/// the order needs only the names, and what files chosen in the window are
/// put in once they have been read for it (see [`arranging`]).
pub fn arranged(
    files: Vec<PathBuf>,
    order: ui::filmstrip::Order,
    glimpsed: &folder::Glimpses,
) -> Vec<PathBuf> {
    let places = order::arrange(files.len(), order, |index| {
        order::Key::of(&files[index], None, glimpsed.get(&files[index]))
    });
    places
        .into_iter()
        .map(|index| files[index].clone())
        .collect()
}

/// What the command line asked for, beyond which files to show.
pub struct Options {
    pub overrides: decode::Overrides,
    pub startup: Startup,
    pub hdr: HdrPreference,
    /// The configuration file, with the command line's flags for the same
    /// panels laid over it.
    pub config: Config,
    pub upscale: Upscale,
    /// What `--size` asked the window to open at, in logical pixels.
    pub size: Option<[u32; 2]>,
    /// Whether an animation opens stopped on its first frame.
    pub paused: bool,
}

/// How long the window is held to the size it asked for, waiting on the
/// compositor to answer — see [`App::size_window_to`]. A compositor that
/// is going to answer does so within a frame or two; one that has the
/// window tiled never does, and must not leave the window pinned.
const SIZING_GRACE: Duration = Duration::from_secs(1);

/// What the surface goes out to. `asked` is what was asked of it: by
/// `--output`, and by every press of the switch since, read against the
/// monitor's mode in [`App::surface_hdr`] and [`App::headroom`]. The rest
/// is the compositor's word, where it gives one: `monitors` its table,
/// `mode` that of the monitor the window is on as last read — `None` until
/// the window has landed on one, and for good where nothing says — and
/// `headroom` its room above white, its peak over its white, read with the
/// mode: what a gain map's lift is weighed against on an HDR surface.
struct Output {
    asked: HdrPreference,
    monitors: Option<Monitors>,
    mode: Option<Mode>,
    headroom: Option<f32>,
}

/// How the window is sized to the picture. `header` is what the header
/// said the first file's size was, so that the window can open at the
/// right shape before the pixels arrive: only ever consulted while there
/// is no picture, and `None` for a format whose header would not say.
/// `asked` is the size `--size` asked the window to open at, in logical
/// pixels, read once when the window is made; every size after that is
/// the compositor's to give. `to_next` is whether the next picture to
/// arrive is to size the window, as the first sizes it at start-up: set
/// while the window shows nothing — it opened on nothing, or the last file
/// was deleted — and spent by the arrival; a window opened at `--size`
/// keeps the size it was asked for, that being a choice rather than a
/// default. `sized_for` is the size the window was given ahead of the
/// picture that spends `to_next`, from its header: the arrival sizes the
/// window again only where the picture turns out another size — turned by
/// its orientation, or a file that would not read walked past. `held` is
/// when the window was last held to a size, while it is: the moment
/// [`App::size_window_to`] pinned its least and greatest size to the one
/// it asked for, which is released when the compositor has answered or
/// after [`SIZING_GRACE`] — see there for why the size is asked for that
/// way.
struct Sizing {
    header: Option<[f32; 2]>,
    asked: Option<[u32; 2]>,
    to_next: bool,
    sized_for: Option<[f32; 2]>,
    held: Option<Instant>,
}

/// One reading of the window and the view, taken at the top of a frame
/// or an event and read by everything under it: the picture's viewport,
/// which panels are up, the window in logical pixels, and the view as it
/// is on screen at that instant. Everything that reads where the picture
/// is reads the same one, so that the pixel under the pointer, the loupe
/// and the frame drawn agree with one another mid-move — and so that the
/// panels' geometry is worked out once rather than by each of them.
#[derive(Clone, Copy)]
struct Sight {
    /// Physical pixels to the interface's point — the monitor's scale and
    /// the interface's together — what egui lays out in.
    scale: f32,
    /// The window in logical pixels.
    logical: [f32; 2],
    /// Where the image is drawn, in physical pixels.
    viewport: Viewport,
    /// Which of the panels that come and go are up.
    parts: Parts,
    /// The view as shown at the reading's instant — see [`App::view_at`].
    view: View,
}

/// What there is once the window is open, made together in `resumed`.
struct Shown {
    /// First, so that the surface goes before the window it draws into.
    renderer: Renderer,
    /// The toolkit's context and its adapter to the window.
    gui: Gui,
    window: Arc<Window>,
}

pub struct App {
    files: Files,
    /// What the command line said about how to read every file: what the
    /// loader is told with each request.
    overrides: decode::Overrides,
    current: Option<Current>,
    startup: Startup,
    /// What the surface goes out to: what was asked of it, and what the
    /// monitor the window is on is in.
    output: Output,
    /// How the window is sized to the picture: ahead of the first file
    /// from its header, and again by a picture arriving into an empty one.
    sizing: Sizing,
    /// The folder beside a single file named alone — see [`folder`].
    beside: folder::Beside,
    /// Lists being read for the list's order before any of them is shown
    /// — see [`arranging`].
    arranging: arranging::Arrangings,
    /// Which of a raw's two pictures is asked for: the developed frame, or
    /// the camera's JPEG. One preference for every raw, kept between runs;
    /// a raw with no JPEG in it shows its developed picture whatever it is.
    rendering: Rendering,
    /// Where the view is going: the pan and zoom every key and press act
    /// on. What is on screen is `App::shown_view`, which is this once it
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
    /// The Mac's menu bar, once the application has finished launching and
    /// it is installed; and until then, how a choice made in it will reach
    /// the loop — see [`menubar`].
    #[cfg(target_os = "macos")]
    menubar: Option<menubar::MenuBar>,
    #[cfg(target_os = "macos")]
    menu_deliver: Option<crate::menubar::Deliver>,
    /// The paths as the command line gave them, and a watch on each directory
    /// among them. A directory is a place to look rather than a fixed list:
    /// images appearing in it or disappearing from it while the window is open
    /// join or leave the walk, noticed on the same cadence as a write to the
    /// file on screen. Empty — and so costing nothing — when every path named
    /// was a file.
    named: Vec<PathBuf>,
    directories: Vec<Watch>,
    /// The web address the map button opens, with `{lat}` and `{lng}` for
    /// the coordinates: the setting.
    open_map_link: String,
    /// Where exiftool is, for the info panel's Tags tab: the setting, and
    /// the program it was found as.
    exiftool: exiftool::Program,
    /// What the Tags tab holds, and how a run of exiftool comes back.
    tags: Tags,
    tags_delivered: exiftool::Deliver,
    glimpsed: HashMap<PathBuf, folder::Glimpse>,
    /// The folder the last picture shown came from, whole, which the
    /// desktop's file dialog starts in; and that folder while the empty
    /// window offers to open it, which it does where it still holds images.
    last_folder: Option<PathBuf>,
    offered_folder: Option<PathBuf>,
    /// The colors everything is drawn in, and the palette file they came
    /// from, watched on the same cadence as the image: Omarchy rewrites it
    /// wholesale when the desktop's theme changes, and the window should
    /// follow rather than stay in the theme it opened under.
    theme: Theme,
    theme_watch: theme::Watch,
    /// How large the interface is drawn, on top of the monitor's own scale:
    /// one of [`ui::scale::SCALES`], or a value between two of them that the
    /// state file was given by hand. The picture is placed in device pixels
    /// whatever it is.
    ui_scale: f32,
    /// The configuration file, watched on the same cadence, so that a key
    /// or a gesture changed in it is in force once the file is saved — see
    /// [`App::reconfigure`]. Idle until `main` asks for it: the tests build
    /// the application from a configuration of their own, and the user's
    /// file is never theirs to read.
    config_watch: Watch,
    /// When to look at it next.
    next_poll: Instant,
    /// The thread that reads files.
    ///
    /// Declared before the renderer on purpose: fields are dropped in the
    /// order they are written, and the loader has been given a handle on the
    /// GPU device. Shutting the thread down first means the last reference to
    /// that device is the renderer's, and so the device is destroyed here on
    /// the thread that made it. See [`Loader::drop`] for what goes wrong when
    /// the thread is still running as the process leaves `main`.
    loader: Loader,
    /// The animation on screen — the thread decoding its frames, its clock
    /// and which frame is up — and `None` for a still or a paged file.
    /// Before the renderer for the loader's reason: the thread has no
    /// handle on the device, but one still decoding as the process leaves
    /// `main` is a thread to have joined.
    animation: Option<Animation>,
    /// The thread making thumbnails of every file on the list, for the
    /// chooser. Told to stop rather than joined — see its own account.
    thumbnailer: Thumbnailer,
    /// The chooser's state: the query, which files fit it, what is known
    /// about each. Whether the popup is open is egui's — see
    /// [`App::chooser_open`].
    chooser: Chooser,
    /// The thumbnails the screen holds, as egui textures.
    thumbs: Thumbs,
    /// The file list's state: the order the list stands in, and the rows
    /// the strip was last drawn from. Whether the strip is up is
    /// [`Panels::show_filmstrip`].
    filmstrip: Filmstrip,
    /// Where what is kept from one run to the next is written back to when
    /// the window closes: the file list's width and the loupe's
    /// magnification.
    state: StateFile,
    /// The files that have been on screen, for going back and forward
    /// through them.
    visited: Visited,
    /// Thumbnails that arrived before there was a context to make textures
    /// in, taken up at the first frame.
    pending_thumbs: Vec<(PathBuf, Thumb)>,
    /// How long the last file of each format took to arrive, in seconds
    /// per pixel, by the name of the decoder that read it: what a read is
    /// judged slow from before it has started — see
    /// [`App::predicted_slow`].
    read_rates: HashMap<&'static str, f64>,
    /// How a player wakes the loop: what `main` made from the loop's proxy.
    wake: player::Wake,
    /// Numbers the players, so that news from one dropped with its file is
    /// told from the one now playing.
    players: u64,
    /// Whether an animation opens stopped on its first frame: `--paused`.
    open_paused: bool,
    /// The window, the renderer drawing into it and the toolkit's context
    /// on it, which are made together in `resumed`: one without the others
    /// is never the case. `None` until then, and in a test, which has no
    /// window. After the loader and the player on purpose — see `loader`.
    shown: Option<Shown>,
    pointer: Pointer,
    panels: Panels,
    /// The region marked out on the picture, and the hand on it.
    marking: Marking,
    /// The message about what was just done, and when it takes itself off.
    /// The one thing on screen that time alone changes.
    toasts: Toasts,
    /// The reasons a picture has lost precision on this device that a toast
    /// has already given: every file of the kind loses the same, and saying
    /// so once is telling the reader about the device, not the file.
    reduced_said: HashSet<Reduced>,
    /// The copies of the picture being prepared on threads of their own,
    /// and how they report back.
    copying: Copying,
    /// The picture measured through its lift on a thread of its own, and
    /// the measures a switch moves between.
    measuring: Measuring,
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
    /// The export dialog, while it is up.
    exporting: Option<Exporting>,
    /// How the desktop's file dialog hands its answer back: what `main`
    /// made from the loop's proxy.
    picker: portal::Deliver,
    /// Whether that dialog is up. One at a time: the buttons that open it
    /// are drawn dead while it is, and the key does nothing.
    picking: bool,
    /// Whether the clipboard holds a picture this program could show, as
    /// the thread watching it last said. `FrameInput::paste` is this and
    /// the interface being on screen.
    clipboard_offers: bool,
    /// Which of the toggles for an image the picture carries is on — the
    /// depth map's or the gain map's — if either: that image is shown in the
    /// picture's place, for this picture and every one after it that
    /// carries one — see [`App::follow_showing`]. One at a time, since only
    /// one image is up. Here rather than in [`Panels`], which nothing under
    /// `ui` reads it from.
    show_beside: Option<Auxiliary>,
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
    /// What each key's name is bound to, and what each gesture does: the
    /// defaults, with the configuration file laid over them. Shared with
    /// the words each frame is named with — see [`input::Namer`].
    keys: Rc<keymap::Keymap>,
    gestures: Rc<Gestures>,
    /// Set if the last render failed, so we report it once rather than every frame.
    reported_error: bool,
    /// The window's size in a test, which has no window: what the frame is
    /// laid out in when the interface is driven over the application.
    #[cfg(test)]
    headless: Option<[f32; 2]>,
    /// The name of the monitor the window is on in a test, which has no
    /// window: what the monitor thread's table is read by.
    #[cfg(test)]
    headless_monitor: Option<String>,
    /// Whether the surface a test has no window for is to count as the HDR
    /// one, for what the headroom and the curve make of the monitor and
    /// the switch.
    #[cfg(test)]
    headless_surface_hdr: bool,
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
        state: StateFile,
        threads: Threads,
    ) -> Self {
        let Threads {
            loader,
            wake,
            monitors,
            thumbnailer,
            picker,
            folder: folder_delivered,
            arranged: arranged_delivered,
            measured,
            tags: tags_delivered,
        } = threads;
        let Options {
            overrides,
            startup,
            hdr,
            config,
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
        // One file named, and nothing else: its folder is there to step on
        // through, once it is asked for.
        let folder = match (named.as_slice(), &source) {
            ([file], Some(Source::Disk))
                if config.browse_folder && files.len() == 1 && !file.is_dir() =>
            {
                Folder::Unread { file: file.clone() }
            }
            _ => Folder::Closed,
        };
        let theme_watch = theme::watch();
        let mut view = View::new();
        view.set_upscale(upscale);
        let kept_state = state.state();
        let mut app = Self {
            files: Files::new(files, index),
            overrides,
            current: None,
            startup,
            output: Output {
                asked: hdr,
                monitors,
                mode: None,
                headroom: None,
            },
            sizing: Sizing {
                header: size,
                asked: asked_size,
                to_next: source.is_none(),
                sized_for: None,
                held: None,
            },
            beside: folder::Beside {
                folder,
                landed: None,
                deliver: folder_delivered,
                browse: config.browse_folder,
            },
            arranging: arranging::Arrangings {
                jobs: Vec::new(),
                landed: Vec::new(),
                deliver: arranged_delivered,
                next_job: 0,
                awaiting_window: false,
            },
            rendering: match kept_state.camera_jpeg {
                true => Rendering::CameraJpeg,
                false => Rendering::Developed,
            },
            view,
            motion: None,
            kept: Kept::default(),
            watch,
            openers: Vec::new(),
            #[cfg(target_os = "macos")]
            menubar: None,
            #[cfg(target_os = "macos")]
            menu_deliver: None,
            named,
            directories,
            open_map_link: config.open_map_link.clone(),
            exiftool: exiftool::Program::new(&config.exiftool),
            tags: Tags::default(),
            tags_delivered,
            glimpsed: HashMap::new(),
            last_folder: None,
            offered_folder: None,
            theme: Theme::detect(),
            theme_watch,
            ui_scale: kept_state.ui_scale,
            config_watch: Watch::idle(),
            next_poll: Instant::now() + watch::INTERVAL,
            loader,
            animation: None,
            thumbnailer,
            chooser: Chooser::default(),
            thumbs: Thumbs::default(),
            filmstrip: Filmstrip::default(),
            state,
            visited: Visited::default(),
            read_rates: HashMap::new(),
            pending_thumbs: Vec::new(),
            wake,
            players: 0,
            open_paused: paused,
            shown: None,
            pointer: Pointer::default(),
            marking: Marking::default(),
            toasts: Toasts::default(),
            reduced_said: HashSet::new(),
            copying: Copying::default(),
            measuring: Measuring::new(measured),
            panels: Panels {
                show_ui: config.show_ui,
                show_filmstrip: config.show_filmstrip,
                show_histogram: config.show_histogram,
                show_luma: true,
                show_planes: true,
                log_counts: config.log_counts,
                mark_clipped: false,
                show_info: config.show_info,
                show_minimap: config.show_minimap,
                show_grid: false,
                show_loupe: false,
                loupe_magnification: kept_state.loupe_magnification,
                pixel_format: kept_state.pixel_format,
                coordinate_format: kept_state.coordinate_format,
                geographic_format: kept_state.geographic_format,
                info_tab: ui::tags::Tab::Facts,
            },
            trash: Trash::detect(),
            edits: Vec::new(),
            renaming: None,
            exporting: None,
            picker,
            picking: false,
            clipboard_offers: false,
            show_beside: None,
            from_command_line: source.is_some(),
            said_how_to_restore: false,
            keys: Rc::new(config.keys),
            gestures: Rc::new(config.gestures),
            reported_error: false,
            #[cfg(test)]
            headless: None,
            #[cfg(test)]
            headless_monitor: None,
            #[cfg(test)]
            headless_surface_hdr: false,
        };
        app.filmstrip.set_slot(kept_state.filmstrip_width);
        app.filmstrip.set_order(kept_state.order);
        match source {
            // Several files from disk under an order that needs more than
            // their names: read for it first, so that the first shown is
            // the first in it. The window opens meanwhile, and is sized
            // by that file when it arrives rather than by the one whose
            // header opened it.
            Some(Source::Disk) if app.files.len() > 1 && kept_state.order.reads_facts() => {
                app.arrange(app.files.paths().to_vec(), arranging::Arrive::Open);
            }
            Some(source) => {
                // Before the window: its first frame is asked for when it opens.
                let request = app.files.open_first(source);
                let _ = app.send(request);
            }
            None => {}
        }
        // The whole list, from the start: the cache fills while the first
        // file is being looked at, and the chooser then has thumbnails the
        // moment it opens.
        app.thumbnailer.enqueue(app.files.paths().to_vec());
        // Up from the start, the strip is scrolled to the first file as it
        // is when it is switched on.
        if app.panels.show_filmstrip {
            app.filmstrip.reveal();
        }
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
        let folder = self.last_folder.clone().filter(|folder| folder.is_dir());
        portal::choose_on_thread(pick, folder, Arc::clone(&self.picker));
    }

    /// Takes in what the dialog answered. A frame is owed either way: the
    /// buttons that put the dialog up were drawn dead while it was.
    fn picked(&mut self, picked: Picked) -> Effect {
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
        Effect::Redraw
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
        // In the list's order, so that the first of them shown is the
        // first in it.
        self.arrange(files, arranging::Arrive::Append);
    }

    /// Sizes the window to `image` as it would have opened on it: the same
    /// reckoning as at start-up, the window's own account of the monitors
    /// standing in for the event loop's, and put in the middle of the
    /// monitor as it is at start-up — see [`window::centered_on`].
    ///
    /// Asked for two ways, because Wayland leaves a window's size to the
    /// compositor and a compositor answers only what it is asked in its own
    /// terms. `request_inner_size` is the toolkit's way, which on Wayland
    /// resizes the surface outright — and is answered at once, with no
    /// `Resized` event to follow, so the renderer is told the new size
    /// here. The toolkit refuses it, though, on any window whose last
    /// configure carried a tiled state, and Hyprland puts the tiled edges
    /// on every window it has, floating ones included. So the window's
    /// least and greatest size are pinned to the size wanted as well: a
    /// compositor holds a floating window inside those on the next
    /// configure, which is a `Resized` event, and `release_size` lets go
    /// of them then — or after [`SIZING_GRACE`], for a compositor that
    /// answers with nothing, since a window that could not be resized by
    /// hand afterwards would be worse than one that opened small.
    fn size_window_to(&mut self, image: [f32; 2]) {
        let Some(shown) = &mut self.shown else {
            return;
        };
        let window = &shown.window;
        let wanted = initial_window_size(
            window.available_monitors(),
            self.output.monitors.as_ref(),
            Some(image),
            None,
            window::Chrome::new(self.ui_scale),
        );
        let moved = window::centered_on(window, wanted);
        let before = window.inner_size();
        if let Some(applied) = window.request_inner_size(wanted)
            && applied != before
        {
            shown.renderer.resize(applied.width, applied.height);
        }
        if let Some(at) = moved {
            window.set_outer_position(at);
        }
        window.set_min_inner_size(Some(wanted));
        window.set_max_inner_size(Some(wanted));
        self.sizing.held = Some(Instant::now());
    }

    /// Lets go of the size the window was held to, if it was.
    fn release_size(&mut self) {
        if self.sizing.held.take().is_some()
            && let Some(shown) = &self.shown
        {
            let window = &shown.window;
            window.set_min_inner_size(None::<winit::dpi::LogicalSize<u32>>);
            window.set_max_inner_size(None::<winit::dpi::LogicalSize<u32>>);
        }
    }

    /// Takes the picture off the screen — the last file deleted — keeping
    /// what it was left in for its return, and stops everything that was
    /// about it: the animation playing, the watch on its file, the region
    /// drawn on it, the move it was in the middle of, and the menu of what
    /// else could open it. The window then shows nothing, and the next
    /// picture sizes it as the first did; nothing showing is no longer the
    /// command line's failure, whatever the list was.
    fn leave_picture(&mut self) {
        if self.current.is_none() {
            return;
        }
        // Read once, here, rather than on every frame the window is empty:
        // the folder the files just left is often empty itself.
        self.offered_folder = self.last_folder.clone().filter(|folder| {
            crate::listing::images_in(folder).is_ok_and(|images| !images.is_empty())
        });
        self.keep_shown();
        self.current = None;
        self.animation = None;
        self.motion = None;
        self.watch = Watch::idle();
        self.openers.clear();
        self.marking.clear();
        self.from_command_line = false;
        self.sizing.to_next = true;
        self.sizing.sized_for = None;
        let title = self.title();
        if let Some(shown) = &mut self.shown {
            shown.renderer.clear_image();
            shown.window.set_title(&title);
        }
    }

    /// Puts `title` on the window, where there is one.
    pub(super) fn set_title(&self, title: &str) {
        if let Some(shown) = &self.shown {
            shown.window.set_title(title);
        }
    }

    /// Keeps what the picture on screen was left in — its view, its
    /// display, and the frame or page it was on — under the file's path,
    /// so that stepping back to it puts it back.
    fn keep_shown(&mut self) {
        let (Some(current), Some(path)) = (&self.current, self.files.shown_path()) else {
            return;
        };
        let left = match (&self.animation, current.sequence) {
            (Some(animation), _) => Some(animation.left()),
            (None, Sequence::Pages { .. }) => Some(Left::Page(current.page)),
            (None, _) => None,
        };
        let Some((view, _)) = self.picture_view() else {
            return;
        };
        self.kept.keep(
            path,
            Settings {
                view,
                display: current.picture().1.clone(),
                left,
                turn: current.turn,
                rendering: current.rendering,
            },
        );
    }

    /// Whether the file chooser is up. Asked of egui, whose popup it is:
    /// `Esc` and a click outside close it there, and nothing here would
    /// know.
    pub(super) fn chooser_open(&self) -> bool {
        self.shown
            .as_ref()
            .is_some_and(|shown| egui::Popup::is_id_open(&shown.gui.ctx, ui::chooser::id()))
    }

    /// Whether the info panel's Tags tab is on screen: the panel is up, on
    /// that tab, with room for it — the reading the frame draws it by, so
    /// that what is drawn and what is run cannot disagree.
    pub(super) fn tags_showing(&self) -> bool {
        self.panels.show_info && self.panels.info_tab == ui::tags::Tab::Tags && self.room().info
    }

    /// Puts the information panel up on `tab`, or takes it down where it is
    /// up on that tab already: what `i` and `I` do. Refused, as the panel's
    /// button is, where the window has no room for it.
    pub(super) fn show_info_on(&mut self, tab: ui::tags::Tab) -> Effect {
        if self.refuses(ui::Control::Info) {
            return Effect::Nothing;
        }
        if self.panels.show_info && self.panels.info_tab == tab {
            self.panels.show_info = false;
        } else {
            self.panels.show_info = true;
            self.panels.info_tab = tab;
            if self.tags_showing() {
                self.request_tags();
            }
        }
        Effect::Redraw
    }

    /// Asks exiftool for the tags of the file on screen, unless they are
    /// kept or on their way: what the tab coming on screen does, and a file
    /// arriving while it is.
    pub(super) fn request_tags(&mut self) {
        if self.current.is_none() {
            return;
        }
        let Some(path) = self.files.shown_path().map(Path::to_path_buf) else {
            return;
        };
        let Some(stamp) = watch::Signature::of(&path) else {
            return;
        };
        let Some(request) = self.tags.want(&path, stamp) else {
            return;
        };
        match self.exiftool.locate() {
            Some(program) => {
                exiftool::run_on_thread(
                    program.to_path_buf(),
                    request,
                    Arc::clone(&self.tags_delivered),
                );
            }
            None => {
                self.tags.take(exiftool::Delivered {
                    asked: request.asked,
                    path: request.path,
                    stamp: request.stamp,
                    outcome: Err(exiftool::Failure::NotInstalled),
                });
            }
        }
    }

    /// Takes in what exiftool said, and says whether the tab on screen
    /// shows it. A spawn that found nothing where the program was found has
    /// it looked for again next time.
    fn tags_read(&mut self, delivered: exiftool::Delivered) -> Effect {
        if matches!(delivered.outcome, Err(exiftool::Failure::NotInstalled)) {
            self.exiftool.lost();
        }
        let about = self.tags.take(delivered);
        Effect::redraw_if(about && self.tags_showing())
    }

    /// The list has changed — a directory read again, a paste taken in —
    /// so the chooser reads it again, and the thread is told about any
    /// files new to it.
    pub(super) fn list_changed(&mut self) {
        // The two panels read the list again by themselves, at the next
        // frame — see [`Files::listing`]. Whatever changed it, the list
        // may have fallen out of its order:
        // a rebuild comes back merged, a file put back lands where it was.
        self.filmstrip.mark_stale();
        self.thumbnailer.enqueue(self.files.paths().to_vec());
    }

    /// Whether the file list is on screen: switched on, with a list of
    /// more than one file to show. Hiding the interface leaves it up,
    /// without its head.
    pub(super) fn filmstrip_showing(&self) -> bool {
        self.panels.show_filmstrip && self.files.len() > 1
    }

    /// What is known about `path` that the list is ordered by: what its
    /// header said, or failing that what a folder read glimpsed of it.
    fn key_of<'p>(
        chooser: &Chooser,
        glimpsed: &HashMap<PathBuf, folder::Glimpse>,
        path: &'p Path,
    ) -> order::Key<'p> {
        order::Key::of(path, chooser.facts_of(path), glimpsed.get(path))
    }

    /// Puts the list in the order the file list asks for, keeping the
    /// file on screen the file on screen. Between reads only, as a rebuild
    /// is — a read in flight is aimed at an index — so under one the list
    /// is left as it is and put in order at the next chance.
    fn apply_order(&mut self) -> Effect {
        if !self.files.is_idle() {
            self.filmstrip.mark_stale();
            return Effect::Nothing;
        }
        let places = order::arrange(self.files.len(), self.filmstrip.order(), |index| {
            Self::key_of(&self.chooser, &self.glimpsed, self.files.path(index))
        });
        let moved = self.files.reorder(&places);
        // The list itself has not changed, only its order: the two panels
        // read it again at the next frame, and the thread has nothing new
        // to make.
        if !moved {
            return Effect::Nothing;
        }
        // The file on screen is wherever the order has put it now, and
        // the strip follows it there.
        self.filmstrip.reveal();
        Effect::Redraw
    }

    /// Puts the list back in its order if it may have fallen out of it,
    /// once per poll however many headers arrived since the last.
    fn poll_order(&mut self) -> Effect {
        if self.filmstrip.take_stale() {
            self.apply_order()
        } else {
            Effect::Nothing
        }
    }

    /// A header was read, by the thread or by the loader: the rows may
    /// wear it, and the order may turn on it.
    fn facts_learned(&mut self) {
        self.filmstrip.facts_changed();
        if self.filmstrip.order().reads_facts() {
            self.filmstrip.mark_stale();
        }
    }

    /// Takes in what the thumbnail thread had to say.
    fn take_thumbnail(&mut self, delivered: Delivered) {
        let facts = matches!(delivered.news, News::Facts(_));
        if let Some((path, thumb)) = self.chooser.take(delivered) {
            self.hold_thumb(path, thumb);
        }
        if facts {
            self.facts_learned();
        }
    }

    /// Puts a thumbnail in a texture for the screen to hold, or keeps it
    /// until there is a context to make one in.
    fn hold_thumb(&mut self, path: PathBuf, thumb: Thumb) {
        let Some(shown) = &self.shown else {
            self.pending_thumbs.push((path, thumb));
            return;
        };
        let copies = thumb.copies.each_ref().map(|copy| {
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [copy.width as usize, copy.height as usize],
                &copy.rgba,
            );
            shown.gui.ctx.load_texture(
                format!("{} ({})", path.display(), copy.width.max(copy.height)),
                image,
                egui::TextureOptions::LINEAR,
            )
        });
        // The rows on screen are seen again first, so that they are never
        // the oldest held: a frame says which rows it shows only when that
        // changes, and a list sitting still while the thread thumbnails
        // the rest of the session would otherwise let its own go.
        if self.chooser_open() {
            for shown in self.chooser.on_screen() {
                self.thumbs.touch(shown);
            }
        }
        if self.filmstrip_showing() {
            for shown in self.filmstrip.on_screen() {
                self.thumbs.touch(shown);
            }
        }
        self.thumbs.insert(path, copies);
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
        self.current.is_none() && self.files.is_idle() && self.arranging.jobs.is_empty()
    }

    /// The size to open the window at: the image's, once there is one, and
    /// otherwise whatever the header claimed on the way past.
    fn opening_size(&self) -> Option<[f32; 2]> {
        self.current
            .as_ref()
            .map(Current::size)
            .or(self.sizing.header)
    }

    /// Whether the surface should be the HDR one. The monitor decides where
    /// the compositor says what it is in: one in HDR mode gets the HDR
    /// surface, which costs the compositor nothing and gives the picture the
    /// room, and one in SDR mode gets the SDR surface — unless `--output
    /// hdr` asked for the other regardless, which is the one route left that
    /// asks a compositor to switch a monitor over. Where nothing says what
    /// the monitor is, what was asked for is all there is to go on.
    fn surface_hdr(&self) -> bool {
        surface_wanted(self.output.mode, self.output.asked)
    }

    /// Whether the picture is going out with room above SDR white, which is
    /// what the gain map's lift is weighed against and half of what every
    /// readout of a value says. It takes three things: a surface with the
    /// room, a monitor not known to be in SDR mode — a compositor maps an
    /// HDR surface down for one that is, and the room is not there however
    /// the surface was made — and the switch not having turned it off.
    /// Before there is a window the answer is the SDR one, and
    /// [`App::refresh_lift`] weighs the lift again whenever any of the three
    /// moves.
    fn headroom(&self) -> Headroom {
        let surface = self
            .shown
            .as_ref()
            .is_some_and(|shown| shown.renderer.output().is_hdr);
        #[cfg(test)]
        let surface = surface || self.headless_surface_hdr;
        headroom_of(surface, self.output.mode, self.output.asked)
    }

    /// Whether the switch has anything to switch, and where it has not, which
    /// of the two reasons — see [`hdr_state_of`] for the rule.
    ///
    /// One answer for three readers — whether the button is drawn dead
    /// ([`App::hdr_available`]), whether it takes a press
    /// ([`App::toggle_hdr`]), and what its tooltip says instead of its name
    /// ([`ui::tooltip::disabled`]) — so a dead switch cannot come to give a
    /// reason it is not dead for.
    fn hdr_state(&self) -> Hdr {
        let offered = self
            .shown
            .as_ref()
            .is_some_and(|shown| shown.renderer.hdr_available());
        let speaks = self
            .output
            .monitors
            .as_ref()
            .is_some_and(Monitors::speaks_modes);
        hdr_state_of(offered, speaks, self.output.mode)
    }

    /// The key the monitor thread's table knows the window's monitor by —
    /// see [`monitor::key_of`]. `None` before the window has landed on one.
    fn monitor_name(&self) -> Option<String> {
        #[cfg(test)]
        if let Some(name) = &self.headless_monitor {
            return Some(name.clone());
        }
        self.shown
            .as_ref()
            .and_then(|shown| shown.window.current_monitor())
            .and_then(|monitor| monitor::key_of(&monitor))
    }

    /// [`App::hdr_state`] read as the yes or no the button is drawn from.
    fn hdr_available(&self) -> bool {
        self.hdr_state() == Hdr::Available
    }

    /// Puts the surface where [`App::surface_hdr`] says and the gain map's
    /// lift at the weight the headroom that leaves gives it, and says whether
    /// the picture changed. Called whenever an input to either moves: the
    /// window landing on a monitor, the monitor changing mode, the switch
    /// being pressed. Which surface it is goes to stderr when it changes, the
    /// way the choice at start-up does, since the bar has room for one word
    /// and the surface's name is several.
    ///
    /// `before` is the headroom as it stood before whatever moved, read by
    /// the caller ahead of its change: the monitor's mode and the switch
    /// each move the headroom without moving the surface — under `--output
    /// hdr` the surface is HDR whatever the monitor is in, and the switch
    /// leaves the surface where it is on purpose — and a reading taken here,
    /// after the change, would find nothing to weigh the lift again for.
    fn sync_output(&mut self, before: Headroom) -> Effect {
        let wanted = self.surface_hdr();
        let mut changed = false;
        if let Some(shown) = &mut self.shown
            && shown.renderer.output().is_hdr != wanted
            && shown.renderer.set_hdr(wanted)
        {
            eprintln!("gamut: {} output", shown.renderer.output().label);
            changed = true;
        }
        if self.headroom() != before {
            self.refresh_lift();
            changed = true;
        }
        Effect::redraw_if(changed)
    }

    /// Reads which monitor the window is on and what the compositor says it
    /// is in, and follows a change. Cheap enough to ask after every batch of
    /// events, which is how a window carried to another monitor is noticed:
    /// nothing else says. Says whether anything on screen changed — the
    /// picture, or only the switch, which a monitor's mode lights or kills.
    fn sync_monitor(&mut self) -> Effect {
        let Some(monitors) = &self.output.monitors else {
            return Effect::Nothing;
        };
        let name = self.monitor_name();
        let mode = name.as_deref().and_then(|name| monitors.mode(name));
        let headroom = name.as_deref().and_then(|name| monitors.headroom(name));
        if mode == self.output.mode && headroom == self.output.headroom {
            return Effect::Nothing;
        }
        let before = self.headroom();
        self.output.headroom = headroom;
        // Worth a line, since it is what lights the switch or kills it; the
        // room above white alone can move by the moment on a Mac, and is not.
        if let (Some(name), Some(mode)) = (&name, mode)
            && Some(mode) != self.output.mode
        {
            let mode = match mode {
                Mode::Hdr => "HDR",
                Mode::Sdr => "SDR",
            };
            eprintln!("gamut: monitor {name} is in {mode} mode");
        }
        self.output.mode = mode;
        // The room moving is a lift to weigh again even where the headroom
        // stays above white: a Mac's display reads none until a window asks
        // it for room and then ramps up, and a picture that arrived in that
        // moment would otherwise keep the lift it got at none.
        self.refresh_lift();
        // The switch changed whatever the picture did.
        self.sync_output(before).also(Effect::Redraw)
    }

    /// How much room above white the picture is going out to: the
    /// monitor's peak over its white, where it has said and the surface has
    /// the room, and none otherwise. What a gain map's lift is weighed
    /// against.
    fn display_headroom(&self) -> f32 {
        if self.headroom() != Headroom::Above {
            return 1.0;
        }
        self.output.headroom.unwrap_or(f32::INFINITY)
    }

    /// Puts the lift of a picture with a gain map where the surface's room
    /// asks, and has the picture measured again through it: the histogram
    /// and every readout describe what is on screen, which is the base on a
    /// monitor with no room above white and the lifted picture on one with
    /// room. The picture follows at once; the numbers follow once
    /// [`measuring`] has them, which for a weight met before is at once
    /// too. Nothing happens for a picture with no map, or a lift already at
    /// the weight.
    fn refresh_lift(&mut self) {
        let headroom = self.display_headroom();
        let Some(current) = &mut self.current else {
            return;
        };
        let Some(map) = &current.image.gain_map else {
            return;
        };
        let weight = map.weight(headroom);
        if current
            .lift
            .as_ref()
            .is_some_and(|table| table.weight() == weight)
        {
            return;
        }
        let table = Arc::new(map.table(weight));
        // The loader measured the base, which is what no lift shows: kept
        // for the lift's coming back to nothing, and a picture arriving on
        // a surface with no room is not measured twice.
        if current.lift.is_none() {
            self.measuring.keep_base(&current.image, &current.stats);
        }
        if let Some(stats) = self.measuring.measure(&current.image, &table) {
            current.stats = stats;
        }
        current.lift = Some(table);
    }

    /// Takes the picture's measure through its lift in, where it is still
    /// the measure of what is on screen.
    fn measured(&mut self, measured: Measured) -> Effect {
        let Some(current) = &mut self.current else {
            return Effect::Nothing;
        };
        match self.measuring.arrived(measured, &current.image) {
            Some(stats) => {
                current.stats = stats;
                Effect::Redraw
            }
            None => Effect::Nothing,
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
    pub(super) fn toggle_hdr(&mut self) -> Effect {
        let before = self.headroom();
        self.output.asked = if before == Headroom::Above {
            HdrPreference::Off
        } else {
            HdrPreference::On
        };
        self.sync_output(before)
    }

    /// Switches every raw between its developed picture and the camera's
    /// JPEG of it, and reads the one on screen again in the other. Nothing to
    /// draw yet: the picture stays until the other arrives, as on a step. A
    /// read already in flight is left to finish — it may be a step — and
    /// [`App::follow_rendering`] asks again once it lands.
    pub(super) fn toggle_camera_jpeg(&mut self) -> Effect {
        self.rendering = self.rendering.toggled();
        if let Some(request) = self.files.rerender() {
            // The file on screen, read again: nothing moves until it is in.
            let _ = self.send(request);
        }
        Effect::Nothing
    }

    /// Shows the image of `kind` the picture carries in its place, or the
    /// picture again, for this picture and every one after it that carries
    /// one. Pressed while the other kind's toggle is on, it takes that one's
    /// place.
    pub(super) fn toggle_beside(&mut self, kind: Auxiliary) -> Effect {
        self.show_beside = (self.show_beside != Some(kind)).then_some(kind);
        self.follow_showing().also(Effect::Redraw)
    }

    /// Which of the file's images the toggles ask for: the one whose toggle
    /// is on, where the picture carries it — and is a still, since an
    /// animation's frames replace the picture as they play — and the
    /// picture otherwise. Put on screen where it is not already.
    fn follow_showing(&mut self) -> Effect {
        let wanted = match (self.current.as_ref(), self.show_beside) {
            (Some(current), Some(kind))
                if self.animation.is_none() && current.picture().0.carries(kind) =>
            {
                Showing::Auxiliary(kind)
            }
            _ => Showing::Picture,
        };
        self.show(wanted)
    }

    /// Puts `showing`, one of the file's images, on screen in place of the
    /// one that is. Nothing is read or decoded: an image the picture carries
    /// came with it, and is worked out and uploaded the first time it goes
    /// up; after that each is held, face and texture, as it was left.
    ///
    /// What is on screen is what everything reads — see [`Current`] — so the
    /// switch is a change of picture in all but the file: the view is
    /// rescaled, as it is between a raw's two renderings, so that the same
    /// detail stays under the same place on screen at the size it had, and
    /// the region, marked out in the other image's pixels, is let go.
    fn show(&mut self, showing: Showing) -> Effect {
        let Some(current) = self.current.as_mut() else {
            return Effect::Nothing;
        };
        let was = current.showing;
        let from = current.size();
        let made = current.show(showing, |picture| match showing {
            Showing::Picture => None,
            Showing::Auxiliary(kind) => picture.auxiliary(kind).map(ui::Face::new),
        });
        if !made {
            return Effect::Nothing;
        }
        let mut reduced = None;
        if let Some(shown) = self.shown.as_mut() {
            match shown.renderer.show(showing, &current.image) {
                Ok(lost) => {
                    current.reduced = lost;
                    reduced = lost;
                }
                Err(error) => {
                    crate::report(&error);
                    current.show(was, |_| None);
                    return Effect::Nothing;
                }
            }
        }
        let to = current.size();
        self.view.rescale(from, to);
        self.motion = None;
        self.marking.clear();
        self.say_reduced(reduced);
        // The picture's lift was worked out for the room the surface had
        // when it was last up, which may have changed since.
        self.refresh_lift();
        Effect::Redraw
    }

    /// The view as it would be over the picture itself, whichever of the
    /// file's images is on screen, and the picture's size: what the file is
    /// left in, and what a file arriving is weighed against, both being
    /// about the file and not about what was being looked at in it.
    fn picture_view(&self) -> Option<(View, [f32; 2])> {
        let current = self.current.as_ref()?;
        let (picture, _) = current.picture();
        let size = current
            .turn
            .size([picture.width as f32, picture.height as f32]);
        let mut view = self.view;
        if current.showing != Showing::Picture {
            view.rescale(current.size(), size);
        }
        Some((view, size))
    }

    /// A read asked for `asked` has landed. Where the preference has moved
    /// since and the picture on screen has another rendering to move to, it
    /// is read again in the one asked for now.
    fn follow_rendering(&mut self, asked: Rendering) {
        let offered = self
            .current
            .as_ref()
            .is_some_and(|current| matches!(current.camera_jpeg, CameraJpeg::Present(_)));
        if asked != self.rendering
            && offered
            && let Some(request) = self.files.rerender()
        {
            let _ = self.send(request);
        }
    }

    fn image_size(&self) -> [f32; 2] {
        self.current
            .as_ref()
            .map(Current::size)
            .unwrap_or([1.0, 1.0])
    }

    fn window_size(&self) -> [f32; 2] {
        #[cfg(test)]
        if let Some(size) = self.headless {
            return size;
        }
        self.shown
            .as_ref()
            .map(|shown| shown.renderer.size())
            .unwrap_or([1.0, 1.0])
    }

    /// The monitor's own scale: the window's physical pixels to its logical
    /// one, as the desktop has it. What the window is sized in and the
    /// checkerboard is drawn by; everything the interface lays out reads
    /// [`App::pixels_per_point`] instead.
    fn device_scale(&self) -> f32 {
        self.shown
            .as_ref()
            .map(|shown| shown.window.scale_factor() as f32)
            .unwrap_or(1.0)
    }

    /// Physical pixels to the interface's point: the monitor's scale and the
    /// interface's together, which is what egui lays out by and so what
    /// every conversion between the pointer, the panels and the picture's
    /// viewport has to use.
    fn pixels_per_point(&self) -> f32 {
        self.device_scale() * self.ui_scale
    }

    /// What was left where it was set by hand, gathered for the state file
    /// as the loop ends.
    pub(super) fn kept_state(&self) -> State {
        State {
            filmstrip_width: self.filmstrip.slot(),
            loupe_magnification: self.panels.loupe_magnification,
            ui_scale: self.ui_scale,
            order: self.filmstrip.order(),
            camera_jpeg: self.rendering == Rendering::CameraJpeg,
            pixel_format: self.panels.pixel_format,
            coordinate_format: self.panels.coordinate_format,
            geographic_format: self.panels.geographic_format,
        }
    }

    /// Puts the interface at `scale` of the monitor's: what egui lays out by,
    /// and what the application converts the pointer and the viewport by.
    /// A change says the new scale in a toast; nowhere to go — the
    /// ladder's end, a reset at 1 — says nothing and owes no frame. The
    /// window keeps its size.
    pub(super) fn rescale(&mut self, scale: f32) -> Effect {
        if scale == self.ui_scale {
            return Effect::Nothing;
        }
        self.ui_scale = scale;
        if let Some(shown) = &self.shown {
            shown.gui.rescale(scale);
        }
        self.toast(ui::scale::said(scale), Level::Message);
        Effect::Redraw
    }

    /// The window in the points the interface is laid out in. Events and the
    /// surface are both in physical pixels.
    fn logical_size(&self) -> [f32; 2] {
        let scale = self.pixels_per_point();
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
        content_area(self.logical_size(), self.panels.show_ui, self.parts())
    }

    /// Which of the panels that come and go are up: what the chrome's
    /// geometry is derived from besides the window size.
    fn parts(&self) -> Parts {
        Parts {
            transport: self.has_transport(),
            filmstrip: self.filmstrip_showing().then(|| self.filmstrip.slot()),
        }
    }

    /// Whether the file the bar names brings the transport bar with it: an
    /// animation, or a file of pages. From the key for a file on its way in,
    /// as its header said, so that the bar is the one it will arrive with.
    fn has_transport(&self) -> bool {
        match self.arriving() {
            Some(path) => self.arriving_transport(path).is_some(),
            None => self
                .current
                .as_ref()
                .is_some_and(|current| current.sequence != Sequence::Still),
        }
    }

    /// What the transport bar shows, for a file that has one: the file on
    /// its way in from the key, and the file on screen otherwise.
    fn transport(&self) -> Option<ui::Transport> {
        if let Some(path) = self.arriving() {
            return self.arriving_transport(path);
        }
        let current = self.current.as_ref()?;
        match (current.sequence, &self.animation) {
            (Sequence::Animation { .. }, Some(animation)) => Some(animation.transport()),
            (Sequence::Pages { count, .. }, _) => Some(ui::Transport {
                index: current.page,
                count,
                kind: ui::transport::Kind::Pages,
            }),
            _ => None,
        }
    }

    /// The transport bar the file at `path` will arrive with, from what its
    /// header said and where it was left: at the frame or page it was left
    /// on, playing unless it was left stopped, and otherwise at the start,
    /// playing, or at the page the file itself opens on. No timeline yet —
    /// no frame of it has been decoded — so the track is laid out evenly.
    /// `None` for a still, and for a file whose header has not been read.
    fn arriving_transport(&self, path: &Path) -> Option<ui::Transport> {
        let sequence = self.chooser.facts_of(path)?.sequence;
        let left = self.kept.left(path).and_then(|settings| settings.left);
        match sequence {
            Sequence::Still => None,
            Sequence::Animation { count, .. } => {
                let (index, playing) = match left {
                    Some(Left::Frame { frame, paused }) => (frame, !paused),
                    _ => (0, true),
                };
                Some(ui::Transport {
                    index: index.min(count.saturating_sub(1)),
                    count,
                    kind: ui::transport::Kind::Animation {
                        playing,
                        delays: Vec::new(),
                    },
                })
            }
            Sequence::Pages { count, default } => {
                let index = match left {
                    Some(Left::Page(page)) => page,
                    _ => default,
                };
                Some(ui::Transport {
                    index: index.min(count.saturating_sub(1)),
                    count,
                    kind: ui::transport::Kind::Pages,
                })
            }
        }
    }

    /// The toast about a read that is taking its time, once it has taken
    /// [`files::SLOW_READ`]: up for as long as the read is, whatever it is
    /// then reading, and gone the moment the file arrives. A folder being
    /// read beside a single file is said the same way.
    fn reading(&self) -> Option<Toast> {
        let Some(pending) = self.files.pending() else {
            return self.reading_folder().or_else(|| self.arranging_toast());
        };
        let raised = pending.announced?;
        let name = file_label(self.files.path(pending.index));
        let message = if self.current.is_some() && pending.index == self.files.index() {
            format!("Reloading {name}\u{2026}")
        } else {
            format!("Loading {name}\u{2026}")
        };
        Some(Toast::waiting(message, raised))
    }

    /// The file on its way in to replace the picture on screen, once its
    /// read has taken long enough to be announced: what the stand-in is of,
    /// and what the panels about the picture wait for. Not the file on
    /// screen read again, which stays up and stays described as it is.
    fn replacing(&self) -> Option<&Path> {
        self.files.pending()?.announced?;
        self.arriving()
    }

    /// The file on its way in to replace the picture on screen, from the
    /// moment it is asked for: what the list's readouts name already, while
    /// what is on screen is still the file before it — so what acts on "the
    /// file" waits, rather than acting on one the bar no longer names.
    pub(super) fn arriving(&self) -> Option<&Path> {
        let pending = self.files.pending()?;
        let path = self.files.path(pending.index);
        let shown = self.current.is_some() && self.files.shown_path() == Some(path);
        (!shown).then_some(path)
    }

    /// The thumbnail standing in for the file on its way in, once its read
    /// has taken long enough to be announced: enlarged to where the picture
    /// will land, and turned as it will be, in place of the picture it is
    /// replacing. Only for another file, whose size its header has said and
    /// whose thumbnail is held; the file on screen read again stays up as it
    /// is, being a better picture of itself than any thumbnail.
    fn standin(&self, sight: &Sight) -> Option<ui::Standin> {
        let path = self.replacing()?;
        let (width, height) = self.chooser.facts_of(path)?.size?;
        let thumb = self.thumbs.get(path)?;
        // Where the picture will land, decided as its arrival will decide it.
        let kept = self.kept.left(path);
        let turn = kept.map_or(Turn::NONE, |left| left.turn);
        let size = turn.size([width as f32, height as f32]);
        // Weighed against the picture on screen as `apply` will weigh it,
        // once it has put the picture itself back up.
        let (view, picture) = match self.picture_view() {
            Some((view, picture)) => (view, Some(picture)),
            None => (self.view, None),
        };
        let shown = picture
            .zip(self.files.shown_path())
            .map(|(size, path)| (path, size));
        let (arrival, _) = arrival(Reload::Fresh, path, size, shown);
        let view = arriving_view(arrival, kept, view, picture, size);
        Some(ui::Standin {
            thumb,
            placement: view.placement(size, sight.viewport),
            turn,
        })
    }

    /// The toast a frame draws: the newer of the message about what was just
    /// done and the one about the file still on its way in, so that a copy
    /// taken while a file loads is said, and the wait is said again once
    /// that message has had its time.
    fn showing_toast(&self) -> Option<Toast> {
        let reading = self.reading();
        match (self.toasts.showing(), reading) {
            (Some(message), Some(reading)) if message.raised < reading.raised => Some(reading),
            (Some(message), _) => Some(message.clone()),
            (None, reading) => reading,
        }
    }

    /// Raises the message at the foot of the window, in place of whatever was
    /// up. Handlers say it and return `Effect::Redraw`; nothing here asks the
    /// window for a frame.
    pub(super) fn toast(&mut self, message: impl Into<String>, level: Level) {
        self.toasts
            .show(Instant::now(), message.into(), level, toast::LINGER);
    }

    /// Says that the picture just put on the device lost precision on the
    /// way: on the terminal each time, and in a toast the first time each
    /// reason comes up.
    fn say_reduced(&mut self, reduced: Option<Reduced>) {
        let Some(reduced) = reduced else {
            return;
        };
        let said = format!("Shown at half-float precision: {}", reduced.reason());
        eprintln!("gamut: {said}");
        if self.reduced_said.insert(reduced) {
            self.toast(said, Level::Warning);
        }
    }

    /// Raises a warning before the window opens: what the command line asked
    /// for and could not have, said where the reader will be looking.
    pub fn say(&mut self, message: &str) {
        self.toasts.show(
            Instant::now(),
            message.to_string(),
            Level::Warning,
            toast::LINGER_AT_OPENING,
        );
    }

    /// Says what the copies prepared on their own threads did. Returns
    /// whether anything was said, and so whether a redraw is owed.
    ///
    /// Taken up on the file check's cadence rather than the moment the thread
    /// finishes: a copy of a large picture takes far longer than the wait
    /// itself, and a quarter of a second either way on a message about it is
    /// not a difference anyone can see.
    fn poll_copies(&mut self) -> Effect {
        let outcomes = self.copying.poll();
        let said = !outcomes.is_empty();
        for outcome in outcomes {
            match outcome {
                Ok(Done::Copied(said)) => self.toast(said, Level::Message),
                Ok(Done::Exported(path)) => self.exported(path),
                Err(error) => self.toast(error, Level::Error),
            }
        }
        Effect::redraw_if(said)
    }

    /// Everything that is asked rather than waited for, on one cadence:
    /// the file on screen, the directories named, the desktop's theme, and
    /// the copies in flight. All of them, always: each has a watch that
    /// only advances when it is polled. Says what the window owes for what
    /// they found; nothing between looks. The clipboard is watched by a
    /// thread of its own — see [`App::clipboard_changed`] — since one look
    /// at it is a round trip to the compositor.
    fn poll(&mut self, now: Instant) -> Effect {
        if now < self.next_poll {
            return Effect::Nothing;
        }
        self.next_poll = now + watch::INTERVAL;
        self.poll_file()
            .also(self.poll_directories())
            .also(self.poll_folder())
            .also(self.settle_arranged())
            .also(Effect::redraw_if(self.arranging_counts()))
            .also(self.poll_order())
            .also(self.poll_theme())
            .also(self.poll_config())
            .also(self.poll_copies())
    }

    /// The things on screen that happen because time passed rather than
    /// because anything arrived: the message about what was just done
    /// having been up long enough, whatever egui is waiting on — a
    /// tooltip's delay, a hover fading — and the animation's clock, the
    /// next frame being due. Says what the window owes, and when the next
    /// frame is due if one is.
    fn tick(&mut self, now: Instant) -> (Effect, Option<Instant>) {
        let mut timed = self.toasts.tick(now);
        if self.shown.as_mut().is_some_and(|shown| shown.gui.due(now)) {
            timed = true;
        }
        let (frame_due, next_frame) = self.tick_playback(now);
        (Effect::redraw_if(timed || frame_due), next_frame)
    }

    /// Where the image is drawn, in physical pixels: what the panels leave in
    /// the middle, or the whole window when they are hidden. Derived rather
    /// than stored, so toggling the interface re-fits a fitted image without
    /// anything having to remember to.
    fn viewport(&self) -> Viewport {
        image_viewport(
            self.window_size(),
            self.pixels_per_point(),
            self.panels.show_ui,
            self.parts(),
        )
    }

    /// The window and the view read now — see [`Sight`].
    fn sight(&self) -> Sight {
        self.sight_at(Instant::now())
    }

    /// The window and the view read at `now`.
    fn sight_at(&self, now: Instant) -> Sight {
        let scale = self.pixels_per_point();
        let physical = self.window_size();
        let logical = [physical[0] / scale, physical[1] / scale];
        let parts = self.parts();
        let viewport = image_viewport(physical, scale, self.panels.show_ui, parts);
        Sight {
            scale,
            logical,
            viewport,
            parts,
            view: self.view_in(now, viewport),
        }
    }

    /// What this frame's interface is laid out from, in a window `logical`
    /// points across at `scale` device pixels to each: everything the
    /// application derives per frame from its window, pointer and loader.
    /// Apart from `redraw` so that the interface can be driven over the
    /// application with no window behind it. The picture's viewport is
    /// worked out at `scale` too, so that a test at another scale reads one
    /// sight rather than two.
    #[cfg(test)]
    fn frame_input(&mut self, logical: [f32; 2], scale: f32) -> FrameInput {
        let sight = self.sight();
        let sight = Sight {
            logical,
            scale,
            viewport: image_viewport(self.window_size(), scale, self.panels.show_ui, sight.parts),
            ..sight
        };
        let conditions = self.conditions();
        self.frame_input_under(&sight, &conditions)
    }

    /// `App::frame_input` from `sight`, under `conditions`, both read
    /// once for the frame.
    fn frame_input_under(&mut self, sight: &Sight, conditions: &input::Conditions) -> FrameInput {
        let Sight {
            logical,
            scale,
            viewport,
            ..
        } = *sight;
        let chooser = self.chooser_open().then(|| {
            let target = self.current.as_ref().and_then(|_| self.files.target_path());
            self.chooser.follow(&self.files);
            self.chooser.input(&self.thumbs, target)
        });
        let tags = if self.tags_showing() {
            let file = self
                .current
                .as_ref()
                .map(|current| current.file.path.clone());
            let configured = self.exiftool.configured().to_string();
            Some(self.tags.input(&file.unwrap_or_default(), &configured))
        } else {
            None
        };
        let filmstrip = if self.filmstrip_showing() {
            self.filmstrip.follow(&self.files);
            let (files, chooser) = (&self.files, &self.chooser);
            let glimpsed = &self.glimpsed;
            Some(self.filmstrip.input(
                &self.thumbs,
                |path| Self::key_of(chooser, glimpsed, path),
                files.target_path(),
                conditions.visited_before,
                conditions.visited_after,
            ))
        } else {
            None
        };
        let rename = self.rename_input();
        let export = self.export_input();
        // What reads the picture on screen reads nothing while a thumbnail
        // of another file stands over it.
        let standin = self.standin(sight);
        let picture = standin.is_none();
        FrameInput {
            logical,
            scale,
            viewport,
            pointer: self.pointer_pixel_in(sight).filter(|_| picture),
            cursor: self.logical_cursor_at(scale),
            minimap_on_screen: self.minimap_on_screen_in(sight) && picture,
            loupe: self.loupe_in(sight).filter(|_| picture),
            loupe_held: self.loupe_held(),
            index: self.files.target(),
            count: self.files.len(),
            arriving: self.arriving().map(file_label),
            deleted: self.watch.missing(),
            headroom: self.headroom(),
            hdr_available: self.hdr_available(),
            can_pan: self.view.can_pan(self.image_size(), viewport),
            openers: self
                .openers
                .iter()
                .map(|opener| opener.name.clone())
                .collect(),
            toast: self.showing_toast(),
            selection: self.marking.selection,
            handle: self.marking.handle,
            grabbing: self.marking.grabbed(),
            over_region: self.marking.over(),
            box_zoom: self.pointer.fit_key.is_some(),
            modifiers: self.pointer.modifiers,
            gestures: Rc::clone(&self.gestures),
            zoom_box: self.marking.zoom_box(),
            transport: self.transport(),
            filmstrip,
            chooser,
            tags,
            rename,
            export,
            empty: self.is_empty(),
            paste: self.clipboard_offers && self.panels.show_ui,
            folder: self
                .offered_folder
                .as_deref()
                .filter(|_| self.is_empty())
                .map(folder::name),
            picking: self.picking,
            standin,
            waiting: self.replacing().is_some(),
        }
    }

    /// The view as it is on screen at `now`: `view` itself once it has
    /// arrived, and somewhere along the way to it while a move is in flight.
    /// Everything that reads the picture — the frame, the pixel under the
    /// pointer, the grid's spacing, the minimap's marker — reads this, so
    /// that they agree with one another about what is on screen mid-move.
    fn view_at(&self, now: Instant) -> View {
        self.view_in(now, self.viewport())
    }

    /// [`App::view_at`] with the viewport already worked out.
    fn view_in(&self, now: Instant, viewport: Viewport) -> View {
        match &self.motion {
            Some(motion) => {
                let to = self.view.position(self.image_size(), viewport);
                self.view.at(motion.position(to, now))
            }
            None => self.view,
        }
    }

    #[cfg(test)]
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
    fn logical_cursor_at(&self, scale: f32) -> Option<[f32; 2]> {
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
        self.pointer_pixel_in(&self.sight())
    }

    /// [`App::pointer_pixel`] under `sight`: the frame's own, so that
    /// the pixel named is under the view drawn.
    fn pointer_pixel_in(&self, sight: &Sight) -> Option<[u32; 2]> {
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
        let viewport = sight.viewport;
        if !viewport.contains(cursor) {
            return None;
        }
        let image = self.current.as_ref()?.size();
        let point = sight.view.placement(image, viewport).image_point(cursor);
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

    /// The point every zoom — a key, a cell of the zoom menu, a double-click,
    /// a notch of the wheel — works about, in window pixels: the pointer
    /// while it is over the picture, so that what is under it stays under
    /// it, and the middle of the viewport otherwise. Over a panel, a menu or
    /// the margin beside the picture, the pointer is not pointing at anything
    /// a zoom could keep.
    pub(super) fn zoom_anchor(&self) -> [f32; 2] {
        let sight = self.sight();
        match (self.pointer_pixel_in(&sight), self.pointer.cursor) {
            (Some(_), Some(cursor)) => cursor,
            _ => sight.viewport.center(),
        }
    }

    /// Whether the minimap is on screen, which takes the toggle and a view
    /// that has something to point out. A view holding the whole image is
    /// already its own map, so the widget would be a second copy of what the
    /// window is showing, over the corner of it; it goes away instead, and
    /// comes back on the zoom that first cuts something off. The toggle keeps
    /// its state through that, so the button stays lit and the minimap
    /// returns without being asked for again.
    fn minimap_on_screen_in(&self, sight: &Sight) -> bool {
        // Panning and the minimap answer the same question: whether any of the
        // image is off screen. Pan is clamped to the image, so a view with
        // nowhere to go is one showing all of it.
        self.panels.show_minimap && sight.view.can_pan(self.image_size(), sight.viewport)
    }

    /// Whether a button is held on the picture whose hold slot is the loupe,
    /// with what is held on the keyboard now.
    fn loupe_held(&self) -> bool {
        self.pointer.held.is_some_and(|button| {
            self.gestures
                .hold(Surface::Image, self.pointer.modifiers, button)
                .is_some()
        })
    }

    /// The loupe, while it is up: its toggle is on, or a button holding it
    /// is held on the picture, and the pointer is on a pixel of the picture
    /// — the same reading the bar's readout is made from, so the loupe is
    /// up exactly when there is a pixel under the pointer to magnify. Where
    /// its circles go is the interface's to say, against the content area
    /// and the pointer in the logical pixels it lays out in. Through a drag
    /// on the picture it follows the hand, the pass handing the pointer
    /// over as `Command::Dragging` while winit is not.
    fn loupe(&self) -> Option<ui::loupe::Loupe> {
        self.loupe_in(&self.sight())
    }

    /// [`App::loupe`] under `sight`.
    fn loupe_in(&self, sight: &Sight) -> Option<ui::loupe::Loupe> {
        if !(self.panels.show_loupe || self.loupe_held()) {
            return None;
        }
        self.pointer_pixel_in(sight)?;
        let cursor = self.logical_cursor_at(sight.scale)?;
        let content = ui::chrome::content_area(sight.logical, self.panels.show_ui, sight.parts);
        Some(ui::loupe::place(
            cursor,
            content,
            self.panels.loupe_magnification,
        ))
    }

    /// Where the minimap's thumbnail goes, in physical pixels: the whole
    /// image, drawn small in the corner the interface will then mark up.
    ///
    /// It is the image layer that draws it, from the same texture as the view
    /// itself, so this is a placement like any other and everything that
    /// applies to the image — the window, the colormap, the tone map — comes
    /// with it for nothing.
    fn minimap_placement(&self, sight: &Sight) -> Option<Placement> {
        if !self.minimap_on_screen_in(sight) {
            return None;
        }
        let image = self.current.as_ref()?.size();
        ui::minimap::placement(
            sight.logical,
            sight.scale,
            self.panels.show_ui,
            sight.parts,
            image,
            self.view.upscale(),
        )
    }

    /// Re-reads the file on screen if something else has written to it, which
    /// is what makes this usable next to whatever produced the image.
    ///
    /// Says whether the window owes a redraw, which it does when the file
    /// has gone or come back: the picture is untouched either way, and the bar
    /// is the only thing that changes.
    fn poll_file(&mut self) -> Effect {
        // Not while a read is already in flight. A file being written
        // continuously would otherwise stack up a decode every interval, and
        // the reply already on its way carries a watch taken later than this
        // one anyway.
        if !self.files.is_idle() {
            return Effect::Nothing;
        }
        let was_missing = self.watch.missing();
        if self.watch.poll()
            && let Some(request) = self.files.reload()
        {
            // The file on screen, read again: nothing moves until it is in.
            let _ = self.send(request);
        }
        Effect::redraw_if(self.watch.missing() != was_missing)
    }

    /// Notices images arriving in or leaving a directory that was named on the
    /// command line, and builds the list from it again. Says whether the
    /// window owes a redraw, which it does only when the list really changed —
    /// the bar counts the files and says which of them is on screen.
    fn poll_directories(&mut self) -> Effect {
        // Between reads only: rebuilding moves the file on screen to a new
        // index, and a reply on its way is aimed at the old one. Nothing is
        // lost by waiting, since a watch not polled is a watch that has not
        // seen the change yet and will see it at a later look.
        if self.directories.is_empty() || !self.files.is_idle() {
            return Effect::Nothing;
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
            // Put in order at once, nothing being read: the rebuild came
            // back merged rather than sorted.
            let _ = self.apply_order();
        }
        Effect::redraw_if(relisted)
    }

    /// Takes in what the thread watching the clipboard said: a picture
    /// this program could show has arrived on it, or the one there has
    /// gone. What puts the paste button on screen and takes it off again,
    /// and so owes a frame where the button is showing — the interface
    /// being up — and the answer moved.
    pub(super) fn clipboard_changed(&mut self, offered: bool) -> Effect {
        let was = std::mem::replace(&mut self.clipboard_offers, offered);
        Effect::redraw_if(was != offered && self.panels.show_ui)
    }

    /// Starts watching the configuration file, wherever it is, and whether
    /// or not there is one yet: a file written later is read as it lands.
    pub fn watch_config(&mut self) {
        if let Some(path) = crate::settings::config_path() {
            self.config_watch = Watch::new(&path);
        }
    }

    /// Reads the configuration file again once a change to it has settled.
    fn poll_config(&mut self) -> Effect {
        if !self.config_watch.poll() {
            return Effect::Nothing;
        }
        let (config, complaint) = Config::load();
        self.reconfigure(config, complaint)
    }

    /// Puts in force what can change in `config` while the window is up:
    /// the keys and the gestures, and with them everything that names them —
    /// the tooltips, the help popup, and a Mac's menu bar — and the web
    /// address the map button opens. The panels it
    /// sets are how the window opens, and toggling one since is not undone;
    /// whether a file named alone browses its folder was settled when the
    /// command line was read. `complaint` is what the file got wrong, said
    /// in place of the word that it was read.
    pub(super) fn reconfigure(&mut self, config: Config, complaint: Option<String>) -> Effect {
        self.keys = Rc::new(config.keys);
        self.gestures = Rc::new(config.gestures);
        self.open_map_link = config.open_map_link;
        // Looked for again where it was not found too, since what the
        // configuration names may have been installed since; and where the
        // tab is waiting on it, it reads the file at once.
        if config.exiftool != self.exiftool.configured() || !self.exiftool.found() {
            self.exiftool = exiftool::Program::new(&config.exiftool);
            if self.tags_showing() {
                self.request_tags();
            }
        }
        #[cfg(target_os = "macos")]
        self.rekey_menubar();
        match complaint {
            Some(complaint) => self.toast(complaint, Level::Warning),
            None => self.toast(RECONFIGURED, Level::Message),
        }
        Effect::Redraw
    }

    /// Notices that the desktop's theme has changed. Says whether the
    /// window owes a redraw, which it does only when the new palette actually
    /// resolves to different colors.
    fn poll_theme(&mut self) -> Effect {
        if !self.theme_watch.poll() {
            return Effect::Nothing;
        }
        let theme = Theme::detect();
        let changed = theme != self.theme;
        self.theme = theme;
        if changed && let Some(shown) = &self.shown {
            shown.gui.retint(&self.theme);
        }
        Effect::redraw_if(changed)
    }

    /// Opens the window, unless the list it opens on is still being put in
    /// order: then it waits for it, for as long as a read waits before it
    /// is said — see `about_to_wait`.
    fn open_window_when_ready(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(due) = self.window_due()
            && Instant::now() < due
        {
            self.arranging.awaiting_window = true;
            event_loop.set_control_flow(ControlFlow::WaitUntil(due));
            return;
        }
        self.open_window(event_loop);
    }

    /// Opens the window, and the renderer and interface with it. Opened
    /// while the command line's list is still being put in order — it has
    /// taken longer than a read waits before it is said — the window opens
    /// at the empty window's size rather than at that of a file that may
    /// not be the first, and the first picture sizes it when it arrives.
    fn open_window(&mut self, event_loop: &ActiveEventLoop) {
        if self.arranging_to_open() {
            self.sizing.header = None;
            self.sizing.to_next = true;
            self.sizing.sized_for = None;
        }
        let size = initial_window_size(
            event_loop.available_monitors(),
            self.output.monitors.as_ref(),
            self.opening_size(),
            self.sizing.asked,
            window::Chrome::new(self.ui_scale),
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
        // In the middle of the monitor, before a frame of it is drawn.
        if let Some(at) = window::centered_on(&window, size) {
            window.set_outer_position(at);
        }
        timing::window_open();
        window::show_icon();

        let main_device = self
            .output
            .monitors
            .as_ref()
            .and_then(Monitors::main_device);
        let mut renderer = match Renderer::new(window.clone(), self.output.asked, main_device) {
            Ok(renderer) => renderer,
            Err(error) => {
                crate::report(&error);
                event_loop.exit();
                return;
            }
        };

        // The file named on the command line, decoded while the window was
        // being made, and waiting here for somewhere to go. There is nothing
        // on screen yet for the wait to interrupt; everything opened
        // afterwards is uploaded on the loader's thread.
        let mut reduced = None;
        if let (Some(current), Some(path)) = (&mut self.current, self.files.shown_path()) {
            match upload_here(&renderer, path, &current.image) {
                Ok(uploaded) => {
                    reduced = renderer.install_image(uploaded);
                    current.reduced = reduced;
                }
                Err(error) => {
                    crate::report(&error);
                    event_loop.exit();
                    return;
                }
            }
        }
        self.say_reduced(reduced);

        // Asked for and not had is worth a line; asked for and had is worth
        // one too, since the switch's later lines say the same thing. A
        // surface that follows the monitor says so when it moves.
        if self.output.asked == HdrPreference::On {
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

        let gui = match Gui::new(
            &window,
            &self.theme,
            renderer.max_texture_side(),
            self.ui_scale,
        ) {
            Ok(gui) => gui,
            Err(error) => {
                crate::report(&error);
                event_loop.exit();
                return;
            }
        };

        // From here on the loader uploads as well as decodes, so that
        // stepping to the next file costs the event loop nothing but the swap.
        self.loader.attach(renderer.uploader());
        self.shown = Some(Shown {
            renderer,
            gui,
            window,
        });
        // The surface exists at last, so a gain map decoded before the
        // window opened can be weighed against the room it is drawn into.
        // Which monitor it is on is not known until it has been shown, and
        // the surface follows it from `about_to_wait`.
        self.refresh_lift();
        // A first frame, asked for outright. With a file on the way its
        // arrival asks for one; a window opened on nothing has nothing
        // coming, and its buttons are owed a frame all the same.
        self.settle(Effect::Redraw, event_loop);
    }

    /// What the window is called: the file last asked for — on screen, or
    /// on its way in to replace the picture that is — the file being read
    /// while there is nothing on screen, or the program's own name while
    /// there is nothing at all.
    fn title(&self) -> String {
        match (&self.current, self.files.pending()) {
            (Some(_), _) => match self.files.target_path() {
                Some(path) => window_title(path),
                None => crate::PROGRAM.to_string(),
            },
            (None, Some(pending)) => loading_title(self.files.path(pending.index)),
            // Not the file named first while the list is being put in
            // order: which file opens is not known yet.
            (None, None) if self.arranging_to_open() => crate::PROGRAM.to_string(),
            (None, None) => match self.files.shown_path() {
                Some(path) => loading_title(path),
                None => crate::PROGRAM.to_string(),
            },
        }
    }

    /// Sends a request to the loader.
    ///
    /// The picture already up stays where it is, still pannable and
    /// zoomable, until the reply arrives at [`App::user_event`], and so does
    /// everything that describes it — the panels, the readouts. What moves
    /// at once is what says where in the list the key has gone: the count,
    /// the file list's highlight, the name in the bar and the title. Hence
    /// the frame owed, for a request that goes somewhere else.
    fn send(&mut self, asked: Asked) -> Effect {
        let mut request = asked.request(self.overrides, self.rendering);
        let elsewhere = request.mode == Reload::Fresh
            && (self.current.is_none() || self.files.shown_path() != Some(request.path.as_path()));
        // A paged file comes back to the page it was left on, which has to
        // be asked for with the file: the page is what is decoded.
        if request.page.is_none()
            && request.mode == Reload::Fresh
            && let Some(Left::Page(page)) = self.kept.left(&request.path).and_then(|left| left.left)
        {
            request.page = Some(page);
            self.files.asked_for_page(page);
        }
        // Slow from the start where the last file of its kind was, so that a
        // walk through a folder of large files puts each one's thumbnail up
        // on the key rather than a beat after it.
        if elsewhere && self.predicted_slow(&request.path) {
            self.files.announce_now(Instant::now());
        }
        self.loader.request(request);
        if !elsewhere {
            return Effect::Nothing;
        }
        self.filmstrip.reveal();
        self.refresh_title();
        Effect::Redraw
    }

    /// Puts [`App::title`] on the window, which names the file last asked
    /// for: called whenever that changes other than by a file arriving.
    fn refresh_title(&self) {
        if let Some(shown) = &self.shown {
            shown.window.set_title(&self.title());
        }
    }

    /// Whether the read of `path` will be slow, going by the last file of its
    /// kind: its time over its pixels, times this one's pixels. Only what
    /// the thumbnail thread has read of the file's header to go on, and
    /// `false` where it has not read it yet.
    fn predicted_slow(&self, path: &Path) -> bool {
        let Some(facts) = self.chooser.facts_of(path) else {
            return false;
        };
        let (Some(format), Some((width, height))) = (facts.format, facts.size) else {
            return false;
        };
        self.read_rates.get(format).is_some_and(|per_pixel| {
            per_pixel * f64::from(width) * f64::from(height) >= files::SLOW_READ.as_secs_f64()
        })
    }

    /// Moves to the next or previous file. With no other file on the list,
    /// reads the folder beside a single file to step into, where there is
    /// one, and says there is nowhere to go where there is not. The picture
    /// stays until the file arrives; the list's readouts move at once.
    pub(super) fn step(&mut self, forward: bool) -> Effect {
        if self.files.len() < 2 {
            if self.files.len() == 0 || self.folder_first(Then::Step(forward)) {
                return Effect::Nothing;
            }
            self.toast("No other files to step to", Level::Message);
            return Effect::Redraw;
        }
        match self.files.step(forward) {
            Some(request) => self.send(request),
            None => Effect::Nothing,
        }
    }

    /// Steps to the first file on the list, or with `last` the last.
    pub(super) fn step_to_end(&mut self, last: bool) -> Effect {
        if self.files.len() < 2 {
            if self.files.len() == 0 || self.folder_first(Then::End(last)) {
                return Effect::Nothing;
            }
            self.toast("No other files to step to", Level::Message);
            return Effect::Redraw;
        }
        match self.files.end(last) {
            Some(request) => self.send(request),
            None => Effect::Nothing,
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
            rendering,
            camera_jpeg,
            format,
        } = ready;
        // What follows weighs the file arriving against the picture on
        // screen, and keeps what the picture was left in: the picture's,
        // not an image it carries shown in its place.
        let _ = self.show(Showing::Picture);
        self.last_folder = std::path::absolute(&file.path)
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .or(self.last_folder.take());
        // The turn the picture arrives under: the file on screen read again
        // keeps its own, whatever it has become — a page of another size is
        // still a page of the same file — and another file takes back the
        // one it was left in. Decided first, since how the file stands to
        // the picture on screen is a matter of the size it will be shown at.
        let turn = if file.mode == Reload::Fresh {
            self.kept
                .left(&file.path)
                .map_or(Turn::NONE, |left| left.turn)
        } else {
            self.current
                .as_ref()
                .map_or(Turn::NONE, |current| current.turn)
        };
        let size = turn.size([image.width as f32, image.height as f32]);
        let shown = self
            .current
            .as_ref()
            .zip(self.files.shown_path())
            .map(|(current, path)| (path, current.size()));
        let (arrival, stepping) = arrival(file.mode, &file.path, size, shown);
        let shown_size = shown.map(|(_, size)| size);
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
        // A file left in its other rendering is not put back in what it was
        // left in: an exposure set on the camera's JPEG means nothing to
        // linear sensor counts, and the other way about.
        let kept = match arrival {
            Arrival::Beside | Arrival::Anew => self
                .kept
                .left(&file.path)
                .filter(|left| left.rendering == rendering)
                .cloned(),
            Arrival::Reread | Arrival::Reshaped | Arrival::Rerendered => None,
        };
        let refreshed = |display: &Display| {
            let mut display = display.clone();
            display.refresh_auto(&stats);
            display
        };
        let display = match (arrival, &self.current, &kept) {
            // Re-reading the same file keeps the user where they were, since
            // they are watching one spot for the change: same exposure and
            // tone map, with only an automatic window re-derived from the
            // new pixels.
            (Arrival::Reread, Some(current), _) => refreshed(&current.display),
            (_, _, Some(settings)) => refreshed(&settings.display),
            _ => Display::for_image_with(&image, &stats, self.startup),
        };

        let mut reduced = None;
        if let Some(renderer) = self.shown.as_mut().map(|shown| &mut shown.renderer) {
            // Already across whenever the window was open when the read
            // started, which is every file but the one named on the command
            // line. The fallback covers only that gap.
            let uploaded = match gpu {
                Some(uploaded) => uploaded,
                None => match upload_here(renderer, &file.path, &image) {
                    Ok(uploaded) => uploaded,
                    Err(error) => {
                        crate::report(&error);
                        return false;
                    }
                },
            };
            reduced = renderer.install_image(uploaded);
        }
        self.say_reduced(reduced);

        // A window that showed nothing takes the size it would have opened
        // at on this picture, as if it had — unless it was given it already,
        // from the header; the fit follows on the frame the new size brings.
        let sized_for = self.sizing.sized_for.take();
        if std::mem::take(&mut self.sizing.to_next)
            && self.sizing.asked.is_none()
            && sized_for != Some(size)
        {
            self.size_window_to(size);
        }
        self.files.shown(file.index);
        // Another file on screen: it goes on the stack of files seen,
        // and the file list scrolls to it.
        if file.mode == Reload::Fresh {
            self.visited.arrived(&file.path);
        }
        if stepping {
            self.filmstrip.reveal();
        }
        self.watch = file.watch;
        // What this file is, for the chooser's row about it, ahead of the
        // thumbnail thread reaching it. A thumbnail is asked for again for a
        // file changed on disk — the one in the cache is of the file as it
        // was, and its modification time no longer matches — and for one the
        // thread had given up on, which has just decoded here.
        let facts = file_facts(&file.path, Some(format));
        let image = Arc::new(image);
        // The file's own size, which is the developed picture's: the
        // camera's JPEG may be a fraction of it, and the file list orders by
        // and shows what the file is.
        let learned_size = match rendering {
            Rendering::Developed => Some((image.width, image.height)),
            Rendering::CameraJpeg => self
                .chooser
                .facts_of(&file.path)
                .and_then(|facts| facts.size),
        };
        let given_up = self.chooser.learn(
            &file.path,
            Facts {
                size: learned_size,
                sequence,
                title: exif.title().map(str::to_string),
                format: facts.reader,
                bytes: facts.bytes,
                modified: facts.modified,
            },
        );
        // Made from the picture itself, decoded already, rather than read
        // and decoded again on the thread — or refused there: its ceiling
        // is what a background decode may hold, and this one is held.
        if file.mode == Reload::InPlace || given_up {
            self.thumbnailer
                .adopt(file.path.clone(), Arc::clone(&image));
        }
        self.facts_learned();
        // A region is of the picture it was drawn on. Stepping to another
        // file takes it off, and so does the file coming back a different
        // size, where the pixels it marked out are no longer the pixels.
        // Another rendering of the picture is other pixels, whatever its size.
        if stepping
            || matches!(
                arrival,
                Arrival::Reshaped | Arrival::Anew | Arrival::Rerendered
            )
        {
            self.marking.clear();
        }
        self.view = arriving_view(arrival, kept.as_ref(), self.view, shown_size, size);
        if arrival != Arrival::Reread {
            // A move under way was about the picture that has just left, and
            // there is nothing for it to carry the eye across any more.
            self.motion = None;
        }
        // And what else could open the file arriving, read here with the rest
        // of what the file itself says about it.
        if matches!(arrival, Arrival::Beside | Arrival::Anew) {
            self.openers = openers::for_file(&file.path);
        }
        self.current = Some(Current {
            shown: ui::Face {
                image,
                stats,
                display,
                reduced,
                lift: None,
            },
            label: file_label(&file.path),
            file: facts,
            exif,
            sequence,
            page,
            turn,
            rendering,
            camera_jpeg,
            showing: Showing::Picture,
            held: Vec::new(),
        });
        // The Tags tab follows the file on screen, whatever brought it:
        // another file, or this one read again after a write.
        if self.tags_showing() {
            self.request_tags();
        }
        // The camera's JPEG was asked for and the file has none: said, since
        // the button that would say it is not drawn for such a file. Not for
        // a file rewritten on disk, which said it when it arrived.
        if self.rendering == Rendering::CameraJpeg
            && camera_jpeg == CameraJpeg::Missing
            && matches!(file.mode, Reload::Fresh | Reload::Rendering)
        {
            self.toast(NO_CAMERA_JPEG_SHOWN, Level::Message);
        }
        // The loader measured the base; a surface with room above white
        // shows the lift, and the numbers follow it.
        self.refresh_lift();
        self.start_player(
            &file.path,
            sequence,
            kept.as_ref().and_then(|kept| kept.left),
        );
        // And the image the toggles ask for in its place, where it carries
        // one: stepping on with the depth map up keeps showing depth. After
        // the player, whose frames would replace an image shown in the
        // picture's place, and which says whether there are any.
        let _ = self.follow_showing();
        self.set_title(&window_title(&file.path));
        true
    }

    /// Turns the picture on screen a quarter, clockwise or not. Nothing is
    /// read, decoded or uploaded: the turn is how the texture is read, and
    /// every other reading of the picture goes through `Current`, which is
    /// in the turned picture's coordinates. The region turns with the
    /// pixels it marks out, and the view with the detail at its center; the
    /// statistics stay, since a histogram does not care which way up. A
    /// fitted view fits the new shape on the next frame by itself.
    pub(super) fn turn_picture(&mut self, clockwise: bool) -> Effect {
        let Some(current) = &mut self.current else {
            return Effect::Nothing;
        };
        let step = match clockwise {
            true => Turn::NONE.clockwise(),
            false => Turn::NONE.counterclockwise(),
        };
        let was = current.pixels();
        current.turn = match clockwise {
            true => current.turn.clockwise(),
            false => current.turn.counterclockwise(),
        };
        self.marking.turned(step, was);
        self.view.turn(clockwise);
        // A move under way was across the picture as it lay.
        self.motion = None;
        Effect::Redraw
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
        self.animation = None;
        let Sequence::Animation { count, loops } = sequence else {
            return;
        };
        self.players += 1;
        self.animation = Some(Animation::start(
            path,
            self.overrides,
            count,
            loops,
            left,
            !self.open_paused,
            self.players,
            Arc::clone(&self.wake),
            Instant::now(),
        ));
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
        let Some(animation) = &mut self.animation else {
            return;
        };
        let Some((head, frame)) = animation.due_frame() else {
            return;
        };
        let mut reduced = None;
        if let Some(renderer) = self.shown.as_mut().map(|shown| &mut shown.renderer) {
            match renderer.refill_image(&frame.image) {
                Ok(lost) => reduced = lost,
                Err(error) => {
                    crate::report(&error);
                    return;
                }
            }
        }
        if let Some(current) = &mut self.current {
            current.image = Arc::clone(&frame.image);
            current.stats = frame.stats.clone();
            current.reduced = reduced.or(current.reduced);
        }
        animation.shown(head);
        self.say_reduced(reduced);
    }

    /// Moves the animation's clock on to `now`. Returns whether the frame
    /// on screen is owed a change, and when the next one is due.
    fn tick_playback(&mut self, now: Instant) -> (bool, Option<Instant>) {
        let Some(animation) = &mut self.animation else {
            return (false, None);
        };
        let ticked = animation.tick(now);
        if let Some(error) = ticked.error {
            eprintln!("gamut: {}", crate::escape_controls(&error));
            self.toast("The animation could not be read to its end", Level::Error);
        }
        (ticked.changed, ticked.deadline)
    }

    /// One frame on or back through the animation, or one page on or back
    /// through a paged file: the same key for both, since a reader stepping
    /// through what a file holds does not care which kind it is.
    pub(super) fn step_frame(&mut self, by: isize) -> Effect {
        if let Some(animation) = &mut self.animation {
            animation.step(by);
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
            // Another page of the file on screen: nothing moves until it is.
            let _ = self.send(request);
        }
        Effect::Nothing
    }

    /// Plays a stopped animation, or stops a playing one.
    pub(super) fn toggle_play(&mut self) -> Effect {
        let Some(animation) = &mut self.animation else {
            return Effect::Nothing;
        };
        animation.toggle(Instant::now());
        Effect::Redraw
    }

    /// Straight to frame `frame` of the animation, stopped there.
    pub(super) fn seek(&mut self, frame: usize) -> Effect {
        let Some(animation) = &mut self.animation else {
            return Effect::Nothing;
        };
        animation.seek(frame);
        Effect::Redraw
    }

    /// Takes in a file the loader has finished with. Held apart from the
    /// handler that receives it, since nothing here needs the event loop.
    /// A frame is owed either way: on success for the new image, and on
    /// failure because the bar may have been saying that a read was under
    /// way.
    fn deliver(&mut self, decoded: Decoded) -> Effect {
        // Anything but the newest request is a file the user has stepped past
        // while it was being read. Its pixels are correct and unwanted.
        let Some(pending) = self.files.accept(decoded.generation) else {
            return Effect::Nothing;
        };

        let index = decoded.file.index;
        let asked = decoded.file.rendering;
        let name = file_label(&decoded.file.path);
        // A file that will not go on screen is a file to step over, whether it
        // was the decode or the upload that would not have it.
        let failed = match decoded.outcome {
            Ok(ready) => match self.apply(decoded.file, ready) {
                true => {
                    self.arrived_in(pending.since.elapsed());
                    None
                }
                false => Some(format!("Could not show {name}.")),
            },
            Err(error) => {
                input::report(&error);
                Some(format!("Could not read {name}: {}", input::briefly(&error)))
            }
        };
        if let Some(said) = failed.clone() {
            match self.files.failed(index, pending.step) {
                // A frame is owed below, whatever this asks for.
                Some(request) => {
                    let _ = self.send(request);
                }
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
        // The name goes back to the file on screen where the read failed
        // and nothing else was asked for.
        self.refresh_title();
        // The rendering was switched while this was being read, and the
        // switch could not interrupt the read: asked for now.
        if failed.is_none() {
            self.follow_rendering(asked);
        }
        // A sort asked for under the read waited for it, and so did a
        // folder read: now is the first chance, and sooner than the next
        // poll.
        let _ = self.poll_order();
        let _ = self.settle_folder();
        let _ = self.settle_arranged();
        Effect::Redraw
    }

    /// The file on screen has just arrived, `took` after it was asked for:
    /// how long its kind takes is noted for the next of them, and the
    /// thumbnails of the files either side are asked for ahead of the rest,
    /// so that a step either way has one to stand in while it is read.
    /// Asked for only where the screen does not hold them already; a
    /// thumbnail in the cache is a quick read, one that is not is made on
    /// the thumbnail thread, which yields to the loader's.
    fn arrived_in(&mut self, took: std::time::Duration) {
        if let Some(current) = &self.current
            && let Some(format) = current.file.reader
        {
            let pixels = f64::from(current.image.width) * f64::from(current.image.height);
            if pixels > 0.0 {
                self.read_rates.insert(format, took.as_secs_f64() / pixels);
            }
        }
        let wanted: Vec<PathBuf> = self
            .files
            .neighbors()
            .into_iter()
            .filter(|path| self.thumbs.get(path).is_none() && !self.chooser.given_up(path))
            .map(Path::to_path_buf)
            .collect();
        if !wanted.is_empty() {
            self.thumbnailer.prioritize(wanted);
        }
    }

    /// Draws one frame, and does what the interface on it asked for. Says
    /// whether the next frame is owed already: for a move still in flight —
    /// asked for from here rather than timed from the loop, so that it
    /// comes when the compositor is ready for one and the move plays at the
    /// display's own rate — or for what a press changed.
    fn redraw(&mut self) -> Effect {
        if self.shown.is_none() {
            return Effect::Nothing;
        }

        // A move that has landed is over: what is on screen is `view`
        // itself, and the frames it was asking for can stop.
        let now = Instant::now();
        if self.motion.as_ref().is_some_and(|motion| motion.done(now)) {
            self.motion = None;
        }
        self.show_due_frame();
        for (path, thumb) in std::mem::take(&mut self.pending_thumbs) {
            self.hold_thumb(path, thumb);
        }

        // The frame's one reading of the window and the view: what the
        // picture is placed by, and what everything under it reads.
        let sight = self.sight_at(now);
        let Sight {
            scale, viewport, ..
        } = sight;
        let view = sight.view;
        let placement = view.placement(self.image_size(), viewport);
        let thumbnail = self.minimap_placement(&sight);
        // Through the same placement the frame is drawn with: the glass
        // magnifies what the eye rings on this very frame.
        let loupe = self
            .loupe_in(&sight)
            .map(|loupe| ui::loupe::glass(loupe, placement, scale));
        let headroom = self.headroom();
        // What holds this frame, read once for the frame's input and for
        // the words the interface may ask for.
        let conditions = self.conditions();
        let input = self.frame_input_under(&sight, &conditions);
        // A thumbnail standing in for the file on its way in takes the
        // picture it is replacing off the image layer, the minimap and the
        // glass with it.
        let picture = input.standin.is_none();
        let thumbnail = thumbnail.filter(|_| picture);
        let loupe = loupe.filter(|_| picture);
        let namer = self.namer_under(conditions);
        let backdrop = ui::backdrop(&self.theme);
        // The checkerboard is drawn at the monitor's own scale, however
        // large the interface is.
        let device_scale = self.device_scale();

        let Some(shown) = &mut self.shown else {
            return Effect::Nothing;
        };
        let mut commands = Vec::new();
        let (painted, textures) = shown.gui.run(&shown.window, |ui| {
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
        // The one check that the application's figure for the interface's
        // point and egui's cannot drift apart: every conversion of the
        // pointer and the viewport above was made with `scale`.
        debug_assert!(
            (shown.gui.ctx.pixels_per_point() - scale).abs() < 1e-4,
            "egui lays out at {} points, the application at {scale}",
            shown.gui.ctx.pixels_per_point(),
        );

        let fallback = Display::default();
        let display = self
            .current
            .as_ref()
            .map(|current| &current.display)
            .unwrap_or(&fallback);

        let scene = Scene {
            picture: picture.then(|| Draw {
                view: placement,
                thumbnail,
                loupe,
                mark_clipped: self.panels.mark_clipped,
                headroom,
                lift: self
                    .current
                    .as_ref()
                    .and_then(|current| current.lift.as_deref()),
                turn: self
                    .current
                    .as_ref()
                    .map_or(Turn::NONE, |current| current.turn),
            }),
            display,
            ui: &painted,
            scale: device_scale,
            backdrop,
            headroom,
        };
        match shown.renderer.render(scene, textures) {
            Ok(()) => self.reported_error = false,
            Err(error) => {
                if !self.reported_error {
                    crate::report(&error);
                    self.reported_error = true;
                }
            }
        }

        // What the interface asked for is done once the frame is off: it
        // was drawn from the state as it was, and the next frame shows what
        // the press did.
        let mut owed = Effect::redraw_if(self.motion.is_some());
        for command in commands {
            owed = owed.also(self.act(command));
        }
        owed
    }

    /// Pays what an event left the window owing: the frame, or the exit.
    /// The one place a frame is asked for, called last by each of the
    /// handlers.
    fn settle(&mut self, effect: Effect, event_loop: &ActiveEventLoop) {
        self.pay(effect, event_loop);
        // What the menus show as they next open, now that the handler has
        // changed what it was going to.
        #[cfg(target_os = "macos")]
        self.publish_menu();
    }

    /// [`App::settle`] with the menus left as they were published: for a
    /// bare move of the pointer, which changes nothing they show but whether
    /// the pointer is on a pixel of the picture — and a change to that is a
    /// frame owed, whose own settling publishes it.
    fn pay(&mut self, effect: Effect, event_loop: &ActiveEventLoop) {
        match effect {
            Effect::Redraw => {
                if let Some(shown) = &self.shown {
                    shown.window.request_redraw();
                }
            }
            Effect::Quit => event_loop.exit(),
            Effect::Nothing => {}
        }
    }
}

/// Whether the surface should be the HDR one, from what the compositor
/// says the monitor is in and what the switch asks for. The monitor
/// decides where the compositor says: one in HDR mode gets the HDR surface,
/// which costs the compositor nothing and gives the picture the room, and
/// one in SDR mode gets the SDR surface — unless `--output hdr` asked for
/// the other regardless, which is the one route left that asks a
/// compositor to switch a monitor over. Where nothing says what the monitor
/// is, what was asked for is all there is to go on.
fn surface_wanted(monitor: Option<Mode>, preference: HdrPreference) -> bool {
    match monitor {
        Some(Mode::Hdr) => true,
        Some(Mode::Sdr) | None => preference == HdrPreference::On,
    }
}

/// Whether the picture is going out with room above SDR white. It takes
/// three things: a surface with the room, a monitor not known to be in SDR
/// mode — a compositor maps an HDR surface down for one that is, and the
/// room is not there however the surface was made — and the switch not
/// having turned it off.
fn headroom_of(surface_hdr: bool, monitor: Option<Mode>, preference: HdrPreference) -> Headroom {
    if surface_hdr && monitor != Some(Mode::Sdr) && preference != HdrPreference::Off {
        Headroom::Above
    } else {
        Headroom::None
    }
}

/// Whether the switch has anything to switch, and where it has not, which
/// of the two reasons: an HDR color space has to be offered for the window,
/// and the monitor has to be in HDR mode — or nothing able to say what it
/// is in. Where the compositor can say and has not yet, which is the moment
/// before the window has landed on a monitor, the answer is
/// [`Hdr::NotInHdrMode`]: most monitors are SDR, and a switch that lit for
/// a frame and then died would be the switch having been wrong.
fn hdr_state_of(offered: bool, speaks_modes: bool, monitor: Option<Mode>) -> Hdr {
    if !offered {
        return Hdr::Unsupported;
    }
    if speaks_modes && monitor != Some(Mode::Hdr) {
        return Hdr::NotInHdrMode;
    }
    Hdr::Available
}

/// Said when the configuration file has been read again and taken whole.
const RECONFIGURED: &str = "Configuration reloaded.";

/// Said when a raw arrives with the camera's JPEG asked for and none in it.
const NO_CAMERA_JPEG_SHOWN: &str = "No camera JPEG in this file; showing the developed picture.";

/// How a file arriving stands to the picture on screen, which is what
/// decides what carries over to it and what starts afresh.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Arrival {
    /// The file on screen read again at the size it was — changed on disk,
    /// or another page of it: everything stays, the user watching one spot
    /// for the change.
    Reread,
    /// The file on screen read again at another size: a new shape to fit,
    /// its settings started over.
    Reshaped,
    /// Another file, of the same size as the picture on screen: the view
    /// carries over, the display is its own.
    Beside,
    /// Another file of another size, or a file into an empty window: put
    /// back as it was left, or fitted afresh.
    Anew,
    /// The file on screen in its other rendering: the view is rescaled so the
    /// same detail stays where it was, the display starts over.
    Rerendered,
}

/// How a file `size` pixels across, read in `mode`, stands to the picture
/// on screen — `shown`, its path and size, where there is one — and whether
/// its arrival is a step between files at all. The file on screen being
/// read again is not a step, whatever it has become: it is neither a
/// departure to be put away nor a return to be restored.
fn arrival(
    mode: Reload,
    path: &Path,
    size: [f32; 2],
    shown: Option<(&Path, [f32; 2])>,
) -> (Arrival, bool) {
    let same_size = shown.is_some_and(|(_, shown)| shown == size);
    let same_file = mode != Reload::Fresh;
    let stepping = shown.is_some_and(|(shown, _)| shown != path);
    let arrival = match (same_file, same_size) {
        _ if mode == Reload::Rendering => Arrival::Rerendered,
        (true, true) => Arrival::Reread,
        (true, false) => Arrival::Reshaped,
        (false, true) => Arrival::Beside,
        (false, false) => Arrival::Anew,
    };
    (arrival, stepping)
}

/// The view a file arriving as `arrival` is shown with, from `view`, the one
/// on screen, and `kept`, what the file was left in if it has been here;
/// `shown_size` is the size of the picture on screen and `size` the size of
/// the one arriving, for a rendering that carries the view across. Shared by the arrival itself and by the thumbnail that stands in for the
/// file while it is read, so that the stand-in lands where the picture will.
fn arriving_view(
    arrival: Arrival,
    kept: Option<&Settings>,
    view: View,
    shown_size: Option<[f32; 2]>,
    size: [f32; 2],
) -> View {
    match (arrival, kept) {
        // The same picture, or one of the same size as the one it is
        // arriving beside — which is almost always part of a set to be
        // compared: frames of a sequence, or one exposure against
        // another, where the point is that the same detail stays under
        // the same pixels. So the pan and zoom carry over from the
        // picture leaving the screen, ahead of anything this file was
        // left in itself: what the comparison is being made at is where
        // the eye already is, not where this file happened to be the
        // last time it was looked at.
        (Arrival::Reread | Arrival::Beside, _) => view,
        // Back to a file of another size that has been here before:
        // exactly where it was left. The magnification filter is not part
        // of a view — it is a standing preference — so it stays as it is.
        (Arrival::Anew, Some(settings)) => {
            let mut left = settings.view;
            left.set_upscale(view.upscale());
            left
        }
        // A new shape, seen for the first time, so it is fitted afresh.
        (Arrival::Anew, None) | (Arrival::Reshaped, _) => {
            let mut fitted = view;
            fitted.reset();
            fitted
        }
        // The same scene rendered again: the detail under the center of
        // the window stays there, at the size it was on screen.
        (Arrival::Rerendered, _) => {
            let mut rescaled = view;
            if let Some(from) = shown_size {
                rescaled.rescale(from, size);
            }
            rescaled
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
/// have been replaced between being read and being asked about. `reader`
/// is what the loader's decoder said the file was, handed over with the
/// picture rather than sniffed again here on the loop.
fn file_facts(path: &std::path::Path, reader: Option<&'static str>) -> FileFacts {
    let metadata = std::fs::metadata(path).ok();
    FileFacts {
        path: path.display().to_string(),
        bytes: metadata.as_ref().map(|metadata| metadata.len()),
        modified: metadata.and_then(|metadata| metadata.modified().ok()),
        reader,
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
        let now = Instant::now();
        // The window kept waiting on the command line's order: opened once
        // the order is in, or once the wait has gone on long enough to say.
        if self.arranging.awaiting_window && self.window_due().is_none_or(|due| now >= due) {
            self.arranging.awaiting_window = false;
            self.open_window(event_loop);
        }
        let moved = self.sync_monitor();

        // The hold on the window's size, given up unanswered.
        if self
            .sizing
            .held
            .is_some_and(|since| now >= since + SIZING_GRACE)
        {
            self.release_size();
        }
        let polled = self.poll(now);
        let (timed, next_frame) = self.tick(now);

        // Sleep until the next thing with a time on it: the file check, the
        // moment a read that is still going becomes worth mentioning, the
        // moment a message has had its time, or the moment egui asked for. A
        // read that finishes first wakes us through the proxy instead.
        let mut deadline = self.next_poll;
        for due in [
            self.toasts.deadline(),
            self.shown.as_ref().and_then(|shown| shown.gui.deadline()),
            next_frame,
            self.sizing.held.map(|since| since + SIZING_GRACE),
            self.window_due().filter(|_| self.arranging.awaiting_window),
            self.folder_due(now),
            self.arranging_due(now),
        ]
        .into_iter()
        .flatten()
        {
            deadline = deadline.min(due);
        }
        let announced = match self.files.announce_slow_read(now) {
            Announce::Waiting(due) => {
                deadline = deadline.min(due);
                Effect::Nothing
            }
            Announce::Now => Effect::Redraw,
            Announce::Nothing => Effect::Nothing,
        };
        event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
        self.settle(moved.also(polled).also(timed).also(announced), event_loop);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        let effect = match event {
            UserEvent::Decoded(decoded) => {
                let delivered = self.deliver(*decoded);
                // Nothing ever reached the screen and nothing else is coming:
                // every file named on the command line failed to decode.
                // Stop, rather than sit in an empty window with nothing on
                // the way. A choice made in the window is answered in the
                // window instead, which stays up for the next choice.
                if self.showed_nothing() && self.files.is_idle() {
                    Effect::Quit
                } else {
                    delivered
                }
            }
            UserEvent::Picked(picked) => self.picked(picked),
            UserEvent::Folder(listed) => self.folder_read(listed),
            UserEvent::Arranged(arranged) => self.arranged_read(arranged),
            UserEvent::Measured(measured) => self.measured(*measured),
            UserEvent::Tags(delivered) => self.tags_read(*delivered),
            UserEvent::Opened(paths) => {
                self.open_sent(paths);
                // The files the program was launched to open, which the
                // window waited for.
                if self.shown.is_none() {
                    self.open_window_when_ready(event_loop);
                }
                Effect::Redraw
            }
            #[cfg(target_os = "macos")]
            UserEvent::Menu(chosen) => self.chose(chosen),
            UserEvent::Clipboard(offered) => self.clipboard_changed(offered),
            UserEvent::Monitor => self.sync_monitor(),
            // A frame from the player of a file already stepped past is news
            // about nothing on screen.
            UserEvent::Frame(event) => Effect::redraw_if(
                self.animation
                    .as_ref()
                    .is_some_and(|animation| animation.is(event.generation)),
            ),
            // A frame only while the chooser is up: with it closed nothing
            // on screen shows a thumbnail, and the news is kept for when it
            // opens.
            UserEvent::Thumbnail(delivered) => {
                self.take_thumbnail(*delivered);
                Effect::redraw_if(self.chooser_open() || self.filmstrip_showing())
            }
        };
        self.settle(effect, event_loop);
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(target_os = "macos")]
        self.install_menubar();
        if self.shown.is_some() {
            return;
        }
        // Launched by Finder to open files, which are on their way through
        // the proxy: the window waits for them, to open at the first one's
        // size rather than open empty and be sized again where it stands.
        #[cfg(target_os = "macos")]
        if crate::finder::sent_any() {
            return;
        }
        self.open_window_when_ready(event_loop);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        // egui sees every event first. What it takes for itself — a press on
        // one of its widgets — goes no further; what it merely wants painted
        // for, a pointer crossing one of them, is a redraw and nothing else.
        // Except for the redraw itself: egui answers `RedrawRequested` with
        // "repaint" too, meaning paint now, and a frame asked for on the
        // strength of that would be a frame asking for the next for ever.
        //
        // Except `Tab`, which is kept from egui while none of its fields has
        // the keyboard: egui-winit reports it taken whether or not one has,
        // and egui would move its focus with it — to a scroll area, say,
        // which then wants every key after it. Nothing here takes the focus
        // a press would give it, so there is nothing for `Tab` to move to,
        // and the key is the table's.
        let tab = matches!(
            &event,
            WindowEvent::KeyboardInput {
                event: KeyEvent {
                    logical_key: winit::keyboard::Key::Named(winit::keyboard::NamedKey::Tab),
                    ..
                },
                ..
            }
        );
        //
        // And the key bound to the chooser, while no field has the keyboard:
        // it goes to the key table alone, so that the press that opens the
        // chooser is not waiting in egui's input to be typed into the field
        // it has just focused. Once the chooser is up its field has the
        // keyboard, and the key is typing like any other.
        let chooser_key = match &event {
            WindowEvent::KeyboardInput { event: key, .. }
                if key.state == winit::event::ElementState::Pressed =>
            {
                let region = matches!(self.marking.selection, ui::Selection::Shown(_));
                let logical = input::logical_key(key, self.pointer.modifiers);
                self.keys
                    .action_for(&logical, key.physical_key, self.pointer.modifiers, region)
                    == Some(input::Action::OpenChooser)
            }
            _ => false,
        };
        let response = self
            .shown
            .as_mut()
            .filter(|shown| {
                let wants = shown.gui.ctx.egui_wants_keyboard_input();
                (!tab || wants) && !(chooser_key && !wants)
            })
            .map(|shown| shown.gui.on_event(&shown.window, &event));
        let repaint = Effect::redraw_if(
            response.as_ref().is_some_and(|response| response.repaint)
                && !matches!(event, WindowEvent::RedrawRequested),
        );
        let consumed = response.is_some_and(|response| response.consumed);
        let moved = matches!(event, WindowEvent::CursorMoved { .. });
        let effect = match event {
            _ if consumed && !matches!(event, WindowEvent::RedrawRequested) => Effect::Nothing,
            WindowEvent::CloseRequested => Effect::Quit,
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.shown.as_mut().map(|shown| &mut shown.renderer) {
                    renderer.resize(size.width, size.height);
                }
                // The compositor has answered the size the window asked
                // for, or the user has taken hold of the window: either
                // way the hold on its size is let go.
                self.release_size();
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
                // The loupe follows the pointer itself, not the pixel under
                // it: a move within one pixel of the picture still moves it.
                let loupe_was = self.loupe();
                self.pointer.cursor = Some([position.x as f32, position.y as f32]);
                Effect::redraw_if(self.pointer_pixel() != was_over || self.loupe() != loupe_was)
            }
            WindowEvent::CursorLeft { .. } => {
                let was_over = self.pointer_pixel().is_some();
                self.pointer.cursor = None;
                Effect::redraw_if(was_over)
            }
            WindowEvent::ScaleFactorChanged { .. } => Effect::Redraw,
            WindowEvent::KeyboardInput { event, .. } => {
                let key = input::logical_key(&event, self.pointer.modifiers);
                self.handle_key(&key, event.physical_key, event.state)
            }
            // A key held as the focus goes is released somewhere else.
            WindowEvent::Focused(false) => {
                self.keys_lost();
                Effect::Nothing
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => Effect::Nothing,
        };
        if moved {
            self.pay(repaint.also(effect), event_loop);
        } else {
            self.settle(repaint.also(effect), event_loop);
        }
    }

    /// The loop is done. A copy that is still being prepared gets to finish
    /// handing its bytes over first: the thread doing it would otherwise go
    /// down with the process, and the whole point of copying here is that it
    /// outlasts the window.
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.copying.join_all();
        self.state.save(self.kept_state());
    }
}

#[cfg(test)]
mod tests;
