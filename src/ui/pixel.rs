//! The readout that follows the pointer: which pixel it is over, what is
//! there, and a swatch of the color it comes out as.
//!
//! What "what is there" means is the reader's to choose, because one pixel
//! answers more than one question. The codes the file holds are the
//! measurement — the count a sensor recorded, the meter a terrain model
//! states — and are the reason anyone points at a pixel; hexadecimal is those
//! same codes written the way the rest of the world writes a color down. The
//! mapped values are what the window, the exposure and the false color have
//! made of them, and are the reason the pixel looks the way it does. One at a
//! time rather than all at once: the bar is one line long, and a reader
//! working in one of them is not reading the others. The swatch stands beside
//! whichever is showing and settles the question none of the three answers
//! without asking anyone to read three decimals and imagine a color.

use crate::image::display::Mapped;
use crate::image::{DecodedImage, Sample, Samples};
use crate::render::{Color, Rect, TextMeasure, UiFrame};
use crate::theme::Theme;

use super::buttons::outline;
use super::{Current, FrameInput, TEXT_SIZE, text_baseline};

/// Side of the color swatch, in logical pixels: the height of a line of
/// text, so that it reads as part of the sentence beside it.
const SWATCH: f32 = 13.0;

/// Between the swatch and the words on either side of it.
pub(super) const GAP: f32 = 8.0;

/// What parts the coordinate from the color: the same middot the bars use
/// between one segment and the next, so that the readout reads as two things
/// the way the rest of the bar does.
const SEPARATOR: &str = "\u{00b7}";

/// How the pixel's value is written out.
///
/// Three ways of saying what is at one pixel, of which the bar shows one:
/// chosen from the menu the dot at the head of the readout opens, or stepped
/// through with the key that does the same.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PixelFormat {
    /// The codes the file holds, run together in hexadecimal with no prefix
    /// and no spaces — `E78040` — which is how a color is written down
    /// everywhere outside this window.
    Hex,
    /// Those codes as numbers, in the units the file keeps them in. The
    /// default: it is the one of the three that says what was measured.
    #[default]
    Decimal,
    /// What the window, the exposure and the false color have made of them.
    Mapped,
}

impl PixelFormat {
    /// Every format, in the order the menu offers them — the same order
    /// [`PixelFormat::next`] steps through, so the key and the cells agree
    /// about what comes after what.
    pub const ALL: [PixelFormat; 3] = [PixelFormat::Hex, PixelFormat::Decimal, PixelFormat::Mapped];

    /// What the interface calls this format, for the cell that chooses it.
    pub fn label(self) -> &'static str {
        match self {
            PixelFormat::Hex => "Hex",
            PixelFormat::Decimal => "Decimal",
            PixelFormat::Mapped => "Mapped",
        }
    }

    pub fn next(self) -> Self {
        match self {
            PixelFormat::Hex => PixelFormat::Decimal,
            PixelFormat::Decimal => PixelFormat::Mapped,
            PixelFormat::Mapped => PixelFormat::Hex,
        }
    }
}

