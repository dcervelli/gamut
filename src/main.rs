//! A GPU-accelerated image previewer with colour management.

mod app;
mod image;
mod render;
mod view;
mod watch;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Result, bail};
use winit::event_loop::{ControlFlow, EventLoop};

use app::{App, Options};
use image::decode::Overrides;
use image::display::{AutoWindow, Colormap, Startup, ToneMap};
use image::{Primaries, Transfer};
use render::HdrPreference;
use view::Upscale;

const USAGE: &str = "\
image-view — preview images

USAGE:
    image-view [OPTIONS] <FILE>...

The first file is shown, stretched to fit the window, and re-read whenever
something else writes to it.

OPTIONS:
    -h, --help              Show this help
    -V, --version           Show the version
        --hdr               Use an HDR surface when the display offers one
        --transfer <FN>     Override the transfer function the file is assumed
                            to use: linear, srgb, pq, hlg, or gamma:<N>
        --primaries <P>     Override the colour primaries: bt709, p3, bt2020,
                            or adobe
        --no-gain-map       Show the SDR base image of an Ultra HDR JPEG,
                            rather than reconstructing the HDR one from the
                            gain map beside it
        --colormap <MAP>    Start with false colour on single-channel images:
                            gray, viridis, magma, or turbo
        --tone-map <MAP>    Start with clip, reinhard, or neutral
        --window <MODE>     Start with the window set to unit, minmax, or pct
        --exposure <STOPS>  Start at this exposure, in stops
        --upscale <FILTER>  How to resample above 100%: nearest or bicubic
        --histogram         Start with the histogram showing
        --minimap           Start with the minimap on
    --                      Treat every later argument as a file name

VIEW KEYS:
    q, Esc           Quit
    +, =             Zoom in
    -, _             Zoom out
    Wheel            Zoom about the pointer
    0                Actual size (100%)
    Arrows           Pan
    f                Cycle fit / fit width / fit height
    u                Cycle the filter used above 100%: nearest, bicubic
    n, p             Next / previous file

