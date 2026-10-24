//! TIFF, via the `tiff` crate directly rather than through `image`.
//!
//! `image` cannot carry this format's full range: `DynamicImage` has no
//! single-band floating-point variant, so a one-band Float32 TIFF — which is
//! what essentially every DEM and scientific raster is — fails outright with
//! "does not support the color type `Unknown(32)`". It also cannot open
//! BigTIFF at all, because its format sniffer only knows the classic magic.
//!
//! Going to `tiff` directly fixes both, and keeps a single-band raster
//! single-band all the way to the GPU: a 5500x4700 elevation model uploads as
//! 103 MB of `R32Float` rather than 413 MB of `Rgba32Float`.
//!
//! A TIFF is a chain of directories, each a picture in its own right, and
//! most files have one. Where there are more they are pages here: the first
//! is what `decode` shows, and any other is a decode of its own through
//! `decode_page`, since a directory may differ from its neighbors in size,
//! depth and layout. Nothing tells a page from an overview or a thumbnail,
//! so a pyramid's reduced copies count as pages too. A transparency mask
//! is told apart, by `NewSubfileType`, and is not a page: it is the
//! coverage of the picture before it, one bit a pixel, and GDAL writes one
//! after the picture and one after each reduced copy.
//!
//! A directory of gray may also hold more samples a pixel than its one, as
//! GDAL writes a raster of several bands — a classifier's probabilities, a
//! stack of a satellite's bands. Each band past the first is a page too,
//! after the picture the first band and its alpha make — see [`runs`]. A
//! band's page is read like any other, every chunk decoded whole and the
//! one sample a pixel kept, and named by GDAL's description of it where the
//! file carries one. RGB with samples beside it that are not alpha is read
//! as its picture alone: the crate hands an interleaved chunk of RGB back
//! with only its color, and a band that could be reached only where the
//! file happens to be stored a plane to a band would come and go with how
//! it was written.
//!
//! The pixels are stored in chunks — strips, or tiles — each compressed on
//! its own, and each is read here on its own too: the rows of chunks are
//! divided between rayon's threads, and every thread opens a decoder of its
//! own over the same file, since the crate's decoder reads through one
//! position. `read_image` would decode the chunks one after another, and a
//! 134-megapixel LZW map took 1.7 s that way on one core.
//!
//! A JPEG-compressed TIFF — what GDAL writes for a scanned map or an aerial
//! photograph with `COMPRESS=JPEG` — stores its pixels as YCbCr, and the
//! crate hands them back that way: it tells the JPEG decoder to upsample the
//! chroma but not to convert, since the conversion is the container's to
//! define. [`YCbCr`] does it here, from the file's own coefficients and
//! coding range, chunk by chunk as they are read.

use std::fs::File;
use std::io::{self, BufReader, Seek};
use std::ops::Range;

use ::image::metadata::Orientation;
use anyhow::{Context, Result, anyhow, bail};
use tiff::decoder::{Decoder, DecodingResult, DecodingSampleType, Limits};
use tiff::tags::Tag;

use super::{Positioned, Width};
use crate::image::orient;
use crate::image::sequence::Sequence;
use crate::image::{AlphaMode, Channels, ColorSpace, DecodedImage, Samples};

/// GDAL writes the no-data value here, as an ASCII string.
const GDAL_NODATA: u16 = 42113;
/// GDAL's metadata, as an XML document of `Item`s: among them each band's
/// description, which is where a band's name is kept.
const GDAL_METADATA: u16 = 42112;
/// `PageName`: the name of the document a directory is a page of.
const PAGE_NAME: u16 = 285;
/// GeoTIFF's key directory, which a raster placed on the ground carries.
const GEO_KEY_DIRECTORY: u16 = 34735;

/// The bit of `NewSubfileType` that marks a directory as a transparency
/// mask of another. The other two bits, a reduced-resolution copy and a
/// page of a document, are both still pictures.
const TRANSPARENCY_MASK: u32 = 4;

/// `YCbCrCoefficients`: the three luma weights, as rationals.
const YCBCR_COEFFICIENTS: u16 = 529;
/// `ReferenceBlackWhite`: the code values of black and white in each
/// channel, as six rationals.
const REFERENCE_BLACK_WHITE: u16 = 532;

const MAX_IFD_VALUE_BYTES: usize = 16 * 1024 * 1024;

pub struct TiffRs;

