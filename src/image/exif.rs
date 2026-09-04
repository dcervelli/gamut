//! What a file says about the photograph in it.
//!
//! The EXIF block is metadata, not pixels: nothing here reaches the decoders
//! or the image, and nothing that is read here changes what is drawn. It is
//! read on the loader thread beside the decode, because it is one more parse
//! of a file whoever wrote it chose the bytes of.
//!
//! What comes back is [`Section`]s, in the order the panel reads them. A
//! photograph is looked at through a handful of fields — what took it, when,
//! at what exposure — and those are gathered, combined and given their units
//! under `Camera`, with the GPS directory becoming `Location`. A raster is
//! looked at through a different handful, which are not EXIF at all but
//! GeoTIFF keys packed into the same directory, and [`super::geo`] takes
//! those apart into `Georeference`. The fields somebody wrote in words are
//! pulled out as `Description`, and whatever is left is listed under the
//! directory it came out of, in the order the file carries it — because this
//! is a viewer for looking at what is actually in a file rather than for a
//! tidy précis of it.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use exif::{Context, In, Rational, Tag, Value};

use super::{directory, geo};

/// How much of a TIFF is read to find its metadata.
///
/// A TIFF *is* its own metadata block: there is no chunk to seek to, and the
/// parser reads the whole of whatever it is handed. So it is handed a prefix
/// rather than the file — a scanned map or an elevation model must not be
/// pulled into memory whole for a date.
///
/// A prefix is enough because of where a directory goes. The 443 MB map this
/// was measured against keeps its first directory at byte 8 with every value
/// inside the first 10 kB, which is what any writer that means the file to be
/// read out of order does; this leaves room for that, its sub-directories, a
/// maker note and a thumbnail besides. A file that keeps its directory past
/// the end of the prefix reads as one with no metadata, which is the same
/// answer as before and now a rare one rather than a certain one.
const TIFF_PREFIX: u64 = 8 << 20;

/// The two byte orders a TIFF announces itself in, which is how the file that
/// takes the prefix is told apart from the ones that do not.
const TIFF_SIGNATURES: [[u8; 4]; 2] = [[0x4d, 0x4d, 0x00, 0x2a], [0x49, 0x49, 0x2a, 0x00]];

/// The same two for BigTIFF, whose version number is 43 rather than 42. This
/// reader is an EXIF reader and EXIF is defined on the original format, so a
/// file that says this goes through [`directory`] and comes back as a block
/// the reader knows.
const BIGTIFF_SIGNATURES: [[u8; 4]; 2] = [[0x4d, 0x4d, 0x00, 0x2b], [0x49, 0x49, 0x2b, 0x00]];

/// How many components a field may have before it is left out of the listing.
/// A TIFF's strip offsets run to thousands of numbers, which is a fact about
/// how the file is laid out rather than one about the photograph.
pub(super) const MAX_COMPONENTS: usize = 32;

/// How long a rendered value may be before it is cut short. Long enough for a
/// lens name or a comment, short enough that one field cannot become the
/// whole panel.
const MAX_VALUE_CHARS: usize = 160;

/// How many digits a number keeps when it is written out. A rational such as
/// 89/50 is exactly 1.78, and a file that stores an aperture that way should
/// not be quoted back as f/1.7799999713880652.
const SIGNIFICANT_DIGITS: usize = 6;

/// One field, named and rendered for reading.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    pub name: String,
    pub value: String,
}

impl Entry {
    pub(super) fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

/// One group of fields under the heading it is read by. Never empty: a
/// heading with a blank under it is a question about where the rest of it
/// went, so a group that came to nothing is not carried at all.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Section {
    pub name: &'static str,
    pub entries: Vec<Entry>,
}

/// A file's EXIF, ready to be read: no tags, no types, no offsets, only what
/// the fields say. Empty when the file carries none, or carries one that will
/// not parse — a photograph with unreadable metadata is still a photograph,
/// so nothing here is an error anything else has to handle.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Exif {
    /// The groups the file's fields fall into, in the order they are read:
    /// what took the picture, where it was taken, where its pixels are on the
    /// ground, what was written about it, then everything left over.
    pub sections: Vec<Section>,
}

impl Exif {
    /// Reads `path`'s metadata, or gives back nothing at all.
    pub fn read(path: &Path) -> Self {
        Self::parse(path, TIFF_PREFIX).unwrap_or_default()
    }

    /// `prefix` is how much of a TIFF to read; see [`TIFF_PREFIX`].
    fn parse(path: &Path, prefix: u64) -> Option<Self> {
        let file = File::open(path).ok()?;
        let mut source = BufReader::new(file);

        // `read_exact` rather than one read: a short read on a file long
        // enough to be a TIFF would send it down the path that reads the
        // whole of it, which is the one thing here that must not happen.
        let mut signature = [0u8; 4];
        let read = source.read_exact(&mut signature).is_ok();
        let tiff = read && TIFF_SIGNATURES.contains(&signature);
        let bigtiff = read && BIGTIFF_SIGNATURES.contains(&signature);
        source.seek(SeekFrom::Start(0)).ok()?;

        let mut reader = exif::Reader::new();
        // A prefix is a file with its end cut off, so offsets running past it
        // are expected rather than exceptional — and in a whole file that
        // will not parse, the fields that did parse are still the file's.
        // Either way, what could be read is worth showing.
        reader.continue_on_error(true);

        let block = if bigtiff {
            // Not a form this reader knows: what it is handed is the same
            // directory written back out as the form it does.
            reader.read_raw(directory::block(path)?)
        } else if tiff {
            // The file is the block, so as much of it as the prefix allows is
            // read and parsed as one, rather than handed back to a container
            // scan that would read the rest of it looking for a chunk.
            let mut held = Vec::new();
            source.by_ref().take(prefix).read_to_end(&mut held).ok()?;
            reader.read_raw(held)
        } else {
            reader.read_from_container(&mut source)
        };
        let block = block
            .or_else(|error| error.distill_partial_result(|_| ()))
            .ok()
            // A TIFF whose directory is past the end of the prefix — which is
            // what a file written straight through, with its directory after
            // its pixels, has — is read the long way round instead. The
            // decoder seeks to it wherever it is.
            .filter(|block| block.fields().len() > 0)
            .or_else(|| {
                tiff.then(|| reader.read_raw(directory::block(path)?).ok())
                    .flatten()
            })?;
        Some(Self::from_block(&block))
    }

