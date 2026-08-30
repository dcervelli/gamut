//! Where a raster's pixels are on the ground.
//!
//! GeoTIFF is not EXIF. It is a second standard sharing the same directory,
//! and rather than taking tags of its own for each thing it has to say, it
//! packs a directory of *keys* into one tag and points them at two others for
//! the values that will not fit in a short. What comes out — the coordinate
//! system, the size of a pixel, where the raster's corner sits — is the first
//! thing anyone opening a scanned map or an elevation model looks for, and it
//! is nowhere else in the file.
//!
//! Nothing here consults a coordinate-system database. A file that names
//! EPSG:2056 is quoted as naming it, along with whatever the file calls it;
//! turning the code into a datum and a projection means shipping the register
//! that defines them, which is a different program's job.

use super::exif::Entry;

/// The tags a georeference is built from, as the file holds them. Pulled out
/// of the directory by [`super::exif`], which is where the parser's types
/// live; everything below is arithmetic on these numbers.
#[derive(Clone, Default, Debug)]
pub struct Tags {
    /// 34735, the key directory: a header of four shorts, then four shorts
    /// per key.
    pub directory: Vec<u16>,
    /// 34737, which the directory's keys point into for their names. The
    /// other pool a key can point at, 34736, holds the parameters of a
    /// projection spelled out rather than named, and nothing here reads one:
    /// its tag stays in the listing with its numbers on show.
    pub ascii: Vec<u8>,
    /// 33550, the size of a pixel on the ground, and 33922, one raster point
    /// tied to one model point.
    pub scale: Vec<f64>,
    pub tiepoint: Vec<f64>,
    /// 34264, which says the same thing as a matrix and is what a raster that
    /// is rotated or sheared has instead.
    pub transform: Vec<f64>,
    /// The raster itself, for working out where its far corner lands.
    pub size: Option<[u32; 2]>,
    /// 42113: the value that means "nothing was measured here", which is a
    /// fact about the pixels rather than about the ground, and is here
    /// because it comes from the same writer and matters for the same work.
    pub nodata: Option<String>,
}

/// The keys this reads. The rest of a directory is the projection's own
/// parameters, which say nothing a reader wants that the coordinate system's
/// name does not say better.
const MODEL_TYPE: u16 = 1024;
const RASTER_TYPE: u16 = 1025;
const CITATION: u16 = 1026;
const GEOGRAPHIC_TYPE: u16 = 2048;
const GEOGRAPHIC_CITATION: u16 = 2049;
const ANGULAR_UNITS: u16 = 2054;
const PROJECTED_TYPE: u16 = 3072;
const PROJECTED_CITATION: u16 = 3073;
const LINEAR_UNITS: u16 = 3076;
const VERTICAL_TYPE: u16 = 4096;
const VERTICAL_CITATION: u16 = 4098;

/// Where a key's value lives: `0` means the key holds it, and otherwise the
/// number is the tag it is stored in.
const IN_ASCII: u16 = 34737;

/// The code a key carries when the file is not using the register at all, but
/// spelling the system out in the projection's own parameters.
const USER_DEFINED: u16 = 32767;

/// A key's value: a code where the key holds it, a name where it points into
/// the pool of them.
enum Value {
    Code(u16),
    Text(String),
}

