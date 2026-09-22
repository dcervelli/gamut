//! ISO 21496-1 in a HEIF: the `tmap` item that says the file is two
//! pictures and a gain map between them.
//!
//! A tone-mapped image item, of type `tmap`, refers by `dimg` to the base
//! picture and the gain map — in that order — and its own payload is the
//! gain map's metadata: the headroom of each rendition, and per channel the
//! range of the map's log2 gains, its gamma and the two offsets. That
//! is the same set of numbers Ultra HDR's XMP carries, so it lands in the
//! same `GainMapMetadata`. An iPhone running iOS 18 or later writes one
//! beside Apple's own auxiliary image, and so does anything else that writes
//! a standard gain map into a HEIC or an AVIF.
//!
//! `libheif` 1.23 knows nothing of the item, so it is found by walking the
//! boxes: `meta`, and in it `iinf` for what the items are, `iref` for what
//! the `tmap` refers to, and `iloc` for where its payload is, which for an
//! item this small is in `meta`'s own `idat` box. The walk reads the file's
//! structure only, as `libheif` does, and stops at the first `mdat`.

use std::collections::HashMap;
use std::io::SeekFrom;

use anyhow::{Context, Result, anyhow, bail};
use ultrahdr_rs::GainMapMetadata;

use crate::image::decode::ReadSeek;
use crate::image::isobmff;

/// What a `tmap` item says.
#[derive(Debug, Clone)]
pub(super) struct ToneMap {
    /// The item id of the base picture.
    pub base: u32,
    /// The item id of the gain map.
    pub gain_map: u32,
    pub metadata: GainMapMetadata,
}

/// The largest `meta` box that will be read: the item tables of a phone's
/// photograph run to a few tens of kilobytes, and one claiming more than
/// this is not carrying a gain map worth finding.
const MAX_META: u64 = 16 << 20;

/// The `tmap` item of the file, if it has one this reader can make sense
/// of. `None` for a file with none, and for one whose item tables say
/// something this reader does not follow — a payload kept somewhere it
/// cannot reach, or a reference the file left out — since the picture
/// without its gain map is still the picture.
pub(super) fn find(source: &mut dyn ReadSeek) -> Result<Option<ToneMap>> {
    source.seek(SeekFrom::Start(0))?;
    let Some(meta) = meta_box(source)? else {
        return Ok(None);
    };
    let Some(found) = Meta::parse(&meta) else {
        return Ok(None);
    };
    let Some(&(item, _)) = found.items.iter().find(|(_, kind)| kind == b"tmap") else {
        return Ok(None);
    };
    let Some(refs) = found.references.get(&(item, *b"dimg")) else {
        return Ok(None);
    };
    let [base, gain_map] = refs[..] else {
        return Ok(None);
    };
    let Some(location) = found.locations.get(&item) else {
        return Ok(None);
    };

    let mut payload = Vec::new();
    for &(offset, length) in &location.extents {
        let length = usize::try_from(length).map_err(|_| anyhow!("tmap extent too long"))?;
        if length > MAX_META as usize {
            bail!("tmap item claims {length} bytes");
        }
        match location.construction {
            // In the file itself.
            0 => {
                source.seek(SeekFrom::Start(offset))?;
                let mut extent = vec![0; length];
                source
                    .read_exact(&mut extent)
                    .context("reading the tmap item")?;
                payload.extend_from_slice(&extent);
            }
            // In `meta`'s own `idat`, where the offsets are from its start.
            1 => {
                let Some(idat) = found.idat else {
                    return Ok(None);
                };
                let from = usize::try_from(offset)
                    .ok()
                    .and_then(|o| o.checked_add(idat.0));
                let Some(extent) = from.and_then(|from| {
                    let to = from.checked_add(length)?;
                    (to <= idat.1).then(|| meta.get(from..to)).flatten()
                }) else {
                    return Ok(None);
                };
                payload.extend_from_slice(extent);
            }
            _ => return Ok(None),
        }
    }
    Ok(metadata(&payload).map(|metadata| ToneMap {
        base,
        gain_map,
        metadata,
    }))
}

