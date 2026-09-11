//! The desktop's thumbnail cache, as the freedesktop thumbnail specification
//! lays it out: where a file's thumbnail is kept, what it is called, what
//! has to be true of it for it to count, and how one is written so that
//! nothing else ever sees half of one.
//!
//! The cache is the desktop's rather than this program's on purpose. A
//! thumbnail made here is one the file manager finds and shows, and one the
//! file manager made is one this program finds and shows — which is most of
//! them, for a directory that has ever been opened in one. That only works
//! if the key agrees to the byte: the name of a thumbnail is the MD5 of the
//! file's URI, and the URI has to be spelled exactly as GLib spells it,
//! since GLib is what every other writer of this cache goes through.
//! [`uri`] is that spelling; `clipboard::file_uri` escapes more than GLib
//! does and must not be used for the key.
//!
//! Pure functions over paths and bytes, plus the file I/O; no threads and
//! nothing of the interface. The directories are passed in rather than
//! found, so that a test writes under a directory of its own and never
//! reads `XDG_CACHE_HOME`. [`Dirs::detect`] is what the program uses.

use std::fs;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::PROGRAM;

/// The longest side of a thumbnail in the `x-large` directory, which is the
/// size the specification gives that directory and the size GNOME's own
/// files there are.
pub const SIDE: u32 = 512;

/// The mode every directory of the cache is made with, and the mode every
/// file in it is written with: the specification asks for both, the cache
/// being a record of what the user has looked at.
const DIR_MODE: u32 = 0o700;
const FILE_MODE: u32 = 0o600;

/// The chunk names the specification requires, and the ones it recommends
/// that this program writes.
const URI_KEY: &str = "Thumb::URI";
const MTIME_KEY: &str = "Thumb::MTime";
const SIZE_KEY: &str = "Thumb::Size";
const WIDTH_KEY: &str = "Thumb::Image::Width";
const HEIGHT_KEY: &str = "Thumb::Image::Height";
const SOFTWARE_KEY: &str = "Software";

/// Where the cache is: its root, the directory of large thumbnails, and the
/// directory this program's version records its failures in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dirs {
    /// `thumbnails/` itself. A file under it is never thumbnailed.
    pub root: PathBuf,
    /// `thumbnails/x-large`, 512 pixels a side.
    pub xlarge: PathBuf,
    /// `thumbnails/fail/<program>-<version>`: the files this version could
    /// not thumbnail, so that they are not tried again on every visit — and
    /// so that a new version, which may read them, tries them afresh.
    pub fail: PathBuf,
}

impl Dirs {
    /// The cache under `root`, which is the `thumbnails/` directory itself.
    pub fn under(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            xlarge: root.join("x-large"),
            fail: root
                .join("fail")
                .join(format!("{PROGRAM}-{}", env!("CARGO_PKG_VERSION"))),
        }
    }

    /// The user's own cache: `$XDG_CACHE_HOME/thumbnails`, or
    /// `~/.cache/thumbnails`. `None` where neither can be named, in which
    /// case there is no cache to read or write.
    pub fn detect() -> Option<Self> {
        Some(Self::under(&cache_dir()?.join("thumbnails")))
    }

    /// Whether `path` is inside the cache. A thumbnail of a thumbnail is
    /// something the specification says not to make, and a directory of
    /// them opened here would otherwise fill the cache with them.
    pub fn holds(&self, path: &Path) -> bool {
        path.starts_with(&self.root)
    }
}

