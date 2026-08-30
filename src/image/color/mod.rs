//! What the numbers in an image mean: the curve they are encoded with and the
//! primaries they are expressed in.
//!
//! Two vocabularies say this in files. [`cicp`] translates the code points of
//! ITU-T H.273, which HEIF and PNG carry; [`icc`] reads what an embedded
//! profile states in a form this model can act on. Both land here.

pub mod cicp;
pub mod icc;

/// The opto-electronic transfer function the stored values are encoded with.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Transfer {
    /// Values are already proportional to light. Sensor counts, EXR, Radiance.
    Linear,
    /// IEC 61966-2-1, the ordinary sRGB curve.
    Srgb,
    /// SMPTE ST 2084, absolute, 10000 nits at 1.0.
    Pq,
    /// ARIB STD-B67 hybrid log-gamma.
    Hlg,
    /// A plain power law, e.g. 2.2 for many older TIFFs.
    Gamma(f32),
}

impl Transfer {
    /// Decodes one encoded component to a linear one.
    ///
    /// Used to build the CPU-side lookup tables in `render::upload`; the GPU
    /// never runs this, because the texture always ends up holding linear
    /// values (see the module docs there for why).
    pub fn to_linear(self, value: f32) -> f32 {
        match self {
            Transfer::Linear => value,
            Transfer::Srgb => {
                if value <= 0.04045 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
            }
            Transfer::Pq => {
                // `value.max(0.0)` below would fold a NaN to 0.0 (`f32::max`
                // returns the non-NaN operand), which then bins and windows as
                // real data. Kept as NaN, it is filtered by the stats scan
                // exactly as an sRGB or HLG NaN already is.
                if value.is_nan() {
                    return value;
                }
                const M1: f32 = 2610.0 / 16384.0;
                const M2: f32 = 128.0 * 2523.0 / 4096.0;
                const C1: f32 = 3424.0 / 4096.0;
                const C2: f32 = 32.0 * 2413.0 / 4096.0;
                const C3: f32 = 32.0 * 2392.0 / 4096.0;
                let encoded = value.max(0.0).powf(1.0 / M2);
                let numerator = (encoded - C1).max(0.0);
                let denominator = C2 - C3 * encoded;
                // 10000 nits full scale, normalised to 203 nits reference white.
                (numerator / denominator).powf(1.0 / M1) * (10000.0 / 203.0)
            }
            Transfer::Hlg => {
                const A: f32 = 0.17883277;
                const B: f32 = 1.0 - 4.0 * A;
                // 0.5 - A * ln(4A), precomputed: `ln` is not const.
                const C: f32 = 0.559_910_7;
                let scene = if value <= 0.5 {
                    value * value / 3.0
                } else {
                    (((value - C) / A).exp() + B) / 12.0
                };
                // Nominal 1000 nit system gamma, normalised to reference white.
                scene * (1000.0 / 203.0)
            }
            Transfer::Gamma(gamma) => {
                if value.is_nan() {
                    return value;
                }
                value.max(0.0).powf(gamma)
            }
        }
    }

    /// Encodes a linear component back the way the file stored it, the exact
    /// inverse of [`Transfer::to_linear`].
    ///
    /// Used to plot histograms on the curve the samples were quantised
    /// against: binning decoded values uniformly leaves gaps between the
    /// codes of an 8-bit file, because a code step at the top of the range is
    /// several times wider in linear terms than one at the bottom.
    pub fn to_encoded(self, value: f32) -> f32 {
        match self {
            Transfer::Linear => value,
            // Negative light has no encoding; the curves below are defined on
            // 0.. only, and `powf` of a negative is NaN. Mirroring keeps such
            // a value ordered relative to its neighbours instead.
            _ if value < 0.0 => -self.to_encoded(-value),
            Transfer::Srgb => {
                if value <= 0.0031308 {
                    value * 12.92
                } else {
                    1.055 * value.powf(1.0 / 2.4) - 0.055
                }
            }
            Transfer::Pq => {
                const M1: f32 = 2610.0 / 16384.0;
                const M2: f32 = 128.0 * 2523.0 / 4096.0;
                const C1: f32 = 3424.0 / 4096.0;
                const C2: f32 = 32.0 * 2413.0 / 4096.0;
                const C3: f32 = 32.0 * 2392.0 / 4096.0;
                let normalised = (value * (203.0 / 10000.0)).powf(M1);
                ((C1 + C2 * normalised) / (1.0 + C3 * normalised)).powf(M2)
            }
            Transfer::Hlg => {
                const A: f32 = 0.17883277;
                const B: f32 = 1.0 - 4.0 * A;
                const C: f32 = 0.559_910_7;
                let scene = value * (203.0 / 1000.0);
                if scene <= 1.0 / 12.0 {
                    (3.0 * scene).sqrt()
                } else {
                    A * (12.0 * scene - B).ln() + C
                }
            }
            Transfer::Gamma(gamma) => value.powf(1.0 / gamma),
        }
    }

