//! The command line: what was asked for, and `--help`.
//!
//! The options are [`FLAGS`], one table that `--help`, the manual page and
//! the parse all read, so a flag is documented by the same edit that adds
//! it. The key sections of the help text are rendered from [`ROWS`] the same
//! way, and the mouse's from the default [`Gestures`]. Both at their
//! defaults: what a configuration file rebinds is the window's help popup's
//! to say.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::PathBuf;

use anyhow::{Result, anyhow, bail};

use crate::PROGRAM;
use crate::app::Options;
use crate::app::input::{MOUSE, ROWS, Section, key_column, mouse_rows};
use crate::app::keymap::Keymap;
use crate::gestures::Gestures;
use crate::image::decode::Overrides;
use crate::image::display::{AutoWindow, Colormap, Startup, ToneMap};
use crate::image::{Primaries, Transfer};
use crate::render::{HdrPreference, Upscale};
use crate::settings::Config;

/// What the program is, after its name at the top of `--help` and in the
/// manual page's NAME line.
const TAGLINE: &str = "preview images";

/// The paragraph between the usage lines and the options, wrapped for a
/// terminal; the manual page sets its own width, and takes it a paragraph
/// at a time.
const DESCRIPTION: &str = "\
The first file is shown, stretched to fit the window, and re-read whenever
something else writes to it. A directory stands for the images directly
inside it, in name order, and is read again as it changes: an image added
to it or taken out of it joins or leaves the list. --paste puts the image
on the clipboard at the front of the list, and needs no path at all. With
no path and no paste the window opens empty, offering to open files or a
folder through the desktop's file dialog, or to paste; Ctrl+O, or Cmd+O on
a Mac, opens that dialog from any window.";

/// Where the options stop, and what `--help` says of it: every argument
/// after it is a path, whatever it starts with. Not an option itself, so
/// read ahead of the table and listed after it.
const END_OF_OPTIONS: (&str, &str) = ("--", "Treat every later argument as a path");

/// What the command line has said so far, as each flag fills it in; what
/// [`parse_args`] makes the [`Options`] from once the configuration file is
/// read under it.
#[derive(Default)]
struct Parsed {
    files: Vec<PathBuf>,
    overrides: Overrides,
    startup: Startup,
    hdr: HdrPreference,
    /// The flags that say which panels are up are only half the answer: the
    /// configuration file is the other, and the flags win over it.
    histogram: Option<bool>,
    info: Option<bool>,
    minimap: Option<bool>,
    paused: bool,
    alone: bool,
    paste: bool,
    upscale: Upscale,
    size: Option<[u32; 2]>,
}

/// What a flag leaves the command line at once it has been read.
enum Then {
    /// The next argument.
    Continue,
    /// Nothing more: the flag printed what was asked for, and the program
    /// is done.
    Quit,
}

/// The arguments after a flag, for the flag to take its value from.
struct Rest<'a> {
    /// The flag, as the command line spelled it, for saying which one wanted
    /// a value it did not get.
    flag: &'static str,
    arguments: &'a mut dyn Iterator<Item = OsString>,
}

impl Rest<'_> {
    /// The next argument, taken as this flag's value; none is refused.
    fn value(&mut self) -> Result<String> {
        match self
            .arguments
            .next()
            .and_then(|value| value.into_string().ok())
        {
            Some(value) => Ok(value),
            None => bail!("`{}` needs a value (try --help)", self.flag),
        }
    }
}

/// One option of the command line: how it is spelled, what `--help` says
/// of it, and what it does.
struct Flag {
    long: &'static str,
    short: Option<&'static str>,
    /// The value it takes, as `--help` names it: `<SURFACE>`, or `<W> <H>`
    /// for two; empty for a flag that takes none.
    value: &'static str,
    /// What it does, on the lines `--help` shows it on — wrapped by hand for
    /// a terminal, which the manual page joins back up. Empty for a flag
    /// `--help` leaves out.
    help: &'static [&'static str],
    /// What it does to what was parsed, taking its value, where it has one,
    /// from the arguments after it.
    apply: fn(&mut Parsed, &mut Rest) -> Result<Then>,
}