DISPLAY KEYS:
    e, E             Exposure down / up, half a stop
    a                Cycle the automatic window: unit, min/max, 99.8%
    [, ]             Slide the window down / up
    , .              Narrow / widen the window
    t                Cycle tone mapping: clip, reinhard, neutral
    c                Cycle false colour for single-channel images
    r                Reset display settings
    h                Toggle the histogram
    m                Toggle the minimap
    `                Toggle the interface panels
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
    let Some(args) = parse_args()? else {
        return Ok(());
    };

    // Decode up front, so a bad path or unsupported format is a plain
    // command-line error rather than an empty window. With several files,
    // step over the ones that fail exactly as `n` and `p` do later, rather
    // than refusing to start because the first of thirty is broken.
    let (index, first) = open_first(&args.files, args.options.overrides)?;

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App::new(args.files, index, first, args.options);
    event_loop.run_app(&mut app)?;
    Ok(())
}

/// The first file that decodes, and where in the list it was. Fails only if
/// none of them do, reporting the first file's error since that is the one
/// the user most likely meant.
fn open_first(files: &[PathBuf], overrides: Overrides) -> Result<(usize, image::DecodedImage)> {
    let mut skipped = Vec::new();
    for (index, path) in files.iter().enumerate() {
        match image::decode::load(path, overrides) {
            Ok(image) => {
                // Worth mentioning only once we know we are carrying on
                // without them. If nothing loads at all, the error we return
                // is reported by `main`, and saying it here as well would
                // print it twice.
                for problem in &skipped {
                    eprintln!("image-view: {problem:#}");
                }
                return Ok((index, image));
            }
            Err(error) => skipped.push(error),
        }
    }
    Err(skipped
        .into_iter()
        .next()
        .expect("the argument list is never empty"))
}

struct Args {
    files: Vec<PathBuf>,
    options: Options,
}

/// `Ok(None)` means we printed help or the version and should exit quietly.
fn parse_args() -> Result<Option<Args>> {
    let mut files = Vec::new();
    let mut overrides = Overrides::default();
    let mut startup = Startup::default();
    let mut hdr = HdrPreference::Off;
    let mut histogram = false;
    let mut minimap = false;
    let mut upscale = Upscale::default();
    let mut only_files = false;

    let mut arguments = std::env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        if !only_files {
            match argument.to_str() {
                Some("-h") | Some("--help") => {
                    print!("{USAGE}");
                    return Ok(None);
                }
                Some("-V") | Some("--version") => {
                    println!("image-view {}", env!("CARGO_PKG_VERSION"));
                    return Ok(None);
                }
                Some("--hdr") => {
                    hdr = HdrPreference::On;
                    continue;
                }
                Some("--transfer") => {
                    let value = next_value(&mut arguments, "--transfer")?;
                    overrides.transfer = Some(parse_transfer(&value)?);
                    continue;
                }
                Some("--no-gain-map") => {
                    overrides.gain_map = false;
                    continue;
                }
                Some("--primaries") => {
                    let value = next_value(&mut arguments, "--primaries")?;
                    overrides.primaries = Some(parse_primaries(&value)?);
                    continue;
                }
                Some("--colormap") => {
                    let value = next_value(&mut arguments, "--colormap")?;
                    startup.colormap = Some(
                        Colormap::parse(&value)
                            .ok_or_else(|| anyhow::anyhow!("unknown colormap `{value}`"))?,
                    );
                    continue;
                }
                Some("--tone-map") => {
                    let value = next_value(&mut arguments, "--tone-map")?;
                    startup.tone_map = Some(
                        ToneMap::parse(&value)
                            .ok_or_else(|| anyhow::anyhow!("unknown tone map `{value}`"))?,
                    );
                    continue;
                }
                Some("--window") => {
                    let value = next_value(&mut arguments, "--window")?;
                    startup.auto = Some(parse_window(&value)?);
                    continue;
                }
                Some("--exposure") => {
                    let value = next_value(&mut arguments, "--exposure")?;
                    startup.exposure_stops = Some(value.parse().map_err(|_| {
                        anyhow::anyhow!("`--exposure` needs a number, got `{value}`")
                    })?);
                    continue;
                }
                Some("--upscale") => {
                    let value = next_value(&mut arguments, "--upscale")?;
                    upscale = Upscale::parse(&value)
                        .ok_or_else(|| anyhow::anyhow!("unknown upscale filter `{value}`"))?;
                    continue;
                }
                Some("--histogram") => {
                    histogram = true;
                    continue;
                }
                Some("--minimap") => {
                    minimap = true;
                    continue;
                }
                Some("--") => {
                    only_files = true;
                    continue;
                }
                Some(other) if other.starts_with('-') && other.len() > 1 => {
                    bail!("unknown option `{other}` (try --help)");
                }
                _ => {}
            }
        }
        files.push(PathBuf::from(argument));
    }

    if files.is_empty() {
        eprint!("{USAGE}");
        bail!("no image files given");
    }
    Ok(Some(Args {
        files,
        options: Options {
            overrides,
            startup,
            hdr,
            histogram,
            minimap,
            upscale,
        },
    }))
}

fn next_value(
    arguments: &mut impl Iterator<Item = std::ffi::OsString>,
    option: &str,
) -> Result<String> {
    match arguments.next().and_then(|value| value.into_string().ok()) {
        Some(value) => Ok(value),
        None => bail!("`{option}` needs a value (try --help)"),
    }
}

fn parse_transfer(value: &str) -> Result<Transfer> {
    Ok(match value.to_ascii_lowercase().as_str() {
        "linear" => Transfer::Linear,
        "srgb" => Transfer::Srgb,
        "pq" => Transfer::Pq,
        "hlg" => Transfer::Hlg,
        other => match other.strip_prefix("gamma:") {
            Some(exponent) => Transfer::Gamma(exponent.parse().map_err(|_| {
                anyhow::anyhow!("`--transfer gamma:` needs a number, got `{exponent}`")
            })?),
            None => bail!("unknown transfer function `{value}` (try --help)"),
        },
    })
}

fn parse_window(value: &str) -> Result<AutoWindow> {
    Ok(match value.to_ascii_lowercase().as_str() {
        "unit" | "off" => AutoWindow::Off,
        "minmax" | "min-max" => AutoWindow::MinMax,
        "pct" | "percentile" => AutoWindow::Percentile,
        other => bail!("unknown window mode `{other}` (try --help)"),
    })
}

fn parse_primaries(value: &str) -> Result<Primaries> {
    Ok(match value.to_ascii_lowercase().as_str() {
        "bt709" | "srgb" | "rec709" => Primaries::Bt709,
        "p3" | "displayp3" => Primaries::DisplayP3,
        "bt2020" | "rec2020" => Primaries::Bt2020,
        "adobe" | "adobergb" => Primaries::AdobeRgb,
        other => bail!("unknown primaries `{other}` (try --help)"),
    })
}
