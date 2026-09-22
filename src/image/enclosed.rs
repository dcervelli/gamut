//! Where a raw file that is not a TIFF at the front keeps the EXIF a TIFF
//! would keep at the front.
//!
//! Most camera formats are TIFFs under another name, and the metadata
//! reader reads them as it reads any TIFF. Five are not, and each keeps a
//! TIFF, or a JPEG with one in it, somewhere inside:
//!
//! - **ORF** and **RW2** are TIFFs with the version number replaced by a
//!   letter — Olympus's `RO`, Panasonic's `U\0` — and read as TIFFs once
//!   the four bytes are put back.
//! - **RAF** starts with a fixed header naming the camera, and at the offset
//!   the header gives, the camera's JPEG of the frame, whose `APP1` segment
//!   is the EXIF.
//! - **MRW** is a run of blocks, and the `TTW` block is a TIFF, byte for
//!   byte.
//! - **CR3** is an ISO base media file, and under a `uuid` box of Canon's
//!   in `moov` the metadata sits as four boxes, each a TIFF of its own with
//!   one directory: `CMT1` the image's, `CMT2` the Exif directory, `CMT3`
//!   the maker note, `CMT4` the GPS directory. Read one alone and its tags
//!   are numbers in the wrong directory — the Exif tags mean nothing in the
//!   image's — so the first, second and fourth are written back out as one
//!   TIFF, the way a camera writing a NEF lays them out, with the offsets
//!   moved to where the values now are.
//!
//! What comes back is a block for [`super::exif`] to read the way it reads
//! everything else, so the names, the units and the words for the
//! enumerations are the same code for every file: the same reasoning as
//! [`super::directory`]. Nothing here is a decoder; the pixels are LibRaw's.
//!
//! CRW is the one raw that has no EXIF anywhere in it — Canon's earlier
//! container, from before they wrote it — and is not here.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use super::isobmff;
use super::tiff::{self, Entry, Kind, Order, tag};

/// How much of a container is read looking for its block: the blocks are
/// all near the front — the `CMT` boxes inside the first 32 kB, an RAF's
/// JPEG at byte 148, an MRW's `TTW` inside the first few kB — and the
/// TIFF-shaped ones are read to the same prefix `exif` gives a TIFF.
const PREFIX: usize = tiff::PREFIX as usize;

/// The EXIF a container carries, in the form the reader takes it.
pub enum Block {
    /// A TIFF, to be read as one.
    Tiff(Vec<u8>),
    /// A JPEG, whose `APP1` segment is read.
    Jpeg(Vec<u8>),
}

/// The block inside `path`, or `None` for a file that is not one of the
/// five containers, or one whose block could not be found.
pub fn block(path: &Path) -> Option<Block> {
    let mut file = File::open(path).ok()?;
    let mut head = [0u8; 16];
    let read = file.read(&mut head).ok()?;
    let head = &head[..read];
    file.seek(SeekFrom::Start(0)).ok()?;

    if head.starts_with(b"IIRO") || head.starts_with(b"IIRS") || head.starts_with(b"IIU\0") {
        return Some(Block::Tiff(relabeled(
            &mut file,
            &tiff::signature(Order::Little, Kind::Classic),
        )?));
    }
    if head.starts_with(b"MMOR") {
        return Some(Block::Tiff(relabeled(
            &mut file,
            &tiff::signature(Order::Big, Kind::Classic),
        )?));
    }
    if head.starts_with(b"FUJIFILMCCD-RAW") {
        return raf(&mut file).map(Block::Jpeg);
    }
    if head.starts_with(b"\0MRM") {
        return mrw(&mut file).map(Block::Tiff);
    }
    if head.get(4..12) == Some(b"ftypcrx ") {
        return cr3(&mut file).map(Block::Tiff);
    }
    None
}

/// The prefix of the file with TIFF's own four bytes over the vendor's.
fn relabeled(file: &mut File, signature: &[u8; 4]) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    file.take(PREFIX as u64).read_to_end(&mut bytes).ok()?;
    if bytes.len() < 8 {
        return None;
    }
    bytes[..4].copy_from_slice(signature);
    Some(bytes)
}

/// The JPEG a RAF names in its header: an offset and a length, big-endian,
/// at bytes 84 and 88.
fn raf(file: &mut File) -> Option<Vec<u8>> {
    let mut header = [0u8; 92];
    file.read_exact(&mut header).ok()?;
    let offset = u32::from_be_bytes(header[84..88].try_into().ok()?);
    let length = u32::from_be_bytes(header[88..92].try_into().ok()?);
    if length as usize > PREFIX {
        return None;
    }
    read_at(file, u64::from(offset), length as usize)
}

