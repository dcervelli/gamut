//! PNG, container and all.
//!
//! `ImageReader` gives back pixels and nothing else, while a PNG can carry in
//! its chunks the thing that says what those pixels mean: a `cICP` chunk,
//! which is how a PNG states that it is BT.2100 PQ or HLG — the whole of what
//! makes a PNG an HDR one — or the same ICC profile a JPEG can. The chunks
//! all precede the pixels, so they are read on the way past and the file is
//! then rewound; a PNG keeps the streaming every other format here gets.
//!
//! Two older chunks say the same thing in a weaker vocabulary, and are read
//! where the file carries neither of the two above: `gAMA`, the encoding
//! exponent, which is how a renderer or a game pipeline marks a PNG as
//! linear; and `cHRM`, the primaries as chromaticities, matched against the
//! four this program can name. An `sRGB` chunk, which the specification has
//! win over both, means what the absence of any chunk means.
//!
//! An `eXIf` chunk can carry an orientation, as a JPEG's EXIF does, and it
//! is applied the same way — to the still, and to every frame of an
//! animation.
//!
//! An animated PNG keeps its frame count and loop count in the same run of
//! chunks, so `sequence` reads them on the same pass. `decode` shows the
//! default image — the still that a reader with no notion of animation sees
//! — and `frames` walks the animation, each frame composited by `image` onto
//! the canvas with the file's disposal and blending applied. The crate does
//! that at eight bits only, so a 16-bit animation is a still here.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::time::Duration;

use anyhow::{Context, Result, bail};

use ::image::codecs::png::PngDecoder;
use ::image::metadata::Orientation;
use ::image::{AnimationDecoder, ImageFormat};

use crate::image::sequence::{Frame, FrameSource, Loops, Sequence};
use crate::image::{ColorSpace, DecodedImage, Primaries, Transfer};

use super::{Overrides, ReadSeek, dynamic};
use crate::image::orient;

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
        // Of the chunks read on the way past, only the orientation changes
        // the size: a quarter turn swaps it, as `decode` will.
        let orientation = header(&mut *source).orientation;
        source
            .seek(SeekFrom::Start(0))
            .context("reading the file")?;
        let (width, height) =
            ::image::ImageReader::with_format(BufReader::new(source), ImageFormat::Png)
                .into_dimensions()?;
        Ok(Some(orient::size(width, height, orientation)))
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

    /// Each frame's delay is in the `fcTL` chunk ahead of its data, one per
    /// frame and in order, so the chunks are walked end to end and every
    /// other chunk's contents are seeked past unread.
    fn delays(&self, source: &mut dyn ReadSeek) -> Result<Option<Vec<Duration>>> {
        source.rewind()?;
        let mut reader = BufReader::new(source);
        let mut signature = [0u8; 8];
        reader
            .read_exact(&mut signature)
            .context("reading the PNG signature")?;
        if signature != *b"\x89PNG\r\n\x1a\n" {
            bail!("not a PNG");
        }
        let mut delays = Vec::new();
        loop {
            let mut head = [0u8; 8];
            reader
                .read_exact(&mut head)
                .context("reading the PNG chunks")?;
            let length = u32::from_be_bytes([head[0], head[1], head[2], head[3]]);
            // Past the contents, and the CRC after them.
            let mut rest = i64::from(length) + 4;
            match &head[4..8] {
                b"IEND" => break,
                b"fcTL" => {
                    let mut control = [0u8; 26];
                    if (length as usize) < control.len() {
                        bail!("an fcTL chunk of {length} bytes");
                    }
                    reader
                        .read_exact(&mut control)
                        .context("reading an fcTL chunk")?;
                    rest -= control.len() as i64;
                    delays.push(apng_delay(
                        u16::from_be_bytes([control[20], control[21]]),
                        u16::from_be_bytes([control[22], control[23]]),
                    ));
                }
                _ => {}
            }
            reader
                .seek_relative(rest)
                .context("reading the PNG chunks")?;
        }
        Ok(Some(delays))
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
            orientation: stated.orientation,
            frames: None,
        };
        frames.rewind()?;
        Ok(Box::new(frames))
    }
}

