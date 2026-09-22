//! What every reader of a TIFF-shaped block shares: the four ways a TIFF
//! announces itself, how much of one is read for its metadata, the byte
//! order and the numbers written in it, the entries of a directory, and
//! the tags that point at another directory. The metadata reader, the XMP
//! reader, the raw containers that keep a TIFF inside them and the TIFF
//! decoder's own sniff all read the same header, and read it here.

/// How much of a TIFF is read to find its metadata.
///
/// A TIFF *is* its own metadata block: there is no chunk to seek to, and the
/// parser reads the whole of whatever it is handed. So it is handed a prefix
/// rather than the file — a scanned map or an elevation model must not be
/// pulled into memory whole for a date.
///
/// A prefix is enough because of where a directory goes. The 443 MB map this
/// was measured against keeps its first directory at byte 8 with every value
/// inside the first 10 kB, which is what any writer that means the file to be
/// read out of order does; this leaves room for that, its sub-directories, a
/// maker note and a thumbnail besides. The raw containers that keep a TIFF
/// inside them are read to the same prefix.
pub const PREFIX: u64 = 8 << 20;

/// A TIFF's byte order, and numbers written in it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Order {
    Little,
    Big,
}

/// Which of the two formats a header announces: the original, whose
/// version number is 42, or BigTIFF, whose is 43 and whose offsets are
/// eight bytes so that a file may pass four gigabytes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Classic,
    Big,
}

/// What `bytes` announce themselves as, from their first four: the byte
/// order and the format, or `None` for anything that is not a TIFF.
pub fn header(bytes: &[u8]) -> Option<(Order, Kind)> {
    Some(match bytes.get(..4)? {
        b"II\x2a\x00" => (Order::Little, Kind::Classic),
        b"MM\x00\x2a" => (Order::Big, Kind::Classic),
        b"II\x2b\x00" => (Order::Little, Kind::Big),
        b"MM\x00\x2b" => (Order::Big, Kind::Big),
        _ => return None,
    })
}

/// The four bytes a TIFF of `order` and `kind` opens with.
pub fn signature(order: Order, kind: Kind) -> [u8; 4] {
    match (order, kind) {
        (Order::Little, Kind::Classic) => *b"II\x2a\x00",
        (Order::Big, Kind::Classic) => *b"MM\x00\x2a",
        (Order::Little, Kind::Big) => *b"II\x2b\x00",
        (Order::Big, Kind::Big) => *b"MM\x00\x2b",
    }
}

impl Order {
    /// The byte order of a block in the original format, or `None` for
    /// anything else — a BigTIFF included, whose directory is not one this
    /// reads.
    pub fn of(tiff: &[u8]) -> Option<Self> {
        match header(tiff)? {
            (order, Kind::Classic) => Some(order),
            (_, Kind::Big) => None,
        }
    }

    pub fn read_u16(self, bytes: &[u8], at: usize) -> Option<u16> {
        let bytes: [u8; 2] = bytes.get(at..at + 2)?.try_into().ok()?;
        Some(match self {
            Order::Little => u16::from_le_bytes(bytes),
            Order::Big => u16::from_be_bytes(bytes),
        })
    }

    pub fn read_u32(self, bytes: &[u8], at: usize) -> Option<u32> {
        let bytes: [u8; 4] = bytes.get(at..at + 4)?.try_into().ok()?;
        Some(match self {
            Order::Little => u32::from_le_bytes(bytes),
            Order::Big => u32::from_be_bytes(bytes),
        })
    }

    pub fn u16(self, value: u16) -> [u8; 2] {
        match self {
            Order::Little => value.to_le_bytes(),
            Order::Big => value.to_be_bytes(),
        }
    }

    pub fn u32(self, value: u32) -> [u8; 4] {
        match self {
            Order::Little => value.to_le_bytes(),
            Order::Big => value.to_be_bytes(),
        }
    }
}

/// The tags that hold the offset of another directory. Which of them a
/// reader leaves out is its own business: a block written back out
/// addresses nothing in the file it came from, and a container's boxes
/// are pointed at afresh.
pub mod tag {
    /// The sub-file directories of a raster: a pyramid's levels, a mask.
    pub const SUB_IFDS: u16 = 330;
    /// The Exif directory: exposure, lens, the maker note.
    pub const EXIF_IFD: u16 = 0x8769;
    /// The GPS directory.
    pub const GPS_IFD: u16 = 0x8825;
    /// The interoperability directory, which says nothing worth the trip.
    pub const INTEROP_IFD: u16 = 0xA005;
}