/// The top-level `meta` box's body. The boxes before it are skipped by
/// their lengths; the search stops at `mdat`, past which nothing but pixels
/// lies.
fn meta_box(source: &mut dyn ReadSeek) -> Result<Option<Vec<u8>>> {
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
        if kind == b"meta" {
            let length = size - header_len;
            if length > MAX_META {
                return Ok(None);
            }
            let mut body = vec![0; length as usize];
            source.seek(SeekFrom::Start(at + header_len))?;
            source
                .read_exact(&mut body)
                .context("reading the meta box")?;
            return Ok(Some(body));
        }
        if kind == b"mdat" {
            return Ok(None);
        }
        at = at
            .checked_add(size)
            .ok_or_else(|| anyhow!("box runs off the file"))?;
        source.seek(SeekFrom::Start(at))?;
    }
}

/// Where an item's payload is: which of the three ways the offsets are
/// meant, and the extents.
#[derive(Debug, Default)]
struct Location {
    /// 0 for file offsets, 1 for offsets into `idat`, 2 for offsets into
    /// another item.
    construction: u8,
    extents: Vec<(u64, u64)>,
}

/// The items an item refers to, by the reference's type.
type References = HashMap<(u32, [u8; 4]), Vec<u32>>;

/// What the item tables say, as far as a `tmap` needs.
#[derive(Debug, Default)]
struct Meta {
    /// Each item's id and type.
    items: Vec<(u32, [u8; 4])>,
    references: References,
    locations: HashMap<u32, Location>,
    /// The `idat` box's body, as a range of `meta`'s body.
    idat: Option<(usize, usize)>,
}

