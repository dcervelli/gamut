//! The formats the `image` crate reads whose containers have nothing to add:
//! GIF, Radiance HDR, OpenEXR, BMP and netpbm. PNG and JPEG have their own
//! modules, since both carry things in their containers that `ImageReader`
//! throws away.
//!
//! GIF takes the plain route, because its container has nothing to say that
//! this program could act on: no profile, no code points, no orientation, and
//! a palette of sRGB bytes by definition. What it does have is animation. The
//! crate's decoder composites every frame onto the logical screen the file
//! declares — disposal, transparency and offsets resolved — so a frame stored
//! as a patch still arrives whole, and `decode` takes the first of them while
//! `frames` walks them all for the player. A GIF's header does not say how
//! many frames it holds, so `sequence` cannot either; the count is learned by
//! reading to the end. Every GIF comes back RGBA, whatever its palette holds,
//! because that is the one layout the crate's decoder produces.
//!
//! BMP takes it too. A `BITMAPV4` or `BITMAPV5` header can name a color space
//! — sRGB, or a whole ICC profile appended after the pixels — but the crate's
//! decoder surfaces neither, and a BMP carrying either is rare enough that
//! reading the header a second time to find one would be work spent on almost
//! nothing. Everything else the format varies — 1, 4, 8, 16, 24 and 32 bits
//! per pixel, palettes, `BI_RLE4` and `BI_RLE8` runs, bitfield masks, and
//! rows stored bottom-up or top-down — the crate resolves before it answers,
//! so all of it arrives here as one of three layouts and means sRGB.
//!
//! Netpbm takes it as well. A PBM, PGM, PPM or PAM states a `MAXVAL`, the
//! value a fully bright sample has, and it need not be 255 or 65535:
//! instrument pipelines write 1023 or 4095, and a bitmap writes 1. Left
//! alone that would matter a great deal, because `Samples::full_scale` says
//! a `U16` image's white is 65535 and a raster stated against 1023 would
//! show at a sixteenth of its brightness — the correction a 10-bit HEIF
//! needs and gets in `super::heif`. The crate already applies it, rescaling
//! every sample to saturate its width before handing the buffer over, so
//! there is nothing left here to do but say that it does.
//! `pnm-maxval1023.pgm` is the fixture that keeps it true.
//!
//! What netpbm says about color is nothing this program can act on either.
//! Its specification names the BT.709 transfer function, which is close
//! enough to sRGB to call it that; a pipeline writing linear measurements
//! into a PGM is indistinguishable from the inside, and is what
//! `--transfer linear` is for.
//!
//! Radiance is read by the crate's decoder built by hand rather than through
//! `ImageReader`, because the reader insists that `#?RADIANCE` be the first
//! ten bytes of the file, and Radiance itself does not. To its own tools a
//! picture's header is text lines up to a blank one, and any of them may
//! come first: `rpict` writes its command line and `VIEW=`, `pfilt` and
//! `pvalue` add theirs, and a line put in front by hand is as good as any.
//! What says the file is a picture is the `FORMAT=` line, wherever it falls.
//! Paul Debevec's `memorial.hdr`, the church every tone-mapping paper shows,
//! goes around with a `VIEW=` line ahead of the signature, and is refused by
//! everything that reads only the first ten bytes. So [`is_radiance`] reads
//! the header the way Radiance does, and the decoder is built with the
//! signature check off — the crate's own provision for the old `.pic` files
//! that never had one.

use std::fs::File;
use std::io::{BufReader, Seek, SeekFrom};
use std::time::Duration;

use anyhow::{Context, Result, bail};

use ::image::codecs::gif::GifDecoder;
use ::image::codecs::hdr::HdrDecoder;
use ::image::{AnimationDecoder, DynamicImage, ImageDecoder, ImageFormat};

use crate::image::sequence::{Frame, FrameSource, Loops, Sequence, gif_delay};
use crate::image::{ColorSpace, DecodedImage, Referred};

use super::{Overrides, ReadSeek, dynamic};

/// The exposures a Radiance picture can state and have been scaled by
/// nothing: `pfilt`'s own tolerance, inside which it writes no line at all.
const UNSCALED: std::ops::RangeInclusive<f32> = 0.98..=1.02;

