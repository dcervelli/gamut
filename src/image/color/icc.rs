//! What an embedded ICC profile says about the numbers in a file.
//!
//! Only the two things this viewer's color model can act on are taken from a
//! profile: which primaries the components are expressed in, and — where the
//! profile states a plain power law — the transfer function. Everything else
//! ICC can describe (lookup-table transforms, non-RGB connection spaces,
//! rendering intents) is outside what the shader does, and is left alone
//! rather than half-applied.
//!
//! Primaries are recognized by matching the profile's colorants against those
//! of the five spaces [`Primaries`] can name, rather than by reading the
//! description text: a profile written by a phone says "Display P3" but one
//! written by a scanner says whatever its vendor felt like, and the numbers
//! are the same either way. Colorants are written adapted to the D50 of
//! the profile connection space, whatever the space's own white, so the
//! comparison is between like and like — and ProPhoto, whose white is D50
//! already, is the one whose colorants are its own.

use moxcms::{ColorProfile, ToneReprCurve};

use super::{ColorSpace, Primaries, Transfer};

/// How far apart two colorant matrices may be and still be taken as the same
/// space. The five candidates are far more different from each other than
/// this — sRGB and Display P3, the closest pair, differ by about 0.08 in
/// their largest entry — so the threshold only has to be loose enough for the
/// rounding a profile picks up on the way through 16-bit fixed point.
const TOLERANCE: f64 = 0.01;

/// Reads `profile`, falling back to `assumed` for anything it does not state
/// in a form this viewer can use.
pub fn color_space(profile: &[u8], assumed: ColorSpace) -> ColorSpace {
    let Ok(parsed) = ColorProfile::new_from_slice(profile) else {
        return assumed;
    };
    ColorSpace {
        primaries: primaries(&parsed).unwrap_or(assumed.primaries),
        transfer: transfer(&parsed).unwrap_or(assumed.transfer),
    }
}

/// The nearest of the primaries we can name, or `None` when the profile does
/// not describe itself with colorants or does not land near any of them.
fn primaries(profile: &ColorProfile) -> Option<Primaries> {
    let colorants = profile.colorant_matrix();

    // A profile that describes its transform with lookup tables leaves the
    // colorant tags absent, and `moxcms` reports those as zero. There is
    // nothing to match against, and matching anyway would call every one of
    // them sRGB.
    let magnitude: f64 = colorants.v.iter().flatten().map(|value| value.abs()).sum();
    if magnitude < 1e-6 {
        return None;
    }

    // Built from `moxcms`'s own reference profiles rather than from constants
    // written out here, so that both sides of the comparison come from the
    // same chromatic adaptation.
    let candidates = [
        (Primaries::Bt709, ColorProfile::new_srgb()),
        (Primaries::DisplayP3, ColorProfile::new_display_p3()),
        (Primaries::Bt2020, ColorProfile::new_bt2020()),
        (Primaries::AdobeRgb, ColorProfile::new_adobe_rgb()),
        (Primaries::ProPhoto, ColorProfile::new_pro_photo_rgb()),
    ];

    let (nearest, distance) = candidates
        .iter()
        .map(|(primaries, reference)| {
            let reference = reference.colorant_matrix();
            let distance = colorants
                .v
                .iter()
                .flatten()
                .zip(reference.v.iter().flatten())
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f64, f64::max);
            (*primaries, distance)
        })
        .min_by(|(_, a), (_, b)| a.total_cmp(b))?;

    (distance <= TOLERANCE).then_some(nearest)
}

