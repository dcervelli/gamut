//! The image as it is on screen, written out as a PNG.
//!
//! The picture that travels is the one the display settings have made — the
//! window, the exposure, the tone curve, the false colour — at the image's own
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

use anyhow::{Context, Result};
use png::{BitDepth, ColorType, Compression, Encoder, SrgbRenderingIntent};

use super::display::{Colormap, Display};
use super::{Channels, DecodedImage, Transfer};

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

/// Runs the display pipeline over every pixel of `image`.
///
/// A single-channel image stays single-channel, because that is what it is and
/// storing the same number three times says nothing more. False colour is the
/// exception: it turns one value into a colour on purpose, and a grey PNG
/// could not hold the result.
///
/// Alpha is carried only where the file had some. An image that was opaque
/// stays opaque rather than gaining a channel of nothing but 255.
pub fn displayed(image: &DecodedImage, display: &Display) -> Raster {
    let source = image.channels();
    let gray = source.is_gray() && display.colormap == Colormap::Gray;
    let alpha = source.alpha_index().is_some();
    let channels = match (gray, alpha) {
        (true, false) => Channels::Gray,
        (true, true) => Channels::GrayAlpha,
        (false, false) => Channels::Rgb,
        (false, true) => Channels::Rgba,
    };

    let pixels = image.width as usize * image.height as usize;
    let mut data = Vec::with_capacity(pixels * channels.count());
    for y in 0..image.height {
        for x in 0..image.width {
            // Only `None` outside the image, which this walk never goes.
            let Some(sample) = image.sample(x, y) else {
                continue;
            };
            let mapped = display.map(&sample);
            if gray {
                // Grey reaches the screen as the same number in all three, so
                // any one of them is the whole of it.
                data.push(quantise(mapped.color[0]));
            } else {
                data.extend(mapped.color.map(quantise));
            }
            if alpha {
                data.push(byte(mapped.alpha));
            }
        }
    }

    Raster {
        width: image.width,
        height: image.height,
        channels,
        data,
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
    // Said outright rather than left to be assumed. Whatever the file's own
    // colour space was, the window and the tone curve have taken it to what
    // the screen was showing, and that was resolved against sRGB.
    encoder.set_source_srgb(SrgbRenderingIntent::Perceptual);

    let mut writer = encoder.write_header().context("writing the PNG header")?;
    writer
        .write_image_data(&raster.data)
        .context("writing the PNG pixels")?;
    writer.finish().context("finishing the PNG")?;
    Ok(bytes)
}

/// One linear working-space component as the byte a PNG stores.
///
/// The sRGB curve is applied here and nowhere earlier: everything upstream of
/// this works in linear light, which is the invariant `render::upload` states
/// and the shaders rely on.
fn quantise(linear: f32) -> u8 {
    byte(Transfer::Srgb.to_encoded(linear.clamp(0.0, 1.0)))
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

    /// `(colour type, bit depth, pixel bytes)` as a PNG decoder reads them
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
    fn a_grey_image_stays_one_channel() {
        let raster = displayed(&image(Channels::Gray, vec![0, 128, 255]), &plain());
        assert_eq!(raster.channels, Channels::Gray);
        let (color, depth, pixels) = round_trip(&raster);
        assert_eq!((color, depth), (ColorType::Grayscale, BitDepth::Eight));
        // Out the far side as it went in: an sRGB image shown unwindowed is
        // decoded to linear and encoded back again, and neither step moves it.
        assert_eq!(pixels, vec![0, 128, 255]);
    }

    /// False colour is three components where the value was one, so the grey
    /// cannot be kept — this is the one thing that widens a single channel.
    #[test]
    fn false_colour_makes_a_grey_image_colour() {
        let mut display = plain();
        display.colormap = Colormap::Viridis;
        let raster = displayed(&image(Channels::Gray, vec![0, 255]), &display);
        assert_eq!(raster.channels, Channels::Rgb);
        let (color, _, pixels) = round_trip(&raster);
        assert_eq!(color, ColorType::Rgb);
        // Viridis runs dark blue-purple to bright yellow; whatever the exact
        // codes, the two ends are not grey and not each other.
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

        let grey_alpha = displayed(&image(Channels::GrayAlpha, vec![200, 64]), &plain());
        assert_eq!(grey_alpha.channels, Channels::GrayAlpha);
        assert_eq!(round_trip(&grey_alpha).0, ColorType::GrayscaleAlpha);
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
        clipped.tone_map = ToneMap::Clip;
        assert_eq!(round_trip(&displayed(&bright, &clipped)).2[0], 255);

        let mut rolled = plain();
        rolled.tone_map = ToneMap::Reinhard;
        let rolled_byte = round_trip(&displayed(&bright, &rolled)).2[0];
        assert!(
            rolled_byte < 255,
            "Reinhard should pull 4.0 back under white, got {rolled_byte}"
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
}
