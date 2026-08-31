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
pub mod directory;
pub mod display;
pub mod encode;
pub mod exif;
pub mod geo;
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
    /// The word for the layout, which the bottom bar and the info panel both
    /// write out.
    pub fn label(self) -> &'static str {
        match self {
            Channels::Gray => "gray",
            Channels::GrayAlpha => "gray+alpha",
            Channels::Rgb => "rgb",
            Channels::Rgba => "rgba",
        }
    }

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

    /// How many components carry colour, alpha aside: one for grey, three
    /// otherwise. Grey is replicated across the three on the way to the
    /// screen, so one value is the whole of what the file said.
    pub fn color_count(self) -> usize {
        if self.is_gray() { 1 } else { 3 }
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

    /// The pixel at `(x, y)`, or `None` when that is outside the image.
    ///
    /// Reads one pixel the way `render::upload` and `shaders/image.wgsl`
    /// between them read every pixel — the transfer curve resolved, any
    /// premultiplication divided back out, the primaries taken to the working
    /// space — so that what comes back is the value the display window acts
    /// on. Alpha never gets the curve, here or there.
    ///
    /// One pixel at a time: this is for a readout following the pointer, not
    /// for anything that walks the image.
    pub fn sample(&self, x: u32, y: u32) -> Option<Sample> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let channels = self.channels();
        let count = channels.count();
        let base = (y as usize * self.width as usize + x as usize) * count;

        let mut stored = [0.0f32; 4];
        match &self.samples {
            Samples::U8 { data, .. } => {
                for (slot, raw) in stored.iter_mut().zip(&data[base..base + count]) {
                    *slot = *raw as f32;
                }
            }
            Samples::U16 { data, .. } => {
                for (slot, raw) in stored.iter_mut().zip(&data[base..base + count]) {
                    *slot = *raw as f32;
                }
            }
            Samples::F32 { data, .. } => {
                for (slot, raw) in stored.iter_mut().zip(&data[base..base + count]) {
                    *slot = *raw;
                }
            }
        }

        let scale = 1.0 / self.samples.full_scale();
        let mut color = [0.0f32; 3];
        for (slot, value) in color.iter_mut().zip(&stored[..channels.color_count()]) {
            *slot = self.color.transfer.to_linear(value * scale);
        }
        let alpha = match channels.alpha_index() {
            Some(index) => (stored[index] * scale).clamp(0.0, 1.0),
            None => 1.0,
        };
        if self.alpha == AlphaMode::Premultiplied {
            // The shader's threshold as well as its division: a texel that has
            // resolved to nearly nothing is nothing, rather than a wild colour
            // divided out of it.
            if alpha > 1e-4 {
                for value in &mut color {
                    *value /= alpha;
                }
            } else {
                color = [0.0; 3];
            }
        }
        if !channels.is_gray() {
            color = to_working_space(self.color.primaries.to_bt709(), color);
        }

        Some(Sample {
            channels,
            stored,
            color,
            alpha,
        })
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
        // Checked, since this is the last guard before the sample count is
        // trusted by the upload path; a wrapping product could make a short
        // buffer look the right length. Nothing that reaches here has dodged
        // the size ceiling, so the overflow is a belt-and-braces case.
        let expected = (self.width as usize)
            .checked_mul(self.height as usize)
            .and_then(|pixels| pixels.checked_mul(self.samples.channels().count()));
        let Some(expected) = expected else {
            return Err(format!(
                "{}x{} {:?} overflows the addressable range",
                self.width,
                self.height,
                self.samples.channels(),
            ));
        };
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

/// One pixel read back out of an image, in both the terms it can be read in.
///
/// The distinction is the one the whole model rests on: what the file holds
/// is a measurement, and what the screen shows is that measurement decoded,
/// converted and windowed. A readout wants to say both, so this carries the
/// file's own numbers alongside the values the display pipeline starts from.
/// [`display::Display::map`] takes it the rest of the way.
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub channels: Channels,
    stored: [f32; 4],
    color: [f32; 3],
    /// Coverage as a fraction; 1.0 where the image has no alpha channel.
    pub alpha: f32,
}

impl Sample {
    /// Every component the file carries, alpha included, in the units it
    /// stores them in: counts for integer samples, the value itself for float
    /// ones.
    pub fn stored(&self) -> &[f32] {
        &self.stored[..self.channels.count()]
    }

    /// The colour, in the linear BT.709 working space with premultiplication
    /// undone: one component for grey, three for colour. This is what
    /// `shaders/image.wgsl` has in hand at the moment it applies the window.
    pub fn color(&self) -> &[f32] {
        &self.color[..self.channels.color_count()]
    }
}

