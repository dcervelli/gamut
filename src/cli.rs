//! The command line: what was asked for, and `--help`.
//!
//! The key sections of the help text are rendered from [`KEYS`], so a binding
//! is documented by the same edit that adds it.

use std::fmt::Write as _;
use std::path::PathBuf;

use anyhow::{Result, bail};

use crate::app::Options;
use crate::app::input::{KEYS, Section};
use crate::image::decode::Overrides;
use crate::image::display::{AutoWindow, Colormap, Startup, ToneMap};
use crate::image::{Primaries, Transfer};
use crate::render::{HdrPreference, Upscale};

const OPTIONS: &str = "\
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
";

/// The whole of `--help`: the options, then every key under its heading.
pub fn usage() -> String {
    let mut text = OPTIONS.to_string();
    for (section, heading) in [
        (Section::View, "VIEW KEYS"),
        (Section::Display, "DISPLAY KEYS"),
    ] {
        let _ = writeln!(text, "\n{heading}:");
        for binding in KEYS.iter().filter(|binding| binding.section == section) {
            let _ = writeln!(text, "    {:<17}{}", binding.shown, binding.help);
        }
    }
    text
}

/// The first file whose header can be read, where in the list it was, and the
/// size it claims to be. Fails only if none of them can be read, reporting the
/// first file's error since that is the one the user most likely meant.
pub fn first_readable(files: &[PathBuf]) -> Result<(usize, Option<[f32; 2]>)> {
    let mut skipped = Vec::new();
    for (index, path) in files.iter().enumerate() {
        match crate::image::decode::probe(path) {
            Ok(size) => {
                // Worth mentioning only once we know we are carrying on
                // without them. If nothing can be read at all, the error we
                // return is reported by `main`, and saying it here as well
                // would print it twice.
                for problem in &skipped {
                    eprintln!("image-view: {problem:#}");
                }
                return Ok((index, size.map(|(w, h)| [w as f32, h as f32])));
            }
            Err(error) => skipped.push(error),
        }
    }
    Err(skipped
        .into_iter()
        .next()
        .expect("the argument list is never empty"))
}

pub struct Args {
    pub files: Vec<PathBuf>,
    pub options: Options,
}

/// `Ok(None)` means we printed help or the version and should exit quietly.
pub fn parse_args() -> Result<Option<Args>> {
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
                    print!("{}", usage());
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
        eprint!("{}", usage());
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every key is in the help text, and nothing in the help text is a key
    /// that does not exist: the two come from one table.
    #[test]
    fn the_help_text_lists_every_binding_once() {
        let text = usage();
        for binding in KEYS {
            let line = format!("    {:<17}{}", binding.shown, binding.help);
            assert_eq!(
                text.matches(&line).count(),
                1,
                "{:?} should appear exactly once",
                binding.shown
            );
        }
        assert!(text.contains("VIEW KEYS:\n") && text.contains("DISPLAY KEYS:\n"));
    }
}