impl Meta {
    fn parse(body: &[u8]) -> Option<Self> {
        let mut meta = Self::default();
        // `meta` is a full box: version and flags before the children.
        for (kind, start, end) in isobmff::boxes(body, 4, body.len()) {
            let child = &body[start..end];
            match &kind {
                b"iinf" => meta.items = iinf(child)?,
                b"iref" => meta.references = iref(child)?,
                b"iloc" => meta.locations = iloc(child)?,
                b"idat" => meta.idat = Some((start, end)),
                _ => {}
            }
        }
        Some(meta)
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn u8(&mut self) -> Option<u8> {
        let value = *self.bytes.get(self.at)?;
        self.at += 1;
        Some(value)
    }
    fn u16(&mut self) -> Option<u16> {
        let value = self.bytes.get(self.at..self.at + 2)?.first_chunk::<2>()?;
        self.at += 2;
        Some(u16::from_be_bytes(*value))
    }
    fn u32(&mut self) -> Option<u32> {
        let value = self.bytes.get(self.at..self.at + 4)?.first_chunk::<4>()?;
        self.at += 4;
        Some(u32::from_be_bytes(*value))
    }
    fn i32(&mut self) -> Option<i32> {
        self.u32().map(|value| value as i32)
    }
    fn u64(&mut self) -> Option<u64> {
        let value = self.bytes.get(self.at..self.at + 8)?.first_chunk::<8>()?;
        self.at += 8;
        Some(u64::from_be_bytes(*value))
    }
    /// A field whose width the table's header chose: 0, 4 or 8 bytes.
    fn sized(&mut self, size: u8) -> Option<u64> {
        match size {
            0 => Some(0),
            4 => self.u32().map(u64::from),
            8 => self.u64(),
            _ => None,
        }
    }
    /// An item id, 16 bits before version 2 of a table and 32 from it.
    fn id(&mut self, version: u8, wide_from: u8) -> Option<u32> {
        if version >= wide_from {
            self.u32()
        } else {
            self.u16().map(u32::from)
        }
    }
}

/// `iinf`: the item ids and their types, from the `infe` boxes inside.
fn iinf(body: &[u8]) -> Option<Vec<(u32, [u8; 4])>> {
    let mut cursor = Cursor { bytes: body, at: 0 };
    let version = cursor.u8()?;
    cursor.at += 3;
    let count = if version == 0 {
        cursor.u16()? as usize
    } else {
        cursor.u32()? as usize
    };
    let mut items = Vec::with_capacity(count.min(1024));
    for (kind, start, end) in isobmff::boxes(body, cursor.at, body.len()) {
        if &kind != b"infe" {
            continue;
        }
        let entry = &body[start..end];
        let mut cursor = Cursor {
            bytes: entry,
            at: 0,
        };
        let version = cursor.u8()?;
        cursor.at += 3;
        // Versions 0 and 1 name no type, and no item of theirs is a `tmap`.
        if version < 2 {
            continue;
        }
        let id = cursor.id(version, 3)?;
        cursor.at += 2; // protection index
        let kind: [u8; 4] = entry.get(cursor.at..cursor.at + 4)?.try_into().ok()?;
        items.push((id, kind));
    }
    Some(items)
}

/// `iref`: for each item, the items it refers to, by the reference's type.
fn iref(body: &[u8]) -> Option<References> {
    let mut references = HashMap::new();
    let version = *body.first()?;
    for (kind, start, end) in isobmff::boxes(body, 4, body.len()) {
        let mut cursor = Cursor {
            bytes: &body[start..end],
            at: 0,
        };
        let from = cursor.id(version, 1)?;
        let count = cursor.u16()?;
        let mut to = Vec::with_capacity(usize::from(count).min(1024));
        for _ in 0..count {
            to.push(cursor.id(version, 1)?);
        }
        references.insert((from, kind), to);
    }
    Some(references)
}

/// `iloc`: where each item's payload is.
fn iloc(body: &[u8]) -> Option<HashMap<u32, Location>> {
    let mut cursor = Cursor { bytes: body, at: 0 };
    let version = cursor.u8()?;
    cursor.at += 3;
    let sizes = cursor.u16()?;
    let offset_size = (sizes >> 12) as u8;
    let length_size = ((sizes >> 8) & 15) as u8;
    let base_offset_size = ((sizes >> 4) & 15) as u8;
    let index_size = if version >= 1 { (sizes & 15) as u8 } else { 0 };
    let count = if version < 2 {
        cursor.u16()? as usize
    } else {
        cursor.u32()? as usize
    };
    let mut locations = HashMap::new();
    for _ in 0..count {
        let id = cursor.id(version, 2)?;
        let construction = if version >= 1 {
            (cursor.u16()? & 15) as u8
        } else {
            0
        };
        cursor.at += 2; // data reference index
        let base = cursor.sized(base_offset_size)?;
        let extents = cursor.u16()?;
        let mut location = Location {
            construction,
            extents: Vec::with_capacity(usize::from(extents).min(1024)),
        };
        for _ in 0..extents {
            let _index = cursor.sized(index_size)?;
            let offset = cursor.sized(offset_size)?;
            let length = cursor.sized(length_size)?;
            location.extents.push((base.checked_add(offset)?, length));
        }
        locations.insert(id, location);
    }
    Some(locations)
}

/// The `tmap` item's payload, read as ISO 21496-1's gain map metadata.
/// `None` for a version this reader does not know, or a payload cut short.
pub(super) fn metadata(payload: &[u8]) -> Option<GainMapMetadata> {
    let mut cursor = Cursor {
        bytes: payload,
        at: 0,
    };
    if cursor.u8()? != 0 {
        return None;
    }
    let _minimum_version = cursor.u16()?;
    let _writer_version = cursor.u16()?;
    let flags = cursor.u8()?;
    let multichannel = flags & 0x80 != 0;
    let use_base_color_space = flags & 0x40 != 0;
    let mut ratio = |signed: bool| -> Option<f64> {
        let numerator = if signed {
            f64::from(cursor.i32()?)
        } else {
            f64::from(cursor.u32()?)
        };
        let denominator = f64::from(cursor.u32()?);
        (denominator != 0.0).then(|| numerator / denominator)
    };
    let base_hdr_headroom = ratio(false)?;
    let alternate_hdr_headroom = ratio(false)?;
    let channels = if multichannel { 3 } else { 1 };
    let mut gain_map_min = [0.0; 3];
    let mut gain_map_max = [0.0; 3];
    let mut gamma = [1.0; 3];
    let mut base_offset = [0.0; 3];
    let mut alternate_offset = [0.0; 3];
    for channel in 0..channels {
        gain_map_min[channel] = ratio(true)?;
        gain_map_max[channel] = ratio(true)?;
        gamma[channel] = ratio(false)?;
        base_offset[channel] = ratio(true)?;
        alternate_offset[channel] = ratio(true)?;
    }
    if !multichannel {
        for channel in 1..3 {
            gain_map_min[channel] = gain_map_min[0];
            gain_map_max[channel] = gain_map_max[0];
            gamma[channel] = gamma[0];
            base_offset[channel] = base_offset[0];
            alternate_offset[channel] = alternate_offset[0];
        }
    }
    // `GainMapMetadata` is non-exhaustive, so it is built by amending the
    // defaults rather than by naming every field.
    let mut metadata = GainMapMetadata::default();
    metadata.gain_map_min = gain_map_min;
    metadata.gain_map_max = gain_map_max;
    metadata.gamma = gamma;
    metadata.base_offset = base_offset;
    metadata.alternate_offset = alternate_offset;
    metadata.base_hdr_headroom = base_hdr_headroom;
    metadata.alternate_hdr_headroom = alternate_hdr_headroom;
    metadata.use_base_color_space = use_base_color_space;
    // The standard has no flag for the direction: the base is the SDR
    // rendition when its headroom is the smaller.
    metadata.backward_direction = base_hdr_headroom > alternate_hdr_headroom;
    Some(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 62 bytes an iPhone 16 Pro on iOS 26 wrote: one channel, the
    /// gain map in the base's color space, the base at no headroom and the
    /// alternate at 1.724 stops — 3.3 times SDR white, which is also what
    /// the file's Apple maker note works out to.
    const IPHONE: [u8; 62] = [
        0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0f, 0x42, 0x40, 0x00,
        0x1a, 0x4f, 0xb6, 0x00, 0x0f, 0x42, 0x40, 0xff, 0xff, 0xfd, 0xb6, 0x00, 0x0f, 0x42, 0x40,
        0x00, 0x1a, 0x4f, 0xb6, 0x00, 0x0f, 0x42, 0x40, 0x00, 0x0a, 0xb2, 0xf4, 0x00, 0x0f, 0x42,
        0x40, 0x00, 0x00, 0x00, 0x0a, 0x00, 0x0f, 0x42, 0x40, 0x00, 0x00, 0x00, 0x0a, 0x00, 0x0f,
        0x42, 0x40,
    ];

    #[test]
    fn an_iphones_metadata_reads_as_the_crates_struct() {
        let metadata = metadata(&IPHONE).expect("the payload parses");
        assert!((metadata.alternate_hdr_headroom - 1.724342).abs() < 1e-6);
        assert_eq!(metadata.base_hdr_headroom, 0.0);
        assert!((metadata.gain_map_max[0] - 1.724342).abs() < 1e-6);
        assert!((metadata.gain_map_min[2] + 0.000586).abs() < 1e-6);
        assert!((metadata.gamma[1] - 0.701172).abs() < 1e-6);
        assert!((metadata.base_offset[0] - 1e-5).abs() < 1e-9);
        assert!((metadata.alternate_offset[1] - 1e-5).abs() < 1e-9);
        assert!(metadata.use_base_color_space);
        assert!(!metadata.backward_direction);
        // Cut short, it says nothing rather than something made up.
        assert!(super::metadata(&IPHONE[..40]).is_none());
        // An unknown version likewise.
        let mut other = IPHONE;
        other[0] = 1;
        assert!(super::metadata(&other).is_none());
    }

    fn full_box(kind: &[u8; 4], version: u8, body: &[u8]) -> Vec<u8> {
        let mut out = plain_box(kind, &[&[version, 0, 0, 0][..], body].concat());
        out.truncate(out.len());
        out
    }

    fn plain_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out
    }

    /// A file laid out the way an iPhone lays its own out: `ftyp`, then
    /// `meta` with the tables and the payload in `idat`, then `mdat`.
    fn file(payload: &[u8], with_reference: bool) -> Vec<u8> {
        // Three items: the picture, the map, the tone map.
        let infe = |id: u16, kind: &[u8; 4]| {
            full_box(
                b"infe",
                2,
                &[&id.to_be_bytes()[..], &[0, 0], kind, b"\0"].concat(),
            )
        };
        let iinf = full_box(
            b"iinf",
            0,
            &[
                &3u16.to_be_bytes()[..],
                &infe(1, b"hvc1"),
                &infe(2, b"hvc1"),
                &infe(3, b"tmap"),
            ]
            .concat(),
        );
        let iref = full_box(
            b"iref",
            0,
            &plain_box(
                b"dimg",
                &[
                    &3u16.to_be_bytes()[..],
                    &2u16.to_be_bytes(),
                    &1u16.to_be_bytes(),
                    &2u16.to_be_bytes(),
                ]
                .concat(),
            ),
        );
        // Version 1, 4-byte offsets and lengths, no base offset or index;
        // the tone map at construction method 1 (in `idat`), 6 bytes in.
        let iloc = full_box(
            b"iloc",
            1,
            &[
                &0x4400u16.to_be_bytes()[..],
                &1u16.to_be_bytes(),
                &3u16.to_be_bytes(),
                &1u16.to_be_bytes(),
                &0u16.to_be_bytes(),
                &1u16.to_be_bytes(),
                &6u32.to_be_bytes(),
                &(payload.len() as u32).to_be_bytes(),
            ]
            .concat(),
        );
        let idat = plain_box(b"idat", &[&[0u8; 6][..], payload].concat());
        let meta = full_box(
            b"meta",
            0,
            &[
                &iinf[..],
                if with_reference { &iref[..] } else { &[][..] },
                &iloc[..],
                &idat[..],
            ]
            .concat(),
        );
        [
            &plain_box(b"ftyp", b"heic\0\0\0\0mif1heic")[..],
            &meta,
            &plain_box(b"mdat", &[0u8; 16]),
        ]
        .concat()
    }

    #[test]
    fn a_tone_map_item_is_found_through_the_item_tables() {
        let bytes = file(&IPHONE, true);
        let found = find(&mut std::io::Cursor::new(bytes))
            .expect("the walk succeeds")
            .expect("the tmap is found");
        assert_eq!((found.base, found.gain_map), (1, 2));
        let expected = metadata(&IPHONE).unwrap();
        assert_eq!(found.metadata.gain_map_max, expected.gain_map_max);
        assert_eq!(found.metadata.gamma, expected.gamma);
        assert_eq!(
            found.metadata.alternate_hdr_headroom,
            expected.alternate_hdr_headroom
        );
    }

    #[test]
    fn a_tone_map_without_its_references_is_left_alone() {
        let bytes = file(&IPHONE, false);
        assert!(find(&mut std::io::Cursor::new(bytes)).unwrap().is_none());
        // And a file with no `meta` at all, or none before `mdat`.
        let plain = plain_box(b"mdat", &[0u8; 4]);
        assert!(find(&mut std::io::Cursor::new(plain)).unwrap().is_none());
        assert!(
            find(&mut std::io::Cursor::new(Vec::new()))
                .unwrap()
                .is_none()
        );
    }
}