impl super::Decoder for TiffRs {
    fn name(&self) -> &'static str {
        "tiff"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["tif", "tiff"]
    }

    fn sniff(&self, header: &[u8]) -> bool {
        // Classic TIFF is 42, BigTIFF is 43, in either byte order.
        crate::image::tiff::header(header).is_some()
    }

    fn dimensions(&self, source: &mut dyn super::ReadSeek) -> Result<Option<(u32, u32)>> {
        let mut decoder = Decoder::new(source)?;
        let (width, height) = decoder.dimensions()?;
        Ok(Some(orient::size(
            width,
            height,
            orientation(&mut decoder)?,
        )))
    }

    fn decode(
        &self,
        source: &mut dyn super::ReadSeek,
        overrides: super::Overrides,
    ) -> Result<DecodedImage> {
        self.decode_page(source, overrides, 0)
    }

    /// The packet is a tag of the first directory, read through the
    /// decoder, which seeks to the directory wherever the file keeps it.
    fn xmp(&self, source: &mut dyn super::ReadSeek) -> Result<Option<Vec<u8>>> {
        Ok(crate::image::directory::packet(source))
    }

    /// How many pictures the chain holds, walked without reading any
    /// pixels, and whether they are all the bands of one directory.
    fn sequence(&self, source: &mut dyn super::ReadSeek) -> Result<Sequence> {
        let pages = pages(&mut Decoder::new(source)?)?;
        let count = pages.len();
        Ok(if count > 1 {
            Sequence::Pages {
                count,
                default: 0,
                bands: pages
                    .iter()
                    .all(|page| page.directory == pages[0].directory),
            }
        } else {
            Sequence::Still
        })
    }

    /// A band's description, from GDAL's metadata, or the directory's own
    /// `PageName` for a page that is the whole of its directory's picture.
    fn page_name(&self, source: &mut dyn super::ReadSeek, page: usize) -> Result<Option<String>> {
        let mut decoder = Decoder::new(source)?;
        let found = find(&mut decoder, page)?;
        let band = match found.run {
            Some(Run {
                first,
                channels: Channels::Gray,
            }) => description(&mut decoder, first),
            _ => None,
        };
        // A directory's name is the name of its picture, and not of one band
        // among several.
        let whole = found.run.is_none_or(|run| run.first == 0);
        Ok(band.or_else(|| {
            whole
                .then(|| decoder.get_tag_ascii_string(Tag::Unknown(PAGE_NAME)).ok())
                .flatten()
                .and_then(named)
        }))
    }

    fn decode_page(
        &self,
        source: &mut dyn super::ReadSeek,
        _overrides: super::Overrides,
        page: usize,
    ) -> Result<DecodedImage> {
        // `Limits` is non-exhaustive, so start from the defaults and raise
        // only what needs raising.
        let mut limits = Limits::default();
        limits.decoding_buffer_size = super::MAX_DECODED_BYTES as usize;
        limits.intermediate_buffer_size = 256 * 1024 * 1024;
        limits.ifd_value_size = MAX_IFD_VALUE_BYTES;

        // Taken before the decoder borrows the source: the file the other
        // threads' decoders will read, where there is one.
        let shared = source.share()?;
        let mut decoder = Decoder::new(source)?.with_limits(limits.clone());
        let found = find(&mut decoder, page)?;
        let directory = found.directory;
        let (width, height) = decoder.dimensions()?;

        // A run of the samples is read as what it was found to be; a
        // directory read whole, as the crate names its layout.
        let (channels, ycbcr) = match found.run {
            Some(run) => (run.channels, None),
            None => {
                let color = decoder.colortype()?;
                let channels = match color {
                    // The decoder expands indexed data to RGB for us; YCbCr
                    // it leaves to us, and only 8-bit is ever seen, since
                    // JPEG is the compression that carries it.
                    tiff::ColorType::RGB(_)
                    | tiff::ColorType::Palette(_)
                    | tiff::ColorType::YCbCr(8) => Channels::Rgb,
                    other => bail!("unsupported TIFF color type {other:?}"),
                };
                let ycbcr = match color {
                    tiff::ColorType::YCbCr(_) => Some(YCbCr::read(&mut decoder)?),
                    _ => None,
                };
                (channels, ycbcr)
            }
        };

        // Checked against what will be held rather than the stored depth:
        // every signed or wide type is widened to `f32` on the way in, up to
        // four times the size, and a signed 8-bit raster at the ceiling must
        // not balloon four times past it.
        let held = held_as(decoder.image_chunk_buffer_layout(0)?.sample_type);
        super::check_decoded_size(width, height, channels.count(), held.bits())?;

        // Read before the pixels move the decoder's position.
        let nodata = decoder
            .get_tag_ascii_string(Tag::Unknown(GDAL_NODATA))
            .ok()
            .and_then(|value| value.trim().parse::<f32>().ok());
        let orientation = orientation(&mut decoder)?;
        let profile = decoder.find_tag(Tag::IccProfile)?;
        // A GeoTIFF's key directory, or a value standing for no data: either
        // says the file is a measurement unless it is 8-bit color — see
        // `color_space`.
        let georeferenced =
            nodata.is_some() || decoder.find_tag(Tag::Unknown(GEO_KEY_DIRECTORY))?.is_some();

        // Chunk by chunk, across threads, wherever the samples are a run the
        // decoder found: interleaved, a pixel's samples side by side in each
        // chunk, or planar, each sample's chunks a plane of their own. What
        // is left — a palette or YCbCr stored planar — is rare enough to
        // leave to `read_image`.
        let planar = decoder
            .find_tag_unsigned::<u16>(Tag::PlanarConfiguration)?
            .is_some_and(|planar| planar == 2);
        let samples = if !planar || found.run.is_some() {
            let geometry = Geometry::of(&decoder, width, height, &found, channels, planar)?;
            let read = Read {
                shared: shared.as_ref(),
                limits: &limits,
                directory,
                geometry: &geometry,
                ycbcr: ycbcr.as_ref(),
            };
            match held {
                Width::U8 => Samples::U8 {
                    channels,
                    data: read_chunks(&mut decoder, &read)?,
                },
                Width::U16 => Samples::U16 {
                    channels,
                    data: read_chunks(&mut decoder, &read)?,
                },
                Width::F32 => Samples::F32 {
                    channels,
                    data: read_chunks(&mut decoder, &read)?,
                },
            }
        } else {
            let mut samples = into_samples(decoder.read_image()?, channels)?;
            if let (Some(ycbcr), Samples::U8 { data, .. }) = (&ycbcr, &mut samples) {
                ycbcr.to_rgb(data);
            }
            samples
        };

        let expected = width as usize * height as usize * channels.count();
        if samples.len() != expected {
            return Err(anyhow!(
                "TIFF decoded {} samples, expected {expected} for {width}x{height} {channels:?}",
                samples.len()
            ));
        }

        // One band of several is a measurement, whatever its depth: nothing
        // stores a picture as a gray channel with others beside it.
        let band = channels == Channels::Gray && found.stride > 1;
        let color = color_space(&samples, profile, georeferenced || band);
        let mut image = DecodedImage::new(
            width,
            height,
            samples,
            color,
            AlphaMode::of(channels, false),
        );
        image.nodata = nodata;
        Ok(orient::apply(image, orientation))
    }
}

