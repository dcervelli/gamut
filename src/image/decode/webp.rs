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
//! display-referred. There is no HDR path to preserve and no grayscale
//! encoding to keep one channel wide — a gray WebP is a gray RGB WebP.
//!
//! An animation's frames are patches: each is composited onto the canvas the
//! `ANIM` chunk describes, blended over what the last frame left and with the
//! rectangle the last frame asked to have cleared cleared, so a frame that is
//! a partial patch still arrives whole. `read_image` does that for the first
//! frame, which is what `decode` shows; `frames` walks the rest through
//! `read_frame`, which does the same for each. The container states the frame
//! count and the loop count outright, so `sequence` answers from the header.
//! The `ANIM` chunk also names a background color, which browsers ignore and
//! so does this: the canvas starts clear.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};

use ::image::metadata::Orientation;
use image_webp::{DecodingError, WebPDecoder};

use crate::image::sequence::{Frame, FrameSource, Loops, Sequence};
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
        Ok(Some(crate::image::orient::size(width, height, orientation)))
    }

    fn decode(
        &self,
        source: &mut dyn super::ReadSeek,
        _overrides: super::Overrides,
    ) -> Result<DecodedImage> {
        let mut opened = Opened::new(source)?;
        let mut data = opened.buffer()?;
        opened
            .decoder
            .read_image(&mut data)
            .context("decoding the image data")?;
        opened.describe(data)
    }

    fn sequence(&self, source: &mut dyn super::ReadSeek) -> Result<Sequence> {
        let decoder =
            WebPDecoder::new(BufReader::new(source)).context("reading the WebP container")?;
        if !decoder.is_animated() {
            return Ok(Sequence::Still);
        }
        Ok(Sequence::Animation {
            count: decoder.num_frames() as usize,
            loops: match decoder.loop_count() {
                image_webp::LoopCount::Forever => Loops::Forever,
                image_webp::LoopCount::Times(times) => Loops::from_count(times.get().into()),
            },
        })
    }

    /// Each frame is an `ANMF` chunk whose header gives its duration in
    /// milliseconds, so the chunks are walked end to end and every frame's
    /// bitstream is seeked past unread. The crate reads the same field, but
    /// only for the whole loop's length or on the way to decoding a frame.
    fn delays(&self, source: &mut dyn super::ReadSeek) -> Result<Option<Vec<Duration>>> {
        source.rewind()?;
        let mut reader = BufReader::new(source);
        let mut header = [0u8; 12];
        reader
            .read_exact(&mut header)
            .context("reading the WebP container")?;
        if !is_webp(&header) {
            bail!("not a WebP");
        }
        let mut delays = Vec::new();
        loop {
            let mut head = [0u8; 8];
            match reader.read_exact(&mut head) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(error) => return Err(error).context("reading the WebP chunks"),
            }
            let length = u32::from_le_bytes([head[4], head[5], head[6], head[7]]);
            // Each chunk is padded to an even length.
            let mut rest = i64::from(length) + i64::from(length & 1);
            if &head[..4] == b"ANMF" {
                // The frame's place and size, three bytes each, and then its
                // duration in three more.
                let mut frame = [0u8; 15];
                if (length as usize) < frame.len() {
                    bail!("an ANMF chunk of {length} bytes");
                }
                reader
                    .read_exact(&mut frame)
                    .context("reading an ANMF chunk")?;
                rest -= frame.len() as i64;
                let milliseconds = u32::from_le_bytes([frame[12], frame[13], frame[14], 0]);
                delays.push(Duration::from_millis(u64::from(milliseconds)));
            }
            reader
                .seek_relative(rest)
                .context("reading the WebP chunks")?;
        }
        Ok(Some(delays))
    }

    fn frames(
        &self,
        source: BufReader<File>,
        _overrides: super::Overrides,
    ) -> Result<Box<dyn FrameSource>> {
        let opened = Opened::new(source.into_inner())?;
        // `read_frame` and `reset_animation` are not questions a still can
        // be asked: the decoder asserts rather than answers.
        if !opened.decoder.is_animated() {
            bail!("the WebP is not an animation");
        }
        Ok(Box::new(WebpFrames { opened }))
    }
}

