//! The info panel's Tags tab as the application holds it: what exiftool said
//! about each file it has been run on, the run waited for, the filter, which
//! groups are folded, and the rows the tab is drawn from.
//!
//! A run is asked for only while the tab is on screen — `App::request_tags`
//! — and its answer is kept per file, under the file as `stat` saw it when
//! it was asked, so that stepping back to a file shows its tags at once and
//! a file rewritten since is read again. An answer to anything but the last
//! run asked for is kept, but ends no wait.
//!
//! With no query the rows are a tree of the groups the tags are in, in the
//! file's own order. A query makes them a list, best first: tags whose name
//! holds every word of it, then tags holding every word anywhere, and only
//! where neither fits anything the matcher's scattered hits — see
//! [`ranked`]. The rows are built again only
//! when something they are built from has changed, and shared with the frame
//! rather than copied into it.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::exiftool::{Delivered, Failure, Report, Request, Tag, Value};
use crate::fuzzy::{self, Matcher};
use crate::ui::tags::{self, Fold, Input, Kind, Line, Row, State, Table};
use crate::watch::Signature;

/// How many files' tags are kept: a few hundred kilobytes each for a raw's
/// maker notes, and more files than a session steps back through.
pub const MAX_REPORTS: usize = 64;

/// A group of the tree: the kind of metadata, and where in it — or the
/// kind alone, for the group that holds the rest.
type GroupKey = (String, Option<String>);

