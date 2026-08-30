//! The formats the `image` crate reads whose containers have nothing to add:
//! GIF, Radiance HDR and OpenEXR. PNG and JPEG have their own modules, since
//! both carry things in their containers that `ImageReader` throws away.
//!
//! GIF takes the plain route, because its container has nothing to say that
//! this program could act on: no profile, no code points, no orientation, and
//! a palette of sRGB bytes by definition. What it does have is animation, and
//! the crate's decoder reads the first frame — composited onto the logical
//! screen the file declares, so a first frame stored as a patch at an offset
//! still arrives whole. The frames after it are not shown, for the same
//! reason an animated WebP's are not: nothing downstream of here has a clock.
//! Every GIF comes back RGBA, whatever its palette holds, because that is the
//! one layout the crate's decoder produces.

use std::io::BufReader;

use anyhow::Result;

use crate::image::{ColorSpace, DecodedImage};

use super::{Overrides, ReadSeek, dynamic};

pub struct ImageRs;

impl super::Decoder for ImageRs {
    fn name(&self) -> &'static str {
        "gif/hdr/exr"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["gif", "hdr", "exr"]
    }

    fn sniff(&self, header: &[u8]) -> bool {
        // Both GIF versions; the four bytes after `GIF` are `87a` or `89a`,
        // and only the first three are a signature.
        header.starts_with(b"GIF87a")
            || header.starts_with(b"GIF89a")
            || header.starts_with(b"#?RADIANCE")
            || header.starts_with(b"#?RGBE")
            || header.starts_with(b"\x76\x2f\x31\x01")
    }

    fn dimensions(&self, source: &mut dyn ReadSeek) -> Result<Option<(u32, u32)>> {
        dynamic::dimensions(source)
    }

    fn decode(&self, source: &mut dyn ReadSeek, _overrides: Overrides) -> Result<DecodedImage> {
        let mut reader = ::image::ImageReader::new(BufReader::new(source)).with_guessed_format()?;
        dynamic::limit(&mut reader);

        let format = reader.format();
        let decoded = reader.decode()?;
        dynamic::describe(decoded, format, ColorSpace::SRGB)
    }
}
