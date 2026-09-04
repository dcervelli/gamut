//! Where a picture pasted from the clipboard is written, and what it is
//! called.
//!
//! A paste is a file and not just a picture on screen: it comes from
//! somewhere with no file of its own — a screenshot, a browser, an editor —
//! and a viewer that showed it without writing it would leave the user
//! nothing to go back to. So it is written where the desktop keeps the
//! pictures a person saves and browses, and joins the walk from there.
//!
//! Which directory that is comes from `xdg-user-dirs`, the freedesktop
//! convention every Linux desktop follows. The catch is that
//! `XDG_PICTURES_DIR` is not one of the variables a session exports — only
//! the base directories are — so it usually has to be read out of
//! `user-dirs.dirs`, a file of shell assignments, and there is a plain
//! fallback for when it says nothing.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result, bail};

use crate::clock;

/// How many names one second may hold. Reached only by pasting faster than
/// the clock ticks, which the name is otherwise unique by.
const NAMES_PER_SECOND: u32 = 100;

/// The directory a pasted picture is written to.
///
/// The environment first, for the session that does export it; then what the
/// user's own `user-dirs.dirs` says; then `~/Pictures`, which is what
/// `xdg-user-dirs` would have created. The directory need not exist yet —
/// pointing the variable at a directory nobody has made is ordinary — so it
/// is made before anything is written into it.
pub fn directory() -> Result<PathBuf> {
    if let Some(dir) = env_path("XDG_PICTURES_DIR") {
        return Ok(dir);
    }
    let home =
        env_path("HOME").context("HOME is unset, so there is nowhere to keep a pasted picture")?;
    Ok(configured(&home).unwrap_or_else(|| home.join("Pictures")))
}

/// Makes an empty file for a pasted picture and hands back its path.
///
/// The name is taken here rather than at the moment the bytes arrive, and
/// taken by creating the file: two pastes in the same second would otherwise
/// agree on a name, and the second would land on top of the first before it
/// had finished being written. Creating it is what settles which of them has
/// it, since the filesystem answers one of the two with `AlreadyExists`.
pub fn reserve(extension: &str) -> Result<PathBuf> {
    reserve_in(&directory()?, extension, SystemTime::now())
}

fn reserve_in(dir: &Path, extension: &str, now: SystemTime) -> Result<PathBuf> {
    fs::create_dir_all(dir).with_context(|| format!("making {}", crate::shown_path(dir)))?;
    let stamp = stamp(now);
    for attempt in 1..=NAMES_PER_SECOND {
        let path = dir.join(name(&stamp, attempt, extension));
        match fs::File::create_new(&path) {
            Ok(_) => return Ok(path),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("making {}", crate::shown_path(&path)));
            }
        }
    }
    bail!(
        "{} already holds {NAMES_PER_SECOND} pictures pasted this second",
        crate::shown_path(dir)
    )
}

/// The moment `time`, as the middle of a filename.
///
/// Local time, and written `2026-09-04_11-20-18` — the shape a desktop's
/// screenshots are named in, since a paste lands in the same directory and
/// is looked for the same way. The punctuation is what a filename takes
/// rather than what ISO 8601 asks for, a colon being awkward in a name and
/// forbidden outright on some filesystems.
fn stamp(time: SystemTime) -> String {
    let at = clock::local(time);
    format!(
        "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
        at.year, at.month, at.day, at.hour, at.minute, at.second
    )
}

/// What the file is called. The first name of a second is the plain one; the
/// rest are numbered, so an ordinary paste is not made to look like one of a
/// series.
fn name(stamp: &str, attempt: u32, extension: &str) -> String {
    match attempt {
        1 => format!("pasted_{stamp}.{extension}"),
        n => format!("pasted_{stamp}-{n}.{extension}"),
    }
}

/// A path from the environment, ignoring the variable that is set to nothing
/// — which is how a session says it has none rather than that the answer is
/// the root of the filesystem.
fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// What the user's `user-dirs.dirs` says pictures are kept in, if it says
/// anything this can read. Every way it can fail — no file, no key, a name
/// the filesystem allows and UTF-8 does not — means the same thing here, and
/// leaves the caller with the default.
fn configured(home: &Path) -> Option<PathBuf> {
    let config = env_path("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"));
    let text = fs::read_to_string(config.join("user-dirs.dirs")).ok()?;
    assignment(&text, "XDG_PICTURES_DIR", home)
}

/// The value `key` is given in a `user-dirs.dirs` file.
///
/// The file is shell, and is written by `xdg-user-dirs` in one shape:
/// assignments of double-quoted values, `$HOME` for the home directory, and
/// comments. That much is read here; a value put together out of anything
/// else a shell would accept is not, and leaves the caller with the default
/// rather than a path made of the wrong half of an expression. The last
/// assignment wins, as it would if the file were sourced.
fn assignment(text: &str, key: &str, home: &Path) -> Option<PathBuf> {
    let mut found = None;
    for line in text.lines() {
        let line = line.trim();
        let statement = line.strip_prefix("export ").unwrap_or(line).trim_start();
        let Some(value) = statement
            .strip_prefix(key)
            .and_then(|rest| rest.strip_prefix('='))
        else {
            continue;
        };
        found = expand(value, home);
    }
    found
}

