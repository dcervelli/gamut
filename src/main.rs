//! A GPU-accelerated image previewer with colour management.

mod app;
mod cli;
mod image;
mod loader;
mod render;
mod theme;
mod timing;
mod ui;
mod view;
mod watch;

use std::process::ExitCode;

use anyhow::Result;
use winit::event_loop::{ControlFlow, EventLoop};

use app::App;
use loader::Loader;

fn main() -> ExitCode {
    timing::begin();
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("image-view: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode> {
    let Some(args) = cli::parse_args()? else {
        return Ok(ExitCode::SUCCESS);
    };

    // Only the header, which is cheap for every format we read. A bad path or
    // an unsupported format is still a plain command-line error rather than a
    // window that opens and closes, and the size it reports opens the window
    // at the right shape — but the pixels are left to the loader thread, so
    // that a large file no longer holds the window shut while it is read.
    let (index, size) = cli::first_readable(&args.files)?;

    // With a user event: it is how the loader hands finished images back, and
    // how it wakes a loop that is otherwise asleep between one file check and
    // the next.
    let event_loop = EventLoop::<loader::Decoded>::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let loader = Loader::new(event_loop.create_proxy());
    let mut app = App::new(args.files, index, size, args.options, loader);
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
