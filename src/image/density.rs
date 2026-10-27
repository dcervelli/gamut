//! How many of the picture's pixels the file says go to an inch, and what that
//! makes of it on paper: the size it prints at, and the sheet that size is,
//! where it is one.
//!
//! Three places say it. The EXIF block's `XResolution`, `YResolution` and
//! `ResolutionUnit`, which [`super::exif`] reads, are asked first: they are
//! what an editor writes and keeps up to date. Where the block says nothing,
//! the container's own word is taken — a PNG's `pHYs` chunk, a JPEG's JFIF
//! header. A unit that is only an aspect ratio — `pHYs` of unknown unit,
//! JFIF's density of 0 — says nothing about size and is left. A density is
//! taken at face value, however unlikely: 72 dpi on a camera's picture is what
//! the file says, and the print size it makes is what the file means.

use std::io::SeekFrom;

use super::decode::ReadSeek;

/// Millimeters to the inch.
const MM_PER_INCH: f64 = 25.4;

/// Pixels to the inch, across and down, in the picture as stored.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Density {
    pub per_inch: [f64; 2],
}

impl Density {
    /// A density in the file's own unit: `per_unit` pixels to `mm`
    /// millimeters. `None` where either axis is no density.
    pub fn new(per_unit: [f64; 2], mm: f64) -> Option<Density> {
        let per_inch = per_unit.map(|value| value * MM_PER_INCH / mm);
        per_inch
            .iter()
            .all(|value| value.is_finite() && *value > 0.0)
            .then_some(Density { per_inch })
    }

    /// Pixels to the millimeter, across and down.
    pub fn per_mm(self) -> [f64; 2] {
        self.per_inch.map(|value| value / MM_PER_INCH)
    }

    /// The same density across the picture turned a quarter.
    pub fn swapped(self) -> Density {
        let [x, y] = self.per_inch;
        Density { per_inch: [y, x] }
    }

    /// What the panel says of it: `600 dpi`, or `600 × 300 dpi` where the
    /// two axes differ.
    pub fn words(self) -> String {
        let [x, y] = self.per_inch.map(|value| decimal(value, 1));
        if x == y {
            format!("{x} dpi")
        } else {
            format!("{x} \u{00d7} {y} dpi")
        }
    }

    /// The size `pixels` print at, in the unit of the sheet it matches —
    /// and the sheet — or in `unit` where it matches none:
    /// `420 × 297 mm · A3`, `8.5 × 11 in · Letter`, `56 × 42 in`.
    pub fn print_size(self, pixels: [u32; 2], unit: Unit) -> String {
        let inches = [0, 1].map(|axis| f64::from(pixels[axis]) / self.per_inch[axis]);
        let sheet = Paper::matching(inches, self.per_inch);
        let unit = sheet.map_or(unit, |paper| paper.unit);
        let size = match unit {
            Unit::Millimeters => {
                let [w, h] = inches.map(|side| decimal(side * MM_PER_INCH, 0));
                format!("{w} \u{00d7} {h} mm")
            }
            Unit::Inches => {
                let [w, h] = inches.map(|side| decimal(side, 2));
                format!("{w} \u{00d7} {h} in")
            }
        };
        match sheet {
            Some(paper) => format!("{size} \u{00b7} {}", paper.name),
            None => size,
        }
    }
}

/// What a length is written in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unit {
    Millimeters,
    Inches,
}

/// A sheet a print size can be: its name, its sides in millimeters, the
/// short one first, and the unit the sheet is spoken of in.
struct Paper {
    name: &'static str,
    mm: [f64; 2],
    unit: Unit,
}

