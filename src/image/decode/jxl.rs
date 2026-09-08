//! JPEG XL: the codestream, and the ISOBMFF container it can also arrive in.
//!
//! `jxl-oxide` is a pure-Rust decoder, so unlike HEIF this format costs the
//! package no shared library and the build no C toolchain. What it asks for
//! in return is that we do the describing: it renders into whatever color
//! encoding the file itself declares and hands back the numbers, leaving what
//! they mean to its caller — which is exactly the bargain the rest of this
//! module is written to.
//!
//! Depth is preserved rather than flattened. JPEG XL is float throughout
//! internally, but every file states the depth it was authored at, and that
//! is what decides the sample type here: an 8-bit photograph comes back `U8`,
//! a 10- or 16-bit one `U16`, and a file authored in floating point `F32`,
//! where values above 1.0 survive instead of clipping. Grayscale stays one
//! channel all the way to the GPU.
//!
//! Color is stated outright in the usual two ways, and both already have a
//! translation here: an enum encoding becomes CICP codes, and anything else
//! carries an ICC profile. So a PQ BT.2100 file and a Display P3 photograph
//! land in the right working space without a flag, the same way a HEIF does.
//!
//! Orientation is the container's business and `jxl-oxide` applies it while
//! rendering, so a rotated photograph arrives upright and the size reported
//! from the header is already the size it will arrive at.
//!
//! Two things the format can hold are deliberately not taken. A CMYK file is
//! refused rather than guessed at: separating it needs the output profile
//! this program does not have, and a wrong guess would look like a decode.
//! An animation shows its first keyframe, as an animated GIF or WebP does,
//! nothing downstream of here having a clock.

use std::io::SeekFrom;

use anyhow::{Context, Result, anyhow, bail};

use jxl_oxide::image::BitDepth;
use jxl_oxide::{AllocTracker, InitializeResult, JxlImage, PixelFormat};

use crate::image::{AlphaMode, Channels, ColorSpace, DecodedImage, Samples};

pub struct Jxl;

/// The bare codestream's two-byte signature.
const CODESTREAM: [u8; 2] = [0xff, 0x0a];

/// The container's signature box: a 12-byte box of type `JXL ` whose payload
/// is the same `\r\n\x87\n` guard JPEG 2000 uses to catch a mangled transfer.
const CONTAINER: [u8; 12] = [
    0x00, 0x00, 0x00, 0x0c, b'J', b'X', b'L', b' ', 0x0d, 0x0a, 0x87, 0x0a,
];

impl super::Decoder for Jxl {
    fn name(&self) -> &'static str {
        "jpeg xl"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["jxl"]
    }

    fn sniff(&self, header: &[u8]) -> bool {
        is_jxl(header)
    }

    fn dimensions(&self, source: &mut dyn super::ReadSeek) -> Result<Option<(u32, u32)>> {
        // The header alone, and not one frame past it. `width`/`height` are
        // the size after the orientation the container asks for, which is
        // also the size `decode` will hand back, so the window opens in the
        // shape the picture actually arrives in.
        let reading = Reading::header(source)?;
        Ok(Some((reading.image.width(), reading.image.height())))
    }

    fn decode(
        &self,
        source: &mut dyn super::ReadSeek,
        _overrides: super::Overrides,
    ) -> Result<DecodedImage> {
        let mut reading = Reading::header(source)?;
        let layout = Layout::of(&reading.image)?;
        // Before the frames are taken in, not after: an image too large to
        // hold is refused while it is still a claim in a header.
        super::check_decoded_size(
            layout.width,
            layout.height,
            layout.channels.count(),
            layout.bits,
        )?;
        reading.finish(source)?;
        let image = reading.image;

        if image.num_loaded_keyframes() == 0 {
            bail!("the JPEG XL file holds no complete frame");
        }
        let render = image.render_frame(0).map_err(|error| anyhow!("{error}"))?;

        let mut stream = render.stream();
        if (stream.width(), stream.height()) != (layout.width, layout.height) {
            // A frame whose rendered size disagrees with the header would
            // otherwise leave the tail of the buffer as it was allocated.
            bail!(
                "JPEG XL header says {}x{} but the rendered frame is {}x{}",
                layout.width,
                layout.height,
                stream.width(),
                stream.height(),
            );
        }
        if stream.channels() as usize != layout.channels.count() {
            bail!(
                "JPEG XL frame streams {} channels, expected {} for {}",
                stream.channels(),
                layout.channels.count(),
                layout.channels.label(),
            );
        }

        // `write_to_buffer` scales into the full range of whichever type it
        // is given — 0..255, 0..65535, or the nominal 0..1 of a float left
        // unclamped — which is what `Samples::full_scale` goes on to assume.
        let count = layout.width as usize * layout.height as usize * layout.channels.count();
        let channels = layout.channels;
        let samples = match layout.depth {
            Depth::U8 => {
                let mut data = vec![0u8; count];
                fill(&mut stream, &mut data)?;
                Samples::U8 { channels, data }
            }
            Depth::U16 => {
                let mut data = vec![0u16; count];
                fill(&mut stream, &mut data)?;
                Samples::U16 { channels, data }
            }
            Depth::F32 => {
                let mut data = vec![0f32; count];
                fill(&mut stream, &mut data)?;
                Samples::F32 { channels, data }
            }
        };

        Ok(DecodedImage::new(
            layout.width,
            layout.height,
            samples,
            color_space(&image),
            AlphaMode::of(channels, layout.premultiplied),
        ))
    }
}

