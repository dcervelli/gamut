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
use super::order::Key;
use crate::ui::filmstrip::{self, Input, Order, Row};

/// The state behind the strip.
pub(super) struct Filmstrip {
    order: Order,
    /// Whether the list may have fallen out of `order`: something changed
    /// that the order is made from. Taken at the next poll.
    stale: bool,
    /// The list as it was last handed over, and a count of the times it
    /// changed: which files, or in what order.
    paths: Vec<PathBuf>,
    listing: u64,
    /// The rows as the frame last saw them, where each row starts, and
    /// whether anything they were built from has changed since.
    rows: Option<Arc<[Row]>>,
    tops: Arc<[f32]>,
    dirty: bool,
    /// Which thumbnails the rows were built against.
    thumbs_seen: u64,
    reveal: bool,
    visible: Range<usize>,
    /// The width each thumbnail is fitted into, which the panel's width
    /// and every file's row are made from: where its edge was last dragged.
    slot: f32,
}

impl Default for Filmstrip {
    /// The default order, and the list to be put in it at the first
    /// chance.
    fn default() -> Self {
        Self {
            order: Order::default(),
            stale: true,
            paths: Vec::new(),
            listing: 0,
            rows: None,
            tops: Arc::from([0.0]),
            dirty: true,
            thumbs_seen: 0,
            reveal: false,
            visible: 0..0,
            slot: filmstrip::SLOT_DEFAULT,
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

    /// The width each thumbnail is fitted into.
    pub(super) fn slot(&self) -> f32 {
        self.slot
    }

    /// Fits the thumbnails into a slot `slot` wide, held between the
    /// narrowest and the widest the list goes. The rows are laid out again
    /// at it; what stays in place on screen as they grow or shrink is the
    /// panel's to say, since only it knows where the strip was scrolled.
    pub(super) fn set_slot(&mut self, slot: f32) {
        let slot = slot.clamp(filmstrip::SLOT_MIN, filmstrip::SLOT_MAX);
        if slot != self.slot {
            self.slot = slot;
            self.dirty = true;
        }
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
            self.listing += 1;
            self.dirty = true;
        }
    }

    /// A header was read: what a row says of its file may have changed,
    /// and the shape of its slot with it.
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

    /// The file at `row` of the rows as the frame last saw them.
    pub(super) fn path_at(&self, row: usize) -> Option<&Path> {
        let index = self.rows.as_ref()?.get(row)?.index;
        self.paths.get(index - 1).map(PathBuf::as_path)
    }

    /// The files on the rows the frame last said were on screen.
    pub(super) fn on_screen(&self) -> impl Iterator<Item = &Path> {
        self.visible.clone().filter_map(|row| self.path_at(row))
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
    /// `key` is what is known about each file, for its row; `current`
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
        let current = current.and_then(|shown| self.paths.iter().position(|path| path == shown));
        Input {
            rows,
            tops: Arc::clone(&self.tops),
            slot: self.slot,
            listing: self.listing,
            current,
            order: self.order,
            back,
            forward,
            reveal: std::mem::take(&mut self.reveal),
            visible: self.visible.clone(),
        }
    }

    /// The rows, one for each file in the order the list stands in; and
    /// where each row starts, each as tall as its picture's shape makes its
    /// slot.
    fn build(
        &self,
        thumbs: &Thumbs,
        key: impl for<'a> Fn(&'a Path) -> Key<'a>,
    ) -> (Arc<[Row]>, Arc<[f32]>) {
        let rows: Vec<Row> = self
            .paths
            .iter()
            .enumerate()
            .map(|(index, path)| {
                let known = key(path);
                Row {
                    index: index + 1,
                    name: path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string()),
                    path: path.display().to_string(),
                    format: known.format,
                    size: known.size,
                    bytes: known.bytes,
                    modified: known.modified,
                    thumb: thumbs.get(path),
                }
            })
            .collect();
        let mut tops = Vec::with_capacity(rows.len() + 1);
        let mut top = 0.0;
        tops.push(top);
        for row in &rows {
            top += filmstrip::row_height(self.slot, row.size);
            tops.push(top);
        }
        (Arc::from(rows), Arc::from(tops))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thumbnailer::Facts;
    use crate::ui::filmstrip::{Direction, SLOT_DEFAULT, SLOT_MIN, Sort, row_height};
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
            modified: None,
        }
    }

    /// The rows are the list, one row each, in its order, each carrying
    /// what is known of its file; each row starts where the one before it
    /// ended, as tall as its picture's shape and square where that is not
    /// known; and the file on screen is found by its path. A new order is
    /// a new listing.
    #[test]
    fn the_rows_follow_the_list() {
        let mut strip = Filmstrip::default();
        let thumbs = Thumbs::default();
        let list = paths(&["a/1.png", "a/2.jpg", "b/3.png"]);
        strip.relist(&list);
        let facts_known: HashMap<PathBuf, Facts> = [
            (
                PathBuf::from("a/1.png"),
                Facts {
                    size: Some((300, 200)),
                    ..facts(Some("PNG"))
                },
            ),
            (PathBuf::from("a/2.jpg"), facts(Some("JPEG"))),
        ]
        .into_iter()
        .collect();

        let input = strip.input(&thumbs, |path| known(&facts_known, path), Some(Path::new("b/3.png")), false, true);
        assert_eq!(input.rows.len(), 3);
        assert_eq!(
            input.rows[1],
            Row {
                index: 2,
                name: "2.jpg".to_string(),
                path: "a/2.jpg".to_string(),
                format: Some("JPEG"),
                size: None,
                bytes: None,
                modified: None,
                thumb: None,
            }
        );
        assert_eq!(input.rows[2].format, None, "not read yet");
        assert_eq!(input.current, Some(2));
        let (wide, square) = (row_height(SLOT_DEFAULT, Some((300, 200))), row_height(SLOT_DEFAULT, None));
        assert!(wide < square);
        assert_eq!(&*input.tops, &[0.0, wide, wide + square, wide + 2.0 * square]);
        let listing = input.listing;
        assert!(!input.back && input.forward);
        assert_eq!(strip.path_at(1), Some(Path::new("a/2.jpg")));
        assert_eq!(strip.path_at(3), None, "past the end");

        // A sort that reads the headers marks the list stale; put in its
        // order, the rows follow it.
        assert!(strip.set_order(Order {
            sort: Sort::Type,
            direction: Direction::Ascending,
        }));
        assert!(strip.take_stale());
        strip.relist(&paths(&["a/2.jpg", "a/1.png", "b/3.png"]));
        let input = strip.input(&thumbs, |path| known(&facts_known, path), Some(Path::new("b/3.png")), false, false);
        let names: Vec<&str> = input.rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["2.jpg", "1.png", "3.png"]);
        assert_eq!(input.rows[0].index, 1);
        assert_eq!(input.current, Some(2));
        assert_ne!(input.listing, listing);

        // A header read lays the rows out again, but they are the same
        // files in the same order.
        strip.facts_changed();
        let again = strip.input(&thumbs, |path| known(&facts_known, path), None, false, false);
        assert_eq!(again.listing, input.listing);
    }

    /// The slot is held to the range the list goes, and every file's row is
    /// laid out again at it, without a reveal: a reveal on every step of
    /// the drag would snap the strip to the file on screen.
    #[test]
    fn a_new_slot_lays_the_rows_out_again() {
        use crate::ui::filmstrip::SLOT_MAX;
        let mut strip = Filmstrip::default();
        let thumbs = Thumbs::default();
        strip.relist(&paths(&["a.png", "b.png"]));
        let nothing = HashMap::new();
        let input = strip.input(&thumbs, |path| known(&nothing, path), None, false, false);
        assert_eq!(input.slot, SLOT_DEFAULT);

        strip.set_slot(200.0);
        let input = strip.input(&thumbs, |path| known(&nothing, path), None, false, false);
        assert_eq!(input.slot, 200.0);
        assert_eq!(&*input.tops, &[0.0, row_height(200.0, None), 2.0 * row_height(200.0, None)]);
        assert!(!input.reveal);

        strip.set_slot(10_000.0);
        assert_eq!(strip.slot(), SLOT_MAX);
        strip.set_slot(0.0);
        assert_eq!(strip.slot(), SLOT_MIN);
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
