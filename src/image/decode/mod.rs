//! The decoder registry.
//!
//! Adding a format means writing a [`Decoder`] and listing it in [`DECODERS`];
//! nothing downstream needs to change, because decoders describe their output
//! with [`Samples`](super::Samples) and [`ColorSpace`]
//! rather than converting it.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};

use super::sequence::{FrameSource, Sequence};
use super::{ColorSpace, DecodedImage, Primaries, Transfer};

pub(crate) use limits::MAX_SEQUENCE_BYTES;
use limits::{MAX_DECODED_BYTES, MAX_TEXTURE_DIMENSION, check_decoded_size};

mod dynamic;
mod heif;
mod ico;
mod image_rs;
mod jpeg;
mod jxl;
mod limits;
mod png;
mod tiff_rs;
mod webp;

#[cfg(test)]
mod fixture_tests;

/// A seekable byte source. Decoders read from the file directly rather than
/// from a slice, so that opening a 600 MB raster does not begin by copying it
/// into memory whole.
pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

/// Enough of the file for any decoder to recognize its own header.
const HEADER: usize = 64;

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
    /// what a decoder produces rather than how it is labeled.
    fn decode(&self, source: &mut dyn ReadSeek, overrides: Overrides) -> Result<DecodedImage>;

    /// The image's size, taken from the header rather than by decoding it.
    ///
    /// Answers before the pixels exist, so that the window can open at the
    /// right size while the file is still being read. `None` where the format
    /// cannot say cheaply, or cannot say correctly — the window then takes a
    /// plain default rather than a wrong shape.
    ///
    /// Whatever this returns has to match what [`Decoder::decode`] goes on to
    /// produce, orientation and all. Every fixture is checked for that.
    fn dimensions(&self, _source: &mut dyn ReadSeek) -> Result<Option<(u32, u32)>> {
        Ok(None)
    }

    /// What the file holds beyond the image [`Decoder::decode`] returns:
    /// nothing, the frames of an animation, or several pictures. Read from
    /// the header where the format keeps it there. A decoder that does not
    /// answer holds one image.
    fn sequence(&self, _source: &mut dyn ReadSeek) -> Result<Sequence> {
        Ok(Sequence::Still)
    }

    /// One picture of a file that holds several, by its place in the file.
    /// Only where [`Decoder::sequence`] said [`Sequence::Pages`]; page zero
    /// of anything else is the image itself.
    fn decode_page(
        &self,
        source: &mut dyn ReadSeek,
        overrides: Overrides,
        page: usize,
    ) -> Result<DecodedImage> {
        if page == 0 {
            self.decode(source, overrides)
        } else {
            bail!(
                "{} holds one image, and page {page} was asked for",
                self.name()
            )
        }
    }

    /// The frames of an animation, from the first. Only where
    /// [`Decoder::sequence`] said [`Sequence::Animation`]. The source is
    /// taken whole rather than borrowed, since the frames outlive the call
    /// and rewinding may mean opening it again.
    fn frames(
        &self,
        _source: BufReader<File>,
        _overrides: Overrides,
    ) -> Result<Box<dyn FrameSource>> {
        bail!("{} is not animated", self.name())
    }
}

