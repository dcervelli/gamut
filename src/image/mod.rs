//! The image data model.
//!
//! Decoders describe what they found rather than normalising it, so that a
//! 16-bit measurement scan and an HDR photograph both survive the trip to the
//! GPU intact. Turning that description into a texture is the renderer's job
//! (see `render::upload`).

pub mod decode;
pub mod display;
pub mod stats;

pub use stats::Stats;

/// How many components each pixel carries, and what they mean.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Channels {
    Gray,
    GrayAlpha,
    Rgb,
    Rgba,
}

impl Channels {
    pub fn count(self) -> usize {
        match self {
            Channels::Gray => 1,
            Channels::GrayAlpha => 2,
            Channels::Rgb => 3,
            Channels::Rgba => 4,
        }
    }

    /// Component index of alpha, if there is one.
    pub fn alpha_index(self) -> Option<usize> {
        match self {
            Channels::Gray | Channels::Rgb => None,
            Channels::GrayAlpha => Some(1),
            Channels::Rgba => Some(3),
        }
    }

    pub fn is_gray(self) -> bool {
        matches!(self, Channels::Gray | Channels::GrayAlpha)
    }
}

/// Pixel data, row-major from the top, tightly packed, in the component type
/// the file actually used.
///
/// Typed vectors rather than `Vec<u8>` plus a tag: alignment for
/// `bytemuck::cast_slice` on upload is then guaranteed, and CPU-side work such
/// as histogramming reads naturally. `f16` is deliberately absent — no decoder
/// produces it, and it only appears as an upload target.
#[derive(Clone, Debug)]
pub enum Samples {
    U8 { channels: Channels, data: Vec<u8> },
    U16 { channels: Channels, data: Vec<u16> },
    F32 { channels: Channels, data: Vec<f32> },
}

impl Samples {
    pub fn channels(&self) -> Channels {
        match *self {
            Samples::U8 { channels, .. }
            | Samples::U16 { channels, .. }
            | Samples::F32 { channels, .. } => channels,
        }
    }

    /// Number of components, i.e. `width * height * channels.count()`.
    pub fn len(&self) -> usize {
        match self {
            Samples::U8 { data, .. } => data.len(),
            Samples::U16 { data, .. } => data.len(),
            Samples::F32 { data, .. } => data.len(),
        }
    }

    /// The value a fully bright component has before any transfer decode.
    /// Float samples are already in their final units, so they scale by one.
    pub fn full_scale(&self) -> f32 {
        match self {
            Samples::U8 { .. } => u8::MAX as f32,
            Samples::U16 { .. } => u16::MAX as f32,
            Samples::F32 { .. } => 1.0,
        }
    }

    pub fn component_name(&self) -> &'static str {
        match self {
            Samples::U8 { .. } => "8-bit",
            Samples::U16 { .. } => "16-bit",
            Samples::F32 { .. } => "32-bit float",
        }
    }
}

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
            Transfer::Gamma(gamma) => value.max(0.0).powf(gamma),
        }
    }

    /// True when the values are already linear and need no conversion.
    pub fn is_linear(self) -> bool {
        matches!(self, Transfer::Linear)
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

/// Whether colour components have already been multiplied by alpha. PNG says
/// straight, EXR usually says premultiplied, and blending them the wrong way
/// shows as haloing.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum AlphaMode {
    Opaque,
    Straight,
    Premultiplied,
}

/// One decoded image, described rather than normalised.
#[derive(Clone, Debug)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub samples: Samples,
    pub color: ColorSpace,
    pub alpha: AlphaMode,
    /// The range the file says its values occupy, in the same linear
    /// working-space units the shader will sample — decoders converting from
    /// something like TIFF's `SMinSampleValue` must scale it themselves.
    /// `None` means the display window is chosen by scanning instead.
    pub value_range: Option<(f32, f32)>,
    /// The value standing in for "no measurement here". Elevation models use
    /// -9999 and similar sentinels, which would otherwise dominate the
    /// automatic window and squash the real data into a sliver.
    pub nodata: Option<f32>,
}

impl DecodedImage {
    pub fn channels(&self) -> Channels {
        self.samples.channels()
    }

    pub fn is_gray(&self) -> bool {
        self.channels().is_gray()
    }

    /// True when the content can exceed the SDR range and so needs either
    /// tone mapping or an HDR output.
    pub fn is_high_dynamic_range(&self) -> bool {
        matches!(self.samples, Samples::F32 { .. })
            || matches!(self.color.transfer, Transfer::Pq | Transfer::Hlg)
    }

    /// Sanity check used by the loader, so a broken decoder fails loudly
    /// rather than reading past the end of a buffer on the upload path.
    pub fn validate(&self) -> Result<(), String> {
        if self.width == 0 || self.height == 0 {
            return Err("image has zero size".into());
        }
        let expected = self.width as usize * self.height as usize * self.samples.channels().count();
        if self.samples.len() != expected {
            return Err(format!(
                "decoder produced {} components, expected {expected} for {}x{} {:?}",
                self.samples.len(),
                self.width,
                self.height,
                self.samples.channels(),
            ));
        }
        Ok(())
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

    #[test]
    fn channel_layout_is_self_consistent() {
        for channels in [
            Channels::Gray,
            Channels::GrayAlpha,
            Channels::Rgb,
            Channels::Rgba,
        ] {
            if let Some(index) = channels.alpha_index() {
                assert_eq!(index, channels.count() - 1, "alpha is the last component");
            }
        }
        assert!(Channels::Gray.is_gray());
        assert!(Channels::GrayAlpha.is_gray());
        assert!(!Channels::Rgb.is_gray());
    }

    fn gray16(data: Vec<u16>, width: u32, height: u32) -> DecodedImage {
        DecodedImage {
            width,
            height,
            samples: Samples::U16 {
                channels: Channels::Gray,
                data,
            },
            color: ColorSpace::LINEAR_BT709,
            alpha: AlphaMode::Opaque,
            value_range: None,
            nodata: None,
        }
    }

    #[test]
    fn validate_rejects_a_buffer_of_the_wrong_length() {
        assert!(gray16(vec![0; 6], 3, 2).validate().is_ok());
        assert!(gray16(vec![0; 5], 3, 2).validate().is_err());
        assert!(gray16(vec![], 0, 2).validate().is_err());
    }

    #[test]
    fn high_dynamic_range_is_recognised_by_type_and_by_curve() {
        let mut image = gray16(vec![0; 6], 3, 2);
        assert!(!image.is_high_dynamic_range());

        image.color.transfer = Transfer::Pq;
        assert!(image.is_high_dynamic_range());

        image.color.transfer = Transfer::Linear;
        image.samples = Samples::F32 {
            channels: Channels::Gray,
            data: vec![0.0; 6],
        };
        assert!(image.is_high_dynamic_range());
    }
}