/// The tags of one kind of metadata, by the group each is in, as the tree
/// is built from them: each tag by its place in the report.
type Kind0<'a> = (&'a str, Vec<(&'a str, Vec<usize>)>);

/// What a run said, as it is kept.
type Outcome = Arc<Result<Report, Failure>>;

/// What a row of the tree stands for, for what a press on it does.
#[derive(Clone, Debug, PartialEq)]
enum RowKey {
    Group(GroupKey),
    /// A tag, by its place in the report.
    Tag(usize),
    /// A piece of a tag holding more than one, by its path.
    Field(usize, String),
}

/// The rows as the frame last saw them, and what they were made from.
struct Built {
    rows: Arc<[Row]>,
    tops: Arc<[f32]>,
    keys: Vec<RowKey>,
    /// The tags that fit the query, best first.
    fitting: Vec<usize>,
    /// Whether every group of the tree is open, and `None` where there is
    /// no tree.
    all_open: Option<bool>,
}

impl Built {
    fn empty() -> Self {
        Self {
            rows: Arc::from([]),
            tops: Arc::from([0.0]),
            keys: Vec::new(),
            fitting: Vec::new(),
            all_open: None,
        }
    }
}

/// A report kept for a file: the run it answered, the file as it was then,
/// and what the run said.
struct Kept {
    asked: u64,
    stamp: Signature,
    outcome: Outcome,
}

pub struct Tags {
    matcher: Box<dyn Matcher>,
    query: String,
    /// Whether a group is open unless it has been flipped, and the groups
    /// flipped since: shut to begin with, and what is opened and shut kept
    /// for the session, from one file to the next.
    open_by_default: bool,
    flipped: HashSet<GroupKey>,
    cache: HashMap<PathBuf, Kept>,
    /// Every file kept, least recently wanted first.
    order: VecDeque<PathBuf>,
    /// The run waited for, and the last one asked for.
    pending: Option<(u64, PathBuf, Signature)>,
    asked: u64,
    /// The file the tab is about, as it was when its tags were wanted.
    showing: Option<(PathBuf, Signature)>,
    built: Option<Built>,
    dirty: bool,
    /// The tag last clicked, marked in the list; and whether the next frame
    /// scrolls it into view.
    marked: Option<RowKey>,
    reveal: bool,
}

impl Default for Tags {
    fn default() -> Self {
        Self::with(fuzzy::default())
    }
}

impl Tags {
    /// Tags filtered with `matcher`.
    pub fn with(matcher: Box<dyn Matcher>) -> Self {
        Self {
            matcher,
            query: String::new(),
            open_by_default: false,
            flipped: HashSet::new(),
            cache: HashMap::new(),
            order: VecDeque::new(),
            pending: None,
            asked: 0,
            showing: None,
            built: None,
            dirty: true,
            marked: None,
            reveal: false,
        }
    }

    /// The tab is about `path`, as `stamp` says it is now: a run to ask
    /// for, unless its tags are kept or already on their way. A kept answer
    /// that the program was not there is asked again: it may be there now.
    pub fn want(&mut self, path: &Path, stamp: Signature) -> Option<Request> {
        let about = Some((path.to_path_buf(), stamp));
        if self.showing != about {
            if self.showing.as_ref().map(|(shown, _)| shown.as_path()) != Some(path) {
                self.marked = None;
            }
            self.showing = about;
            self.dirty = true;
        }
        if let Some(kept) = self.cache.get(path)
            && kept.stamp == stamp
            && !matches!(*kept.outcome, Err(Failure::NotInstalled))
        {
            self.order.retain(|held| held != path);
            self.order.push_back(path.to_path_buf());
            return None;
        }
        if let Some((_, waited, waited_stamp)) = &self.pending
            && waited == path
            && *waited_stamp == stamp
        {
            return None;
        }
        self.asked += 1;
        self.pending = Some((self.asked, path.to_path_buf(), stamp));
        self.dirty = true;
        Some(Request {
            asked: self.asked,
            path: path.to_path_buf(),
            stamp,
        })
    }

    /// Takes in what a run said, keeping it for its file unless a later run
    /// of the same file has already been kept. Says whether it is about the
    /// file the tab is about.
    pub fn take(&mut self, delivered: Delivered) -> bool {
        let Delivered {
            asked,
            path,
            stamp,
            outcome,
        } = delivered;
        if self
            .pending
            .as_ref()
            .is_some_and(|(waited, _, _)| *waited == asked)
        {
            self.pending = None;
        }
        if self.cache.get(&path).is_some_and(|kept| kept.asked > asked) {
            return false;
        }
        self.order.retain(|held| *held != path);
        self.order.push_back(path.clone());
        self.cache.insert(
            path.clone(),
            Kept {
                asked,
                stamp,
                outcome: Arc::new(outcome),
            },
        );
        while self.order.len() > MAX_REPORTS {
            if let Some(oldest) = self.order.pop_front() {
                self.cache.remove(&oldest);
            }
        }
        let about = self.showing.as_ref() == Some(&(path, stamp));
        self.dirty |= about;
        about
    }

    pub fn set_query(&mut self, query: String) {
        if self.query != query {
            self.query = query;
            self.dirty = true;
        }
    }

    /// Folds the group at `row` shut, or opens it, for the session.
    pub fn toggle_group(&mut self, row: usize) {
        let Some(RowKey::Group(key)) = self.key_at(row).cloned() else {
            return;
        };
        if !self.flipped.remove(&key) {
            self.flipped.insert(key);
        }
        self.dirty = true;
    }

    /// Opens every group, or folds them all shut: this file's, and every
    /// file's after it until one is flipped again.
    pub fn fold(&mut self, fold: Fold) {
        self.open_by_default = fold == Fold::Open;
        self.flipped.clear();
        self.dirty = true;
    }

    /// A tag or a piece of one at `row` was clicked, and its value copied:
    /// it is marked, and where it was a hit of a query, the query is put
    /// away and the tag shown in the tree — its groups opened where they
    /// were shut, and nothing else — scrolled into view.
    pub fn choose(&mut self, row: usize) {
        let Some(key) = self.key_at(row).cloned() else {
            return;
        };
        let index = match &key {
            RowKey::Group(_) => return,
            RowKey::Tag(index) | RowKey::Field(index, _) => *index,
        };
        self.marked = Some(key);
        self.dirty = true;
        if self.query.is_empty() {
            return;
        }
        let Some(outcome) = self.report() else {
            return;
        };
        let Ok(report) = outcome.as_ref() else {
            return;
        };
        let Some(tag) = report.tags.get(index) else {
            return;
        };
        self.query.clear();
        for key in [
            (tag.family0.clone(), None),
            (tag.family0.clone(), Some(tag.family1.clone())),
        ] {
            if !self.is_open(&key) && !self.flipped.remove(&key) {
                self.flipped.insert(key);
            }
        }
        self.reveal = true;
    }

    fn is_open(&self, key: &GroupKey) -> bool {
        self.open_by_default != self.flipped.contains(key)
    }

    /// Whether a query is up, whose rows are a list of hits rather than the
    /// tree.
    pub fn filtering(&self) -> bool {
        !self.query.is_empty()
    }

    /// Whether the tags of the file the tab is about are in.
    pub fn tags_in(&self) -> bool {
        self.report().is_some()
    }

    /// How many runs have been asked for.
    #[cfg(test)]
    pub fn asked(&self) -> u64 {
        self.asked
    }

    fn key_at(&self, row: usize) -> Option<&RowKey> {
        self.built.as_ref()?.keys.get(row)
    }

    /// What a click on the row at `row` copies: a tag's value as it is
    /// printed, or one piece of it. `None` for a group.
    pub fn value_at(&self, row: usize) -> Option<String> {
        let outcome = self.report()?;
        let report = outcome.as_ref().as_ref().ok()?;
        match self.key_at(row)? {
            RowKey::Group(_) => None,
            RowKey::Tag(index) => Some(whole(&report.tags.get(*index)?.printed)),
            RowKey::Field(index, path) => report
                .tags
                .get(*index)?
                .printed
                .pieces()
                .into_iter()
                .find(|(at, _)| at == path)
                .map(|(_, text)| text),
        }
    }

    /// What the tab's two buttons copy — the tags that fit the query,
    /// whatever is folded — and how many tags that is. `None` with no tags
    /// in.
    pub fn copied(&mut self, table: Table) -> Option<(String, usize)> {
        self.refresh();
        let outcome = self.report()?;
        let report = outcome.as_ref().as_ref().ok()?;
        let fitting = &self.built.as_ref()?.fitting;
        let chosen: Vec<&Tag> = fitting.iter().map(|&index| &report.tags[index]).collect();
        let lines = || {
            chosen
                .iter()
                .flat_map(|tag| lines(tag))
                .collect::<Vec<Line>>()
        };
        let copied = match table {
            Table::Text => tags::text(&lines()),
            Table::Csv => tags::csv(&lines()),
            Table::Json => json(&chosen),
            Table::Xml => xml(&report.version, &chosen),
        };
        Some((copied, fitting.len()))
    }

    /// What the tab is drawn from. `file` names the file the tags are of,
    /// and `configured` what exiftool was looked for as.
    pub fn input(&mut self, file: &str, configured: &str) -> Input {
        self.refresh();
        let built = self.built.as_ref().expect("built above");
        let marked = self
            .marked
            .as_ref()
            .and_then(|marked| built.keys.iter().position(|key| key == marked));
        let reveal = std::mem::take(&mut self.reveal);
        let (state, version, total) = match self.outcome().as_deref() {
            None => (State::Waiting, None, 0),
            Some(Ok(report)) => (
                State::Ready,
                Some(report.version.clone()),
                report.tags.len(),
            ),
            Some(Err(Failure::NotInstalled)) => (
                State::NotInstalled {
                    configured: configured.to_string(),
                },
                None,
                0,
            ),
            Some(Err(Failure::Failed(said) | Failure::Unreadable(said))) => {
                (State::Failed(said.clone()), None, 0)
            }
        };
        Input {
            file: file.to_string(),
            query: self.query.clone(),
            version,
            state,
            rows: Arc::clone(&built.rows),
            tops: Arc::clone(&built.tops),
            shown: built.fitting.len(),
            total,
            fold: built
                .all_open
                .map(|open| if open { Fold::Shut } else { Fold::Open }),
            marked,
            reveal,
        }
    }

    /// What the last run for the file the tab is about said, where it was
    /// of the file as it is now.
    fn outcome(&self) -> Option<Outcome> {
        let (path, stamp) = self.showing.as_ref()?;
        let kept = self.cache.get(path)?;
        (kept.stamp == *stamp).then(|| Arc::clone(&kept.outcome))
    }

    /// The same, where it is tags.
    fn report(&self) -> Option<Outcome> {
        self.outcome().filter(|outcome| outcome.is_ok())
    }

    fn refresh(&mut self) {
        if self.dirty || self.built.is_none() {
            self.built = Some(self.build());
            self.dirty = false;
        }
    }

    fn build(&self) -> Built {
        let Some(outcome) = self.report() else {
            return Built::empty();
        };
        let Ok(report) = outcome.as_ref() else {
            return Built::empty();
        };
        let candidates: Vec<String> = report.tags.iter().map(candidate).collect();
        let mut rows = Vec::new();
        let mut keys = Vec::new();
        let mut all_open = None;
        let fitting = if self.query.is_empty() {
            all_open = Some(self.tree(report, &mut rows, &mut keys));
            (0..report.tags.len()).collect()
        } else {
            let matches = ranked(self.matcher.as_ref(), &self.query, report, &candidates);
            for (index, positions) in &matches {
                let tag = &report.tags[*index];
                tag_rows(tag, *index, positions, 0, true, &mut rows, &mut keys);
            }
            matches.into_iter().map(|(index, _)| index).collect()
        };
        let tops = tags::tops(&rows);
        Built {
            rows: rows.into(),
            tops: tops.into(),
            keys,
            fitting,
            all_open,
        }
    }

    /// Every tag as a tree: each kind of metadata, the groups in it, and
    /// their tags, in the order each first appears. Says whether every
    /// group is open.
    fn tree(&self, report: &Report, rows: &mut Vec<Row>, keys: &mut Vec<RowKey>) -> bool {
        let mut all_open = true;
        let mut groups: Vec<Kind0> = Vec::new();
        for (index, tag) in report.tags.iter().enumerate() {
            let place = match groups.iter().position(|(name, _)| *name == tag.family0) {
                Some(place) => place,
                None => {
                    groups.push((&tag.family0, Vec::new()));
                    groups.len() - 1
                }
            };
            let inner = &mut groups[place].1;
            let at = match inner.iter().position(|(name, _)| *name == tag.family1) {
                Some(at) => at,
                None => {
                    inner.push((&tag.family1, Vec::new()));
                    inner.len() - 1
                }
            };
            inner[at].1.push(index);
        }
        for (family0, inner) in &groups {
            let key = (family0.to_string(), None);
            let shut = !self.is_open(&key);
            all_open &= !shut;
            let count = inner.iter().map(|(_, members)| members.len()).sum();
            rows.push(group(family0, 0, shut, count));
            keys.push(RowKey::Group(key));
            if shut {
                continue;
            }
            // A kind of metadata with only a group of its own name is that
            // group: the level between would say nothing.
            let flat = matches!(inner.as_slice(), [(only, _)] if only == family0);
            for (family1, members) in inner {
                let depth = if flat {
                    1
                } else {
                    let key = (family0.to_string(), Some(family1.to_string()));
                    let shut = !self.is_open(&key);
                    all_open &= !shut;
                    rows.push(group(family1, 1, shut, members.len()));
                    keys.push(RowKey::Group(key));
                    if shut {
                        continue;
                    }
                    2
                };
                for &index in members {
                    tag_rows(&report.tags[index], index, &[], depth, false, rows, keys);
                }
            }
        }
        all_open
    }
}

/// Which tags fit `query`, best first, each with the chars of its
/// [`candidate`] to light.
///
/// A tag whose title holds every word of the query comes first, then a tag
/// holding every word somewhere — its name, a value — each lit where
/// the words are; among equals, the matcher's score, then the file's order.
/// The matcher's scattered hits, which in a few hundred long candidates
/// fit nearly anything, are kept only where nothing holds the words
/// outright: a query that names a tag should find that tag, not bury it.
fn ranked(
    matcher: &dyn Matcher,
    query: &str,
    report: &Report,
    candidates: &[String],
) -> Vec<(usize, Vec<usize>)> {
    let words: Vec<Vec<char>> = query
        .split_whitespace()
        .map(|word| word.chars().collect())
        .collect();
    let mut scored: Vec<(u8, i64, usize, Vec<usize>)> = Vec::new();
    for (index, (tag, candidate)) in report.tags.iter().zip(candidates).enumerate() {
        let fuzzy = matcher.fuzzy_indices(candidate, query);
        let score = fuzzy.as_ref().map_or(i64::MIN, |(score, _)| *score);
        let title: Vec<char> = title(tag).chars().collect();
        let whole: Vec<char> = candidate.chars().collect();
        let (title_from, _, _) = spans(tag);
        let (tier, positions) = if let Some(lit) = held(&title, &words) {
            (2, lit.into_iter().map(|at| at + title_from).collect())
        } else if let Some(lit) = held(&whole, &words) {
            (1, lit)
        } else if let Some((_, positions)) = fuzzy {
            (0, positions)
        } else {
            continue;
        };
        scored.push((tier, score, index, positions));
    }
    if scored.iter().any(|(tier, ..)| *tier > 0) {
        scored.retain(|(tier, ..)| *tier > 0);
    }
    scored.sort_by_key(|(tier, score, index, _)| {
        (std::cmp::Reverse(*tier), std::cmp::Reverse(*score), *index)
    });
    scored
        .into_iter()
        .map(|(_, _, index, positions)| (index, positions))
        .collect()
}

/// The chars of `text` where each of `words` is first found whole, ignoring
/// case, or `None` where one of them is not there. No words is nothing.
fn held(text: &[char], words: &[Vec<char>]) -> Option<Vec<usize>> {
    if words.is_empty() {
        return None;
    }
    let same = |a: char, b: char| a == b || a.to_lowercase().eq(b.to_lowercase());
    let mut lit = Vec::new();
    for word in words {
        let at = (0..=text.len().checked_sub(word.len())?).find(|&start| {
            word.iter()
                .enumerate()
                .all(|(offset, &c)| same(text[start + offset], c))
        })?;
        lit.extend(at..at + word.len());
    }
    lit.sort_unstable();
    lit.dedup();
    Some(lit)
}

/// What a tag's row is headed by: exiftool's description of it, or its name
/// where exiftool has no words for it.
fn title(tag: &Tag) -> &str {
    if tag.desc.is_empty() {
        &tag.name
    } else {
        &tag.desc
    }
}

/// Where the title starts in a tag's [`candidate`], where it ends, and
/// where the value after the name starts.
fn spans(tag: &Tag) -> (usize, usize, usize) {
    let title_from = tag.family1.chars().count() + 1;
    let title_to = title_from + title(tag).chars().count();
    (
        title_from,
        title_to,
        title_to + 1 + tag.name.chars().count() + 1,
    )
}

/// What the filter is matched against for `tag`: its group, its title, its
/// name, its value, its raw value and its ID, a space between each, so that
/// a query can span them. The title and the value come early, so that
/// where they start is easy to say — see [`spans`].
fn candidate(tag: &Tag) -> String {
    let joined = |value: &Value| {
        value
            .leaves()
            .iter()
            .map(|(_, text)| *text)
            .collect::<Vec<_>>()
            .join(" ")
    };
    format!(
        "{} {} {} {} {} {}",
        tag.family1,
        title(tag),
        tag.name,
        joined(&tag.printed),
        tag.raw.as_ref().map(joined).unwrap_or_default(),
        tag.id.as_deref().unwrap_or_default()
    )
}

fn group(name: &str, depth: u8, collapsed: bool, count: usize) -> Row {
    Row {
        kind: Kind::Group,
        depth,
        name: name.to_string(),
        value: String::new(),
        raw: None,
        group: None,
        name_lit: Vec::new(),
        value_lit: Vec::new(),
        collapsed,
        count,
    }
}

/// The rows for one tag: the tag, and where it holds a structure or a list
/// of more than text, each piece of it under it, one level further in.
/// `positions` are where the query was found in its [`candidate`];
/// `grouped` says which group it is in on its row, for a list that has no
/// tree to say so.
fn tag_rows(
    tag: &Tag,
    index: usize,
    positions: &[usize],
    depth: u8,
    grouped: bool,
    rows: &mut Vec<Row>,
    keys: &mut Vec<RowKey>,
) {
    let (title_from, title_to, value_from) = spans(tag);
    let within = |from: usize, to: usize| -> Vec<usize> {
        positions
            .iter()
            .filter(|&&at| at >= from && at < to)
            .map(|at| at - from)
            .collect()
    };
    let value_lit = match &tag.printed {
        Value::Text(text) => within(value_from, value_from + text.chars().count()),
        Value::List(_) | Value::Struct(_) => Vec::new(),
    };
    rows.push(Row {
        kind: Kind::Tag,
        depth,
        name: title(tag).to_string(),
        value: tag.printed.summary(),
        raw: tag.raw.as_ref().map(Value::summary),
        group: grouped.then(|| tag.family1.clone()),
        name_lit: within(title_from, title_to),
        value_lit,
        collapsed: false,
        count: 0,
    });
    keys.push(RowKey::Tag(index));
    if tag.printed.is_flat() {
        return;
    }
    for (path, text) in tag.printed.pieces() {
        rows.push(Row {
            kind: Kind::Field,
            depth: depth + 1,
            name: path.clone(),
            value: text,
            raw: None,
            group: None,
            name_lit: Vec::new(),
            value_lit: Vec::new(),
            collapsed: false,
            count: 0,
        });
        keys.push(RowKey::Field(index, path));
    }
}

/// A whole value as one piece of text: its text, a list of text as the row
/// writes it, and anything deeper a line for each piece, by its path.
fn whole(value: &Value) -> String {
    if value.is_flat() {
        return value.summary();
    }
    value
        .pieces()
        .into_iter()
        .map(|(path, text)| format!("{path}: {text}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The lines a copy of `tag` makes: one, or one for each piece of a tag
/// holding more than one, named by the tag's name and the piece's path.
fn lines(tag: &Tag) -> Vec<Line> {
    let line = |name: String, value: &str, raw: String| Line {
        group: tag.family0.clone(),
        subgroup: tag.family1.clone(),
        name,
        id: tag.id.clone().unwrap_or_default(),
        description: tag.desc.clone(),
        value: value.to_string(),
        raw,
    };
    if tag.printed.is_flat() {
        let raw = tag.raw.as_ref().map(whole).unwrap_or_default();
        return vec![line(tag.name.clone(), &tag.printed.summary(), raw)];
    }
    let raw_pieces = tag.raw.as_ref().map(Value::pieces).unwrap_or_default();
    let raw_at = |path: &str| {
        raw_pieces
            .iter()
            .find(|(at, _)| at == path)
            .map(|(_, text)| text.clone())
            .unwrap_or_default()
    };
    tag.printed
        .pieces()
        .into_iter()
        .map(|(path, text)| line(format!("{}.{path}", tag.name), &text, raw_at(&path)))
        .collect()
}

/// The heads a copy names each part of a tag by, in JSON and XML as in CSV.
const HEADS: [&str; 7] = [
    "Group",
    "Subgroup",
    "Tag",
    "ID",
    "Description",
    "Value",
    "Raw",
];

/// `tags` as a JSON array, an object to a line, each value kept in its own
/// shape: text a string, a list an array, a structure an object. What is
/// not there — an ID, a raw value — is left out rather than written null.
fn json(tags: &[&Tag]) -> String {
    let [group, subgroup, name, id, description, value, raw] = HEADS.map(json_string);
    let objects: Vec<String> = tags
        .iter()
        .map(|tag| {
            let mut fields = vec![
                format!("{group}: {}", json_string(&tag.family0)),
                format!("{subgroup}: {}", json_string(&tag.family1)),
                format!("{name}: {}", json_string(&tag.name)),
            ];
            if let Some(tag_id) = &tag.id {
                fields.push(format!("{id}: {}", json_string(tag_id)));
            }
            fields.push(format!("{description}: {}", json_string(&tag.desc)));
            fields.push(format!("{value}: {}", json_value(&tag.printed)));
            if let Some(tag_raw) = &tag.raw {
                fields.push(format!("{raw}: {}", json_value(tag_raw)));
            }
            format!("  {{{}}}", fields.join(", "))
        })
        .collect();
    if objects.is_empty() {
        return "[]".to_string();
    }
    format!("[\n{}\n]", objects.join(",\n"))
}

fn json_value(value: &Value) -> String {
    match value {
        Value::Text(text) => json_string(text),
        Value::List(items) => {
            let items: Vec<String> = items.iter().map(json_value).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Struct(fields) => {
            let fields: Vec<String> = fields
                .iter()
                .map(|(name, value)| format!("{}: {}", json_string(name), json_value(value)))
                .collect();
            format!("{{{}}}", fields.join(", "))
        }
    }
}

/// `text` as a JSON string: quoted, with a quote, a backslash and anything
/// below a space escaped.
fn json_string(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for c in text.chars() {
        match c {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            c if u32::from(c) < 0x20 => quoted.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

/// `tags` as XML: a `tag` element each, its groups, name, ID and
/// description as attributes, and its value and raw value as elements
/// holding text, an `item` each for a list, or a `field` each for a
/// structure.
fn xml(version: &str, tags: &[&Tag]) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str(&format!("<tags exiftool=\"{}\">\n", xml_escape(version)));
    for tag in tags {
        let mut attributes = vec![
            ("group", tag.family0.as_str()),
            ("subgroup", tag.family1.as_str()),
            ("name", tag.name.as_str()),
        ];
        if let Some(id) = &tag.id {
            attributes.push(("id", id));
        }
        attributes.push(("description", &tag.desc));
        let attributes: Vec<String> = attributes
            .iter()
            .map(|(name, value)| format!("{name}=\"{}\"", xml_escape(value)))
            .collect();
        out.push_str(&format!("  <tag {}>\n", attributes.join(" ")));
        xml_element(&mut out, "value", None, &tag.printed, 2);
        if let Some(raw) = &tag.raw {
            xml_element(&mut out, "raw", None, raw, 2);
        }
        out.push_str("  </tag>\n");
    }
    out.push_str("</tags>");
    out
}

/// One element called `name` holding `value`, `depth` levels in, carrying
/// the field's name where it is a structure's field.
fn xml_element(out: &mut String, name: &str, field: Option<&str>, value: &Value, depth: usize) {
    let indent = "  ".repeat(depth);
    let attribute = field.map_or(String::new(), |field| {
        format!(" name=\"{}\"", xml_escape(field))
    });
    match value {
        Value::Text(text) => out.push_str(&format!(
            "{indent}<{name}{attribute}>{}</{name}>\n",
            xml_escape(text)
        )),
        Value::List(items) => {
            out.push_str(&format!("{indent}<{name}{attribute}>\n"));
            for item in items {
                xml_element(out, "item", None, item, depth + 1);
            }
            out.push_str(&format!("{indent}</{name}>\n"));
        }
        Value::Struct(fields) => {
            out.push_str(&format!("{indent}<{name}{attribute}>\n"));
            for (field, value) in fields {
                xml_element(out, "field", Some(field), value, depth + 1);
            }
            out.push_str(&format!("{indent}</{name}>\n"));
        }
    }
}

/// `text` safe inside an element or a quoted attribute.
fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests;
