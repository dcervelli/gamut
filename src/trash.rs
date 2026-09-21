//! The desktop's trash: moving a file into it, and moving one back out.
//!
//! The freedesktop.org Trash specification, followed by hand rather than
//! through a crate for the same reason the thumbnail cache is: it is a
//! directory layout and a small text file, and what this program needs of
//! it — put one file in, take that same file out — is a page of code that
//! would otherwise arrive with a date library under it. What matters is that
//! it is the *desktop's* trash. A file moved here shows up in the file
//! manager's Trash beside everything else the user has thrown away, can be
//! restored or emptied from there, and is under a retention policy this
//! program does not have to invent.
//!
//! The layout: a trash directory holds `files/`, where the things thrown
//! away go under a name unique within it, and `info/`, where each has a
//! `<name>.trashinfo` beside it saying where it came from and when. The home
//! trash is `$XDG_DATA_HOME/Trash`; a file on another filesystem goes to a
//! `.Trash-<uid>` at the top of its own mount, so that it is a rename rather
//! than a copy, and only where that cannot be made is it copied into the
//! home trash instead — the two things the specification allows.
//!
//! An [`Entry`] is the one thing this file hands back: exactly which file in
//! exactly which trash, so that [`restore`] puts back the file that was
//! thrown away and not whichever file of the same name went in last.

use std::fs;
use std::io::{self, ErrorKind, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result, anyhow};

use crate::clock;

/// How many names one file may try in a trash before giving up: reached only
/// by a trash already holding that many files of the same name.
const NAMES: u32 = 10_000;

/// Where files thrown away go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trash {
    /// The home trash directory, which holds `files/` and `info/`.
    home: PathBuf,
}

/// One file in a trash, as it was put there: enough to take it out again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The file itself, under `files/`.
    pub held: PathBuf,
    /// Its `.trashinfo`, under `info/`.
    pub info: PathBuf,
    /// Where it came from, absolute: where a restore puts it back.
    pub original: PathBuf,
}

/// Why a restore did not happen.
#[derive(Debug)]
pub enum Refused {
    /// The trash no longer holds the file: it was emptied, or restored
    /// from elsewhere.
    Gone,
    /// Something else now stands where the file came from, and a restore
    /// does not overwrite.
    Taken,
    Failed(anyhow::Error),
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::Gone => write!(f, "the trash no longer holds it"),
            Refused::Taken => write!(f, "something else is there now"),
            Refused::Failed(error) => write!(f, "{error:#}"),
        }
    }
}

impl Trash {
    /// The user's home trash, where the specification puts it: under
    /// `$XDG_DATA_HOME`, or `~/.local/share`. `None` where neither is
    /// known, in which case there is no trash to move anything to.
    pub fn detect() -> Option<Self> {
        let env_path = |name: &str| {
            std::env::var_os(name)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        let data = env_path("XDG_DATA_HOME")
            .or_else(|| Some(env_path("HOME")?.join(".local").join("share")))?;
        Some(Self::under(data.join("Trash")))
    }

    /// A trash whose home directory is `home`, for the tests.
    pub fn under(home: PathBuf) -> Self {
        Self { home }
    }

    /// Moves the file at `path` into the trash, and says exactly where it
    /// went.
    ///
    /// The info file is written first and exclusively, which is what makes
    /// the name the file goes under unique: two programs throwing away
    /// files of one name at once each get their own. Then the file is
    /// renamed under it without replacing anything — a stale file left in
    /// `files/` by a broken trash is not overwritten either — and the name
    /// is tried again with a number in it if either half finds it taken.
    pub fn put(&self, path: &Path) -> Result<Entry> {
        let original = std::path::absolute(path)
            .with_context(|| format!("locating {}", crate::shown_path(path)))?;
        // The file has to be there to be thrown away; asked up front so
        // that the answer is the plain one rather than a failed rename.
        fs::symlink_metadata(&original)
            .with_context(|| format!("reading {}", crate::shown_path(&original)))?;
        let Place {
            dir,
            path_key,
            copy,
        } = self.place_for(&original)?;
        let files = dir.join("files");
        let info = dir.join("info");
        fs::create_dir_all(&files).with_context(|| format!("making {}", files.display()))?;
        fs::create_dir_all(&info).with_context(|| format!("making {}", info.display()))?;

        let name = original
            .file_name()
            .ok_or_else(|| anyhow!("{} has no name to keep", crate::shown_path(&original)))?;
        let (stem, extension) = split_name(name.as_bytes());
        for attempt in 1..=NAMES {
            let mut in_trash = stem.to_vec();
            if attempt > 1 {
                in_trash.extend_from_slice(format!(".{attempt}").as_bytes());
            }
            in_trash.extend_from_slice(extension);
            let in_trash = std::ffi::OsStr::from_bytes(&in_trash);
            let mut info_name = in_trash.to_os_string();
            info_name.push(".trashinfo");
            let info_path = info.join(&info_name);
            let mut file = match fs::File::create_new(&info_path) {
                Ok(file) => file,
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(error).with_context(|| format!("making {}", info_path.display()));
                }
            };
            let written = write!(
                file,
                "[Trash Info]\nPath={}\nDeletionDate={}\n",
                percent_encoded(&path_key),
                deletion_date(SystemTime::now())
            )
            .and_then(|_| file.sync_all());
            if let Err(error) = written {
                let _ = fs::remove_file(&info_path);
                return Err(error).with_context(|| format!("writing {}", info_path.display()));
            }
            let held = files.join(in_trash);
            let moved = if copy {
                copy_over(&original, &held)
            } else {
                rename_no_replace(&original, &held)
            };
            match moved {
                Ok(()) => {
                    return Ok(Entry {
                        held,
                        info: info_path,
                        original,
                    });
                }
                Err(error) => {
                    let _ = fs::remove_file(&info_path);
                    if error.kind() == ErrorKind::AlreadyExists {
                        continue;
                    }
                    return Err(error).with_context(|| {
                        format!(
                            "moving {} to {}",
                            crate::shown_path(&original),
                            held.display()
                        )
                    });
                }
            }
        }
        Err(anyhow!(
            "the trash already holds {NAMES} files called {}",
            crate::shown_path(Path::new(name))
        ))
    }