/// Is this a JPEG XL, by its leading bytes?
///
/// Both spellings the format has: the bare codestream, and the ISOBMFF
/// container. The container's first box is `JXL ` rather than `ftyp`, so it
/// cannot be confused with the HEIF family that shares the box structure —
/// which the tests check, the two decoders being neighbors in the registry.
fn is_jxl(header: &[u8]) -> bool {
    header.starts_with(&CODESTREAM) || header.starts_with(&CONTAINER)
}

/// A file being fed to the decoder, and the bytes read past what it has
/// consumed so far.
///
/// Reading is split in two so that the size ceiling can be applied between
/// the halves: the header is parsed on its own, judged, and only then is the
/// rest of the file handed over. `jxl-oxide`'s own `read` does both at once,
/// which would mean deciding an image was too large after taking it in.
struct Reading {
    image: JxlImage,
    buffer: Vec<u8>,
    /// How much of `buffer` holds bytes read but not yet consumed.
    valid: usize,
}

impl Reading {
    /// Feeds the source until the image header is parsed, and no further.
    fn header(source: &mut dyn super::ReadSeek) -> Result<Self> {
        source
            .seek(SeekFrom::Start(0))
            .context("reading the JPEG XL file")?;

        // The ceiling `jxl-oxide` allocates under. Its own decision to refuse,
        // made against the same budget `check_decoded_size` spends, so that a
        // file lying about its size in the header runs out of room here rather
        // than in the OOM killer.
        let tracker = AllocTracker::with_limit(super::MAX_DECODED_BYTES as usize);
        let mut uninit = JxlImage::builder().alloc_tracker(tracker).build_uninit();

        let mut buffer = vec![0u8; 16 * 1024];
        let mut valid = 0usize;
        loop {
            if read_more(source, &mut buffer, &mut valid)? == 0 {
                bail!("the file ends before the JPEG XL header is complete");
            }
            let consumed = uninit
                .feed_bytes(&buffer[..valid])
                .map_err(|error| anyhow!("{error}"))?;
            buffer.copy_within(consumed..valid, 0);
            valid -= consumed;

            match uninit.try_init().map_err(|error| anyhow!("{error}"))? {
                InitializeResult::Initialized(image) => {
                    return Ok(Self {
                        image,
                        buffer,
                        valid,
                    });
                }
                InitializeResult::NeedMoreData(next) => uninit = next,
            }
        }
    }

