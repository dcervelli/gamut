//! The image as it is on screen, written out as a PNG.
//!
//! The picture that travels is the one the display settings have made — the
//! window, the exposure, the tone curve, the false color — at the image's own
//! size rather than the window's. So this is not a screenshot: it is the
//! rendering pipeline run again on the CPU, over every pixel instead of the
//! one under the pointer.
//!
//! It is run through [`DecodedImage::sample`] and [`Display::map`] for exactly
//! that reason. They are the twins the readout in the bottom bar already uses,
//! kept in step with `shaders/image.wgsl` and `shaders/composite.wgsl`
//! deliberately, so what is copied cannot drift from what was on screen
//! without the readout drifting with it and giving the game away.
//!
//! What comes out is 8-bit sRGB, which is what the screen was showing and what
//! anything receiving a PNG expects. The tone curve has already decided what
//! becomes of anything brighter than white.
//!
//! Running a per-pixel pipeline over every pixel is the slow way round, and it
//! is sped up only in ways that cannot change the answer: the sRGB curve on
//! the way out is a table rather than a `powf` ([`levels`]), and the rows are
//! divided between threads, which changes who does the arithmetic and not what
//! it is.

use std::num::NonZero;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use png::{BitDepth, ColorType, Compression, Encoder, SrgbRenderingIntent};

use super::display::{Colormap, Display, Headroom};
use super::{Channels, DecodedImage, Region, Transfer};

/// Pixels below which the walk is not worth dividing: the threads cost more to
/// start than they save on a picture this small.
const PARALLEL_FROM: usize = 1 << 16;

/// A displayed image reduced to the bytes a PNG stores.
pub struct Raster {
    pub width: u32,
    pub height: u32,
    /// What each pixel carries here, which is not always what the file
    /// carried: see [`displayed`].
    pub channels: Channels,
    /// Row-major from the top, tightly packed, one byte per component.
    pub data: Vec<u8>,
}

/// Runs the display pipeline over every pixel of `image` inside `region`:
/// the whole of it, as [`Region::whole`] says, or the part that was
/// selected. What comes out is the region's own size, its top-left pixel
/// first, so a copy of a selection is a crop of the copy of the picture and
/// not a different rendering of it.
///
/// A single-channel image stays single-channel, because that is what it is and
/// storing the same number three times says nothing more. False color is the
/// exception: it turns one value into a color on purpose, and a gray PNG
/// could not hold the result.
///
/// Alpha is carried only where the file had some. An image that was opaque
/// stays opaque rather than gaining a channel of nothing but 255.
pub fn displayed(image: &DecodedImage, display: &Display, region: Region) -> Raster {
    let source = image.channels();
    let gray = source.is_gray() && display.colormap == Colormap::Gray;
    let alpha = source.alpha_index().is_some();
    let channels = match (gray, alpha) {
        (true, false) => Channels::Gray,
        (true, true) => Channels::GrayAlpha,
        (false, false) => Channels::Rgb,
        (false, true) => Channels::Rgba,
    };

    let stride = region.width as usize * channels.count();
    let height = region.height as usize;
    let mut data = vec![0u8; stride * height];

    // Divided by rows. Every pixel is decided by the file and the display
    // state alone, so a band reads nothing another band writes and needs
    // nothing from it; the split is over who does the work, not over what
    // the work is.
    let bands = bands(stride, height);
    if bands == 1 {
        fill(&mut data, region.y, image, display, channels, region);
    } else {
        let rows = height.div_ceil(bands);
        std::thread::scope(|scope| {
            for (index, band) in data.chunks_mut(stride * rows).enumerate() {
                let first = region.y + (index * rows) as u32;
                scope.spawn(move || fill(band, first, image, display, channels, region));
            }
        });
    }

    Raster {
        width: region.width,
        height: region.height,
        channels,
        data,
    }
}

/// How many ways to split the walk. One for a picture too small to be worth
/// the threads, and never more bands than there are rows to put in them.
fn bands(stride: usize, height: usize) -> usize {
    if stride == 0 || height == 0 || stride * height < PARALLEL_FROM {
        return 1;
    }
    std::thread::available_parallelism()
        .map_or(1, NonZero::get)
        .min(height)
}

