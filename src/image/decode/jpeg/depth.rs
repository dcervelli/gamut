//! Google's depth map: the `GDepth` block a phone's portrait or a Lens Blur
//! photograph carries in its XMP.
//!
//! The block says how the map was normalized — linearly or by the inverse of
//! the distance, between a near and a far plane, in meters or millimeters —
//! and holds the map itself as a base64 PNG or JPEG. A map is far longer than
//! the 64 KB one `APP1` segment can hold, so the packet that names the block
//! points by a GUID at an extended packet, split over as many segments as it
//! takes under XMP's extension header, each saying where in the whole its
//! piece goes. The walk here gathers both, and nothing past the scan.

use std::io::Cursor;
use std::sync::Arc;

use ::image::{DynamicImage, ImageReader};

use crate::image::depth::{DepthMap, Range, Unit};
use crate::image::xmp::Xmp;
use crate::image::{Channels, Samples};

/// Google's namespace for the depth map's properties.
const GDEPTH: &str = "http://ns.google.com/photos/1.0/depthmap/";

/// XMP's note namespace, where the main packet names the extended one.
const XMP_NOTE: &str = "http://ns.adobe.com/xmp/note/";

/// The header an `APP1` segment holding the main packet wears.
const MAIN_HEADER: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";

/// The header each piece of the extended packet wears, followed by the
/// GUID, the whole's length and the piece's place in it.
const EXTENSION_HEADER: &[u8] = b"http://ns.adobe.com/xmp/extension/\0";

/// The GUID's length: 32 hexadecimal digits.
const GUID: usize = 32;

/// The depth map a JPEG's XMP carries, or `None` where it carries none this
/// can read. A map that will not decode costs the readout its depth and
/// nothing else, so every failure here is a `None`.
pub fn find(bytes: &[u8]) -> Option<Arc<DepthMap>> {
    let (main, extensions) = segments(bytes);
    let main = Xmp::parse(main?)?;
    let property = |xmp: &Xmp, name: &str| first(xmp, GDEPTH, name);
    // The data is in the main packet where it fits, which it seldom does,
    // and otherwise in the extended packet the main one names.
    let data = property(&main, "Data").or_else(|| {
        let guid = first(&main, XMP_NOTE, "HasExtendedXMP")?;
        let extended = Xmp::parse(&extended(&extensions, guid.as_bytes())?)?;
        property(&extended, "Data")
    })?;
    let encoded = base64(data.as_bytes())?;
    let (width, height, samples) = decode(&encoded)?;

    let number = |name| {
        property(&main, name)
            .and_then(|value| value.trim().parse::<f32>().ok())
            .filter(|value| value.is_finite())
    };
    let unit = property(&main, "Units")
        .map(|word| Unit::parse(&word))
        .unwrap_or(Unit::Unknown);
    let range = match (
        property(&main, "Format").as_deref(),
        number("Near"),
        number("Far"),
    ) {
        (Some("RangeLinear"), Some(near), Some(far)) => Range::Linear { near, far, unit },
        (Some("RangeInverse"), Some(near), Some(far)) => Range::Inverse { near, far, unit },
        _ => Range::Unstated,
    };
    Some(Arc::new(DepthMap {
        width,
        height,
        samples,
        range,
    }))
}

/// The one value of a plain property.
fn first(xmp: &Xmp, namespace: &str, name: &str) -> Option<String> {
    xmp.property(namespace, name)
        .and_then(|values| values.first())
        .cloned()
}

/// One piece of an extended packet, as its segment states it.
struct Piece<'a> {
    guid: &'a [u8],
    /// The whole packet's length.
    length: u32,
    /// Where in the whole this piece goes.
    offset: u32,
    bytes: &'a [u8],
}

