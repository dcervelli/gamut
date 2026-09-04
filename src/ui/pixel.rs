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
use crate::image::{DecodedImage, Sample, Samples};
use crate::render::{Color, Rect, TextMeasure, UiFrame};
use crate::theme::Theme;

use super::buttons::outline;
use super::chrome::BAR_PADDING;
use super::{Current, FrameInput, TEXT_SIZE, text_baseline};

/// Side of the colour swatch, in logical pixels: the height of a line of
/// text, so that it reads as part of the sentence beside it.
const SWATCH: f32 = 13.0;

/// Between the swatch and the words on either side of it.
const GAP: f32 = 8.0;

/// What parts the coordinate from the colour: the same middot the bars use
/// between one segment and the next, so that the readout reads as two things
/// the way the rest of the bar does.
const SEPARATOR: &str = "\u{00b7}";

/// Draws the readout at the left of the bottom bar, from [`BAR_PADDING`] up to
/// `limit` — where the state text at the other end of the bar begins — for
/// the pixel the pointer is over, if it is over one. What comes out is half
/// the surface's doing, which is why the frame's input comes along rather
/// than the pixel alone.
///
/// The coordinate leads, then the swatch, then the numbers it stands for. The
/// swatch belongs to the colour it depicts rather than to the pixel's address,
/// so it sits in front of the values and moves with them — and the coordinate
/// is set to a fixed width so that they stay put while the pointer moves.
pub(super) fn draw(
    frame: &mut UiFrame,
    text: &mut dyn TextMeasure,
    current: &Current,
    input: &FrameInput,
    bar: Rect,
    limit: f32,
    theme: &Theme,
) {
    let Some(at) = input.pointer else {
        return;
    };
    let Some(sample) = current.image.sample(at[0], at[1]) else {
        return;
    };
    let mapped = current.display.map(&sample, input.headroom);
    let baseline = text_baseline(bar);

    // Whatever else has to go, the coordinate stays: it is the one thing the
    // readout says that nothing else in the window says.
    let coordinate = coordinate(at, [current.image.width, current.image.height]);
    let end = BAR_PADDING + text.measure_mono(&coordinate, TEXT_SIZE)[0];
    frame.text_clipped_mono(
        [BAR_PADDING, baseline],
        TEXT_SIZE,
        theme.text_primary,
        (limit - BAR_PADDING).max(1.0),
        coordinate,
    );

    let values = values(&current.image, &sample, &mapped);
    let values_width = text.measure_text(&values, TEXT_SIZE)[0];
    let separator_width = text.measure_text(SEPARATOR, TEXT_SIZE)[0];
    // A bar too short for the swatch gets the words alone, the way a panel
    // too narrow for a button gets no button.
    let Some((separator_x, swatch_x, values_x)) = place(
        end,
        separator_width,
        values_width,
        limit,
        bar.height >= SWATCH,
    ) else {
        return;
    };
    frame.text(
        [separator_x, baseline],
        TEXT_SIZE,
        theme.text_primary,
        SEPARATOR,
    );
    if let Some(x) = swatch_x {
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
    }
    frame.text_clipped(
        [values_x, baseline],
        TEXT_SIZE,
        theme.text_primary,
        (limit - values_x).max(1.0),
        values,
    );
}

/// Where the separator, the swatch and the values go once the coordinate has
/// ended at `end` — or `None` when what is left of the bar before `limit` has
/// no room for the values, in which case the separator and the swatch go with
/// them rather than standing after the coordinate parting it from nothing.
fn place(
    end: f32,
    separator_width: f32,
    values_width: f32,
    limit: f32,
    swatch: bool,
) -> Option<(f32, Option<f32>, f32)> {
    let separator_x = end + GAP;
    let after = separator_x + separator_width + GAP;
    let values_x = if swatch { after + SWATCH + GAP } else { after };
    (values_x + values_width <= limit).then(|| (separator_x, swatch.then_some(after), values_x))
}

/// Where the pointer is, padded out to the widest coordinate `size` can
/// produce and set monospaced by the caller.
///
/// Both together are what hold the swatch and the numbers after it still: the
/// padding keeps the digit count fixed as the pointer crosses a power of ten,
/// and the face keeps every digit the same width as the last.
fn coordinate(at: [u32; 2], size: [u32; 2]) -> String {
    let places = |extent: u32| extent.saturating_sub(1).max(1).ilog10() as usize + 1;
    format!(
        "({:0>x$}, {:0>y$})",
        at[0],
        at[1],
        x = places(size[0]),
        y = places(size[1])
    )
}

