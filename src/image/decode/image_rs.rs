//! The formats the `image` crate reads whose containers have nothing to add:
//! GIF, Radiance HDR, OpenEXR, BMP and netpbm. PNG and JPEG have their own
//! modules, since both carry things in their containers that `ImageReader`
//! throws away.
//!
//! GIF takes the plain route, because its container has nothing to say that
//! this program could act on: no profile, no code points, no orientation, and
//! a palette of sRGB bytes by definition. What it does have is animation, and
//! the crate's decoder reads the first frame — composited onto the logical
//! screen the file declares, so a first frame stored as a patch at an offset
//! still arrives whole. The frames after it are not shown, for the same
//! reason an animated WebP's are not: nothing downstream of here has a clock.
//! Every GIF comes back RGBA, whatever its palette holds, because that is the
//! one layout the crate's decoder produces.
//!
//! BMP takes it too. A `BITMAPV4` or `BITMAPV5` header can name a colour space
//! — sRGB, or a whole ICC profile appended after the pixels — but the crate's
//! decoder surfaces neither, and a BMP carrying either is rare enough that
//! reading the header a second time to find one would be work spent on almost
//! nothing. Everything else the format varies — 1, 4, 8, 16, 24 and 32 bits
//! per pixel, palettes, `BI_RLE4` and `BI_RLE8` runs, bitfield masks, and
//! rows stored bottom-up or top-down — the crate resolves before it answers,
//! so all of it arrives here as one of three layouts and means sRGB.
//!
//! Netpbm is the one whose header looked like it had something to say. A PBM,
//! PGM, PPM or PAM states a `MAXVAL`, the value a fully bright sample has,
//! and it need not be 255 or 65535: instrument pipelines write 1023 or 4095,
//! and a bitmap writes 1. Left alone that would matter a great deal, because
//! `Samples::full_scale` says a `U16` image's white is 65535 and a raster
//! stated against 1023 would show at a sixteenth of its brightness — the
//! correction a 10-bit HEIF needs and gets in `super::heif`. The crate
//! already applies it, rescaling every sample to saturate its width before
//! handing the buffer over, so there is nothing left here to do but say that
//! it does. `pnm-maxval1023.pgm` is the fixture that keeps it true.
//!
//! What netpbm says about colour is nothing this program can act on either.
//! Its specification names the BT.709 transfer function, which is close
//! enough to sRGB to call it that; a pipeline writing linear measurements
//! into a PGM is indistinguishable from the inside, and is what
//! `--transfer linear` is for.

use std::io::BufReader;

use anyhow::Result;

use crate::image::{ColorSpace, DecodedImage};

use super::{Overrides, ReadSeek, dynamic};

/// The smallest DIB header a BMP can carry, and the largest anything writes.
/// A file claiming less is malformed; one claiming wildly more is not a BMP
/// that happens to start with the right two letters.
const DIB_HEADER: std::ops::RangeInclusive<u32> = 12..=1024;

pub struct ImageRs;

impl super::Decoder for ImageRs {
    fn name(&self) -> &'static str {
        "gif/hdr/exr/bmp/netpbm"
    }

    /// `.pnm` is the format-agnostic spelling netpbm's own tools accept for
    /// any of the four, so it is claimed alongside the specific ones.
    fn extensions(&self) -> &'static [&'static str] {
        &[
            "gif", "hdr", "exr", "bmp", "pnm", "pbm", "pgm", "ppm", "pam",
        ]
    }

    fn sniff(&self, header: &[u8]) -> bool {
        // Both GIF versions; the four bytes after `GIF` are `87a` or `89a`,
        // and only the first three are a signature.
        header.starts_with(b"GIF87a")
            || header.starts_with(b"GIF89a")
            || header.starts_with(b"#?RADIANCE")
            || header.starts_with(b"#?RGBE")
            || header.starts_with(b"\x76\x2f\x31\x01")
            || is_bmp(header)
            || is_netpbm(header)
    }

    fn dimensions(&self, source: &mut dyn ReadSeek) -> Result<Option<(u32, u32)>> {
        dynamic::dimensions(source)
    }

    fn decode(&self, source: &mut dyn ReadSeek, _overrides: Overrides) -> Result<DecodedImage> {
        let mut reader = ::image::ImageReader::new(BufReader::new(source)).with_guessed_format()?;
        dynamic::limit(&mut reader);

        let format = reader.format();
        let decoded = reader.decode()?;
        dynamic::describe(decoded, format, ColorSpace::SRGB)
    }
}

/// BMP's signature is two letters, which is thin enough that plain text can
/// wear it — `BM` opens plenty of English sentences. The DIB header size that
/// follows the file header is what makes the guess safe: it is a small number
/// from a known set, and four bytes of prose are not.
fn is_bmp(header: &[u8]) -> bool {
    let Some(size) = header.get(14..18) else {
        return false;
    };
    header.starts_with(b"BM")
        && DIB_HEADER.contains(&u32::from_le_bytes(size.try_into().expect("four bytes")))
}

/// Netpbm's magic number is `P` and a digit, which needs the whitespace that
/// has to follow it to be worth trusting: the two letters alone would claim
/// any file starting `P5`.
fn is_netpbm(header: &[u8]) -> bool {
    matches!(header, [b'P', kind, space, ..]
        if (b'1'..=b'7').contains(kind) && space.is_ascii_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bytes a `BITMAPINFOHEADER` file opens with: the signature, a file
    /// size, two reserved words, the pixel offset, and the header size.
    fn bmp_header(dib_size: u32) -> Vec<u8> {
        let mut header = b"BM".to_vec();
        header.extend_from_slice(&2358u32.to_le_bytes());
        header.extend_from_slice(&[0; 4]);
        header.extend_from_slice(&54u32.to_le_bytes());
        header.extend_from_slice(&dib_size.to_le_bytes());
        header
    }

    #[test]
    fn every_dib_header_a_bmp_can_carry_is_recognised() {
        // Core, Info, V2, V3, V4, V5.
        for size in [12, 40, 52, 56, 108, 124] {
            assert!(is_bmp(&bmp_header(size)), "{size}");
        }
    }

    /// PBM, PGM, PPM and PAM, in both their ASCII and binary spellings.
    #[test]
    fn every_netpbm_magic_number_is_recognised() {
        for magic in ["P1", "P2", "P3", "P4", "P5", "P6", "P7"] {
            assert!(is_netpbm(format!("{magic}\n32 24\n").as_bytes()), "{magic}");
        }
    }

    /// The same point the DIB header size makes for BMP: a magic number this
    /// short needs what follows it to be checked.
    #[test]
    fn prose_beginning_with_a_netpbm_magic_number_is_not_claimed() {
        assert!(!is_netpbm(b"P4S is a rendering technique."));
        assert!(!is_netpbm(b"P8\n32 24\n"));
        assert!(!is_netpbm(b"P6"));
    }

    /// The point of looking past the signature: two letters alone would claim
    /// files that are not images at all.
    #[test]
    fn prose_beginning_bm_is_not_claimed() {
        assert!(!is_bmp(
            b"BMW ownership has its privileges, and this is one."
        ));
        // A header too short to hold a DIB header size says nothing either.
        assert!(!is_bmp(b"BM"));
        // Smaller than the smallest header there is, and larger than any.
        assert!(!is_bmp(&bmp_header(11)));
        assert!(!is_bmp(&bmp_header(4096)));
    }
}
