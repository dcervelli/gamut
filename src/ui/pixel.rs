//! The readout that follows the pointer: which pixel it is over, what the
//! file holds there, what the display makes of that, and a swatch of the
//! colour it comes out as.
//!
//! Two numbers for one pixel, because they answer different questions. The
//! stored one is the measurement — the count a sensor recorded, the metre a
//! terrain model states — and is the reason anyone points at a pixel. The
//! mapped one is what the window, the exposure and the false colour have made
//! of it, and is the reason the pixel looks the way it does. The swatch
//! settles the second question without asking anyone to read three decimals
//! and imagine a colour.

use crate::image::display::Mapped;
use crate::image::{DecodedImage, Sample, Samples, Transfer};
use crate::render::{Color, Rect, TextMeasure, UiFrame};
use crate::theme::Theme;

use super::buttons::outline;
use super::{Current, PADDING, TEXT_SIZE, status, text_baseline};

/// Side of the colour swatch, in logical pixels: the height of a line of
/// text, so that it reads as part of the sentence beside it.
const SWATCH: f32 = 13.0;

/// Between the swatch and the words it belongs to.
const GAP: f32 = 8.0;

/// Draws the readout at the left of the bottom bar, from [`PADDING`] up to
/// `limit` — where the state text at the other end of the bar begins.
///
/// The swatch stands at the front rather than beside the numbers it depicts:
/// the numbers change with every pixel the pointer crosses, and a swatch
/// pinned to the end of them would jitter along the bar as they got longer
/// and shorter.
pub(super) fn draw(
    frame: &mut UiFrame,
    text: &mut dyn TextMeasure,
    current: &Current,
    at: [u32; 2],
    bar: Rect,
    limit: f32,
    theme: &Theme,
) {
    let Some(sample) = current.image.sample(at[0], at[1]) else {
        return;
    };
    let mapped = current.display.map(&sample);

    let mut x = PADDING;
    // A bar too short for the swatch gets the words alone, the way a panel
    // too narrow for a button gets no button.
    if bar.height >= SWATCH && x + SWATCH < limit {
        let swatch = Rect::new(
            x,
            (bar.y + (bar.height - SWATCH) / 2.0).round(),
            SWATCH,
            SWATCH,
        );
        frame.rect(swatch, swatch_color(&mapped));
        // An outline, so that a transparent or near-black pixel still reads as
        // a swatch showing something rather than as a gap in the bar.
        outline(frame, swatch, 1.0, theme.border);
        x += SWATCH + GAP;
    }

    let width = (limit - x).max(1.0);
    frame.text_clipped(
        [x, text_baseline(bar)],
        TEXT_SIZE,
        theme.text_primary,
        width,
        status::fit_segments(text, &segments(at, &current.image, &sample, &mapped), width),
    );
}

/// Where the pointer is, and what is there: two segments, so that a bar with
/// no room for the values keeps the coordinate rather than clipping the
/// numbers in half.
fn segments(at: [u32; 2], image: &DecodedImage, sample: &Sample, mapped: &Mapped) -> [String; 2] {
    let float = matches!(image.samples, Samples::F32 { .. });
    let stored: Vec<String> = sample
        .stored()
        .iter()
        .map(|value| component(*value, float))
        .collect();
    let displayed: Vec<String> = mapped
        .values()
        .iter()
        .map(|value| format!("{value:.3}"))
        .collect();
    [
        format!("({}, {})", at[0], at[1]),
        format!("{}   \u{2192}   {}", stored.join(" "), displayed.join(" ")),
    ]
}

/// One stored component, in the units the file keeps it in. Integer samples
/// are counts and print as counts; float ones keep four decimals, and fall
/// back to an exponent at the magnitudes where that would be a row of zeroes
/// or a wall of digits.
fn component(value: f32, float: bool) -> String {
    if !float {
        return format!("{value:.0}");
    }
    let magnitude = value.abs();
    if magnitude == 0.0 || (1e-3..1e6).contains(&magnitude) {
        format!("{value:.4}")
    } else {
        format!("{value:.3e}")
    }
}

