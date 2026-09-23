//! The image as it is on screen, written out as a PNG or a JPEG: for the
//! clipboard, for the thumbnail cache, and for a file exported.
//!
//! The picture that travels is the one the display settings have made — the
//! window, the exposure, the tone curve, the false color, and the turn it is
//! shown at — at the image's own size rather than the window's. So this is not a screenshot: it is the
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

use super::gain_map::Table;

use anyhow::{Context, Result};
use png::{BitDepth, ColorType, Compression, Encoder, SrgbRenderingIntent};

use super::display::{Colormap, Display, Headroom};
use super::orient::Turn;
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
/// selected. `image` is as the file holds it and `region` is in the picture
/// as `turn` shows it, which is also how the raster comes out: the turn is
/// read through pixel by pixel, as the screen reads it. What comes out is the region's own size, its top-left pixel
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
pub fn displayed(
    image: &DecodedImage,
    display: &Display,
    turn: Turn,
    region: Region,
    lift: Option<&Table>,
) -> Raster {
    let stride = region.width as usize * displayed_channels(image, display).count();
    displayed_on(
        image,
        display,
        turn,
        region,
        lift,
        bands(stride, region.height as usize),
    )
}

/// [`displayed`], divided between `bands` threads — or kept on one, for a
/// caller that is already on a thread of its own and has no business
/// taking the others: the thumbnailer, which walks a picture no larger
/// than [`crate::thumbnail::SIDE`] a side and wants to stay out of the
/// decoder's way. `displayed` itself picks the count from the picture.
pub fn displayed_on(
    image: &DecodedImage,
    display: &Display,
    turn: Turn,
    region: Region,
    lift: Option<&Table>,
    bands: usize,
) -> Raster {
    let channels = displayed_channels(image, display);
    let stride = region.width as usize * channels.count();
    let height = region.height as usize;
    let mut data = vec![0u8; stride * height];

    // Divided by rows. Every pixel is decided by the file and the display
    // state alone, so a band reads nothing another band writes and needs
    // nothing from it; the split is over who does the work, not over what
    // the work is.
    let bands = bands.clamp(1, height.max(1));
    let walk = Walk {
        image,
        display,
        channels,
        turn,
        region,
        lift,
    };
    if bands == 1 {
        fill(&mut data, region.y, &walk);
    } else {
        let rows = height.div_ceil(bands);
        std::thread::scope(|scope| {
            for (index, band) in data.chunks_mut(stride * rows).enumerate() {
                let first = region.y + (index * rows) as u32;
                let walk = &walk;
                scope.spawn(move || fill(band, first, walk));
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

/// What each pixel of the raster carries, from what the file carried and
/// what the display does to it — see [`displayed`].
fn displayed_channels(image: &DecodedImage, display: &Display) -> Channels {
    let source = image.channels();
    let gray = source.is_gray() && display.colormap() == Colormap::Gray;
    let alpha = source.alpha_index().is_some();
    match (gray, alpha) {
        (true, false) => Channels::Gray,
        (true, true) => Channels::GrayAlpha,
        (false, false) => Channels::Rgb,
        (false, true) => Channels::Rgba,
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

/// What every band of the walk reads: the picture, how it is shown, and
/// the part of it wanted.
struct Walk<'a> {
    image: &'a DecodedImage,
    display: &'a Display,
    channels: Channels,
    /// How the picture is turned, which `region` and the rows are in.
    turn: Turn,
    region: Region,
    /// The lift the screen is drawn through, where the picture has a gain
    /// map — so that what is copied is what is on screen, which on a
    /// monitor with no room above white is the base as it was graded.
    lift: Option<&'a Table>,
}

/// Writes the rows of `band`, which start at row `first` of the image and
/// run across the columns `region` takes in.
fn fill(band: &mut [u8], first: u32, walk: &Walk<'_>) {
    let Walk {
        image,
        display,
        channels,
        turn,
        region,
        lift,
    } = *walk;
    let stored = [image.width, image.height];
    let levels = levels();
    let count = channels.count();
    let gray = channels.is_gray();
    let stride = region.width as usize * count;
    for (offset, row) in band.chunks_exact_mut(stride).enumerate() {
        let y = first + offset as u32;
        for (column, pixel) in row.chunks_exact_mut(count).enumerate() {
            let [x, y] = turn.stored([region.x + column as u32, y], stored);
            // Only `None` outside the image, which this walk never goes.
            let Some(sample) = image.sample(x, y, lift) else {
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
    png_with(raster, &[], Compression::Fast)
}

/// [`png()`] for a file that is being kept: compressed at the crate's own
/// balance of time against size, since the file stays on disk long after
/// the wait for it is over. `High` was not worth it — much more time for a
/// file slightly smaller.
pub fn png_for_file(raster: &Raster) -> Result<Vec<u8>> {
    png_with(raster, &[], Compression::Balanced)
}

/// The quality a JPEG is exported at until the export dialog's slider says
/// otherwise: high enough that the compression is not what a look at the
/// picture notices.
pub const JPEG_QUALITY: u8 = 90;

/// The lowest quality the encoder takes: it reads anything under this as
/// this, so the slider stops here rather than offering a 0 that means 1.
pub const JPEG_QUALITY_MIN: u8 = 1;

/// `raster` as the bytes of a JPEG file, at `quality` from
/// [`JPEG_QUALITY_MIN`] to 100.
///
/// A JPEG holds no alpha, so a raster that carries one loses it: each
/// pixel's color is written as it is, which for a pixel that was fully
/// transparent is whatever color it held — black, from a premultiplied
/// file. Gray stays gray.
pub fn jpeg(raster: &Raster, quality: u8) -> Result<Vec<u8>> {
    use ::image::ExtendedColorType;
    use ::image::codecs::jpeg::JpegEncoder;

    let stripped;
    let (kind, data): (_, &[u8]) = match raster.channels {
        Channels::Gray => (ExtendedColorType::L8, &raster.data),
        Channels::Rgb => (ExtendedColorType::Rgb8, &raster.data),
        Channels::GrayAlpha => {
            stripped = without_alpha(&raster.data, 2);
            (ExtendedColorType::L8, &stripped)
        }
        Channels::Rgba => {
            stripped = without_alpha(&raster.data, 4);
            (ExtendedColorType::Rgb8, &stripped)
        }
    };
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, quality.clamp(JPEG_QUALITY_MIN, 100))
        .encode(data, raster.width, raster.height, kind)
        .context("writing the JPEG")?;
    Ok(bytes)
}

/// Packed pixels of `count` components each, with the last of each left
/// out.
fn without_alpha(data: &[u8], count: usize) -> Vec<u8> {
    data.chunks_exact(count)
        .flat_map(|pixel| &pixel[..count - 1])
        .copied()
        .collect()
}

/// [`png()`], with `text` written into the file as `tEXt` chunks ahead of the
/// pixels, one `(keyword, text)` each: what a thumbnail says about the file
/// it was made of. Both halves must be Latin-1, which is all a `tEXt` chunk
/// can hold; the thumbnail cache's are ASCII.
pub fn png_with_text(raster: &Raster, text: &[(String, String)]) -> Result<Vec<u8>> {
    png_with(raster, text, Compression::Fast)
}

/// The PNG itself, `text` ahead of the pixels and compressed as asked.
fn png_with(
    raster: &Raster,
    text: &[(String, String)],
    compression: Compression,
) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut encoder = Encoder::new(&mut bytes, raster.width, raster.height);
    for (keyword, text) in text {
        encoder
            .add_text_chunk(keyword.clone(), text.clone())
            .with_context(|| format!("adding the {keyword} chunk"))?;
    }
    encoder.set_color(match raster.channels {
        Channels::Gray => ColorType::Grayscale,
        Channels::GrayAlpha => ColorType::GrayscaleAlpha,
        Channels::Rgb => ColorType::Rgb,
        Channels::Rgba => ColorType::Rgba,
    });
    encoder.set_depth(BitDepth::Eight);
    encoder.set_compression(compression);
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
    // over the last is 255. A NaN passes none of them and comes out 0.
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
        super::displayed(
            image,
            display,
            Turn::NONE,
            Region::whole([image.width, image.height]),
            None,
        )
    }

    /// `(color type, bit depth, pixel bytes)` as a PNG decoder reads them
    /// back, which is the only reading of the file that matters.
    fn round_trip(raster: &Raster) -> (ColorType, BitDepth, Vec<u8>) {
        round_trip_bytes(&super::png(raster).expect("a valid raster encodes"))
    }

    /// The same, of a PNG's bytes however it was written.
    fn round_trip_bytes(bytes: &[u8]) -> (ColorType, BitDepth, Vec<u8>) {
        let mut reader = Decoder::new(Cursor::new(bytes)).read_info().unwrap();
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
        display.set_colormap(Colormap::Viridis, true);
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
        brighter.set_exposure(1.0);
        let brighter_byte = round_trip(&displayed(&source, &brighter)).2[0];
        assert!(
            brighter_byte > plain_byte,
            "a stop up should lighten: {plain_byte} → {brighter_byte}"
        );

        let mut windowed = plain();
        windowed.set_window(0.0, 0.1);
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
        clipped.set_tone_map(ToneMap::None, true);
        assert_eq!(round_trip(&displayed(&bright, &clipped)).2[0], 255);

        let mut rolled = plain();
        rolled.set_tone_map(ToneMap::Neutral, true);
        let rolled_byte = round_trip(&displayed(&bright, &rolled)).2[0];
        assert!(
            rolled_byte < 255,
            "the roll-off should pull 4.0 back under white, got {rolled_byte}"
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
        display.set_exposure(0.7);
        display.set_window(0.0, 0.8);

        let mut expected = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let mapped = display.map(&source.sample(x, y, None).unwrap(), Headroom::None);
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

    /// A text chunk written goes in ahead of the pixels, where a reader of
    /// the header alone finds it, and comes back as it went.
    #[test]
    fn a_text_chunk_survives_the_round_trip() {
        let raster = displayed(&image(Channels::Gray, vec![7]), &plain());
        let bytes = png_with_text(
            &raster,
            &[("Thumb::URI".to_string(), "file:///tmp/a.png".to_string())],
        )
        .expect("a valid raster encodes");
        let reader = Decoder::new(Cursor::new(&bytes)).read_info().unwrap();
        let chunks = &reader.info().uncompressed_latin1_text;
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].keyword, "Thumb::URI");
        assert_eq!(chunks[0].text, "file:///tmp/a.png");

        // One band is the same walk on one thread.
        let region = Region::whole([1, 1]);
        let source = image(Channels::Gray, vec![7]);
        assert_eq!(
            displayed_on(&source, &plain(), Turn::NONE, region, None, 1).data,
            displayed_on(&source, &plain(), Turn::NONE, region, None, 8).data
        );
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
        display.set_exposure(0.4);

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
        let part = super::displayed(&source, &display, Turn::NONE, region, None);
        assert_eq!((part.width, part.height), (200, 190));

        let mut expected = Vec::new();
        for y in region.y..region.bottom() {
            let row = y as usize * width as usize * 3;
            let from = row + region.x as usize * 3;
            expected.extend_from_slice(&whole.data[from..from + region.width as usize * 3]);
        }
        assert_eq!(part.data, expected);
    }

    /// A copy of the picture turned on screen is the copy of the picture
    /// turned on the CPU: the walk reads through the turn exactly as the
    /// decoders apply one, region and all.
    #[test]
    fn a_turned_copy_is_the_turned_picture_copied() {
        use crate::image::orient;
        let (width, height) = (7u32, 5u32);
        let data = (0..width * height * 3)
            .map(|index| (index * 7 % 251) as u8)
            .collect();
        let mut source = image(Channels::Rgb, data);
        source.width = width;
        source.height = height;
        let mut display = plain();
        display.set_exposure(0.3);
        let region = Region {
            x: 1,
            y: 2,
            width: 3,
            height: 4,
        };
        let mut turn = Turn::NONE;
        for _ in 0..4 {
            let turned = orient::apply(source.clone(), turn.orientation());
            let region = match turn.is_quarter() {
                true => region,
                false => Region {
                    width: 4,
                    height: 3,
                    ..region
                },
            };
            let ours = super::displayed(&source, &display, turn, region, None);
            let theirs = super::displayed(&turned, &display, Turn::NONE, region, None);
            assert_eq!((ours.width, ours.height), (theirs.width, theirs.height));
            assert_eq!(ours.data, theirs.data, "{turn:?}");
            turn = turn.clockwise();
        }
    }

    /// A JPEG has no alpha: a raster that carries one is written without
    /// it, gray as gray and color as color, at the raster's size.
    #[test]
    fn a_jpeg_is_written_without_alpha() {
        let rgba = Raster {
            width: 2,
            height: 1,
            channels: Channels::Rgba,
            data: vec![200, 100, 50, 255, 10, 20, 30, 0],
        };
        let decoded = ::image::load_from_memory(&jpeg(&rgba, JPEG_QUALITY).unwrap()).unwrap();
        assert_eq!(decoded.color(), ::image::ColorType::Rgb8);
        assert_eq!((decoded.width(), decoded.height()), (2, 1));

        let gray = Raster {
            width: 3,
            height: 2,
            channels: Channels::GrayAlpha,
            data: vec![128, 255, 128, 0, 128, 64, 128, 255, 128, 255, 128, 255],
        };
        let decoded = ::image::load_from_memory(&jpeg(&gray, JPEG_QUALITY).unwrap()).unwrap();
        assert_eq!(decoded.color(), ::image::ColorType::L8);
        assert_eq!((decoded.width(), decoded.height()), (3, 2));
        // Flat gray survives a JPEG within a step or two.
        for value in decoded.into_luma8().into_raw() {
            assert!(value.abs_diff(128) <= 2, "{value}");
        }
    }

    /// The file's PNG and the clipboard's differ only in how hard they are
    /// squeezed: the same pixels come back out of either.
    #[test]
    fn a_file_png_decodes_to_the_same_pixels_as_the_fast_one() {
        let raster = Raster {
            width: 16,
            height: 4,
            channels: Channels::Rgb,
            data: (0..16 * 4 * 3)
                .map(|index| (index * 5 % 256) as u8)
                .collect(),
        };
        let fast = round_trip_bytes(&png(&raster).unwrap());
        let kept = round_trip_bytes(&png_for_file(&raster).unwrap());
        assert_eq!(fast, kept);
        assert_eq!(kept.2, raster.data);
    }
}
