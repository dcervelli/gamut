//! What the files this program opens are called by the rest of the desktop:
//! the MIME types a file's extension stands for. The menu of other programs
//! that open the file asks by these names, a paste is taken under them, and
//! the desktop entry claims them.

use std::ffi::OsStr;
use std::path::Path;

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
pub const MIME_TYPES: &[(&str, &[&str])] = &[
    // `image/jpg` is no registered name, and is what a program that never
    // looked one up writes: read here for the entries that claim it, and
    // taken on the clipboard under it.
    ("jpg", &["image/jpeg", "image/jpg"]),
    ("jpeg", &["image/jpeg", "image/jpg"]),
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
    // The camera raw formats, each under the name shared-mime-info gives
    // it. Every one is also a subclass of `image/x-dcraw` there, which is
    // what a raw developer's desktop entry usually claims instead.
    ("dng", &["image/x-adobe-dng", "image/x-dcraw"]),
    ("nef", &["image/x-nikon-nef", "image/x-dcraw"]),
    ("nrw", &["image/x-nikon-nrw", "image/x-dcraw"]),
    ("cr2", &["image/x-canon-cr2", "image/x-dcraw"]),
    ("cr3", &["image/x-canon-cr3", "image/x-dcraw"]),
    ("crw", &["image/x-canon-crw", "image/x-dcraw"]),
    ("arw", &["image/x-sony-arw", "image/x-dcraw"]),
    ("srf", &["image/x-sony-srf", "image/x-dcraw"]),
    ("sr2", &["image/x-sony-sr2", "image/x-dcraw"]),
    ("raf", &["image/x-fuji-raf", "image/x-dcraw"]),
    ("orf", &["image/x-olympus-orf", "image/x-dcraw"]),
    ("rw2", &["image/x-panasonic-rw2", "image/x-dcraw"]),
    ("rwl", &["image/x-panasonic-rw2", "image/x-dcraw"]),
    ("pef", &["image/x-pentax-pef", "image/x-dcraw"]),
    ("srw", &["image/x-samsung-srw", "image/x-dcraw"]),
    ("3fr", &["image/x-hasselblad-3fr", "image/x-dcraw"]),
    ("fff", &["image/x-hasselblad-fff", "image/x-dcraw"]),
    ("iiq", &["image/x-phaseone-iiq", "image/x-dcraw"]),
    ("mef", &["image/x-mamiya-mef", "image/x-dcraw"]),
    ("mos", &["image/x-leaf-mos", "image/x-dcraw"]),
    ("erf", &["image/x-epson-erf", "image/x-dcraw"]),
    ("dcr", &["image/x-kodak-dcr", "image/x-dcraw"]),
    ("kdc", &["image/x-kodak-kdc", "image/x-dcraw"]),
    ("mrw", &["image/x-minolta-mrw", "image/x-dcraw"]),
];

/// What the desktop would call `path`, judged by its extension alone.
pub fn mime_types(path: &Path) -> &'static [&'static str] {
    let Some(extension) = path.extension().and_then(OsStr::to_str) else {
        return &[];
    };
    let extension = extension.to_ascii_lowercase();
    MIME_TYPES
        .iter()
        .find(|(known, _)| *known == extension)
        .map_or(&[], |(_, types)| *types)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every file this program opens has a name the desktop knows it by, or
    /// the menu of other programs could never offer anything for it. A format added to the
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
        assert_eq!(
            mime_types(Path::new("a.tar.jpeg")),
            ["image/jpeg", "image/jpg"]
        );
        assert!(mime_types(Path::new("photograph")).is_empty());
        assert!(mime_types(Path::new("notes.txt")).is_empty());
        // Both spellings of the same format, so that an entry registered for
        // either is found.
        assert!(mime_types(Path::new("icon.ico")).contains(&"image/x-icon"));
        assert!(mime_types(Path::new("icon.ico")).contains(&"image/vnd.microsoft.icon"));
    }
}
