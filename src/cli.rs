//! The command line: what was asked for, and `--help`.
//!
//! The key sections of the help text are rendered from [`KEYS`], so a binding
//! is documented by the same edit that adds it.

use std::fmt::Write as _;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use crate::APP_ID;
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

/// The headings the keys are listed under, in the order they are printed.
/// Shared by `--help` and the manual page so that neither can grow a section
/// the other does not have.
const SECTIONS: [(Section, &str); 5] = [
    (Section::Zoom, "ZOOM AND POSITION KEYS"),
    (Section::Files, "FILE KEYS"),
    (Section::Clipboard, "CLIPBOARD KEYS"),
    (Section::Interface, "INTERFACE KEYS"),
    (Section::Display, "DISPLAY KEYS"),
];

/// The whole of `--help`: the options, then every key under its heading.
pub fn usage() -> String {
    let mut text = OPTIONS.to_string();
    for (section, heading) in SECTIONS {
        let _ = writeln!(text, "\n{heading}:");
        for binding in KEYS.iter().filter(|binding| binding.section == section) {
            let _ = writeln!(text, "    {:<17}{}", binding.shown, binding.help);
        }
    }
    text
}

/// Where the description column of `OPTIONS` begins. Both the option lines and
/// their continuations are laid out against it, so it is the one number the
/// manual page has to know to take that block apart again.
const DESCRIPTION_COLUMN: usize = 28;

/// Text with the three characters roff reads as instructions defused: a
/// backslash starts an escape, a hyphen is a typographic minus that renderers
/// are free to break a line on, and a leading dot or apostrophe makes the line
/// a request rather than words.
fn roff(text: &str) -> String {
    let escaped = text.replace('\\', r"\e").replace('-', r"\-");
    match escaped.starts_with('.') || escaped.starts_with('\'') {
        true => format!(r"\&{escaped}"),
        false => escaped,
    }
}

/// `OPTIONS` taken apart into one entry per option: the flag column, and the
/// description gathered back into a single line from however many it was
/// wrapped over.
///
/// The block is written for a terminal, where the wrapping is the layout. A
/// manual page sets its own width, so the wrapping has to be undone rather
/// than carried across.
fn option_entries() -> Vec<(String, String)> {
    let body = OPTIONS
        .split_once("\nOPTIONS:\n")
        .expect("OPTIONS has its heading")
        .1;
    let mut entries: Vec<(String, String)> = Vec::new();
    for line in body.lines().filter(|line| !line.trim().is_empty()) {
        match line.len() > DESCRIPTION_COLUMN && line[..DESCRIPTION_COLUMN].trim().is_empty() {
            // A continuation: it belongs to the option above it.
            true => {
                let entry = entries
                    .last_mut()
                    .expect("a continuation follows an option");
                entry.1.push(' ');
                entry.1.push_str(line[DESCRIPTION_COLUMN..].trim());
            }
            false => {
                let (flags, description) = line.split_at(DESCRIPTION_COLUMN.min(line.len()));
                entries.push((flags.trim().to_string(), description.trim().to_string()));
            }
        }
    }
    entries
}

