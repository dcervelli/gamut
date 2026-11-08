//! HEIF and the codecs that ride in it: HEIC (HEVC), AVIF (AV1), and the
//! rest of what the platform's reader can open.
//!
//! There is no usable pure-Rust HEVC decoder, so the pixels come from the
//! platform: the system `libheif` everywhere but the Mac (`libheif.rs`), and
//! on a Mac the system's own ImageIO (`macos.rs`), which decodes with
//! Apple's codecs and leaves nothing to bundle. Both hand back the same
//! [`DecodedImage`](crate::image::DecodedImage) — the picture upright, at
//! its own depth and channels, in the color space the file states — and
//! `imageio`'s tests hold the two to each other over the fixtures and any
//! real files named to them.
//!
//! A HEIF file states its color space in CICP codes (H.273) rather than by
//! convention, so unlike PNG or TIFF the decoder is *told* the transfer
//! function and the primaries, and passes them on rather than guessing.
//!
//! An iPhone's HEIC is two pictures: the graded SDR photograph, and a gain
//! map beside it saying how far above SDR white each pixel went. The map is
//! described twice in a file from iOS 18 on — by an ISO 21496-1 `tmap` item
//! that [`tmap`] reads, and by Apple's own auxiliary image and maker note
//! that [`apple`] reads — and once, Apple's way, in an older one. Either way
//! the map goes with the picture as [`crate::image::gain_map`] describes,
//! the same as an Ultra HDR JPEG's, for the display to apply.
//!
//! A portrait may carry a depth map as well: another auxiliary image, of
//! the type MPEG gives depth, carried beside the picture for the readout
//! under the pointer, with its own XMP saying what its codes stand for —
//! see [`crate::image::depth`].
//!
//! [`items`] reads the file's item tables by walking the boxes, for what
//! neither library says: the `tmap` item everywhere, and on a Mac the size,
//! the color, the channels and the EXIF and XMP payloads as well.

mod apple;
#[cfg(all(test, target_os = "macos"))]
mod imageio;
mod items;
#[cfg(any(not(target_os = "macos"), test))]
mod libheif;
#[cfg(target_os = "macos")]
mod macos;
mod tmap;

#[cfg(not(target_os = "macos"))]
pub use libheif::Heif;
#[cfg(target_os = "macos")]
pub use macos::Heif;

/// The pixel-count ceiling, on the same reasoning as `MAX_DECODED_BYTES`:
/// `max_texture_dimension_2d` is 32768 on current hardware, so anything past
/// 32768 x 32768 could not be displayed even in principle.
const MAX_PIXELS: u64 = 32768 * 32768;

/// The extensions both readers answer to.
const EXTENSIONS: &[&str] = &["heic", "heif", "hif", "avif"];

/// What the `ftyp` box's brands say the file is — its major brand and the
/// compatible ones after it, since an AVIF's major brand may be the plain
/// `mif1` with `avif` among the compatibles — or `None` for a box that is
/// not there or names no brand this reads.
fn brand(header: &[u8]) -> Option<&'static str> {
    let mut heic = false;
    for brand in brands(header) {
        match brand {
            b"avif" | b"avis" => return Some("avif"),
            b"heic" | b"heix" | b"hevc" | b"hevx" | b"heim" | b"heis" | b"hevm" | b"hevs" => {
                heic = true;
            }
            _ => {}
        }
    }
    heic.then_some("heic")
}

/// The brands an `ftyp` box at the head of `header` names, major first,
/// as far as the header reaches. Nothing for a header that is not one.
fn brands(header: &[u8]) -> impl Iterator<Item = &[u8]> {
    let size = (header.get(4..8) == Some(b"ftyp"))
        .then(|| header.get(0..4))
        .flatten()
        .and_then(|size| size.try_into().ok())
        .map_or(0, |size: [u8; 4]| u32::from_be_bytes(size) as usize);
    let end = size.min(header.len());
    // The major brand at 8, the minor version at 12, the compatibles from 16.
    let major = (end >= 12).then(|| header.get(8..12)).flatten();
    major.into_iter().chain(
        (16..end.saturating_sub(3))
            .step_by(4)
            .filter_map(move |at| header.get(at..at + 4)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::decode::Decoder;

    /// Which of the three a file is, by its brands: `avif` wherever it is
    /// named, among the compatibles as well as as the major brand; HEVC's
    /// brands for a HEIC; the container's own name for a box naming
    /// neither.
    #[test]
    fn the_brands_say_which_of_the_three_a_file_is() {
        assert_eq!(
            brand(b"\x00\x00\x00\x18ftypheic\x00\x00\x00\x00mif1heic"),
            Some("heic")
        );
        assert_eq!(
            brand(b"\x00\x00\x00\x1cftypavif\x00\x00\x00\x00avifmif1miaf"),
            Some("avif")
        );
        assert_eq!(
            brand(b"\x00\x00\x00\x1cftypmif1\x00\x00\x00\x00mif1avifmiaf"),
            Some("avif"),
            "named among the compatibles"
        );
        assert_eq!(
            brand(b"\x00\x00\x00\x18ftypmif1\x00\x00\x00\x00mif1msf1"),
            None
        );
        assert_eq!(brand(b"\x89PNG\r\n\x1a\n"), None);
        let decoder = Heif;
        assert_eq!(
            decoder.format(b"\x00\x00\x00\x18ftypmif1\x00\x00\x00\x00mif1msf1"),
            decoder.name()
        );
        assert_eq!(
            crate::image::decode::reader(std::path::Path::new("test_images/avif-rgb8.avif")),
            Some("avif")
        );
        assert_eq!(
            crate::image::decode::reader(std::path::Path::new("test_images/heic-gray8.heic")),
            Some("heic")
        );
    }
}