/// Order matters only when two decoders claim the same extension, in which
/// case the first wins.
static DECODERS: &[&dyn Decoder] = &[
    &tiff_rs::TiffRs,
    // Before the HEIF family, whose `ftyp` box it shares a container
    // structure with. `libheif`'s brand check should decline a `JXL ` box and
    // `decode::jxl`'s tests hold it to that, but the cheaper guarantee is to
    // ask the decoder that can be certain first.
    &jxl::Jxl,
    &heif::Heif,
    &webp::Webp,
    &ico::Ico,
    &png::Png,
    &jpeg::Jpeg,
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

    /// What every decoded image goes through on its way out, whether it was
    /// a file's one picture, a page, or a frame: checked for consistency,
    /// then relabeled as the command line asked.
    pub(crate) fn finish(&self, mut image: DecodedImage, path: &Path) -> Result<DecodedImage> {
        image
            .validate()
            .map_err(|problem| anyhow!("{}: {problem}", path.display()))?;
        image.color = self.apply(image.color);
        Ok(image)
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

/// Opens `path` and works out which decoder owns it, reading only the header.
/// The cheap half of [`load`], and all of [`probe`].
fn open(path: &Path) -> Result<(BufReader<File>, &'static dyn Decoder)> {
    // Only a regular file, decided by a `stat` before the open. A directory, a
    // device such as `/dev/zero`, or a FIFO would each otherwise reach a
    // decoder: `/dev/zero` feeds a decoder that reads to the end (JPEG) until
    // it exhausts memory, and — the reason this comes before `File::open`
    // rather than after — opening a FIFO with no writer blocks in `open(2)`
    // itself, with no window and no way out. `stat` follows symlinks and does
    // not block, so it settles the question first. This is the one place every
    // format passes through, so the check need only live here.
    let metadata =
        std::fs::metadata(path).with_context(|| format!("reading {}", path.display()))?;
    if !metadata.is_file() {
        return Err(anyhow!("{} is not a regular file", path.display()));
    }
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

    Ok((source, *decoder))
}

/// Reads and decodes `path`, picking a decoder by extension and falling back
/// to content sniffing.
pub fn load(path: &Path, overrides: Overrides) -> Result<DecodedImage> {
    let (mut source, decoder) = open(path)?;

    let image = decoder
        .decode(&mut source, overrides)
        .with_context(|| format!("decoding {} as {}", path.display(), decoder.name()))?;
    overrides.finish(image, path)
}

/// Reads one page of a file that holds several, chosen the way [`load`]
/// chooses. What [`load`] returns is one of them: the decoder's default,
/// which [`sequence`] names.
pub fn load_page(path: &Path, overrides: Overrides, page: usize) -> Result<DecodedImage> {
    let (mut source, decoder) = open(path)?;

    let image = decoder
        .decode_page(&mut source, overrides, page)
        .with_context(|| {
            format!(
                "decoding page {page} of {} as {}",
                path.display(),
                decoder.name()
            )
        })?;
    overrides.finish(image, path)
}

/// What `path` holds beyond the image [`load`] returns, from its header.
pub fn sequence(path: &Path) -> Result<Sequence> {
    let (mut source, decoder) = open(path)?;
    decoder.sequence(&mut source).with_context(|| {
        format!(
            "reading the header of {} as {}",
            path.display(),
            decoder.name()
        )
    })
}

/// The frames of the animation at `path`, from the first. Each comes out
/// through [`Frames::next`] finished the way [`load`]'s image is.
///
/// [`Frames::next`]: FrameSource::next
pub fn frames(path: &Path, overrides: Overrides) -> Result<Box<dyn FrameSource>> {
    let (source, decoder) = open(path)?;
    let source = decoder
        .frames(source, overrides)
        .with_context(|| format!("opening {} as {}", path.display(), decoder.name()))?;
    Ok(Box::new(Finished {
        source,
        overrides,
        path: path.to_path_buf(),
    }))
}

/// A decoder's frames with the finish every decoded image gets on the way
/// out of [`load`], so that a frame and a still of the same file agree.
struct Finished {
    source: Box<dyn FrameSource>,
    overrides: Overrides,
    path: std::path::PathBuf,
}

impl FrameSource for Finished {
    fn next(&mut self) -> Result<Option<super::sequence::Frame>> {
        let Some(frame) = self.source.next()? else {
            return Ok(None);
        };
        let image = self.overrides.finish(frame.image, &self.path)?;
        Ok(Some(super::sequence::Frame {
            image,
            delay: frame.delay,
        }))
    }

    fn rewind(&mut self) -> Result<()> {
        self.source.rewind()
    }
}

/// Checks that `path` exists and holds a format we know, and asks that format
/// how large it is — all from the header, without decoding a pixel.
///
/// This is what lets the window open before the file has been read. The error
/// is the point of it as much as the size: a missing path or an unknown format
/// stays a plain command-line failure, rather than a window that appears only
/// to close again.
pub fn probe(path: &Path) -> Result<Option<(u32, u32)>> {
    let (mut source, decoder) = open(path)?;
    decoder.dimensions(&mut source).with_context(|| {
        format!(
            "reading the header of {} as {}",
            path.display(),
            decoder.name()
        )
    })
}

/// Which decoder owns `path`, chosen the way [`load`] chooses it: by what the
/// leading bytes say first, and by the extension only where they say nothing.
/// So this answers what the file turned out to *be* rather than what it is
/// called, which is worth saying out loud for a file whose name was wrong.
///
/// A decoder's name, not a format's: most read one format and are named for
/// it, but the two that read several are named for all of them. Naming the
/// one format in hand would mean asking every decoder to report what it found
/// as well as what it can find, which is a great deal of machinery for one
/// line of a panel.
///
/// `None` where nothing claims it, or where it cannot be opened at all — the
/// panel then says nothing rather than something wrong, which is what it
/// does with every other fact it asks the file for after the event.
pub fn reader(path: &Path) -> Option<&'static str> {
    Some(open(path).ok()?.1.name())
}

/// Reads as much as `buffer` holds, tolerating a file shorter than that.
pub(super) fn fill(source: &mut (impl Read + ?Sized), buffer: &mut [u8]) -> std::io::Result<usize> {
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

    /// A directory, a FIFO or a device is not something to hand a decoder:
    /// `/dev/zero` would be read until memory ran out, and a FIFO would block
    /// the reader for ever. The `stat` in `open` turns each into a plain
    /// error. A directory is the case a test can make portably.
    #[test]
    fn a_non_regular_file_is_refused() {
        let error = load(Path::new("test_images"), Overrides::default())
            .expect_err("a directory is not an image");
        assert!(
            format!("{error:#}").contains("not a regular file"),
            "{error:#}"
        );
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
