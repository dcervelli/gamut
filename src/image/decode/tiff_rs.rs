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
//! so a pyramid's reduced copies count as pages too.
//!
//! The pixels are stored in chunks — strips, or tiles — each compressed on
//! its own, and each is read here on its own too: the rows of chunks are
//! divided between rayon's threads, and every thread opens a decoder of its
//! own over the same file, since the crate's decoder reads through one
//! position. `read_image` would decode the chunks one after another, and a
//! 134-megapixel LZW map took 1.7 s that way on one core.

use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::ops::Range;

use anyhow::{Context, Result, anyhow, bail};
use tiff::decoder::{Decoder, DecodingResult, DecodingSampleType, Limits};
use tiff::tags::Tag;

use super::Positioned;
use crate::image::sequence::Sequence;
use crate::image::{AlphaMode, Channels, ColorSpace, DecodedImage, Samples};

/// GDAL writes the no-data value here, as an ASCII string.
const GDAL_NODATA: u16 = 42113;

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
        header.starts_with(b"II\x2a\x00")
            || header.starts_with(b"MM\x00\x2a")
            || header.starts_with(b"II\x2b\x00")
            || header.starts_with(b"MM\x00\x2b")
    }

    fn dimensions(&self, source: &mut dyn super::ReadSeek) -> Result<Option<(u32, u32)>> {
        Ok(Some(Decoder::new(source)?.dimensions()?))
    }

    fn decode(
        &self,
        source: &mut dyn super::ReadSeek,
        overrides: super::Overrides,
    ) -> Result<DecodedImage> {
        self.decode_page(source, overrides, 0)
    }

    /// How many directories the chain holds, walked without reading any
    /// pixels.
    fn sequence(&self, source: &mut dyn super::ReadSeek) -> Result<Sequence> {
        let mut decoder = Decoder::new(source)?;
        let mut count = 1;
        while decoder.more_images() {
            decoder.next_image()?;
            count += 1;
        }
        Ok(if count > 1 {
            Sequence::Pages { count, default: 0 }
        } else {
            Sequence::Still
        })
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
        if page > 0 {
            decoder.seek_to_image(page)?;
        }
        let (width, height) = decoder.dimensions()?;
        let color = decoder.colortype()?;

        let channels = match color {
            tiff::ColorType::Gray(_) => Channels::Gray,
            tiff::ColorType::GrayA(_) => Channels::GrayAlpha,
            // The decoder expands indexed data to RGB for us.
            tiff::ColorType::RGB(_) | tiff::ColorType::Palette(_) => Channels::Rgb,
            tiff::ColorType::RGBA(_) => Channels::Rgba,
            other => bail!("unsupported TIFF color type {other:?}"),
        };

        // Checked against what will be held rather than the stored depth:
        // every signed or wide type is widened to `f32` on the way in, up to
        // four times the size, and a signed 8-bit raster at the ceiling must
        // not balloon four times past it.
        let held = Held::of(decoder.image_chunk_buffer_layout(0)?.sample_type);
        super::check_decoded_size(width, height, channels.count(), held.bits())?;

        // Read before the pixels move the decoder's position.
        let nodata = decoder
            .get_tag_ascii_string(Tag::Unknown(GDAL_NODATA))
            .ok()
            .and_then(|value| value.trim().parse::<f32>().ok());

        // Chunk by chunk, across threads, wherever the layout is the plain
        // one. A planar file keeps each channel's chunks apart, which is
        // a different assembly and rare enough to leave to `read_image`.
        let chunky = decoder
            .find_tag_unsigned::<u16>(Tag::PlanarConfiguration)?
            .is_none_or(|planar| planar == 1);
        let samples = if chunky {
            let geometry = Geometry::of(&decoder, width, height, channels)?;
            match held {
                Held::U8 => Samples::U8 {
                    channels,
                    data: read_chunks(&mut decoder, shared.as_ref(), &limits, page, &geometry)?,
                },
                Held::U16 => Samples::U16 {
                    channels,
                    data: read_chunks(&mut decoder, shared.as_ref(), &limits, page, &geometry)?,
                },
                Held::F32 => Samples::F32 {
                    channels,
                    data: read_chunks(&mut decoder, shared.as_ref(), &limits, page, &geometry)?,
                },
            }
        } else {
            into_samples(decoder.read_image()?, channels)?
        };

        let expected = width as usize * height as usize * channels.count();
        if samples.len() != expected {
            return Err(anyhow!(
                "TIFF decoded {} samples, expected {expected} for {width}x{height} {channels:?}",
                samples.len()
            ));
        }

        let color = color_space(&samples);
        let mut image = DecodedImage::new(
            width,
            height,
            samples,
            color,
            AlphaMode::of(channels, false),
        );
        image.nodata = nodata;
        Ok(image)
    }
}

/// Which of the three sample types the rest of the program works in a
/// file's samples are held as: the stored type for unsigned 8- and 16-bit
/// data, and `f32` for everything else the format allows.
///
/// Signed and wide integer rasters become floats rather than being rescaled:
/// an elevation model holds meters, and -86 at the Dead Sea is a real value,
/// not something to normalize away. The display window is what turns them
/// into something visible.
#[derive(Clone, Copy)]
enum Held {
    U8,
    U16,
    F32,
}