/// An `fcTL` chunk's delay, a fraction of a second, the way `image` reads
/// it for a frame: a denominator of zero means hundredths, as the
/// specification says, and the fraction is taken to the microsecond,
/// rounded down.
fn apng_delay(numerator: u16, denominator: u16) -> Duration {
    let denominator = match denominator {
        0 => 100,
        stated => u64::from(stated),
    };
    Duration::from_micros(u64::from(numerator) * 1_000_000 / denominator)
}

/// An animated PNG's frames, composited by `image` onto the canvas.
///
/// The same arrangement as a GIF's: the crate's iterator owns the decoder,
/// so a rewind is a fresh decoder over the file seeked back to its start.
struct PngFrames {
    file: File,
    color: ColorSpace,
    orientation: Orientation,
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
                let mut frame = dynamic::frame(frame, ImageFormat::Png, self.color)?;
                frame.image = orient::apply(frame.image, self.orientation);
                Ok(Some(frame))
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
    let stated = header(&mut *source);
    source
        .seek(SeekFrom::Start(start))
        .context("reading the file")?;

    let mut reader =
        ::image::ImageReader::with_format(BufReader::new(source), ::image::ImageFormat::Png);
    dynamic::limit(&mut reader);
    let decoded = reader.decode()?;
    let image = dynamic::describe(
        decoded,
        Some(::image::ImageFormat::Png),
        stated.color.unwrap_or(ColorSpace::SRGB),
    )?;
    Ok(orient::apply(image, stated.orientation))
}

/// What the chunks before the pixels say: the color, the orientation, and
/// the animation.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Header {
    /// `None` where the file says nothing about its color that this program
    /// can act on, and so means sRGB, which is what the format has always
    /// meant.
    color: Option<ColorSpace>,
    /// The `eXIf` chunk's orientation, where there is one.
    orientation: Orientation,
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
/// curves with a table. `sRGB` comes next, then `gAMA` and `cHRM`, which is
/// the order the specification gives.
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
            orientation: Orientation::NoTransforms,
            animation: None,
        };
    };
    let info = reader.info();

    let color = if let Some(points) = info.coding_independent_code_points {
        Some(crate::image::color::cicp::color_space(
            points.color_primaries,
            points.transfer_function,
        ))
    } else if let Some(profile) = info.icc_profile.as_ref() {
        Some(crate::image::color::icc::color_space(
            profile,
            ColorSpace::SRGB,
        ))
    } else if info.srgb.is_some() {
        None
    } else {
        // `gama_chunk` and `chrm_chunk` rather than `source_gamma` and
        // `source_chromaticities`: the latter are filled in for an `sRGB`
        // chunk too, which has just been answered.
        let transfer = info.gama_chunk.map(|gamma| transfer(gamma.into_value()));
        let primaries = info.chrm_chunk.and_then(|chrm| {
            let xy =
                |(x, y): (::png::ScaledFloat, ::png::ScaledFloat)| (x.into_value(), y.into_value());
            Primaries::from_chromaticities(xy(chrm.red), xy(chrm.green), xy(chrm.blue))
        });
        (transfer.is_some() || primaries.is_some()).then(|| ColorSpace {
            transfer: transfer.unwrap_or(Transfer::Srgb),
            primaries: primaries.unwrap_or(Primaries::Bt709),
        })
    };
    let orientation = info
        .exif_metadata
        .as_deref()
        .and_then(Orientation::from_exif_chunk)
        .unwrap_or(Orientation::NoTransforms);
    let animation = info.animation_control().map(|control| Animation {
        frames: control.num_frames,
        plays: control.num_plays,
        playable: info.bit_depth as u8 <= 8,
    });
    Header {
        color,
        orientation,
        animation,
    }
}