/// The transfer function, but only where the profile states a plain power
/// law.
///
/// ICC has two spellings of one. A parametric curve of type 0 is exactly
/// `Y = X^g`, which is what [`Transfer::Gamma`] means; and a `curv` tag
/// with a single entry is the same thing with the exponent in unsigned
/// 8.8 fixed point, which is how Adobe's own profiles state Adobe RGB's
/// 2.2 and ProPhoto's 1.8, and how `moxcms` writes its references. The
/// parametric types 1 to 4 add a linear toe, and the one that matters —
/// sRGB — is already what the caller assumes for every format that carries
/// a profile at all, so reading it back would change nothing while risking
/// a worse answer on the curves that only approximate it.
fn transfer(profile: &ColorProfile) -> Option<Transfer> {
    let gamma = match profile.red_trc.as_ref()? {
        ToneReprCurve::Parametric(values) if values.len() == 1 => values[0],
        ToneReprCurve::Lut(table) if table.len() == 1 => f32::from(table[0]) / 256.0,
        _ => return None,
    };
    (gamma.is_finite() && gamma > 0.0).then_some(Transfer::Gamma(gamma))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trips each reference profile through its own encoder, so the
    /// match is tested against real profile bytes rather than against the
    /// in-memory structure it was built from.
    fn recognize(profile: &ColorProfile) -> ColorSpace {
        let encoded = profile.encode().expect("reference profiles encode");
        color_space(&encoded, ColorSpace::SRGB)
    }

    #[test]
    fn each_reference_profile_is_recognized_as_itself() {
        let cases = [
            (ColorProfile::new_srgb(), Primaries::Bt709),
            (ColorProfile::new_display_p3(), Primaries::DisplayP3),
            (ColorProfile::new_bt2020(), Primaries::Bt2020),
            (ColorProfile::new_adobe_rgb(), Primaries::AdobeRgb),
            (ColorProfile::new_pro_photo_rgb(), Primaries::ProPhoto),
        ];
        for (profile, expected) in cases {
            assert_eq!(recognize(&profile).primaries, expected);
        }
    }

    /// The gamma the reference profiles state as a one-entry `curv` — the
    /// spelling Adobe's own profiles use — is read, and read at its 8.8
    /// precision: 2.2 arrives as 2.19921875, which is what the profile
    /// says, not what its author meant.
    #[test]
    fn a_one_entry_curve_is_a_power_law() {
        assert_eq!(
            recognize(&ColorProfile::new_adobe_rgb()).transfer,
            Transfer::Gamma(2.199_218_8)
        );
        assert_eq!(
            recognize(&ColorProfile::new_pro_photo_rgb()).transfer,
            Transfer::Gamma(1.800_781_2)
        );
        // sRGB's own curve is not a power law, and the assumption stands.
        assert_eq!(
            recognize(&ColorProfile::new_srgb()).transfer,
            Transfer::Srgb
        );
    }

    /// The distinction this whole module exists for. Display P3 is the
    /// closest neighbor sRGB has among the four, and confusing the two is
    /// what makes a phone photograph look washed out.
    #[test]
    fn display_p3_is_not_mistaken_for_srgb() {
        let srgb = ColorProfile::new_srgb().colorant_matrix();
        let p3 = ColorProfile::new_display_p3().colorant_matrix();
        let distance = srgb
            .v
            .iter()
            .flatten()
            .zip(p3.v.iter().flatten())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f64, f64::max);
        assert!(
            distance > TOLERANCE * 4.0,
            "sRGB and P3 are {distance} apart, too close to the {TOLERANCE} tolerance"
        );
    }

    /// Nonsense in must not become a confident color space out.
    #[test]
    fn an_unreadable_profile_leaves_the_assumption_alone() {
        assert_eq!(
            color_space(b"not a profile", ColorSpace::SRGB),
            ColorSpace::SRGB
        );
        assert_eq!(
            color_space(&[], ColorSpace::LINEAR_BT709),
            ColorSpace::LINEAR_BT709
        );
    }

    /// A profile with no colorants — the shape a lookup-table profile has —
    /// must fall back rather than match whichever candidate zero is nearest.
    #[test]
    fn a_profile_without_colorants_falls_back() {
        let mut profile = ColorProfile::new_srgb();
        profile.red_colorant = Default::default();
        profile.green_colorant = Default::default();
        profile.blue_colorant = Default::default();
        assert_eq!(primaries(&profile), None);
    }

    #[test]
    fn a_plain_power_law_is_read_back_as_one() {
        let mut profile = ColorProfile::new_srgb();
        profile.red_trc = Some(ToneReprCurve::Parametric(vec![2.2]));
        assert_eq!(transfer(&profile), Some(Transfer::Gamma(2.2)));

        // The sRGB curve is parametric too, but not a power law, and is left
        // for the caller's assumption to cover.
        profile.red_trc = Some(ToneReprCurve::Parametric(vec![
            2.4, 0.947_867, 0.052_133, 0.077_399, 0.040_45,
        ]));
        assert_eq!(transfer(&profile), None);
    }
}