/// One page of the file: a directory, and which of its samples make it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Page {
    /// Which directory of the chain, by index.
    directory: usize,
    /// How many samples a pixel the crate hands an interleaved chunk back
    /// with: every one the directory holds for gray, and the picture's
    /// alone for RGB.
    stride: usize,
    /// The samples of each pixel the page is made of. `None` is the
    /// directory read whole, as the crate names its layout: a palette,
    /// YCbCr, or an interpretation the program has no run for.
    run: Option<Run>,
}

/// A run of a pixel's samples, read as one picture.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Run {
    /// The first of the pixel's samples in the run.
    first: usize,
    /// What the run is, and so how many samples it takes.
    channels: Channels,
}

/// The pages a directory holds, in order: the picture its photometric
/// interpretation names first, with its alpha where the sample after the
/// color is one, and then, for gray, a page for each sample past those. An
/// `ExtraSamples` tag says which of the extra samples is alpha; with no tag,
/// the one extra sample a picture can have is taken as alpha, as readers
/// have long taken a fourth sample of RGB.
///
/// Only gray is split, and RGB read as its run: a palette is one sample
/// expanded, YCbCr is three converted together, and anything else is left
/// to the crate to name or refuse, as one page.
fn runs(photometric: Option<u16>, samples: usize, extra: Option<&[u16]>) -> Vec<Option<Run>> {
    // WhiteIsZero, BlackIsZero, RGB.
    let color = match photometric {
        Some(0 | 1) => 1,
        Some(2) => 3,
        _ => return vec![None],
    };
    if samples < color {
        return vec![None];
    }
    // Associated or unassociated alpha.
    let alpha = samples > color
        && match extra {
            Some(extra) => matches!(extra.first(), Some(1 | 2)),
            None => samples == color + 1,
        };
    let channels = match (color, alpha) {
        (1, false) => Channels::Gray,
        (1, true) => Channels::GrayAlpha,
        (_, false) => Channels::Rgb,
        (_, true) => Channels::Rgba,
    };
    let picture = Run { first: 0, channels };
    let bands = if color == 1 {
        channels.count()..samples
    } else {
        0..0
    };
    std::iter::once(Some(picture))
        .chain(bands.map(|first| {
            Some(Run {
                first,
                channels: Channels::Gray,
            })
        }))
        .collect()
}

/// The pages of the directory the decoder is on.
fn directory_pages<R: io::Read + Seek>(
    decoder: &mut Decoder<R>,
    directory: usize,
) -> Result<Vec<Page>> {
    let photometric = decoder.find_tag_unsigned::<u16>(Tag::PhotometricInterpretation)?;
    let samples = decoder
        .find_tag_unsigned::<u16>(Tag::SamplesPerPixel)?
        .unwrap_or(1)
        .max(1) as usize;
    let extra = match decoder.find_tag(Tag::ExtraSamples)? {
        Some(value) => Some(value.into_u16_vec()?),
        None => None,
    };
    let runs = runs(photometric, samples, extra.as_deref());
    let stride = match runs[0] {
        Some(Run {
            channels: channels @ (Channels::Rgb | Channels::Rgba),
            ..
        }) => channels.count(),
        _ => samples,
    };
    Ok(runs
        .into_iter()
        .map(|run| Page {
            directory,
            stride,
            run,
        })
        .collect())
}

/// The pages of the whole chain, in order: every directory's but a
/// transparency mask's, which is the coverage of the picture before it
/// rather than a page of its own — and one bit a pixel, which the crate
/// would not decode as a picture anyway. The decoder is left on the last
/// directory; `seek_to_image` finds any of them again from there.
fn pages<R: io::Read + Seek>(decoder: &mut Decoder<R>) -> Result<Vec<Page>> {
    let mut pages = Vec::new();
    let mut index = 0;
    loop {
        let subfile = decoder
            .find_tag_unsigned::<u32>(Tag::NewSubfileType)?
            .unwrap_or(0);
        if subfile & TRANSPARENCY_MASK == 0 {
            pages.extend(directory_pages(decoder, index)?);
        }
        if !decoder.more_images() {
            break;
        }
        decoder.next_image()?;
        index += 1;
    }
    Ok(pages)
}

