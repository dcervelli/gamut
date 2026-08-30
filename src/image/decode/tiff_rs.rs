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

use anyhow::{Result, anyhow, bail};
use tiff::decoder::{Decoder, DecodingResult, Limits};
use tiff::tags::Tag;

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
        _overrides: super::Overrides,
    ) -> Result<DecodedImage> {
        // `Limits` is non-exhaustive, so start from the defaults and raise
        // only what needs raising.
        let mut limits = Limits::default();
        limits.decoding_buffer_size = super::MAX_DECODED_BYTES as usize;
        limits.intermediate_buffer_size = 256 * 1024 * 1024;
        limits.ifd_value_size = MAX_IFD_VALUE_BYTES;

        let mut decoder = Decoder::new(source)?.with_limits(limits);
        let (width, height) = decoder.dimensions()?;
        let color = decoder.colortype()?;

        let channels = match color {
            tiff::ColorType::Gray(_) => Channels::Gray,
            tiff::ColorType::GrayA(_) => Channels::GrayAlpha,
            // The decoder expands indexed data to RGB for us.
            tiff::ColorType::RGB(_) | tiff::ColorType::Palette(_) => Channels::Rgb,
            tiff::ColorType::RGBA(_) => Channels::Rgba,
            other => bail!("unsupported TIFF colour type {other:?}"),
        };

        super::check_decoded_size(width, height, channels.count(), bit_depth(color))?;

        // Read before `read_image` consumes the decoder's position.
        let nodata = decoder
            .get_tag_ascii_string(Tag::Unknown(GDAL_NODATA))
            .ok()
            .and_then(|value| value.trim().parse::<f32>().ok());

        let samples = into_samples(decoder.read_image()?, channels)?;

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

/// Widens everything the format allows onto the three sample types the rest of
/// the program works in.
///
/// Signed and wide integer rasters become floats rather than being rescaled:
/// an elevation model holds metres, and -86 at the Dead Sea is a real value,
/// not something to normalise away. The display window is what turns them into
/// something visible.
fn into_samples(result: DecodingResult, channels: Channels) -> Result<Samples> {
    fn floats<T: Copy, F: Fn(T) -> f32>(data: Vec<T>, convert: F) -> Vec<f32> {
        data.into_iter().map(convert).collect()
    }

    Ok(match result {
        DecodingResult::U8(data) => Samples::U8 { channels, data },
        DecodingResult::U16(data) => Samples::U16 { channels, data },
        DecodingResult::F32(data) => Samples::F32 { channels, data },
        DecodingResult::F16(data) => Samples::F32 {
            channels,
            data: floats(data, |v| v.to_f32()),
        },
        DecodingResult::F64(data) => Samples::F32 {
            channels,
            data: floats(data, |v| v as f32),
        },
        DecodingResult::I8(data) => Samples::F32 {
            channels,
            data: floats(data, |v| v as f32),
        },
        DecodingResult::I16(data) => Samples::F32 {
            channels,
            data: floats(data, |v| v as f32),
        },
        DecodingResult::I32(data) => Samples::F32 {
            channels,
            data: floats(data, |v| v as f32),
        },
        DecodingResult::U32(data) => Samples::F32 {
            channels,
            data: floats(data, |v| v as f32),
        },
        DecodingResult::I64(data) => Samples::F32 {
            channels,
            data: floats(data, |v| v as f32),
        },
        DecodingResult::U64(data) => Samples::F32 {
            channels,
            data: floats(data, |v| v as f32),
        },
    })
}

fn bit_depth(color: tiff::ColorType) -> u8 {
    use tiff::ColorType::*;
    match color {
        Gray(bits) | RGB(bits) | Palette(bits) | GrayA(bits) | RGBA(bits) | CMYK(bits)
        | CMYKA(bits) | YCbCr(bits) | Lab(bits) => bits,
        Multiband { bit_depth, .. } => bit_depth,
        _ => 8,
    }
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