/// What a `gAMA` chunk's value means. The chunk holds the encoding
/// exponent — 1/2.2 for a conventional picture, 1 for linear light — so
/// the transfer function is its reciprocal. Two values are named rather
/// than taken as a power law: 1.0, which is [`Transfer::Linear`] and the
/// reason a renderer writes the chunk at all; and 0.45455, which the
/// specification gives as the value for an sRGB picture, and which every
/// encoder writes beside sRGB data — the sRGB curve is what such a file
/// holds, and a power of 2.2 would put its shadows visibly wrong.
fn transfer(gamma: f32) -> Transfer {
    if !(gamma.is_finite() && gamma > 0.0) {
        return Transfer::Srgb;
    }
    if (gamma - 1.0).abs() < 0.001 {
        Transfer::Linear
    } else if (gamma - 0.45455).abs() < 0.001 {
        Transfer::Srgb
    } else {
        Transfer::Gamma(1.0 / gamma)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::decode::Decoder;
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

        // A PNG with neither is left to its older chunks. ImageMagick writes
        // a `cHRM` naming sRGB's own primaries on every file, and that is
        // read as saying sRGB rather than as saying nothing.
        assert_eq!(color("test_images/png-rgb8.png"), Some(ColorSpace::SRGB));
    }

    /// `gAMA` holds the encoding exponent, and the two values encoders
    /// actually write are named rather than turned into a power law: a
    /// linear file is linear, and the specification's own value for sRGB
    /// is sRGB, since a power of 2.2 in place of the sRGB curve would put
    /// the shadows visibly wrong.
    #[test]
    fn gamma_is_read_as_the_curve_it_stands_for() {
        assert_eq!(transfer(1.0), Transfer::Linear);
        assert_eq!(transfer(0.45455), Transfer::Srgb);
        assert_eq!(transfer(0.5), Transfer::Gamma(2.0));
        assert_eq!(transfer(0.0), Transfer::Srgb);
        assert_eq!(transfer(f32::NAN), Transfer::Srgb);
    }

    /// An APNG's delays are read in order from its `fcTL` chunks, as a
    /// fraction of a second whose denominator of zero means hundredths, and
    /// everything between them is passed over.
    #[test]
    fn an_apng_states_each_frame_delay() {
        let chunk = |kind: &[u8; 4], body: &[u8]| {
            let mut chunk = (body.len() as u32).to_be_bytes().to_vec();
            chunk.extend_from_slice(kind);
            chunk.extend_from_slice(body);
            chunk.extend_from_slice(&[0; 4]);
            chunk
        };
        let control = |numerator: u16, denominator: u16| {
            let mut body = [0u8; 26];
            body[20..22].copy_from_slice(&numerator.to_be_bytes());
            body[22..24].copy_from_slice(&denominator.to_be_bytes());
            chunk(b"fcTL", &body)
        };
        let mut file = b"\x89PNG\r\n\x1a\n".to_vec();
        file.extend(chunk(b"IHDR", &[0; 13]));
        file.extend(control(1, 10));
        file.extend(chunk(b"IDAT", &[0; 7]));
        file.extend(control(3, 0));
        file.extend(chunk(b"fdAT", &[0; 9]));
        file.extend(control(1, 3));
        file.extend(chunk(b"fdAT", &[0; 9]));
        file.extend(chunk(b"IEND", &[]));
        let stated = Png
            .delays(&mut std::io::Cursor::new(file))
            .unwrap()
            .unwrap();
        assert_eq!(
            stated,
            [
                Duration::from_millis(100),
                Duration::from_millis(30),
                Duration::from_micros(333_333),
            ]
        );
    }

    /// A still PNG has no `acTL`, and says so rather than claiming one frame.
    #[test]
    fn a_still_png_is_not_an_animation() {
        let plain = std::fs::File::open("test_images/png-rgb8.png").unwrap();
        assert_eq!(header(plain).animation, None);
    }
}