    fn from_block(exif: &exif::Exif) -> Self {
        let geo = geo::describe(&geo_tags(exif));
        // Whatever a group below has already said is not said again: the
        // listing is what is left in the file, not a second copy of the top
        // of the panel. The georeference speaks for its tags only when it
        // came to something — a directory nothing could be read out of is
        // better listed raw than dropped.
        let mut told: Vec<Tag> = SUMMARISED.to_vec();
        told.extend(DESCRIBED.map(|(tag, _)| tag));
        if !geo.is_empty() {
            told.extend(GEOREFERENCED.map(|number| Tag(Context::Tiff, number)));
        }

        // What is left, under the directory it came out of. The block is
        // already sorted that way — TIFF's own tags describe the file, the
        // Exif directory describes the shot, the GPS directory describes the
        // place — so the listing is grouped by asking each tag where it came
        // from rather than by a table saying where each one belongs. The
        // place carries on from the coordinate the summary drew out of it.
        let mut place = location(exif);
        let mut image = Vec::new();
        let mut capture = Vec::new();
        for field in exif.fields() {
            if field.ifd_num != In::PRIMARY || told.contains(&field.tag) || is_bulk(&field.value) {
                continue;
            }
            let entry = Entry::new(
                tag_name(field.tag),
                shorten(&tidy_numbers(&display(exif, field))),
            );
            // A field whose value is nothing but padding has nothing to say.
            if entry.value.is_empty() {
                continue;
            }
            match field.tag.0 {
                Context::Gps => place.push(entry),
                Context::Tiff => image.push(entry),
                // The interoperability directory is a corner of the Exif one
                // and reads as more of the same.
                _ => capture.push(entry),
            }
        }

        let sections = [
            ("Camera", camera(exif)),
            ("Location", place),
            ("Georeference", geo),
            ("Description", described(exif)),
            ("Image metadata", image),
            ("Capture metadata", capture),
        ]
        .into_iter()
        .filter(|(_, entries)| !entries.is_empty())
        .map(|(name, entries)| Section { name, entries })
        .collect();
        Self { sections }
    }
}

/// The tags `Camera` and `Location` speak for, and so the ones the listing
/// leaves out.
const SUMMARISED: [Tag; 17] = [
    Tag::Make,
    Tag::Model,
    Tag::LensModel,
    Tag::DateTimeOriginal,
    Tag::OffsetTimeOriginal,
    Tag::ExposureTime,
    Tag::FNumber,
    Tag::PhotographicSensitivity,
    Tag::ExposureBiasValue,
    Tag::FocalLength,
    Tag::FocalLengthIn35mmFilm,
    Tag::GPSLatitude,
    Tag::GPSLatitudeRef,
    Tag::GPSLongitude,
    Tag::GPSLongitudeRef,
    Tag::GPSAltitude,
    Tag::GPSAltitudeRef,
];

/// The fields somebody wrote in words, or that the program writing the file
/// wrote on their behalf: what the picture is of, who made it, what may be
/// done with it. They are what a reader looking for sentences rather than
/// numbers is looking for, and the listing below is long enough to lose them
/// in — so they are pulled out of it and named as they would be spoken.
const DESCRIBED: [(Tag, &str); 6] = [
    (Tag::ImageDescription, "Description"),
    (Tag::UserComment, "Comment"),
    (Tag::Artist, "Artist"),
    (Tag::Copyright, "Copyright"),
    (Tag::Software, "Software"),
    (Tag::DateTime, "Written"),
];

/// The tags the georeference speaks for: the two that place the raster, the
/// matrix form of the same thing, the directory of keys and the pool of names
/// it points into, and the value that means nothing was measured.
///
/// Not the pool of doubles, 34736: a key that points into it is a projection
/// parameter nothing above reads, so it stays in the listing.
const GEOREFERENCED: [u16; 6] = [33550, 33922, 34264, 34735, 34737, 42113];

