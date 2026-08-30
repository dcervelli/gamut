//! The bridge from the `image` crate's `DynamicImage` to [`DecodedImage`],
//! shared by every decoder that goes through that crate.
//!
//! The work here is not decoding — the crate does that — but deciding what
//! the decoded numbers *mean*, and handing them on without flattening 16-bit
//! or floating-point data down to bytes.

use std::io::{BufReader, Seek};

use ::image::{DynamicImage, ImageFormat};

use anyhow::{Result, anyhow};

use crate::image::{AlphaMode, Channels, ColorSpace, DecodedImage, Samples};

use super::ReadSeek;

/// The size stated in the header of whatever format the crate recognises.
pub(super) fn dimensions(source: &mut dyn ReadSeek) -> Result<Option<(u32, u32)>> {
    let reader = ::image::ImageReader::new(BufReader::new(source)).with_guessed_format()?;
    Ok(Some(reader.into_dimensions()?))
}

/// The crate defaults to a 512 MiB allocation ceiling, which a large EXR or
/// 16-bit scan passes easily. Match the ceiling the TIFF path uses so the two
/// behave the same way.
pub(super) fn limit<R: std::io::BufRead + Seek>(reader: &mut ::image::ImageReader<R>) {
    let mut limits = ::image::Limits::default();
    limits.max_alloc = Some(super::MAX_DECODED_BYTES);
    reader.limits(limits);
}

pub(super) fn decode_as(bytes: &[u8], format: ::image::ImageFormat) -> Result<DynamicImage> {
    let mut reader = ::image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
    limit(&mut reader);
    Ok(reader.decode()?)
}

/// Wraps a decoded buffer in what we know about it. `stated` is the colour
/// space the container claimed, used wherever the pixel type does not settle
/// the question by itself.
pub(super) fn describe(
    decoded: DynamicImage,
    format: Option<ImageFormat>,
    stated: ColorSpace,
) -> Result<DecodedImage> {
    let (width, height) = (decoded.width(), decoded.height());
    let samples = into_samples(decoded)?;
    let color = color_space(format, &samples, stated);
    let alpha = alpha_mode(format, samples.channels());

    // `image` does not surface EXR's per-channel ranges, so the renderer
    // scans for a window instead.
    Ok(DecodedImage::new(width, height, samples, color, alpha))
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
/// and EXR are scene-linear by definition, whatever a profile might say.
fn color_space(format: Option<ImageFormat>, samples: &Samples, stated: ColorSpace) -> ColorSpace {
    match format {
        Some(ImageFormat::Hdr) | Some(ImageFormat::OpenExr) => ColorSpace::LINEAR_BT709,
        _ => match samples {
            // A float buffer from any source is scene-linear; nothing encodes
            // an sRGB curve into floats.
            Samples::F32 { .. } => ColorSpace::LINEAR_BT709,
            _ => stated,
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