/// `$XDG_CACHE_HOME`, or `~/.cache`. A variable set to nothing is a
/// variable not set, as `pasted` reads its own.
fn cache_dir() -> Option<PathBuf> {
    let env_path = |name: &str| {
        std::env::var_os(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    env_path("XDG_CACHE_HOME").or_else(|| Some(env_path("HOME")?.join(".cache")))
}

/// `path` as GLib's `g_filename_to_uri` writes it, which is the spelling
/// the cache is keyed by. `path` must be absolute.
///
/// GLib leaves `A-Za-z0-9` and `!$&'()*+,-./:=@_~` as they are and
/// percent-encodes every other byte in upper-case hex — a slightly different
/// set from RFC 3986's, and the one that matters here, since agreeing with
/// GLib is the whole point. The bytes are the path's own, so a name that is
/// not valid UTF-8 keys the same file GLib would key.
pub fn uri(path: &Path) -> String {
    debug_assert!(path.is_absolute(), "the cache is keyed by absolute paths");
    let mut uri = String::from("file://");
    for &byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || b"!$&'()*+,-./:=@_~".contains(&byte) {
            uri.push(char::from(byte));
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

/// What a file is called in the cache: its URI, and the name of its
/// thumbnail, which is the MD5 of that URI in hex with `.png` after it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pub uri: String,
    pub name: String,
}

/// The key for `path`, which must be absolute.
pub fn key(path: &Path) -> Key {
    let uri = uri(path);
    let name = format!("{}.png", hex(&md5(uri.as_bytes())));
    Key { uri, name }
}

/// What the cache has for a file: a thumbnail that is up to date, a note
/// that this version could not make one, or nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lookup {
    /// The thumbnail at this path, made of the file as it is now.
    Fresh(PathBuf),
    /// This version of the program tried and could not, and the file has
    /// not changed since.
    Failed,
    Missing,
}

/// What the cache holds for `key`, for a file last modified at `mtime`
/// seconds after the epoch. A thumbnail counts only when it says it was
/// made of this URI at this modification time; anything else — a stale one,
/// a file that will not parse, none at all — is [`Lookup::Missing`].
pub fn lookup(dirs: &Dirs, key: &Key, mtime: u64) -> Lookup {
    let fresh = dirs.xlarge.join(&key.name);
    if describes(&fresh, key, mtime) {
        return Lookup::Fresh(fresh);
    }
    if describes(&dirs.fail.join(&key.name), key, mtime) {
        return Lookup::Failed;
    }
    Lookup::Missing
}

/// Whether the PNG at `path` says it is a thumbnail of `key` as of `mtime`.
fn describes(path: &Path, key: &Key, mtime: u64) -> bool {
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    let Ok(reader) = png::Decoder::new(std::io::BufReader::new(file)).read_info() else {
        return false;
    };
    let info = reader.info();
    let text = |wanted: &str| -> Option<String> {
        let plain = info
            .uncompressed_latin1_text
            .iter()
            .find(|chunk| chunk.keyword == wanted)
            .map(|chunk| chunk.text.clone());
        plain.or_else(|| {
            info.compressed_latin1_text
                .iter()
                .find(|chunk| chunk.keyword == wanted)
                .and_then(|chunk| chunk.get_text().ok())
        })
    };
    text(URI_KEY).as_deref() == Some(key.uri.as_str())
        && text(MTIME_KEY).as_deref() == Some(mtime.to_string().as_str())
}

/// The chunks a thumbnail of `key` carries: the two the specification
/// requires, the URI and the modification time, and the ones it recommends
/// — the file's size in bytes, the image's own dimensions, and what wrote
/// the thumbnail. All ASCII, so Latin-1 `tEXt` holds them exactly.
pub fn text_chunks(
    key: &Key,
    mtime: u64,
    size: u64,
    dimensions: Option<(u32, u32)>,
) -> Vec<(String, String)> {
    let mut chunks = vec![
        (URI_KEY.to_string(), key.uri.clone()),
        (MTIME_KEY.to_string(), mtime.to_string()),
        (SIZE_KEY.to_string(), size.to_string()),
        (SOFTWARE_KEY.to_string(), PROGRAM.to_string()),
    ];
    if let Some((width, height)) = dimensions {
        chunks.push((WIDTH_KEY.to_string(), width.to_string()));
        chunks.push((HEIGHT_KEY.to_string(), height.to_string()));
    }
    chunks
}

/// Puts `png` in `dir` under the name of `key`, making the directories on
/// the way and setting the modes the specification asks for.
///
/// Written to a temporary name in the same directory and renamed into
/// place, so that nothing reading the cache — the file manager, another
/// copy of this program — can open a thumbnail that is half written, and so
/// that the process leaving part way through leaves nothing under the final
/// name. `dirs` says where the cache's root is, since every directory from
/// there down owes the mode.
pub fn write(dirs: &Dirs, dir: &Path, key: &Key, png: &[u8]) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    for made in dir.ancestors() {
        // Best effort: a directory that is not ours to chmod still holds the
        // file, and a refusal here is not a reason to hold the thumbnail
        // back.
        let _ = fs::set_permissions(made, fs::Permissions::from_mode(DIR_MODE));
        if made == dirs.root {
            break;
        }
    }
    let temporary = dir.join(format!(".{}.{}.tmp", key.name, std::process::id()));
    let written = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(FILE_MODE)
        .open(&temporary)
        .with_context(|| format!("creating {}", temporary.display()))
        .and_then(|mut file| {
            file.write_all(png)
                .with_context(|| format!("writing {}", temporary.display()))
        });
    let final_name = dir.join(&key.name);
    let renamed = written.and_then(|()| {
        fs::rename(&temporary, &final_name)
            .with_context(|| format!("moving the thumbnail to {}", final_name.display()))
    });
    if renamed.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    renamed
}

/// Records in the fail directory that this version could not thumbnail the
/// file `key` names as it stood at `mtime`: a one-pixel PNG carrying the
/// two chunks a lookup reads, which is what the specification asks a
/// failure to be.
pub fn write_failure(dirs: &Dirs, key: &Key, mtime: u64) -> Result<()> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        for (keyword, text) in text_chunks(key, mtime, 0, None) {
            encoder
                .add_text_chunk(keyword, text)
                .context("adding a text chunk")?;
        }
        let mut writer = encoder.write_header().context("writing the PNG header")?;
        writer
            .write_image_data(&[0])
            .context("writing the PNG pixel")?;
        writer.finish().context("finishing the PNG")?;
    }
    write(dirs, &dirs.fail, key, &bytes)
}