/// The tags a georeference is built from, as the parser hands them over.
/// Reading them here rather than in [`geo`] keeps that module to arithmetic
/// on numbers, with none of the parser's types in it.
fn geo_tags(exif: &exif::Exif) -> geo::Tags {
    let tiff = |number| primary(exif, Tag(Context::Tiff, number)).map(|field| &field.value);
    let doubles = |number| match tiff(number) {
        Some(Value::Double(values)) => values.clone(),
        _ => Vec::new(),
    };
    let size = |number| primary(exif, Tag(Context::Tiff, number))?.value.get_uint(0);
    geo::Tags {
        directory: match tiff(34735) {
            Some(Value::Short(values)) => values.clone(),
            _ => Vec::new(),
        },
        // The parser splits a text value on its nulls and drops them, and the
        // keys index the pool as it was written, so the nulls go back.
        ascii: match tiff(34737) {
            Some(Value::Ascii(parts)) => parts.join(&0u8),
            _ => Vec::new(),
        },
        scale: doubles(33550),
        tiepoint: doubles(33922),
        transform: doubles(34264),
        size: size(256)
            .zip(size(257))
            .map(|(width, height)| [width, height]),
        nodata: match tiff(42113) {
            Some(Value::Ascii(parts)) => parts
                .first()
                .map(|text| String::from_utf8_lossy(text).trim().to_string())
                .filter(|text| !text.is_empty()),
            _ => None,
        },
    }
}

/// The handful of fields a photograph is read by, in the order they are read:
/// what took it, then when, then at what settings. The exposure belongs with
/// the camera rather than under a heading of its own — a shutter speed and
/// the body it was set on are read as one thought, and two headings over
/// five fields is more furniture than the panel can carry.
fn camera(exif: &exif::Exif) -> Vec<Entry> {
    let mut rows = Vec::new();
    let text = |tag| primary(exif, tag).map(|field| tidy_numbers(&display(exif, field)));

    // The maker is usually the first word of the model — "Canon EOS R6" —
    // and a camera called "Canon Canon EOS R6" reads as a mistake.
    let make = text(Tag::Make);
    let model = text(Tag::Model);
    let camera = match (&make, &model) {
        (Some(make), Some(model)) if model.starts_with(make.as_str()) => Some(model.clone()),
        (Some(make), Some(model)) => Some(format!("{make} {model}")),
        (some, None) | (None, some) => some.clone(),
    };
    push(&mut rows, "Camera", camera);
    push(&mut rows, "Lens", text(Tag::LensModel));

    // The zone the camera was set to, where it recorded one: an hour is worth
    // more than the minute it is quoted to.
    let taken = match (text(Tag::DateTimeOriginal), text(Tag::OffsetTimeOriginal)) {
        (Some(when), Some(offset)) => Some(format!("{when} {offset}")),
        (when, _) => when,
    };
    push(&mut rows, "Taken", taken);

    // One line, because they are read as one setting: the exposure that was
    // made. A compensation of zero is what every camera not being pushed
    // reports, and says nothing.
    let exposure: Vec<String> = [
        text(Tag::ExposureTime),
        text(Tag::FNumber),
        text(Tag::PhotographicSensitivity).map(|iso| format!("ISO {iso}")),
        text(Tag::ExposureBiasValue).filter(|bias| !bias.starts_with('0')),
    ]
    .into_iter()
    .flatten()
    .collect();
    push(&mut rows, "Exposure", join(&exposure));

    // The lens's own focal length, with what it comes to on the format the
    // reader is likelier to have a feel for.
    let focal = match (text(Tag::FocalLength), text(Tag::FocalLengthIn35mmFilm)) {
        (Some(actual), Some(equivalent)) if !actual.starts_with(&equivalent) => {
            Some(format!("{actual} ({equivalent} equivalent)"))
        }
        (Some(actual), _) => Some(actual),
        (None, equivalent) => equivalent.map(|shown| format!("{shown} equivalent")),
    };
    push(&mut rows, "Focal length", focal);

    rows
}

/// Where the camera stood, as the two facts a map wants of it. The rest of
/// the GPS directory is listed under these rather than beside them: this is
/// the head of a section, not the whole of one.
fn location(exif: &exif::Exif) -> Vec<Entry> {
    let mut rows = Vec::new();
    push(&mut rows, "Coordinates", coordinates(exif));
    push(&mut rows, "Altitude", altitude(exif));
    rows
}

/// What the file says in words, under the names those fields are spoken by
/// rather than the ones the standard files them under.
fn described(exif: &exif::Exif) -> Vec<Entry> {
    let mut rows = Vec::new();
    for (tag, name) in DESCRIBED {
        let value = match tag {
            Tag::UserComment => comment(exif),
            tag => primary(exif, tag).map(|field| tidy_numbers(&display(exif, field))),
        };
        push(&mut rows, name, value);
    }
    rows
}

/// What `UserComment` says, which the renderer will not tell us. The field is
/// eight bytes naming a character code and then the text in it, and a
/// renderer that knows only that the type is undefined writes the whole thing
/// out as hex. Hex is not a comment, so it is read here or it is left out.
///
/// The two codes anything writes are ASCII and UTF-16. JIS is left alone
/// because nothing here can read it. The all-zero code means the writer did
/// not say which — but almost every writer that leaves it blank wrote text
/// anyway, so those bytes are taken as text where they will bear it and
/// dropped where they will not.
fn comment(exif: &exif::Exif) -> Option<String> {
    let Value::Undefined(bytes, _) = &primary(exif, Tag::UserComment)?.value else {
        return None;
    };
    let (code, text) = bytes.split_at_checked(8)?;
    let decoded = match code {
        b"ASCII\0\0\0" => String::from_utf8_lossy(text).into_owned(),
        b"UNICODE\0" => {
            // In the byte order of the block it came out of, which is the
            // only thing that says which way round the pairs go.
            let units: Vec<u16> = text
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&pair| {
                    if exif.little_endian() {
                        u16::from_le_bytes(pair)
                    } else {
                        u16::from_be_bytes(pair)
                    }
                })
                .collect();
            String::from_utf16_lossy(&units)
        }
        [0, 0, 0, 0, 0, 0, 0, 0] => std::str::from_utf8(text).ok()?.to_string(),
        _ => return None,
    };
    // Padded out to a round length with nulls, as often as not.
    let trimmed = decoded.trim_matches('\0').trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Where the camera was, in the degrees a map will take: the sexagesimal the
