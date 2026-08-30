//! The image data model.
//!
//! Decoders describe what they found rather than normalising it, so that a
//! 16-bit measurement scan and an HDR photograph both survive the trip to the
//! GPU intact. Turning that description into a texture is the renderer's job
//! (see `render::upload`).
//!
//! The pixel model is here; what the numbers mean is [`color`]; the decoders
//! that produce it are [`decode`].

pub mod color;
pub mod decode;
pub mod display;
pub mod stats;

pub use color::{ColorSpace, Primaries, Transfer};
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

/// Whether colour components have already been multiplied by alpha. PNG says
/// straight, EXR usually says premultiplied, and blending them the wrong way
/// shows as haloing.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum AlphaMode {
    Opaque,
    Straight,
    Premultiplied,
}

impl AlphaMode {
    /// The mode for a layout with or without an alpha channel: opaque where
    /// there is none, and otherwise whatever the file says about
    /// premultiplication.
    pub fn of(channels: Channels, premultiplied: bool) -> Self {
        match (channels.alpha_index(), premultiplied) {
            (None, _) => AlphaMode::Opaque,
            (Some(_), true) => AlphaMode::Premultiplied,
            (Some(_), false) => AlphaMode::Straight,
        }
    }
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
    /// An image with nothing stated beyond its pixels: no declared value
    /// range and no no-data sentinel, which is what most formats can say.
    pub fn new(
        width: u32,
        height: u32,
        samples: Samples,
        color: ColorSpace,
        alpha: AlphaMode,
    ) -> Self {
        Self {
            width,
            height,
            samples,
            color,
            alpha,
            value_range: None,
            nodata: None,
        }
    }

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
