//! The decoder registry.
//!
//! Adding a format means writing a [`Decoder`] and listing it in [`DECODERS`];
//! nothing downstream needs to change, because decoders describe their output
//! with [`Samples`](super::Samples) and [`ColorSpace`](super::ColorSpace)
//! rather than converting it.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{Context, Result, anyhow};

use super::{ColorSpace, DecodedImage, Primaries, Transfer};

mod cicp;
mod heif;
mod icc;
mod image_rs;
mod tiff_rs;
mod ultra_hdr;
mod webp;

#[cfg(test)]
mod fixture_tests;

/// A seekable byte source. Decoders read from the file directly rather than
/// from a slice, so that opening a 600 MB raster does not begin by copying it
/// into memory whole.
pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

/// Enough of the file for any decoder to recognise its own header.
const HEADER: usize = 64;

/// Ceiling on a single decoded image, in bytes.
///
/// Both backends ship conservative defaults — 256 MiB in `tiff`, 512 MiB in
/// `image` — which a survey-grade elevation model passes on the way out of the
/// door: Mount Rainier at 3 m is 18333x15667, or 1.1 GB of floats.
///
/// 4 GiB is not arbitrary. `max_texture_dimension_2d` is 32768 on current
/// hardware, and 32768 x 32768 x 4 bytes is exactly 4 GiB, so this is the
/// largest single-channel 32-bit image that could be displayed even in
/// principle. Anything past it is refused with a sentence rather than left to
/// the OOM killer.
pub(super) const MAX_DECODED_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// Refuses an image too large to hold, using only what the header states, so
/// that nothing is decoded before the decision is made.
pub(super) fn check_decoded_size(
    width: u32,
    height: u32,
    components: usize,
    bits_per_sample: u8,
) -> Result<()> {
    let bytes = u64::from(width)
        * u64::from(height)
        * components as u64
        * u64::from(bits_per_sample.div_ceil(8));
    if bytes > MAX_DECODED_BYTES {
        return Err(anyhow!(
            "{width}x{height} at {bits_per_sample} bits needs {:.1} GB decoded, \
             over the {:.1} GB this build will hold",
            bytes as f64 / 1e9,
            MAX_DECODED_BYTES as f64 / 1e9,
        ));
    }
    Ok(())
}

pub trait Decoder: Sync {
    /// Human-readable name, used in error messages.
    fn name(&self) -> &'static str;

    /// Lowercase file extensions, without the leading dot.
    fn extensions(&self) -> &'static [&'static str];

    /// Does this look like one of our formats, judging only by the leading
    /// bytes? Used to recover from a missing or misleading extension.
    fn sniff(&self, header: &[u8]) -> bool;

    /// `overrides` is passed in as well as applied afterwards, because one
    /// of its settings — whether to reconstruct from a gain map — changes
    /// what a decoder produces rather than how it is labelled.
    fn decode(&self, source: &mut dyn ReadSeek, overrides: Overrides) -> Result<DecodedImage>;
}

/// Order matters only when two decoders claim the same extension, in which
/// case the first wins.
static DECODERS: &[&dyn Decoder] = &[
    &tiff_rs::TiffRs,
    &heif::Heif,
    &webp::Webp,
    &image_rs::ImageRs,
];

/// Overrides for files whose headers cannot say what they mean. A 16-bit TIFF
/// is the usual case: the same container holds both a scanned photograph and a
/// frame of sensor counts, and only the person who made it knows which.
#[derive(Clone, Copy, Debug)]
pub struct Overrides {
    pub transfer: Option<Transfer>,
    pub primaries: Option<Primaries>,
    /// Whether to reconstruct the HDR image an Ultra HDR JPEG describes.
    /// Clearing it shows the SDR base image every other viewer shows, which
    /// is worth having when the two need comparing — or when a gain map is
    /// malformed enough to refuse.
    pub gain_map: bool,
}

impl Default for Overrides {
    fn default() -> Self {
        Self {
            transfer: None,
            primaries: None,
            gain_map: true,
        }
    }
}

