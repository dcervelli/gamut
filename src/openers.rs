//! What else on the desktop can open the file on screen, and starting one of
//! them.
//!
//! The desktop already keeps this answer. Every installed program ships a
//! desktop entry naming the MIME types it will open, `update-desktop-database`
//! indexes those into a `mimeinfo.cache` beside them, and the user's own
//! `mimeapps.list` says which of them is the default and which associations
//! they have added or taken away. Reading those files is what fills the menu
//! under the open button — it is the same list a file manager's "Open With"
//! shows, from the same files, because there is nowhere else the answer
//! lives.
//!
//! Nothing here shells out to find it. `xdg-open` knows only the one default
//! program and would not fill a menu; `gio open` would be a runtime
//! dependency on a package the user may not have. The files are plain text,
//! and reading them costs a millisecond or two when a file goes on screen.
//!
//! What is deliberately not honored, each because acting on it would need
//! something this program does not have:
//!
//! - **D-Bus activation.** An entry marked `DBusActivatable` is started
//!   through its `Exec` line like any other, which the specification requires
//!   entries to keep for exactly this reason. Speaking the activation
//!   protocol would mean linking a D-Bus client for one menu.
//! - **Entries that want a terminal.** `Terminal=true` asks to be run inside
//!   one, and there is no terminal here to run it in — nor any way to know
//!   which the user would want.
//! - **`%i`, and the deprecated field codes.** The icon pair is dropped
//!   rather than passed on: a program is being handed a file, not a window to
//!   decorate.

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow};

use crate::APP_ID;
use crate::clipboard::file_uri;

/// The most of a desktop entry that is read. Entries are a dozen lines of
/// text; anything past this is not one, and reading a file until it stops is
/// how a program is made to swallow a disk.
const MAX_ENTRY_BYTES: u64 = 64 * 1024;

/// And the most of an index. `mimeinfo.cache` holds a line per MIME type the
/// system knows, which runs to tens of kilobytes on a full desktop.
const MAX_INDEX_BYTES: u64 = 4 * 1024 * 1024;

/// The longest name an item of the menu may wear, in characters.
///
/// A program's name is two or three words and nothing here needs to bound it
/// for its own sake. What does is the menu: its items are as wide as the
/// longest name on them — see `ui::menu::open_items` — so a name that ran on
/// would be a popup wider than the window it opened in.
pub const MAX_NAME: usize = 48;

/// How deep the walk of an applications directory goes when there is no index
/// to read instead. The specification nests entries a level or two — a
/// vendor's own subdirectory — and nothing sane goes deeper; the limit is
/// what keeps a symlink pointing at its own parent from walking forever.
const MAX_DEPTH: usize = 4;

/// A program the desktop says can open a file of this kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opener {
    /// What the entry calls itself, in the user's own language where it says
    /// so, and what the menu wears. Control characters are taken out as it is
    /// read: it is somebody else's text on its way into this window.
    pub name: String,
    /// Which entry it came from — the desktop file id, `org.gimp.GIMP.desktop`
    /// and the like. What the association files name a program by, and what
    /// tells two entries wearing the same name apart.
    id: String,
    /// `Exec` split into words, with its field codes still in them: what
    /// stands for the file is not known until there is a file to put there.
    words: Vec<String>,
    /// Where the entry itself is, which is what `%k` stands for.
    entry: PathBuf,
}

impl Opener {
    /// The command line for opening `path`: the entry's own words with the
    /// file put wherever it asked for one.
    ///
    /// `OsString` rather than `String` throughout, because a filename is not
    /// required to be text — the bytes go to the program exactly as the file
    /// system holds them. Nothing passes through a shell, so a name full of
    /// quotes and semicolons is an argument and not a command.
    fn argv(&self, path: &Path) -> Vec<OsString> {
        let mut argv = Vec::with_capacity(self.words.len() + 1);
        let mut took_file = false;
        for word in &self.words {
            let (expanded, file) = self.expand(word, path);
            took_file |= file;
            if let Some(expanded) = expanded {
                argv.push(expanded);
            }
        }
        // An entry with no file code in it is one that never expected to be
        // handed anything. It is still worth pressing — the program is on the
        // menu because it claims the type — so the file goes on the end,
        // which is where a command line takes its arguments.
        if !took_file {
            argv.push(path.as_os_str().to_os_string());
        }
        argv
    }