    /// Feeds the rest of the file, so that every frame is there to render.
    fn finish(&mut self, source: &mut dyn super::ReadSeek) -> Result<()> {
        while !self.image.is_loading_done() {
            let count = read_more(source, &mut self.buffer, &mut self.valid)?;
            if count == 0 {
                break;
            }
            let consumed = self
                .image
                .feed_bytes(&self.buffer[..self.valid])
                .map_err(|error| anyhow!("{error}"))?;
            self.buffer.copy_within(consumed..self.valid, 0);
            self.valid -= consumed;
        }
        self.image.finalize().map_err(|error| anyhow!("{error}"))?;
        Ok(())
    }
}

/// Reads once into the free tail of `buffer`, growing it where the decoder
/// consumed nothing of what it was last given.
///
/// The growth is the point: a decoder that needs a whole box before it will
/// consume any of it leaves the buffer full, and reading into an empty tail
/// would report end-of-file for ever after.
fn read_more(
    source: &mut dyn super::ReadSeek,
    buffer: &mut Vec<u8>,
    valid: &mut usize,
) -> Result<usize> {
    if *valid == buffer.len() {
        buffer.resize(buffer.len() * 2, 0);
    }
    let count = source
        .read(&mut buffer[*valid..])
        .context("reading the JPEG XL file")?;
    *valid += count;
    Ok(count)
}

/// Drains the stream into `data`, which is exactly as long as the image.
fn fill<T: jxl_oxide::FrameBufferSample>(
    stream: &mut jxl_oxide::ImageStream<'_>,
    data: &mut [T],
) -> Result<()> {
    let written = stream.write_to_buffer(data);
    if written != data.len() {
        bail!(
            "JPEG XL frame gave {written} samples, expected {}",
            data.len()
        );
    }
    Ok(())
}

/// Which of the three sample types the file's own depth calls for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Depth {
    U8,
    U16,
    F32,
}

/// The sample type the file's own depth calls for, and the width to charge
/// the size ceiling for it.
///
/// The depth a file was authored at, not the one it decodes through: JPEG XL
/// is float from end to end internally, so taking the internal form at face
/// value would quadruple what an ordinary 8-bit photograph costs on the way
/// to the GPU. A file authored in floating point keeps `F32`, where the
/// highlights above 1.0 that are the point of it survive.
fn depth_of(bit_depth: BitDepth) -> Result<(Depth, u8)> {
    Ok(match bit_depth {
        BitDepth::FloatSample { .. } => (Depth::F32, 32),
        BitDepth::IntegerSample { bits_per_sample } => match bits_per_sample {
            0 => bail!("JPEG XL image reports zero bits per sample"),
            1..=8 => (Depth::U8, 8),
            9..=16 => (Depth::U16, 16),
            // Past 16 bits an integer buffer would have to drop the low end,
            // so the float one takes it instead. The format allows up to 31.
            _ => (Depth::F32, 32),
        },
    })
}

/// What the header says the decoded image will be, before a frame is read.
struct Layout {
    width: u32,
    height: u32,
    channels: Channels,
    depth: Depth,
    /// Bits per component once widened to `depth`, for the size ceiling.
    bits: u8,
    premultiplied: bool,
}

impl Layout {
    fn of(image: &JxlImage) -> Result<Self> {
        let channels = match image.pixel_format() {
            PixelFormat::Gray => Channels::Gray,
            PixelFormat::Graya => Channels::GrayAlpha,
            PixelFormat::Rgb => Channels::Rgb,
            PixelFormat::Rgba => Channels::Rgba,
            // Separating CMYK needs an output profile this program has none
            // of, and the four channels have nowhere to go in the pixel
            // model. Refused by name, rather than shown as a wrong picture.
            format @ (PixelFormat::Cmyk | PixelFormat::Cmyka) => {
                bail!("JPEG XL image is {format:?}, which this build cannot show")
            }
        };

        let (depth, bits) = depth_of(image.image_header().metadata.bit_depth)?;

        // JPEG XL states premultiplication per alpha channel rather than for
        // the file, so the alpha actually being rendered is the one asked.
        let premultiplied = image
            .image_header()
            .metadata
            .ec_info
            .iter()
            .find_map(|info| info.alpha_associated())
            .unwrap_or(false);

        Ok(Self {
            width: image.width(),
            height: image.height(),
            channels,
            depth,
            bits,
            premultiplied,
        })
    }
}