/// One assignment's value as a path: unquoted, with the backslash escapes a
/// double-quoted shell string may carry taken off, and `$HOME` at the front
/// replaced by the home directory. A relative value is relative to home,
/// which is what the specification says of one.
fn expand(value: &str, home: &Path) -> Option<PathBuf> {
    let value = value.trim();
    let unquoted = match value.chars().next()? {
        quote @ ('"' | '\'') => value.strip_prefix(quote)?.strip_suffix(quote)?,
        _ => value,
    };
    let mut text = String::with_capacity(unquoted.len());
    let mut characters = unquoted.chars();
    while let Some(character) = characters.next() {
        match character {
            '\\' => text.push(characters.next()?),
            _ => text.push(character),
        }
    }

    let path = match text
        .strip_prefix("${HOME}")
        .or_else(|| text.strip_prefix("$HOME"))
    {
        Some(rest) => home.join(rest.trim_start_matches('/')),
        None if text.is_empty() => return None,
        None if text.starts_with('/') => PathBuf::from(text),
        // Anything else the shell would have expanded is not read here, and
        // a value that still holds a `$` is one of those.
        None if text.contains('$') => return None,
        None => home.join(text),
    };
    Some(path)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn home() -> PathBuf {
        PathBuf::from("/home/reader")
    }

    fn pictures(text: &str) -> Option<PathBuf> {
        assignment(text, "XDG_PICTURES_DIR", &home())
    }

    /// The file as `xdg-user-dirs` itself writes it.
    #[test]
    fn the_ordinary_file_reads() {
        let text = "\
# This file is written by xdg-user-dirs-update
XDG_DESKTOP_DIR=\"$HOME\"
XDG_DOWNLOAD_DIR=\"$HOME/Downloads\"
XDG_PICTURES_DIR=\"$HOME/Pictures\"
XDG_VIDEOS_DIR=\"$HOME/Videos\"
";
        assert_eq!(pictures(text), Some(home().join("Pictures")));
    }

    /// A desktop that keeps pictures somewhere else altogether, a value
    /// written the other ways a shell would take it, and the assignment that
    /// comes last winning as it would if the file were sourced.
    #[test]
    fn the_other_shapes_a_value_can_take() {
        assert_eq!(
            pictures("XDG_PICTURES_DIR=\"/mnt/photos\"\n"),
            Some(PathBuf::from("/mnt/photos"))
        );
        assert_eq!(
            pictures("XDG_PICTURES_DIR=\"${HOME}/Bilder\"\n"),
            Some(home().join("Bilder"))
        );
        assert_eq!(
            pictures("export XDG_PICTURES_DIR=\"$HOME/Pictures\"\n"),
            Some(home().join("Pictures"))
        );
        assert_eq!(
            pictures("XDG_PICTURES_DIR=\"$HOME/My \\\"Pictures\\\"\"\n"),
            Some(home().join("My \"Pictures\"")),
            "a quote inside the value is escaped, and is part of the name"
        );
        assert_eq!(
            pictures("XDG_PICTURES_DIR=\"$HOME/one\"\nXDG_PICTURES_DIR=\"$HOME/two\"\n"),
            Some(home().join("two"))
        );
    }

    /// Nothing to go on, and nothing pretending to be something: a key that
    /// is not there, one that only looks like it, and a value this does not
    /// undertake to read.
    #[test]
    fn a_file_that_says_nothing_usable_says_nothing() {
        assert_eq!(pictures("XDG_MUSIC_DIR=\"$HOME/Music\"\n"), None);
        assert_eq!(pictures("#XDG_PICTURES_DIR=\"$HOME/Pictures\"\n"), None);
        assert_eq!(pictures("MY_XDG_PICTURES_DIR=\"/elsewhere\"\n"), None);
        assert_eq!(pictures("XDG_PICTURES_DIR=\"\"\n"), None);
        assert_eq!(pictures("XDG_PICTURES_DIR=\"$PICTURES/here\"\n"), None);
    }

    /// Two pastes in the same second do not agree on a name, and neither of
    /// them lands on top of the other: the name is taken by making the file,
    /// which only one of them can do.
    #[test]
    fn a_second_paste_in_the_same_second_takes_a_name_of_its_own() {
        let dir = std::env::temp_dir().join(format!("gamut-paste-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_756_632_722);

        let first = reserve_in(&dir, "png", now).expect("a directory it can make");
        let second = reserve_in(&dir, "png", now).expect("a name of its own");
        assert_ne!(first, second);
        assert!(
            first.exists() && second.exists(),
            "each name is taken by taking it"
        );
        assert_eq!(
            second.file_name().and_then(|name| name.to_str()),
            Some(format!("pasted_{}-2.png", stamp(now)).as_str())
        );

        // And a paste a second later is back to a plain name.
        let later = reserve_in(&dir, "png", now + Duration::from_secs(1)).expect("another second");
        assert_eq!(
            later.file_name().and_then(|name| name.to_str()),
            Some(format!("pasted_{}.png", stamp(now + Duration::from_secs(1))).as_str())
        );
        fs::remove_dir_all(&dir).expect("cleaning up after the test");
    }

    /// The name is the moment it was pasted at, said the way a screenshot
    /// says it, and it is a name every filesystem will take.
    #[test]
    fn the_name_is_the_moment_it_was_pasted() {
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_756_632_722);
        let stamp = stamp(at);
        assert_eq!(name(&stamp, 1, "png"), format!("pasted_{stamp}.png"));
        assert_eq!(name(&stamp, 2, "jpg"), format!("pasted_{stamp}-2.jpg"));
        assert!(
            !stamp.contains(':'),
            "a colon is awkward in a name and refused outright on some filesystems"
        );

        // Under a zone this machine may or may not be in, so the moment is
        // checked against the same clock the name is written from.
        let at = clock::local(at);
        assert_eq!(
            stamp,
            format!(
                "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
                at.year, at.month, at.day, at.hour, at.minute, at.second
            )
        );
    }
}
