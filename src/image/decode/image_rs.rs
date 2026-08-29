//! Everything the `image` crate handles: PNG, JPEG, TIFF, Radiance HDR and
//! OpenEXR.
//!
//! The work here is not decoding — the crate does that — but deciding what the
//! decoded numbers *mean*, and handing them on without flattening 16-bit or
//! floating-point data down to bytes.
//!
//! JPEG takes a longer route than the rest. `ImageReader` gives back pixels
//! and nothing else, while a JPEG from any phone made in the last decade
//! carries two things in its container that change what those pixels mean: an
//! ICC profile saying they are Display P3 rather than sRGB, and possibly a
//! gain map saying how far above SDR white they went. Both need the file's
//! bytes rather than the decoded image, so JPEG is read into memory whole and
//! examined before it is decoded.

use std::io::{BufReader, Seek, SeekFrom};

use ::image::{DynamicImage, ImageFormat};

use anyhow::{Context, Result, anyhow};

use crate::image::{AlphaMode, Channels, ColorSpace, DecodedImage, Samples};

use super::{Overrides, ultra_hdr};

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

    fn decode(
        &self,
        source: &mut dyn super::ReadSeek,
        overrides: Overrides,
    ) -> Result<DecodedImage> {
        if is_jpeg(source)? {
            let mut bytes = Vec::new();
            source.read_to_end(&mut bytes).context("reading the file")?;
            return jpeg(&bytes, overrides);
        }

        let mut reader = ::image::ImageReader::new(BufReader::new(source)).with_guessed_format()?;
        limit(&mut reader);

        let format = reader.format();
        let decoded = reader.decode()?;
        describe(decoded, format, ColorSpace::SRGB)
    }
}

/// The leading bytes, without disturbing where the caller is reading from.
fn is_jpeg(mut source: &mut dyn super::ReadSeek) -> Result<bool> {
    let mut magic = [0u8; 3];
    let read = super::fill(&mut source, &mut magic).context("reading the file")?;
    source
        .seek(SeekFrom::Start(0))
        .context("reading the file")?;
    Ok(read == magic.len() && magic == [0xFF, 0xD8, 0xFF])
}

/// The crate defaults to a 512 MiB allocation ceiling, which a large EXR or
/// 16-bit scan passes easily. Match the ceiling the TIFF path uses so the two
/// behave the same way.
fn limit<R: std::io::BufRead + Seek>(reader: &mut ::image::ImageReader<R>) {
    let mut limits = ::image::Limits::default();
    limits.max_alloc = Some(super::MAX_DECODED_BYTES);
    reader.limits(limits);
}

/// JPEG, container and all.
///
/// A file with a gain map comes back as linear light above SDR white; every
/// other JPEG comes back exactly as it did before, but with its ICC profile
/// believed.
fn jpeg(bytes: &[u8], overrides: Overrides) -> Result<DecodedImage> {
    let container = ultra_hdr::Jpeg::open(bytes);

    // sRGB stays the answer for a JPEG that carries no profile, which is what
    // the format means in the absence of one.
    let color = container
        .as_ref()
        .and_then(|jpeg| jpeg.icc_profile())
        .map(|profile| super::icc::color_space(&profile, ColorSpace::SRGB))
        .unwrap_or(ColorSpace::SRGB);

    if overrides.gain_map
        && let Some(container) = &container
        && let Some(image) = container.gain_mapped(color)?
    {
        return Ok(image);
    }

    let mut reader =
        ::image::ImageReader::with_format(std::io::Cursor::new(bytes), ::image::ImageFormat::Jpeg);
    limit(&mut reader);
    let decoded = reader.decode()?;
    describe(decoded, Some(::image::ImageFormat::Jpeg), color)
}

/// Wraps a decoded buffer in what we know about it. `stated` is the colour
/// space the container claimed, used wherever the pixel type does not settle
/// the question by itself.
fn describe(
    decoded: DynamicImage,
    format: Option<ImageFormat>,
    stated: ColorSpace,
) -> Result<DecodedImage> {
    let (width, height) = (decoded.width(), decoded.height());
    let samples = into_samples(decoded)?;
    let color = color_space(format, &samples, stated);
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