/// One directory entry, with its value either in the entry or at an offset
/// in the block it came from.
pub struct Entry {
    pub tag: u16,
    pub kind: u16,
    pub count: u32,
    pub inline: Option<[u8; 4]>,
    pub offset: usize,
}

/// The entries of the first directory in `tiff`, a block in the original
/// format, with the tags in `skipping` left out. `None` where the block
/// does not announce `order` — a block in the other byte order would be
/// read as nonsense — or does not add up.
pub fn entries(tiff: &[u8], order: Order, skipping: &[u16]) -> Option<Vec<Entry>> {
    if Order::of(tiff)? != order {
        return None;
    }
    let at = order.read_u32(tiff, 4)? as usize;
    let count = order.read_u16(tiff, at)? as usize;
    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        let at = at + 2 + 12 * index;
        let tag = order.read_u16(tiff, at)?;
        let kind = order.read_u16(tiff, at + 2)?;
        let count = order.read_u32(tiff, at + 4)?;
        let value: [u8; 4] = tiff.get(at + 8..at + 12)?.try_into().ok()?;
        if skipping.contains(&tag) {
            continue;
        }
        let size = u64::from(count) * u64::from(type_size(kind)?);
        let (inline, offset) = if size <= 4 {
            (Some(value), 0)
        } else {
            (None, order.read_u32(tiff, at + 8)? as usize)
        };
        entries.push(Entry {
            tag,
            kind,
            count,
            inline,
            offset,
        });
    }
    Some(entries)
}

/// The size of one component of each TIFF type.
pub fn type_size(kind: u16) -> Option<u16> {
    Some(match kind {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 | 13 => 4,
        5 | 10 | 12 => 8,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The four signatures are told apart, and anything else is not a TIFF.
    #[test]
    fn the_four_headers_say_their_order_and_kind() {
        for order in [Order::Little, Order::Big] {
            for kind in [Kind::Classic, Kind::Big] {
                let bytes = signature(order, kind);
                assert_eq!(header(&bytes), Some((order, kind)));
                assert_eq!(Order::of(&bytes), (kind == Kind::Classic).then_some(order));
            }
        }
        assert_eq!(header(b"II\x2c\x00"), None);
        assert_eq!(header(b"\x89PNG"), None);
        assert_eq!(header(b"II"), None, "too short to say");
    }

    /// A directory's entries are read in the block's own order, an entry
    /// too long for its slot pointing into the block, and the tags asked
    /// to be skipped are.
    #[test]
    fn a_directory_is_read_in_its_own_order_and_the_pointers_left_out() {
        for order in [Order::Little, Order::Big] {
            let mut block = signature(order, Kind::Classic).to_vec();
            block.extend(order.u32(8));
            block.extend(order.u16(3));
            // 0x0100 ImageWidth, SHORT, one component, inline.
            block.extend(order.u16(0x0100));
            block.extend(order.u16(3));
            block.extend(order.u32(1));
            block.extend(order.u16(640));
            block.extend([0, 0]);
            // The Exif pointer, to be left out.
            block.extend(order.u16(tag::EXIF_IFD));
            block.extend(order.u16(4));
            block.extend(order.u32(1));
            block.extend(order.u32(999));
            // 0x010e ImageDescription, ASCII, eight bytes, at an offset.
            block.extend(order.u16(0x010e));
            block.extend(order.u16(2));
            block.extend(order.u32(8));
            block.extend(order.u32(50));
            block.extend(order.u32(0));
            let entries = entries(&block, order, &[tag::EXIF_IFD]).expect("reads");
            assert_eq!(entries.len(), 2);
            assert_eq!(entries[0].tag, 0x0100);
            assert!(entries[0].inline.is_some());
            assert_eq!(entries[1].tag, 0x010e);
            assert_eq!((entries[1].inline, entries[1].offset), (None, 50));
            let other = match order {
                Order::Little => Order::Big,
                Order::Big => Order::Little,
            };
            assert!(
                super::entries(&block, other, &[]).is_none(),
                "the other order"
            );
        }
    }
}
