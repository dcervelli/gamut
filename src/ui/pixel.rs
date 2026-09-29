//! The readout that follows the pointer: which pixel it is over, what is
//! there, and a swatch of the color it comes out as.
//!
//! What "what is there" means is the reader's to choose, because one pixel
//! answers more than one question. The codes the file holds are the
//! measurement — the count a sensor recorded, the meter a terrain model
//! states — and are the reason anyone points at a pixel; hexadecimal is those
//! same codes written the way the rest of the world writes a color down. The
//! mapped values are what the window, the exposure and the false color have
//! made of them, and are the reason the pixel looks the way it does. A
//! picture that carries a depth map answers one question more — how far away
//! the thing at that pixel was — which is not in the pixel at all but in the
//! map beside it. One at a time rather than all at once: the bar is one line
//! long, and a reader working in one of them is not reading the others. The
//! swatch stands beside whichever is showing and settles the question none of
//! the others answers without asking anyone to read three decimals and
//! imagine a color.

use egui::{Label, RichText, Sense, StrokeKind, vec2};

use crate::image::depth::{Depth, Unit};
use crate::image::display::Mapped;
use crate::image::geo::Georeference;
use crate::image::{DecodedImage, Sample, Samples};
use crate::render::Color;

use super::Current;
use super::chrome::{Pass, measure};

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
/// Four ways of saying what is at one pixel, of which the bar shows one:
/// chosen from the menu the dot at the head of the readout opens, or stepped
/// through with the key that does the same.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PixelFormat {
    /// The codes the file holds, run together in hexadecimal with no prefix
    /// and no spaces — `E78040` — which is how a color is written down
    /// everywhere outside this window. The default, which the configuration
    /// file can change.
    #[default]
    Hex,
    /// Those codes as numbers, in the units the file keeps them in: the one
    /// of these that says what was measured.
    Decimal,
    /// What the window, the exposure and the false color have made of them.
    Mapped,
    /// How far away the pixel was, from the depth map the file carries
    /// beside the picture: a distance where the file says how to get one,
    /// and the map's own code where it does not. Offered for every file, so
    /// that the choice holds across a folder where only some have a map.
    Depth,
}

impl PixelFormat {
    /// Every format, in the order the menu offers them — the same order
    /// [`PixelFormat::next`] steps through, so the key and the cells agree
    /// about what comes after what.
    pub const ALL: [PixelFormat; 4] = [
        PixelFormat::Hex,
        PixelFormat::Decimal,
        PixelFormat::Mapped,
        PixelFormat::Depth,
    ];

    /// What the interface calls this format, for the cell that chooses it.
    pub fn label(self) -> &'static str {
        match self {
            PixelFormat::Hex => "Hex",
            PixelFormat::Decimal => "Decimal",
            PixelFormat::Mapped => "Mapped",
            PixelFormat::Depth => "Depth",
        }
    }

    /// The format a word names, as the configuration file writes it: the
    /// label, in any case.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|format| format.label().eq_ignore_ascii_case(value))
    }

    pub fn next(self) -> Self {
        match self {
            PixelFormat::Hex => PixelFormat::Decimal,
            PixelFormat::Decimal => PixelFormat::Mapped,
            PixelFormat::Mapped => PixelFormat::Depth,
            PixelFormat::Depth => PixelFormat::Hex,
        }
    }
}

/// How the pointer's place is written: which pixel of the raster it is, or
/// where that pixel is on the ground.
///
/// The two ground readings are offered only for a file that places its
/// pixels — a GeoTIFF — and each only where the file can answer it; wherever
/// it cannot, the readout is the pixel's, whatever was chosen, so that the
/// choice outlives a file with nothing to say about it and applies again at
/// the next map.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CoordinateFormat {
    /// Column and row, from the top left. The default, and every image's.
    #[default]
    Pixel,
    /// The file's own coordinates: easting and northing on its projection,
    /// in its units.
    Projected,
    /// Latitude and longitude on WGS 84.
    Geographic,
}