/// Row-major 3x3 times a colour, the CPU-side twin of the `primaries` matrix
/// multiply in `shaders/image.wgsl`.
fn to_working_space(matrix: [[f32; 3]; 3], color: [f32; 3]) -> [f32; 3] {
    let mut out = [0.0; 3];
    for (slot, row) in out.iter_mut().zip(matrix) {
        *slot = row[0] * color[0] + row[1] * color[1] + row[2] * color[2];
    }
    out
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

    /// The readout the pointer drives has to say two things at once: what the
    /// file holds, in the units the file holds it in, and what the pipeline
    /// will make of that. So a sample carries both, and the file's own numbers
    /// come back unscaled — a 16-bit count reads as a count.
    #[test]
    fn a_sample_reports_the_file_s_own_numbers_and_the_decoded_ones() {
        let image = gray16(vec![0, 1000, 2000, 3000, 4000, 5000], 3, 2);
        let sample = image.sample(1, 1).expect("inside the image");

        assert_eq!(
            sample.stored(),
            [4000.0],
            "the count, not a fraction of one"
        );
        assert!((sample.color()[0] - 4000.0 / 65535.0).abs() < 1e-6);
        assert_eq!(sample.alpha, 1.0, "an image with no alpha is opaque");

        // Row-major from the top, so the last pixel is the bottom right one.
        assert_eq!(image.sample(2, 1).expect("inside").stored(), [5000.0]);
        assert!(image.sample(3, 1).is_none());
        assert!(image.sample(0, 2).is_none());
    }

    #[test]
    fn a_sample_decodes_the_transfer_curve_but_never_the_alpha() {
        let image = DecodedImage::new(
            1,
            1,
            Samples::U8 {
                channels: Channels::GrayAlpha,
                data: vec![128, 128],
            },
            ColorSpace::SRGB,
            AlphaMode::Straight,
        );

        let sample = image.sample(0, 0).expect("inside the image");
        assert_eq!(sample.stored(), [128.0, 128.0]);
        // The same code in both components, and only one of them curved.
        assert!(
            (sample.color()[0] - 0.2158).abs() < 1e-3,
            "{:?}",
            sample.color()
        );
        assert!((sample.alpha - 128.0 / 255.0).abs() < 1e-6);
    }

    /// What the shader has in hand when it applies the window is the straight
    /// colour, so that is what a sample reports — while `stored` keeps the
    /// faded numbers the file actually contains.
    #[test]
    fn a_premultiplied_sample_is_divided_back_out_the_way_the_shader_does_it() {
        let image = DecodedImage {
            width: 2,
            height: 1,
            samples: Samples::F32 {
                channels: Channels::Rgba,
                data: vec![0.25, 0.5, 0.75, 0.5, 0.0, 0.0, 0.0, 0.0],
            },
            color: ColorSpace::LINEAR_BT709,
            alpha: AlphaMode::Premultiplied,
            value_range: None,
            nodata: None,
        };

        let sample = image.sample(0, 0).expect("inside the image");
        assert_eq!(sample.stored(), [0.25, 0.5, 0.75, 0.5]);
        assert_eq!(sample.color(), [0.5, 1.0, 1.5]);

        // A texel that has resolved to nothing is nothing, rather than a wild
        // colour divided out of an alpha of zero.
        let empty = image.sample(1, 0).expect("inside the image");
        assert_eq!(empty.color(), [0.0, 0.0, 0.0]);
        assert_eq!(empty.alpha, 0.0);
    }

    /// Colour comes back in the working space, since that is where the window
    /// and everything after it happens. Grey has no primaries to convert.
    #[test]
    fn a_sample_is_taken_to_the_working_space() {
        let mut image = DecodedImage::new(
            1,
            1,
            Samples::F32 {
                channels: Channels::Rgb,
                data: vec![0.0, 1.0, 0.0],
            },
            ColorSpace::LINEAR_BT709,
            AlphaMode::Opaque,
        );
        assert_eq!(image.sample(0, 0).expect("inside").color(), [0.0, 1.0, 0.0]);

        image.color.primaries = Primaries::DisplayP3;
        let converted = image.sample(0, 0).expect("inside").color().to_vec();
        // P3 green is outside BT.709, which shows as a negative red.
        assert!(converted[0] < 0.0, "{converted:?}");
        assert!(converted[1] > 1.0, "{converted:?}");
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