/// A WebP with its container read: the decoder, positioned to read pixels,
/// and everything the chunks around them said.
struct Opened<R: Read + Seek> {
    decoder: WebPDecoder<BufReader<R>>,
    width: u32,
    height: u32,
    channels: Channels,
    color: ColorSpace,
    orientation: Orientation,
}

impl<R: Read + Seek> Opened<R> {
    fn new(mut source: R) -> Result<Self> {
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

        Ok(Self {
            decoder,
            width,
            height,
            channels,
            color,
            orientation,
        })
    }

    /// A buffer the size the decoder writes one picture into.
    fn buffer(&self) -> Result<Vec<u8>> {
        let size = self.decoder.output_buffer_size().ok_or_else(|| {
            anyhow!(
                "{}x{} is more than this machine can address",
                self.width,
                self.height
            )
        })?;
        Ok(vec![0u8; size])
    }

    /// One picture the decoder wrote, described and turned the right way
    /// up. HEIF's rotation lives in the container and `libheif` applies it;
    /// WebP's lives in a metadata chunk, and applying it is this decoder's
    /// choice.
    fn describe(&self, data: Vec<u8>) -> Result<DecodedImage> {
        // WebP's alpha is straight, in both bitstreams and in the blending
        // the animation chunks describe.
        let image = DecodedImage::new(
            self.width,
            self.height,
            Samples::U8 {
                channels: self.channels,
                data,
            },
            self.color,
            AlphaMode::of(self.channels, false),
        );
        Ok(crate::image::orient::apply(image, self.orientation))
    }
}

/// An animated WebP's frames, each composited onto the canvas by the
/// decoder, which keeps the canvas between calls and can be sent back to the
/// start without the file being opened again.
struct WebpFrames {
    opened: Opened<File>,
}

impl FrameSource for WebpFrames {
    fn next(&mut self) -> Result<Option<Frame>> {
        let mut data = self.opened.buffer()?;
        let delay = match self.opened.decoder.read_frame(&mut data) {
            Ok(milliseconds) => Duration::from_millis(u64::from(milliseconds)),
            Err(DecodingError::NoMoreFrames) => return Ok(None),
            Err(error) => return Err(error).context("decoding a WebP frame"),
        };
        Ok(Some(Frame {
            image: self.opened.describe(data)?,
            delay,
        }))
    }

    fn rewind(&mut self) -> Result<()> {
        self.opened.decoder.reset_animation();
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::decode::Decoder;

    /// An animated WebP's delays are read in order from its `ANMF` chunks'
    /// headers, in milliseconds, and each frame's bitstream — odd lengths
    /// padded — is passed over.
    #[test]
    fn an_animated_webp_states_each_frame_delay() {
        let chunk = |kind: &[u8; 4], body: &[u8]| {
            let mut chunk = kind.to_vec();
            chunk.extend_from_slice(&(body.len() as u32).to_le_bytes());
            chunk.extend_from_slice(body);
            if body.len() % 2 == 1 {
                chunk.push(0);
            }
            chunk
        };
        let frame = |milliseconds: u32| {
            let mut body = vec![0u8; 16];
            body[12..15].copy_from_slice(&milliseconds.to_le_bytes()[..3]);
            body.extend(chunk(b"VP8L", &[0; 5]));
            chunk(b"ANMF", &body)
        };
        let mut file = b"RIFF\0\0\0\0WEBP".to_vec();
        file.extend(chunk(b"VP8X", &[0; 10]));
        file.extend(chunk(b"ANIM", &[0; 6]));
        file.extend(frame(40));
        file.extend(frame(250));
        file.extend(frame(70_000));
        let stated = Webp
            .delays(&mut std::io::Cursor::new(file))
            .unwrap()
            .unwrap();
        assert_eq!(stated, [40, 250, 70_000].map(Duration::from_millis));
    }

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
}