impl CoordinateFormat {
    /// Every format, in the order the menu offers them and the key steps
    /// through them.
    pub const ALL: [CoordinateFormat; 3] = [
        CoordinateFormat::Pixel,
        CoordinateFormat::Projected,
        CoordinateFormat::Geographic,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CoordinateFormat::Pixel => "Pixel",
            CoordinateFormat::Projected => "Projected",
            CoordinateFormat::Geographic => "Geographic",
        }
    }

    /// The format a word names, as the configuration file writes it: the
    /// label, in any case.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|format| format.label().eq_ignore_ascii_case(value))
    }

    /// Whether `georeference` can answer this format. The pixel is always
    /// there to be read.
    pub fn offered(self, georeference: Option<&Georeference>) -> bool {
        match (self, georeference) {
            (CoordinateFormat::Pixel, _) => true,
            (_, None) => false,
            (CoordinateFormat::Projected, Some(geo)) => geo.offers_projected(),
            (CoordinateFormat::Geographic, Some(geo)) => geo.offers_geographic(),
        }
    }

    /// What the readout actually shows for a file: this, where the file can
    /// answer it, and the pixel where it cannot.
    pub fn shown(self, georeference: Option<&Georeference>) -> Self {
        if self.offered(georeference) {
            self
        } else {
            CoordinateFormat::Pixel
        }
    }

    /// The format after the one shown, among those the file offers: what the
    /// key steps to. A file that offers only the pixel stays on it.
    pub fn next(self, georeference: Option<&Georeference>) -> Self {
        let shown = self.shown(georeference);
        let at = Self::ALL
            .iter()
            .position(|format| *format == shown)
            .unwrap_or(0);
        (1..=Self::ALL.len())
            .map(|step| Self::ALL[(at + step) % Self::ALL.len()])
            .find(|format| format.offered(georeference))
            .unwrap_or(CoordinateFormat::Pixel)
    }
}

/// How a latitude and a longitude are written, where they are shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GeographicFormat {
    /// Signed degrees with a decimal fraction, latitude first —
    /// `46.95108, 7.43864` — which is what a map pasted into takes.
    #[default]
    Decimal,
    /// Degrees, minutes and seconds with the hemisphere —
    /// `46°57'03.89"N 7°26'19.07"E` — which is how a chart is marked.
    Dms,
}

impl GeographicFormat {
    pub const ALL: [GeographicFormat; 2] = [GeographicFormat::Decimal, GeographicFormat::Dms];

    pub fn label(self) -> &'static str {
        match self {
            GeographicFormat::Decimal => "Decimal",
            GeographicFormat::Dms => "DMS",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|format| format.label().eq_ignore_ascii_case(value))
    }

    pub fn next(self) -> Self {
        match self {
            GeographicFormat::Decimal => GeographicFormat::Dms,
            GeographicFormat::Dms => GeographicFormat::Decimal,
        }
    }
}

/// Draws the readout for the pixel the pointer is over, if it is over one, in
/// what is left of the bottom bar between the dot that chooses the format and
/// the state text at the other end.
///
/// The coordinate leads, then the swatch, then the value it stands for. The
/// swatch belongs to the color it depicts rather than to the pixel's address,
/// so it sits in front of the value and moves with it — and the coordinate
/// is set to a fixed width so that they stay put while the pointer moves.
/// Whatever else has to go in a narrow bar, the coordinate stays: it is the
/// one thing the readout says that nothing else in the window says.
pub(super) fn show(pass: &mut Pass, ui: &mut egui::Ui) {
    let Some(current) = pass.current else {
        return;
    };
    let Some(at) = pass.input.pointer else {
        return;
    };
    let Some(sample) = current.sample(at[0], at[1]) else {
        return;
    };
    let mapped = current.display.map(&sample, pass.input.headroom);
    let ink: egui::Color32 = pass.theme.text_primary.into();

    let coordinate = place(
        current,
        at,
        pass.panels.coordinate_format,
        pass.panels.geographic_format,
        true,
    );
    ui.add(Label::new(RichText::new(coordinate).monospace().color(ink)).truncate());

    // A bar too short for the swatch gets the words alone, the way a panel
    // too narrow for a button gets no button; one too narrow for the values
    // loses the separator and the swatch with them, rather than leaving the
    // separator parting the coordinate from nothing.
    let values = value(
        &current.image,
        &sample,
        &mapped,
        current.depth(at[0], at[1]),
        pass.panels.pixel_format,
    );
    let swatch = ui.available_height() >= SWATCH;
    let mut needed = GAP + measure(ui, SEPARATOR) + GAP + measure(ui, &values);
    if swatch {
        needed += SWATCH + GAP;
    }
    if ui.available_width() < needed {
        return;
    }
    ui.add_space(GAP);
    ui.add(Label::new(RichText::new(SEPARATOR).color(ink)));
    ui.add_space(GAP);
    if swatch {
        let (rect, _) = ui.allocate_exact_size(vec2(SWATCH, SWATCH), Sense::HOVER);
        ui.painter().rect_filled(rect, 0.0, swatch_color(&mapped));
        // An outline, so that a transparent or near-black pixel still reads
        // as a swatch showing something rather than as a gap in the bar.
        ui.painter().rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.0, pass.theme.border),
            StrokeKind::Inside,
        );
        ui.add_space(GAP);
    }
    ui.add(Label::new(RichText::new(values).color(ink)).truncate());
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