/// The MD5 digest of `bytes`, as RFC 1321 lays it out. In the tree rather
/// than from a crate: it is the one hash the cache's naming wants, it is
/// eighty lines, and nothing here is asking it to be secure — a thumbnail's
/// name only has to agree with the name every other program gives it.
pub fn md5(bytes: &[u8]) -> [u8; 16] {
    const SHIFTS: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    // The binary integer parts of the sines of 1 through 64, as the RFC
    // tabulates them.
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];
    let mut state: [u32; 4] = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476];

    // The message, a 1 bit, zeros out to 56 mod 64, then the bit length in
    // little-endian.
    let mut message = bytes.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((bytes.len() as u64).wrapping_mul(8)).to_le_bytes());

    for block in message.as_chunks::<64>().0 {
        let words: [u32; 16] = std::array::from_fn(|index| {
            u32::from_le_bytes([
                block[4 * index],
                block[4 * index + 1],
                block[4 * index + 2],
                block[4 * index + 3],
            ])
        });
        let [mut a, mut b, mut c, mut d] = state;
        for round in 0..64 {
            let (f, g) = match round / 16 {
                0 => ((b & c) | (!b & d), round),
                1 => ((d & b) | (!d & c), (5 * round + 1) % 16),
                2 => (b ^ c ^ d, (3 * round + 5) % 16),
                _ => (c ^ (b | !d), (7 * round) % 16),
            };
            let rotated = a
                .wrapping_add(f)
                .wrapping_add(K[round])
                .wrapping_add(words[g])
                .rotate_left(SHIFTS[round]);
            (a, b, c, d) = (d, b.wrapping_add(rotated), b, c);
        }
        for (slot, value) in state.iter_mut().zip([a, b, c, d]) {
            *slot = slot.wrapping_add(value);
        }
    }

    let mut digest = [0u8; 16];
    for (chunk, word) in digest.as_chunks_mut::<4>().0.iter_mut().zip(state) {
        *chunk = word.to_le_bytes();
    }
    digest
}

