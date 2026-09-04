//! Coding-independent code points: the small integers a file uses to state
//! its color space outright instead of leaving it to convention.
//!
//! The same numbering serves HEIF's `nclx` box and PNG's `cICP` chunk — both
//! defer to ITU-T H.273 — so one translation covers both, and adding a third
//! format that carries them is a matter of reading two bytes.
//!
//! Translation, not interpretation: what is lost here is only the distinctions
//! this program's color model does not draw. Everything it can name comes
//! through exactly.

use super::{ColorSpace, Primaries, Transfer};

/// H.273 reserves 2 for "unspecified" in both tables, which is what a file
/// written without a care says, and what we substitute when a decoder hands
/// back a code it did not recognise.
pub const UNSPECIFIED: u8 = 2;

pub fn color_space(primaries_code: u8, transfer_code: u8) -> ColorSpace {
    ColorSpace {
        transfer: transfer(transfer_code),
        primaries: primaries(primaries_code),
    }
}

pub fn transfer(code: u8) -> Transfer {
    match code {
        // The one exact match: sRGB's own curve.
        13 => Transfer::Srgb,
        // The BT.709 camera OETF and its BT.601 and BT.2020 relatives. They
        // are not literally the sRGB curve, but content tagged with them is
        // graded on, and meant for, an sRGB-like display; treating them as
        // sRGB is what every viewer does and what the grader saw.
        1 | 6 | 14 | 15 => Transfer::Srgb,
        // SMPTE ST 2084.
        16 => Transfer::Pq,
        // ARIB STD-B67.
        18 => Transfer::Hlg,
        8 => Transfer::Linear,
        // BT.470-6 System M and System B/G, which state plain power laws.
        4 => Transfer::Gamma(2.2),
        5 => Transfer::Gamma(2.8),
        // Unspecified is the common case for a file written without a care;
        // sRGB is the convention for a still image, and `--transfer` is there
        // for when the convention is wrong.
        _ => Transfer::Srgb,
    }
}

pub fn primaries(code: u8) -> Primaries {
    match code {
        // BT.2020, which BT.2100 shares.
        9 => Primaries::Bt2020,
        // EG 432-1 is Display P3. RP 431-2 is the same three primaries with
        // the DCI projector white point rather than D65; nothing here models
        // that white point, and P3 is far closer than BT.709 would be.
        11 | 12 => Primaries::DisplayP3,
        // BT.601 and the rest differ from BT.709 by less than the primaries
        // this program can name, so they round to it.
        _ => Primaries::Bt709,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hdr_curves_are_the_ones_that_must_not_be_rounded() {
        assert_eq!(transfer(16), Transfer::Pq);
        assert_eq!(transfer(18), Transfer::Hlg);
        assert_eq!(transfer(8), Transfer::Linear);
        assert_eq!(transfer(13), Transfer::Srgb);
    }

    /// The BT.709 family is deliberately flattened onto sRGB, so this is a
    /// decision worth pinning rather than an accident.
    #[test]
    fn the_bt709_family_is_treated_as_srgb() {
        for code in [1, 6, 14, 15] {
            assert_eq!(transfer(code), Transfer::Srgb, "code {code}");
        }
    }

    #[test]
    fn power_law_curves_come_through_as_gamma() {
        assert_eq!(transfer(4), Transfer::Gamma(2.2));
        assert_eq!(transfer(5), Transfer::Gamma(2.8));
    }

    #[test]
    fn wide_gamuts_are_recognised_and_the_rest_round_to_bt709() {
        assert_eq!(primaries(9), Primaries::Bt2020);
        assert_eq!(primaries(12), Primaries::DisplayP3);
        assert_eq!(primaries(11), Primaries::DisplayP3);
        assert_eq!(primaries(1), Primaries::Bt709);
        assert_eq!(primaries(6), Primaries::Bt709);
    }

    /// A file that says nothing, and a code we have never heard of, both have
    /// to land on the convention rather than on something arbitrary.
    #[test]
    fn unspecified_and_unknown_fall_back_to_srgb() {
        assert_eq!(color_space(UNSPECIFIED, UNSPECIFIED), ColorSpace::SRGB);
        assert_eq!(color_space(200, 201), ColorSpace::SRGB);
    }

    /// The combination this exists for: HDR10, stated in four small integers.
    #[test]
    fn bt2100_pq_translates_whole() {
        let hdr10 = color_space(9, 16);
        assert_eq!(hdr10.primaries, Primaries::Bt2020);
        assert_eq!(hdr10.transfer, Transfer::Pq);
    }
}