    /// One word of `Exec` with its field codes resolved: what to pass, and
    /// whether the file went into it.
    ///
    /// `None` for a word that resolves to nothing at all — `%i` on an entry
    /// with no icon, a deprecated code standing alone — since passing an
    /// empty argument is not the same as passing none.
    fn expand(&self, word: &str, path: &Path) -> (Option<OsString>, bool) {
        if !word.contains('%') {
            return (Some(OsString::from(word)), false);
        }
        let mut out = OsString::new();
        let mut took_file = false;
        let mut chars = word.chars();
        while let Some(character) = chars.next() {
            if character != '%' {
                out.push(character.to_string());
                continue;
            }
            match chars.next() {
                // The file itself, named or spelled as a URI. The plural
                // codes stand for a list, and a list of one is a file.
                Some('f' | 'F') => {
                    out.push(path.as_os_str());
                    took_file = true;
                }
                Some('u' | 'U') => {
                    out.push(file_uri(path));
                    took_file = true;
                }
                Some('c') => out.push(&self.name),
                Some('k') => out.push(self.entry.as_os_str()),
                Some('%') => out.push("%"),
                // `%i` is an icon flag and its value, which is two arguments
                // where every other code is one, and no use to a program
                // being handed a file. The rest were deprecated by the
                // specification and are to be dropped where they are found.
                Some(_) | None => {}
            }
        }
        match out.is_empty() {
            true => (None, took_file),
            false => (Some(out), took_file),
        }
    }
}

/// Every program the desktop says can open `path`, the default one first and
/// the rest by name.
///
/// Empty where nothing offers — an unusual format, or a desktop with no such
/// program installed — which is what draws the button dead rather than
/// listing nothing.
pub fn for_file(path: &Path) -> Vec<Opener> {
    let types = mime_types(path);
    if types.is_empty() {
        return Vec::new();
    }
    let associations = Associations::read(types);
    let dirs = application_dirs();

    // The user's own associations first, then whatever the indexes offer.
    // Only the order candidates are considered in: what the menu is finally
    // ordered by is below.
    let mut ids: Vec<Candidate> = associations
        .added
        .iter()
        .map(|id| Candidate {
            id: id.clone(),
            // Associated by hand, so the entry need not claim the type
            // itself — that is the whole point of having added it.
            claimed: true,
        })
        .collect();
    for dir in &dirs {
        for id in offered(dir, types) {
            ids.push(Candidate { id, claimed: false });
        }
    }

    // This program is not another application to open the file in: an entry
    // that handed the picture back to the window it is already in is the one
    // item on the menu that could do nothing.
    let ourselves = format!("{APP_ID}.desktop");
    let mut seen = HashSet::new();
    let mut openers = Vec::new();
    for candidate in ids {
        if associations.removed.contains(&candidate.id) || candidate.id == ourselves {
            continue;
        }
        if !seen.insert(candidate.id.clone()) {
            continue;
        }
        if let Some(opener) = read_entry(&candidate, &dirs, types) {
            openers.push(opener);
        }
    }

    // The default first, as every menu of this kind puts it, and the rest in
    // the order a reader would look for a name in. The id breaks a tie, so
    // that two entries wearing one name come out in the same order every
    // time rather than in whichever the directory happened to be read in.
    let default = associations.default;
    openers.sort_by(|a, b| {
        let rank = |opener: &Opener| match default.first() {
            Some(id) if *id == opener.id => 0,
            _ => 1,
        };
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.id.cmp(&b.id))
    });
    distinguish(&mut openers);
    openers
}

/// Puts the entry's own id after any name two of them share.
///
/// The desktop does not promise that a name is unique, and the pair that
/// arrives together is the usual case: one entry to open a file and another
/// to open the directory it is in, both called "Image Viewer" because both
/// are. Two identical items is a menu where one of them is a guess; the id
/// is the one thing that is always different, and is what the desktop calls
/// them apart by itself.
fn distinguish(openers: &mut [Opener]) {
    let duplicated: HashSet<String> = openers
        .iter()
        .filter(|opener| {
            openers
                .iter()
                .filter(|other| other.name == opener.name)
                .count()
                > 1
        })
        .map(|opener| opener.name.clone())
        .collect();
    for opener in openers.iter_mut() {
        if duplicated.contains(&opener.name) {
            let id = opener.id.strip_suffix(".desktop").unwrap_or(&opener.id);
            opener.name = format!("{} ({id})", opener.name);
        }
    }
}