/// Page `page`, with the decoder left on its directory. The first page is
/// the first directory's picture, whatever the directory says of itself,
/// and found without walking the chain; any other is found past the masks.
fn find<R: io::Read + Seek>(decoder: &mut Decoder<R>, page: usize) -> Result<Page> {
    if page == 0 {
        let first = directory_pages(decoder, 0)?[0];
        return Ok(first);
    }
    let found = *pages(decoder)?
        .get(page)
        .ok_or_else(|| anyhow!("TIFF has no page {page}"))?;
    decoder.seek_to_image(found.directory)?;
    Ok(found)
}

/// GDAL's description of the directory's sample `sample`, from the metadata
/// it writes beside the pixels: an `Item` named `DESCRIPTION` in the role of
/// one, for that sample. Metadata that will not parse names nothing.
fn description<R: io::Read + Seek>(decoder: &mut Decoder<R>, sample: usize) -> Option<String> {
    let text = decoder
        .get_tag_ascii_string(Tag::Unknown(GDAL_METADATA))
        .ok()?;
    let document = roxmltree::Document::parse(&text).ok()?;
    let sample = sample.to_string();
    document
        .root_element()
        .children()
        .filter(|node| node.has_tag_name("Item"))
        .find(|item| {
            item.attribute("name") == Some("DESCRIPTION")
                && item.attribute("role") == Some("description")
                && item.attribute("sample") == Some(sample.as_str())
        })
        .and_then(|item| item.text())
        .and_then(|text| named(text.to_string()))
}

/// A name as a file wrote it, trimmed, and nothing where nothing is left.
fn named(name: String) -> Option<String> {
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// The width a file's samples are held at: the stored type for unsigned
/// 8- and 16-bit data, and `f32` for everything else the format allows.
///
/// Signed and wide integer rasters become floats rather than being rescaled:
/// an elevation model holds meters, and -86 at the Dead Sea is a real value,
/// not something to normalize away. The display window is what turns them
/// into something visible. `None` is a depth or format the crate cannot
/// name, which it will not decode either; `f32` keeps the size check honest
/// until it says so.
fn held_as(sample_type: Option<DecodingSampleType>) -> Width {
    match sample_type {
        Some(DecodingSampleType::U8) => Width::U8,
        Some(DecodingSampleType::U16) => Width::U16,
        _ => Width::F32,
    }
}

/// A sample type a file is held as: what one decoded chunk becomes in it.
trait Resident: Copy + Default + Send {
    fn take(result: DecodingResult) -> Result<Vec<Self>>;

    /// A chunk of YCbCr made RGB. Only 8-bit data ever arrives as YCbCr,
    /// so the other types have nothing to do.
    fn to_rgb(_chunk: &mut [Self], _ycbcr: &YCbCr) {}
}

impl Resident for u8 {
    fn take(result: DecodingResult) -> Result<Vec<Self>> {
        match result {
            DecodingResult::U8(data) => Ok(data),
            _ => Err(mismatch()),
        }
    }

    fn to_rgb(chunk: &mut [Self], ycbcr: &YCbCr) {
        ycbcr.to_rgb(chunk);
    }
}

impl Resident for u16 {
    fn take(result: DecodingResult) -> Result<Vec<Self>> {
        match result {
            DecodingResult::U16(data) => Ok(data),
            _ => Err(mismatch()),
        }
    }
}

impl Resident for f32 {
    fn take(result: DecodingResult) -> Result<Vec<Self>> {
        match result {
            DecodingResult::U8(_) | DecodingResult::U16(_) => Err(mismatch()),
            other => Ok(widened(other)),
        }
    }
}

/// The header said one type and a chunk decoded as another. The crate
/// decides both from the same tags, so this is not expected of any file.
fn mismatch() -> anyhow::Error {
    anyhow!("a TIFF chunk decoded as a different type from the image")
}

/// Everything the format allows onto the three sample types the rest of the
/// program works in, for a file read whole. See [`held_as`].
fn into_samples(result: DecodingResult, channels: Channels) -> Result<Samples> {
    Ok(match result {
        DecodingResult::U8(data) => Samples::U8 { channels, data },
        DecodingResult::U16(data) => Samples::U16 { channels, data },
        other => Samples::F32 {
            channels,
            data: widened(other),
        },
    })
}

/// Every type but the two kept as they are, as `f32`. `U8` and `U16` are
/// widened too, for the caller that has already ruled them out.
fn widened(result: DecodingResult) -> Vec<f32> {
    fn floats<T: Copy, F: Fn(T) -> f32>(data: Vec<T>, convert: F) -> Vec<f32> {
        data.into_iter().map(convert).collect()
    }

    match result {
        DecodingResult::U8(data) => floats(data, |v| v as f32),
        DecodingResult::U16(data) => floats(data, |v| v as f32),
        DecodingResult::F32(data) => data,
        DecodingResult::F16(data) => floats(data, |v| v.to_f32()),
        DecodingResult::F64(data) => floats(data, |v| v as f32),
        DecodingResult::I8(data) => floats(data, |v| v as f32),
        DecodingResult::I16(data) => floats(data, |v| v as f32),
        DecodingResult::I32(data) => floats(data, |v| v as f32),
        DecodingResult::U32(data) => floats(data, |v| v as f32),
        DecodingResult::I64(data) => floats(data, |v| v as f32),
        DecodingResult::U64(data) => floats(data, |v| v as f32),
    }
}

/// How a directory's pixels are cut into chunks, which of each pixel's
/// samples are kept, and where each lands.
struct Geometry {
    width: usize,
    height: usize,
    /// Samples per pixel kept: the channels of the page.
    samples: usize,
    /// Where the kept samples are in the directory's, and how they are
    /// stored there.
    layout: Layout,
    /// A chunk's size, before the right and bottom edges clip it: the tile
    /// size, or the image's width by the rows in a strip.
    chunk: (usize, usize),
    /// Chunks across a row of them, and rows of them down the image.
    across: usize,
    down: usize,
}

/// How the samples a page keeps are stored.
#[derive(Clone, Copy)]
enum Layout {
    /// Side by side in every chunk, `stride` samples a pixel, of which the
    /// page keeps those from `first`.
    Interleaved { stride: usize, first: usize },
    /// Each sample's chunks a plane of their own, the planes one after
    /// another; the page reads the planes from `first`.
    Planar { first: usize },
}

impl Geometry {
    fn of<R: io::Read + Seek>(
        decoder: &Decoder<R>,
        width: u32,
        height: u32,
        page: &Page,
        channels: Channels,
        planar: bool,
    ) -> Result<Self> {
        let (chunk_width, chunk_height) = decoder.chunk_dimensions();
        if chunk_width == 0 || chunk_height == 0 {
            bail!("TIFF chunks of {chunk_width}x{chunk_height}");
        }
        let (width, height) = (width as usize, height as usize);
        let chunk = (chunk_width as usize, chunk_height as usize);
        let samples = channels.count();
        // A directory read whole is the crate's layout: as many samples a
        // chunk's pixel as the page has, a palette's expanded already.
        let layout = match (page.run, planar) {
            (Some(run), true) => Layout::Planar { first: run.first },
            (Some(run), false) => Layout::Interleaved {
                stride: page.stride,
                first: run.first,
            },
            (None, _) => Layout::Interleaved {
                stride: samples,
                first: 0,
            },
        };
        if let Layout::Interleaved { stride, first } = layout
            && first + samples > stride
        {
            bail!("TIFF samples {first}.. of {stride} hold no {channels:?}");
        }
        Ok(Self {
            width,
            height,
            samples,
            layout,
            chunk,
            across: width.div_ceil(chunk.0),
            down: height.div_ceil(chunk.1),
        })
    }

    /// The pixel rows a run of chunk rows covers, clipped to the image.
    fn rows(&self, chunk_rows: Range<usize>) -> Range<usize> {
        (chunk_rows.start * self.chunk.1).min(self.height)
            ..(chunk_rows.end * self.chunk.1).min(self.height)
    }
}

/// What every thread reading a directory's chunks is told.
#[derive(Clone, Copy)]
struct Read<'a> {
    /// The file the other threads' decoders read, where there is one.
    shared: Option<&'a File>,
    limits: &'a Limits,
    /// Which directory of the chain the page is.
    directory: usize,
    geometry: &'a Geometry,
    /// The conversion each chunk gets on the way in, for a file that holds
    /// its pixels as YCbCr.
    ycbcr: Option<&'a YCbCr>,
}

