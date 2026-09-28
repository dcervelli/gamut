//! A path as a `file:` URI and back, and the percent-encoding under it. Three
//! spellings in the tree share the encoding and differ in what they leave
//! bare: the clipboard's and the trash's keep RFC 3986's unreserved set,
//! and the thumbnail cache keeps GLib's, since agreeing with GLib is what
//! keys the cache.

use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// `bytes` with every byte `keep` does not vouch for percent-encoded, in
/// upper-case hex. Encoding more than strictly necessary is always correct
/// — it decodes back to the same bytes — where guessing which delimiters a
/// given reader tolerates unescaped is not. The bytes are a path's own, so
/// a name that is not valid UTF-8 survives the round trip.
pub fn percent_encoded(bytes: &[u8], keep: impl Fn(u8) -> bool) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &byte in bytes {
        if keep(byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// RFC 3986's unreserved set, and the separator: what a path keeps bare.
pub fn unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte)
}

/// `path` as a `file:` URI under RFC 3986. `path` must be absolute.
pub fn file(path: &Path) -> String {
    debug_assert!(path.is_absolute(), "a file URI needs an absolute path");
    format!(
        "file://{}",
        percent_encoded(path.as_os_str().as_bytes(), unreserved)
    )
}

/// The local path a `file:` URI names, its percent-escapes undone into the
/// bytes of the path itself. Refused for any other scheme, or for a file
/// on another host, neither of which is a file this program could read.
pub fn path_from_uri(uri: &str) -> Result<PathBuf> {
    let rest = uri
        .strip_prefix("file:")
        .with_context(|| format!("{uri} is not a file URI"))?;
    let path = match rest.strip_prefix("//") {
        Some(authority) => {
            let slash = authority
                .find('/')
                .with_context(|| format!("{uri} names no path"))?;
            let (host, path) = authority.split_at(slash);
            if !host.is_empty() && host != "localhost" {
                bail!("{uri} is on another host");
            }
            path
        }
        None => rest,
    };
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let escaped = (bytes[at] == b'%' && at + 2 < bytes.len())
            .then(|| std::str::from_utf8(&bytes[at + 1..at + 3]).ok())
            .flatten()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(byte) => {
                decoded.push(byte);
                at += 3;
            }
            None => {
                decoded.push(bytes[at]);
                at += 1;
            }
        }
    }
    if decoded.first() != Some(&b'/') {
        bail!("{uri} is not an absolute path");
    }
    Ok(PathBuf::from(OsString::from_vec(decoded)))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::path::PathBuf;

    use super::*;

    /// What the unreserved set leaves bare stays bare, and everything else
    /// — a space, a non-ASCII byte, a reserved character — is encoded as
    /// two upper-case hex digits.
    #[test]
    fn everything_outside_the_kept_set_is_percent_encoded() {
        assert_eq!(
            percent_encoded("/a b/é&~".as_bytes(), unreserved),
            "/a%20b/%C3%A9%26~"
        );
        assert_eq!(
            percent_encoded(b"a&b", |byte| unreserved(byte) || byte == b'&'),
            "a&b"
        );
        assert_eq!(
            file(Path::new("/home/me/my photos/été.jpg")),
            "file:///home/me/my%20photos/%C3%A9t%C3%A9.jpg"
        );
    }

    #[test]
    fn a_plain_path_needs_no_escaping() {
        assert_eq!(
            file(Path::new("/home/me/pictures/sunset_01.png")),
            "file:///home/me/pictures/sunset_01.png"
        );
    }

    /// Spaces, the reserved characters and anything above ASCII, all of which
    /// a reader would otherwise take for punctuation of the URI itself.
    #[test]
    fn everything_else_is_percent_encoded() {
        assert_eq!(
            file(Path::new("/tmp/a b#c?d%e.png")),
            "file:///tmp/a%20b%23c%3Fd%25e.png"
        );
        assert_eq!(
            file(Path::new("/tmp/caf\u{e9}.jpg")),
            "file:///tmp/caf%C3%A9.jpg"
        );
    }

    /// A name the filesystem allows and UTF-8 does not still names a file,
    /// and still has to reach the other program.
    #[test]
    fn a_name_that_is_not_utf8_survives() {
        let path = PathBuf::from(OsStr::from_bytes(b"/tmp/\xff.png"));
        assert_eq!(file(&path), "file:///tmp/%FF.png");
    }

    /// The URI a file dialog hands back is the path with its awkward bytes
    /// escaped — a space, a non-ASCII letter — and comes back as the path.
    /// The round trip through the clipboard's encoder is exact.
    #[test]
    fn a_file_uri_becomes_its_path() {
        assert_eq!(
            path_from_uri("file:///home/me/a%20b.png").unwrap(),
            PathBuf::from("/home/me/a b.png")
        );
        assert_eq!(
            path_from_uri("file://localhost/tmp/x.jpg").unwrap(),
            PathBuf::from("/tmp/x.jpg")
        );
        assert_eq!(
            path_from_uri("file:/tmp/plain").unwrap(),
            PathBuf::from("/tmp/plain")
        );
        let odd = PathBuf::from("/tmp/caf\u{e9} 100%.png");
        assert_eq!(path_from_uri(&file(&odd)).unwrap(), odd);
    }

    /// Anything that is not a local file is refused rather than guessed at.
    #[test]
    fn other_uris_are_refused() {
        assert!(path_from_uri("https://example.com/a.png").is_err());
        assert!(path_from_uri("file://elsewhere/a.png").is_err());
        assert!(path_from_uri("file://").is_err());
        assert!(path_from_uri("file:relative.png").is_err());
    }
}
