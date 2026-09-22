//! Apple's own description of the gain map in an iPhone's HEIC, for the
//! files from before iOS 18 that carry nothing else.
//!
//! The map is an auxiliary image of type
//! `urn:com:apple:photo:2020:aux:hdrgainmap`, and how far it may lift the
//! picture — the headroom, the ratio of the brightest white to SDR white —
//! is worked out from two numbers in the maker note, by the piecewise
//! formula Apple gives in "Applying Apple HDR effect to your photos". The
//! maker note is Apple's own directory inside the EXIF: `Apple iOS\0`,
//! two bytes of version, the byte order, then a TIFF directory whose
//! offsets count from the note's first byte. Tags 33 and 48 are the two,
//! each a signed rational.

/// The auxiliary type of the map.
pub(super) const GAIN_MAP: &str = "urn:com:apple:photo:2020:aux:hdrgainmap";

/// The headroom the maker note describes, from the EXIF block a HEIF
/// carries. `None` where there is no maker note, or Apple's two numbers
/// are not in it.
pub(super) fn headroom(exif_item: &[u8]) -> Option<f32> {
    let note = maker_note(exif_item)?;
    let (maker33, maker48) = (tag(&note, 33)?, tag(&note, 48)?);
    Some(headroom_of(maker33, maker48))
}

/// Apple's formula, verbatim: the two numbers to a count of stops, and the
/// stops to a ratio, never below one.
fn headroom_of(maker33: f32, maker48: f32) -> f32 {
    let stops = if maker33 < 1.0 {
        if maker48 <= 0.01 {
            -20.0 * maker48 + 1.8
        } else {
            -0.101 * maker48 + 1.601
        }
    } else if maker48 <= 0.01 {
        -70.0 * maker48 + 3.0
    } else {
        -0.303 * maker48 + 2.303
    };
    stops.max(0.0).exp2()
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

/// One of the note's signed rationals, by tag.
fn tag(note: &[u8], wanted: u16) -> Option<f32> {
    if !note.starts_with(b"Apple iOS\0") {
        return None;
    }
    let big_endian = match note.get(12..14)? {
        b"MM" => true,
        b"II" => false,
        _ => return None,
    };
    let u16_at = |at: usize| -> Option<u16> {
        let bytes: [u8; 2] = note.get(at..at + 2)?.try_into().ok()?;
        Some(if big_endian {
            u16::from_be_bytes(bytes)
        } else {
            u16::from_le_bytes(bytes)
        })
    };
    let u32_at = |at: usize| -> Option<u32> {
        let bytes: [u8; 4] = note.get(at..at + 4)?.try_into().ok()?;
        Some(if big_endian {
            u32::from_be_bytes(bytes)
        } else {
            u32::from_le_bytes(bytes)
        })
    };
    const DIRECTORY: usize = 14;
    const SRATIONAL: u16 = 10;
    let count = u16_at(DIRECTORY)?;
    for entry in 0..usize::from(count) {
        let at = DIRECTORY + 2 + entry * 12;
        if u16_at(at)? != wanted {
            continue;
        }
        if u16_at(at + 2)? != SRATIONAL || u32_at(at + 4)? != 1 {
            return None;
        }
        let value = u32_at(at + 8)? as usize;
        let numerator = u32_at(value)? as i32;
        let denominator = u32_at(value + 4)? as i32;
        if denominator == 0 {
            return None;
        }
        return Some(numerator as f32 / denominator as f32);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The numbers the iPhone 16 Pro photograph in the tree's test carries:
    /// they have to come to the 3.30 its ISO metadata states outright, or
    /// the two readings of the same map would disagree.
    #[test]
    fn apples_formula_agrees_with_the_iso_metadata_beside_it() {
        let headroom = headroom_of(1058986.0 / 1048501.0, 70379.0 / 36854.0);
        assert!((headroom - 1.724342f32.exp2()).abs() < 1e-3, "{headroom}");
    }

    /// Each of the four branches, and the floor at no headroom.
    #[test]
    fn every_branch_of_the_formula() {
        assert!((headroom_of(0.5, 0.0) - 1.8f32.exp2()).abs() < 1e-5);
        assert!((headroom_of(0.5, 1.0) - 1.5f32.exp2()).abs() < 1e-5);
        assert!((headroom_of(1.0, 0.0) - 3.0f32.exp2()).abs() < 1e-5);
        assert!((headroom_of(2.0, 1.0) - 2.0f32.exp2()).abs() < 1e-5);
        assert_eq!(headroom_of(2.0, 100.0), 1.0);
    }

    /// A note written by hand in Apple's layout, big-endian, with the two
    /// tags and one the reader does not want.
    #[test]
    fn the_two_tags_are_read_out_of_the_note() {
        let mut note = b"Apple iOS\0\0\x01MM".to_vec();
        note.extend_from_slice(&3u16.to_be_bytes());
        let entry = |tag: u16, offset: u32| {
            [
                &tag.to_be_bytes()[..],
                &10u16.to_be_bytes(),
                &1u32.to_be_bytes(),
                &offset.to_be_bytes(),
            ]
            .concat()
        };
        let values = 16 + 3 * 12;
        note.extend_from_slice(&entry(10, 0));
        note.extend_from_slice(&entry(33, values as u32));
        note.extend_from_slice(&entry(48, values as u32 + 8));
        note.extend_from_slice(&101i32.to_be_bytes());
        note.extend_from_slice(&100i32.to_be_bytes());
        note.extend_from_slice(&(-3i32).to_be_bytes());
        note.extend_from_slice(&2i32.to_be_bytes());
        assert!((tag(&note, 33).unwrap() - 1.01).abs() < 1e-6);
        assert_eq!(tag(&note, 48), Some(-1.5));
        assert_eq!(tag(&note, 99), None);
        assert_eq!(tag(b"Nikon\0", 33), None);
    }
}