/// The directory's samples, read a chunk at a time and each put where it
/// belongs, the rows of chunks divided between rayon's threads. Every thread
/// but the first opens a decoder of its own over `read.shared`; with no file
/// to share, or one row of chunks, the decoder in hand reads them all.
fn read_chunks<T: Resident>(
    decoder: &mut Decoder<&mut dyn super::ReadSeek>,
    read: &Read,
) -> Result<Vec<T>> {
    let geometry = read.geometry;
    let mut data = vec![T::default(); geometry.width * geometry.height * geometry.samples];
    let bands = match read.shared {
        Some(_) => geometry.down.min(rayon_core::current_num_threads()).max(1),
        None => 1,
    };
    if bands == 1 {
        read_rows(decoder, read, 0..geometry.down, &mut data)?;
        return Ok(data);
    }
    let file = read
        .shared
        .expect("more than one band means a file to share");

    // Whole chunk rows each, so rounding up the rows can leave fewer bands
    // than asked for; the count follows the rows, not the other way.
    let chunk_rows = geometry.down.div_ceil(bands);
    let bands = geometry.down.div_ceil(chunk_rows);
    let band_samples = geometry.rows(0..chunk_rows).len() * geometry.width * geometry.samples;
    let mut outcomes: Vec<Option<Result<()>>> = (0..bands).map(|_| None).collect();
    rayon_core::scope(|scope| {
        for ((index, band), outcome) in data
            .chunks_mut(band_samples)
            .enumerate()
            .zip(outcomes.iter_mut())
        {
            let first = index * chunk_rows;
            let last = (first + chunk_rows).min(geometry.down);
            scope.spawn(move |_| {
                *outcome = Some((|| {
                    let mut decoder = Decoder::new(BufReader::new(Positioned::new(file)))
                        .context("opening the file again for another thread")?
                        .with_limits(read.limits.clone());
                    if read.directory > 0 {
                        decoder.seek_to_image(read.directory)?;
                    }
                    read_rows(&mut decoder, read, first..last, band)
                })());
            });
        }
    });
    for outcome in outcomes {
        outcome.expect("every band was read")?;
    }
    Ok(data)
}