/// The swatch's colour: what the compositor will put on screen for this
/// pixel, encoded the way interface colours are written.
///
/// An SDR reading of it. On an HDR output the surface carries more range than
/// a swatch in a panel can show, and the panel is the thing it has to sit
/// beside without glowing.
fn swatch_color(mapped: &Mapped) -> Color {
    let channel =
        |value: f32| (Transfer::Srgb.to_encoded(value.clamp(0.0, 1.0)) * 255.0).round() as u8;
    Color::rgba(
        channel(mapped.color[0]),
        channel(mapped.color[1]),
        channel(mapped.color[2]),
        (mapped.alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::display::{Colormap, Display};
    use crate::image::{AlphaMode, Channels, ColorSpace};

    /// Every glyph the same width, so that a test can say how much room a
    /// string has in whole characters.
    struct Monospace;

    impl TextMeasure for Monospace {
        fn measure_text(&mut self, text: &str, size: f32) -> [f32; 2] {
            [text.chars().count() as f32 * size, size]
        }
    }

    /// A 4x5 sRGB image, black but for `pixel` in its bottom right corner at
    /// (3, 4) — which is the pixel every test below points at.
    fn rgb8(pixel: [u8; 3]) -> DecodedImage {
        let mut data = vec![0u8; 4 * 5 * 3];
        data[57..60].copy_from_slice(&pixel);
        DecodedImage::new(
            4,
            5,
            Samples::U8 {
                channels: Channels::Rgb,
                data,
            },
            ColorSpace::SRGB,
            AlphaMode::Opaque,
        )
    }

    fn read(image: &DecodedImage, display: &Display, at: [u32; 2]) -> [String; 2] {
        let sample = image.sample(at[0], at[1]).expect("inside the image");
        segments(at, image, &sample, &display.map(&sample))
    }

    /// The file's numbers in the file's units, and beside them what the
    /// display has made of them. An 8-bit image reads as codes, not as
    /// fractions of one: 231 is what a dropper in any other tool would say.
    #[test]
    fn the_readout_names_the_codes_the_file_holds_and_the_values_they_map_to() {
        let words = read(&rgb8([231, 128, 64]), &Display::default(), [3, 4]);
        assert_eq!(words[0], "(3, 4)");
        // The mapped side is linear light, which is the space the window and
        // everything after it works in — sRGB 231 is 80% of the way up.
        assert_eq!(words[1], "231 128 64   \u{2192}   0.799 0.216 0.051");
    }

    /// Measurement work is why anyone points at a pixel, so float samples
    /// keep their decimals rather than being rounded to the nearest count.
    #[test]
    fn float_samples_keep_their_decimals_and_their_extremes() {
        let image = DecodedImage::new(
            3,
            1,
            Samples::F32 {
                channels: Channels::Gray,
                data: vec![0.125, 0.000_002_5, 1.0e7],
            },
            ColorSpace::LINEAR_BT709,
            AlphaMode::Opaque,
        );
        let display = Display::default();

        assert_eq!(
            read(&image, &display, [0, 0])[1],
            "0.1250   \u{2192}   0.125"
        );
        assert_eq!(
            read(&image, &display, [1, 0])[1],
            "2.500e-6   \u{2192}   0.000"
        );
        assert_eq!(
            read(&image, &display, [2, 0])[1],
            "1.000e7   \u{2192}   10000000.000"
        );
    }

    /// Whatever else has to go, the coordinate stays: it is the one thing the
    /// readout says that nothing else in the window says.
    #[test]
    fn a_narrow_bar_keeps_the_coordinate_and_drops_the_values() {
        let words = read(&rgb8([231, 128, 64]), &Display::default(), [3, 4]);

        let roomy = status::fit_segments(&mut Monospace, &words, 1000.0);
        assert!(roomy.contains("231 128 64"), "{roomy}");

        let cramped = status::fit_segments(&mut Monospace, &words, 100.0);
        assert_eq!(cramped, "(3, 4)");
    }

    /// The swatch is there to answer the question the numbers cannot: a
    /// windowed value of 0.5 is a shade of grey until a colormap is on, and
    /// then it is a colour no column of digits describes.
    #[test]
    fn the_swatch_shows_the_colour_the_pixel_comes_out_rather_than_its_value() {
        let image = DecodedImage::new(
            1,
            1,
            Samples::U8 {
                channels: Channels::Gray,
                data: vec![128],
            },
            ColorSpace::LINEAR_BT709,
            AlphaMode::Opaque,
        );
        let sample = image.sample(0, 0).expect("inside the image");

        let mut display = Display::default();
        let grey = swatch_color(&display.map(&sample));
        assert_eq!(grey.r, grey.g);
        assert_eq!(grey.g, grey.b);
        assert_eq!(grey.a, 255);

        display.colormap = Colormap::Viridis;
        let false_colour = swatch_color(&display.map(&sample));
        assert!(
            false_colour.g > false_colour.r && false_colour.b > false_colour.r,
            "the middle of viridis is teal, not grey: {false_colour:?}"
        );
    }
}
