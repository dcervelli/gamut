//! Which files there are to show, and keeping that list up to date.
//!
//! A directory named on the command line is a place to look rather than a
//! fixed list: it stands for the images directly inside it, and it is read
//! again while the program runs, so that an image arriving in it or leaving it
//! joins or leaves the walk. [`expand`] is the reading start-up does, which
//! reports a directory it can make nothing of; [`relist`] is the same reading
//! done again with a window already open, where a directory that has emptied
//! or gone away is nothing to abandon the picture on screen for.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// The images directly inside `dir`, in name order. One level deep: a
/// directory names a place to look, not a tree to walk.
///
/// The extension decides here, since the alternative is opening every file in
/// the directory to look at its leading bytes. A file named on the command
/// line is still read for what it holds rather than what it is called.
fn images_in(dir: &Path) -> Result<Vec<PathBuf>> {
    let extensions = crate::image::decode::supported_extensions();
    let mut found = Vec::new();
    let entries = std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
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
    found.sort();
    Ok(found)
}

/// Replaces every directory named on the command line with the images
/// directly inside it. Fails only when that leaves nothing at all to show,
/// which is a command line worth answering rather than an empty window.
pub fn expand(named: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut empty = Vec::new();
    for path in named {
        if !path.is_dir() {
            files.push(path);
            continue;
        }
        let mut found = images_in(&path)?;
        if found.is_empty() {
            empty.push(path);
            continue;
        }
        files.append(&mut found);
    }

    if !files.is_empty() {
        // Worth mentioning only once we know we are carrying on without them,
        // as with a file whose header will not read.
        for path in empty {
            eprintln!("gamut: no images in {}", crate::shown_path(&path));
        }
        return Ok(files);
    }
    let names: Vec<String> = empty
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    bail!("no images in {}", names.join(", "))
}

/// The list as it stands now, built from the same names in the same order.
///
/// Silent where [`expand`] speaks up: a directory that has emptied or that
/// will not read any more contributes nothing and is not mentioned. There is
/// a picture on screen by this point, the list is never allowed to lose it
/// (see `Files::relist`), and a directory being written to passes through
/// states not worth a word.
pub fn relist(named: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for path in named {
        if path.is_dir() {
            files.append(&mut images_in(path).unwrap_or_default());
        } else {
            files.push(path.clone());
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_images")
    }

    /// A directory stands for the images in it, in name order, and for
    /// nothing else: the color profile, the shell script and the README
    /// lying beside them are not files to show.
    #[test]
    fn a_directory_becomes_the_images_inside_it() {
        let files = expand(vec![fixtures()]).expect("test_images/ holds images");
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
        assert_eq!(expand(named.clone()).expect("names to pass through"), named);
    }

    /// A directory with nothing to show in it is an error worth naming,
    /// rather than an empty list that opens a window onto nothing.
    #[test]
    fn a_directory_holding_no_images_is_reported() {
        let empty = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
        let error = expand(vec![empty.clone()]).expect_err("src/ holds no images");
        assert!(error.to_string().contains(&empty.display().to_string()));
    }

    /// The running program's reading of the same names: the same list, and no
    /// complaint about the directory that start-up would have called empty.
    #[test]
    fn relisting_reads_the_same_names_again_without_complaint() {
        let named = vec![fixtures()];
        assert_eq!(relist(&named), expand(named.clone()).expect("images"));

        let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
        assert!(
            relist(&[src]).is_empty(),
            "a directory with no images in it"
        );
    }
}