impl Held {
    /// `None` is a depth or format the crate cannot name, which it will not
    /// decode either; `f32` keeps the size check honest until it says so.
    fn of(sample_type: Option<DecodingSampleType>) -> Self {
        match sample_type {
            Some(DecodingSampleType::U8) => Self::U8,
            Some(DecodingSampleType::U16) => Self::U16,
            _ => Self::F32,
        }
    }

    fn bits(self) -> u8 {
        match self {
            Self::U8 => 8,
            Self::U16 => 16,
            Self::F32 => 32,
        }
    }
}

/// A sample type a file is held as: what one decoded chunk becomes in it.
trait Resident: Copy + Default + Send {
    fn take(result: DecodingResult) -> Result<Vec<Self>>;
}

impl Resident for u8 {
    fn take(result: DecodingResult) -> Result<Vec<Self>> {
        match result {
            DecodingResult::U8(data) => Ok(data),
            _ => Err(mismatch()),
        }
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
/// program works in, for a file read whole. See [`Held`].
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

/// How a directory's pixels are cut into chunks, and where each lands.
struct Geometry {
    width: usize,
    height: usize,
    /// Samples per pixel.
    samples: usize,
    /// A chunk's size, before the right and bottom edges clip it: the tile
    /// size, or the image's width by the rows in a strip.
    chunk: (usize, usize),
    /// Chunks across a row of them, and rows of them down the image.
    across: usize,
    down: usize,
}

impl Geometry {
    fn of<R: Read + Seek>(
        decoder: &Decoder<R>,
        width: u32,
        height: u32,
        channels: Channels,
    ) -> Result<Self> {
        let (chunk_width, chunk_height) = decoder.chunk_dimensions();
        if chunk_width == 0 || chunk_height == 0 {
            bail!("TIFF chunks of {chunk_width}x{chunk_height}");
        }
        let (width, height) = (width as usize, height as usize);
        let chunk = (chunk_width as usize, chunk_height as usize);
        Ok(Self {
            width,
            height,
            samples: channels.count(),
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

/// The directory's samples, read a chunk at a time and each put where it
/// belongs, the rows of chunks divided between rayon's threads. Every thread
/// but the first opens a decoder of its own over `shared`; with no file to
/// share, or one row of chunks, the decoder in hand reads them all.
fn read_chunks<T: Resident>(
    decoder: &mut Decoder<&mut dyn super::ReadSeek>,
    shared: Option<&File>,
    limits: &Limits,
    page: usize,
    geometry: &Geometry,
) -> Result<Vec<T>> {
    let mut data = vec![T::default(); geometry.width * geometry.height * geometry.samples];
    let bands = match shared {
        Some(_) => geometry.down.min(rayon_core::current_num_threads()).max(1),
        None => 1,
    };
    if bands == 1 {
        read_rows(decoder, geometry, 0..geometry.down, &mut data)?;
        return Ok(data);
    }
    let file = shared.expect("more than one band means a file to share");

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
                        .with_limits(limits.clone());
                    if page > 0 {
                        decoder.seek_to_image(page)?;
                    }
                    read_rows(&mut decoder, geometry, first..last, band)
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
fn read_rows<R: Read + Seek, T: Resident>(
    decoder: &mut Decoder<R>,
    geometry: &Geometry,
    chunk_rows: Range<usize>,
    band: &mut [T],
) -> Result<()> {
    let row_samples = geometry.width * geometry.samples;
    let top = geometry.rows(0..chunk_rows.start).len();
    for chunk_row in chunk_rows {
        for column in 0..geometry.across {
            let index = chunk_row * geometry.across + column;
            let index = u32::try_from(index).context("too many TIFF chunks")?;
            let (chunk_width, chunk_height) = decoder.chunk_data_dimensions(index);
            let (chunk_width, chunk_height) = (chunk_width as usize, chunk_height as usize);
            let chunk = T::take(decoder.read_chunk(index)?)?;
            let chunk_samples = chunk_width * geometry.samples;
            if chunk.len() != chunk_samples * chunk_height {
                bail!(
                    "TIFF chunk {index} decoded to {} samples, expected {} for {chunk_width}x{chunk_height}",
                    chunk.len(),
                    chunk_samples * chunk_height
                );
            }
            let x = column * geometry.chunk.0 * geometry.samples;
            let y = chunk_row * geometry.chunk.1 - top;
            for (row, source) in chunk.chunks_exact(chunk_samples).enumerate() {
                let start = (y + row) * row_samples + x;
                band[start..start + chunk_samples].copy_from_slice(source);
            }
        }
    }
    Ok(())
}

/// TIFF is the awkward container: the same tags carry a scanned photograph and
/// a frame of sensor counts. Bit depth is the best signal available — 8-bit
/// TIFFs are overwhelmingly pictures, deeper ones overwhelmingly measurements
/// — and `--transfer` overrides it when the guess is wrong.
fn color_space(samples: &Samples) -> ColorSpace {
    match samples {
        Samples::U8 { .. } => ColorSpace::SRGB,
        _ => ColorSpace::LINEAR_BT709,
    }
}