/// Writes the rows of `band`, which start at row `first` of the image and
/// run across the columns `region` takes in.
fn fill(
    band: &mut [u8],
    first: u32,
    image: &DecodedImage,
    display: &Display,
    channels: Channels,
    region: Region,
) {
    let levels = levels();
    let count = channels.count();
    let gray = channels.is_gray();
    let stride = region.width as usize * count;
    for (offset, row) in band.chunks_exact_mut(stride).enumerate() {
        let y = first + offset as u32;
        for (column, pixel) in row.chunks_exact_mut(count).enumerate() {
            // Only `None` outside the image, which this walk never goes.
            let Some(sample) = image.sample(region.x + column as u32, y) else {
                continue;
            };
            // An SDR reading: a PNG stops at white, so what is copied is the
            // picture as an SDR surface shows it, whatever the window is on.
            let mapped = display.map(&sample, Headroom::None);
            if gray {
                // Gray reaches the screen as the same number in all three, so
                // any one of them is the whole of it.
                pixel[0] = quantize(mapped.color[0], levels);
            } else {
                for (slot, value) in pixel.iter_mut().zip(mapped.color) {
                    *slot = quantize(value, levels);
                }
            }
            if let Some(index) = channels.alpha_index() {
                pixel[index] = byte(mapped.alpha);
            }
        }
    }
}

/// `raster` as the bytes of a PNG file.
///
/// Compressed with [`Compression::Fast`], which is the trade this particular
/// file wants: it is made between a key going down and a paste being possible,
/// and it is not being kept, so the time a smaller one would cost is time the
/// window spends not answering.
pub fn png(raster: &Raster) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut encoder = Encoder::new(&mut bytes, raster.width, raster.height);
    encoder.set_color(match raster.channels {
        Channels::Gray => ColorType::Grayscale,
        Channels::GrayAlpha => ColorType::GrayscaleAlpha,
        Channels::Rgb => ColorType::Rgb,
        Channels::Rgba => ColorType::Rgba,
    });
    encoder.set_depth(BitDepth::Eight);
    encoder.set_compression(Compression::Fast);
    // Which brings `Filter::Adaptive` with it, and it is left to. Fixing the
    // filter instead is faster on some pictures — measured over 12 megapixels
    // here, Paeth alone matched Adaptive's size in three quarters of its time
    // — but only because Adaptive was choosing Paeth for that content anyway.
    // On content it would have chosen differently, the same fixed filter cost
    // half as much again in bytes, and the bytes are what the paste waits on.
    // Said outright rather than left to be assumed. Whatever the file's own
    // color space was, the window and the tone curve have taken it to what
    // the screen was showing, and that was resolved against sRGB.
    encoder.set_source_srgb(SrgbRenderingIntent::Perceptual);

    let mut writer = encoder.write_header().context("writing the PNG header")?;
    writer
        .write_image_data(&raster.data)
        .context("writing the PNG pixels")?;
    writer.finish().context("finishing the PNG")?;
    Ok(bytes)
}

/// Where each output byte begins, in linear light: `levels()[i]` is the
/// smallest value that rounds to byte `i + 1`.
///
/// Read the other way round, this is the sRGB curve as a table. Only 256
/// answers exist, so rather than run `to_encoded` on every component of every
/// pixel and round what comes back, ask which of the 256 the value falls in.
///
/// This is not an approximation of the curve: the thresholds are
/// [`Transfer::to_linear`] of the very midpoints the rounding would have
/// used, so what comes back is always the nearest code. It parts from running
/// the curve forwards only where a value lands exactly between two codes and
/// the two ways round the tie fall on either side of it — twice in a sweep of
/// a million, and by the one code that was a coin toss to begin with.
fn levels() -> &'static [f32; 255] {
    static LEVELS: OnceLock<[f32; 255]> = OnceLock::new();
    LEVELS.get_or_init(|| {
        std::array::from_fn(|index| Transfer::Srgb.to_linear((index as f32 + 0.5) / 255.0))
    })
}

/// One linear working-space component as the byte a PNG stores.
///
/// The sRGB curve is applied here and nowhere earlier: everything upstream of
/// this works in linear light, which is the invariant `render::upload` states
/// and the shaders rely on.
fn quantize(linear: f32, levels: &[f32; 255]) -> u8 {
    // How many thresholds the value has passed is the code it lands on, which
    // clamps both ends by itself: nothing under the first is 0, everything
    // over the last is 255. A NaN passes none of them and comes out 0, which
    // is where the arithmetic form put it too.
    levels.partition_point(|&level| level <= linear) as u8
}