/// What the file says its numbers mean.
///
/// The same two ways HEIF says it, and the same order of preference. An enum
/// encoding is the usual case and the more precise, and `jxl-oxide` renders
/// it as CICP codes this program models exactly. A file carrying an ICC
/// profile instead — a custom set of primaries, or a profile from the camera
/// — says the same thing less directly, and is read only where there are no
/// codes to prefer.
///
/// Both are asked of the *rendered* encoding rather than the stored one, so
/// that what is described is what the buffer actually holds.
fn color_space(image: &JxlImage) -> ColorSpace {
    if let Some([primaries, transfer, _matrix, _range]) = image.rendered_cicp() {
        return crate::image::color::cicp::color_space(primaries, transfer);
    }
    match image.original_icc() {
        Some(profile) => crate::image::color::icc::color_space(profile, ColorSpace::SRGB),
        None => ColorSpace::SRGB,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::image::decode::Decoder;

    /// Both spellings of the format, and nothing else. The container shares
    /// its box structure with the HEIF family, so the two decoders sit next
    /// to each other in the registry and must not reach for each other's
    /// files.
    #[test]
    fn both_signatures_are_recognized_and_other_containers_are_not() {
        assert!(is_jxl(&[0xff, 0x0a, 0x00, 0x00]));
        assert!(is_jxl(&CONTAINER));

        assert!(!is_jxl(b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR"));
        assert!(!is_jxl(b"\xff\xd8\xff\xe0\x00\x10JFIF\x00\x01"));
        // A JPEG's `ff d8` is one byte away from the codestream's `ff 0a`.
        assert!(!is_jxl(&[0xff, 0xd8]));
        assert!(!is_jxl(b"\x00\x00\x00\x18ftypheic\x00\x00\x00\x00mif1heic"));
        // Too short to be either.
        assert!(!is_jxl(&[0xff]));
        assert!(!is_jxl(&[]));
    }

    /// The registry asks every decoder in turn, so a container claimed by
    /// both would be decided by the order of the list rather than by what it
    /// holds. Neither may claim the other's.
    #[test]
    fn the_heif_family_and_jpeg_xl_do_not_claim_each_other() {
        let heif = super::super::heif::Heif;
        let heic = b"\x00\x00\x00\x18ftypheic\x00\x00\x00\x00mif1heic";

        assert!(Jxl.sniff(&CONTAINER) && !heif.sniff(&CONTAINER));
        assert!(heif.sniff(heic) && !Jxl.sniff(heic));
    }

    /// The depth the file was authored at decides the sample type. Reading
    /// the internal float form instead would cost an ordinary photograph four
    /// times the texture memory for nothing, and flattening a float file to
    /// bytes would throw away the highlights it exists to carry.
    #[test]
    fn the_authored_depth_decides_the_sample_type() {
        let integer = |bits| BitDepth::IntegerSample {
            bits_per_sample: bits,
        };
        assert_eq!(depth_of(integer(8)).unwrap(), (Depth::U8, 8));
        assert_eq!(depth_of(integer(1)).unwrap(), (Depth::U8, 8));
        assert_eq!(depth_of(integer(10)).unwrap(), (Depth::U16, 16));
        assert_eq!(depth_of(integer(16)).unwrap(), (Depth::U16, 16));
        // Wider than a `u16` holds, so the float buffer takes it.
        assert_eq!(depth_of(integer(24)).unwrap(), (Depth::F32, 32));
        assert_eq!(
            depth_of(BitDepth::FloatSample {
                bits_per_sample: 32,
                exp_bits: 8,
            })
            .unwrap(),
            (Depth::F32, 32)
        );

        // A depth of nothing is a broken header, not a zero-byte sample.
        assert!(depth_of(integer(0)).is_err());
    }

    /// A file that ends mid-header is a plain error rather than a wait for
    /// bytes that will never come.
    #[test]
    fn a_truncated_header_is_refused() {
        let mut source = std::io::Cursor::new(vec![0xff, 0x0a]);
        let Err(error) = Reading::header(&mut source) else {
            panic!("two bytes are not a header");
        };
        assert!(format!("{error:#}").contains("ends before"), "{error:#}");
    }
}
