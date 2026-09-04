//! A TIFF's directory, read by the decoder and written back out as the block
//! the metadata reader knows.
//!
//! Two files need this. A BigTIFF — the same tags and the same types, with
//! eight-byte offsets so that a file may pass four gigabytes, which is how an
//! elevation model or a scanned map is written as a matter of course — is a
//! form the metadata reader does not know at all, EXIF being defined on the
//! original. And an ordinary TIFF that keeps its directory past the end of
//! the prefix [`super::exif`] hands that reader is one the reader cannot
//! reach the front of.
//!
//! Nothing here parses a file. The TIFF decoder reads both forms, is already
//! in the tree, and seeks to the directory wherever it is; this writes what
//! it found back out as an ordinary block, in memory, holding the values but
//! none of the pixels. Everything downstream — the names, the units, the
//! words for the enumerations, the georeference — is then the same code for
//! every file, which is the point of doing it this way rather than rendering
//! a second kind of directory a second way.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use tiff::decoder::Decoder;
use tiff::decoder::ifd::Value;

use super::exif::MAX_COMPONENTS;

/// How large the rewritten block may be. It holds one directory of small
/// values — the long ones are left behind with everything else bulky — so
/// this is a ceiling rather than a size anything reaches.
const MAX_BLOCK: usize = 64 * 1024;

/// The pointer tags, which are dropped.
///
/// Each holds the offset of another directory in the file being read, and the
/// block written here is not that file: a pointer copied across would send
/// the reader into whatever happened to be at that offset. A camera writing
/// its exposure into a BigTIFF is not a file anyone has; a raster is, and a
/// raster keeps everything it says in the first directory.
const POINTERS: [u16; 3] = [330, 34665, 34853];

/// Reads `path`'s first directory and writes it back out as an ordinary TIFF
/// block. `None` for a file that will not open, or that has nothing in it
/// worth writing out.
pub fn block(path: &Path) -> Option<Vec<u8>> {
    let file = File::open(path).ok()?;
    // Only the header and the first directory are read; the decoder does not
    // touch a pixel until it is asked for one.
    let mut decoder = Decoder::new(BufReader::new(file)).ok()?;

    let mut fields: Vec<(u16, Value)> = decoder
        .tag_iter()
        .filter_map(|field| field.ok())
        .map(|(tag, value)| (tag.to_u16(), value))
        .filter(|(tag, _)| !POINTERS.contains(tag))
        .collect();
    // A directory is written in ascending tag order, which is also the order
    // the reader expects to be able to search in.
    fields.sort_by_key(|(tag, _)| *tag);
    write(&fields)
}

/// The block: a header, one directory, and the values too large to sit in an
/// entry gathered after it.
fn write(fields: &[(u16, Value)]) -> Option<Vec<u8>> {
    let mut entries: Vec<[u8; 12]> = Vec::new();
    let mut pool: Vec<u8> = Vec::new();

    // Where the pool will start: the header, the count, one entry per field,
    // and the offset of the directory after this one.
    let start = 8 + 2 + fields.len() * 12 + 4;

    for (tag, value) in fields {
        let Some((kind, count, bytes)) = encode(value) else {
            continue;
        };
        if count > MAX_COMPONENTS || pool.len() + bytes.len() > MAX_BLOCK {
            continue;
        }
        let mut entry = [0u8; 12];
        entry[0..2].copy_from_slice(&tag.to_le_bytes());
        entry[2..4].copy_from_slice(&kind.to_le_bytes());
        entry[4..8].copy_from_slice(&(count as u32).to_le_bytes());
        if bytes.len() <= 4 {
            entry[8..8 + bytes.len()].copy_from_slice(&bytes);
        } else {
            // Values start on an even byte, as the format requires.
            if !pool.len().is_multiple_of(2) {
                pool.push(0);
            }
            entry[8..12].copy_from_slice(&((start + pool.len()) as u32).to_le_bytes());
            pool.extend_from_slice(&bytes);
        }
        entries.push(entry);
    }
    if entries.is_empty() {
        return None;
    }

    // The entries were laid out against a pool starting after all of the
    // fields, and some were dropped; writing the header for the entries that
    // survived would move the pool out from under the offsets already
    // written. The gap is left where it is instead — the reader is following
    // offsets, not measuring the block.
    let mut block = b"II\x2a\x00\x08\x00\x00\x00".to_vec();
    block.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for entry in &entries {
        block.extend_from_slice(entry);
    }
    block.extend_from_slice(&0u32.to_le_bytes());
    block.resize(start, 0);
    block.extend_from_slice(&pool);
    Some(block)
}