/// Hands `path` to `opener`, and returns as soon as it has been started.
///
/// The program is put in a process group of its own, so that it outlives the
/// window that started it: a viewer launched from a terminal and then closed
/// would otherwise take everything it had opened down with it, the whole
/// group being signaled together. Nobody waits on it in line — a thread does
/// that, and does nothing else — so the child is collected rather than left a
/// zombie, and is adopted and goes on running if this program leaves first.
pub fn open(opener: &Opener, path: &Path) -> Result<()> {
    use std::os::unix::process::CommandExt as _;

    let mut argv = opener.argv(path);
    if argv.is_empty() {
        return Err(anyhow!("{} says nothing to run", opener.name));
    }
    let program = argv.remove(0);
    let mut child = Command::new(&program)
        .args(argv)
        // Nothing is written to it and nothing is read back: what it has to
        // say about the file is said in its own window. Standard error is
        // left alone, so that a program that fails to start says so where
        // every other message from here goes.
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .process_group(0)
        .spawn()
        .with_context(|| format!("starting {}", opener.name))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// A desktop file id on its way to being read, and whether the entry behind
/// it still has to prove it claims the type.
struct Candidate {
    id: String,
    claimed: bool,
}

/// What the desktop calls the files this program opens: one entry per
/// extension the decoders read, with every name the same format is known
/// under.
///
/// Several names because a desktop entry lists whichever the program that
/// wrote it thought of, and a viewer registered for `image/x-bmp` opens the
/// same file as one registered for `image/bmp`. Matching any of them finds
/// both, which is what the aliases in `shared-mime-info` mean in the first
/// place.
///
/// The file's name and not its bytes: this is what the *desktop's* database
/// is keyed by, so a file whose extension lies about it is a file no other
/// program will recognize either. That the decoders here sniff their way past
/// such a name is a courtesy this table cannot pass on.
const MIME_TYPES: &[(&str, &[&str])] = &[
    ("jpg", &["image/jpeg"]),
    ("jpeg", &["image/jpeg"]),
    ("jpe", &["image/jpeg"]),
    ("jfif", &["image/jpeg"]),
    ("png", &["image/png"]),
    ("gif", &["image/gif"]),
    ("webp", &["image/webp"]),
    ("jxl", &["image/jxl"]),
    ("tif", &["image/tiff"]),
    ("tiff", &["image/tiff"]),
    ("bmp", &["image/bmp", "image/x-bmp", "image/x-ms-bmp"]),
    ("ico", &["image/vnd.microsoft.icon", "image/x-icon"]),
    ("heic", &["image/heic", "image/heif"]),
    ("heif", &["image/heif", "image/heic"]),
    ("hif", &["image/heif", "image/heic"]),
    ("avif", &["image/avif"]),
    ("exr", &["image/x-exr"]),
    ("hdr", &["image/vnd.radiance", "image/x-hdr"]),
    ("pnm", &["image/x-portable-anymap"]),
    ("pbm", &["image/x-portable-bitmap"]),
    ("pgm", &["image/x-portable-graymap"]),
    ("ppm", &["image/x-portable-pixmap"]),
    ("pam", &["image/x-portable-arbitrarymap"]),
];

/// What the desktop would call `path`, judged by its extension alone.
fn mime_types(path: &Path) -> &'static [&'static str] {
    let Some(extension) = path.extension().and_then(OsStr::to_str) else {
        return &[];
    };
    let extension = extension.to_ascii_lowercase();
    MIME_TYPES
        .iter()
        .find(|(known, _)| *known == extension)
        .map_or(&[], |(_, types)| *types)
}

/// The directories desktop entries are installed in, in the order a name
/// found in two of them should be taken from: the user's own first, then the
/// system's.
fn application_dirs() -> Vec<PathBuf> {
    data_dirs()
        .into_iter()
        .map(|dir| dir.join("applications"))
        .collect()
}

/// `$XDG_DATA_HOME` and then `$XDG_DATA_DIRS`, each defaulted as the base
/// directory specification says. A relative path in either is ignored, which
/// the specification also asks for: it would otherwise be resolved against
/// whatever directory this program happened to be started in.
fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = absolute_env("XDG_DATA_HOME") {
        dirs.push(home);
    } else if let Some(home) = home_dir() {
        dirs.push(home.join(".local/share"));
    }
    match std::env::var_os("XDG_DATA_DIRS") {
        Some(value) if !value.is_empty() => dirs.extend(split_dirs(&value)),
        _ => dirs.extend([
            PathBuf::from("/usr/local/share"),
            PathBuf::from("/usr/share"),
        ]),
    }
    dirs
}

