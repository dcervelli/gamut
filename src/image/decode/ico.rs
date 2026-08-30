//! ICO: a directory of icons, and the choice of which one to show.
//!
//! An ICO is not an image but a folder of them — the same picture at 16, 32,
//! 48 and 256 pixels, so that Windows can pick the one that fits the slot it
//! is drawing into. A viewer has no slot, so it has to choose, and the choice
//! is the whole of what this module adds over `image`'s own ICO decoder.
//!
//! `image` picks by `(bit depth, area)`, depth first, which is right for a
//! toolkit asked for the best-quality rendition and wrong for a viewer: a file
//! whose 256x256 entry is a palette image and whose 16x16 entry is 32-bit
//! shows as a thumbnail. Here the order is reversed — **largest first**, depth
//! only to break a tie — because what someone opening an icon wants to look at
//! is the biggest picture in it.
//!
//! Each entry is a whole file in its own right, in one of two formats, and
//! they take different routes:
//!
//! - **PNG**, which is how every entry above 48 pixels has been written since
//!   Vista. It goes through [`image_rs::png`], the same path a `.png` on disk
//!   takes, so an `iCCP` or `cICP` chunk is read and any pixel layout is kept.
//!   `image` would refuse anything but RGBA8 here, on the strength of a
//!   Microsoft blog post saying embedded PNGs must be 32-bit; browsers display
//!   the others, and so does this.
//!
//! - **BMP**, which is a headerless DIB with two Windows-specific quirks: the
//!   height in its header counts the rows twice, and a 1-bit AND mask may
//!   follow the pixels to carry transparency the colour data has no room for.
//!   `image` handles both, but only from inside its own ICO decoder — the
//!   hooks are `pub(crate)` — so the entry is handed back to it wrapped in a
//!   22-byte container holding nothing else. Rebuilding the DIB reader to
//!   avoid that would be a worse trade: it is a decade of Windows bitmap
//!   variants, already written and already tested.
//!
//! Only type 1, the icon, is claimed. A cursor is the same container under the
//! `.cur` extension, but its directory overloads the colour-plane and bit-depth
//! fields with the hotspot coordinates, so the numbers this module sorts on
//! would mean something else entirely.

use std::io::{Cursor, Read, SeekFrom};

use anyhow::{Context, Result, bail};

use ::image::{ImageDecoder, ImageFormat};

use crate::image::{ColorSpace, DecodedImage};

use super::image_rs;

pub struct Ico;

/// Bytes of the `ICONDIR` at the head of the file: reserved, type, count.
const DIRECTORY: usize = 6;
/// Bytes of each `ICONDIRENTRY` following it.
const ENTRY: usize = 16;
/// The type field's value for an icon, as opposed to 2 for a cursor.
const ICON: u16 = 1;

impl super::Decoder for Ico {
    fn name(&self) -> &'static str {
        "ico"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["ico"]
    }

    /// ICO has no magic number worth the name — four bytes, three of them
    /// zero — so the directory's own structure has to stand in for one. A
    /// file claiming entries it has no room for, or whose first entry starts
    /// inside the directory it is listed in, is not an ICO however its first
    /// four bytes read.
    fn sniff(&self, header: &[u8]) -> bool {
        if header.len() < DIRECTORY + ENTRY {
            return false;
        }
        let reserved = u16::from_le_bytes([header[0], header[1]]);
        let kind = u16::from_le_bytes([header[2], header[3]]);
        let count = u16::from_le_bytes([header[4], header[5]]);
        if reserved != 0 || kind != ICON || count == 0 {
            return false;
        }
        let offset = u32::from_le_bytes([header[18], header[19], header[20], header[21]]);
        offset as u64 >= (DIRECTORY + ENTRY * count as usize) as u64
    }

    fn dimensions(&self, source: &mut dyn super::ReadSeek) -> Result<Option<(u32, u32)>> {
        // The directory states each entry's size, so the one we are going to
        // show can be measured without unpacking it.
        let entries = directory(source)?;
        let chosen = choose(&entries);
        Ok(Some((chosen.width as u32, chosen.height as u32)))
    }

    fn decode(
        &self,
        source: &mut dyn super::ReadSeek,
        _overrides: super::Overrides,
    ) -> Result<DecodedImage> {
        let entries = directory(source)?;
        let chosen = choose(&entries);

        let payload = payload(source, chosen)?;

        if payload.starts_with(b"\x89PNG\r\n\x1a\n") {
            return image_rs::png(&mut Cursor::new(&payload[..]))
                .with_context(|| format!("the {chosen} entry, which holds a PNG"));
        }
        bitmap(&payload, chosen).with_context(|| format!("the {chosen} entry, which holds a BMP"))
    }
}

