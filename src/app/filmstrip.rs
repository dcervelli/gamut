//! The file list's state: the order the list stands in, the rows the strip
//! was last drawn from, and which of them are on screen.
//!
//! The interface's twin, as `app/chooser.rs` is the chooser's: everything
//! the strip draws is built here as an [`Input`], and what it asks for
//! comes back through `App::act`. The order itself is applied to the list
//! — `Files::reorder` — rather than kept here as a view of it, since the
//! order is what `]` and `[` walk; what is kept here is which order, and
//! whether the list has fallen out of it. It falls out of it whenever the
//! list changes under it or a header is read that the order depends on,
//! and is put back in it at the next poll while nothing is being read:
//! once per poll however many headers arrived, and never under a read in
//! flight, which is aimed at an index.
//!
//! The rows are rebuilt only when something they are built from has
//! changed — the list, a header, a thumbnail — and shared with the frame
//! rather than copied into it.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::chooser::Thumbs;
use super::order::{self, Key};
use crate::ui::filmstrip::{self, Input, Order, Row, Section};

/// The state behind the strip.
pub(super) struct Filmstrip {
    order: Order,
    /// Whether the list may have fallen out of `order`: something changed
    /// that the order is made from. Taken at the next poll.
    stale: bool,
    /// The list as it was last handed over.
    paths: Vec<PathBuf>,
    /// The rows as the frame last saw them, where each row starts, and
    /// whether anything they were built from has changed since.
    rows: Option<Arc<[Row]>>,
    tops: Arc<[f32]>,
    dirty: bool,
    /// Which thumbnails the rows were built against.
    thumbs_seen: u64,
    reveal: bool,
    visible: Range<usize>,
}

impl Default for Filmstrip {
    /// The default order, and the list to be put in it at the first
    /// chance.
    fn default() -> Self {
        Self {
            order: Order::default(),
            stale: true,
            paths: Vec::new(),
            rows: None,
            tops: Arc::from([0.0]),
            dirty: true,
            thumbs_seen: 0,
            reveal: false,
            visible: 0..0,
        }
    }
}

impl Filmstrip {
    pub(super) fn order(&self) -> Order {
        self.order
    }

    /// Puts the list under `order`. Returns whether that is a change,
    /// which is whether the list has to be put in it.
    pub(super) fn set_order(&mut self, order: Order) -> bool {
        if self.order == order {
            return false;
        }
        self.order = order;
        self.dirty = true;
        self.stale = true;
        true
    }

    /// Notes that the list may have fallen out of its order.
    pub(super) fn mark_stale(&mut self) {
        self.stale = true;
    }

    /// Whether the list may have fallen out of its order, and no longer:
    /// the caller is about to put it back.
    pub(super) fn take_stale(&mut self) -> bool {
        std::mem::take(&mut self.stale)
    }

    /// The list as it stands now.
    pub(super) fn relist(&mut self, paths: &[PathBuf]) {
        if self.paths != paths {
            self.paths = paths.to_vec();
            self.dirty = true;
        }
    }

    /// A header was read: a row's section may have changed.
    pub(super) fn facts_changed(&mut self) {
        self.dirty = true;
    }

    /// The file on screen changed: the strip scrolls to its row.
    pub(super) fn reveal(&mut self) {
        self.reveal = true;
    }

    /// Whether the next frame scrolls to the file on screen.
    #[cfg(test)]
    pub(super) fn reveals(&self) -> bool {
        self.reveal
    }

    /// The file at `row` of the rows as the frame last saw them, if it is
    /// a file's row.
    pub(super) fn path_at(&self, row: usize) -> Option<&Path> {
        match self.rows.as_ref()?.get(row)? {
            Row::File { index, .. } => self.paths.get(index - 1).map(PathBuf::as_path),
            Row::Header(_) => None,
        }
    }