/// The same for the configuration directories, which is where the user's own
/// associations live.
fn config_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = absolute_env("XDG_CONFIG_HOME") {
        dirs.push(home);
    } else if let Some(home) = home_dir() {
        dirs.push(home.join(".config"));
    }
    match std::env::var_os("XDG_CONFIG_DIRS") {
        Some(value) if !value.is_empty() => dirs.extend(split_dirs(&value)),
        _ => dirs.push(PathBuf::from("/etc/xdg")),
    }
    dirs
}

fn absolute_env(name: &str) -> Option<PathBuf> {
    let value = std::env::var_os(name)?;
    let path = PathBuf::from(value);
    path.is_absolute().then_some(path)
}

fn home_dir() -> Option<PathBuf> {
    absolute_env("HOME")
}

/// A colon-separated search path, as the base directory specification writes
/// one, with the relative entries dropped.
fn split_dirs(value: &OsStr) -> Vec<PathBuf> {
    std::env::split_paths(value)
        .filter(|path| path.is_absolute())
        .collect()
}

/// The desktop file ids `dir` offers for any of `types`.
///
/// From the index `update-desktop-database` writes, where there is one: it is
/// exactly this question, already answered, and reading it saves parsing
/// every entry on the system. A directory with no index is walked instead,
/// and each entry found in it is asked what it claims — which is what happens
/// on a desktop where nothing has ever run that tool.
fn offered(dir: &Path, types: &[&str]) -> Vec<String> {
    match read_capped(&dir.join("mimeinfo.cache"), MAX_INDEX_BYTES) {
        Some(index) => indexed(&index, types),
        None => walk(dir),
    }
}

/// The ids listed under any of `types` in a `mimeinfo.cache`.
fn indexed(index: &str, types: &[&str]) -> Vec<String> {
    let mut ids = Vec::new();
    for (key, value) in group(index, "MIME Cache") {
        if types.iter().any(|wanted| key.eq_ignore_ascii_case(wanted)) {
            ids.extend(list(value));
        }
    }
    ids
}

/// Every desktop file id under `dir`, for the directory that has no index.
///
/// An entry in a subdirectory is named by its path with the separators
/// written as hyphens, which is what the desktop entry specification calls a
/// desktop file id and what [`entry_paths`] undoes to find the file again.
fn walk(dir: &Path) -> Vec<String> {
    fn recurse(dir: &Path, prefix: &str, depth: usize, ids: &mut Vec<String>) {
        if depth > MAX_DEPTH {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                recurse(&entry.path(), &format!("{prefix}{name}-"), depth + 1, ids);
            } else if name.ends_with(".desktop") {
                ids.push(format!("{prefix}{name}"));
            }
        }
    }

    let mut ids = Vec::new();
    recurse(dir, "", 0, &mut ids);
    ids
}

/// Where the entry with this id could be, under one applications directory.
///
/// A hyphen in an id may be a hyphen in a name or the mark of a
/// subdirectory, and nothing but looking says which, so every reading of it
/// is offered in turn — the plain name first, since that is what almost every
/// entry is.
fn entry_paths(id: &str) -> Vec<PathBuf> {
    let mut paths = vec![PathBuf::from(id)];
    let mut rest = id.to_string();
    while let Some(hyphen) = rest.find('-') {
        rest.replace_range(hyphen..hyphen + 1, "/");
        paths.push(PathBuf::from(&rest));
    }
    paths
}