/// One value as the type code, the number of components, and the bytes a
/// directory entry would hold.
///
/// `None` for a value there is no place for in the original format: an offset
/// that only a BigTIFF can hold, a number too large for the type it would
/// have to be written as, or a list whose members disagree about what they
/// are. Each of those is a value nothing downstream would have made sense of
/// either.
fn encode(value: &Value) -> Option<(u16, usize, Vec<u8>)> {
    if let Value::List(items) = value {
        let mut kind = None;
        let mut bytes = Vec::new();
        for item in items {
            let (item_kind, count, item_bytes) = encode(item)?;
            // A list of lists, or of strings, is not something a directory
            // entry can hold.
            if count != 1 || *kind.get_or_insert(item_kind) != item_kind {
                return None;
            }
            bytes.extend_from_slice(&item_bytes);
        }
        return Some((kind?, items.len(), bytes));
    }
    Some(match value {
        Value::Byte(number) => (1, 1, number.to_le_bytes().to_vec()),
        Value::Ascii(text) => {
            // As a directory holds it: the characters, and the null that ends
            // them, counted together.
            let mut bytes = text.as_bytes().to_vec();
            bytes.push(0);
            (2, bytes.len(), bytes)
        }
        Value::Short(number) => (3, 1, number.to_le_bytes().to_vec()),
        Value::Unsigned(number) => (4, 1, number.to_le_bytes().to_vec()),
        Value::Rational(numerator, denominator) => (
            5,
            1,
            [numerator.to_le_bytes(), denominator.to_le_bytes()].concat(),
        ),
        Value::SignedByte(number) => (6, 1, number.to_le_bytes().to_vec()),
        Value::SignedShort(number) => (8, 1, number.to_le_bytes().to_vec()),
        Value::Signed(number) => (9, 1, number.to_le_bytes().to_vec()),
        Value::SRational(numerator, denominator) => (
            10,
            1,
            [numerator.to_le_bytes(), denominator.to_le_bytes()].concat(),
        ),
        Value::Float(number) => (11, 1, number.to_le_bytes().to_vec()),
        Value::Double(number) => (12, 1, number.to_le_bytes().to_vec()),
        // The wide integers, where the number itself is not wide.
        Value::UnsignedBig(number) => (4, 1, u32::try_from(*number).ok()?.to_le_bytes().to_vec()),
        Value::SignedBig(number) => (9, 1, i32::try_from(*number).ok()?.to_le_bytes().to_vec()),
        // A pointer to another directory, and whatever a later version of the
        // format adds: neither is a fact about the image.
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The block a reader is handed has to be one it can walk: the header it
    /// expects, one directory, and every offset landing on the value it was
    /// written for.
    #[test]
    fn a_directory_is_written_as_the_original_format_holds_one() {
        let block = write(&[
            (256, Value::Unsigned(5500)),
            (270, Value::Ascii("an elevation model".into())),
            (
                33550,
                Value::List(vec![
                    Value::Double(10.0),
                    Value::Double(10.0),
                    Value::Double(0.0),
                ]),
            ),
        ])
        .expect("three fields to write");

        assert_eq!(&block[..4], b"II\x2a\x00");
        assert_eq!(u32::from_le_bytes(block[4..8].try_into().unwrap()), 8);
        assert_eq!(u16::from_le_bytes(block[8..10].try_into().unwrap()), 3);

        // Each entry, read back the way the reader will read it.
        let entry = |index: usize| {
            let at = 10 + index * 12;
            let tag = u16::from_le_bytes(block[at..at + 2].try_into().unwrap());
            let kind = u16::from_le_bytes(block[at + 2..at + 4].try_into().unwrap());
            let count = u32::from_le_bytes(block[at + 4..at + 8].try_into().unwrap()) as usize;
            let size = match kind {
                2 => 1,
                4 => 4,
                12 => 8,
                other => panic!("unexpected type {other}"),
            };
            let length = size * count;
            let bytes = if length <= 4 {
                block[at + 8..at + 8 + length].to_vec()
            } else {
                let offset =
                    u32::from_le_bytes(block[at + 8..at + 12].try_into().unwrap()) as usize;
                block[offset..offset + length].to_vec()
            };
            (tag, kind, count, bytes)
        };

        // In ascending tag order, whatever order they arrived in.
        assert_eq!(entry(0).0, 256);
        assert_eq!(entry(1).0, 270);
        assert_eq!(entry(2).0, 33550);

        // A long fits in the entry; a string and three doubles do not.
        let (_, kind, count, bytes) = entry(0);
        assert_eq!((kind, count), (4, 1));
        assert_eq!(u32::from_le_bytes(bytes.try_into().unwrap()), 5500);

        let (_, kind, count, bytes) = entry(1);
        assert_eq!((kind, count), (2, 19), "the null is counted");
        assert_eq!(&bytes[..18], b"an elevation model");

        let (_, kind, count, bytes) = entry(2);
        assert_eq!((kind, count), (12, 3));
        assert_eq!(
            f64::from_le_bytes(bytes[..8].try_into().unwrap()),
            10.0,
            "a pixel is ten meters"
        );
    }

    /// What cannot be written is left out rather than written wrongly, and a
    /// directory of nothing else is no block at all.
    #[test]
    fn a_value_the_original_format_cannot_hold_is_left_out() {
        // A pointer into a file this block is not, a number too wide for the
        // type it would take, and a list that cannot agree what it holds.
        for value in [
            Value::IfdBig(1 << 40),
            Value::UnsignedBig(u64::MAX),
            Value::List(vec![Value::Short(1), Value::Double(2.0)]),
            Value::List(vec![Value::Ascii("a".into())]),
        ] {
            assert!(encode(&value).is_none(), "{value:?}");
            assert!(write(&[(256, value)]).is_none());
        }

        // And the bulky ones: a table of tile offsets says how the file is
        // laid out, not what the image is.
        let offsets = Value::List(vec![Value::Unsigned(0); MAX_COMPONENTS + 1]);
        assert!(encode(&offsets).is_some(), "it encodes, and is dropped");
        assert!(write(&[(324, offsets)]).is_none());
    }
}
