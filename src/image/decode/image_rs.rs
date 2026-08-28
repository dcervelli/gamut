//! Everything the `image` crate handles: PNG, JPEG, TIFF, Radiance HDR and
//! OpenEXR.
//!
//! The work here is not decoding — the crate does that — but deciding what the
//! decoded numbers *mean*, and handing them on without flattening 16-bit or
//! floating-point data down to bytes.

use std::io::BufReader;

use ::image::{DynamicImage, ImageFormat};

use anyhow::{Result, anyhow};

use crate::image::{AlphaMode, Channels, ColorSpace, DecodedImage, Samples};

pub struct ImageRs;

impl super::Decoder for ImageRs {
    fn name(&self) -> &'static str {
        "png/jpeg/hdr/exr"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["png", "jpg", "jpeg", "jpe", "jfif", "hdr", "exr"]
    }

    fn sniff(&self, header: &[u8]) -> bool {
        header.starts_with(b"\x89PNG\r\n\x1a\n")
            || header.starts_with(b"\xff\xd8\xff")
            || header.starts_with(b"#?RADIANCE")
            || header.starts_with(b"#?RGBE")
            || header.starts_with(b"\x76\x2f\x31\x01")
    }

    fn decode(&self, source: &mut dyn super::ReadSeek) -> Result<DecodedImage> {
        let mut reader = ::image::ImageReader::new(BufReader::new(source)).with_guessed_format()?;

        // The crate defaults to a 512 MiB allocation ceiling, which a large
        // EXR or 16-bit scan passes easily. Match the ceiling the TIFF path
        // uses so the two behave the same way.
        let mut limits = ::image::Limits::default();
        limits.max_alloc = Some(super::MAX_DECODED_BYTES);
        reader.limits(limits);

        let format = reader.format();
        let decoded = reader.decode()?;

        let (width, height) = (decoded.width(), decoded.height());
        let samples = into_samples(decoded)?;
        let color = color_space(format, &samples);
        let alpha = alpha_mode(format, samples.channels());

        Ok(DecodedImage {
            width,
            height,
            samples,
            color,
            alpha,
            // `image` does not surface EXR's per-channel ranges, so the
            // renderer scans instead.
            value_range: None,
            nodata: None,
        })
    }
}

/// Moves the decoded buffer across without touching the values. Every
/// `DynamicImage` variant maps onto exactly one `Samples` shape.
fn into_samples(decoded: DynamicImage) -> Result<Samples> {
    Ok(match decoded {
        DynamicImage::ImageLuma8(buffer) => Samples::U8 {
            channels: Channels::Gray,
            data: buffer.into_raw(),
        },
        DynamicImage::ImageLumaA8(buffer) => Samples::U8 {
            channels: Channels::GrayAlpha,
            data: buffer.into_raw(),
        },
        DynamicImage::ImageRgb8(buffer) => Samples::U8 {
            channels: Channels::Rgb,
            data: buffer.into_raw(),
        },
        DynamicImage::ImageRgba8(buffer) => Samples::U8 {
            channels: Channels::Rgba,
            data: buffer.into_raw(),
        },
        DynamicImage::ImageLuma16(buffer) => Samples::U16 {
            channels: Channels::Gray,
            data: buffer.into_raw(),
        },
        DynamicImage::ImageLumaA16(buffer) => Samples::U16 {
            channels: Channels::GrayAlpha,
            data: buffer.into_raw(),
        },
        DynamicImage::ImageRgb16(buffer) => Samples::U16 {
            channels: Channels::Rgb,
            data: buffer.into_raw(),
        },
        DynamicImage::ImageRgba16(buffer) => Samples::U16 {
            channels: Channels::Rgba,
            data: buffer.into_raw(),
        },
        DynamicImage::ImageRgb32F(buffer) => Samples::F32 {
            channels: Channels::Rgb,
            data: buffer.into_raw(),
        },
        DynamicImage::ImageRgba32F(buffer) => Samples::F32 {
            channels: Channels::Rgba,
            data: buffer.into_raw(),
        },
        other => {
            return Err(anyhow!(
                "`image` produced a pixel layout this build does not handle ({:?})",
                other.color()
            ));
        }
    })
}

/// What the stored numbers mean.
///
/// PNG and JPEG are sRGB by definition in the absence of a profile. Radiance
/// and EXR are scene-linear by definition.
fn color_space(format: Option<ImageFormat>, samples: &Samples) -> ColorSpace {
    match format {
        Some(ImageFormat::Hdr) | Some(ImageFormat::OpenExr) => ColorSpace::LINEAR_BT709,
        _ => match samples {
            // A float buffer from any source is scene-linear; nothing encodes
            // an sRGB curve into floats.
            Samples::F32 { .. } => ColorSpace::LINEAR_BT709,
            _ => ColorSpace::SRGB,
        },
    }
}

fn alpha_mode(format: Option<ImageFormat>, channels: Channels) -> AlphaMode {
    if channels.alpha_index().is_none() {
        return AlphaMode::Opaque;
    }
    match format {
        // EXR's convention is premultiplied ("associated") alpha.
        Some(ImageFormat::OpenExr) => AlphaMode::Premultiplied,
        _ => AlphaMode::Straight,
    }
}