impl Overrides {
    fn apply(&self, color: ColorSpace) -> ColorSpace {
        ColorSpace {
            transfer: self.transfer.unwrap_or(color.transfer),
            primaries: self.primaries.unwrap_or(color.primaries),
        }
    }
}

pub fn supported_extensions() -> Vec<&'static str> {
    let mut extensions: Vec<&'static str> = DECODERS
        .iter()
        .flat_map(|d| d.extensions())
        .copied()
        .collect();
    extensions.sort_unstable();
    extensions.dedup();
    extensions
}

/// Reads and decodes `path`, picking a decoder by extension and falling back
/// to content sniffing.
pub fn load(path: &Path, overrides: Overrides) -> Result<DecodedImage> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut source = BufReader::new(file);

    let mut header = [0u8; HEADER];
    let read =
        fill(&mut source, &mut header).with_context(|| format!("reading {}", path.display()))?;
    let header = &header[..read];
    source
        .seek(SeekFrom::Start(0))
        .with_context(|| format!("reading {}", path.display()))?;

    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();

    // Content first, extension second. A file with the wrong extension is
    // ordinary; a file whose leading bytes lie about what it is, is not.
    let decoder = DECODERS
        .iter()
        .find(|d| d.sniff(header))
        .or_else(|| {
            DECODERS
                .iter()
                .find(|d| d.extensions().contains(&extension.as_str()))
        })
        .ok_or_else(|| {
            anyhow!(
                "unsupported image format for {} (known extensions: {})",
                path.display(),
                supported_extensions().join(", ")
            )
        })?;

    let mut image = decoder
        .decode(&mut source, overrides)
        .with_context(|| format!("decoding {} as {}", path.display(), decoder.name()))?;

    image
        .validate()
        .map_err(|problem| anyhow!("{}: {problem}", path.display()))?;

    image.color = overrides.apply(image.color);
    Ok(image)
}

/// Reads as much as `buffer` holds, tolerating a file shorter than that.
fn fill(source: &mut impl Read, buffer: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match source.read(&mut buffer[filled..])? {
            0 => break,
            count => filled += count,
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The size a real survey elevation model needs, which both backends'
    /// stock limits refuse. Mount Rainier at 3 m is the case that prompted
    /// raising them.
    #[test]
    fn a_survey_grade_elevation_model_is_within_the_limit() {
        assert!(check_decoded_size(18333, 15667, 1, 32).is_ok());
        // And the 10 m version, comfortably.
        assert!(check_decoded_size(5500, 4700, 1, 32).is_ok());
    }

    /// The ceiling is the largest image the maximum GPU texture dimension
    /// could hold at all, so nothing displayable is turned away.
    #[test]
    fn the_limit_matches_the_largest_displayable_texture() {
        assert_eq!(MAX_DECODED_BYTES, 32768 * 32768 * 4);
        assert!(check_decoded_size(32768, 32768, 1, 32).is_ok());
    }

    #[test]
    fn an_image_too_large_to_hold_is_refused_before_decoding() {
        let error = check_decoded_size(100_000, 100_000, 1, 32).unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("100000x100000"), "{message}");
        assert!(message.contains("GB"), "{message}");
    }

    /// Multiple channels count toward the ceiling, or an RGBA image four
    /// times the size of an allowed grey one would slip through.
    #[test]
    fn channel_count_and_bit_depth_both_count() {
        assert!(check_decoded_size(32768, 32768, 4, 32).is_err());
        assert!(check_decoded_size(32768, 32768, 4, 8).is_ok());
    }

    /// Two decoders claiming the same extension would make the choice depend
    /// on registry order, which nobody would think to check.
    #[test]
    fn every_extension_is_claimed_by_exactly_one_decoder() {
        let mut seen = std::collections::HashSet::new();
        for decoder in DECODERS {
            for extension in decoder.extensions() {
                assert!(seen.insert(*extension), "`{extension}` is claimed twice");
                assert_eq!(
                    *extension,
                    extension.to_ascii_lowercase(),
                    "extensions must be lowercase"
                );
            }
        }
    }
}