/// What the file says about where its pixels are, as rows for the panel.
/// Empty for an image that says nothing, which is every image that is a
/// picture rather than a measurement.
pub fn describe(tags: &Tags) -> Vec<Entry> {
    let keys = keys(tags);
    let mut rows = Vec::new();

    if let Some(system) = coordinate_system(&keys) {
        rows.push(Entry::new("Coordinate system", system));
    }
    if let Some(vertical) = vertical(&keys) {
        rows.push(Entry::new("Vertical system", vertical));
    }
    // What a coordinate is a coordinate *of*: the corner of a cell, or the
    // point at its centre. Half a pixel, which is half a metre here and
    // fifteen metres in a satellite scene.
    if let Some(Value::Code(code)) = keys
        .iter()
        .find(|(id, _)| *id == RASTER_TYPE)
        .map(|(_, v)| v)
    {
        rows.push(Entry::new(
            "Pixel is",
            match code {
                1 => "area (coordinates are corners)".to_string(),
                2 => "point (coordinates are centres)".to_string(),
                other => format!("raster type {other}"),
            },
        ));
    }

    // Which way round the coordinates read: what the axes are called, and
    // which units they are given in.
    let geographic = code(&keys, MODEL_TYPE) == Some(2);
    let unit = unit(&keys, geographic);
    if let Some(place) = placement(tags) {
        rows.push(Entry::new(
            "Pixel size",
            with_unit(
                &format!(
                    "{} \u{00d7} {}",
                    number(place.scale[0]),
                    number(place.scale[1])
                ),
                unit,
            ),
        ));
        rows.push(Entry::new(
            "Origin",
            format!("{}, {}", number(place.origin[0]), number(place.origin[1])),
        ));
        // One row per axis rather than one row of both: two spans of
        // seven-figure coordinates do not fit on a line of a panel this wide,
        // and a coordinate broken across two lines is a coordinate misread.
        if let Some([across, down]) = extent(&place, tags.size) {
            let (x, y) = if geographic {
                ("Longitude", "Latitude")
            } else {
                ("Easting", "Northing")
            };
            rows.push(Entry::new(x, across));
            rows.push(Entry::new(y, down));
        }
        if place.rotated {
            rows.push(Entry::new(
                "Orientation",
                "rotated: the raster's axes are not the model's".to_string(),
            ));
        }
    }
    if let Some(nodata) = &tags.nodata {
        rows.push(Entry::new("No data", nodata.clone()));
    }
    rows
}

/// The directory, resolved: each key with whatever it was pointed at.
fn keys(tags: &Tags) -> Vec<(u16, Value)> {
    // Four shorts of header, the last of which is how many keys follow. A
    // directory that claims more keys than it holds is read as far as it goes.
    let Some(&count) = tags.directory.get(3) else {
        return Vec::new();
    };
    tags.directory
        .get(4..)
        .unwrap_or_default()
        .as_chunks::<4>()
        .0
        .iter()
        .take(count as usize)
        .filter_map(|entry| {
            let (key, location, count, offset) = (entry[0], entry[1], entry[2], entry[3]);
            let value = match location {
                0 => Some(Value::Code(offset)),
                IN_ASCII => text(&tags.ascii, offset as usize, count as usize).map(Value::Text),
                // A key stored in the pool of doubles: a parameter of a
                // projection this does not name, left where it is.
                _ => None,
            };
            value.map(|value| (key, value))
        })
        .collect()
}