/// Reads the chunks of the chunk rows `chunk_rows` into `band`, which holds
/// exactly the pixel rows they cover.
fn read_rows<R: io::Read + Seek, T: Resident>(
    decoder: &mut Decoder<R>,
    read: &Read,
    chunk_rows: Range<usize>,
    band: &mut [T],
) -> Result<()> {
    let geometry = read.geometry;
    let top = geometry.rows(0..chunk_rows.start).len();
    let per_plane = geometry.across * geometry.down;
    for chunk_row in chunk_rows {
        for column in 0..geometry.across {
            let index = chunk_row * geometry.across + column;
            // The first plane's chunk: every plane is cut the same way.
            let (width, height) = decoder.chunk_data_dimensions(chunk_index(index)?);
            let target = Target {
                x: column * geometry.chunk.0,
                y: chunk_row * geometry.chunk.1 - top,
                width: width as usize,
                height: height as usize,
                row_samples: geometry.width * geometry.samples,
                samples: geometry.samples,
            };
            match geometry.layout {
                Layout::Interleaved { stride, first } => {
                    let mut chunk = T::take(decoder.read_chunk(chunk_index(index)?)?)?;
                    if let Some(ycbcr) = read.ycbcr {
                        T::to_rgb(&mut chunk, ycbcr);
                    }
                    target.put(band, &chunk, index, stride, first, 0, geometry.samples)?;
                }
                Layout::Planar { first } => {
                    for channel in 0..geometry.samples {
                        let index = (first + channel) * per_plane + index;
                        let chunk = T::take(decoder.read_chunk(chunk_index(index)?)?)?;
                        target.put(band, &chunk, index, 1, 0, channel, 1)?;
                    }
                }
            }
        }
    }
    Ok(())
}

/// A chunk's index as the crate takes it.
fn chunk_index(index: usize) -> Result<u32> {
    u32::try_from(index).context("too many TIFF chunks")
}

/// Where one chunk's pixels land in a band of rows.
struct Target {
    /// The chunk's first pixel, in the band.
    x: usize,
    y: usize,
    /// The pixels the chunk holds, its padding past the image's edges left
    /// out.
    width: usize,
    height: usize,
    /// Samples a row of the band, and a pixel of it.
    row_samples: usize,
    samples: usize,
}

impl Target {
    /// Copies `count` samples of each of the chunk's pixels, from its sample
    /// `first` of `stride`, into the band's from its channel `channel`.
    ///
    /// A chunk holding more rows than it has pixels for is read to the rows
    /// it has: the crate pads a planar file's last row of tiles in every
    /// plane but the first, working the padding out from the tile's index
    /// as though the planes were one.
    #[allow(clippy::too_many_arguments)]
    fn put<T: Copy>(
        &self,
        band: &mut [T],
        chunk: &[T],
        index: usize,
        stride: usize,
        first: usize,
        channel: usize,
        count: usize,
    ) -> Result<()> {
        let chunk_row = self.width * stride;
        if chunk_row == 0
            || !chunk.len().is_multiple_of(chunk_row)
            || chunk.len() / chunk_row < self.height
        {
            bail!(
                "TIFF chunk {index} decoded to {} samples, expected {} for {}x{}",
                chunk.len(),
                chunk_row * self.height,
                self.width,
                self.height
            );
        }
        for (row, source) in chunk.chunks_exact(chunk_row).take(self.height).enumerate() {
            let start = (self.y + row) * self.row_samples + self.x * self.samples;
            let destination = &mut band[start..start + self.width * self.samples];
            if stride == count && count == self.samples {
                destination.copy_from_slice(source);
                continue;
            }
            for (pixel, source) in destination
                .chunks_exact_mut(self.samples)
                .zip(source.chunks_exact(stride))
            {
                pixel[channel..channel + count].copy_from_slice(&source[first..first + count]);
            }
        }
        Ok(())
    }
}

/// How a file's YCbCr samples are turned back into RGB: the luma weights
/// the encoder used and the code values it put black and white at, both
/// from the file's own tags, with the defaults TIFF 6.0 gives them.
///
/// The arithmetic is section 21 of the specification, as libtiff does it:
/// each channel's code is first mapped by `ReferenceBlackWhite` onto the
/// full range — 0..255 for luma, -127..127 for chroma — and then the
/// weights undo the luma equation: `R = Y + Cr(2 - 2Kr)`, `B = Y + Cb(2 -
/// 2Kb)`, `G = (Y - Kr R - Kb B) / Kg`. The defaults are BT.601's weights
/// and the range JPEG itself codes in, black at 0 and neutral chroma at
/// 128, which is what nearly every file says outright too.
struct YCbCr {
    /// Every code value of luma, mapped onto the full range.
    luma: [f32; 256],
    /// Every code value of each chroma channel, mapped onto its range and
    /// already multiplied by that channel's weight in `R` or `B`.
    cb: [f32; 256],
    cr: [f32; 256],
    /// The weights of `R` and `B` in luma, and the reciprocal of green's,
    /// for finding `G` from the other two.
    kr: f32,
    kb: f32,
    kg_inverse: f32,
}