    /// The files in `visible` that still lack a thumbnail and have not
    /// been given up on: what to ask the thread for first. `thumbs` is
    /// what the screen holds.
    pub(super) fn wanted(
        &mut self,
        visible: Range<usize>,
        thumbs: &Thumbs,
        given_up: impl Fn(&Path) -> bool,
    ) -> Vec<PathBuf> {
        self.visible = visible.clone();
        visible
            .filter_map(|row| self.path_at(row))
            .filter(|path| !given_up(path) && thumbs.get(path).is_none())
            .map(Path::to_path_buf)
            .collect()
    }

    /// What the frame draws, built afresh only where something changed.
    /// `key` is what is known about each file, for the sections; `current`
    /// the file on screen, marked in the list; `back` and `forward` whether
    /// there is a file seen before it and after it to go to.
    pub(super) fn input(
        &mut self,
        thumbs: &Thumbs,
        key: impl for<'a> Fn(&'a Path) -> Key<'a>,
        current: Option<&Path>,
        back: bool,
        forward: bool,
    ) -> Input {
        if self.dirty || self.rows.is_none() || self.thumbs_seen != thumbs.generation {
            let (rows, tops) = self.build(thumbs, key);
            self.rows = Some(rows);
            self.tops = tops;
            self.dirty = false;
            self.thumbs_seen = thumbs.generation;
        }
        let rows = self.rows.clone().expect("built above");
        let current = current.and_then(|shown| {
            let index = self.paths.iter().position(|path| path == shown)? + 1;
            rows.iter()
                .position(|row| matches!(row, Row::File { index: at, .. } if *at == index))
        });
        Input {
            rows,
            tops: Arc::clone(&self.tops),
            current,
            order: self.order,
            back,
            forward,
            reveal: std::mem::take(&mut self.reveal),
            visible: self.visible.clone(),
        }
    }

    /// The rows: each section's heading, where the list is sectioned, and
    /// under it a row for each of its files; and where each row starts.
    fn build(
        &self,
        thumbs: &Thumbs,
        key: impl for<'a> Fn(&'a Path) -> Key<'a>,
    ) -> (Arc<[Row]>, Arc<[f32]>) {
        let mut rows = Vec::with_capacity(self.paths.len());
        for (label, range) in order::groups(self.paths.len(), self.order.section, |index| {
            key(&self.paths[index])
        }) {
            match (self.order.section, label) {
                (Section::None, _) => {}
                (_, Some(label)) => rows.push(Row::Header(heading(&label))),
                (Section::Type, None) => rows.push(Row::Header("Type not yet known".to_string())),
                (Section::Path, None) => rows.push(Row::Header(heading(""))),
            }
            for index in range {
                let path = &self.paths[index];
                rows.push(Row::File {
                    index: index + 1,
                    name: path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string()),
                    path: path.display().to_string(),
                    thumb: thumbs.get(path),
                });
            }
        }
        let mut tops = Vec::with_capacity(rows.len() + 1);
        let mut top = 0.0;
        tops.push(top);
        for row in &rows {
            top += filmstrip::height(row);
            tops.push(top);
        }
        (Arc::from(rows), Arc::from(tops))
    }
}