/// The smallest DIB header a BMP can carry, and the largest anything writes.
/// A file claiming less is malformed; one claiming wildly more is not a BMP
/// that happens to start with the right two letters.
const DIB_HEADER: std::ops::RangeInclusive<u32> = 12..=1024;

pub struct ImageRs;

impl super::Decoder for ImageRs {
    fn name(&self) -> &'static str {
        "gif/hdr/exr/bmp/netpbm"
    }

    /// `.pnm` is the format-agnostic spelling netpbm's own tools accept for
    /// any of the four, so it is claimed alongside the specific ones.
    fn extensions(&self) -> &'static [&'static str] {
        &[
            "gif", "hdr", "exr", "bmp", "pnm", "pbm", "pgm", "ppm", "pam",
        ]
    }

    fn sniff(&self, header: &[u8]) -> bool {
        is_gif(header)
            || is_radiance(header)
            || is_exr(header)
            || is_bmp(header)
            || is_netpbm(header)
    }

    /// Which of the five, by the same signatures; the name of all of them
    /// for a file claimed by its extension alone.
    fn format(&self, header: &[u8]) -> &'static str {
        if is_gif(header) {
            "gif"
        } else if is_radiance(header) {
            "hdr"
        } else if is_exr(header) {
            "exr"
        } else if is_bmp(header) {
            "bmp"
        } else if is_netpbm(header) {
            "netpbm"
        } else {
            self.name()
        }
    }

    fn dimensions(&self, source: &mut dyn ReadSeek) -> Result<Option<(u32, u32)>> {
        if let Some(decoder) = radiance(source)? {
            return Ok(Some(decoder.dimensions()));
        }
        dynamic::dimensions(source)
    }

    fn decode(&self, source: &mut dyn ReadSeek, _overrides: Overrides) -> Result<DecodedImage> {
        if let Some(mut decoder) = radiance(source)? {
            decoder
                .set_limits(dynamic::limits())
                .context("reading the Radiance header")?;
            let exposure = decoder.metadata().exposure;
            let decoded =
                DynamicImage::from_decoder(decoder).context("decoding the Radiance picture")?;
            let mut image = dynamic::describe(decoded, Some(ImageFormat::Hdr), ColorSpace::SRGB)?;
            // `EXPOSURE=` is what `pfilt` writes once it has scaled the
            // picture to be looked at: the multiplier already applied, every
            // line multiplied in. A picture that carries one has been given
            // its white, and is shown as stored, the way `ximage` shows it;
            // one that does not — `rpict`'s own output, a light probe, a
            // merge of exposures — is in whatever scale it was made in, and
            // is metered. So is one whose line says nothing was done:
            // `pfilt` leaves the line out within two percent of 1, and
            // Blender's own writer put `EXPOSURE=1` on every picture it
            // saved, none of them scaled for anything.
            if let Some(exposure) =
                exposure.filter(|exposure| exposure.is_finite() && *exposure > 0.0)
            {
                image.exposure = Some(exposure);
                if !UNSCALED.contains(&exposure) {
                    image.referred = Referred::Display;
                }
            }
            return Ok(image);
        }
        let mut reader = ::image::ImageReader::new(BufReader::new(source)).with_guessed_format()?;
        dynamic::limit(&mut reader);

        let format = reader.format();
        let decoded = reader.decode()?;
        dynamic::describe(decoded, format, ColorSpace::SRGB)
    }

    /// Only a GIF here is ever more than one image. Its header says nothing
    /// about how many frames follow, so they are counted by walking the file
    /// with the decoding switched off: every frame's bytes are read past and
    /// none is decoded. Whether it loops is in the extension block browsers
    /// introduced for it, met on the same walk.
    fn sequence(&self, source: &mut dyn ReadSeek) -> Result<Sequence> {
        let Some((delays, repeat)) = walk_gif(source)? else {
            return Ok(Sequence::Still);
        };
        Ok(if delays.len() > 1 {
            Sequence::Animation {
                count: delays.len(),
                loops: loops(repeat),
            }
        } else {
            Sequence::Still
        })
    }

    /// Each frame's delay is in the graphic control block ahead of its
    /// pixels, so the walk that counts the frames reads them too.
    fn delays(&self, source: &mut dyn ReadSeek) -> Result<Option<Vec<Duration>>> {
        Ok(walk_gif(source)?.map(|(delays, _)| delays))
    }

    fn frames(
        &self,
        source: BufReader<File>,
        _overrides: Overrides,
    ) -> Result<Box<dyn FrameSource>> {
        let mut file = source.into_inner();
        let mut signature = [0u8; 6];
        file.rewind()?;
        if super::fill(&mut file, &mut signature)? < signature.len() || !is_gif(&signature) {
            bail!("only a GIF among these formats is animated");
        }
        let mut frames = GifFrames { file, frames: None };
        frames.rewind()?;
        Ok(Box::new(frames))
    }
}

