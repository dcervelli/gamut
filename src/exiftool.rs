//! Every tag a file carries, as `exiftool` reads it: the info panel's Tags
//! tab — see `docs/tags.md`.
//!
//! A process rather than a library. `exiftool` reads more formats and more
//! makers' notes than anything in this tree will, and it is run only while
//! the tab is on screen, once per file. It is asked for RDF/XML, which keeps
//! a tag that appears twice and which `roxmltree`, already here for the XMP
//! packet, parses; its JSON drops the second of two tags of one name.
//!
//! [`parse`] is pure; [`run`] is the process, and [`run_on_thread`] the
//! thread it is waited for on, which hands the answer back through the event
//! loop. [`Program`] is where the program was found.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use roxmltree::{Document, Node};

use crate::watch::Signature;

/// The arguments before the path. `-X` is RDF/XML; `-l` adds each tag's
/// description, its printed value and, where it differs, its raw one; `-D`
/// and `-t` its ID and the table it is defined in; `-G1` names each tag by
/// the group it is in; `-a` keeps duplicates, `-u` the tags exiftool has no
/// name for; `-struct` writes a structure as one, rather than flattened
/// into tags of its own. `-s` is not among them: it takes `-l`'s words away.
const ARGS: &[&str] = &[
    "-X",
    "-l",
    "-D",
    "-t",
    "-G1",
    "-a",
    "-u",
    "-struct",
    "-api",
    "LargeFileSupport=1",
];

const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const ET: &str = "http://ns.exiftool.org/1.0/";
const XML: &str = "http://www.w3.org/XML/1998/namespace";
/// What every tag's namespace starts with; the groups follow, then the
/// version.
const NS_PREFIX: &str = "http://ns.exiftool.org/";
const NS_SUFFIX: &str = "/1.0/";
/// Where exiftool is from, which the tab offers to open while it is not
/// found.
pub const WEBSITE: &str = "https://exiftool.org";

/// What `et:toolkit` says before the version.
const TOOLKIT: &str = "Image::ExifTool ";

/// The directories a bare name is looked for in after `PATH`: where the
/// program is put by the packages that do not put it on a PATH a program
/// started from the desktop sees. A Mac app started from Finder or the Dock
/// gets none of the shell's PATH at all.
#[cfg(target_os = "linux")]
const EXTRA_DIRS: &[&str] = &["/usr/bin/vendor_perl"];
#[cfg(target_os = "macos")]
const EXTRA_DIRS: &[&str] = &["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"];

/// What exiftool read from one file.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    /// Its version, as it says it: `13.55`.
    pub version: String,
    /// Every tag, in the order exiftool wrote them, duplicates kept.
    pub tags: Vec<Tag>,
}

/// One tag.
#[derive(Clone, Debug, PartialEq)]
pub struct Tag {
    /// The two groups it is in: the kind of metadata — `EXIF`, `XMP`,
    /// `MakerNotes`, `Composite` — and where in it — `IFD0`, `XMP-dc`.
    /// The same where exiftool gives only one.
    pub family0: String,
    pub family1: String,
    /// Its name: `Orientation`, `Title-fr`.
    pub name: String,
    /// Its ID where it has one — a TIFF tag's number, an XMP property's
    /// name — and the table it is defined in.
    pub id: Option<String>,
    pub table: Option<String>,
    /// The language of one entry of a language alternative.
    pub lang: Option<String>,
    /// What exiftool calls it in words.
    pub desc: String,
    /// Its value as exiftool prints it, and the raw value where that is
    /// something else.
    pub printed: Value,
    pub raw: Option<Value>,
}

/// A tag's value: text, or a list or a structure of values.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Text(String),
    List(Vec<Value>),
    Struct(Vec<(String, Value)>),
}

impl Value {
    /// Every piece of text in it, with the dotted path to it — the field's
    /// names, and a list's places counted from one — the whole value's
    /// being empty: `RegionList.1.Name`.
    pub fn leaves(&self) -> Vec<(String, &str)> {
        let mut leaves = Vec::new();
        self.gather(String::new(), &mut leaves);
        leaves
    }

    fn gather<'a>(&'a self, path: String, leaves: &mut Vec<(String, &'a str)>) {
        let under = |name: &str| {
            if path.is_empty() {
                name.to_string()
            } else {
                format!("{path}.{name}")
            }
        };
        match self {
            Value::Text(text) => leaves.push((path, text)),
            Value::List(items) => {
                for (place, item) in items.iter().enumerate() {
                    item.gather(under(&(place + 1).to_string()), leaves);
                }
            }
            Value::Struct(fields) => {
                for (name, value) in fields {
                    value.gather(under(name), leaves);
                }
            }
        }
    }

    /// The value on one line: the text, a list of text written whole —
    /// `[1, 2, 3]` — or how many things anything deeper holds.
    pub fn summary(&self) -> String {
        match self {
            Value::Text(text) => text.clone(),
            Value::List(items) if self.is_flat() => {
                let texts: Vec<_> = items.iter().map(Value::summary).collect();
                format!("[{}]", texts.join(", "))
            }
            Value::List(items) => count(items.len(), "item"),
            Value::Struct(fields) => count(fields.len(), "field"),
        }
    }

