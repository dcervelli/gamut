//! What the picture is measured in along each of its two axes as shown: its
//! own pixels, a print's inches or millimeters at the density the file
//! says, or the file's own coordinates on the ground. The one reading the
//! grid is spaced by, the region's labels are written in, and the
//! readout's print coordinate is read from, so that the three cannot come
//! to disagree about where a place on the picture is.
//!
//! Only a measure that runs straight along the picture's rows and columns
//! is one: a latitude and a longitude on a projected raster curve across
//! it, and a raster turned on the ground slants its coordinates across it,
//! so neither has lines of its own to draw. Both are measured on the ground
//! the file is placed on where it can be, and in pixels where it cannot.

use crate::image::density;
use crate::image::geo::Georeference;
use crate::image::orient;

use super::Current;
use super::pixel::{Answers, CoordinateFormat};

/// What a measure is in.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Unit {
    /// The picture's own pixels.
    Pixel,
    /// A print's, at the file's density.
    Print(density::Unit),
    /// The file's own coordinates, in the unit it names where it names one.
    Ground(Option<&'static str>),
}

/// One axis of a measure: what it reads at the picture's near edge, and how
/// far it goes with each pixel along — less than nothing where it runs the
/// other way, as a northing runs up a raster whose rows run down.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Axis {
    start: f64,
    per_pixel: f64,
}

/// The picture as shown, measured across and down.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Ruler {
    axes: [Axis; 2],
    pub unit: Unit,
}

impl Ruler {
    /// The picture measured in its own pixels: column and row.
    pub const PIXELS: Ruler = Ruler {
        axes: [Axis {
            start: 0.0,
            per_pixel: 1.0,
        }; 2],
        unit: Unit::Pixel,
    };

    /// What `current` is measured in where its coordinate is written in
    /// `format` and a print is measured in `print`: the print's own unit,
    /// the ground for either way of saying where on it a pixel is — the
    /// file's own coordinates for a latitude, which has no lines of its
    /// own — and the pixels for the rest, or where the file cannot answer.
    pub fn of(current: &Current, format: CoordinateFormat, print: density::Unit) -> Ruler {
        let answers = Answers::of(Some(current));
        match format.shown(answers) {
            CoordinateFormat::Pixel | CoordinateFormat::Direction => Ruler::PIXELS,
            CoordinateFormat::Print => answers
                .density
                .map_or(Ruler::PIXELS, |density| Ruler::print(density, print)),
            CoordinateFormat::Projected | CoordinateFormat::Geographic => answers
                .georeference
                .and_then(|geo| Ruler::ground(current, geo))
                .unwrap_or(Ruler::PIXELS),
        }
    }

    /// A print `density` pixels to the inch across and down, measured in
    /// `unit` from its top left.
    pub fn print(density: [f64; 2], unit: density::Unit) -> Ruler {
        Ruler {
            axes: density.map(|per_inch| Axis {
                start: 0.0,
                per_pixel: unit.per_inch() / per_inch,
            }),
            unit: Unit::Print(unit),
        }
    }

    /// The ground under `current` in the coordinates `geo` places it in,
    /// through whatever the picture has been turned by: across the turned
    /// picture is whichever of the file's two axes the turn lays across it.
    /// `None` for a raster turned on the ground, whose coordinates slant.
    fn ground(current: &Current, geo: &Georeference) -> Option<Ruler> {
        if !geo.aligned() {
            return None;
        }
        let stored = [current.image.width, current.image.height].map(f64::from);
        let at = |turned: [f64; 2]| geo.ground(current.turn.stored_point(turned, stored));
        let origin = at([0.0, 0.0]);
        let steps =
            [at([1.0, 0.0]), at([0.0, 1.0])].map(|step| [step[0] - origin[0], step[1] - origin[1]]);
        // Which of the file's axes runs across: the one that moves with a
        // step across, the other standing still.
        let across = usize::from(steps[0][1].abs() > steps[0][0].abs());
        let axis = |turned: usize, model: usize| Axis {
            start: origin[model],
            per_pixel: steps[turned][model],
        };
        let axes = [axis(0, across), axis(1, 1 - across)];
        axes.iter()
            .all(|axis| axis.per_pixel.is_normal() && axis.start.is_finite())
            .then_some(Ruler {
                axes,
                unit: Unit::Ground(geo.unit()),
            })
    }