/// One `ICONDIRENTRY`, read into the fields the selection and the two
/// payload routes need.
#[derive(Clone, Copy, Debug)]
struct Entry {
    width: u16,
    height: u16,
    /// Bits per pixel as the directory states it, which is 0 in files whose
    /// writer did not bother. Only ever compared against other entries of the
    /// same file, and never trusted over what the entry's own header says.
    depth: u16,
    /// The stored `ICONDIRENTRY` bytes, kept whole so that a BMP entry can be
    /// handed back to `image` in a container it will read the same way.
    raw: [u8; ENTRY],
    offset: u32,
    length: u32,
}

/// The entry to show: the largest, and the deepest of any that tie. An icon
/// file is one image at several sizes, and the biggest is the one worth the
/// screen it is being given.
fn choose(entries: &[Entry]) -> &Entry {
    entries
        .iter()
        .max_by_key(|entry| (entry.width as u32 * entry.height as u32, entry.depth))
        .expect("the directory is never empty")
}

impl std::fmt::Display for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

/// Reads the `ICONDIR` and every `ICONDIRENTRY` behind it.
fn directory(source: &mut dyn super::ReadSeek) -> Result<Vec<Entry>> {
    source.rewind().context("reading the directory")?;

    let mut head = [0u8; DIRECTORY];
    source
        .read_exact(&mut head)
        .context("reading the directory")?;
    let kind = u16::from_le_bytes([head[2], head[3]]);
    if kind != ICON {
        // A cursor reaches here only through a `.ico` extension, since the
        // sniff does not claim one. Say which it is rather than failing later
        // on fields that mean something else.
        if kind == 2 {
            bail!("this is a cursor (.cur), not an icon");
        }
        bail!("the directory says type {kind}, and only 1 is an icon");
    }

    let count = u16::from_le_bytes([head[4], head[5]]);
    if count == 0 {
        bail!("the directory lists no images");
    }

    (0..count)
        .map(|index| {
            let mut raw = [0u8; ENTRY];
            source
                .read_exact(&mut raw)
                .with_context(|| format!("reading directory entry {index} of {count}"))?;
            Ok(Entry {
                // A zero byte means 256: the field is one byte wide and the
                // largest icon is one larger than it can hold.
                width: if raw[0] == 0 { 256 } else { raw[0] as u16 },
                height: if raw[1] == 0 { 256 } else { raw[1] as u16 },
                depth: u16::from_le_bytes([raw[6], raw[7]]),
                offset: u32::from_le_bytes([raw[12], raw[13], raw[14], raw[15]]),
                length: u32::from_le_bytes([raw[8], raw[9], raw[10], raw[11]]),
                raw,
            })
        })
        .collect()
}