    /// True when the values are already linear and need no conversion.
    pub fn is_linear(self) -> bool {
        matches!(self, Transfer::Linear)
    }

    /// `linear`, `srgb`, `pq`, `hlg`, or `gamma:<N>`, as the command line
    /// names them.
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.to_ascii_lowercase().as_str() {
            "linear" => Transfer::Linear,
            "srgb" => Transfer::Srgb,
            "pq" => Transfer::Pq,
            "hlg" => Transfer::Hlg,
            other => {
                let gamma: f32 = other.strip_prefix("gamma:")?.parse().ok()?;
                // A non-positive or non-finite exponent is not a transfer
                // function: `gamma:0` maps every sample to 1.0, and `gamma:nan`
                // poisons the lookup table built from it. `icc.rs` already
                // guards the exponent it reads from a profile; the command
                // line gets the same guard.
                if !gamma.is_finite() || gamma <= 0.0 {
                    return None;
                }
                Transfer::Gamma(gamma)
            }
        })
    }
}

/// The chromaticities the RGB components are expressed in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Primaries {
    /// sRGB / Rec.709. The working space, so this needs no conversion.
    Bt709,
    DisplayP3,
    Bt2020,
    AdobeRgb,
}

impl Primaries {
    /// `bt709`, `p3`, `bt2020`, or `adobe`, as the command line names them,
    /// with the aliases people actually type.
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.to_ascii_lowercase().as_str() {
            "bt709" | "srgb" | "rec709" => Primaries::Bt709,
            "p3" | "displayp3" => Primaries::DisplayP3,
            "bt2020" | "rec2020" => Primaries::Bt2020,
            "adobe" | "adobergb" => Primaries::AdobeRgb,
            _ => return None,
        })
    }

    /// Row-major 3x3 taking these primaries to the linear BT.709 working
    /// space, both with a D65 white point.
    pub fn to_bt709(self) -> [[f32; 3]; 3] {
        match self {
            Primaries::Bt709 => [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            Primaries::DisplayP3 => [
                [1.224_940_2, -0.224_940_18, 0.0],
                [-0.042_056_95, 1.042_056_9, 0.0],
                [-0.019_637_555, -0.078_636_04, 1.098_273_6],
            ],
            Primaries::Bt2020 => [
                [1.660_496, -0.587_656_1, -0.072_839_91],
                [-0.124_547_43, 1.132_896, -0.008_348_5],
                [-0.018_154_59, -0.100_597_69, 1.118_752_3],
            ],
            Primaries::AdobeRgb => [
                [1.398_374_5, -0.398_374_47, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, -0.042_930_14, 1.042_930_1],
            ],
        }
    }
}

/// Transfer function plus primaries: everything needed to interpret the
/// numbers in `Samples`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ColorSpace {
    pub transfer: Transfer,
    pub primaries: Primaries,
}

impl ColorSpace {
    /// What an ordinary PNG or JPEG without an embedded profile means.
    pub const SRGB: Self = Self {
        transfer: Transfer::Srgb,
        primaries: Primaries::Bt709,
    };

    /// What EXR and Radiance mean: scene-linear light, Rec.709 primaries.
    pub const LINEAR_BT709: Self = Self {
        transfer: Transfer::Linear,
        primaries: Primaries::Bt709,
    };