/// Where the pointer is, in `format` where the file can say and as the pixel
/// where it cannot: set for the bar when `padded`, and for a copy when not.
///
/// `at` is in the picture as it is turned on screen; the file's placement is
/// of the raster as it is stored, which is what is read through here.
pub fn place(
    current: &Current,
    at: [u32; 2],
    format: CoordinateFormat,
    geographic: GeographicFormat,
    padded: bool,
) -> String {
    let pixel = || match padded {
        true => coordinate(at, current.pixels()),
        false => copied_coordinate(at),
    };
    let georeference = current.exif.georeference.as_ref();
    let Some(geo) = georeference.filter(|_| format.shown(georeference) != CoordinateFormat::Pixel)
    else {
        return pixel();
    };
    let size = [current.image.width, current.image.height];
    let stored = current.turn.stored(at, size);
    match format {
        CoordinateFormat::Projected => projected(geo, stored, size, padded),
        CoordinateFormat::Geographic => match geo.latitude_longitude(stored) {
            Some(point) => latitude_longitude(point, geographic, padded),
            None => pixel(),
        },
        CoordinateFormat::Pixel => pixel(),
    }
}

/// The file's own coordinates at the middle of the stored pixel `at`, with
/// as many places as a pixel of its size is worth and the unit after them.
///
/// Padded, each is as wide as the widest the raster's corners come to, for
/// the reason the pixel's coordinate is: so that what follows it holds still.
fn projected(geo: &Georeference, at: [u32; 2], size: [u32; 2], padded: bool) -> String {
    let places = places_for(geo.step(), 0, 6);
    let write = |value: f64| format!("{value:.places$}");
    let [x, y] = geo.model(at).map(write);
    let unit = geo
        .unit()
        .map(|unit| format!(" {unit}"))
        .unwrap_or_default();
    if !padded {
        return format!("{x},{y}");
    }
    let [right, bottom] = [size[0].saturating_sub(1), size[1].saturating_sub(1)];
    let corners =
        [[0, 0], [right, 0], [0, bottom], [right, bottom]].map(|corner| geo.model(corner));
    let widest = |axis: usize| {
        corners
            .iter()
            .map(|corner| write(corner[axis]).len())
            .max()
            .unwrap_or(0)
    };
    format!("({x:>w$}, {y:>h$}){unit}", w = widest(0), h = widest(1))
}

/// How many places after the point a coordinate is worth when one pixel
/// covers `step` of its units: enough to tell one pixel from the next and no
/// more, between `least` and `most`.
fn places_for(step: f64, least: usize, most: usize) -> usize {
    if !(step.is_finite() && step > 0.0) {
        return most;
    }
    (-step.log10()).ceil().clamp(least as f64, most as f64) as usize
}

/// Places after the point of a decimal degree: six, about a tenth of a
/// meter, which is what a map pasted into takes and gives back.
const DEGREE_PLACES: usize = 6;

/// Places after the point of the seconds: two, about a third of a meter,
/// which is how a chart or a GPS writes them.
const SECOND_PLACES: usize = 2;

/// A latitude and a longitude, in degrees, written in `format`, each to a
/// fixed number of places whatever the size of a pixel, so that a pasted or
/// compared coordinate always reads the same way.
///
/// Padded, a latitude is written as wide as the widest one is and a
/// longitude likewise, so that the readout holds still across the equator
/// and the meridian.
fn latitude_longitude(
    [latitude, longitude]: [f64; 2],
    format: GeographicFormat,
    padded: bool,
) -> String {
    match format {
        GeographicFormat::Decimal => {
            let places = DEGREE_PLACES;
            let (lat, lon) = (
                format!("{latitude:.places$}"),
                format!("{longitude:.places$}"),
            );
            if !padded {
                return format!("{lat},{lon}");
            }
            // "-90." and "-180." ahead of the places.
            let (w, h) = (places + 4, places + 5);
            format!("{lat:>w$}, {lon:>h$}")
        }
        GeographicFormat::Dms => {
            let places = SECOND_PLACES;
            let (lat, lon) = (
                dms(latitude, ['N', 'S'], places),
                dms(longitude, ['E', 'W'], places),
            );
            if !padded {
                return format!("{lat} {lon}");
            }
            // As wide as the largest angle the hemisphere reaches.
            let (w, h) = (
                dms(-90.0, ['N', 'S'], places).chars().count(),
                dms(-180.0, ['E', 'W'], places).chars().count(),
            );
            format!("{lat:>w$} {lon:>h$}")
        }
    }
}