/// Every frame of a GIF read past without being decoded: how long each is
/// shown for, as the frames will say once they are decoded, and the loop
/// extension. `None` for a file that is not a GIF.
fn walk_gif(source: &mut dyn ReadSeek) -> Result<Option<(Vec<Duration>, gif::Repeat)>> {
    let mut signature = [0u8; 6];
    source.rewind()?;
    if super::fill(source, &mut signature)? < signature.len() || !is_gif(&signature) {
        return Ok(None);
    }
    source.rewind()?;
    let mut options = gif::DecodeOptions::new();
    options.skip_frame_decoding(true);
    let mut decoder = options
        .read_info(BufReader::new(source))
        .context("reading the GIF header")?;
    let mut delays = Vec::new();
    while let Some(frame) = decoder
        .next_frame_info()
        .context("reading the GIF frames")?
    {
        // Hundredths of a second, as `image` reads them for a frame.
        delays.push(gif_delay(Duration::from_millis(
            u64::from(frame.delay) * 10,
        )));
    }
    Ok(Some((delays, decoder.repeat())))
}

/// The loop extension as browsers read it. Its count is how many times to
/// *repeat*, so a file saying 1 plays twice, and a file with no extension —
/// which the crate reports as zero repeats — plays once. Zero in the
/// extension itself means for ever, and the crate has already read it so.
fn loops(repeat: gif::Repeat) -> Loops {
    match repeat {
        gif::Repeat::Infinite => Loops::Forever,
        gif::Repeat::Finite(repeats) => Loops::from_count(u32::from(repeats) + 1),
    }
}

fn is_gif(header: &[u8]) -> bool {
    // Both GIF versions; the four bytes after `GIF` are `87a` or `89a`,
    // and only the first three are a signature.
    header.starts_with(b"GIF87a") || header.starts_with(b"GIF89a")
}

/// A GIF's frames, composited by `image` onto the logical screen.
///
/// The crate's iterator takes the decoder and the decoder takes the reader,
/// so a rewind is a fresh decoder over the same open file: the handle is
/// kept, seeked back to the start, and read again from there.
struct GifFrames {
    file: File,
    frames: Option<::image::Frames<'static>>,
}

impl FrameSource for GifFrames {
    fn next(&mut self) -> Result<Option<Frame>> {
        let Some(frames) = self.frames.as_mut() else {
            return Ok(None);
        };
        match frames.next() {
            None => Ok(None),
            Some(frame) => {
                let frame = frame.context("decoding a GIF frame")?;
                let mut frame = dynamic::frame(frame, ImageFormat::Gif, ColorSpace::SRGB)?;
                frame.delay = gif_delay(frame.delay);
                Ok(Some(frame))
            }
        }
    }

    fn rewind(&mut self) -> Result<()> {
        // Dropped before the file is seeked: the old decoder holds a clone
        // of the same handle, and the two share one offset.
        self.frames = None;
        self.file.seek(SeekFrom::Start(0))?;
        let handle = self.file.try_clone().context("reopening the GIF")?;
        let mut decoder =
            GifDecoder::new(BufReader::new(handle)).context("reading the GIF header")?;
        decoder
            .set_limits(dynamic::limits())
            .context("reading the GIF header")?;
        self.frames = Some(decoder.into_frames());
        Ok(())
    }
}

