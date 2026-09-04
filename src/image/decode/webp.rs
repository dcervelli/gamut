//! WebP: the RIFF container, both of its bitstreams, and what it says about
//! color.
//!
//! `image-webp` is a direct dependency rather than a feature of `image`,
//! because everything worth having here lives in the container beside the
//! pixels and `ImageReader` hands back only the pixels. A WebP can carry an
//! `ICCP` chunk saying it is Display P3, an `EXIF` chunk saying which way up
//! it goes, and an animation whose first frame is a patch composited onto a
//! canvas rather than a picture in its own right — none of which survives the
//! trip through `DynamicImage`.
//!
//! What the format cannot say is anything about depth or range: both
//! bitstreams are 8-bit, VP8 through YCbCr 4:2:0 and VP8L through an exact
//! RGBA, so the samples are always `U8` and the color is always
//! display-referred. There is no HDR path to preserve and no greyscale
//! encoding to keep one channel wide — a grey WebP is a grey RGB WebP.
//!
//! Animation is decoded as far as its first frame. `read_image` composites
//! that frame onto the canvas the `ANIM` chunk describes, so a file whose
//! first frame is a partial patch still arrives whole; the frames after it
//! are not shown, because nothing downstream of here has a clock.

use std::io::{BufReader, SeekFrom};

use anyhow::{Context, Result, anyhow, bail};

use ::image::DynamicImage;
use ::image::metadata::Orientation;
use image_webp::WebPDecoder;

use crate::image::{AlphaMode, Channels, ColorSpace, DecodedImage, Samples};

pub struct Webp;

impl super::Decoder for Webp {
    fn name(&self) -> &'static str {
        "webp"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["webp"]
    }

    fn sniff(&self, header: &[u8]) -> bool {
        is_webp(header)
    }

    fn dimensions(&self, source: &mut dyn super::ReadSeek) -> Result<Option<(u32, u32)>> {
        let mut decoder =
            WebPDecoder::new(BufReader::new(source)).context("reading the WebP container")?;
        let (width, height) = decoder.dimensions();
        // A quarter turn swaps them, exactly as `reorient` will once the
        // pixels are read. Reporting the stored size for a rotated file would
        // open the window in the wrong shape.
        let orientation = decoder
            .exif_metadata()
            .context("reading the EXIF chunk")?
            .as_deref()
            .and_then(Orientation::from_exif_chunk)
            .unwrap_or(Orientation::NoTransforms);
        Ok(Some(if quarter_turn(orientation) {
            (height, width)
        } else {
            (width, height)
        }))
    }

    fn decode(
        &self,
        source: &mut dyn super::ReadSeek,
        _overrides: super::Overrides,
    ) -> Result<DecodedImage> {
        // The length settles how much a metadata chunk may claim: a chunk
        // lives in the file, so it cannot be larger than the file, however
        // large its declared size says. Taken before the decoder borrows the
        // source.
        let length = source
            .seek(SeekFrom::End(0))
            .context("reading the WebP container")?;
        source
            .seek(SeekFrom::Start(0))
            .context("reading the WebP container")?;

        // The decoder reads the container in small pieces — chunk headers,
        // then a seek to each one — so it wants a buffer in front of it.
        let mut decoder =
            WebPDecoder::new(BufReader::new(source)).context("reading the WebP container")?;

        let (width, height) = decoder.dimensions();
        // Both bitstreams are 8-bit, and alpha is the only thing that varies:
        // an `ALPH` chunk beside a lossy frame, or the `alpha_is_used` bit in
        // a lossless one.
        let channels = if decoder.has_alpha() {
            Channels::Rgba
        } else {
            Channels::Rgb
        };
        super::check_decoded_size(width, height, channels.count(), 8)?;

        // The stock limit is `usize::MAX`; the decoder zeroes a chunk's
        // declared size before reading it, so a tiny file declaring a 4 GiB
        // `ICCP` chunk would otherwise allocate 4 GiB. Bound it by what the
        // file could hold or the pixels need, whichever is larger — never the
        // global ceiling, which a 30-byte file has no business reaching.
        let decoded = u64::from(width) * u64::from(height) * channels.count() as u64;
        let budget = length.max(decoded).min(super::MAX_DECODED_BYTES);
        decoder.set_memory_limit(usize::try_from(budget).unwrap_or(usize::MAX));

        // Read the metadata chunks before the pixels. Both seek away from
        // where the bitstream sits, and doing it first keeps the one
        // expensive read last.
        let color = match decoder.icc_profile().context("reading the ICCP chunk")? {
            Some(profile) => crate::image::color::icc::color_space(&profile, ColorSpace::SRGB),
            None => ColorSpace::SRGB,
        };
        let orientation = decoder
            .exif_metadata()
            .context("reading the EXIF chunk")?
            .as_deref()
            .and_then(Orientation::from_exif_chunk)
            .unwrap_or(Orientation::NoTransforms);

        let size = decoder
            .output_buffer_size()
            .ok_or_else(|| anyhow!("{width}x{height} is more than this machine can address"))?;
        let mut data = vec![0u8; size];
        decoder
            .read_image(&mut data)
            .context("decoding the image data")?;

        let (data, width, height) = reorient(data, width, height, channels, orientation)?;

        // WebP's alpha is straight, in both bitstreams and in the blending
        // the animation chunks describe.
        Ok(DecodedImage::new(
            width,
            height,
            Samples::U8 { channels, data },
            color,
            AlphaMode::of(channels, false),
        ))
    }
}

