//! Renaming a file without replacing whatever is already at the new name.
//!
//! `std::fs::rename` replaces its target, which is never what this program
//! wants: a rename of the file on screen, an export's temporary put in place,
//! a file put in the trash or taken back out of it must each refuse rather
//! than overwrite a file the user had. How a rename is told to refuse is the
//! kernel's business, which is why this is a file of its own.

use std::ffi::CString;
use std::fs;
use std::io::{self, ErrorKind};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// Renames `from` to `to`, refusing with `AlreadyExists` if anything is at
/// `to` — atomically, through `renameat2`, where the filesystem can, and by
/// looking first where it cannot. What a rename of the file on screen goes
/// through as well, for the same refusal.
pub fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rename_refuses_to_replace_what_is_there() {
        let dir = std::env::temp_dir().join(format!("gamut-rename-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let (from, to) = (dir.join("from"), dir.join("to"));
        fs::write(&from, b"moved").unwrap();
        fs::write(&to, b"kept").unwrap();
        let refused = rename_no_replace(&from, &to).unwrap_err();
        assert_eq!(refused.kind(), ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&to).unwrap(), b"kept");
        fs::remove_file(&to).unwrap();
        rename_no_replace(&from, &to).unwrap();
        assert_eq!(fs::read(&to).unwrap(), b"moved");
        assert!(!from.exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