    /// Which trash `original` goes to, and how.
    ///
    /// The home trash when the file is on the same filesystem as it, since
    /// then the move is a rename. Otherwise the trash at the top of the
    /// file's own mount — `.Trash/<uid>` inside a sticky `.Trash` an
    /// administrator made, or a `.Trash-<uid>` of the user's own — made if
    /// it is not there. Where that cannot be made, the home trash after
    /// all, by copying.
    fn place_for(&self, original: &Path) -> Result<Place> {
        let device = fs::symlink_metadata(original)?.dev();
        let home_device = existing_ancestor(&self.home).map(|dir| dir.dev());
        if home_device == Some(device) {
            return Ok(Place {
                dir: self.home.clone(),
                path_key: original.to_path_buf(),
                copy: false,
            });
        }
        let top = mount_top(original, device);
        if let Some(dir) = mounted_trash(&top) {
            // Relative to the top of the mount, as the specification asks
            // of a trash that travels with its volume.
            let path_key = original
                .strip_prefix(&top)
                .map_or_else(|_| original.to_path_buf(), Path::to_path_buf);
            return Ok(Place {
                dir,
                path_key,
                copy: false,
            });
        }
        Ok(Place {
            dir: self.home.clone(),
            path_key: original.to_path_buf(),
            copy: true,
        })
    }
}

/// Where a file is going: the trash directory, what its `Path=` says, and
/// whether it has to be copied rather than renamed to get there.
struct Place {
    dir: PathBuf,
    path_key: PathBuf,
    copy: bool,
}

/// Puts `entry` back where it came from.
///
/// Without replacing anything: a file that has since been made under the
/// original name is not the user's to lose, and the entry stays in the
/// trash for the file manager to sort out. The info file goes last, so a
/// restore that fails half way leaves an entry the trash still lists.
pub fn restore(entry: &Entry) -> Result<(), Refused> {
    if fs::symlink_metadata(&entry.held).is_err() {
        return Err(Refused::Gone);
    }
    // The directory it came from may itself have gone in the meantime.
    if let Some(parent) = entry.original.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        return Err(Refused::Failed(
            anyhow!(error).context(format!("making {}", crate::shown_path(parent))),
        ));
    }
    let moved = if same_device(&entry.held, &entry.original) {
        rename_no_replace(&entry.held, &entry.original)
    } else {
        copy_over(&entry.held, &entry.original)
    };
    match moved {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::AlreadyExists => return Err(Refused::Taken),
        Err(error) => {
            return Err(Refused::Failed(anyhow!(error).context(format!(
                "moving {} back to {}",
                entry.held.display(),
                crate::shown_path(&entry.original)
            ))));
        }
    }
    match fs::remove_file(&entry.info) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Refused::Failed(
            anyhow!(error).context(format!("removing {}", entry.info.display())),
        )),
    }
}

