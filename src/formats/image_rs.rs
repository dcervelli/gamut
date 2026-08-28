//! PNG, JPEG and TIFF, via the `image` crate.

use std::io::Cursor;

use anyhow::Result;

use super::{DecodedImage, Decoder};

pub struct ImageRs;

impl Decoder for ImageRs {
    fn name(&self) -> &'static str {
        "png/jpeg/tiff"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["png", "jpg", "jpeg", "jpe", "jfif", "tif", "tiff"]
    }

    fn sniff(&self, header: &[u8]) -> bool {
        header.starts_with(b"\x89PNG\r\n\x1a\n")
            || header.starts_with(b"\xff\xd8\xff")
            || header.starts_with(b"II\x2a\x00")
            || header.starts_with(b"MM\x00\x2a")
    }

    fn decode(&self, bytes: &[u8]) -> Result<DecodedImage> {
        let decoded = image::ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()?
            .decode()?;

        // Normalises 16-bit TIFF, palettes, greyscale and CMYK down to the
        // RGBA8 the texture upload path expects.
        let rgba = decoded.to_rgba8();
        Ok(DecodedImage {
            width: rgba.width(),
            height: rgba.height(),
            rgba: rgba.into_raw(),
        })
    }
}
