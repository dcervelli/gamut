//! The file chooser's state: what was typed, which files fit it, which row
//! the cursor is on, and what is known about each file so far.
//!
//! Pure, and the interface's twin: everything the popup draws is built
//! here as an [`Input`], and everything it asks for comes back through
//! `App::act` to the methods below. The matching is done against the path
//! relative to the deepest directory every file in the session shares, so
//! that a session over one directory matches names alone and a session
//! over several can be narrowed by where a file is.
//!
//! What a row knows about its file arrives in pieces, from the thumbnail
//! thread — the header's facts first, the thumbnail later, or a failure —
//! and from the application itself for the file it has just put on screen.
//! The rows are rebuilt only when something they are built from has
//! changed, and shared with the frame rather than copied into it.

use std::collections::{HashMap, HashSet, VecDeque};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::fuzzy::{self, Matcher};
use crate::image::sequence::Sequence;
use crate::thumbnailer::{Delivered, Facts, News, Thumb};
use crate::ui::chooser::{Input, Row, Step};

/// How many thumbnails the screen keeps at once: about 32 MiB at the
/// display copy's size, and more rows than any list is scrolled through in
/// one sitting. Past it the least recently seen is let go, and comes back
/// from the cache when it is seen again.
pub const MAX_THUMBS: usize = 512;

/// The state behind the popup.
pub struct Chooser {
    matcher: Box<dyn Matcher>,
    query: String,
    cursor: usize,
    /// The list as it was last handed over, and each file's place in it
    /// relative to the directory they all share.
    paths: Vec<PathBuf>,
    relative: Vec<(String, String)>,
    several_dirs: bool,
    /// Which files fit the query, best first, with where the query was
    /// found in each: [`rank`] over `relative`, kept between frames.
    matches: Vec<(usize, Vec<usize>)>,
    facts: HashMap<PathBuf, Facts>,
    failed: HashSet<PathBuf>,
    /// The rows as the frame last saw them, and whether anything they were
    /// built from has changed since.
    rows: Option<Arc<[Row]>>,
    dirty: bool,
    /// Which thumbnails the rows were built against.
    thumbs_seen: u64,
    /// Set by [`Chooser::open`] and taken by the first [`Chooser::input`]
    /// after it — see [`Input::opened`].
    opened: bool,
    reveal: bool,
    visible: Range<usize>,
}

impl Default for Chooser {
    fn default() -> Self {
        Self::with(fuzzy::default())
    }
}

impl Chooser {
    /// A chooser matching with `matcher`.
    pub fn with(matcher: Box<dyn Matcher>) -> Self {
        Self {
            matcher,
            query: String::new(),
            cursor: 0,
            paths: Vec::new(),
            relative: Vec::new(),
            several_dirs: false,
            matches: Vec::new(),
            facts: HashMap::new(),
            failed: HashSet::new(),
            rows: None,
            dirty: true,
            thumbs_seen: 0,
            opened: false,
            reveal: false,
            visible: 0..0,
        }
    }

    /// Opens over `paths`, the query cleared and the cursor on `shown`, the
    /// file already on screen: `Enter` at once is then no move at all, and
    /// `Down` is the next file, which is what the list is walked by.
    pub fn open(&mut self, paths: &[PathBuf], shown: usize) {
        self.query.clear();
        self.relist(paths);
        self.cursor = self
            .matches
            .iter()
            .position(|(index, _)| *index == shown)
            .unwrap_or(0);
        self.opened = true;
        self.reveal = true;
        self.visible = 0..0;
    }

    /// Takes in the list as it now stands, keeping the query: the file on
    /// screen may have moved, and files may have come or gone.
    pub fn relist(&mut self, paths: &[PathBuf]) {
        if self.paths != paths {
            self.paths = paths.to_vec();
            let common = common_dir(paths);
            self.relative = paths.iter().map(|path| relative(path, &common)).collect();
            self.several_dirs = self.relative.iter().any(|(dir, _)| !dir.is_empty());
        }
        self.rematch();
        self.cursor = self.cursor.min(self.matches.len().saturating_sub(1));
    }

    pub fn set_query(&mut self, query: String) {
        if self.query == query {
            return;
        }
        self.query = query;
        self.rematch();
        self.cursor = 0;
        self.reveal = true;
    }