/// Reads the entry `candidate` names, and says whether it belongs on the
/// menu.
///
/// `None` for everything that would be a dead item: an entry that is not a
/// program, one hidden from menus, one wanting a terminal, one whose program
/// is not installed — and one that never claimed the type at all, unless the
/// user associated it by hand.
fn read_entry(candidate: &Candidate, dirs: &[PathBuf], types: &[&str]) -> Option<Opener> {
    let (path, text) = dirs
        .iter()
        .flat_map(|dir| {
            entry_paths(&candidate.id)
                .into_iter()
                .map(|tail| dir.join(tail))
        })
        .find_map(|path| read_capped(&path, MAX_ENTRY_BYTES).map(|text| (path, text)))?;

    let mut name = None;
    let mut exec = None;
    let mut try_exec = None;
    let mut kind = None;
    let mut mime: Vec<String> = Vec::new();
    let mut terminal = false;
    let mut hidden = false;
    let locales = locales();
    let mut best_locale = usize::MAX;
    for (key, value) in group(&text, "Desktop Entry") {
        match key {
            "Type" => kind = Some(value),
            "Exec" => exec = Some(value),
            "TryExec" => try_exec = Some(value),
            "MimeType" => mime = list(value),
            "Terminal" => terminal = value == "true",
            // `Hidden` means the entry has been deleted — the user's own
            // copy of it standing in front of the system's to say so — and a
            // deleted entry is not an installed program.
            //
            // `NoDisplay` is not the same thing and is not read here: the
            // specification gives it for exactly this case, a program that
            // wants to be associated with a type and handed files by other
            // programs without also appearing in the applications menu. This
            // menu is that association, not that menu.
            "Hidden" => hidden |= value == "true",
            _ => {}
        }
        // The name in the user's own language where the entry has one, and
        // the nearest match where it has several: a program is called what
        // the desktop calls it everywhere else.
        if let Some(locale) = name_locale(key) {
            let rank = match locale {
                None => locales.len(),
                Some(locale) => match locales.iter().position(|known| known == locale) {
                    Some(rank) => rank,
                    None => continue,
                },
            };
            if rank <= best_locale {
                best_locale = rank;
                name = Some(unescape(value));
            }
        }
    }

    if kind != Some("Application") || hidden || terminal {
        return None;
    }
    // Case-insensitively, as the index above is read: a type is a type
    // whichever case the entry that claims it happened to write.
    let claims = mime
        .iter()
        .any(|held| types.iter().any(|wanted| held.eq_ignore_ascii_case(wanted)));
    if !candidate.claimed && !claims {
        return None;
    }
    // What the entry says to try before offering itself, where it says one:
    // the usual way a package leaves an entry behind for a program that is
    // no longer installed.
    if let Some(try_exec) = try_exec
        && !installed(Path::new(try_exec))
    {
        return None;
    }
    let words = words(exec?)?;
    if !installed(Path::new(words.first()?)) {
        return None;
    }

    let name = name.unwrap_or_else(|| candidate.id.clone());
    Some(Opener {
        name: shortened(&crate::escape_controls(&name)),
        id: candidate.id.clone(),
        words,
        entry: path,
    })
}

/// `name` cut to [`MAX_NAME`], with the ellipsis that says it was cut.
fn shortened(name: &str) -> String {
    match name.char_indices().nth(MAX_NAME) {
        Some((end, _)) => format!("{}\u{2026}", &name[..end]),
        None => name.to_string(),
    }
}

/// Whether `program` is there to be run: a path as it stands, and a bare name
/// on `PATH`, which is where a desktop entry usually leaves it.
fn installed(program: &Path) -> bool {
    if program.components().count() > 1 {
        return program.is_file();
    }
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(program).is_file())
}

/// The locales to look for a name in, best first: the language with its
/// country and then the language alone, as the desktop entry specification
/// matches them.
fn locales() -> Vec<String> {
    let Some(locale) = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|name| std::env::var(name).ok())
        .filter(|locale| !locale.is_empty() && locale != "C" && locale != "POSIX")
    else {
        return Vec::new();
    };
    // `de_DE.UTF-8@euro` down to `de_DE`: the encoding is about the bytes and
    // the modifier is a variant almost nothing writes a name for.
    let locale = locale
        .split(['.', '@'])
        .next()
        .unwrap_or_default()
        .to_string();
    match locale.split_once('_') {
        Some((language, _)) => vec![locale.clone(), language.to_string()],
        None => vec![locale],
    }
}

/// Whether `key` is a name, and in which locale: `None` for the plain `Name`,
/// and the locale itself for `Name[de_DE]`.
fn name_locale(key: &str) -> Option<Option<&str>> {
    if key == "Name" {
        return Some(None);
    }
    let locale = key.strip_prefix("Name[")?.strip_suffix(']')?;
    Some(Some(locale))
}

/// The key and value of every line in `group` of a desktop-entry file.
///
/// The format is the same for entries, indexes and association files:
/// `[Group]` headings, `key=value` lines under them, `#` comments, and
/// whitespace around a key or a value that is not part of it.
fn group<'a>(text: &'a str, group: &str) -> Vec<(&'a str, &'a str)> {
    let mut lines = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(heading) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            inside = heading == group;
            continue;
        }
        if !inside {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            lines.push((key.trim(), value.trim()));
        }
    }
    lines
}