/// A folder's heading: the directory as the list spells it, or the
/// current directory's own name for a file named with no directory at all.
fn heading(dir: &str) -> String {
    if dir.is_empty() {
        ".".to_string()
    } else {
        dir.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thumbnailer::Facts;
    use crate::ui::filmstrip::{HEADER_HEIGHT, ROW_HEIGHT, Sort};
    use std::collections::HashMap;

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    /// What is known about `path`, out of `known`. A function rather
    /// than a closure, which cannot say that the key borrows the path
    /// and nothing else.
    fn known<'a>(known: &HashMap<PathBuf, Facts>, path: &'a Path) -> Key<'a> {
        Key::of(path, known.get(path))
    }

    fn facts(format: Option<&'static str>) -> Facts {
        Facts {
            size: None,
            sequence: crate::image::sequence::Sequence::Still,
            title: None,
            format,
            bytes: None,
        }
    }

    /// The rows are the list, one row each, with a heading over each
    /// section where the list is sectioned; each row starts where the one
    /// before it ended; and the file on screen is found by its path,
    /// wherever the sections have put it.
    #[test]
    fn the_rows_follow_the_list_and_its_sections() {
        let mut strip = Filmstrip::default();
        let thumbs = Thumbs::default();
        let list = paths(&["a/1.png", "a/2.jpg", "b/3.png"]);
        strip.relist(&list);
        let facts_known: HashMap<PathBuf, Facts> = [
            (PathBuf::from("a/1.png"), facts(Some("PNG"))),
            (PathBuf::from("a/2.jpg"), facts(Some("JPEG"))),
        ]
        .into_iter()
        .collect();

        let input = strip.input(&thumbs, |path| known(&facts_known, path), Some(Path::new("b/3.png")), false, true);
        assert_eq!(input.rows.len(), 3, "no headings without sections");
        assert_eq!(input.current, Some(2));
        assert_eq!(
            &*input.tops,
            &[0.0, ROW_HEIGHT, 2.0 * ROW_HEIGHT, 3.0 * ROW_HEIGHT]
        );
        assert!(!input.back && input.forward);
        assert_eq!(strip.path_at(1), Some(Path::new("a/2.jpg")));

        // Sectioned by folder — the list already in that order — each
        // folder is a heading, and the current row moves down past it.
        assert!(strip.set_order(Order {
            section: Section::Path,
            sort: Sort::Name,
        }));
        assert!(strip.take_stale());
        let input = strip.input(&thumbs, |path| known(&facts_known, path), Some(Path::new("b/3.png")), false, false);
        assert_eq!(
            &*input.rows,
            &[
                Row::Header("a".to_string()),
                Row::File {
                    index: 1,
                    name: "1.png".to_string(),
                    path: "a/1.png".to_string(),
                    thumb: None
                },
                Row::File {
                    index: 2,
                    name: "2.jpg".to_string(),
                    path: "a/2.jpg".to_string(),
                    thumb: None
                },
                Row::Header("b".to_string()),
                Row::File {
                    index: 3,
                    name: "3.png".to_string(),
                    path: "b/3.png".to_string(),
                    thumb: None
                },
            ]
        );
        assert_eq!(input.current, Some(4));
        assert_eq!(input.tops[1], HEADER_HEIGHT);
        assert_eq!(strip.path_at(0), None, "a heading is no file");
        assert_eq!(strip.path_at(4), Some(Path::new("b/3.png")));

        // Sectioned by type, with the list put in that order, the file
        // whose type is not known yet is under a heading that says so.
        strip.set_order(Order {
            section: Section::Type,
            sort: Sort::Name,
        });
        strip.relist(&paths(&["a/2.jpg", "a/1.png", "b/3.png"]));
        let input = strip.input(&thumbs, |path| known(&facts_known, path), None, false, false);
        let headings: Vec<&str> = input
            .rows
            .iter()
            .filter_map(|row| match row {
                Row::Header(label) => Some(label.as_str()),
                Row::File { .. } => None,
            })
            .collect();
        assert_eq!(headings, ["JPEG", "PNG", "Type not yet known"]);
        assert_eq!(input.current, None);
    }

    /// The rows on screen are asked for by their files, less the ones the
    /// screen already holds and the ones given up on; and the reveal is
    /// said once.
    #[test]
    fn the_rows_on_screen_are_wanted_and_the_reveal_is_said_once() {
        let mut strip = Filmstrip::default();
        let thumbs = Thumbs::default();
        strip.relist(&paths(&["a.png", "b.png", "c.png"]));
        let nothing = HashMap::new();
        let _ = strip.input(&thumbs, |path| known(&nothing, path), None, false, false);
        assert_eq!(
            strip.wanted(0..3, &thumbs, |path| path == Path::new("b.png")),
            paths(&["a.png", "c.png"])
        );
        assert_eq!(strip.input(&thumbs, |path| known(&nothing, path), None, false, false).visible, 0..3);

        strip.reveal();
        assert!(strip.input(&thumbs, |path| known(&nothing, path), None, false, false).reveal);
        assert!(!strip.input(&thumbs, |path| known(&nothing, path), None, false, false).reveal);
    }
}