    fn rematch(&mut self) {
        self.matches = match index_query(&self.query) {
            Some(digits) => rank_by_index(digits, self.paths.len()),
            None => {
                let candidates: Vec<String> = self
                    .relative
                    .iter()
                    .map(|(dir, name)| candidate(dir, name))
                    .collect();
                let borrowed: Vec<&str> = candidates.iter().map(String::as_str).collect();
                rank(self.matcher.as_ref(), &self.query, &borrowed)
            }
        };
        self.dirty = true;
    }

    /// Moves the cursor, clamped to the list.
    pub fn step(&mut self, step: Step) {
        let last = self.matches.len().saturating_sub(1);
        self.cursor = match step {
            Step::Up => self.cursor.saturating_sub(1),
            Step::Down => (self.cursor + 1).min(last),
            Step::Page { down: false, rows } => self.cursor.saturating_sub(rows),
            Step::Page { down: true, rows } => (self.cursor + rows).min(last),
            Step::First => 0,
            Step::Last => last,
        };
        self.reveal = true;
    }

    /// Takes in what the thumbnail thread had to say about a file. Hands
    /// back the thumbnail, if that is what it was, for the screen to hold.
    pub fn take(&mut self, delivered: Delivered) -> Option<(PathBuf, Thumb)> {
        let Delivered { path, news } = delivered;
        self.dirty = true;
        match news {
            News::Facts(facts) => {
                self.facts.insert(path, facts);
                None
            }
            News::Failed => {
                self.failed.insert(path);
                None
            }
            News::Thumb(thumb) => Some((path, thumb)),
        }
    }

    /// What the application itself has learned about a file — the one it
    /// has just put on screen — ahead of the thread reaching it. A file that
    /// has decoded on screen is no failure, whatever the thread found when
    /// it looked: a paste is on the list before its bytes have arrived, and
    /// the thread may have reached the empty file first. Returns whether it
    /// had been given up on, so that the caller can ask for it again.
    pub fn learn(&mut self, path: &Path, facts: Facts) -> bool {
        if self.facts.get(path) != Some(&facts) {
            self.facts.insert(path.to_path_buf(), facts);
            self.dirty = true;
        }
        let was_failed = self.failed.remove(path);
        self.dirty |= was_failed;
        was_failed
    }

    /// The file at `row` of the list as the frame last saw it.
    pub fn path_at(&self, row: usize) -> Option<&Path> {
        let (index, _) = self.matches.get(row)?;
        self.paths.get(*index).map(PathBuf::as_path)
    }

    /// The rows in `visible` that still lack a thumbnail, or the facts
    /// under it, and have not been given up on: what to ask the thread for
    /// first. `thumbs` is what the screen holds.
    pub fn wanted(&mut self, visible: Range<usize>, thumbs: &Thumbs) -> Vec<PathBuf> {
        self.visible = visible.clone();
        visible
            .filter_map(|row| self.path_at(row))
            .filter(|path| {
                !self.failed.contains(*path)
                    && (thumbs.get(path).is_none() || !self.facts.contains_key(*path))
            })
            .map(Path::to_path_buf)
            .collect()
    }

    /// What the frame draws, built afresh only where something changed.
    /// `current` is the file on screen, marked in the list.
    pub fn input(&mut self, thumbs: &Thumbs, current: Option<&Path>) -> Input {
        if self.dirty || self.rows.is_none() || self.thumbs_seen != thumbs.generation {
            self.rows = Some(self.build(thumbs));
            self.dirty = false;
            self.thumbs_seen = thumbs.generation;
        }
        let rows = self.rows.clone().expect("built above");
        let current = current.and_then(|shown| {
            self.matches
                .iter()
                .position(|(index, _)| self.paths[*index] == shown)
        });
        Input {
            query: self.query.clone(),
            rows,
            cursor: self.cursor,
            current,
            several_dirs: self.several_dirs,
            count: self.paths.len(),
            opened: std::mem::take(&mut self.opened),
            reveal: std::mem::take(&mut self.reveal),
            visible: self.visible.clone(),
        }
    }

    fn build(&self, thumbs: &Thumbs) -> Arc<[Row]> {
        self.matches
            .iter()
            .map(|(index, positions)| {
                let path = &self.paths[*index];
                let (dir, name) = &self.relative[*index];
                let facts = self.facts.get(path);
                Row {
                    name: name.clone(),
                    dir: dir.clone(),
                    kind: kind(path, facts),
                    index: index + 1,
                    dimensions: facts.and_then(|facts| facts.size),
                    thumb: thumbs.get(path),
                    positions: positions.clone(),
                }
            })
            .collect()
    }
}