/// `count` characters of the ASCII pool from `offset`.
///
/// The pool is one string with its parts run together, and a part ends in the
/// pipe that stands in for the null a C program would find there.
fn text(ascii: &[u8], offset: usize, count: usize) -> Option<String> {
    let end = offset.checked_add(count)?.min(ascii.len());
    let bytes = ascii.get(offset..end)?;
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim_end_matches(['|', '\0']).trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn code(keys: &[(u16, Value)], id: u16) -> Option<u16> {
    keys.iter().find_map(|(key, value)| match value {
        Value::Code(code) if *key == id => Some(*code),
        _ => None,
    })
}

fn name(keys: &[(u16, Value)], id: u16) -> Option<&str> {
    keys.iter().find_map(|(key, value)| match value {
        Value::Text(text) if *key == id => Some(text.as_str()),
        _ => None,
    })
}

/// What the file calls its coordinate system, and the code it files it under.
/// Either alone is worth saying; a file that gives neither is not saying
/// where its pixels are, whatever else its directory holds.
fn coordinate_system(keys: &[(u16, Value)]) -> Option<String> {
    let named = name(keys, CITATION)
        .or_else(|| name(keys, PROJECTED_CITATION))
        .or_else(|| name(keys, GEOGRAPHIC_CITATION));
    let registered = registered(keys, PROJECTED_TYPE).or_else(|| registered(keys, GEOGRAPHIC_TYPE));
    match (named, registered) {
        // A citation that already quotes the code does not want it twice.
        (Some(named), Some(code)) if named.contains(&code.to_string()) => Some(named.to_string()),
        (Some(named), Some(code)) => Some(format!("{named} (EPSG:{code})")),
        (Some(named), None) => Some(named.to_string()),
        (None, Some(code)) => Some(format!("EPSG:{code}")),
        (None, None) => None,
    }
}

fn vertical(keys: &[(u16, Value)]) -> Option<String> {
    let named = name(keys, VERTICAL_CITATION);
    match (named, registered(keys, VERTICAL_TYPE)) {
        (Some(named), Some(code)) if !named.contains(&code.to_string()) => {
            Some(format!("{named} (EPSG:{code})"))
        }
        (Some(named), _) => Some(named.to_string()),
        (None, Some(code)) => Some(format!("EPSG:{code}")),
        (None, None) => None,
    }
}

/// A key's code, where it is one the register defines. Zero is a key that was
/// written and left unset; the user-defined code means the file is spelling
/// the system out in parameters instead, and quoting 32767 as if it were a
/// system would be quoting the word "other".
fn registered(keys: &[(u16, Value)], id: u16) -> Option<u16> {
    code(keys, id).filter(|code| *code != 0 && *code != USER_DEFINED)
}

/// The unit the coordinates are in, for the rows that are lengths.
///
/// Only where the file says: the register defines a unit for every system it
/// names, so a file that leaves the key out is not being unclear, and a
/// number with the wrong unit on it is worse than one with none.
fn unit(keys: &[(u16, Value)], geographic: bool) -> Option<&'static str> {
    let code = code(
        keys,
        if geographic {
            ANGULAR_UNITS
        } else {
            LINEAR_UNITS
        },
    )?;
    match code {
        9001 => Some("m"),
        9002 => Some("ft"),
        9003 => Some("us-ft"),
        9036 => Some("km"),
        9101 => Some("rad"),
        9102 => Some("\u{00b0}"),
        9103 => Some("\u{2032}"),
        9104 => Some("\u{2033}"),
        9105 => Some("grad"),
        _ => None,
    }
}

fn with_unit(value: &str, unit: Option<&str>) -> String {
    match unit {
        Some(unit) => format!("{value} {unit}"),
        None => value.to_string(),
    }
}

/// Where the raster sits in the model's coordinates: the ground under its
/// first corner, and how much ground a pixel covers.
struct Placement {
    origin: [f64; 2],
    scale: [f64; 2],
    /// Whether the raster's rows run along the model's axes. When they do not
    /// there is no rectangle to quote as an extent.
    rotated: bool,
}

/// A tiepoint and a scale, or the matrix that says the same thing.
///
/// The tiepoint ties one raster point to one model point, and is almost
/// always the raster's own corner; where it is not, it is walked back along
/// the axes. The vertical one runs the other way in each — down the raster,
/// up the ground — which is the sign below and the only subtlety here.
fn placement(tags: &Tags) -> Option<Placement> {
    if let (Some(scale), Some(tie)) = (tags.scale.get(..2), tags.tiepoint.get(..6)) {
        if scale[0] == 0.0 && scale[1] == 0.0 {
            return None;
        }
        return Some(Placement {
            origin: [tie[3] - tie[0] * scale[0], tie[4] + tie[1] * scale[1]],
            scale: [scale[0], scale[1]],
            rotated: false,
        });
    }
    // The matrix form: model x and y are each an affine function of the
    // raster's column and row, so the translation is the origin, the leading
    // diagonal is the scale, and the off-diagonal terms are the rotation.
    let matrix = tags.transform.get(..8)?;
    let (rotation, scale) = ([matrix[1], matrix[4]], [matrix[0], -matrix[5]]);
    if scale[0] == 0.0 && scale[1] == 0.0 {
        return None;
    }
    Some(Placement {
        origin: [matrix[3], matrix[7]],
        scale,
        rotated: rotation[0] != 0.0 || rotation[1] != 0.0,
    })
}