/// A semicolon-separated list, as every list in these files is written. The
/// trailing semicolon the format asks for leaves an empty last item, which is
/// not one.
fn list(value: &str) -> Vec<String> {
    value
        .split(';')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

/// The escapes a value of the format may carry, undone.
fn unescape(value: &str) -> String {
    if !value.contains('\\') {
        return value.to_string();
    }
    let mut out = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        match characters.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// An `Exec` value split into the words it will be run as.
///
/// Shell-like quoting, and only the part of it the specification allows:
/// double quotes around a word, a backslash escaping the character after it.
/// Nothing is expanded — no variables, no globs, no word splitting of what
/// came out of a quote — because nothing here goes through a shell. `None`
/// for a value that has no words in it at all.
fn words(exec: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quoted = false;
    let mut characters = exec.chars();
    while let Some(character) = characters.next() {
        match character {
            '\\' => match characters.next() {
                Some(escaped) => {
                    started = true;
                    word.push(escaped);
                }
                None => break,
            },
            '"' => {
                started = true;
                quoted = !quoted;
            }
            character if character.is_whitespace() && !quoted => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            character => {
                started = true;
                word.push(character);
            }
        }
    }
    if started {
        words.push(word);
    }
    (!words.is_empty()).then_some(words)
}

/// Reads at most `limit` bytes of `path` as text. `None` for a file that is
/// not there, cannot be read, or is not the UTF-8 every one of these formats
/// is defined to be.
fn read_capped(path: &Path, limit: u64) -> Option<String> {
    let file = File::open(path).ok()?;
    let mut text = String::new();
    file.take(limit).read_to_string(&mut text).ok()?;
    Some(text)
}

/// What the user's own association files say about a set of MIME types:
/// which program is the default for them, which have been associated by hand,
/// and which have been taken away.
#[derive(Debug, Default, PartialEq, Eq)]
struct Associations {
    /// The defaults, best first. Only the first is treated as *the* default;
    /// the rest are held because the file lists them in preference order and
    /// a later one takes over when the first is not installed.
    default: Vec<String>,
    added: Vec<String>,
    removed: Vec<String>,
}

impl Associations {
    /// Reads them from every `mimeapps.list` on the search path, in the order
    /// the association specification lays down: the user's configuration
    /// first, then the system's, then the deprecated copies beside the
    /// entries themselves.
    fn read(types: &[&str]) -> Self {
        let mut associations = Associations::default();
        // The first file to mention a program for a type settles it. A
        // desktop that adds back what the system took away, or the other way
        // round, is answered by whichever file the user's own configuration
        // is nearer to.
        let mut decided: HashSet<(String, String)> = HashSet::new();
        for path in association_files() {
            let Some(text) = read_capped(&path, MAX_INDEX_BYTES) else {
                continue;
            };
            for (heading, into) in [
                ("Added Associations", &mut associations.added),
                ("Removed Associations", &mut associations.removed),
            ] {
                for (key, value) in group(&text, heading) {
                    if !types.iter().any(|wanted| key.eq_ignore_ascii_case(wanted)) {
                        continue;
                    }
                    for id in list(value) {
                        if decided.insert((key.to_string(), id.clone())) {
                            into.push(id);
                        }
                    }
                }
            }
            for (key, value) in group(&text, "Default Applications") {
                if types.iter().any(|wanted| key.eq_ignore_ascii_case(wanted)) {
                    associations.default.extend(list(value));
                }
            }
        }
        associations
    }
}

/// Every place a `mimeapps.list` may be, in falling order of precedence.
fn association_files() -> Vec<PathBuf> {
    let desktops: Vec<String> = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .filter(|desktop| !desktop.is_empty())
        .map(str::to_lowercase)
        .collect();
    // The desktop's own file comes before the plain one in each directory:
    // what the session sets is more specific than what the user set for every
    // session.
    let named = |dir: &Path| {
        let mut files: Vec<PathBuf> = desktops
            .iter()
            .map(|desktop| dir.join(format!("{desktop}-mimeapps.list")))
            .collect();
        files.push(dir.join("mimeapps.list"));
        files
    };

    let mut files: Vec<PathBuf> = config_dirs().iter().flat_map(|dir| named(dir)).collect();
    files.extend(
        data_dirs()
            .iter()
            .flat_map(|dir| named(&dir.join("applications"))),
    );
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every file this program opens has a name the desktop knows it by, or
    /// the button could never offer anything for it. A format added to the
    /// decoders and not to the table would be one this menu was silently
    /// empty for.
    #[test]
    fn every_extension_the_decoders_read_has_a_mime_type() {
        for extension in crate::image::decode::supported_extensions() {
            assert!(
                !mime_types(Path::new(&format!("photograph.{extension}"))).is_empty(),
                ".{extension} has no MIME type"
            );
        }
    }

    /// And it is the extension that is read, whatever case it is written in
    /// and whatever else is in the name.
    #[test]
    fn the_type_is_read_off_the_extension() {
        assert_eq!(mime_types(Path::new("/a/b.PNG")), ["image/png"]);
        assert_eq!(mime_types(Path::new("a.tar.jpeg")), ["image/jpeg"]);
        assert!(mime_types(Path::new("photograph")).is_empty());
        assert!(mime_types(Path::new("notes.txt")).is_empty());
        // Both spellings of the same format, so that an entry registered for
        // either is found.
        assert!(mime_types(Path::new("icon.ico")).contains(&"image/x-icon"));
        assert!(mime_types(Path::new("icon.ico")).contains(&"image/vnd.microsoft.icon"));
    }

    fn entry(exec: &str) -> Opener {
        Opener {
            name: "Editor".to_string(),
            id: "editor.desktop".to_string(),
            words: words(exec).expect("an exec line with words in it"),
            entry: PathBuf::from("/usr/share/applications/editor.desktop"),
        }
    }

    /// An `Exec` line is split the way a shell would split it, and only that
    /// far: quotes and backslashes group words, and nothing in a word is
    /// expanded afterwards.
    #[test]
    fn an_exec_line_is_split_into_words() {
        assert_eq!(words("gimp %U").unwrap(), ["gimp", "%U"]);
        assert_eq!(
            words("  flatpak run --file-forwarding org.gimp.GIMP @@u %U @@  ").unwrap(),
            [
                "flatpak",
                "run",
                "--file-forwarding",
                "org.gimp.GIMP",
                "@@u",
                "%U",
                "@@"
            ]
        );
        assert_eq!(
            words(r#""/opt/an editor/bin" --open %f"#).unwrap(),
            ["/opt/an editor/bin", "--open", "%f"]
        );
        assert_eq!(
            words(r#"editor \"quoted\" %f"#).unwrap(),
            ["editor", "\"quoted\"", "%f"]
        );
        assert_eq!(words("   ").as_deref(), None);
    }

    /// The file goes wherever the entry asked for one, in whichever of the
    /// four ways it asked, and on the end of the line where it asked for
    /// none.
    #[test]
    fn the_file_is_put_where_the_entry_asked_for_it() {
        let path = Path::new("/pictures/a photo.png");
        assert_eq!(
            entry("viewer %f").argv(path),
            ["viewer", "/pictures/a photo.png"]
        );
        assert_eq!(
            entry("viewer %F").argv(path),
            ["viewer", "/pictures/a photo.png"]
        );
        assert_eq!(
            entry("viewer %u").argv(path),
            ["viewer", "file:///pictures/a%20photo.png"]
        );
        assert_eq!(
            entry("viewer --open=%f").argv(path),
            ["viewer", "--open=/pictures/a photo.png"]
        );
        assert_eq!(
            entry("viewer").argv(path),
            ["viewer", "/pictures/a photo.png"]
        );
    }

    /// The codes that are not the file: the ones with something to say are
    /// said, and the ones the specification deprecated are dropped rather
    /// than passed on as empty arguments.
    #[test]
    fn the_other_field_codes_are_resolved_or_dropped() {
        let path = Path::new("/pictures/photo.png");
        assert_eq!(
            entry("viewer %c %k %f").argv(path),
            [
                "viewer",
                "Editor",
                "/usr/share/applications/editor.desktop",
                "/pictures/photo.png"
            ]
        );
        assert_eq!(
            entry("viewer %i %v %f").argv(path),
            ["viewer", "/pictures/photo.png"]
        );
        assert_eq!(
            entry("viewer 100%% %f").argv(path),
            ["viewer", "100%", "/pictures/photo.png"]
        );
    }

    /// A name that is not text is still a name: the bytes the file system
    /// holds go to the program as they are, rather than being lost to a
    /// lossy conversion on the way.
    #[test]
    fn a_filename_that_is_not_utf8_survives() {
        use std::os::unix::ffi::OsStrExt as _;

        let path = PathBuf::from(OsStr::from_bytes(b"/pictures/\xff\xfe.png"));
        let argv = entry("viewer %f").argv(&path);
        assert_eq!(argv[1].as_os_str().as_bytes(), b"/pictures/\xff\xfe.png");
    }

    /// The index is read for the types asked about and no others.
    #[test]
    fn the_index_answers_for_the_types_asked_about() {
        let index = "\
[MIME Cache]
image/png=one.desktop;two.desktop;
image/gif=three.desktop;
text/plain=four.desktop;
";
        assert_eq!(
            indexed(index, &["image/png"]),
            ["one.desktop", "two.desktop"]
        );
        assert_eq!(
            indexed(index, &["image/png", "image/gif"]),
            ["one.desktop", "two.desktop", "three.desktop"]
        );
        assert!(indexed(index, &["image/jxl"]).is_empty());
    }

    /// A group is read to the next heading and no further, and the lines
    /// outside it are somebody else's.
    #[test]
    fn a_group_holds_only_its_own_lines() {
        let text = "\
# a comment
[Desktop Entry]
Type=Application
Name=An editor
Exec=editor %f

[Desktop Action new]
Name=New window
Exec=editor --new
";
        let entry = group(text, "Desktop Entry");
        assert_eq!(
            entry,
            [
                ("Type", "Application"),
                ("Name", "An editor"),
                ("Exec", "editor %f")
            ]
        );
        assert_eq!(
            group(text, "Desktop Action new"),
            [("Name", "New window"), ("Exec", "editor --new")]
        );
    }

    /// An id names one file, and a hyphen in it may be either a hyphen or a
    /// directory, so both readings are offered — the plain one first.
    #[test]
    fn an_id_is_looked_for_under_every_reading_of_its_hyphens() {
        assert_eq!(
            entry_paths("org.gimp.GIMP.desktop"),
            [PathBuf::from("org.gimp.GIMP.desktop")]
        );
        assert_eq!(
            entry_paths("kde-org.kde.gwenview.desktop"),
            [
                PathBuf::from("kde-org.kde.gwenview.desktop"),
                PathBuf::from("kde/org.kde.gwenview.desktop"),
            ]
        );
    }

    /// A list ends in a semicolon, which is a terminator and not an empty
    /// item.
    #[test]
    fn a_list_ends_rather_than_holding_an_empty_item() {
        assert_eq!(list("a;b;"), ["a", "b"]);
        assert_eq!(list("a;b"), ["a", "b"]);
        assert!(list("").is_empty());
        assert!(list(";").is_empty());
    }

    /// A name is cut to a length a menu can wear, and said to have been cut.
    /// Nothing ordinary is touched.
    #[test]
    fn a_name_too_long_for_a_menu_is_cut() {
        assert_eq!(
            shortened("GNU Image Manipulation Program"),
            "GNU Image Manipulation Program"
        );
        let long = "N".repeat(MAX_NAME + 10);
        let cut = shortened(&long);
        assert_eq!(cut.chars().count(), MAX_NAME + 1);
        assert!(cut.ends_with('\u{2026}'));
        // Cut by characters and not by bytes: a name in another script comes
        // back as text rather than as a panic on a split character.
        let wide = "\u{753b}".repeat(MAX_NAME + 2);
        assert_eq!(shortened(&wide).chars().count(), MAX_NAME + 1);
    }

    #[test]
    fn the_escapes_a_value_may_carry_are_undone() {
        assert_eq!(unescape(r"An\seditor"), "An editor");
        assert_eq!(unescape(r"one\\two"), r"one\two");
        assert_eq!(unescape("plain"), "plain");
    }

    /// The name is taken in the nearest language the entry offers, and the
    /// plain one where it offers nothing nearer.
    #[test]
    fn a_name_says_which_locale_it_is_in() {
        assert_eq!(name_locale("Name"), Some(None));
        assert_eq!(name_locale("Name[de_DE]"), Some(Some("de_DE")));
        assert_eq!(name_locale("GenericName"), None);
        assert_eq!(name_locale("Exec"), None);
    }
}