/// The manual page, in roff.
///
/// Rendered from `OPTIONS` and [`KEYS`] rather than written out beside them,
/// for the reason `--help` is: there is one list of options and one list of
/// keys, and a second copy would be a second thing to keep in step. `--help`
/// and `image-view(1)` therefore cannot disagree.
pub fn man() -> String {
    let mut text = String::new();
    let version = env!("CARGO_PKG_VERSION");
    let upper = APP_ID.to_uppercase();
    let tagline = OPTIONS
        .lines()
        .next()
        .and_then(|line| line.split_once('\u{2014}'))
        .map(|(_, rest)| rest.trim())
        .unwrap_or("preview images");

    let _ = writeln!(
        text,
        r#".TH {} 1 "" "{} {}" "User Commands""#,
        roff(&upper),
        roff(APP_ID),
        roff(version)
    );
    let _ = writeln!(text, ".SH NAME\n{} \\- {}", roff(APP_ID), roff(tagline));

    let _ = writeln!(text, ".SH SYNOPSIS\n.B {}", roff(APP_ID));
    let _ = writeln!(text, r"[\fIOPTIONS\fR] \fIPATH\fR\&...");

    // The prose between the usage line and the options list, as its own
    // paragraphs; blank lines in the block are the paragraph breaks.
    let _ = writeln!(text, ".SH DESCRIPTION");
    let header = OPTIONS
        .split_once("\nOPTIONS:\n")
        .expect("OPTIONS has its heading")
        .0;
    let prose = header
        .split_once("<PATH>...\n")
        .map(|(_, rest)| rest)
        .unwrap_or("");
    for paragraph in prose.split("\n\n").filter(|p| !p.trim().is_empty()) {
        let _ = writeln!(text, ".PP\n{}", roff(paragraph.trim()));
    }

    let _ = writeln!(text, ".SH OPTIONS");
    for (flags, description) in option_entries() {
        let _ = writeln!(text, ".TP\n.B {}\n{}", roff(&flags), roff(&description));
    }

    for (section, heading) in SECTIONS {
        let _ = writeln!(text, ".SH {heading}");
        for binding in KEYS.iter().filter(|binding| binding.section == section) {
            let _ = writeln!(
                text,
                ".TP\n.B {}\n{}",
                roff(binding.shown),
                roff(binding.help)
            );
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
                    println!("{APP_ID} {}", env!("CARGO_PKG_VERSION"));
                    return Ok(None);
                }
                // Undocumented, like `--serve-clipboard`: this is how the
                // package build gets a manual page, not something to press.
                Some("--print-man") => {
                    print!("{}", man());
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
        for (_, heading) in SECTIONS {
            assert!(
                text.contains(&format!("{heading}:\n")),
                "{heading} should be a heading of its own"
            );
        }
    }

    fn packaging() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("packaging")
    }

    /// Every long option, as `--help` spells them.
    fn long_options() -> Vec<String> {
        option_entries()
            .iter()
            .flat_map(|(flags, _)| {
                flags
                    .split(',')
                    .map(|flag| {
                        flag.trim()
                            .split(' ')
                            .next()
                            .unwrap_or_default()
                            .to_string()
                    })
                    .filter(|flag| flag.starts_with("--") && flag.len() > 2)
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// The manual page is the help text in another notation, so it says the
    /// same things: every option and every key, once each.
    #[test]
    fn the_manual_page_lists_every_option_and_binding() {
        let page = man();
        for (flags, description) in option_entries() {
            assert!(
                page.contains(&roff(&flags)),
                "{flags:?} should reach the manual page"
            );
            assert!(
                page.contains(&roff(&description)),
                "the description of {flags:?} should reach the manual page"
            );
        }
        for binding in KEYS {
            assert!(
                page.contains(&roff(binding.help)),
                "{:?} should reach the manual page",
                binding.shown
            );
        }
        assert!(
            page.starts_with(".TH "),
            "a manual page opens with its title"
        );
    }

    /// Taking `OPTIONS` apart must not lose a line of it: every option line
    /// becomes an entry, and every continuation joins the entry above it.
    #[test]
    fn every_option_line_is_accounted_for() {
        let body = OPTIONS.split_once("\nOPTIONS:\n").expect("a heading").1;
        let lines = body.lines().filter(|line| !line.trim().is_empty()).count();
        let entries = option_entries();
        assert!(entries.len() >= 15, "every option should be found");
        assert!(
            entries.len() <= lines,
            "an entry cannot come from no line at all"
        );
        for (flags, description) in &entries {
            assert!(!flags.is_empty(), "an entry has a flag column");
            assert!(!description.is_empty(), "{flags:?} should say what it does");
            assert!(
                !description.starts_with(char::is_lowercase) || flags == "--",
                "{flags:?}: a description starts where the column does"
            );
        }
    }

    /// The completions offer what the program actually accepts. They are
    /// written by hand — there is no `clap` here to generate them — so this is
    /// what stops a new flag from reaching `--help` alone.
    #[test]
    fn the_completions_offer_every_long_option() {
        for file in [
            format!("{APP_ID}.bash"),
            format!("_{APP_ID}"),
            format!("{APP_ID}.fish"),
        ] {
            let path = packaging().join("completions").join(&file);
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            for option in long_options() {
                // fish spells a long option without its dashes.
                let spelt = match file.ends_with(".fish") {
                    true => format!("-l {}", option.trim_start_matches('-')),
                    false => option.clone(),
                };
                assert!(text.contains(&spelt), "{file} should offer {option}");
            }
        }
    }

    /// The desktop entry claims every format the decoders can actually read.
    /// A file manager offers this program for a picture because of this list,
    /// so a format added without touching it would silently not be offered.
    #[test]
    fn the_desktop_entry_claims_every_format() {
        // The extension a file carries, and the media type a file manager
        // knows it by. Taken from shared-mime-info, which is what decides
        // which application a desktop offers for a file.
        const TYPES: &[(&str, &str)] = &[
            ("jpg", "image/jpeg"),
            ("jpeg", "image/jpeg"),
            ("jpe", "image/jpeg"),
            ("jfif", "image/jpeg"),
            ("png", "image/png"),
            ("gif", "image/gif"),
            ("bmp", "image/bmp"),
            ("tif", "image/tiff"),
            ("tiff", "image/tiff"),
            ("webp", "image/webp"),
            ("avif", "image/avif"),
            ("heic", "image/heif"),
            ("heif", "image/heif"),
            ("hif", "image/heif"),
            ("ico", "image/vnd.microsoft.icon"),
            ("hdr", "image/vnd.radiance"),
            ("exr", "image/x-exr"),
            ("pnm", "image/x-portable-anymap"),
            ("pbm", "image/x-portable-bitmap"),
            ("pgm", "image/x-portable-graymap"),
            ("ppm", "image/x-portable-pixmap"),
            ("pam", "image/x-portable-arbitrarymap"),
        ];

        let path = packaging().join(format!("{APP_ID}.desktop"));
        let entry = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let claimed = entry
            .lines()
            .find_map(|line| line.strip_prefix("MimeType="))
            .expect("the desktop entry has a MimeType line");

        for extension in crate::image::decode::supported_extensions() {
            let media = TYPES
                .iter()
                .find(|(name, _)| *name == extension)
                .unwrap_or_else(|| panic!("{extension} has no media type in this table"))
                .1;
            assert!(
                claimed.contains(&format!("{media};")),
                ".{extension} is decoded but {media} is not claimed in the desktop entry"
            );
        }
    }

    /// One name, in one place.
    ///
    /// The Wayland `app_id`, the desktop entry, the icon and the Arch package
    /// all have to be the same word for a taskbar to pair a window with its
    /// icon and for a file manager to launch the right thing. That word is the
    /// crate's own name, so renaming the program is editing `Cargo.toml` and
    /// moving the files this test names — and forgetting one is a failure here
    /// rather than a wrong icon on somebody's desktop.
    #[test]
    fn everything_is_named_after_the_crate() {
        let entry = packaging().join(format!("{APP_ID}.desktop"));
        let icon = packaging().join(format!("{APP_ID}.svg"));
        let pkgbuild = packaging().join("PKGBUILD");
        for path in [&entry, &icon, &pkgbuild] {
            assert!(path.exists(), "{} is missing", path.display());
        }
        assert!(
            packaging()
                .join("completions")
                .join(format!("{APP_ID}.bash"))
                .exists()
                && packaging()
                    .join("completions")
                    .join(format!("_{APP_ID}"))
                    .exists()
                && packaging()
                    .join("completions")
                    .join(format!("{APP_ID}.fish"))
                    .exists(),
            "the completions are named after the crate too"
        );

        let text = std::fs::read_to_string(&entry).expect("the desktop entry reads");
        for field in ["Exec", "TryExec", "Icon", "StartupWMClass"] {
            let value = text
                .lines()
                .find_map(|line| line.strip_prefix(&format!("{field}=")))
                .unwrap_or_else(|| panic!("the desktop entry has no {field}"));
            assert!(
                value.split(' ').next() == Some(APP_ID),
                "{field}={value} should name {APP_ID}"
            );
        }

        let text = std::fs::read_to_string(&pkgbuild).expect("the PKGBUILD reads");
        assert!(
            text.contains(&format!("pkgname={APP_ID}")),
            "the PKGBUILD packages {APP_ID}"
        );
        assert!(
            text.contains(&format!("pkgver={}", env!("CARGO_PKG_VERSION"))),
            "the PKGBUILD is at the version Cargo.toml says"
        );

        assert!(
            OPTIONS.contains(APP_ID),
            "the help text still calls the program {APP_ID}"
        );
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
