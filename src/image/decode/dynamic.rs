//! The bridge from the `image` crate's `DynamicImage` to [`DecodedImage`],
//! shared by every decoder that goes through that crate.
//!
//! The work here is not decoding — the crate does that — but deciding what
//! the decoded numbers *mean*, and handing them on without flattening 16-bit
//! or floating-point data down to bytes.

use std::io::{BufReader, Seek};
use std::time::Duration;

use ::image::{DynamicImage, ImageFormat};

use anyhow::{Result, anyhow, bail};

use crate::image::{AlphaMode, Channels, ColorSpace, DecodedImage, Samples};

use super::ReadSeek;

/// The size stated in the header of whatever format the crate recognizes.
pub(super) fn dimensions(source: &mut dyn ReadSeek) -> Result<Option<(u32, u32)>> {
    let reader = ::image::ImageReader::new(BufReader::new(source)).with_guessed_format()?;
    Ok(Some(reader.into_dimensions()?))
}

/// The crate defaults to a 512 MiB allocation ceiling, which a large EXR or
/// 16-bit scan passes easily. Match the ceiling the TIFF path uses so the two
/// behave the same way.
pub(super) fn limit<R: std::io::BufRead + Seek>(reader: &mut ::image::ImageReader<R>) {
    reader.limits(limits());
}

/// The same ceiling, for a decoder built directly rather than through
/// `ImageReader` — which is how an animation is opened, since the reader
/// hands back one image and no frames.
pub(super) fn limits() -> ::image::Limits {
    let mut limits = ::image::Limits::default();
    limits.max_alloc = Some(super::MAX_DECODED_BYTES);
    // `max_alloc` is documented as advisory — some decoders honor it, some do
    // not — while the dimension limits are strict for every one. Set them to
    // the same ceiling the GPU imposes, so a format `image` decodes without
    // consulting `max_alloc` (EXR, HDR) still cannot claim an unbounded size.
    limits.max_image_width = Some(super::MAX_TEXTURE_DIMENSION);
    limits.max_image_height = Some(super::MAX_TEXTURE_DIMENSION);
    limits
}

/// One frame of an animation the crate composited, as this program's own
/// frame. The crate's frames are always RGBA8, whatever the file held, so
/// the bridge is the one `describe` already has for that layout.
pub(super) fn frame(
    frame: ::image::Frame,
    format: ImageFormat,
    stated: ColorSpace,
) -> Result<crate::image::sequence::Frame> {
    let delay = delay(frame.delay());
    let image = describe(
        DynamicImage::ImageRgba8(frame.into_buffer()),
        Some(format),
        stated,
    )?;
    Ok(crate::image::sequence::Frame { image, delay })
}

/// A frame's delay as a duration. The crate keeps it as a ratio of
/// milliseconds, exact for what the file said; a denominator of zero is not
/// something the crate produces, and reads as no delay rather than a panic.
fn delay(delay: ::image::Delay) -> Duration {
    let (numer, denom) = delay.numer_denom_ms();
    if denom == 0 {
        return Duration::ZERO;
    }
    Duration::from_micros(u64::from(numer) * 1000 / u64::from(denom))
}

pub(super) fn decode_as(bytes: &[u8], format: ::image::ImageFormat) -> Result<DynamicImage> {
    let mut reader = ::image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
    limit(&mut reader);
    Ok(reader.decode()?)
}

/// Wraps a decoded buffer in what we know about it. `stated` is the color
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

/// The image turned the way `orientation` says, with everything else about
/// it kept. A quarter turn swaps width and height. The eight cases are
/// `image`'s, which has them tested; the buffer goes across to it and back
/// through the same one-to-one mapping the decoders use, so nothing is
/// converted on the way.
pub(super) fn reorient(
    image: DecodedImage,
    orientation: ::image::metadata::Orientation,
) -> Result<DecodedImage> {
    if orientation == ::image::metadata::Orientation::NoTransforms {
        return Ok(image);
    }
    let DecodedImage {
        width,
        height,
        samples,
        color,
        alpha,
        referred,
        nodata,
    } = image;
    let mut dynamic = from_samples(width, height, samples)?;
    dynamic.apply_orientation(orientation);
    let (width, height) = (dynamic.width(), dynamic.height());
    Ok(DecodedImage {
        width,
        height,
        samples: into_samples(dynamic)?,
        color,
        alpha,
        referred,
        nodata,
    })
}

/// The other direction of [`into_samples`]: the buffer lent to `image` for
/// something it does better, such as a turn.
fn from_samples(width: u32, height: u32, samples: Samples) -> Result<DynamicImage> {
    let short = || anyhow!("the buffer holds fewer pixels than {width}x{height}");
    Ok(match samples {
        Samples::U8 { channels, data } => match channels {
            Channels::Gray => DynamicImage::ImageLuma8(
                ::image::ImageBuffer::from_raw(width, height, data).ok_or_else(short)?,
            ),
            Channels::GrayAlpha => DynamicImage::ImageLumaA8(
                ::image::ImageBuffer::from_raw(width, height, data).ok_or_else(short)?,
            ),
            Channels::Rgb => DynamicImage::ImageRgb8(
                ::image::ImageBuffer::from_raw(width, height, data).ok_or_else(short)?,
            ),
            Channels::Rgba => DynamicImage::ImageRgba8(
                ::image::ImageBuffer::from_raw(width, height, data).ok_or_else(short)?,
            ),
        },
        Samples::U16 { channels, data } => match channels {
            Channels::Gray => DynamicImage::ImageLuma16(
                ::image::ImageBuffer::from_raw(width, height, data).ok_or_else(short)?,
            ),
            Channels::GrayAlpha => DynamicImage::ImageLumaA16(
                ::image::ImageBuffer::from_raw(width, height, data).ok_or_else(short)?,
            ),
            Channels::Rgb => DynamicImage::ImageRgb16(
                ::image::ImageBuffer::from_raw(width, height, data).ok_or_else(short)?,
            ),
            Channels::Rgba => DynamicImage::ImageRgba16(
                ::image::ImageBuffer::from_raw(width, height, data).ok_or_else(short)?,
            ),
        },
        Samples::F32 { channels, data } => match channels {
            Channels::Rgb => DynamicImage::ImageRgb32F(
                ::image::ImageBuffer::from_raw(width, height, data).ok_or_else(short)?,
            ),
            Channels::Rgba => DynamicImage::ImageRgba32F(
                ::image::ImageBuffer::from_raw(width, height, data).ok_or_else(short)?,
            ),
            gray => bail!("`image` has no floating-point {gray:?} layout to turn"),
        },
    })
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