/// The main packet, and every piece of an extended one, from the `APP1`
/// segments ahead of the scan.
fn segments(bytes: &[u8]) -> (Option<&[u8]>, Vec<Piece<'_>>) {
    let mut main = None;
    let mut pieces = Vec::new();
    let mut at = 2;
    while let (Some(&0xff), Some(&marker)) = (bytes.get(at), bytes.get(at + 1)) {
        match marker {
            0xff => {
                at += 1;
                continue;
            }
            0xda | 0xd9 => break,
            0x01 | 0xd0..=0xd7 => {
                at += 2;
                continue;
            }
            _ => {}
        }
        let Some(length) = bytes.get(at + 2..at + 4) else {
            break;
        };
        let length = usize::from(u16::from_be_bytes([length[0], length[1]]));
        let Some(payload) = length
            .checked_sub(2)
            .and_then(|size| bytes.get(at + 4..at + 4 + size))
        else {
            break;
        };
        if marker == 0xe1 {
            if let Some(packet) = payload.strip_prefix(MAIN_HEADER) {
                main.get_or_insert(packet);
            } else if let Some(rest) = payload.strip_prefix(EXTENSION_HEADER)
                && rest.len() >= GUID + 8
            {
                let word = |from: usize| {
                    u32::from_be_bytes([rest[from], rest[from + 1], rest[from + 2], rest[from + 3]])
                };
                pieces.push(Piece {
                    guid: &rest[..GUID],
                    length: word(GUID),
                    offset: word(GUID + 4),
                    bytes: &rest[GUID + 8..],
                });
            }
        }
        at += 2 + length;
    }
    (main, pieces)
}

/// The extended packet `guid` names, put together from its pieces. `None`
/// where a piece would land outside the length they state, or where the
/// pieces leave a gap.
fn extended(pieces: &[Piece], guid: &[u8]) -> Option<Vec<u8>> {
    let mine = || pieces.iter().filter(|piece| piece.guid == guid);
    let length = mine().next()?.length as usize;
    // The pieces are in the file already, so a whole longer than the file is
    // a length nothing wrote.
    let held: usize = mine().map(|piece| piece.bytes.len()).sum();
    if held < length {
        return None;
    }
    let mut whole = vec![0u8; length];
    let mut filled = 0;
    for piece in mine() {
        let offset = piece.offset as usize;
        whole
            .get_mut(offset..offset.checked_add(piece.bytes.len())?)?
            .copy_from_slice(piece.bytes);
        filled += piece.bytes.len();
    }
    (filled == length).then_some(whole)
}

/// Standard base64, with the line breaks and spaces a packet may carry
/// passed over. `None` for anything else that is not in the alphabet.
fn base64(text: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut buffer = 0u32;
    let mut bits = 0;
    for &byte in text {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b' ' | b'\t' | b'\r' | b'\n' => continue,
            _ => return None,
        };
        buffer = (buffer << 6 | u32::from(value)) & 0xffff;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    Some(out)
}

/// The map's picture decoded to one channel, at the width the file stored:
/// sixteen bits where it has them, and eight otherwise.
fn decode(encoded: &[u8]) -> Option<(u32, u32, Samples)> {
    let mut reader = ImageReader::new(Cursor::new(encoded))
        .with_guessed_format()
        .ok()?;
    super::super::dynamic::limit(&mut reader);
    let image = reader.decode().ok()?;
    let (width, height) = (image.width(), image.height());
    if width == 0 || height == 0 {
        return None;
    }
    let channels = Channels::Gray;
    let samples = match image {
        DynamicImage::ImageLuma16(_)
        | DynamicImage::ImageLumaA16(_)
        | DynamicImage::ImageRgb16(_)
        | DynamicImage::ImageRgba16(_) => Samples::U16 {
            channels,
            data: image.into_luma16().into_raw(),
        },
        _ => Samples::U8 {
            channels,
            data: image.into_luma8().into_raw(),
        },
    };
    Some((width, height, samples))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_reads_through_line_breaks_and_padding() {
        assert_eq!(base64(b"aGVsbG8="), Some(b"hello".to_vec()));
        assert_eq!(base64(b"aGVs\n bG8h"), Some(b"hello!".to_vec()));
        assert_eq!(base64(b"aGVs*G8="), None);
    }

    /// The pieces of an extended packet go back where each says it goes,
    /// in whatever order they came; a gap, or a piece past the end, is no
    /// packet.
    #[test]
    fn an_extended_packet_is_put_back_together() {
        let guid = b"0123456789ABCDEF0123456789ABCDEF".as_slice();
        let other = b"FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF".as_slice();
        let piece = |guid, length, offset, bytes| Piece {
            guid,
            length,
            offset,
            bytes,
        };
        let pieces = [
            piece(guid, 6, 3, b"def".as_slice()),
            piece(other, 3, 0, b"xyz".as_slice()),
            piece(guid, 6, 0, b"abc".as_slice()),
        ];
        assert_eq!(extended(&pieces, guid), Some(b"abcdef".to_vec()));
        assert_eq!(extended(&pieces[..2], guid), None, "a gap");
        let past = [piece(guid, 3, 2, b"abc".as_slice())];
        assert_eq!(extended(&past, guid), None, "past the end");
    }
}