/// A 0..1 fraction rounded to a byte. Coverage comes through here untouched by
/// any curve, which is what alpha means in a PNG and on the way to the screen
/// alike.
fn byte(unit: f32) -> u8 {
    // A NaN clamps to NaN rather than to an end, and casts to 0 rather than
    // wrapping; it can only arise from a sample that was already NaN.
    (unit.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use ::png::Decoder;

    use super::*;
    use crate::image::display::ToneMap;
    use crate::image::{AlphaMode, ColorSpace, Primaries, Samples};

    /// An sRGB image of `channels`, one row wide, from raw 8-bit components.
    fn image(channels: Channels, data: Vec<u8>) -> DecodedImage {
        DecodedImage::new(
            (data.len() / channels.count()) as u32,
            1,
            Samples::U8 { channels, data },
            ColorSpace {
                transfer: Transfer::Srgb,
                primaries: Primaries::Bt709,
            },
            if channels.alpha_index().is_some() {
                AlphaMode::Straight
            } else {
                AlphaMode::Opaque
            },
        )
    }

    /// The display as it starts for an already-graded image: 0..1, no
    /// exposure, nothing to tone map.
    fn plain() -> Display {
        Display::default()
    }

    /// The whole of `image`, walked.
    fn displayed(image: &DecodedImage, display: &Display) -> Raster {
        super::displayed(image, display, Region::whole([image.width, image.height]))
    }

    /// `(color type, bit depth, pixel bytes)` as a PNG decoder reads them
    /// back, which is the only reading of the file that matters.
    fn round_trip(raster: &Raster) -> (ColorType, BitDepth, Vec<u8>) {
        let bytes = super::png(raster).expect("a valid raster encodes");
        let mut reader = Decoder::new(Cursor::new(&bytes)).read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut pixels).unwrap();
        pixels.truncate(info.buffer_size());
        (info.color_type, info.bit_depth, pixels)
    }

    #[test]
    fn a_gray_image_stays_one_channel() {
        let raster = displayed(&image(Channels::Gray, vec![0, 128, 255]), &plain());
        assert_eq!(raster.channels, Channels::Gray);
        let (color, depth, pixels) = round_trip(&raster);
        assert_eq!((color, depth), (ColorType::Grayscale, BitDepth::Eight));
        // Out the far side as it went in: an sRGB image shown unwindowed is
        // decoded to linear and encoded back again, and neither step moves it.
        assert_eq!(pixels, vec![0, 128, 255]);
    }

    /// False color is three components where the value was one, so the gray
    /// cannot be kept — this is the one thing that widens a single channel.
    #[test]
    fn false_color_makes_a_gray_image_color() {
        let mut display = plain();
        display.colormap = Colormap::Viridis;
        let raster = displayed(&image(Channels::Gray, vec![0, 255]), &display);
        assert_eq!(raster.channels, Channels::Rgb);
        let (color, _, pixels) = round_trip(&raster);
        assert_eq!(color, ColorType::Rgb);
        // Viridis runs dark blue-purple to bright yellow; whatever the exact
        // codes, the two ends are not gray and not each other.
        assert_eq!(pixels.len(), 6);
        assert!(pixels[2] > pixels[0], "the low end is blue: {pixels:?}");
        assert!(pixels[3] > pixels[5], "the high end is yellow: {pixels:?}");
    }

    /// An image that was opaque does not gain a channel of nothing but 255.
    #[test]
    fn alpha_is_carried_only_where_the_file_had_it() {
        let opaque = displayed(&image(Channels::Rgb, vec![10, 20, 30]), &plain());
        assert_eq!(opaque.channels, Channels::Rgb);
        assert_eq!(round_trip(&opaque).0, ColorType::Rgb);

        let with_alpha = displayed(&image(Channels::Rgba, vec![10, 20, 30, 128]), &plain());
        assert_eq!(with_alpha.channels, Channels::Rgba);
        let (color, _, pixels) = round_trip(&with_alpha);
        assert_eq!(color, ColorType::Rgba);
        // Coverage takes no curve on the way through.
        assert_eq!(pixels[3], 128);

        let gray_alpha = displayed(&image(Channels::GrayAlpha, vec![200, 64]), &plain());
        assert_eq!(gray_alpha.channels, Channels::GrayAlpha);
        assert_eq!(round_trip(&gray_alpha).0, ColorType::GrayscaleAlpha);
    }

    /// The whole point: what is copied is what the display settings made, not
    /// what the file holds.
    #[test]
    fn the_display_settings_are_baked_in() {
        let source = image(Channels::Gray, vec![128]);
        let plain_byte = round_trip(&displayed(&source, &plain())).2[0];

        let mut brighter = plain();
        brighter.exposure_stops = 1.0;
        let brighter_byte = round_trip(&displayed(&source, &brighter)).2[0];
        assert!(
            brighter_byte > plain_byte,
            "a stop up should lighten: {plain_byte} → {brighter_byte}"
        );

        let mut windowed = plain();
        windowed.low = 0.0;
        windowed.high = 0.1;
        let windowed_byte = round_trip(&displayed(&source, &windowed)).2[0];
        assert_eq!(windowed_byte, 255, "a value over the window clips to white");
    }

    /// A tone curve is a display setting like any other, and travels with the
    /// picture rather than being left behind on the screen.
    #[test]
    fn the_tone_curve_travels_with_the_picture() {
        // Float samples, so there is something above white to map down.
        let bright = DecodedImage::new(
            1,
            1,
            Samples::F32 {
                channels: Channels::Gray,
                data: vec![4.0],
            },
            ColorSpace {
                transfer: Transfer::Linear,
                primaries: Primaries::Bt709,
            },
            AlphaMode::Opaque,
        );

        let mut clipped = plain();
        clipped.tone_map = ToneMap::None;
        assert_eq!(round_trip(&displayed(&bright, &clipped)).2[0], 255);

        let mut rolled = plain();
        rolled.tone_map = ToneMap::Reinhard;
        let rolled_byte = round_trip(&displayed(&bright, &rolled)).2[0];
        assert!(
            rolled_byte < 255,
            "Reinhard should pull 4.0 back under white, got {rolled_byte}"
        );
    }

    /// The table is meant to be the sRGB curve and not a likeness of it, so
    /// what it returns has to be the nearest of the 256 codes to where the
    /// curve actually puts the value — never a code out, which would be a
    /// copy quietly a shade off what was on screen.
    #[test]
    fn the_level_table_returns_the_nearest_code() {
        let levels = levels();
        for step in 0..=200_000u32 {
            let linear = step as f32 / 200_000.0;
            let code = quantize(linear, levels);
            let exact = Transfer::Srgb.to_encoded(linear) * 255.0;
            // Half a code is the whole of the allowance, and the slack on top
            // of it is float error at a boundary rather than room to be wrong.
            assert!(
                (exact - f32::from(code)).abs() <= 0.5 + 1e-3,
                "{linear} encodes to {exact}, which is not code {code}"
            );
        }

        // The 256 codes of an 8-bit sRGB file, which is the case that has to
        // be exact: shown unwindowed, every one of them comes back itself.
        for code in 0..=u8::MAX {
            let linear = Transfer::Srgb.to_linear(f32::from(code) / 255.0);
            assert_eq!(quantize(linear, levels), code);
        }

        // Both ends clamp by themselves, having either no threshold passed or
        // every one of them.
        assert_eq!(quantize(-1.0, levels), 0);
        assert_eq!(quantize(4.0, levels), 255);
        assert_eq!(quantize(f32::NAN, levels), 0);
    }

    /// Dividing the walk between threads must not move a single pixel of it.
    /// Checked against the same pipeline written out in the obvious way, one
    /// pixel after another on one thread, over an image large enough that
    /// `displayed` really does split it.
    #[test]
    fn a_divided_walk_agrees_with_a_plain_one() {
        let (width, height) = (320u32, 256u32);
        assert!(
            (width * height) as usize >= PARALLEL_FROM,
            "the test image has to be big enough to be split"
        );
        let data = (0..width * height * 4)
            .map(|index| (index % 251) as u8)
            .collect();
        let mut source = image(Channels::Rgba, data);
        source.width = width;
        source.height = height;

        let mut display = plain();
        display.exposure_stops = 0.7;
        display.high = 0.8;

        let mut expected = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let mapped = display.map(&source.sample(x, y).unwrap(), Headroom::None);
                for value in mapped.color {
                    expected.push(quantize(value, levels()));
                }
                expected.push(byte(mapped.alpha));
            }
        }

        assert!(
            bands(width as usize * 4, height as usize) > 1,
            "the walk has to actually be divided for this to be testing anything"
        );
        assert_eq!(displayed(&source, &display).data, expected);
    }

    /// Sizes come from the image, not from the window it is being viewed in.
    #[test]
    fn the_png_is_the_size_of_the_image() {
        let mut source = image(Channels::Rgb, vec![0; 3 * 12]);
        source.width = 4;
        source.height = 3;
        let raster = displayed(&source, &plain());
        assert_eq!((raster.width, raster.height), (4, 3));
        assert_eq!(raster.data.len(), 4 * 3 * 3);
    }

    /// A region comes out as exactly the crop of the whole: the same pixels
    /// through the same pipeline, and none of the others. Over an image
    /// large enough to be divided between threads, so that the bands are
    /// offset by the region's own rows and not the image's.
    #[test]
    fn a_region_is_a_crop_of_the_whole() {
        let (width, height) = (320u32, 256u32);
        let data = (0..width * height * 3)
            .map(|index| (index % 253) as u8)
            .collect();
        let mut source = image(Channels::Rgb, data);
        source.width = width;
        source.height = height;
        let mut display = plain();
        display.exposure_stops = 0.4;

        let whole = displayed(&source, &display);
        let region = Region {
            x: 17,
            y: 40,
            width: 200,
            height: 190,
        };
        assert!(
            bands(region.width as usize * 3, region.height as usize) > 1,
            "the region has to be divided for the offset to be tested"
        );
        let part = super::displayed(&source, &display, region);
        assert_eq!((part.width, part.height), (200, 190));

        let mut expected = Vec::new();
        for y in region.y..region.bottom() {
            let row = y as usize * width as usize * 3;
            let from = row + region.x as usize * 3;
            expected.extend_from_slice(&whole.data[from..from + region.width as usize * 3]);
        }
        assert_eq!(part.data, expected);
    }
}