/// BMP's signature is two letters, which is thin enough that plain text can
/// wear it — `BM` opens plenty of English sentences. The DIB header size that
/// follows the file header is what makes the guess safe: it is a small number
/// from a known set, and four bytes of prose are not.
/// OpenEXR's magic number.
fn is_exr(header: &[u8]) -> bool {
    header.starts_with(b"\x76\x2f\x31\x01")
}

fn is_bmp(header: &[u8]) -> bool {
    let Some(size) = header.get(14..18) else {
        return false;
    };
    header.starts_with(b"BM")
        && DIB_HEADER.contains(&u32::from_le_bytes(size.try_into().expect("four bytes")))
}

/// The crate's Radiance decoder over `source`, if `source` is a Radiance
/// picture; `None` hands anything else on to `ImageReader`. The header is
/// read the way Radiance reads it, so a `VIEW=` line ahead of the signature
/// is a picture still.
fn radiance(source: &mut dyn ReadSeek) -> Result<Option<HdrDecoder<BufReader<&mut dyn ReadSeek>>>> {
    let mut header = [0u8; super::HEADER];
    source.rewind()?;
    let read = super::fill(source, &mut header)?;
    if !is_radiance(&header[..read]) {
        source.rewind()?;
        return Ok(None);
    }
    source.rewind()?;
    let decoder =
        HdrDecoder::new_nonstrict(BufReader::new(source)).context("reading the Radiance header")?;
    Ok(Some(decoder))
}

/// Whether `header` opens a Radiance picture. The signature, `#?RADIANCE`
/// or the `#?RGBE` some writers put instead, is usually the first line, but
/// Radiance's own reader takes the header as any text lines up to a blank
/// one and identifies a picture by its `FORMAT=` line, so a picture whose
/// signature comes second — or is missing, as the oldest `.pic` files' is —
/// is read the same way here. A line that is not text ends the search: a
/// header is text, and a file that is not cannot be claimed by a `FORMAT=`
/// that happens to fall in its first few kilobytes.
fn is_radiance(header: &[u8]) -> bool {
    for line in header.split(|&b| b == b'\n') {
        if line.is_empty()
            || !line
                .iter()
                .all(|b| b.is_ascii_graphic() || *b == b' ' || *b == b'\t')
        {
            return false;
        }
        if line == b"#?RADIANCE"
            || line == b"#?RGBE"
            || line.starts_with(b"FORMAT=32-bit_rle_rgbe")
            || line.starts_with(b"FORMAT=32-bit_rle_xyze")
        {
            return true;
        }
    }
    false
}