impl Flag {
    /// The flag as the option column writes it: `-h, --help`,
    /// `--output <SURFACE>`.
    fn column(&self) -> String {
        let mut column = match self.short {
            Some(short) => format!("{short}, {}", self.long),
            None => self.long.to_string(),
        };
        if !self.value.is_empty() {
            column.push(' ');
            column.push_str(self.value);
        }
        column
    }
}

/// Every option, in the order `--help` lists them.
const FLAGS: &[Flag] = &[
    Flag {
        long: "--help",
        short: Some("-h"),
        value: "",
        help: &["Show this help"],
        apply: |_, _| {
            print!("{}", usage());
            Ok(Then::Quit)
        },
    },
    Flag {
        long: "--version",
        short: Some("-V"),
        value: "",
        help: &["Show the version"],
        apply: |_, _| {
            println!("{PROGRAM} {}", env!("CARGO_PKG_VERSION"));
            Ok(Then::Quit)
        },
    },
    Flag {
        long: "--print-config",
        short: None,
        value: "",
        help: &[
            "Print a configuration file, every setting at its",
            "default and commented out, to be saved as",
            "~/.config/gamut/config and edited",
        ],
        apply: |_, _| {
            print!("{}", Config::template());
            Ok(Then::Quit)
        },
    },
    // Undocumented, like `--serve-clipboard`: this is how the package build
    // gets a manual page, not something to press.
    Flag {
        long: "--print-man",
        short: None,
        value: "",
        help: &[],
        apply: |_, _| {
            print!("{}", man());
            Ok(Then::Quit)
        },
    },
    Flag {
        long: "--output",
        short: None,
        value: "<SURFACE>",
        help: &[
            "Start on an sdr or an hdr surface. Left alone, the",
            "surface follows the monitor: HDR where the",
            "compositor says it is in HDR mode. hdr asks for",
            "one regardless; o switches later",
        ],
        apply: |parsed, rest| {
            let value = rest.value()?;
            parsed.hdr = HdrPreference::parse(&value)
                .ok_or_else(|| anyhow!("unknown output `{value}`: sdr or hdr (try --help)"))?;
            Ok(Then::Continue)
        },
    },
    Flag {
        long: "--transfer",
        short: None,
        value: "<FN>",
        help: &[
            "Override the transfer function the file is assumed",
            "to use: linear, srgb, pq, hlg, or gamma:<N>",
        ],
        apply: |parsed, rest| {
            let value = rest.value()?;
            parsed.overrides.transfer =
                Some(Transfer::parse(&value).ok_or_else(
                    || match value.strip_prefix("gamma:") {
                        Some(exponent) => {
                            anyhow!("`--transfer gamma:` needs a number, got `{exponent}`")
                        }
                        None => anyhow!("unknown transfer function `{value}` (try --help)"),
                    },
                )?);
            Ok(Then::Continue)
        },
    },
    Flag {
        long: "--primaries",
        short: None,
        value: "<P>",
        help: &[
            "Override the color primaries: bt709, p3, bt2020,",
            "adobe, or prophoto",
        ],
        apply: |parsed, rest| {
            let value = rest.value()?;
            parsed.overrides.primaries = Some(
                Primaries::parse(&value)
                    .ok_or_else(|| anyhow!("unknown primaries `{value}` (try --help)"))?,
            );
            Ok(Then::Continue)
        },
    },
    Flag {
        long: "--no-gain-map",
        short: None,
        value: "",
        help: &[
            "Show the SDR base image of an Ultra HDR JPEG or",
            "a HEIC with a gain map, rather than reconstructing",
            "the HDR one from the map beside it",
        ],
        apply: |parsed, _| {
            parsed.overrides.gain_map = false;
            Ok(Then::Continue)
        },
    },
    Flag {
        long: "--colormap",
        short: None,
        value: "<MAP>",
        help: &[
            "Start with false color on single-channel images:",
            "gray, viridis, magma, or turbo",
        ],
        apply: |parsed, rest| {
            let value = rest.value()?;
            parsed.startup.colormap =
                Some(Colormap::parse(&value).ok_or_else(|| anyhow!("unknown colormap `{value}`"))?);
            Ok(Then::Continue)
        },
    },
    Flag {
        long: "--tone-map",
        short: None,
        value: "<MAP>",
        help: &["Start with none or neutral"],
        apply: |parsed, rest| {
            let value = rest.value()?;
            parsed.startup.tone_map =
                Some(ToneMap::parse(&value).ok_or_else(|| anyhow!("unknown tone map `{value}`"))?);
            Ok(Then::Continue)
        },
    },
    Flag {
        long: "--window",
        short: None,
        value: "<MODE>",
        help: &["Start with the window set to stored, full, or trimmed"],
        apply: |parsed, rest| {
            let value = rest.value()?;
            parsed.startup.auto = Some(
                AutoWindow::parse(&value)
                    .ok_or_else(|| anyhow!("unknown window mode `{value}` (try --help)"))?,
            );
            Ok(Then::Continue)
        },
    },
    Flag {
        long: "--exposure",
        short: None,
        value: "<STOPS>",
        help: &["Start at this exposure, in stops"],
        apply: |parsed, rest| {
            let value = rest.value()?;
            let stops: f32 = value
                .parse()
                .map_err(|_| anyhow!("`--exposure` needs a number, got `{value}`"))?;
            // `nan` and `inf` both parse as `f32`, and unclamped they would
            // put a non-finite gain into the shader uniform, the pixel
            // readout and the status bar. The keyboard path clamps to +/-16
            // stops; the command line gets the same ceiling, and rejects a
            // value that is not a number at all.
            if !stops.is_finite() {
                bail!("`--exposure` needs a finite number, got `{value}`");
            }
            parsed.startup.exposure_stops = Some(stops.clamp(-16.0, 16.0));
            Ok(Then::Continue)
        },
    },
    Flag {
        long: "--upscale",
        short: None,
        value: "<FILTER>",
        help: &["How to resample above 100%: nearest or bicubic"],
        apply: |parsed, rest| {
            let value = rest.value()?;
            parsed.upscale = Upscale::parse(&value)
                .ok_or_else(|| anyhow!("unknown upscale filter `{value}`"))?;
            Ok(Then::Continue)
        },
    },
    Flag {
        long: "--size",
        short: None,
        value: "<W> <H>",
        help: &[
            "Open the window at this size in logical pixels,",
            "rather than at the image's own",
        ],
        apply: |parsed, rest| {
            let width = rest.value()?;
            let height = rest.value()?;
            parsed.size = Some([side(&width)?, side(&height)?]);
            Ok(Then::Continue)
        },
    },
    Flag {
        long: "--histogram",
        short: None,
        value: "",
        help: &["Start with the histogram showing"],
        apply: |parsed, _| {
            parsed.histogram = Some(true);
            Ok(Then::Continue)
        },
    },
    Flag {
        long: "--info",
        short: None,
        value: "",
        help: &["Start with the file information panel showing"],
        apply: |parsed, _| {
            parsed.info = Some(true);
            Ok(Then::Continue)
        },
    },
    Flag {
        long: "--no-minimap",
        short: None,
        value: "",
        help: &["Start with the minimap off"],
        apply: |parsed, _| {
            parsed.minimap = Some(false);
            Ok(Then::Continue)
        },
    },
    Flag {
        long: "--alone",
        short: None,
        value: "",
        help: &[
            "Step only through the files named, even when that",
            "is a single file, rather than on through the",
            "other images in its folder",
        ],
        apply: |parsed, _| {
            parsed.alone = true;
            Ok(Then::Continue)
        },
    },
    Flag {
        long: "--paused",
        short: None,
        value: "",
        help: &[
            "Open an animation stopped on its first frame,",
            "rather than playing",
        ],
        apply: |parsed, _| {
            parsed.paused = true;
            Ok(Then::Continue)
        },
    },
    Flag {
        long: "--paste",
        short: None,
        value: "",
        help: &[
            "Paste the image on the clipboard, saved among your",
            "pictures, and show it first, ahead of any PATH",
        ],
        apply: |parsed, _| {
            parsed.paste = true;
            Ok(Then::Continue)
        },
    },
    Flag {
        long: "--timing",
        short: None,
        value: "",
        help: &["Print decode and startup timings to stderr"],
        apply: |_, _| {
            crate::timing::enable();
            Ok(Then::Continue)
        },
    },
];

