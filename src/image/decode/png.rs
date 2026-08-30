//! PNG, container and all.
//!
//! `ImageReader` gives back pixels and nothing else, while a PNG can carry in
//! its chunks the thing that says what those pixels mean: a `cICP` chunk,
//! which is how a PNG states that it is BT.2100 PQ or HLG — the whole of what
//! makes a PNG an HDR one — or the same ICC profile a JPEG can. The chunks
//! all precede the pixels, so they are read on the way past and the file is
//! then rewound; a PNG keeps the streaming every other format here gets.

use std::io::{BufReader, Seek, SeekFrom};

use anyhow::{Context, Result};

use crate::image::{ColorSpace, DecodedImage};

use super::{Overrides, ReadSeek, dynamic};

pub struct Png;

impl super::Decoder for Png {
    fn name(&self) -> &'static str {
        "png"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["png"]
    }

    fn sniff(&self, header: &[u8]) -> bool {
        header.starts_with(b"\x89PNG\r\n\x1a\n")
    }

    fn dimensions(&self, source: &mut dyn ReadSeek) -> Result<Option<(u32, u32)>> {
        // The chunks read on the way past change nothing about the size.
        dynamic::dimensions(source)
    }

    fn decode(&self, source: &mut dyn ReadSeek, _overrides: Overrides) -> Result<DecodedImage> {
        decode(source)
    }
}

/// PNG, container and all.
///
/// `source` must be positioned at the start of the PNG, which is not always
/// the start of a file: an ICO entry holds one at an offset, and reaches this
/// through a cursor over just those bytes.
pub(super) fn decode(source: &mut dyn ReadSeek) -> Result<DecodedImage> {
    // The chunks that say what the numbers mean all precede the pixels, so
    // they are read on the way past and the source is then rewound.
    let start = source.stream_position().context("reading the file")?;
    let color = color_space(&mut *source).unwrap_or(ColorSpace::SRGB);
    source
        .seek(SeekFrom::Start(start))
        .context("reading the file")?;

    let mut reader =
        ::image::ImageReader::with_format(BufReader::new(source), ::image::ImageFormat::Png);
    dynamic::limit(&mut reader);
    let decoded = reader.decode()?;
    dynamic::describe(decoded, Some(::image::ImageFormat::Png), color)
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
fn color_space(source: impl std::io::Read + Seek) -> Option<ColorSpace> {
    let decoder = ::png::Decoder::new(BufReader::new(source));
    let reader = decoder.read_info().ok()?;
    let info = reader.info();

    if let Some(points) = info.coding_independent_code_points {
        return Some(crate::image::color::cicp::color_space(
            points.color_primaries,
            points.transfer_function,
        ));
    }
    let profile = info.icc_profile.as_ref()?;
    Some(crate::image::color::icc::color_space(
        profile,
        ColorSpace::SRGB,
    ))
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
        let mut tagged = color_space(both).expect("cICP is read");
        assert_eq!(tagged.transfer, Transfer::Pq);
        assert_eq!(tagged.primaries, Primaries::Bt2020);

        // And a profile alone still answers.
        let profiled = std::fs::File::open("test_images/png-icc-p3.png").unwrap();
        tagged = color_space(profiled).expect("iCCP is read");
        assert_eq!(tagged.primaries, Primaries::DisplayP3);

        // A PNG with neither says nothing, and the caller supplies sRGB.
        let plain = std::fs::File::open("test_images/png-rgb8.png").unwrap();
        assert_eq!(color_space(plain), None);
    }
}