/// The `TTW` block of an MRW: after the eight-byte file header, blocks of a
/// four-byte name and a big-endian length, the length not counting the
/// eight bytes of its own.
fn mrw(file: &mut File) -> Option<Vec<u8>> {
    let mut header = [0u8; 8];
    file.read_exact(&mut header).ok()?;
    let end = u64::from(u32::from_be_bytes(header[4..8].try_into().ok()?)) + 8;
    let mut at = 8;
    while at + 8 <= end {
        let mut block = [0u8; 8];
        file.seek(SeekFrom::Start(at)).ok()?;
        file.read_exact(&mut block).ok()?;
        let length = u32::from_be_bytes(block[4..8].try_into().ok()?) as usize;
        if &block[..4] == b"\0TTW" {
            return (length <= PREFIX)
                .then(|| read_at(file, at + 8, length))
                .flatten();
        }
        at += 8 + length as u64;
    }
    None
}

/// The `CMT1`, `CMT2` and `CMT4` boxes of a CR3, written back out as one
/// TIFF.
fn cr3(file: &mut File) -> Option<Vec<u8>> {
    // Everything wanted is in `moov`, which comes right after `ftyp`; the
    // pixels are in `mdat` after it, and nothing here goes near them.
    let mut bytes = Vec::new();
    file.take(PREFIX as u64).read_to_end(&mut bytes).ok()?;

    let (moov, _) = isobmff::find(&bytes, 0, bytes.len(), b"moov")?;
    let (uuid, uuid_end) = isobmff::find(&bytes, moov, bytes.len(), b"uuid")?;
    // Canon's uuid, the 16 bytes after the box header.
    const CANON: [u8; 16] = [
        0x85, 0xc0, 0xb6, 0x87, 0x82, 0x0f, 0x11, 0xe0, 0x81, 0x11, 0xf4, 0xce, 0x46, 0x2b, 0x6a,
        0x48,
    ];
    if bytes.get(uuid..uuid + 16)? != CANON {
        return None;
    }
    let body = |name: &[u8; 4]| {
        let (start, end) = isobmff::find(&bytes, uuid + 16, uuid_end, name)?;
        bytes.get(start..end)
    };
    let image = body(b"CMT1")?;
    let exif = body(b"CMT2");
    let gps = body(b"CMT4");
    combine(image, exif, gps)
}

/// The three directories as one TIFF. Each box is a TIFF of its own, its
/// values addressed from its own first byte; each is copied in whole at a
/// known place and its directory rewritten to address them from there.
/// The image directory comes first, with the pointers to the other two
/// added the way a camera writes them.
fn combine(image: &[u8], exif: Option<&[u8]>, gps: Option<&[u8]>) -> Option<Vec<u8>> {
    let order = Order::of(image)?;
    let mut out = Vec::with_capacity(
        image.len() + exif.map_or(0, <[u8]>::len) + gps.map_or(0, <[u8]>::len) + 256,
    );

    // The header, pointing at the directory written next.
    out.extend_from_slice(&image[..4]);
    out.extend_from_slice(&order.u32(8));

    // The image directory: its entries, then the two pointers. The bodies
    // are appended after every directory, so where each will land is
    // worked out first.
    let image_entries = tiff::entries(image, order, &POINTERS)?;
    let exif_entries = exif.and_then(|body| tiff::entries(body, order, &POINTERS));
    let gps_entries = gps.and_then(|body| tiff::entries(body, order, &POINTERS));
    let pointers = usize::from(exif_entries.is_some()) + usize::from(gps_entries.is_some());
    let directory_size = |entries: &[Entry]| 2 + 12 * entries.len() + 4;

    let image_at = 8;
    let exif_at = image_at + directory_size(&image_entries) + 12 * pointers;
    let gps_at = exif_at + exif_entries.as_ref().map_or(0, |e| directory_size(e));
    let mut body_at = gps_at + gps_entries.as_ref().map_or(0, |e| directory_size(e));

    let image_base = body_at;
    body_at += image.len();
    let exif_base = body_at;
    body_at += exif.map_or(0, <[u8]>::len);
    let gps_base = body_at;

    let write = |out: &mut Vec<u8>, entries: &[Entry], base: usize, extra: &[(u16, usize)]| {
        out.extend_from_slice(&order.u16((entries.len() + extra.len()) as u16));
        // A directory is searched in ascending tag order; the pointers'
        // tags are higher than the image's own, so they go last.
        for entry in entries {
            out.extend_from_slice(&order.u16(entry.tag));
            out.extend_from_slice(&order.u16(entry.kind));
            out.extend_from_slice(&order.u32(entry.count));
            match entry.inline {
                Some(inline) => out.extend_from_slice(&inline),
                None => out.extend_from_slice(&order.u32((base + entry.offset) as u32)),
            }
        }
        for (tag, at) in extra {
            out.extend_from_slice(&order.u16(*tag));
            out.extend_from_slice(&order.u16(4));
            out.extend_from_slice(&order.u32(1));
            out.extend_from_slice(&order.u32(*at as u32));
        }
        out.extend_from_slice(&order.u32(0));
    };

    let mut extra = Vec::new();
    if exif_entries.is_some() {
        extra.push((0x8769, exif_at));
    }
    if gps_entries.is_some() {
        extra.push((0x8825, gps_at));
    }
    write(&mut out, &image_entries, image_base, &extra);
    if let Some(entries) = &exif_entries {
        write(&mut out, entries, exif_base, &[]);
    }
    if let Some(entries) = &gps_entries {
        write(&mut out, entries, gps_base, &[]);
    }
    debug_assert_eq!(out.len(), image_base);
    out.extend_from_slice(image);
    if let Some(body) = exif {
        out.extend_from_slice(body);
    }
    if let Some(body) = gps {
        out.extend_from_slice(body);
    }
    Some(out)
}