/// The flags `--help` lists: every one it has words for.
fn documented() -> impl Iterator<Item = &'static Flag> {
    FLAGS.iter().filter(|flag| !flag.help.is_empty())
}

/// The heading a section's keys are listed under: its title in capitals,
/// as the option heading is, with `KEYS` after it. `--help`, the manual page
/// and the help popup all go by [`Section::ALL`], so none of the three can
/// grow a section the others do not have.
fn heading(section: Section) -> String {
    let title = match section {
        // The plural of the thing rather than the name of the section, the
        // way the others read.
        Section::Files => "FILE".to_string(),
        _ => section.title().to_uppercase(),
    };
    format!("{title} KEYS")
}

/// Every line of the key table under its heading, at the default chords,
/// and then the mouse's gestures under theirs: the key column and what it
/// does, as `--help` and the manual page both list them.
fn listed() -> Vec<(String, Vec<(String, &'static str)>)> {
    let keys = Keymap::default();
    let gestures = Gestures::default();
    Section::ALL
        .into_iter()
        .map(|section| {
            let lines = ROWS
                .iter()
                .filter(|row| row.section == section)
                .map(|row| (key_column(&keys, &gestures, row), row.help))
                .collect();
            (heading(section), lines)
        })
        .chain([(MOUSE.to_uppercase(), mouse_rows(&keys, &gestures))])
        .collect()
}

/// One line of `--help`'s key sections: the key column, and what it does
/// where the column ends — or a space after it, for a key column too long
/// to end there.
fn key_line(key: &str, help: &str) -> String {
    let gap = 19usize.saturating_sub(key.chars().count()).max(1);
    format!("    {key}{:gap$}{help}", "")
}

/// One line of the options block: `column` — a flag's, or nothing for a
/// line carrying on a description — set in from the left by `indent`, and
/// `words` where the description column begins.
fn option_line(indent: usize, column: &str, words: &str) -> String {
    let width = DESCRIPTION_COLUMN - indent;
    format!("{:indent$}{column:<width$}{words}", "")
}

/// The whole of `--help`: the usage lines and the description, the options
/// in two columns, then every key under its heading, then the mouse.
pub fn usage() -> String {
    let mut text = format!(
        "{PROGRAM} \u{2014} {TAGLINE}\n\
         \n\
         USAGE:\n    {PROGRAM} [OPTIONS] [PATH]...\n    {PROGRAM} --paste [OPTIONS] [PATH]...\n\
         \n\
         {DESCRIPTION}\n\
         \n\
         OPTIONS:\n"
    );
    for flag in documented() {
        // A flag with a short form starts at the margin; one without starts
        // where the long forms line up.
        let indent = if flag.short.is_some() { 4 } else { 8 };
        let Some((first, rest)) = flag.help.split_first() else {
            continue;
        };
        let _ = writeln!(text, "{}", option_line(indent, &flag.column(), first));
        for words in rest {
            let _ = writeln!(text, "{}", option_line(DESCRIPTION_COLUMN, "", words));
        }
    }
    let (end, words) = END_OF_OPTIONS;
    let _ = writeln!(text, "{}", option_line(4, end, words));
    for (heading, lines) in listed() {
        let _ = writeln!(text, "\n{heading}:");
        for (key, help) in lines {
            let _ = writeln!(text, "{}", key_line(&key, help));
        }
    }
    text
}

/// Where the description column of the options block begins. Both the
/// option lines and their continuations are laid out against it.
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

/// The manual page, in roff.
///
/// Rendered from [`FLAGS`] and [`ROWS`] rather than written out beside them,
/// for the reason `--help` is: there is one list of options and one list of
/// keys, and a second copy would be a second thing to keep in step. `--help`
/// and `gamut(1)` therefore cannot disagree.
pub fn man() -> String {
    let mut text = String::new();
    let version = env!("CARGO_PKG_VERSION");
    let upper = PROGRAM.to_uppercase();

    let _ = writeln!(
        text,
        r#".TH {} 1 "" "{} {}" "User Commands""#,
        roff(&upper),
        roff(PROGRAM),
        roff(version)
    );
    let _ = writeln!(text, ".SH NAME\n{} \\- {}", roff(PROGRAM), roff(TAGLINE));

    // Both usage lines, each its own line of the synopsis.
    let _ = writeln!(text, ".SH SYNOPSIS");
    let _ = writeln!(text, ".B {}", roff(PROGRAM));
    let _ = writeln!(text, r"[\fIOPTIONS\fR] [\fIPATH\fR]...");
    let _ = writeln!(text, ".br\n.B {} \\-\\-paste", roff(PROGRAM));
    let _ = writeln!(text, r"[\fIOPTIONS\fR] [\fIPATH\fR]...");

    // The description as its own paragraphs, the blank lines being the
    // breaks; the page sets its own width, so the terminal's wrapping is
    // taken out rather than carried across.
    let _ = writeln!(text, ".SH DESCRIPTION");
    for paragraph in DESCRIPTION.split("\n\n").filter(|p| !p.trim().is_empty()) {
        let _ = writeln!(text, ".PP\n{}", roff(paragraph.trim()));
    }

    let _ = writeln!(text, ".SH OPTIONS");
    for flag in documented() {
        let _ = writeln!(
            text,
            ".TP\n.B {}\n{}",
            roff(&flag.column()),
            roff(&flag.help.join(" "))
        );
    }
    let (end, words) = END_OF_OPTIONS;
    let _ = writeln!(text, ".TP\n.B {}\n{}", roff(end), roff(words));

    for (heading, lines) in listed() {
        let _ = writeln!(text, ".SH {heading}");
        for (key, help) in lines {
            let _ = writeln!(text, ".TP\n.B {}\n{}", roff(&key), roff(help));
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
                    crate::report(problem);
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
    /// Whether the image on the clipboard goes at the front of the list. The
    /// clipboard is not looked at here: what is on it is a word with the
    /// compositor, which `main` has when it puts the list together.
    pub paste: bool,
    /// The paths as they were given, before any directory among them was
    /// replaced by the images inside it. Kept so that the list can be built
    /// again, in the same order, when one of those directories changes.
    pub named: Vec<PathBuf>,
    /// What the window says about the configuration file, if some of it
    /// could not be used; the terminal has already been told.
    pub complaint: Option<String>,
    pub options: Options,
}

/// `Ok(None)` means we printed help or the version and should exit quietly.
pub fn parse_args() -> Result<Option<Args>> {
    let mut parsed = Parsed::default();
    let mut only_files = false;

    let mut arguments = std::env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        if !only_files && let Some(word) = argument.to_str() {
            if word == END_OF_OPTIONS.0 {
                only_files = true;
                continue;
            }
            if let Some(flag) = FLAGS
                .iter()
                .find(|flag| flag.long == word || flag.short == Some(word))
            {
                let mut rest = Rest {
                    flag: flag.long,
                    arguments: &mut arguments,
                };
                match (flag.apply)(&mut parsed, &mut rest)? {
                    Then::Continue => continue,
                    Then::Quit => return Ok(None),
                }
            }
            if word.starts_with('-') && word.len() > 1 {
                bail!("unknown option `{word}` (try --help)");
            }
        }
        parsed.files.push(PathBuf::from(argument));
    }
    let Parsed {
        files: named,
        overrides,
        startup,
        hdr,
        histogram,
        info,
        minimap,
        paused,
        alone,
        paste,
        upscale,
        size,
    } = parsed;

    // No path at all is a complete command line: the window opens on the
    // buttons that give it something. So is `--paste` alone, a paste being
    // a file to show; whether the clipboard actually holds one is found out
    // when it is asked for.
    let files = match crate::listing::expand(named.clone()) {
        Ok(files) => files,
        // Nothing to show among the paths is fatal only when they were all
        // there was: with `--paste`, whether there is anything to show is
        // the clipboard's to answer, and a directory with no images in it is
        // worth the word it would get beside a path that could be read.
        Err(error) if paste => {
            if !named.is_empty() {
                crate::report(&error);
            }
            Vec::new()
        }
        Err(error) => return Err(error),
    };
    let (mut config, complaint) = Config::load();
    config.show_histogram = histogram.unwrap_or(config.show_histogram);
    config.show_info = info.unwrap_or(config.show_info);
    config.show_minimap = minimap.unwrap_or(config.show_minimap);
    config.browse_folder &= !alone;
    Ok(Some(Args {
        files,
        paste,
        named,
        complaint,
        options: Options {
            overrides,
            startup,
            hdr,
            config,
            upscale,
            size,
            paused,
        },
    }))
}

/// One side of `--size`, in logical pixels.
///
/// Zero is refused rather than clamped: a window of no width is not a smaller
/// window but a mistyped one, and saying so is more use than opening something
/// the caller did not ask for. A size that is merely small is clamped instead,
/// where the window is opened.
fn side(value: &str) -> Result<u32> {
    match value.parse::<u32>() {
        Ok(pixels) if pixels > 0 => Ok(pixels),
        _ => bail!("`--size` needs two whole numbers of pixels, got `{value}`"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::APP_ID;

    /// Every key is in the help text, and nothing in the help text is a key
    /// that does not exist: the two come from one table. The mouse's
    /// gestures come after, under a heading of their own.
    #[test]
    fn the_help_text_lists_every_binding_once() {
        let text = usage();
        let keys = Keymap::default();
        // Once among the keys: a line a click runs is under the mouse too.
        let (key_sections, _) = text.split_once("\nMOUSE:\n").expect("a mouse section");
        for row in ROWS {
            let line = key_line(&key_column(&keys, &Gestures::default(), row), row.help);
            assert_eq!(
                key_sections.matches(&line).count(),
                1,
                "{line:?} should appear exactly once"
            );
        }
        let mouse = mouse_rows(&keys, &Gestures::default());
        assert!(!mouse.is_empty());
        for (gesture, does) in &mouse {
            let line = key_line(gesture, does);
            assert!(text.contains(&line), "{line:?} should appear");
        }
        assert!(text.contains("\nMOUSE:\n    Drag"), "{text}");
        for section in Section::ALL {
            let heading = heading(section);
            assert!(
                text.contains(&format!("{heading}:\n")),
                "{heading} should be a heading of its own"
            );
        }
        assert!(
            text.contains("\nFILE KEYS:\n"),
            "the file keys read as before"
        );
    }

    fn packaging() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("packaging")
    }

    /// Every long option `--help` lists.
    fn long_options() -> Vec<String> {
        documented().map(|flag| flag.long.to_string()).collect()
    }

    /// The manual page is the help text in another notation, so it says the
    /// same things: every option and every key, once each.
    #[test]
    fn the_manual_page_lists_every_option_and_binding() {
        let page = man();
        for flag in documented() {
            let column = flag.column();
            assert!(
                page.contains(&roff(&column)),
                "{column:?} should reach the manual page"
            );
            assert!(
                page.contains(&roff(&flag.help.join(" "))),
                "the description of {column:?} should reach the manual page"
            );
        }
        let (end, words) = END_OF_OPTIONS;
        assert!(page.contains(&format!(".B {}\n{words}\n", roff(end))));
        for row in ROWS {
            assert!(
                page.contains(&roff(row.help)),
                "{:?} should reach the manual page",
                row.help
            );
        }
        assert!(page.contains(".SH MOUSE\n"), "the gestures reach it too");
        assert!(
            page.starts_with(".TH "),
            "a manual page opens with its title"
        );
    }

    /// The options block is two columns: every flag's column stops short of
    /// the description column, each is spelled once, and each description
    /// starts where the column does, with a capital, on a line of its own.
    #[test]
    fn the_options_are_laid_out_in_two_columns() {
        let text = usage();
        let mut names: Vec<&str> = Vec::new();
        assert!(documented().count() >= 15, "every option should be listed");
        for flag in FLAGS {
            assert!(
                flag.long.starts_with("--") && flag.long.len() > 2,
                "{}",
                flag.long
            );
            for name in [Some(flag.long), flag.short].into_iter().flatten() {
                assert!(!names.contains(&name), "{name} twice");
                names.push(name);
            }
            let Some((first, rest)) = flag.help.split_first() else {
                assert!(!text.contains(flag.long), "{} is undocumented", flag.long);
                continue;
            };
            let indent = if flag.short.is_some() { 4 } else { 8 };
            assert!(
                indent + flag.column().len() + 2 <= DESCRIPTION_COLUMN,
                "{} reaches the description column",
                flag.long
            );
            assert!(
                !first.starts_with(char::is_lowercase),
                "{}: a description starts with a capital",
                flag.long
            );
            let line = option_line(indent, &flag.column(), first);
            assert_eq!(text.matches(&line).count(), 1, "{line:?}");
            for words in rest {
                let line = option_line(DESCRIPTION_COLUMN, "", words);
                assert!(text.contains(&format!("\n{line}\n")), "{line:?}");
            }
        }
        assert!(text.contains(&format!("\n{}\n\n", option_line(4, "--", END_OF_OPTIONS.1))));
    }

    /// The completions offer what the program actually accepts. They are
    /// written by hand — there is no `clap` here to generate them — so this is
    /// what stops a new flag from reaching `--help` alone.
    #[test]
    fn the_completions_offer_every_long_option() {
        for file in [
            format!("{PROGRAM}.bash"),
            format!("_{PROGRAM}"),
            format!("{PROGRAM}.fish"),
        ] {
            let path = packaging().join("completions").join(&file);
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            for option in long_options() {
                // fish spells a long option without its dashes.
                let spelled = match file.ends_with(".fish") {
                    true => format!("-l {}", option.trim_start_matches('-')),
                    false => option.clone(),
                };
                assert!(text.contains(&spelled), "{file} should offer {option}");
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
            ("jxl", "image/jxl"),
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
            ("dng", "image/x-adobe-dng"),
            ("nef", "image/x-nikon-nef"),
            ("nrw", "image/x-nikon-nrw"),
            ("cr2", "image/x-canon-cr2"),
            ("cr3", "image/x-canon-cr3"),
            ("crw", "image/x-canon-crw"),
            ("arw", "image/x-sony-arw"),
            ("srf", "image/x-sony-srf"),
            ("sr2", "image/x-sony-sr2"),
            ("raf", "image/x-fuji-raf"),
            ("orf", "image/x-olympus-orf"),
            ("rw2", "image/x-panasonic-rw2"),
            ("rwl", "image/x-panasonic-rw2"),
            ("pef", "image/x-pentax-pef"),
            ("srw", "image/x-samsung-srw"),
            ("3fr", "image/x-hasselblad-3fr"),
            ("fff", "image/x-hasselblad-fff"),
            ("iiq", "image/x-phaseone-iiq"),
            ("mef", "image/x-mamiya-mef"),
            ("mos", "image/x-leaf-mos"),
            ("erf", "image/x-epson-erf"),
            ("dcr", "image/x-kodak-dcr"),
            ("kdc", "image/x-kodak-kdc"),
            ("mrw", "image/x-minolta-mrw"),
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

    /// Two names, each in one place.
    ///
    /// The desktop entry and the icon are named for the Wayland `app_id`, and
    /// the entry says that name again in `Icon=` and `StartupWMClass=`: that
    /// is what a taskbar pairs a window with its icon by. The binary a user
    /// types, the Arch package and the completions are named for the program
    /// itself, which is what the entry launches in `Exec=`. Renaming either is
    /// editing the constant in `main.rs` and moving the files this test names —
    /// and forgetting one is a failure here rather than a wrong icon on
    /// somebody's desktop.
    #[test]
    fn everything_is_named_after_the_program_or_the_app_id() {
        let entry = packaging().join(format!("{APP_ID}.desktop"));
        let icon = packaging().join(format!("{APP_ID}.svg"));
        let pkgbuild = packaging().join("PKGBUILD");
        for path in [&entry, &icon, &pkgbuild] {
            assert!(path.exists(), "{} is missing", path.display());
        }
        assert!(
            packaging()
                .join("completions")
                .join(format!("{PROGRAM}.bash"))
                .exists()
                && packaging()
                    .join("completions")
                    .join(format!("_{PROGRAM}"))
                    .exists()
                && packaging()
                    .join("completions")
                    .join(format!("{PROGRAM}.fish"))
                    .exists(),
            "the completions are named after the program too"
        );

        let text = std::fs::read_to_string(&entry).expect("the desktop entry reads");
        for (field, name) in [
            ("Exec", PROGRAM),
            ("TryExec", PROGRAM),
            ("Icon", APP_ID),
            ("StartupWMClass", APP_ID),
        ] {
            let value = text
                .lines()
                .find_map(|line| line.strip_prefix(&format!("{field}=")))
                .unwrap_or_else(|| panic!("the desktop entry has no {field}"));
            assert!(
                value.split(' ').next() == Some(name),
                "{field}={value} should name {name}"
            );
        }

        let text = std::fs::read_to_string(&pkgbuild).expect("the PKGBUILD reads");
        assert!(
            text.contains(&format!("pkgname={PROGRAM}")),
            "the PKGBUILD packages {PROGRAM}"
        );
        assert!(
            text.contains(&format!("pkgver={}", env!("CARGO_PKG_VERSION"))),
            "the PKGBUILD is at the version Cargo.toml says"
        );
        assert!(
            text.contains(APP_ID),
            "the PKGBUILD installs the entry and the icon under {APP_ID}"
        );

        assert!(
            usage().starts_with(&format!("{PROGRAM} ")),
            "the help text still calls the program {PROGRAM}"
        );
    }

    /// The application bundle's `Info.plist` names the bundle for `APP_ID`,
    /// starts the program `PROGRAM`, and leaves the version to `bin/bundle`,
    /// which takes it from the binary so the two cannot differ.
    #[test]
    fn the_bundle_is_named_after_the_program_and_the_app_id() {
        let plist =
            std::fs::read_to_string(packaging().join("Info.plist")).expect("the Info.plist reads");
        for (key, value) in [
            ("CFBundleIdentifier", APP_ID),
            ("CFBundleExecutable", PROGRAM),
            ("CFBundleIconFile", PROGRAM),
        ] {
            assert!(
                plist.contains(&format!("<key>{key}</key>\n\t<string>{value}</string>")),
                "the Info.plist's {key} should be {value}"
            );
        }
        assert!(
            !plist.contains("CFBundleShortVersionString") && !plist.contains("CFBundleVersion"),
            "the version is stamped by bin/bundle, not kept in the tree"
        );
    }

    /// The bundle claims every format the decoders read, by the type macOS
    /// knows the extension as — so Finder offers this program for it — or,
    /// where macOS knows none, by a type the bundle declares for it itself.
    /// Asked of the system, so only on a Mac.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_bundle_claims_every_format() {
        use objc2_foundation::NSString;
        use objc2_uniform_type_identifiers::UTType;

        let plist =
            std::fs::read_to_string(packaging().join("Info.plist")).expect("the Info.plist reads");
        let (claimed, imported) = plist
            .split_once("<key>UTImportedTypeDeclarations</key>")
            .expect("the Info.plist declares the types macOS lacks");
        for extension in crate::image::decode::supported_extensions() {
            let kind = UTType::typeWithFilenameExtension(&NSString::from_str(extension))
                .expect("every extension has a type, if only a dynamic one");
            if kind.isDynamic() {
                assert!(
                    imported.contains(&format!("<string>{extension}</string>")),
                    ".{extension} has no type on this Mac and none is declared for it"
                );
            } else {
                let identifier = kind.identifier().to_string();
                assert!(
                    claimed.contains(&format!("<string>{identifier}</string>")),
                    ".{extension} is decoded but {identifier} is not claimed"
                );
            }
        }
    }

    /// The Homebrew formula builds the release the PKGBUILD does: the same
    /// tarball, at the version `Cargo.toml` says, under the same checksum.
    /// `bin/release` and `bin/pkgbuild-sha` move the two together, and this is
    /// what notices when one was edited by hand without the other.
    #[test]
    fn the_formula_and_the_pkgbuild_name_the_same_release() {
        let formula = std::fs::read_to_string(packaging().join(format!("{PROGRAM}.rb")))
            .expect("the formula reads");
        let pkgbuild =
            std::fs::read_to_string(packaging().join("PKGBUILD")).expect("the PKGBUILD reads");
        let version = env!("CARGO_PKG_VERSION");
        assert!(
            formula.contains(&format!("/archive/refs/tags/v{version}.tar.gz\"")),
            "the formula names the v{version} tarball"
        );
        let checksum = pkgbuild
            .lines()
            .find_map(|line| line.strip_prefix("sha256sums=('"))
            .and_then(|rest| rest.split('\'').next())
            .expect("the PKGBUILD has a checksum");
        assert!(
            formula.contains(&format!("sha256 \"{checksum}\"")),
            "the formula carries the PKGBUILD's checksum, {checksum}"
        );
        for completion in [
            format!("packaging/completions/{PROGRAM}.bash"),
            format!("packaging/completions/_{PROGRAM}"),
            format!("packaging/completions/{PROGRAM}.fish"),
        ] {
            assert!(
                formula.contains(&completion),
                "the formula installs {completion}"
            );
        }
    }
}
