//! Apple's own description of the gain map in an iPhone's HEIC, for the
//! files from before iOS 18 that carry nothing else.
//!
//! The map is an auxiliary image of type
//! `urn:com:apple:photo:2020:aux:hdrgainmap`, and how far it may lift the
//! picture — the headroom, the ratio of the brightest white to SDR white —
//! is worked out from two numbers in the maker note, by the piecewise
//! formula Apple gives in "Applying Apple HDR effect to your photos", which
//! [`apple_note::headroom`] works out. What is left here is finding the
//! maker note in a HEIF's Exif item.

use crate::image::apple_note;

/// The headroom the maker note describes, from the EXIF block a HEIF
/// carries. `None` where there is no maker note, or Apple's two numbers
/// are not in it.
pub(super) fn headroom(exif_item: &[u8]) -> Option<f32> {
    apple_note::headroom(&maker_note(exif_item)?)
}

/// The maker note out of a HEIF's Exif item: the item's four-byte offset to
/// the TIFF header, then the block the EXIF reader reads.
fn maker_note(exif_item: &[u8]) -> Option<Vec<u8>> {
    let offset = u32::from_be_bytes(exif_item.get(..4)?.try_into().ok()?) as usize;
    let tiff = exif_item.get(4 + offset..)?;
    let exif = exif::Reader::new().read_raw(tiff.to_vec()).ok()?;
    let field = exif.get_field(exif::Tag::MakerNote, exif::In::PRIMARY)?;
    match &field.value {
        exif::Value::Undefined(bytes, _) => Some(bytes.clone()),
        _ => None,
    }
}