    pub fn label(&self) -> String {
        let transfer = match self.transfer {
            Transfer::Linear => "linear".to_string(),
            Transfer::Srgb => "sRGB".to_string(),
            Transfer::Pq => "PQ".to_string(),
            Transfer::Hlg => "HLG".to_string(),
            Transfer::Gamma(g) => format!("gamma {g:.2}"),
        };
        let primaries = match self.primaries {
            Primaries::Bt709 => "BT.709",
            Primaries::DisplayP3 => "Display P3",
            Primaries::Bt2020 => "BT.2020",
            Primaries::AdobeRgb => "Adobe RGB",
        };
        format!("{primaries} / {transfer}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn srgb_decodes_at_the_documented_anchors() {
        assert!(close(Transfer::Srgb.to_linear(0.0), 0.0));
        assert!(close(Transfer::Srgb.to_linear(1.0), 1.0));
        // Mid grey: sRGB 0.5 is a little over 21% of the light.
        assert!(close(Transfer::Srgb.to_linear(0.5), 0.214_041));
        // The curve is continuous across the join between its two pieces.
        let below = Transfer::Srgb.to_linear(0.040_44);
        let above = Transfer::Srgb.to_linear(0.040_46);
        assert!((above - below).abs() < 1e-5);
    }

    #[test]
    fn linear_transfer_is_the_identity() {
        for value in [0.0, 0.25, 1.0, 8.0] {
            assert!(close(Transfer::Linear.to_linear(value), value));
        }
        assert!(Transfer::Linear.is_linear());
        assert!(!Transfer::Srgb.is_linear());
    }

    #[test]
    fn pq_puts_reference_white_at_one() {
        // ST 2084 encodes 203 nits — the reference white the pipeline
        // normalises to — at roughly 0.58 of its code range.
        let white = Transfer::Pq.to_linear(0.580_69);
        assert!((white - 1.0).abs() < 0.02, "got {white}");
        // And it reaches far above SDR range at full scale.
        assert!(Transfer::Pq.to_linear(1.0) > 45.0);
    }

    /// Every curve has to round-trip, since the histogram plots samples on
    /// the curve they were quantised against and reads the window back off it.
    #[test]
    fn every_transfer_encodes_back_to_where_it_started() {
        let transfers = [
            Transfer::Linear,
            Transfer::Srgb,
            Transfer::Pq,
            Transfer::Hlg,
            Transfer::Gamma(2.2),
        ];
        for transfer in transfers {
            for encoded in [0.0, 0.02, 0.25, 0.5, 0.75, 1.0] {
                let round_trip = transfer.to_encoded(transfer.to_linear(encoded));
                assert!(
                    (round_trip - encoded).abs() < 1e-3,
                    "{transfer:?}: {encoded} came back as {round_trip}"
                );
            }
        }
    }

    /// Windows can sit below zero once exposure and contrast have been at
    /// them, and a NaN there would put a marker nowhere.
    #[test]
    fn encoding_a_negative_value_stays_finite_and_ordered() {
        for transfer in [Transfer::Srgb, Transfer::Gamma(2.2), Transfer::Pq] {
            let low = transfer.to_encoded(-0.5);
            let high = transfer.to_encoded(-0.1);
            assert!(low.is_finite() && high.is_finite(), "{transfer:?}");
            assert!(low < high && high < 0.0, "{transfer:?}: {low} {high}");
        }
    }

    #[test]
    fn bt709_needs_no_primaries_conversion() {
        let matrix = Primaries::Bt709.to_bt709();
        for (row_index, row) in matrix.iter().enumerate() {
            for (column_index, value) in row.iter().enumerate() {
                let expected = if row_index == column_index { 1.0 } else { 0.0 };
                assert!(close(*value, expected));
            }
        }
    }

    /// A conversion matrix must leave the white point alone, or every image
    /// through it picks up a colour cast.
    #[test]
    fn primaries_conversions_preserve_white() {
        for primaries in [Primaries::DisplayP3, Primaries::Bt2020, Primaries::AdobeRgb] {
            let matrix = primaries.to_bt709();
            for (index, row) in matrix.iter().enumerate() {
                let sum: f32 = row.iter().sum();
                assert!(
                    (sum - 1.0).abs() < 1e-3,
                    "{primaries:?} row {index} sums to {sum}"
                );
            }
        }
    }
}
