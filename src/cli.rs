//! The command line: what was asked for, and `--help`.
//!
//! The key sections of the help text are rendered from [`KEYS`], so a binding
//! is documented by the same edit that adds it.

use std::fmt::Write as _;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use crate::app::Options;
use crate::app::input::{KEYS, Section};
use crate::image::decode::Overrides;
use crate::image::display::{AutoWindow, Colormap, Startup, ToneMap};
use crate::image::{Primaries, Transfer};
use crate::render::{HdrPreference, Upscale};

const OPTIONS: &str = "\
image-view — preview images

USAGE:
    image-view [OPTIONS] <PATH>...

The first file is shown, stretched to fit the window, and re-read whenever
something else writes to it. A directory stands for the images directly
inside it, in name order.

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
        --info              Start with the file information panel showing
        --no-minimap        Start with the minimap off; it is on by default
        --timing            Print decode and startup timings to stdout
    --                      Treat every later argument as a path
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
        // `probe` runs a real header parser on the main thread before any
        // window exists; a panic in one would take the process down before it
        // drew anything. Caught, it is just another file that would not read.
        let probed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::image::decode::probe(path)
        }))
        .unwrap_or_else(|_| {
            Err(anyhow::anyhow!(
                "panicked while reading the header of {}",
                path.display()
            ))
        });
        match probed {
            Ok(size) => {
                // Worth mentioning only once we know we are carrying on
                // without them. If nothing can be read at all, the error we
                // return is reported by `main`, and saying it here as well
                // would print it twice.
                for problem in &skipped {
                    eprintln!(
                        "image-view: {}",
                        crate::escape_controls(&format!("{problem:#}"))
                    );
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

/// Replaces every directory named on the command line with the images
/// directly inside it, in name order. One level deep: a directory names a
/// place to look, not a tree to walk.
///
/// Inside a directory the extension decides, since the alternative is opening
/// every file there to look at its leading bytes. A file named on the command
/// line is still read for what it holds rather than what it is called.
fn expand_directories(named: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    let extensions = crate::image::decode::supported_extensions();
    let mut files = Vec::new();
    let mut empty = Vec::new();
    for path in named {
        if !path.is_dir() {
            files.push(path);
            continue;
        }
        let mut found = Vec::new();
        let entries =
            std::fs::read_dir(&path).with_context(|| format!("reading {}", path.display()))?;
        for entry in entries {
            let entry = entry.with_context(|| format!("reading {}", path.display()))?;
            let candidate = entry.path();
            let extension = candidate
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
                .unwrap_or_default();
            // The extension is checked first because it costs nothing; the
            // directory test that follows it is for the rare directory named
            // like an image.
            if extensions.contains(&extension.as_str()) && !candidate.is_dir() {
                found.push(candidate);
            }
        }
        if found.is_empty() {
            empty.push(path);
            continue;
        }
        found.sort();
        files.append(&mut found);
    }

    if !files.is_empty() {
        // Worth mentioning only once we know we are carrying on without them,
        // as with a file whose header will not read.
        for path in empty {
            eprintln!("image-view: no images in {}", crate::shown_path(&path));
        }
        return Ok(files);
    }
    let names: Vec<String> = empty
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    bail!("no images in {}", names.join(", "))
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
    let mut info = false;
    let mut minimap = true;
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
                    overrides.transfer = Some(Transfer::parse(&value).ok_or_else(|| {
                        match value.strip_prefix("gamma:") {
                            Some(exponent) => anyhow::anyhow!(
                                "`--transfer gamma:` needs a number, got `{exponent}`"
                            ),
                            None => {
                                anyhow::anyhow!("unknown transfer function `{value}` (try --help)")
                            }
                        }
                    })?);
                    continue;
                }
                Some("--no-gain-map") => {
                    overrides.gain_map = false;
                    continue;
                }
                Some("--primaries") => {
                    let value = next_value(&mut arguments, "--primaries")?;
                    overrides.primaries = Some(Primaries::parse(&value).ok_or_else(|| {
                        anyhow::anyhow!("unknown primaries `{value}` (try --help)")
                    })?);
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
                    startup.auto = Some(AutoWindow::parse(&value).ok_or_else(|| {
                        anyhow::anyhow!("unknown window mode `{value}` (try --help)")
                    })?);
                    continue;
                }
                Some("--exposure") => {
                    let value = next_value(&mut arguments, "--exposure")?;
                    let stops: f32 = value.parse().map_err(|_| {
                        anyhow::anyhow!("`--exposure` needs a number, got `{value}`")
                    })?;
                    // `nan` and `inf` both parse as `f32`, and unclamped they
                    // would put a non-finite gain into the shader uniform, the
                    // pixel readout and the status bar. The keyboard path
                    // clamps to +/-16 stops; the command line gets the same
                    // ceiling, and rejects a value that is not a number at all.
                    if !stops.is_finite() {
                        anyhow::bail!("`--exposure` needs a finite number, got `{value}`");
                    }
                    startup.exposure_stops = Some(stops.clamp(-16.0, 16.0));
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
                Some("--info") => {
                    info = true;
                    continue;
                }
                Some("--timing") => {
                    crate::timing::enable();
                    continue;
                }
                Some("--no-minimap") => {
                    minimap = false;
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
    let files = expand_directories(files)?;
    Ok(Some(Args {
        files,
        options: Options {
            overrides,
            startup,
            hdr,
            histogram,
            info,
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

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_images")
    }

    /// A directory stands for the images in it, in name order, and for
    /// nothing else: the colour profile, the shell script and the README
    /// lying beside them are not files to show.
    #[test]
    fn a_directory_becomes_the_images_inside_it() {
        let files = expand_directories(vec![fixtures()]).expect("test_images/ holds images");
        let mut sorted = files.clone();
        sorted.sort();
        assert_eq!(files, sorted, "the list is in name order");
        assert!(files.contains(&fixtures().join("png-rgb8.png")));

        let extensions = crate::image::decode::supported_extensions();
        for file in &files {
            let extension = file
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            assert!(
                extensions.contains(&extension.as_str()),
                "{} is not an image and should not be listed",
                file.display()
            );
        }
        assert!(
            !files.contains(&fixtures().join("unsupported.tga")),
            "a format we cannot read is not worth stepping through"
        );
    }

    /// Whatever is not a directory is passed through untouched, extension and
    /// all, so that the sniffing which opens a JPEG named `.png` still has its
    /// chance and a missing file still reports itself.
    #[test]
    fn files_are_left_as_they_were_named() {
        let named = vec![
            fixtures().join("unsupported.tga"),
            PathBuf::from("no-such-file"),
        ];
        assert_eq!(
            expand_directories(named.clone()).expect("names to pass through"),
            named
        );
    }

    /// A directory with nothing to show in it is an error worth naming,
    /// rather than an empty list that opens a window onto nothing.
    #[test]
    fn a_directory_holding_no_images_is_reported() {
        let empty = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
        let error = expand_directories(vec![empty.clone()]).expect_err("src/ holds no images");
        assert!(error.to_string().contains(&empty.display().to_string()));
    }
}