    /// A measure reading `start` at the picture's top left and going
    /// `per_pixel` with each pixel across and down, for the tests of what
    /// is drawn by one.
    #[cfg(test)]
    pub fn test(start: [f64; 2], per_pixel: [f64; 2], unit: Unit) -> Ruler {
        Ruler {
            axes: [0, 1].map(|axis| Axis {
                start: start[axis],
                per_pixel: per_pixel[axis],
            }),
            unit,
        }
    }

    /// What the measure reads `pixels` along `axis` from the picture's near
    /// edge — a point, not a pixel: a pixel's middle is half a pixel in.
    pub fn at(&self, axis: usize, pixels: f64) -> f64 {
        let Axis { start, per_pixel } = self.axes[axis];
        start + pixels * per_pixel
    }

    /// Where along `axis` the measure reads `value`, in pixels from the
    /// picture's near edge: the other way round from [`Ruler::at`].
    pub fn pixels(&self, axis: usize, value: f64) -> f64 {
        let Axis { start, per_pixel } = self.axes[axis];
        (value - start) / per_pixel
    }

    /// How far `pixels` reach along `axis`, in the measure's unit.
    pub fn length(&self, axis: usize, pixels: f64) -> f64 {
        pixels * self.per_pixel(axis)
    }

    /// How far one pixel reaches along `axis`.
    pub fn per_pixel(&self, axis: usize) -> f64 {
        self.axes[axis].per_pixel.abs()
    }

    /// How many places after the point a reading is worth: enough to tell
    /// one pixel from the next along the axis a pixel covers least of, and
    /// none for the pixels themselves.
    pub fn places(&self) -> usize {
        match self.unit {
            Unit::Pixel => 0,
            _ => places_for(self.per_pixel(0).min(self.per_pixel(1)), 0, 6),
        }
    }

    /// `value` written to [`Ruler::places`], with no sign on a nought.
    pub fn write(&self, value: f64) -> String {
        let places = self.places();
        let written = format!("{value:.places$}");
        match written.strip_prefix('-') {
            Some(rest) if rest.chars().all(|c| c == '0' || c == '.') => rest.to_string(),
            _ => written,
        }
    }

    /// What follows a reading, space and all: nothing for a pixel, nor for
    /// a file that does not say what its coordinates are in, and an angle's
    /// mark set against the number as an angle is written.
    pub fn suffix(&self) -> String {
        match self.unit {
            Unit::Pixel | Unit::Ground(None) => String::new(),
            Unit::Print(unit) => format!(" {}", unit.word()),
            Unit::Ground(Some(unit)) if angle(unit) => unit.to_string(),
            Unit::Ground(Some(unit)) => format!(" {unit}"),
        }
    }
}

/// Whether `unit` is an angle's mark, which goes against its number.
pub(super) fn angle(unit: &str) -> bool {
    matches!(unit, "\u{00b0}" | "\u{2032}" | "\u{2033}")
}

/// How many pixels of the picture as shown go to an inch across and down,
/// where the file says: its density, which is of the file as stored, laid
/// along the picture as it is shown — the orientation the file asks for,
/// then the turn — and spread over whichever of the file's images is on
/// screen, which a map beside the picture may hold at a size of its own.
pub fn density(current: &Current) -> Option<[f64; 2]> {
    let density = current.exif.density?;
    let stood = current.exif.orientation.is_some_and(orient::quarter_turn);
    let upright = if stood { density.swapped() } else { density }.per_inch;
    let picture = current.picture().0;
    let [width, height] = [
        f64::from(current.image.width) / f64::from(picture.width),
        f64::from(current.image.height) / f64::from(picture.height),
    ];
    let scaled = [upright[0] * width, upright[1] * height];
    scaled
        .iter()
        .all(|value| value.is_normal())
        .then(|| current.turn.size(scaled))
}

