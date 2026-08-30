//! How large an image this build will hold.

use anyhow::{Result, anyhow};

/// Ceiling on a single decoded image, in bytes.
///
/// Both backends ship conservative defaults — 256 MiB in `tiff`, 512 MiB in
/// `image` — which a survey-grade elevation model passes on the way out of the
/// door: Mount Rainier at 3 m is 18333x15667, or 1.1 GB of floats.
///
/// 4 GiB is not arbitrary. `max_texture_dimension_2d` is 32768 on current
/// hardware, and 32768 x 32768 x 4 bytes is exactly 4 GiB, so this is the
/// largest single-channel 32-bit image that could be displayed even in
/// principle. Anything past it is refused with a sentence rather than left to
/// the OOM killer.
pub(crate) const MAX_DECODED_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// Refuses an image too large to hold, using only what the header states, so
/// that nothing is decoded before the decision is made.
pub(crate) fn check_decoded_size(
    width: u32,
    height: u32,
    components: usize,
    bits_per_sample: u8,
) -> Result<()> {
    let bytes = u64::from(width)
        * u64::from(height)
        * components as u64
        * u64::from(bits_per_sample.div_ceil(8));
    if bytes > MAX_DECODED_BYTES {
        return Err(anyhow!(
            "{width}x{height} at {bits_per_sample} bits needs {:.1} GB decoded, \
             over the {:.1} GB this build will hold",
            bytes as f64 / 1e9,
            MAX_DECODED_BYTES as f64 / 1e9,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The size a real survey elevation model needs, which both backends'
    /// stock limits refuse. Mount Rainier at 3 m is the case that prompted
    /// raising them.
    #[test]
    fn a_survey_grade_elevation_model_is_within_the_limit() {
        assert!(check_decoded_size(18333, 15667, 1, 32).is_ok());
        // And the 10 m version, comfortably.
        assert!(check_decoded_size(5500, 4700, 1, 32).is_ok());
    }

    /// The ceiling is the largest image the maximum GPU texture dimension
    /// could hold at all, so nothing displayable is turned away.
    #[test]
    fn the_limit_matches_the_largest_displayable_texture() {
        assert_eq!(MAX_DECODED_BYTES, 32768 * 32768 * 4);
        assert!(check_decoded_size(32768, 32768, 1, 32).is_ok());
    }

    #[test]
    fn an_image_too_large_to_hold_is_refused_before_decoding() {
        let error = check_decoded_size(100_000, 100_000, 1, 32).unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("100000x100000"), "{message}");
        assert!(message.contains("GB"), "{message}");
    }

    /// Multiple channels count toward the ceiling, or an RGBA image four
    /// times the size of an allowed grey one would slip through.
    #[test]
    fn channel_count_and_bit_depth_both_count() {
        assert!(check_decoded_size(32768, 32768, 4, 32).is_err());
        assert!(check_decoded_size(32768, 32768, 4, 8).is_ok());
    }
}