/// One angle as degrees, minutes and seconds, the seconds to `places`, and
/// the hemisphere after them. Minutes and seconds are two digits each, as a
/// chart writes them; the rounding is done once on the whole angle, so that
/// 59.995 seconds carries into the minute rather than reading as 60.
fn dms(angle: f64, hemispheres: [char; 2], places: usize) -> String {
    let hemisphere = if angle < 0.0 {
        hemispheres[1]
    } else {
        hemispheres[0]
    };
    let scale = 10f64.powi(places as i32);
    let total = (angle.abs() * 3600.0 * scale).round() as u64;
    let unit = 3600 * scale as u64;
    let (degrees, rest) = (total / unit, total % unit);
    let (minutes, rest) = (rest / (60 * scale as u64), rest % (60 * scale as u64));
    let seconds = rest as f64 / scale;
    let width = if places == 0 { 2 } else { places + 3 };
    format!("{degrees}\u{00b0}{minutes:02}'{seconds:0width$.places$}\"{hemisphere}")
}

/// What the readout says in place of a depth, for a picture with no map.
pub const NO_DEPTH: &str = "(no depth)";

/// What is there, in `format`. Also what a copy of the value contains, so
/// that what is copied is exactly what was read. `depth` is the map under
/// the pixel, where the picture has one.
pub fn value(
    image: &DecodedImage,
    sample: &Sample,
    mapped: &Mapped,
    depth: Option<Depth>,
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
        PixelFormat::Depth => match depth {
            Some(depth) => distance(depth),
            None => NO_DEPTH.to_string(),
        },
    }
}