impl YCbCr {
    const DEFAULT_COEFFICIENTS: [f32; 3] = [0.299, 0.587, 0.114];
    const DEFAULT_REFERENCE: [f32; 6] = [0.0, 255.0, 128.0, 255.0, 128.0, 255.0];

    /// Reads the two tags off the directory in hand; a tag that is absent
    /// or malformed takes its default. Only the 8-bit case is built, since
    /// only 8-bit data reaches here.
    fn read<R: io::Read + Seek>(decoder: &mut Decoder<R>) -> Result<Self> {
        let coefficients = match decoder.get_tag_f32_vec(Tag::Unknown(YCBCR_COEFFICIENTS)) {
            Ok(values) if values.len() == 3 => [values[0], values[1], values[2]],
            _ => Self::DEFAULT_COEFFICIENTS,
        };
        let reference = match decoder.get_tag_f32_vec(Tag::Unknown(REFERENCE_BLACK_WHITE)) {
            Ok(values) if values.len() == 6 => {
                let mut reference = [0.0; 6];
                reference.copy_from_slice(&values);
                reference
            }
            _ => Self::DEFAULT_REFERENCE,
        };
        Self::new(coefficients, reference)
    }

    fn new(coefficients: [f32; 3], reference: [f32; 6]) -> Result<Self> {
        let [kr, kg, kb] = coefficients;
        if !(kr > 0.0 && kg > 0.0 && kb > 0.0) || !(kr + kg + kb).is_finite() {
            bail!("TIFF YCbCr coefficients {coefficients:?}");
        }
        // libtiff's `Code2V`: the code's distance from black, scaled from
        // the coded range onto the full one; a coded range of zero is taken
        // as one rather than dividing by it.
        let scale = |code: f32, black: f32, white: f32, full: f32| {
            let coded = if white == black { 1.0 } else { white - black };
            (code - black) * full / coded
        };
        let table = |f: &dyn Fn(f32) -> f32| std::array::from_fn(|code| f(code as f32));
        Ok(Self {
            luma: table(&|code| scale(code, reference[0], reference[1], 255.0)),
            cb: table(&|code| scale(code, reference[2], reference[3], 127.0) * (2.0 - 2.0 * kb)),
            cr: table(&|code| scale(code, reference[4], reference[5], 127.0) * (2.0 - 2.0 * kr)),
            kr,
            kb,
            kg_inverse: 1.0 / kg,
        })
    }

    /// Converts interleaved 8-bit YCbCr to RGB in place.
    fn to_rgb(&self, pixels: &mut [u8]) {
        let code = |value: f32| value.round().clamp(0.0, 255.0) as u8;
        for pixel in pixels.as_chunks_mut::<3>().0 {
            let y = self.luma[pixel[0] as usize];
            let r = y + self.cr[pixel[2] as usize];
            let b = y + self.cb[pixel[1] as usize];
            let g = (y - self.kr * r - self.kb * b) * self.kg_inverse;
            pixel[0] = code(r);
            pixel[1] = code(g);
            pixel[2] = code(b);
        }
    }
}

/// The way the directory says the picture is to be turned: the
/// `Orientation` tag, which a scanner or a camera writing TIFF sets and a
/// GIS leaves out. A value the tag does not define asks for nothing.
fn orientation<R: io::Read + Seek>(decoder: &mut Decoder<R>) -> Result<Orientation> {
    Ok(orient::from_tag(
        decoder.find_tag_unsigned::<u8>(Tag::Orientation)?,
    ))
}

/// TIFF is the awkward container: the same tags carry a scanned photograph and
/// a frame of sensor counts. An embedded profile settles it — a measurement
/// has none, and a picture out of Lightroom or Photoshop has the one it was
/// graded in, which is Adobe RGB or ProPhoto far more often than sRGB — and
/// is read through the same reader every other format's goes through, with
/// sRGB assumed for whatever it does not state. Without one, the layout is
/// the best signal available: 8-bit files are overwhelmingly pictures; so
/// are 16-bit files of three colors, which is what a scan or an editor's
/// export saved without its profile looks like, where a measurement is
/// nearly always one channel — an elevation, a mask, a frame of counts — or
/// floats, which nothing encodes a curve into. A file that says where its
/// pixels are on the ground, or which value means no data, is data unless
/// it is 8-bit color — an orthophoto, a rendered basemap, a scanned map,
/// which GIS tools write as three channels or a palette (the decoder hands
/// a palette over as three). One 8-bit channel there is a slope, a class or
/// a scaled index far more often than it is a hillshade. The *Photo* /
/// *Data* switch and `--transfer` override the guess when it is wrong.
fn color_space(
    samples: &Samples,
    profile: Option<tiff::decoder::ifd::Value>,
    georeferenced: bool,
) -> ColorSpace {
    if let Some(profile) = profile.and_then(|value| value.into_u8_vec().ok()) {
        return ColorSpace::stated(None, Some(&profile));
    }
    match samples {
        Samples::U8 { channels, .. }
            if !georeferenced || matches!(channels, Channels::Rgb | Channels::Rgba) =>
        {
            ColorSpace::SRGB
        }
        Samples::U16 { channels, .. }
            if matches!(channels, Channels::Rgb | Channels::Rgba) && !georeferenced =>
        {
            ColorSpace::SRGB
        }
        _ => ColorSpace::LINEAR_BT709,
    }
}

