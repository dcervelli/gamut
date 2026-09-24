//! The order the file list stands in: the permutation an [`Order`] makes
//! of it, and the sections that order breaks it into.
//!
//! Pure: told what is known about each file, it says where each goes.
//! The sort is stable, so that files the key cannot tell apart keep the
//! order they were in — and since the list is sorted in place each time,
//! sorting by size and then by type leaves files of one type in size
//! order, as successive sorts of a spreadsheet do. A file whose key is not
//! known yet — its header not read — sorts after every file whose key is,
//! and moves into place once it is.

use std::cmp::Ordering;
use std::ops::Range;
use std::path::Path;

use crate::thumbnailer::Facts;
use crate::ui::filmstrip::{Order, Section, Sort};

/// What one file is ordered by: its path, and what its header said, where
/// it has been read.
#[derive(Clone, Copy, Debug)]
pub(super) struct Key<'a> {
    pub path: &'a Path,
    /// The decoder that claims the file, by name.
    pub format: Option<&'static str>,
    /// Its size on disk.
    pub bytes: Option<u64>,
    /// Its size in pixels.
    pub size: Option<(u32, u32)>,
}

impl<'a> Key<'a> {
    /// What is known about `path`: its header's facts, where they have
    /// been read.
    pub(super) fn of(path: &'a Path, facts: Option<&Facts>) -> Self {
        Key {
            path,
            format: facts.and_then(|facts| facts.format),
            bytes: facts.and_then(|facts| facts.bytes),
            size: facts.and_then(|facts| facts.size),
        }
    }

    /// The section the file belongs to under `section`: `None` for a
    /// section not yet known, or where the list is not sectioned.
    fn section(&self, section: Section) -> Option<String> {
        match section {
            Section::None => None,
            Section::Path => Some(
                self.path
                    .parent()
                    .map(|parent| parent.display().to_string())
                    .unwrap_or_default(),
            ),
            Section::Type => self.format.map(str::to_string),
        }
    }

    fn name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// What a key sorts by, once `sort` has picked it out: a word or a number,
/// and either not yet known.
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Rank {
    Word(String),
    Number(u64),
}

fn rank(key: &Key, sort: Sort) -> Option<Rank> {
    Some(match sort {
        Sort::Name => Rank::Word(key.name()),
        Sort::Path => Rank::Word(key.path.display().to_string()),
        Sort::Type => Rank::Word(key.format?.to_string()),
        Sort::Size => Rank::Number(key.bytes?),
        Sort::Width => Rank::Number(u64::from(key.size?.0)),
        Sort::Height => Rank::Number(u64::from(key.size?.1)),
        Sort::Area => {
            let (width, height) = key.size?;
            Rank::Number(u64::from(width) * u64::from(height))
        }
    })
}

/// `None` after every `Some`: what is not known yet goes to the end.
fn unknown_last<T: Ord>(a: &Option<T>, b: &Option<T>) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.cmp(b),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// The places of `count` files under `order`: each entry the index, in the
/// list as it stands, of the file that now goes there. Sections first,
/// ascending, unknown last; then the sort within each, the same way; and
/// files neither tells apart in the order they stand.
pub(super) fn arrange<'a>(count: usize, order: Order, key: impl Fn(usize) -> Key<'a>) -> Vec<usize> {
    let ranked: Vec<(Option<String>, Option<Rank>)> = (0..count)
        .map(|index| {
            let key = key(index);
            (key.section(order.section), rank(&key, order.sort))
        })
        .collect();
    let mut places: Vec<usize> = (0..count).collect();
    places.sort_by(|&a, &b| {
        let (section_a, rank_a) = &ranked[a];
        let (section_b, rank_b) = &ranked[b];
        unknown_last(section_a, section_b).then_with(|| unknown_last(rank_a, rank_b))
    });
    places
}

