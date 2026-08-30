//! JPEG, container and all.
//!
//! `ImageReader` gives back pixels and nothing else, while a JPEG from any
//! phone made in the last decade carries, in its container, an ICC profile
//! saying it is Display P3 rather than sRGB, and may carry a gain map saying
//! how far above SDR white it went. The gain map sits at the end of the file
//! and the reader that finds it borrows a slice, so JPEG is the one format
//! here that has to be held whole.
//!
//! EXIF orientation is not applied: the tag would need a container pass of
//! its own to reach, and the WebP decoder is the only one that has one.

mod gain_map;

use anyhow::{Context, Result};

use crate::image::{ColorSpace, DecodedImage};

use super::{Overrides, ReadSeek, dynamic};

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
        // size stated in the header is the size either way.
        dynamic::dimensions(source)
    }

    fn decode(&self, source: &mut dyn ReadSeek, overrides: Overrides) -> Result<DecodedImage> {
        let mut bytes = Vec::new();
        source.read_to_end(&mut bytes).context("reading the file")?;
        decode(&bytes, overrides)
    }
}

/// JPEG, container and all.
///
/// A file with a gain map comes back as linear light above SDR white; every
/// other JPEG comes back exactly as it did before, but with its ICC profile
/// believed.
fn decode(bytes: &[u8], overrides: Overrides) -> Result<DecodedImage> {
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
        dynamic::decode_as(bytes, ::image::ImageFormat::Jpeg)?,
        Some(::image::ImageFormat::Jpeg),
        color,
    )
}