#[cfg(test)]
mod tests {
    use super::{Run, YCbCr, runs};
    use crate::image::Channels;

    fn run(first: usize, channels: Channels) -> Option<Run> {
        Some(Run { first, channels })
    }

    #[test]
    fn a_directory_of_gray_is_a_page_to_a_band() {
        // BlackIsZero, nine samples, eight of them unspecified.
        assert_eq!(
            runs(Some(1), 9, Some(&[0; 8])),
            (0..9)
                .map(|band| run(band, Channels::Gray))
                .collect::<Vec<_>>()
        );
        // Alpha first among the extras goes with the gray; the rest are bands.
        assert_eq!(
            runs(Some(1), 3, Some(&[2, 0])),
            [run(0, Channels::GrayAlpha), run(2, Channels::Gray)]
        );
        assert_eq!(runs(Some(0), 1, None), [run(0, Channels::Gray)]);
    }

    #[test]
    fn rgb_is_its_picture_whatever_is_beside_it() {
        assert_eq!(runs(Some(2), 3, None), [run(0, Channels::Rgb)]);
        assert_eq!(runs(Some(2), 4, Some(&[0])), [run(0, Channels::Rgb)]);
        assert_eq!(runs(Some(2), 5, Some(&[1, 0])), [run(0, Channels::Rgba)]);
    }

    #[test]
    fn one_extra_sample_with_no_tag_is_alpha() {
        assert_eq!(runs(Some(2), 4, None), [run(0, Channels::Rgba)]);
        assert_eq!(runs(Some(1), 2, None), [run(0, Channels::GrayAlpha)]);
        // Past one, untagged samples are bands.
        assert_eq!(
            runs(Some(1), 3, None),
            [
                run(0, Channels::Gray),
                run(1, Channels::Gray),
                run(2, Channels::Gray)
            ]
        );
    }

    #[test]
    fn everything_else_is_left_whole() {
        // Palette, YCbCr, CMYK, no interpretation at all, and RGB short of
        // its three samples.
        for photometric in [Some(3), Some(6), Some(5), None] {
            assert_eq!(runs(photometric, 3, None), [None], "{photometric:?}");
        }
        assert_eq!(runs(Some(2), 2, None), [None]);
    }

    fn rgb(ycbcr: &YCbCr, pixel: [u8; 3]) -> [u8; 3] {
        let mut pixel = pixel;
        ycbcr.to_rgb(&mut pixel);
        pixel
    }

    /// Every channel within a code value: the weights are quoted to three
    /// places, and a primary's chroma is 127 rather than the 127.5 that would
    /// land it exactly.
    fn near(actual: [u8; 3], expected: [u8; 3]) {
        for (a, e) in actual.iter().zip(expected) {
            assert!(
                a.abs_diff(e) <= 1,
                "{actual:?} is not within a code of {expected:?}"
            );
        }
    }

    #[test]
    fn defaults_are_jpegs_own_range() {
        let ycbcr = YCbCr::new(YCbCr::DEFAULT_COEFFICIENTS, YCbCr::DEFAULT_REFERENCE).unwrap();
        assert_eq!(rgb(&ycbcr, [0, 128, 128]), [0, 0, 0]);
        assert_eq!(rgb(&ycbcr, [128, 128, 128]), [128, 128, 128]);
        assert_eq!(rgb(&ycbcr, [255, 128, 128]), [255, 255, 255]);
        // BT.601's primaries, as JPEG codes them.
        near(rgb(&ycbcr, [76, 85, 255]), [255, 0, 0]);
        near(rgb(&ycbcr, [150, 44, 21]), [0, 255, 0]);
        near(rgb(&ycbcr, [29, 255, 107]), [0, 0, 255]);
    }

    #[test]
    fn reference_maps_the_coded_range_onto_the_full_one() {
        // Video range: luma from 16 to 235, chroma from 16 to 240 about 128.
        let reference = [16.0, 235.0, 128.0, 240.0, 128.0, 240.0];
        let ycbcr = YCbCr::new(YCbCr::DEFAULT_COEFFICIENTS, reference).unwrap();
        assert_eq!(rgb(&ycbcr, [16, 128, 128]), [0, 0, 0]);
        assert_eq!(rgb(&ycbcr, [235, 128, 128]), [255, 255, 255]);
        // Past white clips rather than wraps.
        assert_eq!(rgb(&ycbcr, [255, 128, 128]), [255, 255, 255]);
        near(rgb(&ycbcr, [81, 90, 240]), [255, 0, 0]);
    }

    #[test]
    fn coefficients_that_cannot_weigh_anything_are_refused() {
        assert!(YCbCr::new([0.0, 1.0, 0.0], YCbCr::DEFAULT_REFERENCE).is_err());
        assert!(YCbCr::new([0.3, f32::NAN, 0.1], YCbCr::DEFAULT_REFERENCE).is_err());
    }
}