/// How many places after the point a coordinate is worth when one pixel
/// covers `step` of its units: enough to tell one pixel from the next and no
/// more, between `least` and `most`.
pub(super) fn places_for(step: f64, least: usize, most: usize) -> usize {
    if !(step.is_finite() && step > 0.0) {
        return most;
    }
    (-step.log10()).ceil().clamp(least as f64, most as f64) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::color::ColorSpace;
    use crate::image::density::Density;
    use crate::image::geo::Tags;
    use crate::image::orient::Turn;
    use crate::image::{AlphaMode, Channels, DecodedImage, Samples};

    /// A gray picture `size` pixels across and down, on screen.
    fn picture(size: [u32; 2]) -> Current {
        let image = DecodedImage::new(
            size[0],
            size[1],
            Samples::U8 {
                channels: Channels::Gray,
                data: vec![128; (size[0] * size[1]) as usize],
            },
            ColorSpace::SRGB,
            AlphaMode::Opaque,
        );
        Current::of(image, "map.tif")
    }

    /// On UTM zone 18N, in meters, a meter to the pixel, its first corner
    /// at (500000, 4500000): a projection there is a way back to the globe
    /// from, so it offers a latitude as well.
    fn placed(size: [u32; 2]) -> Current {
        let mut current = picture(size);
        current.exif.georeference = Georeference::read(&Tags {
            directory: vec![
                1, 1, 0, 3, 1024, 0, 1, 1, 3072, 0, 1, 32618, 3076, 0, 1, 9001,
            ],
            scale: vec![1.0, 1.0, 0.0],
            tiepoint: vec![0.0, 0.0, 0.0, 500_000.0, 4_500_000.0, 0.0],
            ..Tags::default()
        });
        current
    }

    /// The ground is measured from the raster's corner, eastings across
    /// and northings falling down it; a latitude, which has no lines of its
    /// own, is measured on the same ground.
    #[test]
    fn the_ground_is_measured_in_the_files_own_coordinates() {
        let current = placed([100, 50]);
        for format in [CoordinateFormat::Projected, CoordinateFormat::Geographic] {
            let ruler = Ruler::of(&current, format, density::Unit::Inches);
            assert_eq!(ruler.unit, Unit::Ground(Some("m")));
            assert_eq!(ruler.at(0, 0.0), 500_000.0);
            assert_eq!(ruler.at(0, 100.0), 500_100.0);
            assert_eq!(ruler.at(1, 0.0), 4_500_000.0);
            assert_eq!(ruler.at(1, 50.0), 4_499_950.0);
            assert_eq!(ruler.pixels(1, 4_499_990.0), 10.0);
        }
        // The way a pixel looks has nothing to do with the ground.
        let ruler = Ruler::of(&current, CoordinateFormat::Direction, density::Unit::Inches);
        assert_eq!(ruler, Ruler::PIXELS);
    }

    /// Turned a quarter clockwise, what was the raster's left edge is the
    /// top: the northings run across, rising to the right, and the
    /// eastings down.
    #[test]
    fn the_ground_turns_with_the_picture() {
        let mut current = placed([100, 50]);
        current.turn = Turn::NONE.clockwise();
        let ruler = Ruler::of(&current, CoordinateFormat::Projected, density::Unit::Inches);
        assert_eq!(ruler.at(0, 0.0), 4_499_950.0);
        assert_eq!(ruler.at(0, 50.0), 4_500_000.0);
        assert_eq!(ruler.at(1, 0.0), 500_000.0);
        assert_eq!(ruler.at(1, 100.0), 500_100.0);
    }

    /// A raster turned on the ground has coordinates slanting across it,
    /// which no line along its rows follows: it is measured in pixels.
    #[test]
    fn a_slanted_raster_is_measured_in_pixels() {
        let mut current = picture([100, 100]);
        current.exif.georeference = Georeference::read(&Tags {
            directory: vec![1, 1, 0, 2, 1024, 0, 1, 1, 3072, 0, 1, 32618],
            transform: vec![
                2.0, 1.0, 0.0, 100.0, //
                0.5, -2.0, 0.0, 500.0, //
                0.0, 0.0, 0.0, 0.0, //
                0.0, 0.0, 0.0, 1.0,
            ],
            ..Tags::default()
        });
        assert!(current.exif.georeference.is_some());
        let ruler = Ruler::of(&current, CoordinateFormat::Projected, density::Unit::Inches);
        assert_eq!(ruler, Ruler::PIXELS);
    }

    /// The print is measured at the file's density, laid along the picture
    /// as it is turned; with no density it is measured in pixels.
    #[test]
    fn a_print_is_measured_at_the_files_density() {
        let mut current = picture([600, 300]);
        assert_eq!(
            Ruler::of(&current, CoordinateFormat::Print, density::Unit::Inches),
            Ruler::PIXELS
        );
        current.exif.density = Density::new([300.0, 150.0], 25.4);
        assert_eq!(density(&current), Some([300.0, 150.0]));
        let ruler = Ruler::of(&current, CoordinateFormat::Print, density::Unit::Inches);
        assert_eq!(ruler.at(0, 600.0), 2.0);
        assert_eq!(ruler.at(1, 300.0), 2.0);
        current.turn = Turn::NONE.clockwise();
        assert_eq!(density(&current), Some([150.0, 300.0]));
    }

    #[test]
    fn pixels_measure_as_they_are_counted() {
        let ruler = Ruler::PIXELS;
        assert_eq!(ruler.at(0, 12.0), 12.0);
        assert_eq!(ruler.pixels(1, 7.0), 7.0);
        assert_eq!(ruler.write(1920.0), "1920");
        assert_eq!(ruler.suffix(), "");
    }

    /// A 300-pixel-to-the-inch print reads three places of an inch, and two
    /// of a millimeter, which is what tells one pixel from the next.
    #[test]
    fn a_print_is_measured_from_its_top_left() {
        let inches = Ruler::print([300.0, 150.0], density::Unit::Inches);
        assert_eq!(inches.at(0, 300.0), 1.0);
        assert_eq!(inches.at(1, 300.0), 2.0);
        assert_eq!(inches.places(), 3);
        assert_eq!(inches.write(inches.at(0, 450.0)), "1.500");
        assert_eq!(inches.suffix(), " in");
        let mm = Ruler::print([300.0, 300.0], density::Unit::Millimeters);
        assert!((mm.at(0, 300.0) - 25.4).abs() < 1e-9);
        assert_eq!(mm.places(), 2);
        assert_eq!(mm.suffix(), " mm");
    }

    #[test]
    fn a_nought_is_written_without_a_sign() {
        let ruler = Ruler::print([100.0, 100.0], density::Unit::Inches);
        assert_eq!(ruler.write(-0.0001), "0.00");
        assert_eq!(ruler.write(-0.5), "-0.50");
    }

    #[test]
    fn an_angle_is_written_against_its_number() {
        let ruler = Ruler {
            unit: Unit::Ground(Some("\u{00b0}")),
            ..Ruler::PIXELS
        };
        assert_eq!(ruler.suffix(), "\u{00b0}");
        let meters = Ruler {
            unit: Unit::Ground(Some("m")),
            ..Ruler::PIXELS
        };
        assert_eq!(meters.suffix(), " m");
    }
}