    /// Whether it is said whole on one line: text, or a list of text.
    pub fn is_flat(&self) -> bool {
        match self {
            Value::Text(_) => true,
            Value::List(items) => items.iter().all(|item| matches!(item, Value::Text(_))),
            Value::Struct(_) => false,
        }
    }

    /// Each piece of it said on a line of its own, with the dotted path to
    /// it, as [`Value::leaves`] but with a list of text kept whole and
    /// written as [`Value::summary`] writes it: the rows under a tag, and
    /// the lines of a copy of one.
    pub fn pieces(&self) -> Vec<(String, String)> {
        let mut pieces = Vec::new();
        self.gather_pieces(String::new(), &mut pieces);
        pieces
    }

    fn gather_pieces(&self, path: String, pieces: &mut Vec<(String, String)>) {
        let under = |name: &str| {
            if path.is_empty() {
                name.to_string()
            } else {
                format!("{path}.{name}")
            }
        };
        match self {
            Value::List(items) if !self.is_flat() => {
                for (place, item) in items.iter().enumerate() {
                    item.gather_pieces(under(&(place + 1).to_string()), pieces);
                }
            }
            Value::Struct(fields) => {
                for (name, value) in fields {
                    value.gather_pieces(under(name), pieces);
                }
            }
            Value::Text(_) | Value::List(_) => pieces.push((path, self.summary())),
        }
    }
}

fn count(count: usize, word: &str) -> String {
    if count == 1 {
        format!("1 {word}")
    } else {
        format!("{count} {word}s")
    }
}

/// Why there is no report.
#[derive(Clone, Debug, PartialEq)]
pub enum Failure {
    /// The program is not there to run.
    NotInstalled,
    /// It ran and said no: the first line it wrote to its error stream, or
    /// how it ended where it wrote none.
    Failed(String),
    /// It ran and said yes, in something that could not be read.
    Unreadable(String),
}

/// Reads exiftool's `-X` output.
pub fn parse(xml: &str) -> Result<Report, String> {
    let document = Document::parse(xml).map_err(|error| error.to_string())?;
    let root = document.root_element();
    if !root.has_tag_name((RDF, "RDF")) {
        return Err("not RDF".to_string());
    }
    let description = root
        .children()
        .find(|node| {
            node.has_tag_name((RDF, "Description")) && node.attribute((ET, "toolkit")).is_some()
        })
        .ok_or_else(|| "no description of the file".to_string())?;
    let toolkit = description.attribute((ET, "toolkit")).unwrap_or_default();
    let version = toolkit.strip_prefix(TOOLKIT).unwrap_or(toolkit).to_string();
    let tags = description
        .children()
        .filter(Node::is_element)
        .map(tag)
        .collect();
    Ok(Report { version, tags })
}

/// One tag's element: its name and groups from the element itself, the rest
/// from the description inside it.
fn tag(node: Node) -> Tag {
    let (family0, family1) = families(node.tag_name().namespace().unwrap_or_default());
    let wrapper = node
        .children()
        .find(|child| child.has_tag_name((RDF, "Description")));
    let attribute = |name: (&str, &str)| {
        wrapper
            .and_then(|wrapper| wrapper.attribute(name))
            .map(str::to_string)
    };
    let child = |name: &str| {
        wrapper.and_then(|wrapper| {
            wrapper
                .children()
                .find(|child| child.has_tag_name((ET, name)))
        })
    };
    Tag {
        family0,
        family1,
        name: node.tag_name().name().to_string(),
        id: attribute((ET, "id")),
        table: attribute((ET, "table")),
        lang: attribute((XML, "lang")),
        desc: child("desc")
            .and_then(|desc| desc.text())
            .unwrap_or_default()
            .to_string(),
        printed: child("prt").map_or(Value::Text(String::new()), value),
        raw: child("val").map(value),
    }
}

/// The two groups a tag's namespace names: `http://ns.exiftool.org/EXIF/IFD0/1.0/`
/// is `EXIF` and `IFD0`; one naming only one group is that group twice.
fn families(namespace: &str) -> (String, String) {
    let groups = namespace
        .strip_prefix(NS_PREFIX)
        .unwrap_or(namespace)
        .strip_suffix(NS_SUFFIX)
        .unwrap_or(namespace);
    let mut parts = groups.split('/');
    let family0 = parts.next().unwrap_or_default().to_string();
    let family1 = parts.next().map_or_else(|| family0.clone(), str::to_string);
    (family0, family1)
}

