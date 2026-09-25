//! The order the file list stands in: the permutation an [`Order`] makes
//! of it.
//!
//! Pure: told what is known about each file, it says where each goes.
//! The sort is stable, so that files the key cannot tell apart keep the
//! order they were in — and since the list is sorted in place each time,
//! sorting by size and then by type leaves files of one type in size
//! order, as successive sorts of a spreadsheet do. A file whose key is not
//! known yet — its header not read — sorts after every file whose key is,
//! and moves into place once it is.

use std::cmp::Ordering;
use std::path::Path;
use std::time::SystemTime;

use crate::thumbnailer::Facts;
use crate::ui::filmstrip::{Direction, Order, Sort};

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
    /// When it was last written.
    pub modified: Option<SystemTime>,
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
            modified: facts.and_then(|facts| facts.modified),
        }
    }

    fn name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// What a key sorts by, once `sort` has picked it out: a word, a number
/// or a moment, and any of them not yet known.
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Rank {
    Word(String),
    Number(u64),
    Moment(SystemTime),
}

fn rank(key: &Key, sort: Sort) -> Option<Rank> {
    Some(match sort {
        Sort::Name => Rank::Word(key.name()),
        Sort::Path => Rank::Word(key.path.display().to_string()),
        Sort::Type => Rank::Word(key.format?.to_string()),
        Sort::Size => Rank::Number(key.bytes?),
        Sort::Date => Rank::Moment(key.modified?),
        Sort::Width => Rank::Number(u64::from(key.size?.0)),
        Sort::Height => Rank::Number(u64::from(key.size?.1)),
        Sort::Area => {
            let (width, height) = key.size?;
            Rank::Number(u64::from(width) * u64::from(height))
        }
    })
}

/// `None` after every `Some`, whichever way the known ones run: what is
/// not known yet goes to the end.
fn unknown_last<T: Ord>(a: &Option<T>, b: &Option<T>, direction: Direction) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => match direction {
            Direction::Ascending => a.cmp(b),
            Direction::Descending => b.cmp(a),
        },
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// The places of `count` files under `order`: each entry the index, in the
/// list as it stands, of the file that now goes there. The sort, the way
/// the order runs, unknown last; and files it cannot tell apart in the
/// order they stand — whichever way the sort runs, since the comparison is
/// turned round rather than the list.
pub(super) fn arrange<'a>(
    count: usize,
    order: Order,
    key: impl Fn(usize) -> Key<'a>,
) -> Vec<usize> {
    let ranked: Vec<Option<Rank>> = (0..count)
        .map(|index| rank(&key(index), order.sort))
        .collect();
    let mut places: Vec<usize> = (0..count).collect();
    places.sort_by(|&a, &b| unknown_last(&ranked[a], &ranked[b], order.direction));
    places
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Duration;

    struct File {
        path: PathBuf,
        format: Option<&'static str>,
        bytes: Option<u64>,
        size: Option<(u32, u32)>,
        modified: Option<SystemTime>,
    }

    fn file(
        path: &str,
        format: Option<&'static str>,
        bytes: Option<u64>,
        size: Option<(u32, u32)>,
    ) -> File {
        File {
            path: PathBuf::from(path),
            format,
            bytes,
            size,
            modified: None,
        }
    }

    fn key<'a>(files: &'a [File]) -> impl Fn(usize) -> Key<'a> {
        move |index| Key {
            path: &files[index].path,
            format: files[index].format,
            bytes: files[index].bytes,
            size: files[index].size,
            modified: files[index].modified,
        }
    }

    fn order(sort: Sort) -> Order {
        Order {
            sort,
            direction: Direction::Ascending,
        }
    }

    /// Descending turns the sort round and nothing else: what is not known
    /// stays last, and ties keep the order they stand in rather than coming
    /// out reversed.
    #[test]
    fn descending_turns_the_sort_round_and_nothing_else() {
        let files = [
            file("b/2.png", Some("PNG"), Some(2), None),
            file("a/9.jpg", Some("JPEG"), None, None),
            file("a/1.png", Some("PNG"), Some(2), None),
            file("b/1.gif", None, Some(3), None),
        ];
        let descending = |sort| Order {
            sort,
            direction: Direction::Descending,
        };
        assert_eq!(
            arrange(files.len(), descending(Sort::Name), key(&files)),
            [1, 0, 2, 3],
            "9, 2, 1.png, 1.gif"
        );
        assert_eq!(
            arrange(files.len(), descending(Sort::Size), key(&files)),
            [3, 0, 2, 1],
            "3, then the two 2s as they stand, then the unknown"
        );
    }

    /// Each sort puts what it reads in ascending order, and what it
    /// cannot read after that.
    #[test]
    fn each_sort_puts_its_key_ascending_and_the_unknown_last() {
        let mut files = [
            file("b/two.png", Some("PNG"), Some(300), Some((10, 40))),
            file("a/three.jpg", Some("JPEG"), None, Some((30, 10))),
            file("c/one.gif", None, Some(100), None),
            file("a/four.png", Some("PNG"), Some(200), Some((20, 20))),
        ];
        for (file, seconds) in files.iter_mut().zip([Some(50), Some(10), None, Some(30)]) {
            file.modified =
                seconds.map(|seconds| SystemTime::UNIX_EPOCH + Duration::from_secs(seconds));
        }
        let arranged = |sort| arrange(files.len(), order(sort), key(&files));
        assert_eq!(arranged(Sort::Name), [3, 2, 1, 0], "four, one, three, two");
        assert_eq!(arranged(Sort::Path), [3, 1, 0, 2]);
        assert_eq!(
            arranged(Sort::Type),
            [1, 0, 3, 2],
            "JPEG, PNG, PNG, then unknown"
        );
        assert_eq!(
            arranged(Sort::Size),
            [2, 3, 0, 1],
            "100, 200, 300, then unknown"
        );
        assert_eq!(
            arranged(Sort::Date),
            [1, 3, 0, 2],
            "earliest first, then unknown"
        );
        assert_eq!(arranged(Sort::Width), [0, 3, 1, 2]);
        assert_eq!(arranged(Sort::Height), [1, 3, 0, 2]);
        assert_eq!(
            arranged(Sort::Area),
            [1, 0, 3, 2],
            "300, 400, 400, then unknown"
        );
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
        let by_size = arrange(files.len(), order(Sort::Size), key(&files));
        assert_eq!(by_size, [1, 2, 0, 3]);
        let sized: Vec<File> = by_size
            .iter()
            .map(|&index| {
                file(
                    files[index].path.to_str().unwrap(),
                    files[index].format,
                    files[index].bytes,
                    None,
                )
            })
            .collect();
        let by_type = arrange(sized.len(), order(Sort::Type), key(&sized));
        let names: Vec<&str> = by_type
            .iter()
            .map(|&index| sized[index].path.to_str().unwrap())
            .collect();
        assert_eq!(
            names,
            ["x.jpg", "w.jpg", "z.png", "y.png"],
            "size order within each type"
        );
    }
}
