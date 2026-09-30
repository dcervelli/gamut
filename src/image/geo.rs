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
//! What the panel says is quoted rather than looked up: a file that names
//! EPSG:2056 is quoted as naming it, along with whatever the file calls it.
//! The one place the register is consulted is [`Georeference`], which the
//! pointer's readout asks where a pixel is in latitude and longitude — a
//! question the file's own coordinates cannot answer without knowing the
//! projection they are in. `proj4rs` does the projection's arithmetic, and
//! the table of EPSG codes it carries says what a code stands for; a system
//! that table does not know, or that the file spells out in parameters
//! rather than naming, keeps its own coordinates and is not given a latitude.

use std::sync::Arc;

use proj4rs::Proj;

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
    // point at its center. Half a pixel, which is half a meter here and
    // fifteen meters in a satellite scene.
    if let Some(Value::Code(code)) = keys
        .iter()
        .find(|(id, _)| *id == RASTER_TYPE)
        .map(|(_, v)| v)
    {
        rows.push(Entry::new(
            "Pixel is",
            match code {
                1 => "area (coordinates are corners)".to_string(),
                2 => "point (coordinates are centers)".to_string(),
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
        // One row per end of each axis rather than a span: a span of two
        // seven-figure coordinates does not fit on a line of a panel this
        // wide, and a coordinate broken across two lines is a coordinate
        // misread.
        if let Some(axes) = extent(&place, tags.size) {
            let names = if geographic {
                ["longitude", "latitude"]
            } else {
                ["easting", "northing"]
            };
            for (axis, [low, high]) in names.into_iter().zip(axes) {
                rows.push(Entry::new(format!("Min {axis}"), number(low)));
                rows.push(Entry::new(format!("Max {axis}"), number(high)));
            }
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

/// Where each pixel of a raster is, for the pointer's readout: in the
/// file's own coordinates, and — where the system they are in is one the
/// register knows, or is longitude and latitude already — in latitude and
/// longitude on WGS 84.
///
/// Built once per file, alongside the panel's rows: resolving a code and
/// setting a projection up is work the pointer should not repeat on every
/// frame it moves.
#[derive(Clone, Debug)]
pub struct Georeference {
    affine: [f64; 6],
    /// How far into a pixel its coordinate is taken: to its middle, which is
    /// half a pixel in where the file's coordinates are corners and no way
    /// in where they are centers already.
    inset: f64,
    /// Whether the model's coordinates are longitude and latitude, in
    /// degrees, rather than lengths on a projection.
    geographic: bool,
    /// The unit the model's coordinates are in, where the file says.
    unit: Option<&'static str>,
    /// The code the system is filed under, which is what `lift` was built
    /// from; kept so that two references can be compared without comparing
    /// projections.
    code: Option<u16>,
    /// The way from the model's coordinates to longitude and latitude on
    /// WGS 84, where there is one to take; `None` for a geographic model
    /// read as it stands.
    lift: Option<Lift>,
}

/// A projection and its way back to the globe, shared rather than rebuilt
/// when the metadata it hangs off is cloned.
#[derive(Clone)]
struct Lift {
    from: Arc<Proj>,
    to: Arc<Proj>,
}

impl std::fmt::Debug for Lift {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Lift({} to longlat)", self.from.projname())
    }
}

/// What a reference is compared by: everything but the projection, which
/// the code it was built from stands for. Floats by their bits, which is
/// what makes equality an equivalence here — these are numbers read out of
/// a file, never worked out, so no two that mean the same differ in them.
impl PartialEq for Georeference {
    fn eq(&self, other: &Self) -> bool {
        let bits = |map: &[f64; 6]| map.map(f64::to_bits);
        bits(&self.affine) == bits(&other.affine)
            && self.inset.to_bits() == other.inset.to_bits()
            && self.geographic == other.geographic
            && self.unit == other.unit
            && self.code == other.code
    }
}

impl Eq for Georeference {}

/// The system everything is lifted to: what a map on a phone reads, and what
/// a latitude and longitude with no datum named are taken to be in.
const WGS84: &str = "+proj=longlat +datum=WGS84 +no_defs";

/// The code WGS 84 itself is filed under, which needs no lift.
const EPSG_WGS84: u16 = 4326;

impl Georeference {
    /// Where `tags` place the raster, or `None` where they do not — no
    /// tiepoint and no matrix, or a model this cannot read, which is a
    /// geocentric one or a geographic one in an angle other than degrees.
    pub fn read(tags: &Tags) -> Option<Self> {
        let affine = affine(tags)?;
        let keys = keys(tags);
        let projected = registered(&keys, PROJECTED_TYPE);
        let geographic = match code(&keys, MODEL_TYPE) {
            Some(1) => false,
            Some(2) => true,
            // Geocentric, or something the standard does not define: not
            // coordinates a pixel has a place in.
            Some(_) => return None,
            // A file that leaves the model unsaid is taken at the system it
            // names; one that names none is a projection nobody has named.
            None => projected.is_none() && registered(&keys, GEOGRAPHIC_TYPE).is_some(),
        };
        if geographic && code(&keys, ANGULAR_UNITS).is_some_and(|unit| unit != 9102) {
            return None;
        }
        let system = if geographic {
            registered(&keys, GEOGRAPHIC_TYPE)
        } else {
            projected
        };
        let inset = match code(&keys, RASTER_TYPE) {
            Some(2) => 0.0,
            _ => 0.5,
        };
        let lift = system.filter(|code| *code != EPSG_WGS84).and_then(|code| {
            let from = Proj::from_epsg_code(code).ok()?;
            let to = Proj::from_proj_string(WGS84).ok()?;
            (from.has_inverse() && from.is_latlong() == geographic).then(|| Lift {
                from: Arc::new(from),
                to: Arc::new(to),
            })
        });
        Some(Self {
            affine,
            inset,
            geographic,
            unit: unit(&keys, geographic),
            code: system,
            lift,
        })
    }

    /// Whether the file's own coordinates are lengths on a projection, and
    /// so something other than a latitude to read out.
    pub fn offers_projected(&self) -> bool {
        !self.geographic
    }

    /// Whether a pixel can be given a latitude and a longitude.
    pub fn offers_geographic(&self) -> bool {
        self.geographic || self.lift.is_some()
    }

    /// The unit the file's coordinates are in, where it says.
    pub fn unit(&self) -> Option<&'static str> {
        self.unit
    }

    /// The middle of the stored pixel `at`, in the file's own coordinates.
    pub fn model(&self, at: [u32; 2]) -> [f64; 2] {
        let [a, b, c, d, e, f] = self.affine;
        let (column, row) = (at[0] as f64 + self.inset, at[1] as f64 + self.inset);
        [a * column + b * row + c, d * column + e * row + f]
    }

    /// The middle of the stored pixel `at` as latitude and longitude on
    /// WGS 84, in degrees, in that order; `None` where the file cannot say,
    /// or where the point is off the part of the globe its projection maps.
    pub fn latitude_longitude(&self, at: [u32; 2]) -> Option<[f64; 2]> {
        let [x, y] = self.model(at);
        let [longitude, latitude] = match (&self.lift, self.geographic) {
            (None, true) => [x, y],
            (None, false) => return None,
            (Some(lift), geographic) => {
                // A geographic system goes in in radians, as it comes out.
                let mut point = if geographic {
                    (x.to_radians(), y.to_radians(), 0.0)
                } else {
                    (x, y, 0.0)
                };
                proj4rs::transform::transform(&lift.from, &lift.to, &mut point).ok()?;
                [point.0.to_degrees(), point.1.to_degrees()]
            }
        };
        (longitude.is_finite() && latitude.is_finite() && latitude.abs() <= 90.0)
            .then_some([latitude, longitude])
    }

    /// How far one pixel reaches along the shorter of its two sides, in the
    /// file's own units: what decides how many places a coordinate in them
    /// is worth.
    pub fn step(&self) -> f64 {
        let [a, b, _, d, e, _] = self.affine;
        a.hypot(d).min(b.hypot(e))
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

/// A tiepoint and a scale, or the matrix that says the same thing, as the
/// corner and the size of a pixel the panel quotes.
fn placement(tags: &Tags) -> Option<Placement> {
    let [a, b, c, d, e, f] = affine(tags)?;
    Some(Placement {
        origin: [c, f],
        scale: [a, -e],
        rotated: b != 0.0 || d != 0.0,
    })
}

/// Where the raster sits in the model's coordinates, as the affine map it
/// is: model x is `a·column + b·row + c`, model y is `d·column + e·row + f`,
/// for `[a, b, c, d, e, f]`.
///
/// The tiepoint ties one raster point to one model point, and is almost
/// always the raster's own corner; where it is not, it is walked back along
/// the axes. The vertical one runs the other way in each — down the raster,
/// up the ground — which is the sign below and the only subtlety here. The
/// matrix form says the same thing with room for a rotation.
fn affine(tags: &Tags) -> Option<[f64; 6]> {
    let map = if let (Some(scale), Some(tie)) = (tags.scale.get(..2), tags.tiepoint.get(..6)) {
        [
            scale[0],
            0.0,
            tie[3] - tie[0] * scale[0],
            0.0,
            -scale[1],
            tie[4] + tie[1] * scale[1],
        ]
    } else {
        let matrix = tags.transform.get(..8)?;
        [
            matrix[0], matrix[1], matrix[3], matrix[4], matrix[5], matrix[7],
        ]
    };
    if map[0] == 0.0 && map[4] == 0.0 {
        return None;
    }
    map.iter().all(|value| value.is_finite()).then_some(map)
}

/// The ground the whole raster covers, which is its corner and its size in
/// pixels multiplied out. Only for a raster whose rows run east and whose
/// columns run south, since anything else is not a rectangle in these
/// coordinates and quoting one would be inventing corners.
///
/// Each axis as its least and its greatest.
fn extent(place: &Placement, size: Option<[u32; 2]>) -> Option<[[f64; 2]; 2]> {
    let size = size?;
    if place.rotated {
        return None;
    }
    let far = [
        place.origin[0] + size[0] as f64 * place.scale[0],
        place.origin[1] - size[1] as f64 * place.scale[1],
    ];
    let span = |a: f64, b: f64| [a.min(b), a.max(b)];
    Some([span(place.origin[0], far[0]), span(place.origin[1], far[1])])
}

/// A coordinate as a reader wants it. Rust writes a float as the shortest
/// text that reads back as the same number, which is exactly right for a
/// round number of meters and far too much for a degree that came out of an
/// arithmetic; the panel's own rounding settles the second case.
fn number(value: f64) -> String {
    super::exif::tidy(&format!("{value}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Swiss national map series: a projected system named in the ASCII
    /// pool, a 2.5 m pixel, and a corner at the top left.
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
                ("Min easting", "2655000"),
                ("Max easting", "2690000"),
                ("Min northing", "1086000"),
                ("Max northing", "1110000"),
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
            !turned.iter().any(|(name, _)| name.ends_with("easting")),
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
                "point (coordinates are centers)".to_string()
            )]
        );
    }

    /// A raster one pixel of which is `pixel` across, in the model `model`
    /// (1 projected, 2 geographic) under the system `system`, with the
    /// middle of its first pixel at `middle`.
    fn placed(model: u16, system: u16, middle: [f64; 2], pixel: f64) -> Tags {
        let key = if model == 2 {
            GEOGRAPHIC_TYPE
        } else {
            PROJECTED_TYPE
        };
        Tags {
            directory: vec![1, 1, 0, 2, MODEL_TYPE, 0, 1, model, key, 0, 1, system],
            scale: vec![pixel, pixel, 0.0],
            tiepoint: vec![
                0.0,
                0.0,
                0.0,
                middle[0] - pixel / 2.0,
                middle[1] + pixel / 2.0,
                0.0,
            ],
            size: Some([100, 100]),
            ..Tags::default()
        }
    }

    fn near(have: [f64; 2], want: [f64; 2], within: f64) {
        assert!(
            (have[0] - want[0]).abs() < within && (have[1] - want[1]).abs() < within,
            "{have:?} is not within {within} of {want:?}"
        );
    }

    /// A pixel's coordinate is its middle, in the file's own system, and its
    /// latitude is what `cs2cs` makes of that middle: here the old
    /// observatory in Bern, which the Swiss grid is measured from; the
    /// corner of Manhattan on UTM; and the same corner again in US survey
    /// feet on the state plane.
    #[test]
    fn a_projected_pixel_is_found_on_the_globe() {
        let bern =
            Georeference::read(&placed(1, 2056, [2_600_000.0, 1_200_000.0], 2.5)).expect("placed");
        assert_eq!(bern.model([0, 0]), [2_600_000.0, 1_200_000.0]);
        assert_eq!(bern.model([1, 2]), [2_600_002.5, 1_199_995.0]);
        assert!(bern.offers_projected() && bern.offers_geographic());
        near(
            bern.latitude_longitude([0, 0]).expect("on the globe"),
            [46.951_082_77, 7.438_632_42],
            2e-5,
        );

        let utm =
            Georeference::read(&placed(1, 32618, [583_000.0, 4_507_000.0], 1.0)).expect("placed");
        near(
            utm.latitude_longitude([0, 0]).expect("on the globe"),
            [40.709_735_67, -74.017_402_95],
            1e-6,
        );

        let feet =
            Georeference::read(&placed(1, 2263, [988_000.0, 200_000.0], 1.0)).expect("placed");
        near(
            feet.latitude_longitude([0, 0]).expect("on the globe"),
            [40.715_629_70, -73.986_472_63],
            1e-5,
        );
    }

    /// A raster in longitude and latitude on WGS 84 is read as it stands,
    /// and offers nothing that is not a latitude: its own coordinates are
    /// one.
    #[test]
    fn a_geographic_pixel_is_its_own_latitude() {
        let geo = Georeference::read(&placed(2, 4326, [5.9505, 47.8085], 0.001)).expect("placed");
        assert!(!geo.offers_projected() && geo.offers_geographic());
        near(
            geo.latitude_longitude([0, 0]).expect("a latitude"),
            [47.8085, 5.9505],
            1e-12,
        );
        near(
            geo.latitude_longitude([10, 20]).expect("a latitude"),
            [47.7885, 5.9605],
            1e-12,
        );
    }

    /// A system spelled out in parameters, or named by a code the table
    /// does not hold, keeps its own coordinates and is not given a latitude.
    #[test]
    fn a_system_nothing_can_look_up_is_left_projected() {
        for system in [USER_DEFINED, 1] {
            let geo =
                Georeference::read(&placed(1, system, [1000.0, 2000.0], 1.0)).expect("placed");
            assert!(geo.offers_projected(), "{system}");
            assert!(!geo.offers_geographic(), "{system}");
            assert_eq!(geo.latitude_longitude([0, 0]), None);
        }
        // And a raster with nothing to place it by is not a reference at all.
        assert_eq!(Georeference::read(&Tags::default()), None);
    }

    /// Where a file's coordinates are the centers of its pixels, the middle
    /// of the first pixel is the tiepoint itself; and a raster given as a
    /// turned matrix is read through the whole of it.
    #[test]
    fn a_pixel_is_read_where_the_file_puts_it() {
        let mut tags = swiss();
        let corner = Georeference::read(&tags).expect("placed");
        assert_eq!(corner.model([0, 0]), [2_655_001.25, 1_109_998.75]);
        // The raster type's value: pixel is point.
        tags.directory[4 + 4 + 3] = 2;
        let center = Georeference::read(&tags).expect("placed");
        assert_eq!(center.model([0, 0]), [2_655_000.0, 1_110_000.0]);

        let turned = Tags {
            scale: Vec::new(),
            tiepoint: Vec::new(),
            transform: vec![
                0.0, 2.0, 0.0, 100.0, //
                -2.0, 0.0, 0.0, 500.0, //
                0.0, 0.0, 0.0, 0.0, //
                0.0, 0.0, 0.0, 1.0,
            ],
            ..swiss()
        };
        // All rotation and no scale on the diagonal: not a raster this
        // places, as the panel does not quote one either.
        assert_eq!(Georeference::read(&turned), None);
        let sheared = Tags {
            transform: vec![
                2.0, 1.0, 0.0, 100.0, //
                0.5, -2.0, 0.0, 500.0, //
                0.0, 0.0, 0.0, 0.0, //
                0.0, 0.0, 0.0, 1.0,
            ],
            ..turned
        };
        let geo = Georeference::read(&sheared).expect("placed");
        assert_eq!(
            geo.model([2, 4]),
            [100.0 + 2.5 * 2.0 + 4.5, 500.0 + 2.5 * 0.5 - 4.5 * 2.0]
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