/// The sheets a print size is named as: ISO 216's A series, which most of the
/// world prints on, and the North American sizes.
const PAPERS: &[Paper] = &[
    Paper {
        name: "A0",
        mm: [841.0, 1189.0],
        unit: Unit::Millimeters,
    },
    Paper {
        name: "A1",
        mm: [594.0, 841.0],
        unit: Unit::Millimeters,
    },
    Paper {
        name: "A2",
        mm: [420.0, 594.0],
        unit: Unit::Millimeters,
    },
    Paper {
        name: "A3",
        mm: [297.0, 420.0],
        unit: Unit::Millimeters,
    },
    Paper {
        name: "A4",
        mm: [210.0, 297.0],
        unit: Unit::Millimeters,
    },
    Paper {
        name: "A5",
        mm: [148.0, 210.0],
        unit: Unit::Millimeters,
    },
    Paper {
        name: "A6",
        mm: [105.0, 148.0],
        unit: Unit::Millimeters,
    },
    Paper {
        name: "Letter",
        mm: [215.9, 279.4],
        unit: Unit::Inches,
    },
    Paper {
        name: "Legal",
        mm: [215.9, 355.6],
        unit: Unit::Inches,
    },
    Paper {
        name: "Tabloid",
        mm: [279.4, 431.8],
        unit: Unit::Inches,
    },
];

impl Paper {
    /// The sheet a print of `inches` is, either way up: each side within a
    /// millimeter and a pixel of the sheet's, the pixel being what a size
    /// worked out to whole pixels may be off by.
    fn matching(inches: [f64; 2], per_inch: [f64; 2]) -> Option<&'static Paper> {
        let mm = inches.map(|side| side * MM_PER_INCH);
        let slack = per_inch.map(|density| 1.0 + MM_PER_INCH / density);
        let near =
            |sides: [f64; 2]| (0..2).all(|axis| (mm[axis] - sides[axis]).abs() <= slack[axis]);
        PAPERS.iter().find(|paper| {
            let [short, long] = paper.mm;
            near([short, long]) || near([long, short])
        })
    }
}

/// `value` to at most `places` decimals, with no zeros trailing.
fn decimal(value: f64, places: usize) -> String {
    let written = format!("{value:.places$}");
    if written.contains('.') {
        written
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    } else {
        written
    }
}

/// The density in a PNG's `pHYs` chunk, which comes before the image data
/// if it comes at all, so the walk stops there.
pub fn in_png(source: &mut dyn ReadSeek) -> Option<Density> {
    source.seek(SeekFrom::Start(0)).ok()?;
    let mut signature = [0u8; 8];
    source.read_exact(&mut signature).ok()?;
    if &signature != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    loop {
        let mut header = [0u8; 8];
        source.read_exact(&mut header).ok()?;
        let length = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
        match &header[4..8] {
            b"pHYs" if length == 9 => {
                let mut data = [0u8; 9];
                source.read_exact(&mut data).ok()?;
                let x = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                let y = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                // Pixels to the meter; a unit of 0 is an aspect ratio alone.
                return (data[8] == 1)
                    .then(|| Density::new([f64::from(x), f64::from(y)], 1000.0))
                    .flatten();
            }
            b"IDAT" | b"IEND" => return None,
            _ => {
                // The data and the CRC.
                source.seek(SeekFrom::Current(i64::from(length) + 4)).ok()?;
            }
        }
    }
}

/// The density in a JPEG's JFIF header, the `APP0` segment that opens the
/// file when it is there at all.
pub fn in_jpeg(source: &mut dyn ReadSeek) -> Option<Density> {
    source.seek(SeekFrom::Start(0)).ok()?;
    // The start of the image, the APP0 marker and its length, and as much
    // of the segment as holds the density.
    let mut head = [0u8; 18];
    source.read_exact(&mut head).ok()?;
    if head[..4] != [0xff, 0xd8, 0xff, 0xe0] || &head[6..11] != b"JFIF\0" {
        return None;
    }
    let x = u16::from_be_bytes([head[14], head[15]]);
    let y = u16::from_be_bytes([head[16], head[17]]);
    // Dots to the inch, or to the centimeter; 0 is an aspect ratio alone.
    let mm = match head[13] {
        1 => MM_PER_INCH,
        2 => 10.0,
        _ => return None,
    };
    Density::new([f64::from(x), f64::from(y)], mm)
}

