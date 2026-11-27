//! Apple's maker note: the directory an iPhone or an iPad keeps inside its
//! EXIF, under the `MakerNote` tag. `Apple iOS\0`, two bytes of version, the
//! byte order, then a TIFF directory whose offsets count from the note's
//! first byte. What is read out of it here is its signed rationals by tag:
//! the two numbers a gain map's headroom is worked out from
//! ([`headroom`]), and the accelerometer's reading (see
//! [`super::attitude`]).

use super::tiff::{self, Order};

/// The note's signed rationals under `wanted`, as many as the entry holds.
/// `None` where `note` is not Apple's, the tag is not in it, or it is not
/// signed rationals.
pub fn rationals(note: &[u8], wanted: u16) -> Option<Vec<f64>> {
    if !note.starts_with(b"Apple iOS\0") {
        return None;
    }
    let order = match note.get(12..14)? {
        b"MM" => Order::Big,
        b"II" => Order::Little,
        _ => return None,
    };
    const DIRECTORY: usize = 14;
    const SRATIONAL: u16 = 10;
    let entry = tiff::entries_at(note, order, DIRECTORY, &[])?
        .into_iter()
        .find(|entry| entry.tag == wanted)?;
    if entry.kind != SRATIONAL || entry.count == 0 {
        return None;
    }
    // Eight bytes each, so never inline, and never more than the note holds:
    // the count is checked against the note before anything is allocated.
    let count = entry.count as usize;
    note.get(entry.offset..entry.offset.checked_add(count.checked_mul(8)?)?)?;
    (0..count)
        .map(|index| {
            let at = entry.offset + 8 * index;
            let numerator = order.read_u32(note, at)? as i32;
            let denominator = order.read_u32(note, at + 4)? as i32;
            (denominator != 0).then(|| f64::from(numerator) / f64::from(denominator))
        })
        .collect()
}

/// The headroom a gain map beside the picture may lift it by — the ratio of
/// its brightest white to SDR white — from tags 33 and 48 of the note, by
/// the formula Apple gives in "Applying Apple HDR effect to your photos".
/// `None` where `note` is not Apple's, or the two are not in it. An iPhone
/// writes the same note into a HEIC and a JPEG, so both read it here.
pub fn headroom(note: &[u8]) -> Option<f32> {
    let (maker33, maker48) = (tag(note, 33)?, tag(note, 48)?);
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

/// One of the note's signed rationals, by tag, where the entry holds one.
fn tag(note: &[u8], wanted: u16) -> Option<f32> {
    match rationals(note, wanted)?.as_slice() {
        [value] => Some(*value as f32),
        _ => None,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// One entry of a note written by hand: its tag, its type, and the
    /// rationals under it, each a numerator and a denominator.
    pub(crate) type Written<'a> = (u16, u16, &'a [(i32, i32)]);

    /// A note written by hand in Apple's layout, big-endian, holding
    /// `entries`, with their values after the directory.
    pub(crate) fn note(entries: &[Written]) -> Vec<u8> {
        let mut note = b"Apple iOS\0\0\x01MM".to_vec();
        note.extend_from_slice(&(entries.len() as u16).to_be_bytes());
        let mut values = Vec::new();
        let start = 16 + 12 * entries.len();
        for (tag, kind, numbers) in entries {
            note.extend_from_slice(&tag.to_be_bytes());
            note.extend_from_slice(&kind.to_be_bytes());
            note.extend_from_slice(&(numbers.len() as u32).to_be_bytes());
            note.extend_from_slice(&((start + values.len()) as u32).to_be_bytes());
            for (numerator, denominator) in *numbers {
                values.extend_from_slice(&numerator.to_be_bytes());
                values.extend_from_slice(&denominator.to_be_bytes());
            }
        }
        note.extend_from_slice(&values);
        note
    }

    #[test]
    fn every_rational_of_an_entry_is_read() {
        let note = note(&[
            (8, 10, &[(-1, 2), (3, 4), (5, -8)]),
            (33, 10, &[(101, 100)]),
        ]);
        assert_eq!(rationals(&note, 8), Some(vec![-0.5, 0.75, -0.625]));
        assert_eq!(rationals(&note, 33), Some(vec![1.01]));
        assert_eq!(rationals(&note, 9), None);
        assert_eq!(rationals(b"Nikon\0", 8), None);
    }

    /// A count running past the end of the note gives nothing rather than
    /// asking for room for it.
    #[test]
    fn a_count_past_the_end_is_refused() {
        let mut note = note(&[(8, 10, &[(1, 1)])]);
        note[20..24].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(rationals(&note, 8), None);
    }

    #[test]
    fn another_type_or_a_zero_denominator_is_refused() {
        assert_eq!(rationals(&note(&[(8, 5, &[(1, 2)])]), 8), None);
        assert_eq!(rationals(&note(&[(8, 10, &[(1, 0)])]), 8), None);
    }

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

    /// An iPhone 17's note carries LONG8 entries (type 16) beside the two,
    /// and those must not cost the directory.
    #[test]
    fn a_long8_entry_does_not_hide_the_two() {
        let mut note = b"Apple iOS\0\0\x01MM".to_vec();
        note.extend_from_slice(&3u16.to_be_bytes());
        let entry = |tag: u16, kind: u16, offset: u32| {
            [
                &tag.to_be_bytes()[..],
                &kind.to_be_bytes(),
                &1u32.to_be_bytes(),
                &offset.to_be_bytes(),
            ]
            .concat()
        };
        let values = 16 + 3 * 12;
        note.extend_from_slice(&entry(23, 16, values as u32));
        note.extend_from_slice(&entry(33, 10, values as u32 + 8));
        note.extend_from_slice(&entry(48, 10, values as u32 + 16));
        note.extend_from_slice(&7u64.to_be_bytes());
        note.extend_from_slice(&101i32.to_be_bytes());
        note.extend_from_slice(&100i32.to_be_bytes());
        note.extend_from_slice(&(-3i32).to_be_bytes());
        note.extend_from_slice(&2i32.to_be_bytes());
        assert!((tag(&note, 33).unwrap() - 1.01).abs() < 1e-6);
        assert_eq!(tag(&note, 48), Some(-1.5));
    }
}