/// The tags that point at another directory. Each holds an offset into the
/// box it came from, where there is nothing this reader wants: the Exif and
/// GPS directories arrive as boxes of their own and are pointed at afresh,
/// and the interoperability directory says nothing worth the trip.
const POINTERS: [u16; 3] = [tag::EXIF_IFD, tag::GPS_IFD, tag::INTEROP_IFD];

fn read_at(file: &mut File, offset: u64, length: usize) -> Option<Vec<u8>> {
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut bytes = vec![0; length];
    file.read_exact(&mut bytes).ok()?;
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-directory TIFF of the given entries, as a CR3 box holds one:
    /// each entry `(tag, kind, count, value or bytes)`, the long values
    /// placed after the directory.
    fn tiff(entries: &[(u16, u16, u32, Vec<u8>)]) -> Vec<u8> {
        let mut out = b"II\x2a\x00\x08\x00\x00\x00".to_vec();
        out.extend((entries.len() as u16).to_le_bytes());
        let mut tail = Vec::new();
        let tail_at = 8 + 2 + 12 * entries.len() + 4;
        for (tag, kind, count, value) in entries {
            out.extend(tag.to_le_bytes());
            out.extend(kind.to_le_bytes());
            out.extend(count.to_le_bytes());
            if value.len() <= 4 {
                let mut inline = [0u8; 4];
                inline[..value.len()].copy_from_slice(value);
                out.extend(inline);
            } else {
                out.extend(((tail_at + tail.len()) as u32).to_le_bytes());
                tail.extend_from_slice(value);
            }
        }
        out.extend(0u32.to_le_bytes());
        out.extend(tail);
        out
    }

    /// The combined block reads back through the metadata reader with the
    /// image's tags in the image directory and the Exif tags in the Exif
    /// directory, the long values reached at their moved offsets.
    #[test]
    fn a_cr3s_boxes_combine_into_one_readable_tiff() {
        let image = tiff(&[
            (0x010F, 2, 6, b"Canon\0".to_vec()),
            (0x0112, 3, 1, vec![6, 0]),
            // A pointer into the box, which must not survive.
            (0x8769, 4, 1, vec![0xff, 0xff, 0, 0]),
        ]);
        let exif = tiff(&[
            (0x829A, 5, 1, vec![1, 0, 0, 0, 200, 0, 0, 0]),
            (0x8827, 3, 1, vec![100, 0]),
        ]);
        let gps = tiff(&[(0x0001, 2, 2, b"N\0".to_vec())]);

        let block = combine(&image, Some(&exif), Some(&gps)).unwrap();
        let parsed = exif::Reader::new().read_raw(block).unwrap();

        let field = |tag| parsed.get_field(tag, exif::In::PRIMARY);
        assert_eq!(
            field(exif::Tag::Make).unwrap().display_value().to_string(),
            "\"Canon\""
        );
        assert_eq!(
            field(exif::Tag::Orientation)
                .unwrap()
                .display_value()
                .to_string(),
            "row 0 at right and column 0 at top"
        );
        assert_eq!(
            field(exif::Tag::ExposureTime)
                .unwrap()
                .display_value()
                .to_string(),
            "1/200"
        );
        assert_eq!(
            field(exif::Tag::PhotographicSensitivity)
                .unwrap()
                .display_value()
                .to_string(),
            "100"
        );
        assert_eq!(
            field(exif::Tag::GPSLatitudeRef)
                .unwrap()
                .display_value()
                .to_string(),
            "N"
        );
    }

    /// Without the Exif box the image directory stands alone, and a box in
    /// the wrong byte order is not read.
    #[test]
    fn a_missing_or_foreign_box_is_left_out() {
        let image = tiff(&[(0x010F, 2, 6, b"Canon\0".to_vec())]);
        let block = combine(&image, None, None).unwrap();
        let parsed = exif::Reader::new().read_raw(block).unwrap();
        assert_eq!(parsed.fields().count(), 1);

        let mut foreign = tiff(&[(0x829A, 3, 1, vec![1, 0])]);
        foreign[..4].copy_from_slice(b"MM\x00\x2a");
        let block = combine(&image, Some(&foreign), None).unwrap();
        let parsed = exif::Reader::new().read_raw(block).unwrap();
        assert!(
            parsed
                .get_field(exif::Tag::ExposureTime, exif::In::PRIMARY)
                .is_none()
        );
    }
}