/// file holds is exact and unusable, and the hemisphere is kept as its letter
/// rather than as a sign, which is read wrong more often than not.
fn coordinates(exif: &exif::Exif) -> Option<String> {
    let axis = |value: Tag, reference: Tag| {
        let degrees = degrees(&primary(exif, value)?.value)?;
        let hemisphere = primary(exif, reference)?
            .display_value()
            .to_string()
            .trim_matches('"')
            .to_string();
        Some(format!("{degrees:.5}\u{00b0} {hemisphere}"))
    };
    let latitude = axis(Tag::GPSLatitude, Tag::GPSLatitudeRef)?;
    let longitude = axis(Tag::GPSLongitude, Tag::GPSLongitudeRef)?;
    Some(format!("{latitude}, {longitude}"))
}

/// Degrees, minutes and seconds as one number.
fn degrees(value: &Value) -> Option<f64> {
    let parts: &[Rational] = match value {
        Value::Rational(parts) => parts,
        _ => return None,
    };
    let part = |index: usize| parts.get(index).map_or(0.0, |part| part.to_f64());
    if parts.is_empty() {
        return None;
    }
    Some(part(0) + part(1) / 60.0 + part(2) / 3600.0)
}

/// How high the camera was, to the metre. Below sea level is a real answer and
/// a signed one, which is why the reference tag is asked as well.
fn altitude(exif: &exif::Exif) -> Option<String> {
    let metres = match &primary(exif, Tag::GPSAltitude)?.value {
        Value::Rational(parts) => parts.first()?.to_f64(),
        _ => return None,
    };
    let below = primary(exif, Tag::GPSAltitudeRef)
        .and_then(|field| field.value.get_uint(0))
        .is_some_and(|reference| reference == 1);
    let signed = if below { -metres } else { metres };
    Some(format!("{signed:.0} m"))
}

fn primary(exif: &exif::Exif, tag: Tag) -> Option<&exif::Field> {
    exif.get_field(tag, In::PRIMARY)
}

/// What a field says, as words.
///
/// The renderer is what gives a value its unit, its fraction and the word for
/// its enumeration, so everything goes through it — but it quotes plain text,
/// which is right for a dump and wrong in a panel. A single quoted string is
/// therefore unwrapped: `Make` is Apple, not "Apple". A date is left as it
/// comes, since the renderer has already turned it into one.
fn display(exif: &exif::Exif, field: &exif::Field) -> String {
    // The renderer knows the compressions a photograph is stored in and calls
    // the rest reserved. A raster is usually one of the rest, and "reserved
    // compression 5" is a worse answer than LZW for a code the format has
    // meant LZW since 1992.
    if field.tag == Tag::Compression
        && let Some(code) = field.value.get_uint(0)
        && let Some(name) = compression(code)
    {
        return name.to_string();
    }
    let shown = field.display_value().with_unit(exif).to_string();
    let single = matches!(&field.value, Value::Ascii(parts) if parts.len() == 1);
    match shown
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    {
        Some(text) if single => text.to_string(),
        _ => shown,
    }
}

fn push(rows: &mut Vec<Entry>, name: &str, value: Option<String>) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        rows.push(Entry::new(name, shorten(&value)));
    }
}

/// Between one part of a compound value and the next — the same thin gap the
/// bars part their segments with, this being the same middot doing the same
/// work a panel further in.
const SEPARATOR: &str = " \u{00b7} ";

fn join(parts: &[String]) -> Option<String> {
    (!parts.is_empty()).then(|| parts.join(SEPARATOR))
}

/// Names for tags the metadata standard does not describe.
///
/// It covers what a photograph carries and no more, so the rest of TIFF 6,
/// the tags GeoTIFF and GDAL park in the same directory, and the blocks other
/// standards park there too all arrive as numbers. A raster is mostly these,
/// and a column of numbered tags says what is in the file without saying what
/// any of it is.
const TIFF_NAMES: [(u16, &str); 26] = [
    (266, "FillOrder"),
    (269, "DocumentName"),
    (285, "PageName"),
    (316, "HostComputer"),
    (317, "Predictor"),
    (320, "ColorMap"),
    (322, "TileWidth"),
    (323, "TileLength"),
    (324, "TileOffsets"),
    (325, "TileByteCounts"),
    (338, "ExtraSamples"),
    (339, "SampleFormat"),
    (340, "SMinSampleValue"),
    (341, "SMaxSampleValue"),
    (347, "JPEGTables"),
    (700, "XMP"),
    (33550, "ModelPixelScale"),
    (33723, "IPTC"),
    (33922, "ModelTiepoint"),
    (34264, "ModelTransformation"),
    (34675, "ICCProfile"),
    (34735, "GeoKeyDirectory"),
    (34736, "GeoDoubleParams"),
    (34737, "GeoAsciiParams"),
    (42112, "GdalMetadata"),
    (42113, "GdalNoData"),
];