/// What the row says the file is: its extension in capitals, and — once
/// the header has been read — the frames or pages it holds.
fn kind(path: &Path, facts: Option<&Facts>) -> String {
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_uppercase())
        .unwrap_or_default();
    match facts.map(|facts| facts.sequence) {
        Some(Sequence::Animation { count, .. }) => {
            format!("{extension}, {count} {}", plural(count, "frame"))
        }
        Some(Sequence::Pages { count, .. }) => {
            format!("{extension}, {count} {}", plural(count, "page"))
        }
        Some(Sequence::Still) | None => extension,
    }
}

fn plural(count: usize, word: &str) -> String {
    if count == 1 {
        word.to_string()
    } else {
        format!("{word}s")
    }
}

/// `dir/name`, or `name` alone where there is no directory to say: what
/// the query is matched against, so that a hit in either counts.
fn candidate(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

/// Which of `candidates` fit `query`, best first, each with the char
/// indices the query was found at. Ties keep the order they came in, so a
/// list of files stays in its own order among equals; an empty query is
/// every candidate in order, with nothing to light.
pub fn rank(matcher: &dyn Matcher, query: &str, candidates: &[&str]) -> Vec<(usize, Vec<usize>)> {
    if query.is_empty() {
        return (0..candidates.len())
            .map(|index| (index, Vec::new()))
            .collect();
    }
    let mut scored: Vec<(i64, usize, Vec<usize>)> = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| {
            let (score, positions) = matcher.fuzzy_indices(candidate, query)?;
            Some((score, index, positions))
        })
        .collect();
    scored.sort_by_key(|(score, _, _)| std::cmp::Reverse(*score));
    scored
        .into_iter()
        .map(|(_, index, positions)| (index, positions))
        .collect()
}

/// What follows the `:` of a query that asks for a file by its place in
/// the list rather than by its name — `:12` — or `None` for a query that
/// does not start with one.
pub fn index_query(query: &str) -> Option<&str> {
    query.strip_prefix(':')
}

/// The rows a query of `:digits` fits, over a list `count` long: the file
/// at exactly that place first, where there is one, then every file whose
/// place has those digits in it, in the list's order — `:1` is the first
/// file and then the tenth through the nineteenth. Nothing but digits
/// fits nothing, and `:` alone is the whole list. No chars are lit: the
/// digits are the row's index, which is not a run of text.
pub fn rank_by_index(digits: &str, count: usize) -> Vec<(usize, Vec<usize>)> {
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Vec::new();
    }
    let exact = digits
        .parse::<usize>()
        .ok()
        .filter(|place| (1..=count).contains(place));
    exact
        .into_iter()
        .chain(
            (1..=count).filter(|place| Some(*place) != exact && place.to_string().contains(digits)),
        )
        .map(|place| (place - 1, Vec::new()))
        .collect()
}

/// The deepest directory every path in `paths` is under: the one a session
/// over a single directory names, and the shared ancestor of a session over
/// several. Empty where they share nothing, in which case each file is
/// shown with the whole of its own directory.
pub fn common_dir(paths: &[PathBuf]) -> PathBuf {
    let mut parents = paths
        .iter()
        .map(|path| path.parent().unwrap_or(Path::new("")));
    let Some(first) = parents.next() else {
        return PathBuf::new();
    };
    let mut common: Vec<_> = first.components().collect();
    for parent in parents {
        let shared = common
            .iter()
            .zip(parent.components())
            .take_while(|(a, b)| *a == b)
            .count();
        common.truncate(shared);
    }
    common.iter().collect()
}

/// `path` split for a row: the directory it is in relative to `common`,
/// with no separator at its end, and its name.
pub fn relative(path: &Path, common: &Path) -> (String, String) {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parent = path.parent().unwrap_or(Path::new(""));
    let dir = parent.strip_prefix(common).unwrap_or(parent);
    let dir = dir.to_string_lossy().trim_end_matches('/').to_string();
    (dir, name)
}

/// The thumbnails the screen holds, least recently seen first to go.
#[derive(Default)]
pub struct Thumbs {
    textures: HashMap<PathBuf, egui::TextureHandle>,
    /// Every path held, oldest first; a path seen again moves to the end.
    order: VecDeque<PathBuf>,
    /// Counts every change, so that rows built against one set of
    /// thumbnails can tell they are stale.
    pub generation: u64,
}