/// The sections of a list already arranged by `section`: each run of
/// files with one label, in order, with its label — `None` for the run of
/// files whose section is not yet known, and for the whole list where it
/// is not sectioned. The ranges cover the list without a gap.
pub(super) fn groups<'a>(
    count: usize,
    section: Section,
    key: impl Fn(usize) -> Key<'a>,
) -> Vec<(Option<String>, Range<usize>)> {
    let mut groups: Vec<(Option<String>, Range<usize>)> = Vec::new();
    for index in 0..count {
        let label = key(index).section(section);
        match groups.last_mut() {
            Some((last, range)) if *last == label => range.end = index + 1,
            _ => groups.push((label, index..index + 1)),
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct File {
        path: PathBuf,
        format: Option<&'static str>,
        bytes: Option<u64>,
        size: Option<(u32, u32)>,
    }

    fn file(path: &str, format: Option<&'static str>, bytes: Option<u64>, size: Option<(u32, u32)>) -> File {
        File {
            path: PathBuf::from(path),
            format,
            bytes,
            size,
        }
    }

    fn key<'a>(files: &'a [File]) -> impl Fn(usize) -> Key<'a> {
        move |index| Key {
            path: &files[index].path,
            format: files[index].format,
            bytes: files[index].bytes,
            size: files[index].size,
        }
    }

    fn order(section: Section, sort: Sort) -> Order {
        Order { section, sort }
    }

    /// Each sort puts what it reads in ascending order, and what it
    /// cannot read after that.
    #[test]
    fn each_sort_puts_its_key_ascending_and_the_unknown_last() {
        let files = [
            file("b/two.png", Some("PNG"), Some(300), Some((10, 40))),
            file("a/three.jpg", Some("JPEG"), None, Some((30, 10))),
            file("c/one.gif", None, Some(100), None),
            file("a/four.png", Some("PNG"), Some(200), Some((20, 20))),
        ];
        let arranged = |sort| arrange(files.len(), order(Section::None, sort), key(&files));
        assert_eq!(arranged(Sort::Name), [3, 2, 1, 0], "four, one, three, two");
        assert_eq!(arranged(Sort::Path), [3, 1, 0, 2]);
        assert_eq!(arranged(Sort::Type), [1, 0, 3, 2], "JPEG, PNG, PNG, then unknown");
        assert_eq!(arranged(Sort::Size), [2, 3, 0, 1], "100, 200, 300, then unknown");
        assert_eq!(arranged(Sort::Width), [0, 3, 1, 2]);
        assert_eq!(arranged(Sort::Height), [1, 3, 0, 2]);
        assert_eq!(arranged(Sort::Area), [1, 0, 3, 2], "300, 400, 400, then unknown");
    }

    /// Files the key cannot tell apart keep the order they stand in, so
    /// that sorting by one thing and then another composes.
    #[test]
    fn ties_keep_the_order_they_stand_in() {
        let files = [
            file("y.png", Some("PNG"), Some(3), None),
            file("x.jpg", Some("JPEG"), Some(1), None),
            file("z.png", Some("PNG"), Some(2), None),
            file("w.jpg", Some("JPEG"), Some(4), None),
        ];
        let by_size = arrange(files.len(), order(Section::None, Sort::Size), key(&files));
        assert_eq!(by_size, [1, 2, 0, 3]);
        let sized: Vec<File> = by_size
            .iter()
            .map(|&index| file(files[index].path.to_str().unwrap(), files[index].format, files[index].bytes, None))
            .collect();
        let by_type = arrange(sized.len(), order(Section::None, Sort::Type), key(&sized));
        let names: Vec<&str> = by_type
            .iter()
            .map(|&index| sized[index].path.to_str().unwrap())
            .collect();
        assert_eq!(names, ["x.jpg", "w.jpg", "z.png", "y.png"], "size order within each type");
    }

    /// Sections come before the sort: the list is broken up first, each
    /// piece sorted on its own, and the piece whose label is not known
    /// yet comes last.
    #[test]
    fn sections_come_first_ascending_with_the_unknown_last() {
        let files = [
            file("b/2.png", Some("PNG"), None, None),
            file("a/9.jpg", Some("JPEG"), None, None),
            file("a/1.png", Some("PNG"), None, None),
            file("b/1.gif", None, None, None),
        ];
        let by_folder = arrange(files.len(), order(Section::Path, Sort::Name), key(&files));
        assert_eq!(by_folder, [2, 1, 3, 0], "a/1, a/9, then b/1, b/2");
        let by_type = arrange(files.len(), order(Section::Type, Sort::Name), key(&files));
        assert_eq!(by_type, [1, 2, 0, 3], "JPEG, then the PNGs, then the one not yet known");

        let arranged: Vec<File> = by_type
            .iter()
            .map(|&index| file(files[index].path.to_str().unwrap(), files[index].format, None, None))
            .collect();
        assert_eq!(
            groups(arranged.len(), Section::Type, key(&arranged)),
            [
                (Some("JPEG".to_string()), 0..1),
                (Some("PNG".to_string()), 1..3),
                (None, 3..4),
            ]
        );
        assert_eq!(
            groups(files.len(), Section::None, key(&files)),
            [(None, 0..4)],
            "one unlabeled run where the list is not sectioned"
        );
        assert!(groups(0, Section::Path, key(&files)).is_empty());
    }
}