/// A depth, as a distance in the file's unit where it says how to get one —
/// to the millimeter, whichever unit that is — and otherwise as the code the
/// map holds, in the terms `Decimal` writes a pixel's codes in.
fn distance(depth: Depth) -> String {
    match depth.distance {
        Some((value, Unit::Meters)) => format!("{value:.3} m"),
        Some((value, Unit::Millimeters)) => format!("{value:.0} mm"),
        Some((value, Unit::Unknown)) => format!("{value:.3}"),
        None => component(depth.stored, depth.float),
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
        let sample = image.sample(at[0], at[1], None).expect("inside the image");
        let depth = image
            .depth
            .as_ref()
            .and_then(|map| map.at(at[0], at[1], image.width, image.height));
        value(
            image,
            &sample,
            &display.map(&sample, Headroom::None),
            depth,
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

    /// The depth is the map's, not the pixel's: a distance where the file
    /// states the planes, the code where it does not, and a word saying so
    /// where there is no map at all.
    #[test]
    fn depth_reads_the_map_beside_the_picture() {
        use crate::image::depth::{DepthMap, Range};
        use std::sync::Arc;
        let display = Display::default();
        let mut image = rgb8([231, 128, 64]);
        assert_eq!(read(&image, &display, [3, 4], PixelFormat::Depth), NO_DEPTH);

        let map = |range| {
            Arc::new(DepthMap {
                width: 2,
                height: 1,
                samples: Samples::U8 {
                    channels: Channels::Gray,
                    data: vec![0, 51],
                },
                range,
            })
        };
        image.depth = Some(map(Range::Unstated));
        assert_eq!(read(&image, &display, [3, 4], PixelFormat::Depth), "51");
        assert_eq!(read(&image, &display, [0, 0], PixelFormat::Depth), "0");

        image.depth = Some(map(Range::Linear {
            near: 1.0,
            far: 5.0,
            unit: Unit::Meters,
        }));
        assert_eq!(
            read(&image, &display, [3, 4], PixelFormat::Depth),
            "1.800 m"
        );
        image.depth = Some(map(Range::Linear {
            near: 1000.0,
            far: 5000.0,
            unit: Unit::Millimeters,
        }));
        assert_eq!(
            read(&image, &display, [3, 4], PixelFormat::Depth),
            "1800 mm"
        );
        // The other formats still read the pixel.
        assert_eq!(read(&image, &display, [3, 4], PixelFormat::Hex), "E78040");
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

    /// A georeference offering what `placed` offers: projected where the
    /// model is (1), geographic too where the code is one the table holds.
    fn georeference(model: u16, system: u16) -> Georeference {
        use crate::image::geo::Tags;
        let key = if model == 2 { 2048 } else { 3072 };
        Georeference::read(&Tags {
            directory: vec![1, 1, 0, 2, 1024, 0, 1, model, key, 0, 1, system],
            scale: vec![1.0, 1.0, 0.0],
            tiepoint: vec![0.0, 0.0, 0.0, 500_000.0, 4_500_000.0, 0.0],
            ..Tags::default()
        })
        .expect("placed")
    }

    /// The key steps through what the file offers, from what the bar is
    /// showing, and a choice the file cannot answer is shown as the pixel
    /// without being forgotten.
    #[test]
    fn the_coordinates_step_through_what_the_file_offers() {
        use CoordinateFormat::*;
        let both = georeference(1, 32618);
        let projected = georeference(1, 32767);
        let geographic = georeference(2, 4326);

        assert_eq!(Pixel.next(Some(&both)), Projected);
        assert_eq!(Projected.next(Some(&both)), Geographic);
        assert_eq!(Geographic.next(Some(&both)), Pixel);

        assert_eq!(Projected.next(Some(&projected)), Pixel);
        assert_eq!(Pixel.next(Some(&geographic)), Geographic);
        // Shown as the pixel, so the next is the first thing after it.
        assert_eq!(Geographic.shown(Some(&projected)), Pixel);
        assert_eq!(Geographic.next(Some(&projected)), Projected);

        for format in CoordinateFormat::ALL {
            assert_eq!(format.shown(None), Pixel);
            assert_eq!(format.next(None), Pixel);
        }
    }

    /// A latitude is written to six decimal places or to hundredths of a
    /// second, whatever the pixel, and the seconds round as a whole,
    /// carrying into the minute.
    #[test]
    fn a_latitude_is_written_to_fixed_places() {
        let bern = [46.951_082_77, 7.438_632_42];
        assert_eq!(
            latitude_longitude(bern, GeographicFormat::Decimal, false),
            "46.951083,7.438632"
        );
        assert_eq!(
            latitude_longitude(bern, GeographicFormat::Decimal, true),
            " 46.951083,    7.438632"
        );
        assert_eq!(
            latitude_longitude(bern, GeographicFormat::Dms, false),
            "46\u{00b0}57'03.90\"N 7\u{00b0}26'19.08\"E"
        );
        assert_eq!(dms(-0.999_999_9, ['N', 'S'], 2), "1\u{00b0}00'00.00\"S");
        assert_eq!(dms(-73.5, ['E', 'W'], 0), "73\u{00b0}30'00\"W");

        // Padded, a latitude and a longitude are as wide at the equator as
        // at the poles, so that nothing after them moves.
        let wide = latitude_longitude([-89.5, -179.5], GeographicFormat::Dms, true);
        let narrow = latitude_longitude([0.5, 0.5], GeographicFormat::Dms, true);
        assert_eq!(wide.chars().count(), narrow.chars().count());
        let wide = latitude_longitude([-89.5, -179.5], GeographicFormat::Decimal, true);
        let narrow = latitude_longitude([0.5, 0.5], GeographicFormat::Decimal, true);
        assert_eq!(wide.len(), narrow.len());
    }

    /// The file's own coordinates keep as many places as a pixel is worth,
    /// wear the file's unit, and are as wide as the raster's widest corner.
    #[test]
    fn projected_coordinates_are_written_in_the_file_s_units() {
        use crate::image::geo::Tags;
        let geo = Georeference::read(&Tags {
            // Projected, in meters.
            directory: vec![1, 1, 0, 2, 1024, 0, 1, 1, 3076, 0, 1, 9001],
            scale: vec![0.5, 0.5, 0.0],
            tiepoint: vec![0.0, 0.0, 0.0, 995.0, 2000.0, 0.0],
            ..Tags::default()
        })
        .expect("placed");
        assert_eq!(projected(&geo, [0, 0], [100, 100], false), "995.2,1999.8");
        // The far column reaches 1044.8, a digit more than the first.
        assert_eq!(
            projected(&geo, [0, 0], [100, 100], true),
            "( 995.2, 1999.8) m"
        );
        assert_eq!(
            projected(&geo, [99, 99], [100, 100], true),
            "(1044.8, 1950.2) m"
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
        let sample = image.sample(0, 0, None).expect("inside the image");

        let mut display = Display::default();
        let gray = swatch_color(&display.map(&sample, Headroom::None));
        assert_eq!(gray.r, gray.g);
        assert_eq!(gray.g, gray.b);
        assert_eq!(gray.a, 255);

        display.set_colormap(Colormap::Viridis, true);
        let false_color = swatch_color(&display.map(&sample, Headroom::None));
        assert!(
            false_color.g > false_color.r && false_color.b > false_color.r,
            "the middle of viridis is teal, not gray: {false_color:?}"
        );
    }
}