/// Renames `from` to `to`, refusing with `AlreadyExists` if anything is at
/// `to` — atomically, through `renameat2`, where the filesystem can, and by
/// looking first where it cannot. What a rename of the file on screen goes
/// through as well, for the same refusal.
pub(crate) fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    use std::ffi::CString;
    let c = |path: &Path| CString::new(path.as_os_str().as_bytes()).map_err(io::Error::other);
    let (from_c, to_c) = (c(from)?, c(to)?);
    // SAFETY: two valid NUL-terminated paths, and flags the kernel defines.
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from_c.as_ptr(),
            libc::AT_FDCWD,
            to_c.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        // A filesystem that does not know the flag: check, then rename.
        Some(libc::EINVAL | libc::ENOSYS | libc::ENOTSUP) => {
            if fs::symlink_metadata(to).is_ok() {
                return Err(io::Error::from(ErrorKind::AlreadyExists));
            }
            fs::rename(from, to)
        }
        _ => Err(error),
    }
}

/// Moves `from` to `to` across filesystems: a copy, then the original
/// removed — and the copy removed instead if the original cannot be, so
/// that the file is in one place afterwards whichever way it went.
fn copy_over(from: &Path, to: &Path) -> io::Result<()> {
    let mut target = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)?;
    let copied = fs::File::open(from).and_then(|mut source| io::copy(&mut source, &mut target));
    if let Err(error) = copied.and_then(|_| target.sync_all()) {
        let _ = fs::remove_file(to);
        return Err(error);
    }
    if let Err(error) = fs::remove_file(from) {
        let _ = fs::remove_file(to);
        return Err(error);
    }
    Ok(())
}

fn same_device(a: &Path, b: &Path) -> bool {
    let device = |path: &Path| {
        fs::symlink_metadata(path)
            .ok()
            .or_else(|| existing_ancestor(path))
            .map(|metadata| metadata.dev())
    };
    device(a).is_some() && device(a) == device(b)
}

/// The metadata of the nearest ancestor of `path` that exists, `path`
/// itself included: what says which filesystem a path not yet made is on.
fn existing_ancestor(path: &Path) -> Option<fs::Metadata> {
    path.ancestors()
        .find_map(|ancestor| fs::symlink_metadata(ancestor).ok())
}

/// The top of the mount `path` is on: the highest ancestor still on
/// `device`.
fn mount_top(path: &Path, device: u64) -> PathBuf {
    let mut top = path.to_path_buf();
    for ancestor in path.ancestors().skip(1) {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.dev() == device => top = ancestor.to_path_buf(),
            _ => break,
        }
    }
    top
}

/// The trash directory at the top of a mount, made if need be: `.Trash/<uid>`
/// inside a `.Trash` that an administrator made sticky, or else the user's
/// own `.Trash-<uid>`. `None` where neither can be had.
fn mounted_trash(top: &Path) -> Option<PathBuf> {
    // SAFETY: getuid cannot fail and takes nothing.
    let uid = unsafe { libc::getuid() };
    let shared = top.join(".Trash");
    if let Ok(metadata) = fs::symlink_metadata(&shared)
        && metadata.is_dir()
        && metadata.mode() & 0o1000 != 0
    {
        let mine = shared.join(uid.to_string());
        if fs::create_dir_all(&mine).is_ok() {
            return Some(mine);
        }
    }
    let mine = top.join(format!(".Trash-{uid}"));
    fs::create_dir_all(&mine).ok()?;
    Some(mine)
}

/// A name cut before its extension, so that a number can go between: the
/// file keeps the extension a file manager reads its kind from. A name
/// with no extension, or a dot-file's, is all stem.
fn split_name(name: &[u8]) -> (&[u8], &[u8]) {
    match name.iter().rposition(|&byte| byte == b'.') {
        Some(dot) if dot > 0 => name.split_at(dot),
        _ => (name, &[]),
    }
}

