//! PNG, container and all.
//!
//! `ImageReader` gives back pixels and nothing else, while a PNG can carry in
//! its chunks the thing that says what those pixels mean: a `cICP` chunk,
//! which is how a PNG states that it is BT.2100 PQ or HLG — the whole of what
//! makes a PNG an HDR one — or the same ICC profile a JPEG can. The chunks
//! all precede the pixels, so they are read on the way past and the file is
//! then rewound; a PNG keeps the streaming every other format here gets.
//!
//! An animated PNG keeps its frame count and loop count in the same run of
//! chunks, so `sequence` reads them on the same pass. `decode` shows the
//! default image — the still that a reader with no notion of animation sees
//! — and `frames` walks the animation, each frame composited by `image` onto
//! the canvas with the file's disposal and blending applied. The crate does
//! that at eight bits only, so a 16-bit animation is a still here.

use std::fs::File;
use std::io::{BufReader, Seek, SeekFrom};

use anyhow::{Context, Result, bail};

use ::image::codecs::png::PngDecoder;
use ::image::{AnimationDecoder, ImageFormat};

use crate::image::sequence::{Frame, FrameSource, Loops, Sequence};
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

    fn sequence(&self, source: &mut dyn ReadSeek) -> Result<Sequence> {
        Ok(match header(&mut *source).animation {
            Some(animation) if animation.frames > 1 && animation.playable => Sequence::Animation {
                count: animation.frames as usize,
                loops: Loops::from_count(animation.plays),
            },
            _ => Sequence::Still,
        })
    }

    fn frames(
        &self,
        source: BufReader<File>,
        _overrides: Overrides,
    ) -> Result<Box<dyn FrameSource>> {
        let mut file = source.into_inner();
        let stated = header(&mut file);
        if !stated.animation.is_some_and(|animation| animation.playable) {
            bail!("the PNG is not an animation this build can play");
        }
        let mut frames = PngFrames {
            file,
            color: stated.color.unwrap_or(ColorSpace::SRGB),
            frames: None,
        };
        frames.rewind()?;
        Ok(Box::new(frames))
    }
}

/// An animated PNG's frames, composited by `image` onto the canvas.
///
/// The same arrangement as a GIF's: the crate's iterator owns the decoder,
/// so a rewind is a fresh decoder over the file seeked back to its start.
struct PngFrames {
    file: File,
    color: ColorSpace,
    frames: Option<::image::Frames<'static>>,
}

impl FrameSource for PngFrames {
    fn next(&mut self) -> Result<Option<Frame>> {
        let Some(frames) = self.frames.as_mut() else {
            return Ok(None);
        };
        match frames.next() {
            None => Ok(None),
            Some(frame) => {
                let frame = frame.context("decoding a PNG frame")?;
                Ok(Some(dynamic::frame(frame, ImageFormat::Png, self.color)?))
            }
        }
    }

    fn rewind(&mut self) -> Result<()> {
        self.frames = None;
        self.file.seek(SeekFrom::Start(0))?;
        let handle = self.file.try_clone().context("reopening the PNG")?;
        let decoder = PngDecoder::with_limits(BufReader::new(handle), dynamic::limits())
            .context("reading the PNG header")?;
        let animation = decoder.apng().context("reading the PNG animation")?;
        self.frames = Some(animation.into_frames());
        Ok(())
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
    let color = header(&mut *source).color.unwrap_or(ColorSpace::SRGB);
    source
        .seek(SeekFrom::Start(start))
        .context("reading the file")?;

    let mut reader =
        ::image::ImageReader::with_format(BufReader::new(source), ::image::ImageFormat::Png);
    dynamic::limit(&mut reader);
    let decoded = reader.decode()?;
    dynamic::describe(decoded, Some(::image::ImageFormat::Png), color)
}

/// What the chunks before the pixels say: the color, and the animation.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Header {
    /// `None` where the file carries neither `cICP` nor `iCCP`, and so means
    /// sRGB, which is what the format has always meant.
    color: Option<ColorSpace>,
    /// The `acTL` chunk, where there is one.
    animation: Option<Animation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Animation {
    frames: u32,
    /// Zero for ever, as every animated format says it.
    plays: u32,
    /// Whether the frames can be read here: `image` composites at eight
    /// bits and refuses a deeper file.
    playable: bool,
}

/// What a PNG says about itself in the chunks up to the first `IDAT`, and
/// no further.
///
/// `cICP` is preferred over `iCCP` where a file carries both, for the same
/// reason HEIF prefers its `nclx` box: code points name a transfer function
/// this program models exactly, while a profile can only approximate the HDR
/// curves with a table.
///
/// `read_info` rather than `read_header_info`: the latter stops at `IHDR`,
/// before any of these chunks has been seen, and reports them all absent.
/// A header that will not parse says nothing rather than failing: the
/// decode that follows will say what is wrong, with the pixels in hand.
fn header(source: impl std::io::Read + Seek) -> Header {
    let decoder = ::png::Decoder::new(BufReader::new(source));
    let Ok(reader) = decoder.read_info() else {
        return Header {
            color: None,
            animation: None,
        };
    };
    let info = reader.info();

    let color = if let Some(points) = info.coding_independent_code_points {
        Some(crate::image::color::cicp::color_space(
            points.color_primaries,
            points.transfer_function,
        ))
    } else {
        info.icc_profile
            .as_ref()
            .map(|profile| crate::image::color::icc::color_space(profile, ColorSpace::SRGB))
    };
    let animation = info.animation_control().map(|control| Animation {
        frames: control.num_frames,
        plays: control.num_plays,
        playable: info.bit_depth as u8 <= 8,
    });
    Header { color, animation }
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
        let color = |file: &str| header(std::fs::File::open(file).unwrap()).color;
        let tagged = color("test_images/png-cicp-pq.png").expect("cICP is read");
        assert_eq!(tagged.transfer, Transfer::Pq);
        assert_eq!(tagged.primaries, Primaries::Bt2020);

        // And a profile alone still answers.
        let tagged = color("test_images/png-icc-p3.png").expect("iCCP is read");
        assert_eq!(tagged.primaries, Primaries::DisplayP3);

        // A PNG with neither says nothing, and the caller supplies sRGB.
        assert_eq!(color("test_images/png-rgb8.png"), None);
    }

    /// A still PNG has no `acTL`, and says so rather than claiming one frame.
    #[test]
    fn a_still_png_is_not_an_animation() {
        let plain = std::fs::File::open("test_images/png-rgb8.png").unwrap();
        assert_eq!(header(plain).animation, None);
    }
}
