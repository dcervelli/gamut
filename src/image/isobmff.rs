//! The ISO base media file format's boxes: a big-endian length counting
//! the header, then a four-byte type, with a length of one meaning an
//! eight-byte length follows and a length of zero meaning the box runs to
//! the end. [`top_level`] walks a file's own boxes as a stream, seeking
//! past each until the one wanted, so that a HEIF's `meta` box, a Canon
//! CR3's `moov` and a JPEG XL container's `xml ` box are each read into
//! memory alone; [`boxes`] and [`find`] walk the boxes inside one of those;
//! and [`Fields`] reads the fields of a box's body in order, at the widths
//! its own header chose. What a HEIF's item tables and an ISO 21496-1
//! gain-map item are read through.

use std::io::SeekFrom;

use anyhow::{Context, Result, anyhow};

use crate::image::decode::ReadSeek;

/// The body of the first top-level box named `want`, read from the start
/// of `source`. The boxes before it are seeked past by their lengths, and
/// the walk ends — as `None` — at the end of the file, at a box named
/// `stop` (`mdat`, past which nothing but pixels lies), or at a header
/// that does not add up. A body longer than `max` is `None` too: what a
/// `meta` box or an `xml ` box may reasonably run to is known, and a file
/// claiming more is not carrying what is looked for.
pub fn top_level(
    source: &mut dyn ReadSeek,
    want: &[u8; 4],
    stop: Option<&[u8; 4]>,
    max: u64,
) -> Result<Option<Vec<u8>>> {
    source.seek(SeekFrom::Start(0))?;
    let mut at = 0u64;
    loop {
        let mut header = [0u8; 8];
        if source.read_exact(&mut header).is_err() {
            return Ok(None);
        }
        let mut size = u64::from(u32::from_be_bytes([
            header[0], header[1], header[2], header[3],
        ]));
        let kind = &header[4..8];
        let mut header_len = 8u64;
        if size == 1 {
            let mut large = [0u8; 8];
            if source.read_exact(&mut large).is_err() {
                return Ok(None);
            }
            size = u64::from_be_bytes(large);
            header_len = 16;
        } else if size == 0 {
            // To the end of the file: only ever the last box.
            size = source.seek(SeekFrom::End(0))?.saturating_sub(at);
        }
        if size < header_len {
            return Ok(None);
        }
        if kind == want {
            let length = size - header_len;
            if length > max {
                return Ok(None);
            }
            let mut body = vec![0; length as usize];
            source.seek(SeekFrom::Start(at + header_len))?;
            source
                .read_exact(&mut body)
                .with_context(|| format!("reading the {} box", String::from_utf8_lossy(want)))?;
            return Ok(Some(body));
        }
        if stop.is_some_and(|stop| kind == stop) {
            return Ok(None);
        }
        at = at
            .checked_add(size)
            .ok_or_else(|| anyhow!("box runs off the file"))?;
        source.seek(SeekFrom::Start(at))?;
    }
}

/// The boxes between `from` and `to` in `bytes`: each one's type and the
/// range of its body. A box that does not add up — shorter than its own
/// header, or with a length past what a size can hold — ends the walk, as
/// does one running past `to`, whose body is cut there.
pub fn boxes(bytes: &[u8], mut from: usize, to: usize) -> Vec<([u8; 4], usize, usize)> {
    let mut out = Vec::new();
    while from + 8 <= to {
        let Some(head) = bytes.get(from..from + 8) else {
            break;
        };
        let Some((size, kind)) = head.split_first_chunk::<4>() else {
            break;
        };
        let Some(kind) = kind.first_chunk::<4>() else {
            break;
        };
        let mut size = u32::from_be_bytes(*size) as usize;
        let mut header = 8;
        if size == 1 {
            let Some(large) = bytes
                .get(from + 8..from + 16)
                .and_then(|large| large.first_chunk::<8>())
            else {
                break;
            };
            let Ok(large) = usize::try_from(u64::from_be_bytes(*large)) else {
                break;
            };
            size = large;
            header = 16;
        } else if size == 0 {
            size = to - from;
        }
        // A header that runs past the range is a box that is not in it.
        if from + header > to || size < header {
            break;
        }
        let Some(end) = from.checked_add(size) else {
            break;
        };
        let end = end.min(to);
        out.push((*kind, from + header, end));
        from = end;
    }
    out
}

