//! The ISO base media file format's boxes, walked in memory: a big-endian
//! length counting the header, then a four-byte type, with a length of one
//! meaning an eight-byte length follows and a length of zero meaning the
//! box runs to the end. What a HEIF's `meta` box, a Canon CR3's `moov` and
//! an ISO 21496-1 gain-map item are all read through. The one walker that
//! is not here is the XMP reader's over a JPEG XL container, which walks
//! the file as a stream so as not to read it into memory for a packet.

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
        if size < header {
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
