//! The base directory specification's answers, read once for everything
//! that keeps something under the user's home: where the home is, and the
//! directories a session keeps its cache, its configuration and its data
//! in — each from the variable named for it, and otherwise from home.
//!
//! A variable set to nothing is a variable not set, which is how a session
//! says it has none rather than that the answer is the root of the
//! filesystem; and a relative path is ignored, as the specification asks,
//! since it would otherwise be resolved against whatever directory this
//! program happened to be started in.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

/// The absolute path in the environment variable `name`, or `None` where
/// it is unset, empty or relative.
pub fn env_path(name: &str) -> Option<PathBuf> {
    absolute(std::env::var_os(name)?)
}

/// The user's home directory.
pub fn home() -> Option<PathBuf> {
    env_path("HOME")
}

/// `$XDG_CACHE_HOME`, or `~/.cache`.
pub fn cache_home() -> Option<PathBuf> {
    env_path("XDG_CACHE_HOME").or_else(|| Some(home()?.join(".cache")))
}

/// `$XDG_CONFIG_HOME`, or `~/.config`.
pub fn config_home() -> Option<PathBuf> {
    env_path("XDG_CONFIG_HOME").or_else(|| Some(home()?.join(".config")))
}

/// `$XDG_DATA_HOME`, or `~/.local/share`.
pub fn data_home() -> Option<PathBuf> {
    env_path("XDG_DATA_HOME").or_else(|| Some(home()?.join(".local/share")))
}

/// The configuration directories in the order a name found in two of them
/// is taken from: the user's own, then `$XDG_CONFIG_DIRS` or `/etc/xdg`.
pub fn config_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = config_home().into_iter().collect();
    match std::env::var_os("XDG_CONFIG_DIRS") {
        Some(value) if !value.is_empty() => dirs.extend(split_dirs(&value)),
        _ => dirs.push(PathBuf::from("/etc/xdg")),
    }
    dirs
}

/// The data directories, the same way: the user's own, then
/// `$XDG_DATA_DIRS` or `/usr/local/share` and `/usr/share`.
pub fn data_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = data_home().into_iter().collect();
    match std::env::var_os("XDG_DATA_DIRS") {
        Some(value) if !value.is_empty() => dirs.extend(split_dirs(&value)),
        _ => dirs.extend([
            PathBuf::from("/usr/local/share"),
            PathBuf::from("/usr/share"),
        ]),
    }
    dirs
}

/// `value` as a path, if it is one worth having: not empty, and absolute.
fn absolute(value: OsString) -> Option<PathBuf> {
    if value.is_empty() {
        return None;
    }
    let path = PathBuf::from(value);
    path.is_absolute().then_some(path)
}

/// A colon-separated search path, as the specification writes one, with
/// the relative entries dropped.
fn split_dirs(value: &OsStr) -> Vec<PathBuf> {
    std::env::split_paths(value)
        .filter(|path| path.is_absolute())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A variable set to nothing, or to a relative path, is one not set.
    #[test]
    fn a_variable_counts_only_when_it_names_an_absolute_path() {
        assert_eq!(absolute(OsString::from("")), None);
        assert_eq!(absolute(OsString::from("cache")), None);
        assert_eq!(absolute(OsString::from("./cache")), None);
        assert_eq!(
            absolute(OsString::from("/home/me/.cache")),
            Some(PathBuf::from("/home/me/.cache"))
        );
    }

    /// A search path is split at the colons, and the relative entries in
    /// it are dropped rather than resolved against the working directory.
    #[test]
    fn a_search_path_keeps_its_absolute_entries_in_order() {
        assert_eq!(
            split_dirs(OsStr::new("/usr/local/share:relative:/usr/share:")),
            [
                PathBuf::from("/usr/local/share"),
                PathBuf::from("/usr/share")
            ]
        );
    }
}