/// Is this a RIFF file whose form type says WebP?
///
/// The four bytes between the two are the RIFF size, which says nothing about
/// the format and is skipped rather than checked: a truncated file still
/// deserves this decoder's error message rather than the registry's
/// "unsupported image format".
fn is_webp(header: &[u8]) -> bool {
    header.len() >= 12 && &header[..4] == b"RIFF" && &header[8..12] == b"WEBP"
}

/// Whether an orientation turns the image on its side, and so swaps its
/// width and height.
fn quarter_turn(orientation: Orientation) -> bool {
    matches!(
        orientation,
        Orientation::Rotate90
            | Orientation::Rotate270
            | Orientation::Rotate90FlipH
            | Orientation::Rotate270FlipH
    )
}

/// Applies the EXIF orientation, returning the buffer and the dimensions the
/// picture has once it is the right way up.
///
/// A quarter turn swaps width and height, which is why this hands back both
/// rather than transforming in place. The eight cases are `image`'s, which is
/// already a dependency and has them tested; what is not delegated is the
/// decision to apply them at all. HEIF's rotation lives in the container and
/// `libheif` applies it; WebP's lives in a metadata chunk, and applying it is
/// this decoder's choice.
fn reorient(
    data: Vec<u8>,
    width: u32,
    height: u32,
    channels: Channels,
    orientation: Orientation,
) -> Result<(Vec<u8>, u32, u32)> {
    if orientation == Orientation::NoTransforms {
        return Ok((data, width, height));
    }

    let short = || anyhow!("WebP decoded to fewer pixels than {width}x{height}");
    let mut image = match channels {
        Channels::Rgb => DynamicImage::ImageRgb8(
            ::image::RgbImage::from_raw(width, height, data).ok_or_else(short)?,
        ),
        Channels::Rgba => DynamicImage::ImageRgba8(
            ::image::RgbaImage::from_raw(width, height, data).ok_or_else(short)?,
        ),
        // Neither bitstream has a one-channel encoding, so this is not a
        // layout a WebP can arrive in.
        gray => bail!("a WebP decoded to {gray:?}"),
    };
    image.apply_orientation(orientation);

    let (width, height) = (image.width(), image.height());
    let data = match image {
        DynamicImage::ImageRgb8(buffer) => buffer.into_raw(),
        DynamicImage::ImageRgba8(buffer) => buffer.into_raw(),
        // `apply_orientation` moves pixels about; it does not convert them.
        other => bail!(
            "rotating a WebP changed its pixel layout to {:?}",
            other.color()
        ),
    };
    Ok((data, width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The form type is what makes a RIFF file ours. A WAV is a RIFF file
    /// too, and claiming it would take it away from the "unsupported format"
    /// message that actually explains itself.
    #[test]
    fn only_the_webp_form_of_riff_is_claimed() {
        assert!(is_webp(b"RIFF\x3c\x00\x00\x00WEBPVP8L"));
        assert!(is_webp(b"RIFF\x00\x00\x00\x00WEBPVP8X"));

        assert!(!is_webp(b"RIFF\x24\x00\x00\x00WAVEfmt "));
        assert!(!is_webp(b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0d"));
        // Long enough for the signature but not for the form type.
        assert!(!is_webp(b"RIFF\x3c\x00\x00\x00"));
        assert!(!is_webp(b""));
    }

    /// A quarter turn has to hand back swapped dimensions, or every later
    /// reader of the buffer walks off the end of a row.
    #[test]
    fn a_quarter_turn_swaps_the_dimensions() {
        // A 2x1 RGB image: red then green.
        let data = vec![255, 0, 0, 0, 255, 0];
        let (turned, width, height) =
            reorient(data.clone(), 2, 1, Channels::Rgb, Orientation::Rotate90).unwrap();
        assert_eq!((width, height), (1, 2));
        // Clockwise: the left pixel ends up on top.
        assert_eq!(turned, vec![255, 0, 0, 0, 255, 0]);

        let (flipped, width, height) =
            reorient(data.clone(), 2, 1, Channels::Rgb, Orientation::Rotate180).unwrap();
        assert_eq!((width, height), (2, 1));
        assert_eq!(flipped, vec![0, 255, 0, 255, 0, 0]);

        // And the common case costs nothing: the same buffer straight back.
        let (untouched, width, height) =
            reorient(data.clone(), 2, 1, Channels::Rgb, Orientation::NoTransforms).unwrap();
        assert_eq!((width, height), (2, 1));
        assert_eq!(untouched, data);
    }

    /// Alpha has to survive the turn as well as color.
    #[test]
    fn rotation_carries_the_alpha_channel() {
        let data = vec![1, 2, 3, 64, 5, 6, 7, 128];
        let (turned, width, height) =
            reorient(data, 2, 1, Channels::Rgba, Orientation::Rotate180).unwrap();
        assert_eq!((width, height), (2, 1));
        assert_eq!(turned, vec![5, 6, 7, 128, 1, 2, 3, 64]);
    }
}
