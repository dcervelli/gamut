//! JPEG, container and all.
//!
//! `ImageReader` gives back pixels and nothing else, while a JPEG from any
//! phone made in the last decade carries, in its container, an ICC profile
//! saying it is Display P3 rather than sRGB, an EXIF segment saying which
//! way up it goes, and may carry a gain map saying how far above SDR white
//! it went. The gain map sits at the end of the file and the reader that
//! finds it borrows a slice, so JPEG is the one format here that has to be
//! held whole.
//!
//! The orientation comes from the crate's own JPEG decoder, built by hand
//! rather than through `ImageReader`: the decoder keeps the EXIF segment it
//! passed on the way to the pixels, and `ImageReader` would throw it away.
//! It is applied, as WebP's is, so a photograph taken on its side arrives
//! upright — and the size `dimensions` reports is the size after the turn,
//! so the window opens in the shape the picture will arrive in.

mod gain_map;

use std::io::{BufReader, Cursor, Read, SeekFrom};

use anyhow::{Context, Result, bail};

use ::image::codecs::jpeg::JpegDecoder;
use ::image::metadata::Orientation;
use ::image::{DynamicImage, ImageDecoder};

use crate::image::{ColorSpace, DecodedImage};

use super::{MAX_DECODED_BYTES, Overrides, ReadSeek, dynamic, orient};

pub struct Jpeg;

impl super::Decoder for Jpeg {
    fn name(&self) -> &'static str {
        "jpeg"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["jpg", "jpeg", "jpe", "jfif"]
    }

    fn sniff(&self, header: &[u8]) -> bool {
        header.starts_with(b"\xff\xd8\xff")
    }

    fn dimensions(&self, source: &mut dyn ReadSeek) -> Result<Option<(u32, u32)>> {
        // An Ultra HDR JPEG is reconstructed onto its own base image, so the
        // size stated in the header is the size either way. A quarter turn
        // swaps it, exactly as `decode` will once the pixels are read.
        let mut decoder = JpegDecoder::new(BufReader::new(source)).context("reading the header")?;
        let (width, height) = decoder.dimensions();
        Ok(Some(orient::size(
            width,
            height,
            orientation(&mut decoder)?,
        )))
    }

    fn decode(&self, source: &mut dyn ReadSeek, overrides: Overrides) -> Result<DecodedImage> {
        // JPEG is the one format read whole (the gain-map reader borrows a
        // slice of the tail), so the length is bounded before the read: a
        // decoded JPEG cannot exceed its file, so the same ceiling that caps
        // the decoded image caps the bytes held. Without this a huge `.jpg`
        // is read entirely into memory before anything looks at it.
        let length = source.seek(SeekFrom::End(0)).context("reading the file")?;
        if length > MAX_DECODED_BYTES {
            bail!(
                "{:.1} GB is too large to read, over the {:.1} GB this build will hold",
                length as f64 / 1e9,
                MAX_DECODED_BYTES as f64 / 1e9,
            );
        }
        source
            .seek(SeekFrom::Start(0))
            .context("reading the file")?;
        let mut bytes = Vec::with_capacity(length as usize);
        source
            .take(length)
            .read_to_end(&mut bytes)
            .context("reading the file")?;
        decode(&bytes, overrides)
    }
}

/// JPEG, container and all, turned the way its EXIF says.
///
/// A file with a gain map comes back as linear light above SDR white; every
/// other JPEG comes back as `image` decodes it, with its ICC profile
/// believed.
pub(super) fn decode(bytes: &[u8], overrides: Overrides) -> Result<DecodedImage> {
    let mut decoder = open(bytes)?;
    let orientation = orientation(&mut decoder)?;
    Ok(orient::apply(
        stored(decoder, bytes, overrides)?,
        orientation,
    ))
}

/// The same, as the pixels are stored, whatever the EXIF says about the
/// way up. For the JPEG a raw carries of itself: the camera's orientation
/// is the raw's to apply, read from the raw's own header, and a preview
/// that carries the same tag would otherwise be turned twice.
pub(super) fn decode_stored(bytes: &[u8], overrides: Overrides) -> Result<DecodedImage> {
    stored(open(bytes)?, bytes, overrides)
}

/// The crate's decoder over the bytes, with the headers read and the size
/// ceiling set.
fn open(bytes: &[u8]) -> Result<JpegDecoder<Cursor<&[u8]>>> {
    let mut decoder = JpegDecoder::new(Cursor::new(bytes)).context("reading the header")?;
    decoder.set_limits(dynamic::limits())?;
    Ok(decoder)
}

/// The way the file asks to be turned. A file with no EXIF, or an EXIF
/// with no orientation in it, asks for nothing.
fn orientation<R: std::io::BufRead + std::io::Seek>(
    decoder: &mut JpegDecoder<R>,
) -> Result<Orientation> {
    decoder.orientation().context("reading the EXIF segment")
}

/// The picture as stored, decoded through `decoder` — or, where the
/// container holds a gain map and it is wanted, reconstructed from the
/// base image and the map, with `decoder` having served only to say which
/// way up the file goes.
fn stored(
    decoder: JpegDecoder<Cursor<&[u8]>>,
    bytes: &[u8],
    overrides: Overrides,
) -> Result<DecodedImage> {
    let container = gain_map::Container::open(bytes);

    // sRGB stays the answer for a JPEG that carries no profile, which is what
    // the format means in the absence of one.
    let color = container
        .as_ref()
        .and_then(|jpeg| jpeg.icc_profile())
        .map(|profile| crate::image::color::icc::color_space(&profile, ColorSpace::SRGB))
        .unwrap_or(ColorSpace::SRGB);

    if overrides.gain_map
        && let Some(container) = &container
        && let Some(image) = container.gain_mapped(color)?
    {
        return Ok(image);
    }

    dynamic::describe(
        DynamicImage::from_decoder(decoder)?,
        Some(::image::ImageFormat::Jpeg),
        color,
    )
}