/// What a field is called, for a reader: its name where the tag is one the
/// standards describe or one of [`TIFF_NAMES`], and otherwise the number it
/// is filed under, which is the only honest thing to call it.
fn tag_name(tag: Tag) -> String {
    if tag.description().is_some() {
        return tag.to_string();
    }
    if tag.context() == Context::Tiff
        && let Some((_, name)) = TIFF_NAMES
            .iter()
            .find(|(number, _)| *number == tag.number())
    {
        return name.to_string();
    }
    let context = match tag.context() {
        Context::Tiff => "TIFF",
        Context::Exif => "Exif",
        Context::Gps => "GPS",
        Context::Interop => "Interop",
        _ => "Unknown",
    };
    format!("{context} tag {}", tag.number())
}

/// What a compression code means, where it is one this says anything about.
/// `None` leaves the answer to the renderer, which knows the ones a JPEG or a
/// TIFF thumbnail uses.
fn compression(code: u32) -> Option<&'static str> {
    Some(match code {
        1 => "uncompressed",
        2 => "CCITT modified Huffman",
        3 => "CCITT Group 3 fax",
        4 => "CCITT Group 4 fax",
        5 => "LZW",
        6 => "JPEG (old-style)",
        7 => "JPEG",
        8 => "Deflate",
        32773 => "PackBits",
        32946 => "Deflate (old-style)",
        34712 => "JPEG 2000",
        34887 => "LERC",
        34925 => "LZMA",
        50000 => "Zstandard",
        50001 => "WebP",
        50002 => "JPEG XL",
        _ => return None,
    })
}

/// Whether a value is bulk rather than a fact: a maker note, a color map, a
/// table of strip offsets. Written out it would be pages of hexadecimal, and
/// rendering it costs the memory of the string as well as the room.
fn is_bulk(value: &Value) -> bool {
    match value {
        Value::Byte(parts) => parts.len() > MAX_COMPONENTS,
        Value::Short(parts) => parts.len() > MAX_COMPONENTS,
        Value::Long(parts) => parts.len() > MAX_COMPONENTS,
        Value::Rational(parts) => parts.len() > MAX_COMPONENTS,
        Value::SByte(parts) => parts.len() > MAX_COMPONENTS,
        Value::SShort(parts) => parts.len() > MAX_COMPONENTS,
        Value::SLong(parts) => parts.len() > MAX_COMPONENTS,
        Value::SRational(parts) => parts.len() > MAX_COMPONENTS,
        Value::Float(parts) => parts.len() > MAX_COMPONENTS,
        Value::Double(parts) => parts.len() > MAX_COMPONENTS,
        // The maker note lands here, and so does anything else a camera
        // stores as a block of bytes. Short ones — the Exif version is four
        // characters — are worth keeping.
        Value::Undefined(bytes, _) => bytes.len() > MAX_COMPONENTS,
        Value::Ascii(_) | Value::Unknown(..) => false,
    }
}

/// Cuts a rendered value down to something a panel can hold, and takes the
/// control characters out of it. The text is whatever was written into the
/// file: a newline in it would break the column it is laid out in, and a
/// terminating null is not a character to draw.
fn shorten(value: &str) -> String {
    let value: String = value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let value = value.trim();
    match value.char_indices().nth(MAX_VALUE_CHARS) {
        Some((cut, _)) => format!("{}\u{2026}", &value[..cut]),
        None => value.to_string(),
    }
}

/// Rewrites every number in `text` to the digits it is worth.
///
/// A rational rendered through binary floating point comes out as
/// "1.7799999713880652" — exact, and not what the file means. Rounding the
/// numbers where they are written keeps everything else the renderer does
/// well: the units, the fractions a shutter speed keeps, and the words the
/// enumerations are given.
fn tidy_numbers(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(|c: char| c.is_ascii_digit()) {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let end = rest
            .find(|c: char| !c.is_ascii_digit() && c != '.')
            .unwrap_or(rest.len());
        let (number, after) = rest.split_at(end);
        out.push_str(&tidy(number));
        rest = after;
        // A run that ended on something that is not a digit: step past it, so
        // that a stray '.' cannot leave the scan where it started.
        if let Some(next) = rest.chars().next()
            && !next.is_ascii_digit()
        {
            out.push(next);
            rest = &rest[next.len_utf8()..];
        }
    }
    out.push_str(rest);
    out
}

