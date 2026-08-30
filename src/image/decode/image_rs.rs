//! Everything the `image` crate handles: PNG, JPEG, TIFF, Radiance HDR and
//! OpenEXR.
//!
//! The work here is not decoding — the crate does that — but deciding what the
//! decoded numbers *mean*, and handing them on without flattening 16-bit or
//! floating-point data down to bytes.
//!
//! JPEG and PNG take a longer route than the rest. `ImageReader` gives back
//! pixels and nothing else, while both formats can carry, in their
//! containers, the thing that says what those pixels mean:
//!
//! - a JPEG from any phone made in the last decade has an ICC profile saying
//!   it is Display P3 rather than sRGB, and may have a gain map saying how far
//!   above SDR white it went;
//! - a PNG can carry a `cICP` chunk, which is how a PNG states that it is
//!   BT.2100 PQ or HLG — the whole of what makes a PNG an HDR one — as well as
//!   the same ICC profile a JPEG can.
//!
//! All of it needs the container rather than the decoded image, so both
//! formats are read into memory whole and examined before they are decoded.

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

    fn dimensions(&self, source: &mut dyn super::ReadSeek) -> Result<Option<(u32, u32)>> {
        // The same guess `decode` makes, stopped at the header. Both of the
        // special paths below end up at the size stated there: an Ultra HDR
        // JPEG is reconstructed onto its own base image, and the PNG path
        // differs only in which chunks it reads on the way past.
        let reader = ::image::ImageReader::new(BufReader::new(source)).with_guessed_format()?;
        Ok(Some(reader.into_dimensions()?))
    }

    fn decode(
        &self,
        source: &mut dyn super::ReadSeek,
        overrides: Overrides,
    ) -> Result<DecodedImage> {
        match tagged_format(source)? {
            // The gain map sits at the end of the file and the reader that
            // finds it borrows a slice, so JPEG is the one format here that
            // has to be held whole.
            Some(Tagged::Jpeg) => {
                let mut bytes = Vec::new();
                source.read_to_end(&mut bytes).context("reading the file")?;
                return jpeg(&bytes, overrides);
            }
            // PNG keeps the streaming that every other format here gets: the
            // chunks that say what the numbers mean all precede the pixels,
            // so they are read on the way past and the file is then rewound.
            Some(Tagged::Png) => return png(source),
            None => {}
        }

        let mut reader = ::image::ImageReader::new(BufReader::new(source)).with_guessed_format()?;
        limit(&mut reader);

        let format = reader.format();
        let decoded = reader.decode()?;
        describe(decoded, format, ColorSpace::SRGB)
    }
}

/// The two formats whose containers have something to say about colour.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tagged {
    Jpeg,
    Png,
}

/// Which of them this is, judged from the leading bytes and without
/// disturbing where the caller is reading from.
fn tagged_format(mut source: &mut dyn super::ReadSeek) -> Result<Option<Tagged>> {
    let mut magic = [0u8; 8];
    let read = super::fill(&mut source, &mut magic).context("reading the file")?;
    source
        .seek(SeekFrom::Start(0))
        .context("reading the file")?;

    let magic = &magic[..read];
    Ok(if magic.starts_with(b"\xFF\xD8\xFF") {
        Some(Tagged::Jpeg)
    } else if magic.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(Tagged::Png)
    } else {
        None
    })
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

    describe(
        decode_as(bytes, ::image::ImageFormat::Jpeg)?,
        Some(::image::ImageFormat::Jpeg),
        color,
    )
}

/// PNG, container and all.
///
/// `source` must be positioned at the start of the PNG, which is not always
/// the start of a file: an ICO entry holds one at an offset, and reaches this
/// through a cursor over just those bytes.
pub(super) fn png(source: &mut dyn super::ReadSeek) -> Result<DecodedImage> {
    // The chunks that say what the numbers mean all precede the pixels, so
    // they are read on the way past and the source is then rewound.
    let start = source.stream_position().context("reading the file")?;
    let color = png_color_space(&mut *source).unwrap_or(ColorSpace::SRGB);
    source
        .seek(SeekFrom::Start(start))
        .context("reading the file")?;

    let mut reader =
        ::image::ImageReader::with_format(BufReader::new(source), ::image::ImageFormat::Png);
    limit(&mut reader);
    let decoded = reader.decode()?;
    describe(decoded, Some(::image::ImageFormat::Png), color)
}

/// What a PNG says about its own colour, read from the chunks up to the first
/// `IDAT` and no further.
///
/// `cICP` is preferred over `iCCP` where a file carries both, for the same
/// reason HEIF prefers its `nclx` box: code points name a transfer function
/// this program models exactly, while a profile can only approximate the HDR
/// curves with a table. `None` — a PNG carrying neither — leaves the caller
/// with sRGB, which is what the format has always meant.
///
/// `read_info` rather than `read_header_info`: the latter stops at `IHDR`,
/// before either chunk has been seen, and reports both as absent.
fn png_color_space(source: impl std::io::Read + Seek) -> Option<ColorSpace> {
    let decoder = ::png::Decoder::new(BufReader::new(source));
    let reader = decoder.read_info().ok()?;
    let info = reader.info();

    if let Some(points) = info.coding_independent_code_points {
        return Some(super::cicp::color_space(
            points.color_primaries,
            points.transfer_function,
        ));
    }
    let profile = info.icc_profile.as_ref()?;
    Some(super::icc::color_space(profile, ColorSpace::SRGB))
}

fn decode_as(bytes: &[u8], format: ::image::ImageFormat) -> Result<DynamicImage> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::{Primaries, Transfer};

    /// `cICP` wins over `iCCP`, because code points name the HDR curves
    /// exactly and a profile can only tabulate them. A file carrying both is
    /// what a careful HDR encoder writes, for the sake of readers that
    /// understand only one.
    #[test]
    fn code_points_are_preferred_over_a_profile() {
        let both = std::fs::File::open("test_images/png-cicp-pq.png").unwrap();
        let mut tagged = png_color_space(both).expect("cICP is read");
        assert_eq!(tagged.transfer, Transfer::Pq);
        assert_eq!(tagged.primaries, Primaries::Bt2020);

        // And a profile alone still answers.
        let profiled = std::fs::File::open("test_images/png-icc-p3.png").unwrap();
        tagged = png_color_space(profiled).expect("iCCP is read");
        assert_eq!(tagged.primaries, Primaries::DisplayP3);

        // A PNG with neither says nothing, and the caller supplies sRGB.
        let plain = std::fs::File::open("test_images/png-rgb8.png").unwrap();
        assert_eq!(png_color_space(plain), None);
    }
}