/// A value's element: a structure where it says it is a resource, a list
/// where it holds one, and its text otherwise.
fn value(node: Node) -> Value {
    if node.attribute((RDF, "parseType")) == Some("Resource") {
        return Value::Struct(
            node.children()
                .filter(Node::is_element)
                .map(|field| (field.tag_name().name().to_string(), value(field)))
                .collect(),
        );
    }
    let mut elements = node.children().filter(Node::is_element);
    if let (Some(list), None) = (elements.next(), elements.next())
        && ["Bag", "Seq", "Alt"]
            .iter()
            .any(|kind| list.has_tag_name((RDF, *kind)))
    {
        return Value::List(
            list.children()
                .filter(|item| item.has_tag_name((RDF, "li")))
                .map(value)
                .collect(),
        );
    }
    Value::Text(node.text().unwrap_or_default().to_string())
}

/// Where the program is, as the configuration names it: a path is taken as
/// it is, and a bare name looked for in `PATH` and then in the directories
/// the desktop's PATH misses.
pub struct Program {
    configured: String,
    found: Option<PathBuf>,
}

impl Program {
    pub fn new(configured: &str) -> Self {
        Self {
            configured: configured.to_string(),
            found: None,
        }
    }

    /// The program taken to be at `path`, without looking.
    #[cfg(test)]
    pub fn at(path: &Path) -> Self {
        Self {
            configured: path.display().to_string(),
            found: Some(path.to_path_buf()),
        }
    }

    /// What the configuration named, for saying what was not found.
    pub fn configured(&self) -> &str {
        &self.configured
    }

    /// The program, looked for again where it has not been found yet: it
    /// may have been installed since. A few `stat`s.
    pub fn locate(&mut self) -> Option<&Path> {
        if self.found.is_none() {
            let path = std::env::var_os("PATH").unwrap_or_default();
            let dirs = std::env::split_paths(&path)
                .chain(EXTRA_DIRS.iter().map(PathBuf::from))
                .collect::<Vec<_>>();
            self.found = locate_in(&self.configured, &dirs);
        }
        self.found.as_deref()
    }

    /// Whether it has been found, as of the last look.
    pub fn found(&self) -> bool {
        self.found.is_some()
    }

    /// Forgets where it was found, for a spawn that found nothing there.
    pub fn lost(&mut self) {
        self.found = None;
    }
}

/// `configured` taken as a path where it has more than one component, and
/// looked for in each of `dirs` in turn where it is a bare name.
fn locate_in(configured: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let named = Path::new(configured);
    if configured.is_empty() {
        return None;
    }
    if named.components().count() > 1 {
        return named.is_file().then(|| named.to_path_buf());
    }
    dirs.iter()
        .map(|dir| dir.join(named))
        .find(|candidate| candidate.is_file())
}

/// Runs `program` on `path` and reads what it says. No shell, and the path
/// made absolute — though not resolved, so that a missing file is
/// exiftool's to say — so that it cannot be read as an option.
pub fn run(program: &Path, path: &Path) -> Result<Report, Failure> {
    let path = std::path::absolute(path).map_err(|error| Failure::Failed(error.to_string()))?;
    let output = Command::new(program)
        .args(ARGS)
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => Failure::NotInstalled,
            _ => Failure::Failed(error.to_string()),
        })?;
    // A damaged file is still read, and what is wrong with it arrives as
    // tags as well as on the error stream: only a refusal is read from
    // there.
    if !output.status.success() {
        let said = String::from_utf8_lossy(&output.stderr);
        let first = said
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map_or_else(|| output.status.to_string(), str::to_string);
        return Err(Failure::Failed(first));
    }
    parse(&String::from_utf8_lossy(&output.stdout)).map_err(Failure::Unreadable)
}

/// A run asked for: which asking it answers, and the file as it was when
/// asked.
pub struct Request {
    pub asked: u64,
    pub path: PathBuf,
    pub stamp: Signature,
}

/// A run's answer, on its way to the loop.
pub struct Delivered {
    pub asked: u64,
    pub path: PathBuf,
    pub stamp: Signature,
    pub outcome: Result<Report, Failure>,
}

/// How an answer reaches the loop.
pub type Deliver = Arc<dyn Fn(Delivered) + Send + Sync>;

/// Runs `program` on the requested file on a thread of its own, one per
/// run, and hands the answer to `deliver`. There is no timeout: a run
/// superseded while it waits is dropped by its `asked` when it lands.
pub fn run_on_thread(program: PathBuf, request: Request, deliver: Deliver) {
    let Request { asked, path, stamp } = request;
    let (asking, answer) = (path.clone(), Arc::clone(&deliver));
    let spawned = std::thread::Builder::new()
        .name("exiftool".into())
        .spawn(move || {
            let outcome = run(&program, &path);
            deliver(Delivered {
                asked,
                path,
                stamp,
                outcome,
            });
        });
    // Answered all the same, so that the tab does not wait on a run that
    // never started.
    if let Err(error) = spawned {
        eprintln!("{}: starting exiftool: {error}", crate::PROGRAM);
        answer(Delivered {
            asked,
            path: asking,
            stamp,
            outcome: Err(Failure::Failed(error.to_string())),
        });
    }
}

#[cfg(test)]
mod tests;