/// One number, at [`SIGNIFICANT_DIGITS`] and without the zeroes that leaves.
/// One rounding policy for the whole panel: [`super::geo`] writes its
/// coordinates through this as well.
pub(super) fn tidy(number: &str) -> String {
    // Only a plain decimal is rewritten. Anything else — a version, a date,
    // a number with two points in it — is left exactly as it was written.
    let Some((whole, fraction)) = number.split_once('.') else {
        return number.to_string();
    };
    if fraction.contains('.') || fraction.is_empty() {
        return number.to_string();
    }
    let Ok(value) = number.parse::<f64>() else {
        return number.to_string();
    };
    // Digits after the point, so that the whole number carries the same
    // count of them whatever its size. A number below 1 keeps its leading
    // zeroes as well, so that a small one does not round away to nothing.
    let integral = whole.trim_start_matches(['-', '0']).len();
    let leading = if integral == 0 {
        fraction.len() - fraction.trim_start_matches('0').len()
    } else {
        0
    };
    let places = (SIGNIFICANT_DIGITS + leading).saturating_sub(integral);
    if fraction.len() <= places {
        return number.to_string();
    }
    let rounded = format!("{value:.places$}");
    match rounded.split_once('.') {
        Some((whole, fraction)) => {
            let fraction = fraction.trim_end_matches('0');
            if fraction.is_empty() {
                whole.to_string()
            } else {
                format!("{whole}.{fraction}")
            }
        }
        None => rounded,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One IFD under construction: its entries, and the values too large to
    /// sit inside one. Laid out at a given offset from the start of the TIFF
    /// header, since that is what every offset in the block is measured from.
    struct Block {
        entries: Vec<[u8; 12]>,
        pool: Vec<u8>,
        /// Which entries hold a place in the pool rather than a value, and so
        /// which have to be told where the pool landed.
        offsets: Vec<usize>,
    }

    impl Block {
        fn new() -> Self {
            Self {
                entries: Vec::new(),
                pool: Vec::new(),
                offsets: Vec::new(),
            }
        }

        /// What this IFD will come to: the count, the entries, the offset of
        /// the next IFD, and the values that did not fit in them.
        fn length(&self) -> usize {
            Self::length_of(self.entries.len(), self.pool.len())
        }

        fn length_of(entries: usize, pool: usize) -> usize {
            2 + entries * 12 + 4 + pool
        }

        fn entry(&mut self, tag: u16, kind: u16, count: u32, inline: [u8; 4]) {
            let mut entry = [0u8; 12];
            entry[0..2].copy_from_slice(&tag.to_le_bytes());
            entry[2..4].copy_from_slice(&kind.to_le_bytes());
            entry[4..8].copy_from_slice(&count.to_le_bytes());
            entry[8..12].copy_from_slice(&inline);
            self.entries.push(entry);
        }

        fn short(&mut self, tag: u16, value: u16) {
            let mut inline = [0u8; 4];
            inline[0..2].copy_from_slice(&value.to_le_bytes());
            self.entry(tag, 3, 1, inline);
        }

        /// A value that lives in the pool. The entry holds where it went in
        /// the pool; `at` turns that into an offset in the file.
        fn pooled(&mut self, tag: u16, kind: u16, count: u32, bytes: &[u8]) {
            let at = self.pool.len() as u32;
            self.pool.extend_from_slice(bytes);
            self.entry(tag, kind, count, at.to_le_bytes());
            self.offsets.push(self.entries.len() - 1);
        }

        fn ascii(&mut self, tag: u16, text: &str) {
            let mut bytes = text.as_bytes().to_vec();
            bytes.push(0);
            // Four bytes or fewer live in the entry itself, which is what a
            // hemisphere — "S", and a null — actually does.
            if bytes.len() <= 4 {
                let mut inline = [0u8; 4];
                inline[..bytes.len()].copy_from_slice(&bytes);
                self.entry(tag, 2, bytes.len() as u32, inline);
                return;
            }
            self.pooled(tag, 2, bytes.len() as u32, &bytes);
        }

        fn undefined(&mut self, tag: u16, bytes: &[u8]) {
            self.pooled(tag, 7, bytes.len() as u32, bytes);
        }

        fn rational(&mut self, tag: u16, parts: &[(u32, u32)]) {
            let mut bytes = Vec::new();
            for (numerator, denominator) in parts {
                bytes.extend_from_slice(&numerator.to_le_bytes());
                bytes.extend_from_slice(&denominator.to_le_bytes());
            }
            self.pooled(tag, 5, parts.len() as u32, &bytes);
        }

        /// The laid-out IFD, to sit at `base` bytes from the TIFF header.
        fn at(mut self, base: usize) -> Vec<u8> {
            let pool_at = (base + 2 + self.entries.len() * 12 + 4) as u32;
            for index in &self.offsets {
                let entry = &mut self.entries[*index];
                let at = u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]);
                entry[8..12].copy_from_slice(&(at + pool_at).to_le_bytes());
            }
            let mut out = (self.entries.len() as u16).to_le_bytes().to_vec();
            for entry in &self.entries {
                out.extend_from_slice(entry);
            }
            // No IFD after this one.
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&self.pool);
            out
        }
    }

    /// A JPEG carrying `block` and nothing else. The parser wants the
    /// container's marker and the segment around the metadata; it neither
    /// looks for nor needs an image.
    fn jpeg_with(block: Vec<u8>) -> Vec<u8> {
        let mut payload = b"Exif\0\0".to_vec();
        payload.extend_from_slice(&block);
        let mut file = b"\xff\xd8\xff\xe1".to_vec();
        file.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        file.extend_from_slice(&payload);
        file.extend_from_slice(b"\xff\xd9");
        file
    }

    fn empty(exif: &Exif) -> bool {
        exif.sections.is_empty()
    }

    /// What one group holds, and nothing where the file gave that group no
    /// fields and it was therefore never made.
    fn section<'a>(exif: &'a Exif, name: &str) -> &'a [Entry] {
        match exif.sections.iter().find(|section| section.name == name) {
            Some(section) => &section.entries,
            None => &[],
        }
    }

    /// Every field the file came back with, whichever group it landed in:
    /// for the tests that care that a fact is there rather than where.
    fn all(exif: &Exif) -> Vec<&Entry> {
        exif.sections
            .iter()
            .flat_map(|section| section.entries.iter())
            .collect()
    }

    fn written(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("gamut-exif-{name}"));
        std::fs::write(&path, bytes).expect("the temporary directory is writable");
        path
    }

    /// A photograph's own IFD, with a sub-IFD of exposure settings and a GPS
    /// one, laid out as a camera writes them.
    fn photograph() -> Vec<u8> {
        let mut exif = Block::new();
        exif.rational(0x829a, &[(1, 50)]); // ExposureTime
        exif.rational(0x829d, &[(89, 50)]); // FNumber
        exif.short(0x8827, 200); // PhotographicSensitivity
        exif.ascii(0x9003, "2026:08:27 20:06:17"); // DateTimeOriginal
        exif.ascii(0x9011, "+12:00"); // OffsetTimeOriginal
        exif.rational(0x920a, &[(6765, 1000)]); // FocalLength
        exif.short(0xa405, 24); // FocalLengthIn35mmFilm
        exif.ascii(0xa434, "A Lens 6.765mm f/1.78"); // LensModel
        // UserComment: eight bytes of character code, then the words.
        exif.undefined(0x9286, b"ASCII\0\0\0On a post by the jetty\0");

        let mut gps = Block::new();
        gps.ascii(0x0001, "S"); // GPSLatitudeRef
        gps.rational(0x0002, &[(44, 1), (40, 1), (5527, 100)]); // GPSLatitude
        gps.ascii(0x0003, "E"); // GPSLongitudeRef
        gps.rational(0x0004, &[(169, 1), (9, 1), (4304, 100)]); // GPSLongitude
        gps.short(0x0005, 0); // GPSAltitudeRef, above sea level
        gps.rational(0x0006, &[(3329957, 10000)]); // GPSAltitude

        let mut ifd0 = Block::new();
        ifd0.ascii(0x010f, "Apple"); // Make
        ifd0.ascii(0x0110, "Apple iPhone 16 Pro"); // Model
        ifd0.short(0x0112, 6); // Orientation
        ifd0.ascii(0x0131, "26.6"); // Software

        // The two sub-IFDs follow the one that points at them, so where they
        // go is known once the pointers themselves have been counted in.
        const HEADER: usize = 8;
        let first = Block::length_of(ifd0.entries.len() + 2, ifd0.pool.len());
        let exif_at = HEADER + first;
        let gps_at = exif_at + exif.length();
        ifd0.entry(0x8769, 4, 1, (exif_at as u32).to_le_bytes()); // ExifIFDPointer
        ifd0.entry(0x8825, 4, 1, (gps_at as u32).to_le_bytes()); // GPSInfoIFDPointer
        assert_eq!(
            ifd0.length(),
            first,
            "the sub-IFDs go where they were said to"
        );

        // The header, and the three IFDs one after another.
        let mut block = b"II\x2a\x00\x08\x00\x00\x00".to_vec();
        block.extend_from_slice(&ifd0.at(HEADER));
        block.extend_from_slice(&exif.at(exif_at));
        block.extend_from_slice(&gps.at(gps_at));
        block
    }

    /// The panel's top sections: the fields a photograph is read by, combined
    /// into the lines they are read as, in units a reader can use, and under
    /// the headings they are looked for beneath.
    #[test]
    fn a_photograph_is_summarised_as_it_would_be_read() {
        let path = written("photograph.jpg", &jpeg_with(photograph()));
        let exif = Exif::read(&path);
        let _ = std::fs::remove_file(&path);

        let rows = |name: &str| -> Vec<(String, String)> {
            section(&exif, name)
                .iter()
                .map(|entry| (entry.name.clone(), entry.value.clone()))
                .collect()
        };
        let pairs = |listed: &[(&str, &str)]| -> Vec<(String, String)> {
            listed
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect()
        };

        // The exposure is read together with the body it was set on, so the
        // two are one section rather than two.
        assert_eq!(
            rows("Camera"),
            pairs(&[
                // The maker is not said twice, though the file says it twice.
                ("Camera", "Apple iPhone 16 Pro"),
                ("Lens", "A Lens 6.765mm f/1.78"),
                ("Taken", "2026-08-27 20:06:17 +12:00"),
                // 89/50 is exactly 1.78, and is quoted as such.
                (
                    "Exposure",
                    "1/50 s \u{00b7} f/1.78 \u{00b7} ISO 200"
                ),
                ("Focal length", "6.765 mm (24 mm equivalent)"),
            ])
        );
        assert_eq!(
            rows("Location"),
            pairs(&[
                ("Coordinates", "44.68202\u{00b0} S, 169.16196\u{00b0} E"),
                ("Altitude", "333 m"),
            ])
        );

        // What a section above spoke for is not listed again; what none of
        // them did is, under the directory it came out of. The software that
        // wrote the file is one of the fields worth reading in words, so it
        // is drawn out of the listing rather than left in it.
        let listing: Vec<&str> = section(&exif, "Image metadata")
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        assert_eq!(listing, ["Orientation"], "{listing:?}");
        // The comment is read out of its character code rather than written
        // out as the hex the renderer would make of an undefined type.
        assert_eq!(
            rows("Description"),
            pairs(&[("Comment", "On a post by the jetty"), ("Software", "26.6")]),
            "{exif:?}"
        );
        let every: Vec<&str> = all(&exif).iter().map(|e| e.name.as_str()).collect();
        assert!(!every.contains(&"Model"), "{every:?}");
        assert!(!every.contains(&"FNumber"), "{every:?}");
        assert!(!every.contains(&"GPSAltitude"), "{every:?}");
    }

    /// A file with no metadata, and one whose metadata is nonsense, are the
    /// same thing to everything upstream: nothing to show, no error to carry.
    #[test]
    fn a_file_with_nothing_to_say_says_nothing() {
        for (name, bytes) in [
            ("empty.jpg", b"\xff\xd8\xff\xd9".to_vec()),
            ("truncated.jpg", jpeg_with(b"II\x2a\x00\x08".to_vec())),
            ("nonsense.jpg", jpeg_with(vec![0x5a; 64])),
            ("nothing.bin", Vec::new()),
        ] {
            let path = written(name, &bytes);
            let exif = Exif::read(&path);
            let _ = std::fs::remove_file(&path);
            assert!(empty(&exif), "{name} produced {exif:?}");
        }

        // A path that is not there at all is the same again.
        assert!(empty(&Exif::read(Path::new("/nonexistent/image.jpg"))));
    }

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_images")
            .join(name)
    }

    /// A real file, read through the container it arrives in: the fixture
    /// carries an orientation and nothing else.
    #[test]
    fn a_files_own_block_is_found_through_its_container() {
        let exif = Exif::read(&fixture("webp-exif-rotated.webp"));
        assert!(
            all(&exif).iter().any(|entry| entry.name == "Orientation"),
            "{exif:?}"
        );
    }

    /// A BigTIFF has to go the long way round — the reader knows the
    /// original format only — and comes back with the same fields under the
    /// same names as the file it is a bigger version of.
    #[test]
    fn a_bigtiff_is_read_through_its_directory() {
        let big = Exif::read(&fixture("tiff-bigtiff.tif"));
        // The same image in the ordinary form: both fixtures are the 32x24
        // float raster, one written each way.
        let ordinary = Exif::read(&fixture("tiff-nodata.tif"));
        let named = |exif: &Exif, name: &str| {
            all(exif)
                .into_iter()
                .find(|entry| entry.name == name)
                .map(|entry| entry.value.clone())
        };
        for name in ["ImageWidth", "ImageLength", "SampleFormat", "BitsPerSample"] {
            assert_eq!(named(&big, name), named(&ordinary, name), "{name}");
            assert!(named(&big, name).is_some(), "{name} is missing");
        }
        assert_eq!(named(&big, "ImageWidth").as_deref(), Some("32 pixels"));
    }

    /// A measurement raster, read out of a real TIFF: the value that stands
    /// for nothing measured belongs to the georeference rather than to the
    /// listing, and having been said there it is not said twice.
    #[test]
    fn a_rasters_own_facts_are_taken_out_of_the_listing() {
        let exif = Exif::read(&fixture("tiff-nodata.tif"));
        assert_eq!(
            section(&exif, "Georeference"),
            [Entry::new("No data", "-9999")],
            "{exif:?}"
        );
        assert!(
            !all(&exif).iter().any(|entry| entry.name.contains("42113")),
            "{exif:?}"
        );
        // And the tags the standard does not describe are named rather than
        // numbered: this one says its pixels are floating point.
        assert!(
            all(&exif)
                .iter()
                .any(|entry| entry.name == "SampleFormat" && entry.value == "3"),
            "{exif:?}"
        );
    }

    /// A TIFF is its own metadata block, and a large one must not be read
    /// whole to find it. What is read is a prefix, and a directory at the
    /// front of the file — where every writer of a large one puts it — is
    /// inside it however long the file goes on.
    #[test]
    fn a_tiff_is_read_as_far_as_its_directory_and_no_further() {
        let mut block = photograph();
        let directory = block.len();
        // Everything a file of this shape holds after its directory: pixels,
        // as far as this is concerned.
        block.extend(std::iter::repeat_n(0x5a, 4096));
        let path = written("prefixed.tif", &block);

        // Read no further than the directory itself, and the fields are all
        // still there.
        let bounded = Exif::parse(&path, directory as u64).expect("the directory parses");
        let whole = Exif::parse(&path, u64::MAX).expect("the whole file parses");
        let _ = std::fs::remove_file(&path);
        assert_eq!(bounded.sections, whole.sections);
        assert!(!bounded.sections.is_empty());

        // And a prefix that stops short of it is a file with nothing to say,
        // rather than an error anything upstream has to handle.
        assert!(empty(
            &Exif::parse(Path::new("/nonexistent.tif"), 8).unwrap_or_default()
        ));
    }

    #[test]
    fn a_number_is_written_to_the_digits_it_is_worth() {
        // The reason this exists: a rational through binary floating point.
        assert_eq!(tidy_numbers("f/1.7799999713880652"), "f/1.78");
        assert_eq!(tidy_numbers("6.764999866370901 mm"), "6.765 mm");
        assert_eq!(
            tidy_numbers("332.9957081545064 meters above sea level"),
            "332.996 meters above sea level"
        );
        assert_eq!(
            tidy_numbers("2.220000028611935-15.659999847383 mm, f/1.7799999713880652-2.8"),
            "2.22-15.66 mm, f/1.78-2.8"
        );

        // Everything that is already what it should be is left alone.
        for text in [
            "1/50 s",
            "ISO 200",
            "2026-08-27 20:06:17",
            "08:06:14.88",
            "2.32",
            "44 deg 40 min 55.27 sec S",
            "rectangle (x=1464, y=1730, w=497, h=497)",
            "0",
            "pattern",
            "",
        ] {
            assert_eq!(tidy_numbers(text), text, "{text:?} was rewritten");
        }

        // A number smaller than one keeps its figures rather than rounding
        // away to nothing.
        assert_eq!(tidy_numbers("0.000123456789 s"), "0.000123457 s");
    }
}