/// The container's own word on the density, for a file whose EXIF says none.
pub fn in_container(source: &mut dyn ReadSeek) -> Option<Density> {
    in_png(source).or_else(|| in_jpeg(source))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn png(chunks: &[(&[u8], &[u8])]) -> Vec<u8> {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        for (kind, data) in chunks {
            png.extend_from_slice(&(data.len() as u32).to_be_bytes());
            png.extend_from_slice(kind);
            png.extend_from_slice(data);
            png.extend_from_slice(&[0; 4]);
        }
        png
    }

    fn phys(x: u32, y: u32, unit: u8) -> Vec<u8> {
        let mut data = x.to_be_bytes().to_vec();
        data.extend_from_slice(&y.to_be_bytes());
        data.push(unit);
        data
    }

    /// 23622 pixels to the meter is 600 to the inch, near enough to say so.
    #[test]
    fn a_pngs_density_is_read_in_meters() {
        let file = png(&[
            (b"IHDR", &[0; 13]),
            (b"pHYs", &phys(23622, 23622, 1)),
            (b"IDAT", &[0; 4]),
            (b"IEND", &[]),
        ]);
        let density = in_container(&mut Cursor::new(file)).unwrap();
        assert_eq!(density.words(), "600 dpi");
    }

    /// An aspect ratio alone, or a chunk after the image data where none may
    /// be, is no density.
    #[test]
    fn a_pngs_aspect_alone_is_no_density() {
        let aspect = png(&[
            (b"IHDR", &[0; 13]),
            (b"pHYs", &phys(2, 1, 0)),
            (b"IEND", &[]),
        ]);
        assert_eq!(in_png(&mut Cursor::new(aspect)), None);
        let late = png(&[
            (b"IHDR", &[0; 13]),
            (b"IDAT", &[0; 4]),
            (b"pHYs", &phys(23622, 23622, 1)),
            (b"IEND", &[]),
        ]);
        assert_eq!(in_png(&mut Cursor::new(late)), None);
    }

    fn jfif(unit: u8, x: u16, y: u16) -> Vec<u8> {
        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10];
        jpeg.extend_from_slice(b"JFIF\0\x01\x02");
        jpeg.push(unit);
        jpeg.extend_from_slice(&x.to_be_bytes());
        jpeg.extend_from_slice(&y.to_be_bytes());
        jpeg.extend_from_slice(&[0, 0, 0xff, 0xd9]);
        jpeg
    }

    #[test]
    fn a_jfif_density_is_read_in_either_unit() {
        let inches = in_container(&mut Cursor::new(jfif(1, 300, 150))).unwrap();
        assert_eq!(inches.words(), "300 \u{00d7} 150 dpi");
        let centimeters = in_container(&mut Cursor::new(jfif(2, 118, 118))).unwrap();
        assert_eq!(centimeters.words(), "299.7 dpi");
        assert_eq!(in_jpeg(&mut Cursor::new(jfif(0, 1, 1))), None);
        assert_eq!(in_jpeg(&mut Cursor::new(jfif(1, 0, 72))), None);
    }

    /// A sheet is named in its own unit, either way up, whatever the
    /// locale's; a size no sheet is, in the locale's.
    #[test]
    fn a_print_size_is_named_by_its_sheet() {
        let density = Density::new([23622.0; 2], 1000.0).unwrap();
        assert_eq!(
            density.print_size([9921, 7016], Unit::Inches),
            "420 \u{00d7} 297 mm \u{00b7} A3"
        );
        let letter = Density::new([300.0; 2], MM_PER_INCH).unwrap();
        assert_eq!(
            letter.print_size([2550, 3300], Unit::Millimeters),
            "8.5 \u{00d7} 11 in \u{00b7} Letter"
        );
        let camera = Density::new([72.0; 2], MM_PER_INCH).unwrap();
        assert_eq!(
            camera.print_size([4032, 3024], Unit::Inches),
            "56 \u{00d7} 42 in"
        );
        assert_eq!(
            camera.print_size([4032, 3024], Unit::Millimeters),
            "1422 \u{00d7} 1067 mm"
        );
    }
}
