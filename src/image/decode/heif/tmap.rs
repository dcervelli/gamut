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
//! `libheif` 1.23 knows nothing of the item, nor ImageIO of its metadata, so
//! it is found through [`super::items`]: `iinf` for what the items are,
//! `iref` for what the `tmap` refers to, and `iloc` for where its payload
//! is, which for an item this small is in `meta`'s own `idat` box.

use anyhow::Result;
use ultrahdr_rs::GainMapMetadata;

use super::items::Meta;
use crate::image::decode::ReadSeek;
use crate::image::isobmff::Fields;

/// What a `tmap` item says.
#[derive(Debug, Clone)]
pub(super) struct ToneMap {
    /// The item id of the base picture.
    pub base: u32,
    /// The item id of the gain map, which `libheif` decodes by; ImageIO
    /// finds the map itself.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub gain_map: u32,
    pub metadata: GainMapMetadata,
}

/// The most a `tmap` item's payload may run to, over all its extents: the
/// metadata is a few dozen bytes for one channel and under two hundred for
/// three, so an item claiming kilobytes — in one extent or in thousands —
/// is not one, and is refused before it is gathered.
const MAX_TMAP: usize = 4096;

/// The `tmap` item of the file, if it has one this reader can make sense
/// of. `None` for a file with none, and for one whose item tables say
/// something this reader does not follow — a payload kept somewhere it
/// cannot reach, or a reference the file left out — since the picture
/// without its gain map is still the picture.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub(super) fn find(source: &mut dyn ReadSeek) -> Result<Option<ToneMap>> {
    let Some(meta) = Meta::read(source)? else {
        return Ok(None);
    };
    in_tables(&meta, source)
}

/// [`find`] in tables already read.
pub(super) fn in_tables(meta: &Meta, source: &mut dyn ReadSeek) -> Result<Option<ToneMap>> {
    let Some(item) = meta.items.iter().find(|item| &item.kind == b"tmap") else {
        return Ok(None);
    };
    let [base, gain_map] = meta.references(item.id, b"dimg")[..] else {
        return Ok(None);
    };
    let Some(payload) = meta.payload(source, item.id, MAX_TMAP)? else {
        return Ok(None);
    };
    Ok(metadata(&payload).map(|metadata| ToneMap {
        base,
        gain_map,
        metadata,
    }))
}

/// The `tmap` item's payload, read as ISO 21496-1's gain map metadata.
/// `None` for a version this reader does not know, or a payload cut short.
pub(super) fn metadata(payload: &[u8]) -> Option<GainMapMetadata> {
    let mut fields = Fields::new(payload);
    if fields.u8()? != 0 {
        return None;
    }
    let _minimum_version = fields.u16()?;
    let _writer_version = fields.u16()?;
    let flags = fields.u8()?;
    let multichannel = flags & 0x80 != 0;
    let use_base_color_space = flags & 0x40 != 0;
    let mut ratio = |signed: bool| -> Option<f64> {
        let numerator = if signed {
            f64::from(fields.i32()?)
        } else {
            f64::from(fields.u32()?)
        };
        let denominator = f64::from(fields.u32()?);
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
        plain_box(kind, &[&[version, 0, 0, 0][..], body].concat())
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

    /// An item claiming more than any tone map's metadata comes to is
    /// refused before its extents are gathered, rather than read in.
    #[test]
    fn a_tone_map_claiming_kilobytes_is_refused() {
        let bytes = file(&vec![0u8; MAX_TMAP + 1], true);
        let error = find(&mut std::io::Cursor::new(bytes)).expect_err("refused");
        assert!(format!("{error:#}").contains("claims"), "{error:#}");
        // And one at the ceiling is read, and found not to parse.
        let bytes = file(&vec![0u8; MAX_TMAP], true);
        assert!(find(&mut std::io::Cursor::new(bytes)).unwrap().is_none());
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