/// A digest as the lower-case hex the cache names files by.
pub fn hex(digest: &[u8; 16]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cache directory of the test's own, under the system's temporary
    /// directory, so that nothing here reads or writes the user's.
    fn cache(name: &str) -> (Dirs, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("gamut-thumbnail-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        (Dirs::under(&root), root)
    }

    /// The URI is spelled as GLib spells it: its unreserved set kept, and
    /// everything else — a space, a semicolon, a bracket, a byte of UTF-8 —
    /// percent-encoded in upper-case hex.
    #[test]
    fn the_uri_is_spelled_as_glib_spells_it() {
        assert_eq!(
            uri(Path::new("/tmp/a b!$&'()*+,;=:@?[]#%~")),
            "file:///tmp/a%20b!$&'()*+,%3B=:@%3F%5B%5D%23%25~"
        );
        assert_eq!(
            uri(Path::new("/tmp/ünïcode 日本.png")),
            "file:///tmp/%C3%BCn%C3%AFcode%20%E6%97%A5%E6%9C%AC.png"
        );
    }

    /// RFC 1321's own test vectors, and one input of exactly a block, where
    /// the padding has to start a second one.
    #[test]
    fn md5_matches_the_rfc() {
        let digest = |text: &str| hex(&md5(text.as_bytes()));
        assert_eq!(digest(""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(digest("a"), "0cc175b9c0f1b6a831c399e269772661");
        assert_eq!(digest("abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(digest("message digest"), "f96b697d7cb7938d525a2f31aaf161d0");
        assert_eq!(
            digest("abcdefghijklmnopqrstuvwxyz"),
            "c3fcd3d76192e4007dfb496cca67e13b"
        );
        assert_eq!(
            digest(
                "12345678901234567890123456789012345678901234567890123456789012345678901234567890"
            ),
            "57edf4a22be3c955ac49da2e2107b67a"
        );
        let block = "a".repeat(64);
        assert_eq!(digest(&block), "014842d480b571495a4a0363793f7367");
    }

    /// The name is the hex of the digest of the URI, with `.png` after it —
    /// what `md5sum` prints for the same URI, which is what every other
    /// writer of the cache computes.
    #[test]
    fn the_key_is_the_digest_of_the_uri() {
        let plain = key(Path::new("/tmp/a.png"));
        assert_eq!(plain.uri, "file:///tmp/a.png");
        assert_eq!(plain.name, "a04bfd79b77efaccf5f6adb271b86f1e.png");
        let escaped = key(Path::new("/tmp/a b!$&'()*+,;=:@?[]#%~"));
        assert_eq!(escaped.name, "0be3eb25dd82c55f0c60f676598fe5a1.png");
    }

    /// The fail directory is this program's own, by version, and the cache
    /// knows what is inside itself.
    #[test]
    fn the_directories_are_where_the_specification_puts_them() {
        let dirs = Dirs::under(Path::new("/home/someone/.cache/thumbnails"));
        assert_eq!(
            dirs.xlarge,
            Path::new("/home/someone/.cache/thumbnails/x-large")
        );
        assert_eq!(
            dirs.fail,
            Path::new("/home/someone/.cache/thumbnails/fail")
                .join(format!("gamut-{}", env!("CARGO_PKG_VERSION")))
        );
        assert!(dirs.holds(Path::new("/home/someone/.cache/thumbnails/x-large/abc.png")));
        assert!(!dirs.holds(Path::new("/home/someone/Pictures/abc.png")));
    }

    /// A thumbnail written is a thumbnail found, as long as the file has not
    /// changed since; the modes are the specification's; and nothing of the
    /// writing is left behind.
    #[test]
    fn a_thumbnail_written_is_found_until_the_file_changes() {
        let (dirs, root) = cache("round-trip");
        let key = key(Path::new("/tmp/picture.png"));
        assert_eq!(lookup(&dirs, &key, 1000), Lookup::Missing);

        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 2, 1);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            for (keyword, text) in text_chunks(&key, 1000, 4096, Some((20, 10))) {
                encoder.add_text_chunk(keyword, text).unwrap();
            }
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[0; 6]).unwrap();
            writer.finish().unwrap();
        }
        write(&dirs, &dirs.xlarge, &key, &bytes).expect("the temporary directory is writable");

        let found = dirs.xlarge.join(&key.name);
        assert_eq!(lookup(&dirs, &key, 1000), Lookup::Fresh(found.clone()));
        assert_eq!(lookup(&dirs, &key, 1001), Lookup::Missing);
        assert_eq!(
            fs::metadata(&found).unwrap().permissions().mode() & 0o777,
            FILE_MODE
        );
        for dir in [&dirs.root, &dirs.xlarge] {
            assert_eq!(
                fs::metadata(dir).unwrap().permissions().mode() & 0o777,
                DIR_MODE,
                "{}",
                dir.display()
            );
        }
        let leftovers: Vec<_> = fs::read_dir(&dirs.xlarge)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name.to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");

        // A different file with the same name in the cache — a collision, or
        // a file moved — is not this file's thumbnail.
        let other = Key {
            uri: "file:///tmp/other.png".to_string(),
            name: key.name.clone(),
        };
        assert_eq!(lookup(&dirs, &other, 1000), Lookup::Missing);

        fs::remove_dir_all(root).unwrap();
    }

    /// A failure recorded is a failure found, until the file changes.
    #[test]
    fn a_failure_is_remembered_for_the_file_as_it_was() {
        let (dirs, root) = cache("failure");
        let key = key(Path::new("/tmp/broken.png"));
        write_failure(&dirs, &key, 7).expect("the temporary directory is writable");
        assert_eq!(lookup(&dirs, &key, 7), Lookup::Failed);
        assert_eq!(lookup(&dirs, &key, 8), Lookup::Missing);
        assert_eq!(
            fs::metadata(&dirs.fail).unwrap().permissions().mode() & 0o777,
            DIR_MODE
        );
        fs::remove_dir_all(root).unwrap();
    }
}
