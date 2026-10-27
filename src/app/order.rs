//! The order the file list stands in: the permutation an [`Order`] makes
//! of it.
//!
//! Pure: told what is known about each file, it says where each goes.
//! Three keys, each breaking the ties of the one before: the sort in
//! force, the sort that was in force before it — so that sorting by size
//! and then by type leaves files of one type in size order, as successive
//! sorts of a spreadsheet do — and last the folder and the name, which
//! tell any two files apart. Where a file goes is so a matter of what is
//! known of it alone, never of the order it arrived in. A file whose key
//! is not known yet — its header not read — sorts after every file whose
//! key is, and moves into place once it is.
//!
//! A sort by path ranks by the folder alone, so that each folder's files
//! keep together, in the order the key under it puts them in.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::folder::Glimpse;
use crate::image::exif::Taken;
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
    /// When it was taken.
    pub taken: Option<Taken>,
}

impl<'a> Key<'a> {
    /// What is known about `path`: its header's facts, where they have
    /// been read, and what a folder read glimpsed of it where they have
    /// not.
    pub(super) fn of(path: &'a Path, facts: Option<&Facts>, glimpse: Option<&Glimpse>) -> Self {
        // What the header said, or failing that what a folder read
        // glimpsed, or nothing.
        let Glimpse {
            format,
            bytes,
            size,
            modified,
            taken,
        } = facts
            .map(Glimpse::of)
            .or_else(|| glimpse.copied())
            .unwrap_or_default();
        Key {
            path,
            format,
            bytes,
            size,
            modified,
            taken,
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
    /// Compared a component at a time, so that `maps/fences` follows
    /// `maps` rather than `maps-old`.
    Folder(PathBuf),
    Number(u64),
    Moment(SystemTime),
}

fn rank(key: &Key, sort: Sort) -> Option<Rank> {
    Some(match sort {
        Sort::Name => Rank::Word(key.name()),
        Sort::Path => Rank::Folder(folder(key.path).to_path_buf()),
        Sort::Type => Rank::Word(key.format?.to_string()),
        Sort::Size => Rank::Number(key.bytes?),
        Sort::Modified => Rank::Moment(key.modified?),
        Sort::Taken => Rank::Moment(key.taken?.moment),
        Sort::Width => Rank::Number(u64::from(key.size?.0)),
        Sort::Height => Rank::Number(u64::from(key.size?.1)),
        Sort::Area => {
            let (width, height) = key.size?;
            Rank::Number(u64::from(width) * u64::from(height))
        }
    })
}

/// The folder `path` is in: nothing, for a bare name.
fn folder(path: &Path) -> &Path {
    path.parent().unwrap_or(Path::new(""))
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
/// it runs, unknown last; its ties by what breaks them, the way that runs,
/// unknown last; and what is left by folder and then by name, ascending
/// whichever way the others run.
pub(super) fn arrange<'a>(
    count: usize,
    order: Order,
    key: impl Fn(usize) -> Key<'a>,
) -> Vec<usize> {
    let keys: Vec<Key> = (0..count).map(key).collect();
    let ranked = |sort| -> Vec<Option<Rank>> { keys.iter().map(|key| rank(key, sort)).collect() };
    let first = ranked(order.sort);
    let then = order.tie_break().map(|by| (ranked(by.sort), by.direction));
    let mut places: Vec<usize> = (0..count).collect();
    places.sort_by(|&a, &b| {
        unknown_last(&first[a], &first[b], order.direction)
            .then_with(|| match &then {
                Some((then, direction)) => unknown_last(&then[a], &then[b], *direction),
                None => Ordering::Equal,
            })
            .then_with(|| {
                let (a, b) = (keys[a].path, keys[b].path);
                (folder(a), a.file_name()).cmp(&(folder(b), b.file_name()))
            })
    });
    places
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::filmstrip::By;
    use std::time::Duration;

    struct File {
        path: PathBuf,
        format: Option<&'static str>,
        bytes: Option<u64>,
        size: Option<(u32, u32)>,
        modified: Option<SystemTime>,
        taken: Option<Taken>,
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
            taken: None,
        }
    }

    fn key<'a>(files: &'a [File]) -> impl Fn(usize) -> Key<'a> {
        move |index| Key {
            path: &files[index].path,
            format: files[index].format,
            bytes: files[index].bytes,
            size: files[index].size,
            modified: files[index].modified,
            taken: files[index].taken,
        }
    }

    fn order(sort: Sort) -> Order {
        Order {
            sort,
            direction: Direction::Ascending,
            then: By {
                sort: Sort::Name,
                direction: Direction::Ascending,
            },
        }
    }

    /// Descending turns the sort round and nothing else: what is not known
    /// stays last, and ties go by what breaks them rather than coming out
    /// reversed.
    #[test]
    fn descending_turns_the_sort_round_and_nothing_else() {
        let files = [
            file("b/2.png", Some("PNG"), Some(2), None),
            file("a/9.jpg", Some("JPEG"), None, None),
            file("a/1.png", Some("PNG"), Some(2), None),
            file("b/1.gif", None, Some(3), None),
        ];
        let descending = |sort| Order {
            direction: Direction::Descending,
            ..order(sort)
        };
        assert_eq!(
            arrange(files.len(), descending(Sort::Name), key(&files)),
            [1, 0, 2, 3],
            "9, 2, 1.png, 1.gif"
        );
        assert_eq!(
            arrange(files.len(), descending(Sort::Size), key(&files)),
            [3, 2, 0, 1],
            "3, then the two 2s by name, then the unknown"
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
        let moment = |seconds| SystemTime::UNIX_EPOCH + Duration::from_secs(seconds);
        for (file, seconds) in files.iter_mut().zip([Some(50), Some(10), None, Some(30)]) {
            file.modified = seconds.map(moment);
        }
        for (file, seconds) in files.iter_mut().zip([Some(20), None, Some(40), Some(5)]) {
            file.taken = seconds.map(|seconds| Taken {
                moment: moment(seconds),
                wall: crate::clock::utc(moment(seconds)),
            });
        }
        let arranged = |sort| arrange(files.len(), order(sort), key(&files));
        assert_eq!(arranged(Sort::Name), [3, 2, 1, 0], "four, one, three, two");
        assert_eq!(
            arranged(Sort::Path),
            [3, 1, 0, 2],
            "a's two by name, then b, then c"
        );
        assert_eq!(
            arranged(Sort::Type),
            [1, 3, 0, 2],
            "JPEG, PNG, PNG by name, then unknown"
        );
        assert_eq!(
            arranged(Sort::Size),
            [2, 3, 0, 1],
            "100, 200, 300, then unknown"
        );
        assert_eq!(
            arranged(Sort::Modified),
            [1, 3, 0, 2],
            "earliest first, then unknown"
        );
        assert_eq!(
            arranged(Sort::Taken),
            [3, 0, 2, 1],
            "earliest first, then unknown"
        );
        assert_eq!(arranged(Sort::Width), [0, 3, 1, 2]);
        assert_eq!(arranged(Sort::Height), [1, 3, 0, 2]);
        assert_eq!(
            arranged(Sort::Area),
            [1, 3, 0, 2],
            "300, 400, 400 by name, then unknown"
        );
    }

    /// A sort by path keeps each folder's files together, a folder's
    /// subfolders after it rather than among its files or its neighbors
    /// whose names run on past it, and within a folder puts them as the
    /// key under it does.
    #[test]
    fn a_sort_by_path_groups_by_folder_and_keeps_the_order_within_one() {
        let files = [
            file("maps/labels/town.png", None, Some(1), None),
            file("maps/harbor.png", None, Some(1), None),
            file("maps-old/a.png", None, Some(1), None),
            file("maps/fences/post.png", None, Some(1), None),
            file("maps/grid.png", None, Some(2), None),
        ];
        let by_size = Order {
            then: By {
                sort: Sort::Size,
                direction: Direction::Descending,
            },
            ..order(Sort::Path)
        };
        let places = arrange(files.len(), by_size, key(&files));
        let paths: Vec<&str> = places
            .iter()
            .map(|&index| files[index].path.to_str().unwrap())
            .collect();
        assert_eq!(
            paths,
            [
                "maps/grid.png",
                "maps/harbor.png",
                "maps/fences/post.png",
                "maps/labels/town.png",
                "maps-old/a.png",
            ]
        );
    }

    /// Sorting by one thing and then another composes, as a spreadsheet's
    /// sorts do, and comes out the same whatever order the files stood in.
    #[test]
    fn the_sort_before_breaks_the_ties() {
        let files = [
            file("y.png", Some("PNG"), Some(3), None),
            file("x.jpg", Some("JPEG"), Some(1), None),
            file("z.png", Some("PNG"), Some(2), None),
            file("w.jpg", Some("JPEG"), Some(4), None),
        ];
        let by_type = Order::default().sorted_by(Sort::Size).sorted_by(Sort::Type);
        let names = |files: &[File]| -> Vec<String> {
            arrange(files.len(), by_type, key(files))
                .iter()
                .map(|&index| files[index].path.to_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(
            names(&files),
            ["x.jpg", "w.jpg", "z.png", "y.png"],
            "size order within each type"
        );
        let before = names(&files);
        let mut reversed = files;
        reversed.reverse();
        assert_eq!(names(&reversed), before, "whatever the order before");
    }

    /// A sort pressed puts the one in force under it, in the direction it
    /// ran; the one under it pressed trades places with it; and the one in
    /// force pressed again does nothing.
    #[test]
    fn a_sort_pressed_puts_the_one_in_force_under_it() {
        let by = |sort, direction| By { sort, direction };
        let taken_down = Order {
            direction: Direction::Descending,
            ..Order::default().sorted_by(Sort::Taken)
        };
        assert_eq!(
            taken_down.then,
            by(Sort::Name, Direction::Ascending),
            "the default's name under it"
        );
        let by_path = taken_down.sorted_by(Sort::Path);
        assert_eq!(
            (by_path.sort, by_path.direction, by_path.then),
            (
                Sort::Path,
                Direction::Descending,
                by(Sort::Taken, Direction::Descending)
            ),
            "a new sort keeps the direction in force"
        );
        let up = Order {
            direction: Direction::Ascending,
            ..by_path
        };
        let back = up.sorted_by(Sort::Taken);
        assert_eq!(
            (back.sort, back.direction, back.then),
            (
                Sort::Taken,
                Direction::Descending,
                by(Sort::Path, Direction::Ascending)
            ),
            "the two trade places, each with its own direction"
        );
        assert_eq!(back.sorted_by(Sort::Taken), back);
    }
}