/// Draws the readout for the pixel the pointer is over, if it is over one, in
/// `room`: the strip of the bottom bar between the button that chooses the
/// format and the state text at the other end. What comes out is half the
/// surface's doing, which is why the frame's input comes along rather than the
/// pixel alone.
///
/// The coordinate leads, then the swatch, then the value it stands for. The
/// swatch belongs to the color it depicts rather than to the pixel's address,
/// so it sits in front of the value and moves with it — and the coordinate
/// is set to a fixed width so that they stay put while the pointer moves.
pub(super) fn draw(
    frame: &mut UiFrame,
    text: &mut dyn TextMeasure,
    current: &Current,
    input: &FrameInput,
    room: Rect,
    format: PixelFormat,
    theme: &Theme,
) {
    let (start, limit) = (room.x, room.right());
    let Some(at) = input.pointer else {
        return;
    };
    let Some(sample) = current.image.sample(at[0], at[1]) else {
        return;
    };
    let mapped = current.display.map(&sample, input.headroom);
    let baseline = text_baseline(room);

    // Whatever else has to go, the coordinate stays: it is the one thing the
    // readout says that nothing else in the window says.
    let coordinate = coordinate(at, [current.image.width, current.image.height]);
    let end = start + text.measure_mono(&coordinate, TEXT_SIZE)[0];
    frame.text_clipped_mono(
        [start, baseline],
        TEXT_SIZE,
        theme.text_primary,
        (limit - start).max(1.0),
        coordinate,
    );

    let values = value(&current.image, &sample, &mapped, format);
    let values_width = text.measure_text(&values, TEXT_SIZE)[0];
    let separator_width = text.measure_text(SEPARATOR, TEXT_SIZE)[0];
    // A bar too short for the swatch gets the words alone, the way a panel
    // too narrow for a button gets no button.
    let Some((separator_x, swatch_x, values_x)) = place(
        end,
        separator_width,
        values_width,
        limit,
        room.height >= SWATCH,
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
            (room.y + (room.height - SWATCH) / 2.0).round(),
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

/// The pixel's address as a copy of it is written: `x,y`, with no padding and
/// no parentheses.
///
/// Not what the bar shows. The bar is read in place, where the padding holds
/// the readout still and the parentheses say that the pair is one thing; a
/// copy is bound for somewhere else — a command line, a cell of a spreadsheet,
/// a note — where both would have to be taken back off.
pub fn copied_coordinate(at: [u32; 2]) -> String {
    format!("{},{}", at[0], at[1])
}

/// What is there, in `format`. Also what a copy of the value contains, so
/// that what is copied is exactly what was read.
pub fn value(
    image: &DecodedImage,
    sample: &Sample,
    mapped: &Mapped,
    format: PixelFormat,
) -> String {
    match format {
        PixelFormat::Hex => hex(image, sample),
        PixelFormat::Decimal => {
            let float = matches!(image.samples, Samples::F32 { .. });
            let stored: Vec<String> = sample
                .stored()
                .iter()
                .map(|value| component(*value, float))
                .collect();
            stored.join(" ")
        }
        PixelFormat::Mapped => {
            let displayed: Vec<String> = mapped
                .values()
                .iter()
                .map(|value| format!("{value:.3}"))
                .collect();
            displayed.join(" ")
        }
    }
}

/// The file's own codes, run together in hexadecimal: every component in
/// upper case, each padded to the width of a sample, with nothing between
/// them and nothing in front. `E78040` is what an 8-bit color comes to, which
/// is what a color is called everywhere else.
///
/// A float sample has no code — the number is the value — so what is written
/// there is the bit pattern the file actually holds, which is the only thing
/// hexadecimal could honestly say about it.
fn hex(image: &DecodedImage, sample: &Sample) -> String {
    sample
        .stored()
        .iter()
        .map(|value| match image.samples {
            Samples::U8 { .. } => format!("{:02X}", *value as u32),
            Samples::U16 { .. } => format!("{:04X}", *value as u32),
            Samples::F32 { .. } => format!("{:08X}", value.to_bits()),
        })
        .collect()
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

/// The swatch's color: what the compositor will put on screen for this
/// pixel, encoded the way interface colors are written.
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
    use crate::ui::chrome::BAR_PADDING;

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

    fn read(image: &DecodedImage, display: &Display, at: [u32; 2], format: PixelFormat) -> String {
        let sample = image.sample(at[0], at[1]).expect("inside the image");
        value(
            image,
            &sample,
            &display.map(&sample, Headroom::None),
            format,
        )
    }

    /// The three formats are three answers about one pixel, and the bar shows
    /// one of them at a time. An 8-bit image reads as codes, not as fractions
    /// of one: 231 is what a dropper in any other tool would say, and E7 is
    /// what one writing hex would.
    #[test]
    fn each_format_says_what_is_there_in_its_own_terms() {
        assert_eq!(coordinate([3, 4], [4, 5]), "(3, 4)");
        let image = rgb8([231, 128, 64]);
        let display = Display::default();
        let read = |format| read(&image, &display, [3, 4], format);

        // No prefix and no spaces: what goes in a color picker.
        assert_eq!(read(PixelFormat::Hex), "E78040");
        assert_eq!(read(PixelFormat::Decimal), "231 128 64");
        // The mapped side is linear light, which is the space the window and
        // everything after it works in — sRGB 231 is 80% of the way up.
        assert_eq!(read(PixelFormat::Mapped), "0.799 0.216 0.051");
    }

    /// Every component the file carries, alpha included, and at the width the
    /// file keeps it: two digits to an 8-bit sample and four to a 16-bit one,
    /// so a code is as long as the thing it names.
    #[test]
    fn hex_is_as_wide_as_the_samples_are_and_covers_every_channel() {
        let rgba8 = DecodedImage::new(
            1,
            1,
            Samples::U8 {
                channels: Channels::Rgba,
                data: vec![1, 222, 243, 128],
            },
            ColorSpace::SRGB,
            AlphaMode::Straight,
        );
        assert_eq!(
            read(&rgba8, &Display::default(), [0, 0], PixelFormat::Hex),
            "01DEF380"
        );

        let gray16 = DecodedImage::new(
            1,
            1,
            Samples::U16 {
                channels: Channels::Gray,
                data: vec![4660],
            },
            ColorSpace::SRGB,
            AlphaMode::Opaque,
        );
        assert_eq!(
            read(&gray16, &Display::default(), [0, 0], PixelFormat::Hex),
            "1234"
        );
    }

    /// Measurement work is why anyone points at a pixel, so float samples
    /// keep their decimals rather than being rounded to the nearest count.
    /// A float has no code, so hex writes the bits that are actually there.
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
        let read = |at, format| read(&image, &display, at, format);

        assert_eq!(read([0, 0], PixelFormat::Decimal), "0.1250");
        assert_eq!(read([1, 0], PixelFormat::Decimal), "2.500e-6");
        assert_eq!(read([2, 0], PixelFormat::Decimal), "1.000e7");

        assert_eq!(read([0, 0], PixelFormat::Mapped), "0.125");
        assert_eq!(read([1, 0], PixelFormat::Mapped), "0.000");
        assert_eq!(read([2, 0], PixelFormat::Mapped), "10000000.000");

        assert_eq!(
            read([0, 0], PixelFormat::Hex),
            format!("{:08X}", 0.125f32.to_bits())
        );
    }

    /// The key steps through every format and comes back to where it started,
    /// in the order the menu lays them out.
    #[test]
    fn the_formats_cycle_in_the_order_the_menu_offers_them() {
        let mut format = PixelFormat::ALL[0];
        for expected in PixelFormat::ALL
            .iter()
            .skip(1)
            .chain([&PixelFormat::ALL[0]])
        {
            format = format.next();
            assert_eq!(format, *expected);
        }
    }

    /// A copy of the coordinate is bound for somewhere else, so it is the two
    /// numbers and nothing else — no padding to keep a bar still, and no
    /// parentheses to take back off.
    #[test]
    fn a_copied_coordinate_is_the_pair_and_nothing_else() {
        assert_eq!(copied_coordinate([7, 9]), "7,9");
        assert_eq!(copied_coordinate([0, 0]), "0,0");
        // Padded on screen, bare in a copy, for the same pixel.
        assert_eq!(coordinate([7, 9], [1920, 1080]), "(0007, 0009)");
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
        let values = read(
            &rgb8([231, 128, 64]),
            &Display::default(),
            [3, 4],
            PixelFormat::Decimal,
        );
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
    /// windowed value of 0.5 is a shade of gray until a colormap is on, and
    /// then it is a color no column of digits describes.
    #[test]
    fn the_swatch_shows_the_color_the_pixel_comes_out_rather_than_its_value() {
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
        let gray = swatch_color(&display.map(&sample, Headroom::None));
        assert_eq!(gray.r, gray.g);
        assert_eq!(gray.g, gray.b);
        assert_eq!(gray.a, 255);

        display.colormap = Colormap::Viridis;
        let false_color = swatch_color(&display.map(&sample, Headroom::None));
        assert!(
            false_color.g > false_color.r && false_color.b > false_color.r,
            "the middle of viridis is teal, not gray: {false_color:?}"
        );
    }
}