/// Netpbm's magic number is `P` and a digit, which needs the whitespace that
/// has to follow it to be worth trusting: the two letters alone would claim
/// any file starting `P5`.
fn is_netpbm(header: &[u8]) -> bool {
    matches!(header, [b'P', kind, space, ..]
        if (b'1'..=b'7').contains(kind) && space.is_ascii_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::decode::Decoder;

    /// A GIF's delays are read in order from its frames' control blocks,
    /// in hundredths, with the browsers' floor applied to the fastest.
    #[test]
    fn a_gif_states_each_frame_delay() {
        let mut file = Vec::new();
        {
            let mut encoder =
                gif::Encoder::new(&mut file, 1, 1, &[0, 0, 0, 255, 255, 255]).unwrap();
            for delay in [5, 30, 1] {
                let mut frame = gif::Frame::from_indexed_pixels(1, 1, vec![0], None);
                frame.delay = delay;
                encoder.write_frame(&frame).unwrap();
            }
        }
        let stated = ImageRs
            .delays(&mut std::io::Cursor::new(file))
            .unwrap()
            .unwrap();
        assert_eq!(
            stated,
            [50, 300, 100].map(Duration::from_millis),
            "a hundredth is played as a tenth"
        );
    }

    /// The five formats are one decoder, but not one kind of file: each
    /// fixture is named for what it is, and a file claimed by its
    /// extension alone for the lot.
    #[test]
    fn each_format_is_named_for_itself() {
        let format =
            |name: &str| super::super::reader(std::path::Path::new(&format!("test_images/{name}")));
        assert_eq!(format("gif-palette.gif"), Some("gif"));
        assert_eq!(format("hdr-rgbe.hdr"), Some("hdr"));
        assert_eq!(format("exr-rgb.exr"), Some("exr"));
        assert_eq!(format("bmp-rgb8.bmp"), Some("bmp"));
        assert_eq!(format("pnm-gray8.pgm"), Some("netpbm"));
        assert_eq!(ImageRs.format(b"not a picture at all"), ImageRs.name());
    }

    /// The bytes a `BITMAPINFOHEADER` file opens with: the signature, a file
    /// size, two reserved words, the pixel offset, and the header size.
    fn bmp_header(dib_size: u32) -> Vec<u8> {
        let mut header = b"BM".to_vec();
        header.extend_from_slice(&2358u32.to_le_bytes());
        header.extend_from_slice(&[0; 4]);
        header.extend_from_slice(&54u32.to_le_bytes());
        header.extend_from_slice(&dib_size.to_le_bytes());
        header
    }

    #[test]
    fn every_dib_header_a_bmp_can_carry_is_recognized() {
        // Core, Info, V2, V3, V4, V5.
        for size in [12, 40, 52, 56, 108, 124] {
            assert!(is_bmp(&bmp_header(size)), "{size}");
        }
    }

    /// Debevec's `memorial.hdr`, as the copies of it going around have it:
    /// the signature on the second line, behind the view someone wrote in
    /// front of it.
    #[test]
    fn a_radiance_header_led_by_a_view_line_is_recognized() {
        let header = b"VIEW= -vtv -vh 90 -vv 150\n#?RADIANCE\npvalue -r +e 0.1865 -s 15 -h -H +y 768 +x 512 -df\nFORMAT=32-bit_rle_rgbe\npflip -v\n\n-Y 768 +X 512\n\x02\x02\x02\x00";
        assert!(is_radiance(header));
        assert!(is_radiance(b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n"));
        assert!(is_radiance(b"#?RGBE\n"));
        // The oldest pictures have no signature at all; the format line is
        // what says what they are.
        assert!(is_radiance(b"pvalue -r\nFORMAT=32-bit_rle_rgbe\n\n"));
    }

    /// The format line is only trusted inside a text header: past a blank
    /// line, or past bytes that are not text, it is not a header any more.
    #[test]
    fn a_format_line_outside_a_text_header_is_not_a_radiance_picture() {
        assert!(!is_radiance(b"pvalue -r\n\nFORMAT=32-bit_rle_rgbe\n"));
        assert!(!is_radiance(b"\x89PNG\r\n\x1a\nFORMAT=32-bit_rle_rgbe\n"));
        assert!(!is_radiance(b""));
    }

    /// PBM, PGM, PPM and PAM, in both their ASCII and binary spellings.
    #[test]
    fn every_netpbm_magic_number_is_recognized() {
        for magic in ["P1", "P2", "P3", "P4", "P5", "P6", "P7"] {
            assert!(is_netpbm(format!("{magic}\n32 24\n").as_bytes()), "{magic}");
        }
    }

    /// The same point the DIB header size makes for BMP: a magic number this
    /// short needs what follows it to be checked.
    #[test]
    fn prose_beginning_with_a_netpbm_magic_number_is_not_claimed() {
        assert!(!is_netpbm(b"P4S is a rendering technique."));
        assert!(!is_netpbm(b"P8\n32 24\n"));
        assert!(!is_netpbm(b"P6"));
    }

    /// The point of looking past the signature: two letters alone would claim
    /// files that are not images at all.
    #[test]
    fn prose_beginning_bm_is_not_claimed() {
        assert!(!is_bmp(
            b"BMW ownership has its privileges, and this is one."
        ));
        // A header too short to hold a DIB header size says nothing either.
        assert!(!is_bmp(b"BM"));
        // Smaller than the smallest header there is, and larger than any.
        assert!(!is_bmp(&bmp_header(11)));
        assert!(!is_bmp(&bmp_header(4096)));
    }
}