impl Thumbs {
    /// Holds `texture` for `path`, letting the least recently seen go if
    /// that makes one too many.
    pub fn insert(&mut self, path: PathBuf, texture: egui::TextureHandle) {
        self.order.retain(|held| *held != path);
        self.order.push_back(path.clone());
        self.textures.insert(path, texture);
        while self.order.len() > MAX_THUMBS {
            if let Some(oldest) = self.order.pop_front() {
                self.textures.remove(&oldest);
            }
        }
        self.generation += 1;
    }

    /// Notes that `path`'s thumbnail is on screen, which is what keeps it.
    pub fn touch(&mut self, path: &Path) {
        if self.textures.contains_key(path) {
            self.order.retain(|held| held != path);
            self.order.push_back(path.to_path_buf());
        }
    }

    /// The thumbnail for `path`, as the painter draws it, if it is held.
    pub fn get(&self, path: &Path) -> Option<egui::load::SizedTexture> {
        self.textures
            .get(path)
            .map(egui::load::SizedTexture::from_handle)
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.textures.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fuzzy::Plain;

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    /// An empty query is the list as it stands, with nothing lit; a query
    /// drops what it does not fit; and equal fits keep their order, so the
    /// list's own order is what the user sees among them.
    #[test]
    fn ranking_keeps_order_among_equals_and_drops_misses() {
        let names = ["b.png", "a.png", "c.jpg", "ab.png"];
        assert_eq!(
            rank(&Plain, "", &names),
            vec![(0, vec![]), (1, vec![]), (2, vec![]), (3, vec![])]
        );
        // `png` spans three chars in each, so all three tie and stay put.
        assert_eq!(
            rank(&Plain, "png", &names),
            vec![(0, vec![2, 3, 4]), (1, vec![2, 3, 4]), (3, vec![3, 4, 5])]
        );
        // A tighter fit outranks a looser one whatever the order.
        let names = ["a-b.png", "ab.png"];
        assert_eq!(rank(&Plain, "ab", &names)[0].0, 1);
        assert!(rank(&Plain, "zzz", &names).is_empty());
    }

    /// The positions are char indices of `dir/name`, and the row splits
    /// them at the separator by the directory's char count, so a hit in a
    /// name with a multi-byte char before it still lands on the right char.
    #[test]
    fn positions_are_char_indices_across_the_separator() {
        let common = PathBuf::from("root");
        let (dir, name) = relative(Path::new("root/über/Ünïcode.png"), &common);
        assert_eq!((dir.as_str(), name.as_str()), ("über", "Ünïcode.png"));
        let candidate = candidate(&dir, &name);
        let (_, positions) = Plain.fuzzy_indices(&candidate, "ün").expect("found");
        // The first `ü` is in the directory; `n` is the third char of the
        // name, past the directory's four chars and the separator.
        assert_eq!(positions, vec![0, 6]);
        let dir_chars = dir.chars().count() + 1;
        assert_eq!(positions[1] - dir_chars, 1);
        assert_eq!(name.chars().nth(1), Some('n'));
    }

    /// A query beginning with `:` asks by place in the list: the exact
    /// place first, then every place with those digits in it, in order;
    /// `:` alone keeps the whole list, and anything but digits after it
    /// fits nothing.
    #[test]
    fn a_colon_query_asks_by_index() {
        let rows = |digits: &str| -> Vec<usize> {
            rank_by_index(digits, 25)
                .into_iter()
                .map(|(index, positions)| {
                    assert!(positions.is_empty());
                    index + 1
                })
                .collect()
        };
        assert_eq!(rows("3"), vec![3, 13, 23]);
        assert_eq!(
            rows("1"),
            vec![1, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 21]
        );
        assert_eq!(rows("25"), vec![25]);
        assert_eq!(rows("26"), Vec::<usize>::new());
        assert_eq!(rows("2"), vec![2, 12, 20, 21, 22, 23, 24, 25]);
        assert_eq!(rows("").len(), 25);
        assert_eq!(rows("1x"), Vec::<usize>::new());
        assert_eq!(rows("99999999999999999999999"), Vec::<usize>::new());
        assert_eq!(index_query(":12"), Some("12"));
        assert_eq!(index_query("12"), None);

        // Through the chooser: the cursor lands on the exact place, and
        // the row's path is that file's.
        let mut chooser = Chooser::with(Box::new(Plain));
        let list: Vec<PathBuf> = (1..=12)
            .map(|n| PathBuf::from(format!("{n}.png")))
            .collect();
        chooser.open(&list, 0);
        chooser.set_query(":2".to_string());
        assert_eq!(chooser.cursor, 0);
        assert_eq!(chooser.path_at(0), Some(Path::new("2.png")));
        assert_eq!(chooser.path_at(1), Some(Path::new("12.png")));
        let input = chooser.input(&Thumbs::default(), None);
        assert_eq!(input.count, 12);
        assert_eq!(input.rows.len(), 2);
    }

    /// One directory is nothing to show; nested directories show their
    /// tails under the shared parent; disjoint roots show every parent
    /// whole.
    #[test]
    fn the_common_directory_is_the_deepest_shared_one() {
        assert_eq!(
            common_dir(&paths(&["pics/a.png", "pics/b.png"])),
            PathBuf::from("pics")
        );
        assert_eq!(
            relative(Path::new("pics/a.png"), Path::new("pics")),
            ("".into(), "a.png".into())
        );

        let nested = paths(&["/home/me/pics/2024/a.png", "/home/me/pics/2025/june/b.png"]);
        assert_eq!(common_dir(&nested), PathBuf::from("/home/me/pics"));
        assert_eq!(
            relative(&nested[1], Path::new("/home/me/pics")),
            ("2025/june".into(), "b.png".into())
        );

        let disjoint = paths(&["/mnt/a/x.png", "/srv/b/y.png"]);
        assert_eq!(common_dir(&disjoint), PathBuf::from("/"));
        assert_eq!(
            relative(&disjoint[0], &common_dir(&disjoint)),
            ("mnt/a".into(), "x.png".into())
        );
        let relatives = paths(&["a/x.png", "b/y.png"]);
        assert_eq!(common_dir(&relatives), PathBuf::new());
        assert_eq!(
            relative(&relatives[1], &common_dir(&relatives)),
            ("b".into(), "y.png".into())
        );
        assert_eq!(common_dir(&paths(&["a.png"])), PathBuf::new());
        assert_eq!(common_dir(&[]), PathBuf::new());
    }

    /// The cursor stays inside the list whatever is asked of it, and a
    /// page is as many rows as the list had room for.
    #[test]
    fn the_cursor_is_clamped_to_the_list() {
        let mut chooser = Chooser::with(Box::new(Plain));
        chooser.open(&paths(&["a.png", "b.png", "c.png", "d.png"]), 1);
        assert_eq!(chooser.cursor, 1);
        chooser.step(Step::Up);
        chooser.step(Step::Up);
        assert_eq!(chooser.cursor, 0);
        chooser.step(Step::Page {
            down: true,
            rows: 10,
        });
        assert_eq!(chooser.cursor, 3);
        chooser.step(Step::Down);
        assert_eq!(chooser.cursor, 3);
        chooser.step(Step::Page {
            down: false,
            rows: 2,
        });
        assert_eq!(chooser.cursor, 1);
        chooser.step(Step::Last);
        assert_eq!(chooser.cursor, 3);
        chooser.step(Step::First);
        assert_eq!(chooser.cursor, 0);

        // A query resets it to the best fit, and an empty list has nowhere
        // for it to be.
        chooser.step(Step::Last);
        chooser.set_query("c".to_string());
        assert_eq!(chooser.cursor, 0);
        assert_eq!(chooser.path_at(0), Some(Path::new("c.png")));
        chooser.set_query("zzz".to_string());
        assert_eq!(chooser.cursor, 0);
        assert_eq!(chooser.path_at(0), None);
        chooser.step(Step::Down);
        assert_eq!(chooser.cursor, 0);
    }

    /// The rows say what is known: the kind from the name at first, then
    /// from the header; the size once it is known; and the file on screen
    /// is marked wherever the query has put it.
    #[test]
    fn the_rows_fill_in_as_facts_arrive() {
        let mut chooser = Chooser::with(Box::new(Plain));
        let list = paths(&["clip.gif", "scan.tiff", "photo.jpeg"]);
        chooser.open(&list, 2);
        let thumbs = Thumbs::default();
        let input = chooser.input(&thumbs, Some(Path::new("photo.jpeg")));
        assert_eq!(input.rows.len(), 3);
        assert_eq!(input.rows[0].kind, "GIF");
        assert_eq!(input.rows[0].index, 1);
        assert_eq!(input.rows[0].dimensions, None);
        assert_eq!(input.current, Some(2));
        assert_eq!(input.cursor, 2);
        assert!(input.reveal);
        assert!(input.opened);
        assert!(!input.several_dirs);
        let again = chooser.input(&thumbs, None);
        assert!(!again.reveal, "revealed once");
        assert!(!again.opened, "opened once");

        assert!(!chooser.learn(
            Path::new("clip.gif"),
            Facts {
                size: Some((320, 240)),
                sequence: Sequence::Animation {
                    count: 12,
                    loops: crate::image::sequence::Loops::Forever,
                },
            },
        ));
        chooser.take(Delivered {
            path: PathBuf::from("scan.tiff"),
            news: News::Facts(Facts {
                size: None,
                sequence: Sequence::Pages {
                    count: 1,
                    default: 0,
                },
            }),
        });
        let input = chooser.input(&thumbs, None);
        assert_eq!(input.rows[0].kind, "GIF, 12 frames");
        assert_eq!(input.rows[0].dimensions, Some((320, 240)));
        assert_eq!(input.rows[1].kind, "TIFF, 1 page");
        assert_eq!(input.current, None);

        // The query reorders the rows, and the mark follows the file.
        chooser.set_query("photo".to_string());
        let input = chooser.input(&thumbs, Some(Path::new("photo.jpeg")));
        assert_eq!(input.rows.len(), 1);
        assert_eq!(input.rows[0].index, 3);
        assert_eq!(input.current, Some(0));
        assert_eq!(input.rows[0].positions, vec![0, 1, 2, 3, 4]);
    }

    /// What the visible rows want is what they lack and have not been
    /// given up on.
    #[test]
    fn the_visible_rows_ask_for_what_they_lack() {
        let mut chooser = Chooser::with(Box::new(Plain));
        chooser.open(&paths(&["a.png", "b.png", "c.png"]), 0);
        let mut thumbs = Thumbs::default();
        chooser.take(Delivered {
            path: PathBuf::from("b.png"),
            news: News::Failed,
        });
        let ctx = egui::Context::default();
        let texture = ctx.load_texture(
            "c",
            egui::ColorImage::filled([1, 1], egui::Color32::BLACK),
            egui::TextureOptions::LINEAR,
        );
        thumbs.insert(PathBuf::from("c.png"), texture);
        chooser.learn(
            Path::new("c.png"),
            Facts {
                size: Some((1, 1)),
                sequence: Sequence::Still,
            },
        );
        assert_eq!(chooser.wanted(0..3, &thumbs), paths(&["a.png"]));
        assert_eq!(chooser.input(&thumbs, None).visible, 0..3);

        // A file given up on that then decodes on screen is wanted again.
        let facts = Facts {
            size: Some((1, 1)),
            sequence: Sequence::Still,
        };
        assert!(chooser.learn(Path::new("b.png"), facts));
        assert!(!chooser.learn(Path::new("b.png"), facts));
        assert_eq!(chooser.wanted(0..3, &thumbs), paths(&["a.png", "b.png"]));
    }

    /// The screen keeps the newest thumbnails and the ones most recently
    /// seen, and lets the rest go in the order they were last seen.
    #[test]
    fn thumbnails_are_let_go_least_recently_seen_first() {
        let ctx = egui::Context::default();
        let texture = |name: &str| {
            ctx.load_texture(
                name,
                egui::ColorImage::filled([1, 1], egui::Color32::BLACK),
                egui::TextureOptions::LINEAR,
            )
        };
        let mut thumbs = Thumbs::default();
        for index in 0..MAX_THUMBS {
            thumbs.insert(PathBuf::from(format!("{index}.png")), texture("t"));
        }
        assert_eq!(thumbs.len(), MAX_THUMBS);
        assert!(thumbs.get(Path::new("0.png")).is_some());

        // Seeing the oldest again spares it; the next oldest goes instead.
        thumbs.touch(Path::new("0.png"));
        let before = thumbs.generation;
        thumbs.insert(PathBuf::from("new.png"), texture("t"));
        assert_eq!(thumbs.len(), MAX_THUMBS);
        assert!(thumbs.get(Path::new("0.png")).is_some());
        assert!(thumbs.get(Path::new("1.png")).is_none());
        assert!(thumbs.get(Path::new("new.png")).is_some());
        assert_eq!(thumbs.generation, before + 1);

        // Held again under the same path is one entry, not two.
        thumbs.insert(PathBuf::from("new.png"), texture("t"));
        assert_eq!(thumbs.len(), MAX_THUMBS);
    }
}