/// `path` as the `Path=` line writes it: every byte outside the unreserved
/// set percent-encoded, the separator apart, as a URI's path is.
fn percent_encoded(path: &Path) -> String {
    let mut encoded = String::new();
    for &byte in path.as_os_str().as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                encoded.push(char::from(byte));
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

/// The moment `now` as the `DeletionDate=` line writes it: local time, in
/// the shape the specification gives.
fn deletion_date(now: SystemTime) -> String {
    let at = clock::local(now);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        at.year, at.month, at.day, at.hour, at.minute, at.second
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory of the test's own, with a trash under it, so that the
    /// tests running alongside each other cannot tread on each other's
    /// files.
    fn sandbox(name: &str) -> (PathBuf, Trash) {
        let dir = std::env::temp_dir().join(format!("gamut-trash-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("the temporary directory is writable");
        let trash = Trash::under(dir.join("Trash"));
        (dir, trash)
    }

    #[test]
    fn a_file_put_in_the_trash_is_listed_there_and_comes_back() {
        let (dir, trash) = sandbox("round-trip");
        let path = dir.join("sunset.jpg");
        fs::write(&path, b"pixels").expect("writable");

        let entry = trash.put(&path).expect("the trash takes it");
        assert!(!path.exists(), "the file has gone from where it was");
        assert_eq!(entry.original, path);
        assert_eq!(entry.held, dir.join("Trash/files/sunset.jpg"));
        assert_eq!(entry.info, dir.join("Trash/info/sunset.jpg.trashinfo"));
        assert_eq!(fs::read(&entry.held).expect("held"), b"pixels");
        let info = fs::read_to_string(&entry.info).expect("written");
        let mut lines = info.lines();
        assert_eq!(lines.next(), Some("[Trash Info]"));
        assert_eq!(
            lines.next(),
            Some(format!("Path={}", percent_encoded(&path)).as_str())
        );
        let date = lines.next().expect("a date");
        assert!(
            date.starts_with("DeletionDate=")
                && date.len() == "DeletionDate=2026-09-21T12:00:00".len(),
            "{date}"
        );

        restore(&entry).expect("nothing is in the way");
        assert_eq!(fs::read(&path).expect("back"), b"pixels");
        assert!(!entry.held.exists());
        assert!(!entry.info.exists());

        fs::remove_dir_all(dir).expect("cleanup");
    }

    /// Two files of one name are two entries, the second numbered before
    /// its extension, and each restores to its own place.
    #[test]
    fn a_second_file_of_the_same_name_gets_a_number() {
        let (dir, trash) = sandbox("twins");
        let one = dir.join("a").join("photo.png");
        let two = dir.join("b").join("photo.png");
        fs::create_dir_all(one.parent().expect("a")).expect("writable");
        fs::create_dir_all(two.parent().expect("b")).expect("writable");
        fs::write(&one, b"one").expect("writable");
        fs::write(&two, b"two").expect("writable");

        let first = trash.put(&one).expect("taken");
        let second = trash.put(&two).expect("taken too");
        assert_eq!(first.held, dir.join("Trash/files/photo.png"));
        assert_eq!(second.held, dir.join("Trash/files/photo.2.png"));
        assert_eq!(second.info, dir.join("Trash/info/photo.2.png.trashinfo"));

        restore(&second).expect("its own place is free");
        assert_eq!(fs::read(&two).expect("back"), b"two");
        assert!(first.held.exists(), "the other is still in the trash");
        restore(&first).expect("and comes back on its own");
        assert_eq!(fs::read(&one).expect("back"), b"one");

        fs::remove_dir_all(dir).expect("cleanup");
    }

    /// A restore does not overwrite: with something new at the original
    /// path the entry stays in the trash, and once the trash has been
    /// emptied there is nothing to restore.
    #[test]
    fn a_restore_refuses_to_replace_and_notices_an_emptied_trash() {
        let (dir, trash) = sandbox("refused");
        let path = dir.join("scan.tif");
        fs::write(&path, b"old").expect("writable");
        let entry = trash.put(&path).expect("taken");

        fs::write(&path, b"new").expect("writable");
        assert!(matches!(restore(&entry), Err(Refused::Taken)));
        assert_eq!(fs::read(&path).expect("untouched"), b"new");
        assert!(entry.held.exists() && entry.info.exists());

        fs::remove_file(&path).expect("writable");
        fs::remove_file(&entry.held).expect("emptied");
        fs::remove_file(&entry.info).expect("emptied");
        assert!(matches!(restore(&entry), Err(Refused::Gone)));

        fs::remove_dir_all(dir).expect("cleanup");
    }

    /// A file that is not there cannot be thrown away, and says so plainly.
    #[test]
    fn a_missing_file_is_refused() {
        let (dir, trash) = sandbox("missing");
        let error = trash.put(&dir.join("nothing.png")).expect_err("not there");
        assert!(format!("{error:#}").contains("nothing.png"), "{error:#}");
        assert!(
            !dir.join("Trash").exists(),
            "nothing was made for a file that was not moved"
        );
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn the_path_is_written_as_a_uri_path() {
        assert_eq!(
            percent_encoded(Path::new("/home/me/my photos/été.jpg")),
            "/home/me/my%20photos/%C3%A9t%C3%A9.jpg"
        );
        assert_eq!(split_name(b"photo.png"), (&b"photo"[..], &b".png"[..]));
        assert_eq!(
            split_name(b"archive.tar.gz"),
            (&b"archive.tar"[..], &b".gz"[..])
        );
        assert_eq!(split_name(b"README"), (&b"README"[..], &b""[..]));
        assert_eq!(split_name(b".hidden"), (&b".hidden"[..], &b""[..]));
    }

    #[test]
    fn the_date_is_written_as_the_specification_gives_it() {
        let date = deletion_date(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(86_400));
        assert_eq!(date.len(), "1970-01-02T00:00:00".len());
        assert!(date.starts_with("1970-01-0"), "{date}");
        assert_eq!(&date[10..11], "T");
    }
}