/// The body of the first box named `name` between `from` and `to`, as the
/// range of its bytes after the header.
pub fn find(bytes: &[u8], from: usize, to: usize, name: &[u8; 4]) -> Option<(usize, usize)> {
    boxes(bytes, from, to)
        .into_iter()
        .find(|(kind, _, _)| kind == name)
        .map(|(_, start, end)| (start, end))
}

/// The fields of a box's body, read in order as the format writes them:
/// big-endian, at the widths a table's own header chose. Each read answers
/// `None` past the end rather than reading past it.
pub struct Fields<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Fields<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    /// Where the next field is, from the body's first byte.
    pub fn at(&self) -> usize {
        self.at
    }

    /// Past `count` bytes that are not wanted: a full box's flags, a
    /// reserved field.
    pub fn skip(&mut self, count: usize) {
        self.at += count;
    }

    /// The next `count` bytes as they are.
    pub fn bytes(&mut self, count: usize) -> Option<&'a [u8]> {
        let value = self.bytes.get(self.at..self.at.checked_add(count)?)?;
        self.at += count;
        Some(value)
    }

    pub fn u8(&mut self) -> Option<u8> {
        self.bytes(1).map(|value| value[0])
    }

    pub fn u16(&mut self) -> Option<u16> {
        self.bytes(2)
            .and_then(|value| value.first_chunk::<2>())
            .map(|value| u16::from_be_bytes(*value))
    }

    pub fn u32(&mut self) -> Option<u32> {
        self.bytes(4)
            .and_then(|value| value.first_chunk::<4>())
            .map(|value| u32::from_be_bytes(*value))
    }

    pub fn i32(&mut self) -> Option<i32> {
        self.u32().map(|value| value as i32)
    }

    pub fn u64(&mut self) -> Option<u64> {
        self.bytes(8)
            .and_then(|value| value.first_chunk::<8>())
            .map(|value| u64::from_be_bytes(*value))
    }

    /// A field whose width the table's header chose: 0, 4 or 8 bytes.
    pub fn sized(&mut self, size: u8) -> Option<u64> {
        match size {
            0 => Some(0),
            4 => self.u32().map(u64::from),
            8 => self.u64(),
            _ => None,
        }
    }

    /// An item id, 16 bits before version `wide_from` of a table and 32
    /// from it.
    pub fn id(&mut self, version: u8, wide_from: u8) -> Option<u32> {
        if version >= wide_from {
            self.u32()
        } else {
            self.u16().map(u32::from)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed(name: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = ((8 + body.len()) as u32).to_be_bytes().to_vec();
        out.extend(name);
        out.extend(body);
        out
    }

    /// Boxes are walked by length and found by name, inside one another.
    #[test]
    fn boxes_are_walked_by_length_and_name() {
        let mut bytes = boxed(b"ftyp", b"crx \0\0\0\x01");
        bytes.extend(boxed(b"moov", &boxed(b"CMT1", b"abcd")));
        let (moov, end) = find(&bytes, 0, bytes.len(), b"moov").unwrap();
        assert_eq!((moov, end), (24, 36));
        assert_eq!(find(&bytes, moov, end, b"CMT1"), Some((32, 36)));
        assert_eq!(find(&bytes, 0, bytes.len(), b"mdat"), None);
        assert_eq!(
            boxes(&bytes, 0, bytes.len())
                .iter()
                .map(|(kind, ..)| *kind)
                .collect::<Vec<_>>(),
            [*b"ftyp", *b"moov"]
        );
    }

    /// The two long forms: an eight-byte length, and a box that runs to
    /// the end.
    #[test]
    fn the_long_and_the_open_ended_lengths_are_read() {
        let mut bytes = 1u32.to_be_bytes().to_vec();
        bytes.extend(b"mdat");
        bytes.extend(20u64.to_be_bytes());
        bytes.extend(b"pixl");
        bytes.extend(0u32.to_be_bytes());
        bytes.extend(b"free");
        bytes.extend(b"rest of the file");
        assert_eq!(
            boxes(&bytes, 0, bytes.len()),
            [(*b"mdat", 16, 20), (*b"free", 28, bytes.len())]
        );
    }

    /// The top-level walk over a reader finds a box by name past the ones
    /// before it, stops at the box it is told to, refuses a body over its
    /// ceiling, and reads a box that runs to the end of the file to there.
    #[test]
    fn a_top_level_box_is_found_by_seeking_past_the_others() {
        let mut bytes = boxed(b"ftyp", b"jxl     jxl ");
        bytes.extend(boxed(b"jxlp", &[7; 40]));
        bytes.extend(boxed(b"xml ", b"<x/>"));
        let mut file = std::io::Cursor::new(bytes.clone());
        assert_eq!(
            top_level(&mut file, b"xml ", None, 64).unwrap().as_deref(),
            Some(&b"<x/>"[..])
        );
        assert_eq!(
            top_level(&mut file, b"xml ", Some(b"jxlp"), 64).unwrap(),
            None,
            "stopped short of it"
        );
        assert_eq!(
            top_level(&mut file, b"xml ", None, 3).unwrap(),
            None,
            "over the ceiling"
        );
        assert_eq!(top_level(&mut file, b"free", None, 64).unwrap(), None);

        let mut open = boxed(b"ftyp", b"heic");
        open.extend(0u32.to_be_bytes());
        open.extend(b"meta");
        open.extend(b"to the end");
        assert_eq!(
            top_level(&mut std::io::Cursor::new(open), b"meta", None, 64)
                .unwrap()
                .as_deref(),
            Some(&b"to the end"[..])
        );

        // Cut off in a header, and a header shorter than itself.
        assert_eq!(
            top_level(&mut std::io::Cursor::new(&bytes[..20]), b"xml ", None, 64).unwrap(),
            None
        );
        let mut short = bytes.clone();
        short[..4].copy_from_slice(&2u32.to_be_bytes());
        assert_eq!(
            top_level(&mut std::io::Cursor::new(short), b"xml ", None, 64).unwrap(),
            None
        );
    }

    /// The fields of a body come out in order, at the widths asked, and
    /// stop at the end.
    #[test]
    fn fields_are_read_in_order_and_stop_at_the_end() {
        let body = [1u8, 0, 2, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 4, 9];
        let mut fields = Fields::new(&body);
        assert_eq!(fields.u8(), Some(1));
        assert_eq!(fields.u16(), Some(2));
        assert_eq!(fields.sized(4), Some(3));
        assert_eq!(fields.at(), 7);
        assert_eq!(fields.sized(8), Some(4));
        assert_eq!(fields.sized(0), Some(0));
        assert_eq!(fields.sized(2), None);
        assert_eq!(fields.bytes(1), Some(&[9][..]));
        assert_eq!(fields.u8(), None);
        let mut ids = Fields::new(&[0, 5, 0, 0, 0, 6]);
        assert_eq!(ids.id(0, 1), Some(5));
        assert_eq!(ids.id(2, 1), Some(6));
        let mut skipped = Fields::new(&[0, 0, 0, 8]);
        skipped.skip(3);
        assert_eq!(skipped.u8(), Some(8));
    }

    /// A box whose sixteen-byte header runs past the end of the range,
    /// with bytes beyond the range for the header to be read from, ends the
    /// walk rather than being handed back with its body starting past its
    /// end.
    #[test]
    fn a_long_header_past_the_range_ends_the_walk() {
        let mut bytes = boxed(b"ftyp", &[0; 8]);
        bytes.extend(1u32.to_be_bytes());
        bytes.extend(b"mdat");
        bytes.extend(40u64.to_be_bytes());
        bytes.extend([0; 24]);
        // The range ends inside the second box's long length field.
        assert_eq!(boxes(&bytes, 0, 28), [(*b"ftyp", 8, 16)]);
        assert_eq!(boxes(&bytes, 0, 32).len(), 2);
    }

    /// A box that does not add up ends the walk rather than looping on it
    /// or reading past the bytes there are: one shorter than its own
    /// header, one whose length runs past the end, and one cut off in its
    /// header.
    #[test]
    fn a_box_that_does_not_add_up_ends_the_walk() {
        let mut bytes = boxed(b"ftyp", b"crx \0\0\0\x01");
        bytes.extend(boxed(b"moov", &boxed(b"CMT1", b"abcd")));
        let mut broken = bytes.clone();
        broken[16..20].copy_from_slice(&2u32.to_be_bytes());
        assert_eq!(find(&broken, 0, broken.len(), b"CMT1"), None);
        assert_eq!(boxes(&broken, 0, broken.len()).len(), 1);

        let mut long = bytes.clone();
        long[16..20].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(boxes(&long, 0, long.len())[1], (*b"moov", 24, long.len()));

        let truncated = &bytes[..20];
        assert_eq!(boxes(truncated, 0, truncated.len()), [(*b"ftyp", 8, 16)]);
        let mut wide = 1u32.to_be_bytes().to_vec();
        wide.extend(b"mdat");
        wide.extend(u64::MAX.to_be_bytes());
        assert_eq!(boxes(&wide, 0, wide.len()), [(*b"mdat", 16, wide.len())]);
    }
}