/// The chosen entry's bytes: a whole PNG or a whole DIB, either way a file
/// this program already knows how to read.
///
/// The stated length is a ceiling rather than a promise: `read_to_end` behind
/// a `take` grows its buffer as it reads, so a file claiming four gigabytes
/// for a 16x16 icon costs what it actually holds, and only what was really
/// there is passed on.
fn payload(source: &mut dyn super::ReadSeek, entry: &Entry) -> Result<Vec<u8>> {
    source
        .seek(SeekFrom::Start(entry.offset as u64))
        .with_context(|| format!("seeking to the {entry} entry"))?;

    let mut payload = Vec::new();
    source
        .take(entry.length as u64)
        .read_to_end(&mut payload)
        .with_context(|| format!("reading the {entry} entry"))?;

    // Both formats need more than this before they say anything at all; an
    // entry shorter than a PNG signature is a truncated file, not a picture.
    if payload.len() < 8 {
        bail!("the {entry} entry holds {} bytes", payload.len());
    }
    Ok(payload)
}

/// A BMP entry, decoded by handing it back to `image` alone in a container.
///
/// The container is 22 bytes: a one-image `ICONDIR` and the entry's own
/// `ICONDIRENTRY`, copied across so that the dimensions and depth `image`
/// checks the DIB against are the ones the original file stated. Only the
/// offset and length are rewritten, to point at the single payload that
/// follows — the length to what was actually read, so that the
/// "is there an AND mask after the pixels" arithmetic works from a true size
/// rather than a claimed one.
fn bitmap(payload: &[u8], entry: &Entry) -> Result<DecodedImage> {
    let mut container = Vec::with_capacity(DIRECTORY + ENTRY + payload.len());
    container.extend_from_slice(&[0, 0, ICON as u8, 0, 1, 0]);
    container.extend_from_slice(&entry.raw);
    let length = u32::try_from(payload.len()).expect("an entry is read through a `u32` limit");
    container[DIRECTORY + 8..DIRECTORY + 12].copy_from_slice(&length.to_le_bytes());
    container[DIRECTORY + 12..DIRECTORY + 16]
        .copy_from_slice(&((DIRECTORY + ENTRY) as u32).to_le_bytes());
    container.extend_from_slice(payload);

    let decoder = ::image::codecs::ico::IcoDecoder::new(Cursor::new(&container))
        .context("reading the bitmap header")?;

    // The dimensions come from the DIB rather than from the directory, and
    // nothing has bounded them yet. ICO's own fields stop at 256, but the
    // header inside is free to claim more, and `from_decoder` allocates
    // whatever it claims.
    let (width, height) = decoder.dimensions();
    // Always RGBA8: the ICO path adds an alpha channel whatever the stored
    // depth, because that is where the AND mask has to go.
    super::check_decoded_size(width, height, 4, 8)?;

    let decoded = ::image::DynamicImage::from_decoder(decoder).context("decoding the bitmap")?;
    // A BMP has nothing to say about colour that this program can act on, so
    // sRGB stands — the same default a PNG entry without a profile gets.
    image_rs::describe(decoded, Some(ImageFormat::Ico), ColorSpace::SRGB)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::decode::Decoder;

    /// Builds an `ICONDIR` with the given `(width, height, depth)` entries,
    /// each pointing at a one-byte payload. Enough for the sniff and the
    /// selection, which read nothing else.
    fn dir(kind: u16, entries: &[(u8, u8, u16)]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&kind.to_le_bytes());
        bytes.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        let start = DIRECTORY + ENTRY * entries.len();
        for (index, (width, height, depth)) in entries.iter().enumerate() {
            bytes.extend_from_slice(&[*width, *height, 0, 0]);
            bytes.extend_from_slice(&1u16.to_le_bytes());
            bytes.extend_from_slice(&depth.to_le_bytes());
            bytes.extend_from_slice(&1u32.to_le_bytes());
            bytes.extend_from_slice(&((start + index) as u32).to_le_bytes());
        }
        bytes.resize(start + entries.len(), 0);
        bytes
    }

    fn chosen(entries: &[(u8, u8, u16)]) -> (u16, u16) {
        let bytes = dir(1, entries);
        let read = directory(&mut Cursor::new(bytes)).unwrap();
        let best = read
            .iter()
            .max_by_key(|entry| (entry.width as u32 * entry.height as u32, entry.depth))
            .unwrap();
        (best.width, best.height)
    }

    /// The point of the module. `image` scores depth before size and would
    /// answer 16x16 here; a viewer asked to show an icon should show the
    /// picture, not the thumbnail.
    #[test]
    fn the_largest_entry_wins_over_the_deepest() {
        assert_eq!(chosen(&[(16, 16, 32), (48, 48, 8)]), (48, 48));
        // Depth still breaks a tie between two of a size.
        assert_eq!(chosen(&[(32, 32, 8), (32, 32, 32)]), (32, 32));
        // And a directory that states no depths at all — 0 is legal, and
        // common — still picks by size rather than by position.
        assert_eq!(chosen(&[(64, 64, 0), (16, 16, 0)]), (64, 64));
    }

    /// The one size the format cannot state directly: the field is a byte,
    /// and 256 does not fit in it.
    #[test]
    fn a_zero_dimension_means_256() {
        assert_eq!(chosen(&[(0, 0, 32), (48, 48, 32)]), (256, 256));
    }

    /// Four bytes, three of them zero, is not a signature. Without the
    /// structural checks the sniff would claim files it cannot read, taking
    /// them from the message that explains itself.
    #[test]
    fn sniffing_needs_more_than_the_leading_zeroes() {
        assert!(Ico.sniff(&dir(1, &[(16, 16, 32)])));
        assert!(Ico.sniff(&dir(1, &[(0, 0, 32), (16, 16, 4)])));

        // A cursor is the same container, and deliberately not claimed.
        assert!(!Ico.sniff(&dir(2, &[(32, 32, 0)])));
        // An empty directory holds no image to show.
        assert!(!Ico.sniff(&dir(1, &[])));
        // Zeroes are the commonest leading bytes there are.
        assert!(!Ico.sniff(&[0u8; 64]));
        // Too short to hold the directory it claims.
        assert!(!Ico.sniff(&dir(1, &[(16, 16, 32)])[..20]));

        // An entry starting inside the directory that lists it.
        let mut overlapping = dir(1, &[(16, 16, 32)]);
        overlapping[18..22].copy_from_slice(&8u32.to_le_bytes());
        assert!(!Ico.sniff(&overlapping));
    }

    /// A cursor opened as `.ico` gets past the sniff only by its extension,
    /// and should be told what it is rather than mis-sorted on hotspots.
    #[test]
    fn a_cursor_is_named_rather_than_misread() {
        let error = directory(&mut Cursor::new(dir(2, &[(32, 32, 0)]))).unwrap_err();
        assert!(format!("{error:#}").contains("cursor"), "{error:#}");
    }

    /// The wrapper `image` is handed has to describe the payload that follows
    /// it, or the AND mask is looked for in the wrong place.
    #[test]
    fn the_rebuilt_container_describes_what_follows_it() {
        let entries = dir(1, &[(16, 16, 32)]);
        let entry = directory(&mut Cursor::new(entries)).unwrap()[0];

        let payload = vec![0xABu8; 40];
        let mut container = Vec::new();
        container.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
        container.extend_from_slice(&entry.raw);
        container[14..18].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        container[18..22].copy_from_slice(&22u32.to_le_bytes());
        container.extend_from_slice(&payload);

        // The same numbers `bitmap` writes, checked here rather than through
        // a decode so that a mistake in them is not hidden by a BMP error.
        assert!(Ico.sniff(&container));
        let rebuilt = directory(&mut Cursor::new(container)).unwrap();
        assert_eq!(rebuilt.len(), 1);
        assert_eq!(rebuilt[0].offset as usize, DIRECTORY + ENTRY);
        assert_eq!(rebuilt[0].length as usize, payload.len());
        assert_eq!((rebuilt[0].width, rebuilt[0].height), (16, 16));
    }
}