/// What is there: the numbers the file holds, and beside them what the
/// display has made of them.
fn values(image: &DecodedImage, sample: &Sample, mapped: &Mapped) -> String {
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
    format!("{}   \u{2192}   {}", stored.join(" "), displayed.join(" "))
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
/// An SDR reading of it: `from_linear` stops at white. On an HDR output the
/// surface carries more range than a swatch in a panel can show, and the
/// panel is the thing it has to sit beside without glowing.
fn swatch_color(mapped: &Mapped) -> Color {
    Color::from_linear(mapped.color)
        .with_alpha((mapped.alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::display::{Colormap, Display, Headroom};
    use crate::image::{AlphaMode, Channels, ColorSpace};

    use crate::ui::Monospace;

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

    fn read(image: &DecodedImage, display: &Display, at: [u32; 2]) -> String {
        let sample = image.sample(at[0], at[1]).expect("inside the image");
        values(image, &sample, &display.map(&sample, Headroom::None))
    }

    /// The file's numbers in the file's units, and beside them what the
    /// display has made of them. An 8-bit image reads as codes, not as
    /// fractions of one: 231 is what a dropper in any other tool would say.
    #[test]
    fn the_readout_names_the_codes_the_file_holds_and_the_values_they_map_to() {
        assert_eq!(coordinate([3, 4], [4, 5]), "(3, 4)");
        // The mapped side is linear light, which is the space the window and
        // everything after it works in — sRGB 231 is 80% of the way up.
        assert_eq!(
            read(&rgb8([231, 128, 64]), &Display::default(), [3, 4]),
            "231 128 64   \u{2192}   0.799 0.216 0.051"
        );
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

        assert_eq!(read(&image, &display, [0, 0]), "0.1250   \u{2192}   0.125");
        assert_eq!(
            read(&image, &display, [1, 0]),
            "2.500e-6   \u{2192}   0.000"
        );
        assert_eq!(
            read(&image, &display, [2, 0]),
            "1.000e7   \u{2192}   10000000.000"
        );
    }

    /// The coordinate is as wide at (0, 0) as it is at the far corner, so
    /// that nothing after it moves as the pointer crosses a power of ten.
    /// One column per digit the image can actually reach: a 1000-wide image
    /// counts to 999.
    #[test]
    fn the_coordinate_is_padded_to_the_widest_the_image_can_read() {
        assert_eq!(coordinate([7, 9], [1920, 1080]), "(0007, 0009)");
        assert_eq!(coordinate([1919, 1079], [1920, 1080]), "(1919, 1079)");
        assert_eq!(coordinate([999, 0], [1000, 1000]), "(999, 000)");
        assert_eq!(coordinate([0, 0], [1, 1]), "(0, 0)", "a one-pixel image");
    }

    /// Whatever else has to go, the coordinate stays: it is the one thing the
    /// readout says that nothing else in the window says. The separator and
    /// the swatch go with the values, having nothing to say once they are
    /// gone.
    #[test]
    fn a_narrow_bar_keeps_the_coordinate_and_drops_the_swatch_with_the_values() {
        let values = read(&rgb8([231, 128, 64]), &Display::default(), [3, 4]);
        let width = Monospace.measure_text(&values, TEXT_SIZE)[0];
        let dot = Monospace.measure_text(SEPARATOR, TEXT_SIZE)[0];
        let end = BAR_PADDING + Monospace.measure_mono(&coordinate([3, 4], [4, 5]), TEXT_SIZE)[0];

        let roomy = place(end, dot, width, end + 1000.0, true).expect("room for all of it");
        let after_dot = end + GAP + dot + GAP;
        assert_eq!(
            roomy,
            (end + GAP, Some(after_dot), after_dot + SWATCH + GAP)
        );

        assert_eq!(place(end, dot, width, end + width, true), None);
    }

    /// A bar too short to draw a swatch in still reads out the numbers, and
    /// closes the space the swatch would have taken. The separator stays: it
    /// parts the coordinate from the values, not from the swatch.
    #[test]
    fn a_short_bar_gives_the_values_the_swatch_s_place() {
        let (separator_x, swatch, values_x) =
            place(100.0, 4.0, 50.0, 1000.0, false).expect("room for the values");
        assert_eq!(separator_x, 100.0 + GAP);
        assert_eq!(swatch, None);
        assert_eq!(values_x, 100.0 + GAP + 4.0 + GAP);
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
        let grey = swatch_color(&display.map(&sample, Headroom::None));
        assert_eq!(grey.r, grey.g);
        assert_eq!(grey.g, grey.b);
        assert_eq!(grey.a, 255);

        display.colormap = Colormap::Viridis;
        let false_colour = swatch_color(&display.map(&sample, Headroom::None));
        assert!(
            false_colour.g > false_colour.r && false_colour.b > false_colour.r,
            "the middle of viridis is teal, not grey: {false_colour:?}"
        );
    }
}
