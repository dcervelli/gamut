//! A GPU-accelerated image previewer with color management.

mod app;
mod cli;
mod clipboard;
mod clock;
mod fuzzy;
mod image;
mod listing;
mod loader;
mod monitor;
mod motion;
mod openers;
mod pasted;
mod player;
mod render;
mod theme;
mod thumbnail;
mod thumbnailer;
mod timing;
mod ui;
mod view;
mod watch;

use std::path::Path;
use std::process::ExitCode;

use anyhow::Result;
use winit::event_loop::{ControlFlow, EventLoop};

use app::App;
use loader::Loader;

/// The word this program is called by: the binary a user types, the name in a
/// window title, the Arch package, the man page and the completions. It is the
/// crate's own name so that `Cargo.toml` is the single place to change it;
/// `cli`'s tests check that everything in `packaging/` still agrees.
pub(crate) const PROGRAM: &str = env!("CARGO_PKG_NAME");

/// The name this program is known by to a desktop: the Wayland `app_id` and
/// the class half of the X11 `WM_CLASS`, and the basename of the desktop entry
/// and of the icon, which have to match it for a taskbar to pair a window with
/// its icon. It is in the reverse-DNS form a desktop expects of an application
/// id, so it is written out here rather than taken from [`PROGRAM`] — a crate
/// name cannot hold the dots.
pub(crate) const APP_ID: &str = "com.dcervelli.gamut";

/// Replaces control characters with the replacement character before a string
/// reaches a terminal. A filename is attacker-chosen data, and a terminal
/// reads bytes like `\e]0;…\a` (retitle) or `\e[2J` (clear) or an OSC 52
/// clipboard write as commands; nothing this program prints should be able to
/// carry one. Ordinary names pass through unchanged.
pub(crate) fn escape_controls(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { '\u{FFFD}' } else { c })
        .collect()
}

/// A path rendered for a message, with any control characters defused.
pub(crate) fn shown_path(path: &Path) -> String {
    escape_controls(&path.display().to_string())
}

fn main() -> ExitCode {
    timing::begin();
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("gamut: {}", escape_controls(&format!("{error:#}")));
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode> {
    // Not a command line the user writes: it is how a copy re-runs this
    // program to hold the clipboard after the window has gone. Answered
    // before the arguments are parsed, since there is no image to show.
    let mut arguments = std::env::args_os().skip(1);
    if arguments
        .next()
        .is_some_and(|first| first == clipboard::SERVE_ARGUMENT)
    {
        clipboard::serve(arguments.next())?;
        return Ok(ExitCode::SUCCESS);
    }

    let Some(args) = cli::parse_args()? else {
        return Ok(ExitCode::SUCCESS);
    };

    // Only the header, which is cheap for every format we read. A bad path or
    // an unsupported format is still a plain command-line error rather than a
    // window that opens and closes, and the size it reports opens the window
    // at the right shape — but the pixels are left to the loader thread, so
    // that a large file no longer holds the window shut while it is read.
    let (index, size) = cli::first_readable(&args.files)?;

    // With a user event: it is how the loader hands finished images back and
    // how the monitor watch says a monitor has changed, and how either wakes
    // a loop that is otherwise asleep between one file check and the next.
    let event_loop = EventLoop::<app::UserEvent>::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let proxy = event_loop.create_proxy();
    let loader = Loader::new(move |decoded| {
        proxy
            .send_event(app::UserEvent::Decoded(Box::new(decoded)))
            .is_ok()
    });
    let proxy = event_loop.create_proxy();
    let wake: player::Wake =
        std::sync::Arc::new(move |event| proxy.send_event(app::UserEvent::Frame(event)).is_ok());
    let proxy = event_loop.create_proxy();
    let monitors = monitor::watch(move || {
        let _ = proxy.send_event(app::UserEvent::Monitor);
    });
    let proxy = event_loop.create_proxy();
    let thumbnailer = thumbnailer::Thumbnailer::new(args.options.overrides, move |delivered| {
        proxy
            .send_event(app::UserEvent::Thumbnail(Box::new(delivered)))
            .is_ok()
    });
    let mut app = App::new(
        args.files,
        args.named,
        index,
        size,
        args.options,
        app::Threads {
            loader,
            wake,
            monitors,
            thumbnailer,
        },
    );
    event_loop.run_app(&mut app)?;

    // Every file passed the header check and then failed to decode. Each
    // failure was reported as it happened, so the status is all that is left
    // to say.
    Ok(if app.showed_nothing() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}