/// The ground the whole raster covers, which is its corner and its size in
/// pixels multiplied out. Only for a raster whose rows run east and whose
/// columns run south, since anything else is not a rectangle in these
/// coordinates and quoting one would be inventing corners.
fn extent(place: &Placement, size: Option<[u32; 2]>) -> Option<[String; 2]> {
    let size = size?;
    if place.rotated {
        return None;
    }
    let far = [
        place.origin[0] + size[0] as f64 * place.scale[0],
        place.origin[1] - size[1] as f64 * place.scale[1],
    ];
    let span = |a: f64, b: f64| {
        let (low, high) = if a <= b { (a, b) } else { (b, a) };
        format!("{}\u{2013}{}", number(low), number(high))
    };
    Some([span(place.origin[0], far[0]), span(place.origin[1], far[1])])
}

/// A coordinate as a reader wants it. Rust writes a float as the shortest
/// text that reads back as the same number, which is exactly right for a
/// round number of metres and far too much for a degree that came out of an
/// arithmetic; the panel's own rounding settles the second case.
fn number(value: f64) -> String {
    super::exif::tidy(&format!("{value}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Swiss national map series, as the file that prompted all this
    /// holds it: a projected system named in the ASCII pool, a 2.5 m pixel,
    /// and a corner at the top left.
    fn swiss() -> Tags {
        Tags {
            // Version 1.1.0, seven keys.
            directory: vec![
                1,
                1,
                0,
                7, //
                MODEL_TYPE,
                0,
                1,
                1, // projected
                RASTER_TYPE,
                0,
                1,
                1, // pixel is area
                CITATION,
                IN_ASCII,
                15,
                0, // "CH1903+ / LV95|"
                GEOGRAPHIC_CITATION,
                IN_ASCII,
                8,
                15, // "CH1903+|"
                ANGULAR_UNITS,
                0,
                1,
                9102, //
                PROJECTED_TYPE,
                0,
                1,
                2056, //
                LINEAR_UNITS,
                0,
                1,
                9001,
            ],
            ascii: b"CH1903+ / LV95|CH1903+|".to_vec(),
            scale: vec![2.5, 2.5, 0.0],
            tiepoint: vec![0.0, 0.0, 0.0, 2_655_000.0, 1_110_000.0, 0.0],
            transform: Vec::new(),
            size: Some([14000, 9600]),
            nodata: None,
        }
    }

    fn rows(tags: &Tags) -> Vec<(String, String)> {
        describe(tags)
            .into_iter()
            .map(|entry| (entry.name, entry.value))
            .collect()
    }

    /// Every corner here is one `gdalinfo` prints for the same file.
    #[test]
    fn a_projected_raster_is_placed_on_the_ground() {
        assert_eq!(
            rows(&swiss()),
            [
                ("Coordinate system", "CH1903+ / LV95 (EPSG:2056)"),
                ("Pixel is", "area (coordinates are corners)"),
                ("Pixel size", "2.5 \u{00d7} 2.5 m"),
                ("Origin", "2655000, 1110000"),
                ("Easting", "2655000\u{2013}2690000"),
                ("Northing", "1086000\u{2013}1110000"),
            ]
            .map(|(name, value)| (name.to_string(), value.to_string()))
        );
    }

    /// The tiepoint need not be the raster's own corner; where it is not, the
    /// corner is where it would have been.
    #[test]
    fn a_tiepoint_away_from_the_corner_is_walked_back_to_it() {
        let mut tags = swiss();
        // The same ground, tied through the pixel at (100, 40) instead.
        tags.tiepoint = vec![100.0, 40.0, 0.0, 2_655_250.0, 1_109_900.0, 0.0];
        let placed = placement(&tags).expect("a tiepoint and a scale");
        assert_eq!(placed.origin, [2_655_000.0, 1_110_000.0]);
    }

    /// A raster given as a matrix says the same things, and one whose axes
    /// are turned off the model's is not given an extent it does not have.
    #[test]
    fn the_matrix_form_is_read_and_a_turned_raster_keeps_its_corner_only() {
        let mut tags = swiss();
        tags.scale.clear();
        tags.tiepoint.clear();
        tags.transform = vec![
            2.5,
            0.0,
            0.0,
            2_655_000.0, //
            0.0,
            -2.5,
            0.0,
            1_110_000.0, //
            0.0,
            0.0,
            0.0,
            0.0, //
            0.0,
            0.0,
            0.0,
            1.0,
        ];
        assert_eq!(rows(&tags), rows(&swiss()));

        // Turned: the corner and the pixel are still true, the rectangle is
        // not, and the panel says so rather than quoting one.
        tags.transform[1] = 0.5;
        let turned = rows(&tags);
        assert!(
            !turned.iter().any(|(name, _)| name == "Easting"),
            "{turned:?}"
        );
        assert!(turned.iter().any(|(name, _)| name == "Orientation"));
        assert!(
            turned
                .iter()
                .any(|(name, value)| name == "Origin" && value == "2655000, 1110000")
        );
    }

    /// What is quoted is what the file says. A system it names only by code
    /// is given as the code; one it spells out in parameters is not given a
    /// code at all, since the code for that means "not one of these".
    #[test]
    fn a_system_is_quoted_as_the_file_gives_it() {
        let mut tags = swiss();
        tags.ascii.clear();
        tags.directory = vec![1, 1, 0, 1, PROJECTED_TYPE, 0, 1, 32631];
        assert_eq!(rows(&tags)[0].1, "EPSG:32631");

        tags.directory = vec![1, 1, 0, 1, PROJECTED_TYPE, 0, 1, USER_DEFINED];
        assert!(
            !rows(&tags)
                .iter()
                .any(|(name, _)| name == "Coordinate system"),
            "a user-defined system is not a system to name"
        );

        // And a file with no directory at all still has a corner, if it holds
        // the tags that give it one.
        tags.directory.clear();
        let rows = rows(&tags);
        assert_eq!(rows.first().map(|row| row.0.as_str()), Some("Pixel size"));
        // With no key to say what the numbers are in, they are given bare.
        assert_eq!(rows[0].1, "2.5 \u{00d7} 2.5");
    }

    /// A picture carries none of this, and is not given an empty section.
    #[test]
    fn an_image_that_says_nothing_produces_nothing() {
        assert!(describe(&Tags::default()).is_empty());
        // A directory whose header promises keys that are not there is read
        // as far as it actually goes rather than trusted or refused.
        let claimed = Tags {
            directory: vec![1, 1, 0, 9, RASTER_TYPE, 0, 1, 2],
            ..Tags::default()
        };
        assert_eq!(
            rows(&claimed),
            [(
                "Pixel is".to_string(),
                "point (coordinates are centres)".to_string()
            )]
        );
    }

    /// Degrees, where the file is in them: the unit follows the key, and a
    /// coordinate keeps enough figures to be somewhere.
    #[test]
    fn a_geographic_raster_is_given_in_its_own_units() {
        let tags = Tags {
            directory: vec![
                1,
                1,
                0,
                3, //
                MODEL_TYPE,
                0,
                1,
                2, // geographic
                GEOGRAPHIC_TYPE,
                0,
                1,
                4326, //
                ANGULAR_UNITS,
                0,
                1,
                9102,
            ],
            scale: vec![0.000833333333333, 0.000833333333333, 0.0],
            tiepoint: vec![0.0, 0.0, 0.0, 5.9505, 47.8085, 0.0],
            size: Some([1200, 1200]),
            ..Tags::default()
        };
        let rows = rows(&tags);
        assert_eq!(rows[0], ("Coordinate system".into(), "EPSG:4326".into()));
        assert!(
            rows.iter()
                .any(|(name, value)| name == "Pixel size" && value.ends_with('\u{00b0}')),
            "{rows:?}"
        );
        assert!(
            rows.iter()
                .any(|(name, value)| name == "Origin" && value == "5.9505, 47.8085"),
            "{rows:?}"
        );
    }
}
