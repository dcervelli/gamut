//! A small GPU-accelerated image viewer.

mod app;
mod formats;
mod renderer;
mod view;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use winit::event_loop::{ControlFlow, EventLoop};

use app::App;

const USAGE: &str = "\
image-view — preview images

USAGE:
    image-view [OPTIONS] <FILE>...

The first file is shown, stretched to fit the window.

OPTIONS:
    -h, --help       Show this help
    -V, --version    Show the version
    --               Treat every later argument as a file name

KEYS:
    q, Esc           Quit
    +, =             Zoom in
    -, _             Zoom out
    0                Actual size (100%)
    Arrows           Pan
    f                Cycle fit / fit width / fit height
    n, p             Next / previous file
";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("image-view: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let files = match parse_args()? {
        Some(files) => files,
        None => return Ok(()),
    };

    // Decode the first image up front, so a bad path or unsupported format is
    // a plain command-line error rather than an empty window.
    let first = formats::load(&files[0])?;

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App::new(files, first);
    event_loop.run_app(&mut app)?;
    Ok(())
}

/// `Ok(None)` means we printed help or the version and should exit quietly.
fn parse_args() -> Result<Option<Vec<PathBuf>>> {
    let mut files = Vec::new();
    let mut only_files = false;

    for arg in std::env::args_os().skip(1) {
        if !only_files {
            match arg.to_str() {
                Some("-h") | Some("--help") => {
                    print!("{USAGE}");
                    return Ok(None);
                }
                Some("-V") | Some("--version") => {
                    println!("image-view {}", env!("CARGO_PKG_VERSION"));
                    return Ok(None);
                }
                Some("--") => {
                    only_files = true;
                    continue;
                }
                Some(other) if other.starts_with('-') && other.len() > 1 => {
                    anyhow::bail!("unknown option `{other}` (try --help)");
                }
                _ => {}
            }
        }
        files.push(PathBuf::from(arg));
    }

    if files.is_empty() {
        eprint!("{USAGE}");
        anyhow::bail!("no image files given");
    }
    Ok(Some(files))
}
