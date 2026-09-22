//! A path as a `file:` URI, and the percent-encoding under it. Three
//! spellings in the tree share the encoding and differ in what they leave
//! bare: the clipboard's and the trash's keep RFC 3986's unreserved set,
//! and the thumbnail cache keeps GLib's, since agreeing with GLib is what
//! keys the cache.

use std::os::unix::ffi::OsStrExt;
use std::path::Path;

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
}
