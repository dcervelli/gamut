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

use super::{Positioned, orient};
use crate::image::sequence::Sequence;
use crate::image::{AlphaMode, Channels, ColorSpace, DecodedImage, Samples};

/// GDAL writes the no-data value here, as an ASCII string.
const GDAL_NODATA: u16 = 42113;

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
        header.starts_with(b"II\x2a\x00")
            || header.starts_with(b"MM\x00\x2a")
            || header.starts_with(b"II\x2b\x00")
            || header.starts_with(b"MM\x00\x2b")
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

    /// How many pictures the chain holds, walked without reading any
    /// pixels.
    fn sequence(&self, source: &mut dyn super::ReadSeek) -> Result<Sequence> {
        let count = pages(&mut Decoder::new(source)?)?.len();
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
        // The first page is the first directory, whatever it says of itself;
        // any other is found past the masks.
        let directory = match page {
            0 => 0,
            _ => *pages(&mut decoder)?
                .get(page)
                .ok_or_else(|| anyhow!("TIFF has no page {page}"))?,
        };
        if directory > 0 {
            decoder.seek_to_image(directory)?;
        }
        let (width, height) = decoder.dimensions()?;
        let color = decoder.colortype()?;

        let channels = match color {
            tiff::ColorType::Gray(_) => Channels::Gray,
            tiff::ColorType::GrayA(_) => Channels::GrayAlpha,
            // The decoder expands indexed data to RGB for us; YCbCr it
            // leaves to us, and only 8-bit is ever seen, since JPEG is the
            // compression that carries it.
            tiff::ColorType::RGB(_) | tiff::ColorType::Palette(_) | tiff::ColorType::YCbCr(8) => {
                Channels::Rgb
            }
            tiff::ColorType::RGBA(_) => Channels::Rgba,
            other => bail!("unsupported TIFF color type {other:?}"),
        };
        let ycbcr = match color {
            tiff::ColorType::YCbCr(_) => Some(YCbCr::read(&mut decoder)?),
            _ => None,
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
        let orientation = orientation(&mut decoder)?;
        let profile = decoder.find_tag(Tag::IccProfile)?;

        // Chunk by chunk, across threads, wherever the layout is the plain
        // one. A planar file keeps each channel's chunks apart, which is
        // a different assembly and rare enough to leave to `read_image`.
        let chunky = decoder
            .find_tag_unsigned::<u16>(Tag::PlanarConfiguration)?
            .is_none_or(|planar| planar == 1);
        let samples = if chunky {
            let geometry = Geometry::of(&decoder, width, height, channels)?;
            let read = Read {
                shared: shared.as_ref(),
                limits: &limits,
                directory,
                geometry: &geometry,
                ycbcr: ycbcr.as_ref(),
            };
            match held {
                Held::U8 => Samples::U8 {
                    channels,
                    data: read_chunks(&mut decoder, &read)?,
                },
                Held::U16 => Samples::U16 {
                    channels,
                    data: read_chunks(&mut decoder, &read)?,
                },
                Held::F32 => Samples::F32 {
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

        let color = color_space(&samples, profile);
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

/// The directories that are pictures, in order, by index in the chain: every
/// one but a transparency mask, which is the coverage of the picture before
/// it rather than a page of its own — and one bit a pixel, which the crate
/// would not decode as a picture anyway. The decoder is left on the last
/// directory; `seek_to_image` finds any of them again from there.
fn pages<R: io::Read + Seek>(decoder: &mut Decoder<R>) -> Result<Vec<usize>> {
    let mut pages = Vec::new();
    let mut index = 0;
    loop {
        let subfile = decoder
            .find_tag_unsigned::<u32>(Tag::NewSubfileType)?
            .unwrap_or(0);
        if subfile & TRANSPARENCY_MASK == 0 {
            pages.push(index);
        }
        if !decoder.more_images() {
            break;
        }
        decoder.next_image()?;
        index += 1;
    }
    Ok(pages)
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
    fn of<R: io::Read + Seek>(
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
    let row_samples = geometry.width * geometry.samples;
    let top = geometry.rows(0..chunk_rows.start).len();
    for chunk_row in chunk_rows {
        for column in 0..geometry.across {
            let index = chunk_row * geometry.across + column;
            let index = u32::try_from(index).context("too many TIFF chunks")?;
            let (chunk_width, chunk_height) = decoder.chunk_data_dimensions(index);
            let (chunk_width, chunk_height) = (chunk_width as usize, chunk_height as usize);
            let mut chunk = T::take(decoder.read_chunk(index)?)?;
            if let Some(ycbcr) = read.ycbcr {
                T::to_rgb(&mut chunk, ycbcr);
            }
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
    Ok(decoder
        .find_tag_unsigned::<u8>(Tag::Orientation)?
        .and_then(Orientation::from_exif)
        .unwrap_or(Orientation::NoTransforms))
}

/// TIFF is the awkward container: the same tags carry a scanned photograph and
/// a frame of sensor counts. An embedded profile settles it — a measurement
/// has none, and a picture out of Lightroom or Photoshop has the one it was
/// graded in, which is Adobe RGB or ProPhoto far more often than sRGB — and
/// is read through the same reader every other format's goes through, with
/// sRGB assumed for whatever it does not state. Without one, bit depth is
/// the best signal available — 8-bit TIFFs are overwhelmingly pictures,
/// deeper ones overwhelmingly measurements — and `--transfer` overrides it
/// when the guess is wrong.
fn color_space(samples: &Samples, profile: Option<tiff::decoder::ifd::Value>) -> ColorSpace {
    if let Some(profile) = profile.and_then(|value| value.into_u8_vec().ok()) {
        return crate::image::color::icc::color_space(&profile, ColorSpace::SRGB);
    }
    match samples {
        Samples::U8 { .. } => ColorSpace::SRGB,
        _ => ColorSpace::LINEAR_BT709,
    }
}

#[cfg(test)]
mod tests {
    use super::YCbCr;

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
