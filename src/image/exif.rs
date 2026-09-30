//! What a file says about the photograph in it.
//!
//! The EXIF block is metadata, not pixels: nothing here reaches the decoders
//! or the image, and nothing that is read here changes what is drawn. It is
//! read on the loader thread beside the decode, because it is one more parse
//! of a file whoever wrote it chose the bytes of.
//!
//! What comes back is [`Section`]s, in the order the panel reads them. A
//! photograph is looked at through a handful of fields — what took it, and
//! when and at what exposure — and those are gathered, combined and given
//! their units under `Camera` and `Exposure`, with the GPS directory
//! becoming `Location`. A raster is
//! looked at through a different handful, which are not EXIF at all but
//! GeoTIFF keys packed into the same directory, and [`super::geo`] takes
//! those apart into `Georeference`. The fields somebody wrote in words are
//! pulled out as `About` — from the EXIF block, and from the XMP packet
//! beside it, which [`super::xmp`] reads and which is where a title, a
//! caption or a keyword is written when a file has one at all — with the
//! regions the packet marks out on the picture kept aside, to be written out
//! against the picture as it is shown. Nothing is listed field by field:
//! once those are read out of the block, what is left describes how the
//! file is laid out and how the camera describes itself, and a panel for
//! reading about the picture is better without it.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use ::image::metadata::Orientation;
use exif::{Context, In, Rational, Tag, Value};

use super::metadata_region::{MetadataRegion, Placed};
use super::orient::Turn;
use super::xmp::{self, Xmp};
use super::{directory, enclosed, geo, tiff};

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

/// One region as the panel shows it, in the picture as shown: see
/// [`Exif::regions`].
#[derive(Clone, PartialEq, Debug)]
pub struct ShownRegion {
    /// The row as it is copied: named by the region's kind, and saying who
    /// or what is in it, what is written about it, and where it is.
    pub entry: Entry,
    /// Who or what is in it, or what kind of region it is where nothing
    /// says who.
    pub subject: String,
    /// Its kind, who or what is in it, and what is written about it.
    pub about: String,
    /// Where it is; `None` for a region that names something without
    /// saying where.
    pub placed: Option<Placed>,
}

/// One group of fields under the heading it is read by. Never empty: a
/// heading with a blank under it is a question about where the rest of it
/// went, so a group that came to nothing is not carried at all.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Section {
    pub group: Group,
    pub entries: Vec<Entry>,
}

/// Which group a section is, in the order the panel reads them: what was
/// written about the picture, what took it, how, where, and where its
/// pixels are on the ground. The panel puts what it says about the picture
/// itself between the first and the rest — see `ui/info.rs`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Group {
    About,
    Camera,
    Exposure,
    Location,
    Georeference,
}

impl Group {
    /// Its heading.
    pub fn name(self) -> &'static str {
        match self {
            Group::Camera => "Camera",
            Group::Exposure => "Exposure",
            Group::Location => "Location",
            Group::Georeference => "Georeference",
            Group::About => "About",
        }
    }
}

/// A file's metadata, ready to be read: no tags, no types, no offsets, only
/// what the fields say. Empty when the file carries none, or carries some
/// that will not parse — a photograph with unreadable metadata is still a
/// photograph, so nothing here is an error anything else has to handle.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Exif {
    /// The groups the file's fields fall into, in the order they are read:
    /// what was written about the picture, what took it, where it was taken,
    /// and where its pixels are on the ground.
    pub sections: Vec<Section>,
    /// Where the raster's pixels are on the ground, for the pointer's
    /// readout: the same tags the `Georeference` section is written from,
    /// kept as numbers. `None` for everything that is not a map.
    pub georeference: Option<geo::Georeference>,
    /// Where the camera was, as the GPS directory says: the latitude and
    /// the longitude in signed degrees, for the map the `Location` section
    /// offers to open. `None` where the file does not say.
    pub position: Option<[f64; 2]>,
    /// The regions the metadata marks out on the picture — the block's
    /// `SubjectArea`, then the XMP packet's — kept as numbers: they are in
    /// the picture as stored, and the panel writes them out in the picture
    /// as shown, which the turn in force decides — see [`Exif::regions`].
    pub regions: Vec<MetadataRegion>,
    /// The turn the EXIF orientation tag asks for, which the regions were
    /// marked out before. `None` where the file does not say.
    pub orientation: Option<Orientation>,
    /// How the pixels are compressed, for a TIFF: its own directory says,
    /// and the info panel's `Image` section shows it. `None` for every other
    /// file — a raw's directory is about the preview in front of it, and
    /// any other container's EXIF block has no pixels of its own.
    pub compression: Option<String>,
}

impl Exif {
    /// Reads `path`'s metadata, or gives back nothing at all.
    pub fn read(path: &Path) -> Self {
        let mut exif = Self::read_with(path, tiff::PREFIX);
        // A raw has a second reader of its header, the library that will
        // develop it. What it made of the camera and the exposure fills in
        // whatever the EXIF left out, a row at a time — all of it, for a
        // CRW, which has no EXIF, and the exposure of a Phase One, whose
        // EXIF names the camera and stops — and the color temperature, which
        // only the library's reading of the sensor can work out.
        if let Some(rows) = super::decode::facts(path) {
            for entry in rows {
                let group = if rank(&entry.name) < rank(TAKEN) {
                    Group::Camera
                } else {
                    Group::Exposure
                };
                exif.fill(group, entry);
            }
            exif.compression = None;
        }
        exif
    }

    /// Puts `entry` in `group`'s section where the section does not already
    /// have a row of its name: where the EXIF reader would have put it,
    /// before the first row that comes after it, and the section made where
    /// there was none, in its place among the others.
    fn fill(&mut self, group: Group, entry: Entry) {
        let at = self
            .sections
            .iter()
            .position(|section| section.group >= group)
            .unwrap_or(self.sections.len());
        if self
            .sections
            .get(at)
            .is_none_or(|section| section.group != group)
        {
            self.sections.insert(
                at,
                Section {
                    group,
                    entries: Vec::new(),
                },
            );
        }
        let entries = &mut self.sections[at].entries;
        if entries.iter().any(|have| have.name == entry.name) {
            return;
        }
        let before = entries
            .iter()
            .position(|have| rank(&have.name) > rank(&entry.name))
            .unwrap_or(entries.len());
        entries.insert(before, entry);
    }

    /// `prefix` is how much of a TIFF to read; see [`tiff::PREFIX`].
    fn read_with(path: &Path, prefix: u64) -> Self {
        let block = Self::parse(path, prefix);
        // A TIFF keeps its packet in a tag of the directory just read, so it
        // is taken from there where the read reached it; every other
        // container keeps it in a chunk of its own, which is found by
        // walking the file a second time — as is a TIFF's when the
        // directory had to be read the long way round, since the block
        // written back leaves anything this long behind.
        let xmp = match block.as_ref().and_then(embedded_packet) {
            Some(packet) => Xmp::read_with(path, Some(packet)),
            None => Xmp::read(path),
        };
        Self::assemble(block.as_ref(), &xmp)
    }

    /// What the file is called, as the panel shows it: the chooser's row
    /// says the same words, so a file found by its title reads the same
    /// in both.
    pub fn title(&self) -> Option<&str> {
        self.sections
            .iter()
            .find(|section| section.group == Group::About)
            .and_then(|section| section.entries.iter().find(|entry| entry.name == TITLE))
            .map(|entry| entry.value.as_str())
    }

    /// The regions marked out on the picture, a row each, in the order the
    /// packet lists them: named by what kind of region it is, and saying who
    /// or what is in it, anything written about it, and where it is.
    ///
    /// Where it is is in the picture as shown — `shown` pixels across and
    /// down, turned by `turn` from the upright picture — so that it reads as
    /// the pointer's coordinate and a marked region do: the top left corner
    /// and the size. The packet's numbers are in the picture as stored,
    /// before the orientation tag turned it, so they are carried through
    /// that turn and then through `turn`, and taken as shares of the
    /// picture's sides, so that a picture made smaller since it was marked
    /// is still marked in the right place.
    ///
    /// A region that says nothing beyond its kind is no row.
    pub fn regions(&self, shown: [u32; 2], turn: Turn) -> Vec<ShownRegion> {
        let orientation = self.orientation.unwrap_or(Orientation::NoTransforms);
        let mut shown_regions = Vec::new();
        for region in &self.regions {
            let placed = region.placed(orientation, turn, shown);
            let parts: Vec<String> = region
                .name
                .iter()
                .chain(&region.details)
                .cloned()
                .chain(placed.map(Placed::words))
                .collect();
            let Some(value) = join(&parts).filter(|value| !value.is_empty()) else {
                continue;
            };
            let entry = Entry::new(region.label.clone(), shorten(&value));
            let about: Vec<String> = std::iter::once(region.label.clone())
                .chain(region.name.clone())
                .chain(region.details.iter().cloned())
                .collect();
            shown_regions.push(ShownRegion {
                entry,
                subject: shorten(region.name.as_deref().unwrap_or(&region.label)),
                about: shorten(&about.join(SEPARATOR)),
                placed,
            });
        }
        shown_regions
    }

    /// The EXIF block, parsed; `None` where there is none to parse.
    fn parse(path: &Path, prefix: u64) -> Option<exif::Exif> {
        let file = File::open(path).ok()?;
        let mut source = BufReader::new(file);

        // `read_exact` rather than one read: a short read on a file long
        // enough to be a TIFF would send it down the path that reads the
        // whole of it, which is the one thing here that must not happen.
        let mut signature = [0u8; 4];
        let read = source.read_exact(&mut signature).is_ok();
        let header = read.then(|| tiff::header(&signature)).flatten();
        // EXIF is defined on the original format: a BigTIFF goes through
        // [`directory`] and comes back as a block the reader knows.
        let tiff = matches!(header, Some((_, tiff::Kind::Classic)));
        let bigtiff = matches!(header, Some((_, tiff::Kind::Big)));
        source.seek(SeekFrom::Start(0)).ok()?;

        let mut reader = exif::Reader::new();
        // A prefix is a file with its end cut off, so offsets running past it
        // are expected rather than exceptional — and in a whole file that
        // will not parse, the fields that did parse are still the file's.
        // Either way, what could be read is worth showing.
        reader.continue_on_error(true);

        let enclosed = if tiff || bigtiff {
            None
        } else {
            enclosed::block(path)
        };
        let block = if bigtiff {
            // Not a form this reader knows: what it is handed is the same
            // directory written back out as the form it does.
            reader.read_raw(directory::block(path)?)
        } else if let Some(enclosed) = enclosed {
            // A raw container that is not a TIFF at the front, with the
            // block it keeps inside it brought out.
            match enclosed {
                enclosed::Block::Tiff(block) => reader.read_raw(block),
                enclosed::Block::Jpeg(jpeg) => {
                    reader.read_from_container(&mut std::io::Cursor::new(jpeg))
                }
            }
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
        block
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
            })
    }

    /// The sections, from an EXIF block where there is one and the XMP
    /// packet, which may be all a file has.
    fn assemble(exif: Option<&exif::Exif>, xmp: &Xmp) -> Self {
        let described = described(exif, xmp);
        let mut regions = MetadataRegion::mwg(xmp.regions());
        regions.extend(MetadataRegion::microsoft(xmp.people(), &regions));
        let Some(exif) = exif else {
            let sections = (!described.is_empty())
                .then_some(Section {
                    group: Group::About,
                    entries: described,
                })
                .into_iter()
                .collect();
            return Self {
                sections,
                georeference: None,
                position: None,
                regions,
                orientation: None,
                compression: None,
            };
        };

        let tags = geo_tags(exif);
        let geo = geo::describe(&tags);
        // The subject the camera found is a region like any other, and is
        // said with them.
        regions.splice(0..0, subject(exif));

        let sections = [
            (Group::About, described),
            (Group::Camera, camera(exif)),
            (Group::Exposure, exposure(exif)),
            (Group::Location, location(exif)),
            (Group::Georeference, geo),
        ]
        .into_iter()
        .filter(|(_, entries)| !entries.is_empty())
        .map(|(group, entries)| Section { group, entries })
        .collect();
        let orientation = primary(exif, Tag::Orientation)
            .and_then(|field| field.value.get_uint(0))
            .and_then(|value| u8::try_from(value).ok())
            .and_then(Orientation::from_exif);
        Self {
            sections,
            georeference: geo::Georeference::read(&tags),
            position: position(exif),
            regions,
            orientation,
            compression: primary(exif, Tag::Compression)
                .map(|field| display(exif, field))
                .filter(|value| !value.is_empty()),
        }
    }
}

/// The row of `About` that names the file, which the chooser reads back out.
pub const TITLE: &str = "Title";

/// One of the fields somebody wrote in words: what it is called when it is
/// spoken of, the EXIF tag that holds it, and the XMP property that does —
/// either of which a file may have, or both, or neither.
struct Described {
    name: &'static str,
    tag: Option<Tag>,
    /// The property's namespace and its name in it.
    property: Option<(&'static str, &'static str)>,
}

/// The fields somebody wrote in words: what the picture is called and what
/// it is of, who made it, what may be done with it: what a reader looking
/// for sentences rather than numbers is looking for, named as they would be
/// spoken.
///
/// Most have two homes. EXIF has a tag for the caption and the artist,
/// and XMP's Dublin Core has a property for each; a title and a set of
/// keywords have no EXIF tag at all, which is why a file that carries only
/// those is one the EXIF reader alone had nothing to say about. Where both
/// speak, the EXIF field is shown: it is the older of the two, and a program
/// that writes both writes them alike.
///
/// The program that wrote the file and when it last did are not among them,
/// though both are said in words: a camera fills them in on every file, with
/// its firmware and the moment of the shot, so they are not read as
/// something said about the picture.
const DESCRIBED: [Described; 6] = [
    Described {
        name: TITLE,
        tag: None,
        property: Some((xmp::DC, "title")),
    },
    // "Caption" rather than "Description": it is the word the cataloging
    // programs that write the field use for it, and it is what the field
    // holds — a sentence about the picture, not a description of the file.
    Described {
        name: "Caption",
        tag: Some(Tag::ImageDescription),
        property: Some((xmp::DC, "description")),
    },
    Described {
        name: "Comment",
        tag: Some(Tag::UserComment),
        property: None,
    },
    Described {
        name: "Artist",
        tag: Some(Tag::Artist),
        property: Some((xmp::DC, "creator")),
    },
    Described {
        name: "Keywords",
        tag: None,
        property: Some((xmp::DC, "subject")),
    },
    Described {
        name: "Copyright",
        tag: Some(Tag::Copyright),
        property: Some((xmp::DC, "rights")),
    },
];

/// The XMP packet a TIFF keeps in its own directory, under tag 700, as the
/// parser hands it over: a run of bytes, typed as bytes or as undefined
/// depending on who wrote the file.
fn embedded_packet(exif: &exif::Exif) -> Option<&[u8]> {
    match &primary(exif, Tag(Context::Tiff, 700))?.value {
        Value::Byte(bytes) | Value::Undefined(bytes, _) => Some(bytes),
        _ => None,
    }
}

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
/// when it was taken, what took it, then at what settings. The exposure
/// belongs with the camera rather than under a heading of its own — a
/// shutter speed and the body it was set on are read as one thought — and
/// each of its settings is a row of its own, being a thing to copy on its
/// own. The rows are named as [`super::decode::facts`] names LibRaw's, so
/// that a raw's fills in whichever of them the EXIF left out.
fn camera(exif: &exif::Exif) -> Vec<Entry> {
    let mut rows = Vec::new();
    let text = |tag| primary(exif, tag).map(|field| tidy_numbers(&display(exif, field)));
    let make = text(Tag::Make);
    push(
        &mut rows,
        CAMERA,
        camera_name(make.clone(), text(Tag::Model)),
    );
    push(&mut rows, LENS, lens(exif, make.as_deref()));

    // Whose camera it is, and which camera and lens: set in the camera's
    // menu, and padded with spaces where they were not.
    let named = |tag| text(tag).map(|value: String| value.trim().to_string());
    push(&mut rows, OWNER, named(Tag::CameraOwnerName));
    push(&mut rows, CAMERA_SERIAL, named(Tag::BodySerialNumber));
    push(&mut rows, LENS_SERIAL, named(Tag::LensSerialNumber));
    rows
}

/// How this picture was taken: when, how the exposure was decided and what
/// it came to, the focal length it was taken at, and what the camera did
/// about the light — its metering, its balance, its flash, and whether it
/// merged several frames.
fn exposure(exif: &exif::Exif) -> Vec<Entry> {
    let mut rows = Vec::new();
    let text = |tag| primary(exif, tag).map(|field| tidy_numbers(&display(exif, field)));

    // The zone the camera was set to, where it recorded one: an hour is worth
    // more than the minute it is quoted to.
    let taken = match (text(Tag::DateTimeOriginal), text(Tag::OffsetTimeOriginal)) {
        (Some(when), Some(offset)) => Some(format!("{when} {offset}")),
        (when, _) => when,
    };
    push(&mut rows, TAKEN, taken);

    // How the exposure was decided, then the exposure that was made. A
    // compensation of zero is what every camera not being pushed reports,
    // and says nothing.
    push(&mut rows, MODE, mode(exif));
    push(&mut rows, SHUTTER, text(Tag::ExposureTime));
    push(&mut rows, APERTURE, text(Tag::FNumber));
    push(&mut rows, ISO, text(Tag::PhotographicSensitivity));
    push(
        &mut rows,
        COMPENSATION,
        text(Tag::ExposureBiasValue).filter(|bias| !bias.starts_with('0')),
    );

    // The lens's own focal length, with what it comes to on the format the
    // reader is likelier to have a feel for.
    let focal = match (text(Tag::FocalLength), text(Tag::FocalLengthIn35mmFilm)) {
        (Some(actual), Some(equivalent)) if !actual.starts_with(&equivalent) => {
            Some(format!("{actual} ({equivalent} equivalent)"))
        }
        (Some(actual), _) => Some(actual),
        (None, equivalent) => equivalent.map(|shown| format!("{shown} equivalent")),
    };
    // A zoom the camera made by cropping rather than with the lens: only
    // where it made one.
    let zoom = rational(exif, Tag::DigitalZoomRatio)
        .filter(|ratio| *ratio > 1.0)
        .map(|ratio| format!("{}\u{00d7} digital zoom", number(ratio)));
    let focal = match (focal, zoom) {
        (Some(focal), Some(zoom)) => Some(format!("{focal}{SEPARATOR}{zoom}")),
        (focal, zoom) => focal.or(zoom),
    };
    push(&mut rows, FOCAL_LENGTH, focal);

    push(&mut rows, METERING, metering(exif));
    push(
        &mut rows,
        WHITE_BALANCE,
        match uint(exif, Tag::WhiteBalance) {
            Some(0) => Some("Auto".to_string()),
            Some(1) => Some("Manual".to_string()),
            _ => None,
        },
    );
    push(&mut rows, FLASH, flash(exif));
    push(
        &mut rows,
        COMPOSITE,
        match uint(exif, Tag::CompositeImage) {
            Some(2) => Some("Merged from several images".to_string()),
            Some(3) => Some("Merged from several frames as it was taken".to_string()),
            _ => None,
        },
    );
    rows
}

/// The camera, by its maker and its model. The maker is usually the first
/// word of the model — "Canon EOS R6", and Nikon's "NIKON D100" under the
/// make "NIKON CORPORATION" — and a camera called "Canon Canon EOS R6"
/// reads as a mistake, so a model that starts with the make, or with its
/// first word, is the name on its own.
pub fn camera_name(make: Option<String>, model: Option<String>) -> Option<String> {
    match (make, model) {
        (Some(make), Some(model)) => {
            let first = make.split_whitespace().next().unwrap_or_default();
            let lower = model.to_lowercase();
            if lower.starts_with(&make.to_lowercase())
                || (!first.is_empty() && lower.starts_with(&first.to_lowercase()))
            {
                Some(model)
            } else {
                Some(format!("{make} {model}"))
            }
        }
        (some, None) | (None, some) => some,
    }
}

/// A tag holding one whole number.
fn uint(exif: &exif::Exif, tag: Tag) -> Option<u32> {
    primary(exif, tag)?.value.get_uint(0)
}

/// The lens, by its name where the file gives one, with its maker's in
/// front where that is not the camera's and the name does not already say
/// it — a Sigma on a Sony. Where the file gives no name, the range of focal
/// lengths and apertures it says the lens has stands in for one: the name
/// nearly always says the same, so the two are never shown together.
fn lens(exif: &exif::Exif, camera_make: Option<&str>) -> Option<String> {
    let text = |tag| {
        primary(exif, tag)
            .map(|field| tidy_numbers(&display(exif, field)).trim().to_string())
            .filter(|value| !value.is_empty())
    };
    if let Some(model) = text(Tag::LensModel) {
        return Some(match text(Tag::LensMake) {
            Some(make)
                if !model.starts_with(&make)
                    && camera_make.is_none_or(|camera| !camera.eq_ignore_ascii_case(&make)) =>
            {
                format!("{make} {model}")
            }
            _ => model,
        });
    }
    let Value::Rational(parts) = &primary(exif, Tag::LensSpecification)?.value else {
        return None;
    };
    let known = |at: usize| {
        parts
            .get(at)
            .map(Rational::to_f64)
            .filter(|value| value.is_finite() && *value > 0.0)
    };
    let range = |low: Option<f64>, high: Option<f64>| match (low, high) {
        (Some(low), Some(high)) if low != high => {
            Some(format!("{}\u{2013}{}", number(low), number(high)))
        }
        (Some(one), _) | (None, Some(one)) => Some(number(one)),
        (None, None) => None,
    };
    let focal = range(known(0), known(1)).map(|focal| format!("{focal} mm"));
    let aperture = range(known(2), known(3)).map(|aperture| format!("f/{aperture}"));
    match (focal, aperture) {
        (Some(focal), Some(aperture)) => Some(format!("{focal} {aperture}")),
        (focal, aperture) => focal.or(aperture),
    }
}

/// A number from a rational, to two places at most and no more than it
/// has: 18, 3.5, 1.25.
fn number(value: f64) -> String {
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// How the exposure was decided: which of its settings the camera chose and
/// which were chosen for it, and whether it was one of a bracket.
fn mode(exif: &exif::Exif) -> Option<String> {
    let program = match uint(exif, Tag::ExposureProgram) {
        Some(1) => Some("Manual"),
        Some(2) => Some("Program"),
        Some(3) => Some("Aperture priority"),
        Some(4) => Some("Shutter priority"),
        Some(5) => Some("Creative"),
        Some(6) => Some("Action"),
        Some(7) => Some("Portrait"),
        Some(8) => Some("Landscape"),
        _ => None,
    };
    let bracket = (uint(exif, Tag::ExposureMode) == Some(2)).then_some("Auto bracket");
    let parts: Vec<String> = program
        .into_iter()
        .chain(bracket)
        .map(str::to_string)
        .collect();
    join(&parts)
}

/// How the camera measured the light it set the exposure by.
fn metering(exif: &exif::Exif) -> Option<String> {
    Some(
        match uint(exif, Tag::MeteringMode)? {
            1 => "Average",
            2 => "Center-weighted",
            3 => "Spot",
            4 => "Multi-spot",
            5 => "Evaluative",
            6 => "Partial",
            _ => return None,
        }
        .to_string(),
    )
}

/// Whether the flash fired, and what it was set to: the tag is a set of
/// bits, and a camera with no flash says so in one of them, which is no row.
fn flash(exif: &exif::Exif) -> Option<String> {
    let bits = uint(exif, Tag::Flash)?;
    if bits & 0x20 != 0 {
        return None;
    }
    let fired = bits & 0x01 != 0;
    let mut parts = vec![if fired { "Fired" } else { "Did not fire" }];
    match (bits >> 3) & 0x03 {
        1 => parts.push("forced on"),
        2 => parts.push("off"),
        3 => parts.push("auto"),
        _ => {}
    }
    if bits & 0x40 != 0 {
        parts.push("red-eye reduction");
    }
    if fired && (bits >> 1) & 0x03 == 2 {
        parts.push("no return light");
    }
    let parts: Vec<String> = parts.into_iter().map(str::to_string).collect();
    join(&parts)
}

/// Where a row stands in its section, `Camera` or `Exposure`: the order
/// [`camera`] and [`exposure`] push them in, which LibRaw's rows for a raw
/// are fitted into, and which of the two it is in — every row before
/// `Taken` is the camera's.
fn rank(name: &str) -> usize {
    ORDER
        .iter()
        .position(|row| *row == name)
        .unwrap_or(usize::MAX)
}

/// The rows of `Camera`, then of `Exposure`, in the order they are read.
const ORDER: [&str; 17] = [
    CAMERA,
    LENS,
    OWNER,
    CAMERA_SERIAL,
    LENS_SERIAL,
    TAKEN,
    MODE,
    SHUTTER,
    APERTURE,
    ISO,
    COMPENSATION,
    FOCAL_LENGTH,
    METERING,
    WHITE_BALANCE,
    COLOR_TEMPERATURE,
    FLASH,
    COMPOSITE,
];

/// The names of the `Camera` and `Exposure` sections' rows, which LibRaw's
/// rows for a raw are named by too.
pub const CAMERA: &str = "Camera";
pub const TAKEN: &str = "Taken";
pub const LENS: &str = "Lens";
const OWNER: &str = "Owner";
const CAMERA_SERIAL: &str = "Camera serial number";
const LENS_SERIAL: &str = "Lens serial number";
pub const MODE: &str = "Mode";
pub const SHUTTER: &str = "Shutter speed";
pub const APERTURE: &str = "Aperture";
pub const ISO: &str = "ISO";
pub const COMPENSATION: &str = "Exposure compensation";
pub const FOCAL_LENGTH: &str = "Focal length";
const METERING: &str = "Metering";
const WHITE_BALANCE: &str = "White balance";
pub const COLOR_TEMPERATURE: &str = "Color temperature";
const FLASH: &str = "Flash";
const COMPOSITE: &str = "Composite";

/// Where the camera stood and which way it faced: the two coordinates a map
/// wants, how high it was, where it was pointed, and how far out the fix
/// may be.
fn location(exif: &exif::Exif) -> Vec<Entry> {
    let mut rows = Vec::new();
    if let Some((latitude, longitude)) = coordinates(exif) {
        push(&mut rows, LATITUDE, Some(latitude));
        push(&mut rows, LONGITUDE, Some(longitude));
    }
    push(&mut rows, "Altitude", altitude(exif));
    push(&mut rows, "Direction", direction(exif));
    push(&mut rows, "Speed", speed(exif));
    push(&mut rows, "Positioning error", positioning_error(exif));
    rows
}

/// The names of the location's two coordinates, which head its section.
pub const LATITUDE: &str = "Latitude";
pub const LONGITUDE: &str = "Longitude";

/// Which way the camera was pointed, in whole degrees clockwise from the
/// north the file names — true or magnetic, which can be twenty degrees
/// apart.
fn direction(exif: &exif::Exif) -> Option<String> {
    let degrees = rational(exif, Tag::GPSImgDirection)?;
    let north = match &primary(exif, Tag::GPSImgDirectionRef).map(|field| &field.value) {
        Some(Value::Ascii(parts)) => match parts.first().map(Vec::as_slice) {
            Some(b"T") => Some("true"),
            Some(b"M") => Some("magnetic"),
            _ => None,
        },
        _ => None,
    };
    let degrees = degrees.round().rem_euclid(360.0);
    Some(match north {
        Some(north) => format!("{degrees:.0}\u{00b0} from {north} north"),
        None => format!("{degrees:.0}\u{00b0}"),
    })
}

/// How fast the camera was moving, in the unit the file names — kilometers,
/// miles or knots an hour, kilometers where it names none, as the standard
/// says — to a tenth under ten, and to the whole unit above.
fn speed(exif: &exif::Exif) -> Option<String> {
    let value = rational(exif, Tag::GPSSpeed)?;
    let unit = match &primary(exif, Tag::GPSSpeedRef).map(|field| &field.value) {
        Some(Value::Ascii(parts)) => match parts.first().map(Vec::as_slice) {
            Some(b"M") => "mph",
            Some(b"N") => "knots",
            _ => "km/h",
        },
        _ => "km/h",
    };
    Some(if value < 10.0 {
        format!("{} {unit}", tidy_numbers(&format!("{value:.1}")))
    } else {
        format!("{value:.0} {unit}")
    })
}

/// How far from where the file says the camera may have been, as the
/// receiver judged it: to a tenth of a meter under ten, and to the meter
/// above.
fn positioning_error(exif: &exif::Exif) -> Option<String> {
    let meters = rational(exif, Tag::GPSHPositioningError)?;
    Some(if meters < 10.0 {
        format!("\u{00b1}{} m", tidy_numbers(&format!("{meters:.1}")))
    } else {
        format!("\u{00b1}{meters:.0} m")
    })
}

/// A tag holding one rational, as a number; `None` for one that holds
/// anything else, or a zero denominator.
fn rational(exif: &exif::Exif, tag: Tag) -> Option<f64> {
    match &primary(exif, tag)?.value {
        Value::Rational(parts) => Some(parts.first()?.to_f64()).filter(|value| value.is_finite()),
        _ => None,
    }
}

/// What the file says in words, under the names those fields are spoken by
/// rather than the ones the standards file them under. A field with several
/// values — the creators, the keywords — is one row, its values parted the
/// way the bars part their segments.
fn described(exif: Option<&exif::Exif>, xmp: &Xmp) -> Vec<Entry> {
    let mut rows = Vec::new();
    for described in &DESCRIBED {
        let from_exif = exif.and_then(|exif| match described.tag? {
            Tag::UserComment => comment(exif),
            tag => primary(exif, tag).map(|field| tidy_numbers(&display(exif, field))),
        });
        let from_xmp = described
            .property
            .and_then(|(namespace, name)| join(xmp.property(namespace, name)?));
        push(&mut rows, described.name, from_exif.or(from_xmp));
    }
    rows
}

/// The subject the camera found, as regions: `SubjectArea`, and
/// `SubjectLocation`, the older tag for the same thing, which holds a point.
/// A location at the middle of the area is the same subject said twice, so
/// it is said once. Both are in the pixels the block's own
/// `PixelXDimension` and `PixelYDimension` count where it has both.
fn subject(exif: &exif::Exif) -> Vec<MetadataRegion> {
    let side = |tag| {
        primary(exif, tag)
            .and_then(|field| field.value.get_uint(0))
            .map(f64::from)
    };
    let of = side(Tag::PixelXDimension)
        .zip(side(Tag::PixelYDimension))
        .map(|(w, h)| [w, h]);
    let values = |tag| -> Option<Vec<u32>> {
        match &primary(exif, tag)?.value {
            Value::Short(values) => Some(values.iter().map(|&value| u32::from(value)).collect()),
            Value::Long(values) => Some(values.clone()),
            _ => None,
        }
    };
    let area =
        values(Tag::SubjectArea).and_then(|values| MetadataRegion::subject_area(&values, of));
    let location = values(Tag::SubjectLocation)
        .filter(|values| values.len() == 2)
        .and_then(|values| MetadataRegion::subject_area(&values, of));
    let center = |region: &Option<MetadataRegion>| {
        region
            .as_ref()
            .and_then(|region| region.shape)
            .map(|shape| shape.center())
    };
    let said = area.is_some() && center(&area) == center(&location);
    area.into_iter().chain(location.filter(|_| !said)).collect()
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
fn coordinates(exif: &exif::Exif) -> Option<(String, String)> {
    let [latitude, longitude] = hemispheres(exif)
        .map(|axis| axis.map(|(degrees, hemisphere)| format!("{degrees:.5}\u{00b0} {hemisphere}")));
    Some((latitude?, longitude?))
}

/// Where the camera was as the two signed numbers a map's address takes:
/// the latitude, south of the equator below zero, and the longitude, west
/// of Greenwich below zero.
fn position(exif: &exif::Exif) -> Option<[f64; 2]> {
    let [latitude, longitude] = hemispheres(exif).map(|axis| {
        let (degrees, hemisphere) = axis?;
        Some(match hemisphere.as_str() {
            "S" | "W" => -degrees,
            _ => degrees,
        })
    });
    Some([latitude?, longitude?])
}

/// The latitude and the longitude, each as degrees and the letter of its
/// hemisphere.
fn hemispheres(exif: &exif::Exif) -> [Option<(f64, String)>; 2] {
    let axis = |value: Tag, reference: Tag| {
        let degrees = degrees(&primary(exif, value)?.value)?;
        let hemisphere = primary(exif, reference)?
            .display_value()
            .to_string()
            .trim_matches('"')
            .to_string();
        Some((degrees, hemisphere))
    };
    [
        axis(Tag::GPSLatitude, Tag::GPSLatitudeRef),
        axis(Tag::GPSLongitude, Tag::GPSLongitudeRef),
    ]
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

/// How high the camera was, to the meter. Below sea level is a real answer and
/// a signed one, which is why the reference tag is asked as well.
fn altitude(exif: &exif::Exif) -> Option<String> {
    let meters = match &primary(exif, Tag::GPSAltitude)?.value {
        Value::Rational(parts) => parts.first()?.to_f64(),
        _ => return None,
    };
    let below = primary(exif, Tag::GPSAltitudeRef)
        .and_then(|field| field.value.get_uint(0))
        .is_some_and(|reference| reference == 1);
    let signed = if below { -meters } else { meters };
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
    // A field of several strings — a lens name padded out with empty
    // ones, a maker's name with a version byte after it — is the strings
    // that read as words, and one alone is written without its quotes.
    if let Value::Ascii(parts) = &field.value
        && parts.len() > 1
    {
        let words: Vec<String> = parts
            .iter()
            .filter_map(|part| {
                let text = std::str::from_utf8(part).ok()?.trim();
                (!text.is_empty() && !text.chars().any(char::is_control)).then(|| text.to_string())
            })
            .collect();
        if let Some(joined) = join(&words) {
            return joined;
        }
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

/// Cuts a rendered value down to something a panel can hold, and takes the
/// control characters out of it. The text is whatever was written into the
/// file: a newline in it would break the column it is laid out in, and a
/// terminating null is not a character to draw. `pub(crate)` for the
/// thumbnail thread, whose title for a chooser row has to come out as the
/// panel's does.
pub(crate) fn shorten(value: &str) -> String {
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

    /// A raw's panel has what the library read beside what the EXIF said:
    /// the camera named once, and the color temperature, in an `Exposure`
    /// section made for it after `Camera`.
    #[test]
    fn a_raw_gets_one_camera_and_its_color_temperature() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_images")
            .join("dng-cfa.dng");
        let exif = Exif::read(&path);
        let names: Vec<Group> = exif.sections.iter().map(|section| section.group).collect();
        assert_eq!(names, [Group::Camera, Group::Exposure], "{exif:?}");
        assert_eq!(
            section(&exif, Group::Exposure),
            [Entry::new(COLOR_TEMPERATURE, "6500 K")]
        );
        assert_eq!(
            section(&exif, Group::Camera)
                .iter()
                .filter(|entry| entry.name == CAMERA)
                .count(),
            1
        );
    }

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
    fn section(exif: &Exif, group: Group) -> &[Entry] {
        match exif.sections.iter().find(|section| section.group == group) {
            Some(section) => &section.entries,
            None => &[],
        }
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
        // SubjectArea: a rectangle about the middle, in the pixels the two
        // after it count.
        exif.pooled(
            0x9214,
            3,
            4,
            &[2000u16, 1500, 800, 600].map(u16::to_le_bytes).concat(),
        );
        exif.short(0xa002, 4000); // PixelXDimension
        exif.short(0xa003, 3000); // PixelYDimension
        // SubjectLocation: the same subject's middle again, which is said
        // once.
        exif.entry(0xa214, 3, 2, [0xd0, 0x07, 0xdc, 0x05]);
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
        gps.ascii(0x000c, "K"); // GPSSpeedRef
        gps.rational(0x000d, &[(1234, 100)]); // GPSSpeed
        gps.ascii(0x0010, "T"); // GPSImgDirectionRef, true north
        gps.rational(0x0011, &[(21180, 100)]); // GPSImgDirection
        gps.ascii(0x0012, "WGS-84"); // GPSMapDatum, said nowhere
        gps.rational(0x001f, &[(47, 10)]); // GPSHPositioningError

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

    /// The regions as rows, at `shown` and with no turn.
    fn regions_of(exif: &Exif, shown: [u32; 2]) -> Vec<String> {
        exif.regions(shown, Turn::NONE)
            .into_iter()
            .map(|region| format!("{}: {}", region.entry.name, region.entry.value))
            .collect()
    }

    /// The panel's top sections: the fields a photograph is read by, combined
    /// into the lines they are read as, in units a reader can use, and under
    /// the headings they are looked for beneath.
    #[test]
    fn a_photograph_is_summarized_as_it_would_be_read() {
        let path = written("photograph.jpg", &jpeg_with(photograph()));
        let exif = Exif::read(&path);
        let _ = std::fs::remove_file(&path);

        let rows = |group: Group| -> Vec<(String, String)> {
            section(&exif, group)
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

        // What took it, and then how: the time it was taken first.
        assert_eq!(
            rows(Group::Camera),
            pairs(&[
                // The maker is not said twice, though the file says it twice.
                ("Camera", "Apple iPhone 16 Pro"),
                ("Lens", "A Lens 6.765mm f/1.78"),
            ])
        );
        assert_eq!(
            rows(Group::Exposure),
            pairs(&[
                ("Taken", "2026-08-27 20:06:17 +12:00"),
                ("Shutter speed", "1/50 s"),
                // 89/50 is exactly 1.78, and is quoted as such.
                ("Aperture", "f/1.78"),
                ("ISO", "200"),
                ("Focal length", "6.765 mm (24 mm equivalent)"),
            ])
        );
        assert_eq!(
            rows(Group::Location),
            pairs(&[
                ("Latitude", "44.68202\u{00b0} S"),
                ("Longitude", "169.16196\u{00b0} E"),
                ("Altitude", "333 m"),
                ("Direction", "212\u{00b0} from true north"),
                ("Speed", "12 km/h"),
                ("Positioning error", "\u{00b1}4.7 m"),
            ])
        );
        // And the same place as the signed numbers a map's address takes:
        // south below zero.
        let [latitude, longitude] = exif.position.expect("a position");
        assert!((latitude + 44.68202).abs() < 1e-5, "{latitude}");
        assert!((longitude - 169.16196).abs() < 1e-5, "{longitude}");
        // Nothing is listed field by field: not the rest of the GPS
        // directory, not the software that wrote the file — the camera's
        // firmware — and not the orientation, which the `Image` section says.
        let names: Vec<Group> = exif.sections.iter().map(|section| section.group).collect();
        assert_eq!(
            names,
            [
                Group::About,
                Group::Camera,
                Group::Exposure,
                Group::Location
            ],
            "{names:?}"
        );
        assert_eq!(exif.orientation, Some(Orientation::Rotate90));
        // The comment is read out of its character code rather than written
        // out as the hex the renderer would make of an undefined type.
        assert_eq!(
            rows(Group::About),
            pairs(&[("Comment", "On a post by the jetty")]),
            "{exif:?}"
        );
        // The subject is a region, and is written out in the picture as
        // shown: stood on its side by the orientation tag, and made smaller.
        assert_eq!(
            regions_of(&exif, [3000, 4000]),
            ["Subject: 600 \u{00d7} 800 at 1200, 1600"]
        );
        assert_eq!(
            regions_of(&exif, [1500, 2000]),
            ["Subject: 300 \u{00d7} 400 at 600, 800"]
        );
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

    /// A packet saying the things EXIF has no tag for, and one it has.
    const PACKET: &[u8] = br#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/">
<dc:title><rdf:Alt><rdf:li xml:lang="x-default">Common Buzzard</rdf:li></rdf:Alt></dc:title>
<dc:subject><rdf:Bag><rdf:li>bird</rdf:li><rdf:li>raptor</rdf:li></rdf:Bag></dc:subject>
<dc:description><rdf:Alt><rdf:li xml:lang="x-default">From the packet</rdf:li></rdf:Alt></dc:description>
</rdf:Description></rdf:RDF></x:xmpmeta>"#;

    fn description(exif: &Exif) -> Vec<(String, String)> {
        section(exif, Group::About)
            .iter()
            .map(|entry| (entry.name.clone(), entry.value.clone()))
            .collect()
    }

    /// A file with an XMP packet and no EXIF block at all — which is what a
    /// file that was only ever given a title is — has an About section and
    /// nothing else, the title first and the keywords as one row.
    #[test]
    fn a_title_is_read_from_a_file_with_no_exif() {
        let mut webp = b"RIFF\0\0\0\0WEBPXMP ".to_vec();
        webp.extend_from_slice(&(PACKET.len() as u32).to_le_bytes());
        webp.extend_from_slice(PACKET);
        let path = written("titled.webp", &webp);
        let exif = Exif::read(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(
            description(&exif),
            [
                ("Title".to_string(), "Common Buzzard".to_string()),
                ("Caption".to_string(), "From the packet".to_string()),
                ("Keywords".to_string(), "bird \u{00b7} raptor".to_string()),
            ]
        );
        assert_eq!(exif.sections.len(), 1, "{exif:?}");
    }

    /// A file saying every field About shows, the four EXIF has a tag for in
    /// both blocks: all six rows, in the table's order, each of the four
    /// read from EXIF and the comment out of UTF-16; and the program that
    /// wrote the file and when, said nowhere.
    #[test]
    fn every_field_about_shows_is_read() {
        let exif = Exif::read(&fixture("jpeg-about.jpg"));
        assert_eq!(
            description(&exif),
            [
                ("Title", "Four Quadrants"),
                ("Caption", "Red, green, blue and white, a quadrant each"),
                ("Comment", "Pattern \u{2014} made by generate.sh"),
                ("Artist", "Test Pattern"),
                (
                    "Keywords",
                    "red \u{00b7} green \u{00b7} blue \u{00b7} white"
                ),
                ("Copyright", "CC0 1.0"),
            ]
            .map(|(name, value)| (name.to_string(), value.to_string()))
        );
        assert_eq!(exif.title(), Some("Four Quadrants"));
        let names: Vec<Group> = exif.sections.iter().map(|section| section.group).collect();
        assert_eq!(names, [Group::About], "{exif:?}");
    }

    /// A file saying none of them has no About section, rather than an
    /// empty one.
    #[test]
    fn a_file_saying_nothing_in_words_has_no_about() {
        let exif = Exif::read(&fixture("jpeg-exif-rotated.jpg"));
        assert!(
            exif.sections
                .iter()
                .all(|section| section.group != Group::About),
            "{exif:?}"
        );
    }

    /// Each region is a row named by its kind, saying who is in it, what was
    /// written about it and where it is, in the pixels of the picture as it
    /// is shown: carried through the orientation tag and the turn in force,
    /// and scaled to the picture's size rather than the one it was marked on.
    #[test]
    fn a_region_says_who_is_in_it_and_where() {
        const PACKET: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about=""
 xmlns:mwg-rs="http://www.metadataworkinggroup.com/schemas/regions/"
 xmlns:stDim="http://ns.adobe.com/xap/1.0/sType/Dimensions#"
 xmlns:stArea="http://ns.adobe.com/xmp/sType/Area#">
<mwg-rs:Regions rdf:parseType="Resource">
<mwg-rs:AppliedToDimensions stDim:w="4000" stDim:h="3000" stDim:unit="pixel"/>
<mwg-rs:RegionList><rdf:Bag>
<rdf:li rdf:parseType="Resource" mwg-rs:Name="Jane Doe" mwg-rs:Type="Face" mwg-rs:Rotation="0.00000">
<mwg-rs:Area stArea:x="0.5" stArea:y="0.25" stArea:w="0.1" stArea:h="0.2"/></rdf:li>
<rdf:li rdf:parseType="Resource" mwg-rs:Type="BarCode" mwg-rs:Description="On the ring" mwg-rs:BarCodeValue="A-1234">
<mwg-rs:Area stArea:x="0.3" stArea:y="0.6" stArea:d="0.05"/></rdf:li>
<rdf:li rdf:parseType="Resource" mwg-rs:Name="Rex" mwg-rs:Rotation="12.5"/>
<rdf:li rdf:parseType="Resource" mwg-rs:Type="Focus" mwg-rs:FocusUsage="EvaluatedUsed">
<mwg-rs:Area stArea:x="0.001" stArea:y="0.5" stArea:unit="normalized"/></rdf:li>
<rdf:li rdf:parseType="Resource">
<mwg-rs:Area stArea:x="2000" stArea:y="750" stArea:unit="pixel"/></rdf:li>
</rdf:Bag></mwg-rs:RegionList></mwg-rs:Regions>
<MP:RegionInfo xmlns:MP="http://ns.microsoft.com/photo/1.2/" rdf:parseType="Resource">
<MPRI:Regions xmlns:MPRI="http://ns.microsoft.com/photo/1.2/t/RegionInfo#"><rdf:Bag>
<rdf:li rdf:parseType="Resource" xmlns:MPReg="http://ns.microsoft.com/photo/1.2/t/Region#">
<MPReg:PersonDisplayName>Jane Doe</MPReg:PersonDisplayName>
<MPReg:Rectangle>0.45, 0.15, 0.1, 0.2</MPReg:Rectangle></rdf:li>
<rdf:li rdf:parseType="Resource" xmlns:MPReg="http://ns.microsoft.com/photo/1.2/t/Region#">
<MPReg:PersonDisplayName>John Doe</MPReg:PersonDisplayName>
<MPReg:Rectangle>0, 0, 0.25, 0.5</MPReg:Rectangle></rdf:li>
</rdf:Bag></MPRI:Regions></MP:RegionInfo>
</rdf:Description></rdf:RDF></x:xmpmeta>"#;
        let xmp = Xmp::parse(PACKET.as_bytes()).expect("the packet parses");
        let mut exif = Exif::assemble(None, &xmp);
        assert!(exif.sections.is_empty(), "{exif:?}");
        let rows = |exif: &Exif, shown: [u32; 2], turn: Turn| -> Vec<(String, String)> {
            exif.regions(shown, turn)
                .into_iter()
                .map(|region| (region.entry.name, region.entry.value))
                .collect()
        };
        let owned = |rows: [(&str, &str); 6]| {
            rows.map(|(name, value)| (name.to_string(), value.to_string()))
        };

        let upright = owned([
            ("Face", "Jane Doe \u{00b7} 400 \u{00d7} 600 at 1800, 450"),
            (
                "Barcode",
                "On the ring \u{00b7} A-1234 \u{00b7} 200 across at 1200, 1800",
            ),
            ("Region", "Rex \u{00b7} rotated 12.5\u{00b0}"),
            ("Focus", "focused on \u{00b7} 4, 1500"),
            ("Region", "2000, 750"),
            // Microsoft's tag for Jane Doe says again what the first row
            // says, and is left out; John Doe's is new.
            ("Person", "John Doe \u{00b7} 1000 \u{00d7} 1500 at 0, 0"),
        ]);
        assert_eq!(rows(&exif, [4000, 3000], Turn::NONE), upright);

        // The same picture made half the size since it was marked.
        assert_eq!(
            rows(&exif, [2000, 1500], Turn::NONE)[0].1,
            "Jane Doe \u{00b7} 200 \u{00d7} 300 at 900, 225"
        );

        // Stood on its side, whether by the tag or by a turn: the top of the
        // picture as stored is down the right.
        let on_its_side = owned([
            ("Face", "Jane Doe \u{00b7} 600 \u{00d7} 400 at 1950, 1800"),
            (
                "Barcode",
                "On the ring \u{00b7} A-1234 \u{00b7} 200 across at 1200, 1200",
            ),
            ("Region", "Rex \u{00b7} rotated 12.5\u{00b0}"),
            ("Focus", "focused on \u{00b7} 1500, 4"),
            ("Region", "2250, 2000"),
            ("Person", "John Doe \u{00b7} 1500 \u{00d7} 1000 at 1500, 0"),
        ]);
        assert_eq!(
            rows(&exif, [3000, 4000], Turn::NONE.clockwise()),
            on_its_side
        );
        exif.orientation = Some(Orientation::Rotate90);
        assert_eq!(rows(&exif, [3000, 4000], Turn::NONE), on_its_side);
        // And turned back by hand.
        assert_eq!(
            rows(&exif, [4000, 3000], Turn::NONE.counterclockwise()),
            upright
        );
    }

    /// A subject location somewhere other than the middle of the subject
    /// area is a second subject, and one on its own is a point; one that is
    /// not two numbers is no shape, and no region.
    #[test]
    fn a_subject_location_is_a_point() {
        let read = |area: Option<[u16; 4]>, location: &[u16]| {
            let mut exif = Block::new();
            if let Some(area) = area {
                exif.pooled(0x9214, 3, 4, &area.map(u16::to_le_bytes).concat());
            }
            let location: Vec<u8> = location.iter().flat_map(|v| v.to_le_bytes()).collect();
            if location.len() <= 4 {
                let mut inline = [0u8; 4];
                inline[..location.len()].copy_from_slice(&location);
                exif.entry(0xa214, 3, (location.len() / 2) as u32, inline);
            } else {
                exif.pooled(0xa214, 3, (location.len() / 2) as u32, &location);
            }
            const HEADER: usize = 8;
            let mut ifd0 = Block::new();
            let exif_at = HEADER + Block::length_of(1, 0);
            ifd0.entry(0x8769, 4, 1, (exif_at as u32).to_le_bytes()); // ExifIFDPointer
            let mut block = b"II\x2a\x00\x08\x00\x00\x00".to_vec();
            block.extend_from_slice(&ifd0.at(HEADER));
            block.extend_from_slice(&exif.at(exif_at));
            let path = written("subject.jpg", &jpeg_with(block));
            let exif = Exif::read(&path);
            let _ = std::fs::remove_file(&path);
            exif
        };
        let exif = read(Some([50, 40, 20, 10]), &[10, 20]);
        assert_eq!(
            regions_of(&exif, [100, 80]),
            ["Subject: 20 \u{00d7} 10 at 40, 35", "Subject: 10, 20"]
        );
        let exif = read(None, &[10, 20]);
        assert_eq!(regions_of(&exif, [100, 80]), ["Subject: 10, 20"]);
        let exif = read(None, &[10, 20, 30]);
        assert!(regions_of(&exif, [100, 80]).is_empty());
    }

    /// Where the block and the packet both describe the picture, the block's
    /// words are the ones shown; the packet fills in what the block has no
    /// field for.
    #[test]
    fn the_exif_field_stands_where_both_speak() {
        let mut ifd0 = Block::new();
        ifd0.ascii(0x010e, "From the block"); // ImageDescription
        let mut block = b"II\x2a\x00\x08\x00\x00\x00".to_vec();
        block.extend_from_slice(&ifd0.at(8));

        let mut jpeg = b"\xff\xd8".to_vec();
        let mut exif_payload = b"Exif\0\0".to_vec();
        exif_payload.extend_from_slice(&block);
        let mut xmp_payload = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
        xmp_payload.extend_from_slice(PACKET);
        for payload in [exif_payload, xmp_payload] {
            jpeg.extend_from_slice(b"\xff\xe1");
            jpeg.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
            jpeg.extend_from_slice(&payload);
        }
        jpeg.extend_from_slice(b"\xff\xd9");
        let path = written("both.jpg", &jpeg);
        let exif = Exif::read(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(
            description(&exif),
            [
                ("Title".to_string(), "Common Buzzard".to_string()),
                ("Caption".to_string(), "From the block".to_string()),
                ("Keywords".to_string(), "bird \u{00b7} raptor".to_string()),
            ]
        );
    }

    /// A TIFF keeps its packet in its own directory, so it is read out of
    /// the block rather than found in the file, and the directory's other
    /// tags are not listed.
    #[test]
    fn a_tiffs_packet_is_read_out_of_its_directory() {
        let mut ifd0 = Block::new();
        ifd0.short(0x0100, 32); // ImageWidth
        ifd0.pooled(700, 1, PACKET.len() as u32, PACKET); // XMP, as bytes
        let mut tiff = b"II\x2a\x00\x08\x00\x00\x00".to_vec();
        tiff.extend_from_slice(&ifd0.at(8));
        let path = written("titled.tif", &tiff);
        let exif = Exif::read(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(
            description(&exif).first(),
            Some(&("Title".to_string(), "Common Buzzard".to_string())),
            "{exif:?}"
        );
        let names: Vec<Group> = exif.sections.iter().map(|section| section.group).collect();
        assert_eq!(names, [Group::About], "{exif:?}");
    }

    /// A block of `ifd0` and an Exif directory of `exif`, one after the
    /// other.
    fn with_exif(mut ifd0: Block, exif: Block) -> Vec<u8> {
        const HEADER: usize = 8;
        let first = Block::length_of(ifd0.entries.len() + 1, ifd0.pool.len());
        ifd0.entry(0x8769, 4, 1, ((HEADER + first) as u32).to_le_bytes()); // ExifIFDPointer
        let mut block = b"II\x2a\x00\x08\x00\x00\x00".to_vec();
        block.extend_from_slice(&ifd0.at(HEADER));
        block.extend_from_slice(&exif.at(HEADER + first));
        block
    }

    /// The rows of `Camera`, then of `Exposure`.
    fn camera_rows(ifd0: Block, exif: Block) -> Vec<(String, String)> {
        let path = written("settings.jpg", &jpeg_with(with_exif(ifd0, exif)));
        let exif = Exif::read(&path);
        let _ = std::fs::remove_file(&path);
        section(&exif, Group::Camera)
            .iter()
            .chain(section(&exif, Group::Exposure))
            .map(|entry| (entry.name.clone(), entry.value.clone()))
            .collect()
    }

    fn owned(rows: &[(&str, &str)]) -> Vec<(String, String)> {
        rows.iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect()
    }

    /// How the exposure was decided and what else the camera was set to,
    /// in words; a lens by another maker than the camera's named with its
    /// maker; and whose camera it was, the padding taken off.
    #[test]
    fn a_cameras_settings_are_read_as_words() {
        let mut ifd0 = Block::new();
        ifd0.ascii(0x010f, "Sony"); // Make
        ifd0.ascii(0x0110, "ILCE-7M4"); // Model
        let mut exif = Block::new();
        exif.short(0x8822, 3); // ExposureProgram, aperture priority
        exif.rational(0x920a, &[(50, 1)]); // FocalLength
        exif.short(0x9207, 5); // MeteringMode, pattern
        // Flash: fired, on auto, with red-eye reduction.
        exif.short(0x9209, 0x59);
        exif.ascii(0xa430, "Test Owner      "); // CameraOwnerName
        exif.ascii(0xa431, "4321"); // BodySerialNumber
        exif.ascii(0xa433, "Sigma"); // LensMake
        exif.ascii(0xa434, "24-70mm F2.8 DG DN | Art"); // LensModel
        exif.ascii(0xa435, "8765"); // LensSerialNumber
        exif.short(0xa402, 2); // ExposureMode, auto bracket
        exif.short(0xa403, 1); // WhiteBalance, manual
        exif.rational(0xa404, &[(2, 1)]); // DigitalZoomRatio
        exif.short(0xa460, 3); // CompositeImage, made while shooting
        assert_eq!(
            camera_rows(ifd0, exif),
            owned(&[
                ("Camera", "Sony ILCE-7M4"),
                ("Lens", "Sigma 24-70mm F2.8 DG DN | Art"),
                ("Owner", "Test Owner"),
                ("Camera serial number", "4321"),
                ("Lens serial number", "8765"),
                ("Mode", "Aperture priority \u{00b7} Auto bracket"),
                ("Focal length", "50 mm \u{00b7} 2\u{00d7} digital zoom"),
                ("Metering", "Evaluative"),
                ("White balance", "Manual"),
                ("Flash", "Fired \u{00b7} auto \u{00b7} red-eye reduction"),
                ("Composite", "Merged from several frames as it was taken"),
            ])
        );
    }

    /// The maker is said once: where the model starts with it, or with the
    /// first word of a make that is a company's whole name.
    #[test]
    fn a_camera_is_named_once() {
        let name = |make: &str, model: &str| {
            camera_name(Some(make.to_string()), Some(model.to_string())).unwrap()
        };
        assert_eq!(name("Canon", "Canon EOS R6m2"), "Canon EOS R6m2");
        assert_eq!(name("NIKON CORPORATION", "NIKON D100"), "NIKON D100");
        assert_eq!(name("Sony", "ILCE-7M4"), "Sony ILCE-7M4");
        assert_eq!(name("FUJIFILM", "X-T5"), "FUJIFILM X-T5");
    }

    /// A lens the file gives no name for is its range, a zoom's apertures
    /// at each end; a lens by the camera's own maker is not named twice; a
    /// camera with no flash has no flash row; and a zoom of one is none.
    #[test]
    fn a_lens_with_no_name_is_its_range() {
        let mut ifd0 = Block::new();
        ifd0.ascii(0x010f, "Canon"); // Make
        let mut exif = Block::new();
        exif.short(0x9209, 0x20); // Flash, no flash function
        exif.rational(0xa404, &[(1, 1)]); // DigitalZoomRatio
        exif.rational(0xa432, &[(18, 1), (55, 1), (35, 10), (56, 10)]); // LensSpecification
        exif.ascii(0xa433, "Canon"); // LensMake
        assert_eq!(
            camera_rows(ifd0, exif),
            owned(&[
                ("Camera", "Canon"),
                ("Lens", "18\u{2013}55 mm f/3.5\u{2013}5.6")
            ])
        );

        let mut ifd0 = Block::new();
        ifd0.ascii(0x010f, "Canon"); // Make
        let mut exif = Block::new();
        exif.short(0x9209, 0x10); // Flash, off and did not fire
        exif.ascii(0xa433, "Canon"); // LensMake
        exif.ascii(0xa434, "RF50mm F1.8 STM"); // LensModel
        assert_eq!(
            camera_rows(ifd0, exif),
            owned(&[
                ("Camera", "Canon"),
                ("Lens", "RF50mm F1.8 STM"),
                ("Flash", "Did not fire \u{00b7} off"),
            ])
        );
    }

    /// A TIFF's compression is said, for the `Image` section; a raw's, which
    /// is its preview's, is not.
    #[test]
    fn a_tiffs_compression_is_said() {
        let exif = Exif::read(&fixture("tiff-lzw.tif"));
        assert_eq!(exif.compression.as_deref(), Some("LZW"));
        assert_eq!(Exif::read(&fixture("dng-cfa.dng")).compression, None);
    }

    /// A real file, read through the container it arrives in: the fixture
    /// carries an orientation and nothing else, which the `Image` section
    /// says.
    #[test]
    fn a_files_own_block_is_found_through_its_container() {
        let exif = Exif::read(&fixture("webp-exif-rotated.webp"));
        assert_eq!(exif.orientation, Some(Orientation::Rotate180), "{exif:?}");
        assert!(exif.sections.is_empty(), "{exif:?}");
    }

    /// A BigTIFF has to go the long way round — the reader knows the
    /// original format only — and comes back saying what the file it is a
    /// bigger version of says.
    #[test]
    fn a_bigtiff_is_read_through_its_directory() {
        let big = Exif::read(&fixture("tiff-bigtiff.tif"));
        // The same image in the ordinary form: both fixtures are the 32x24
        // float raster, one written each way.
        let ordinary = Exif::read(&fixture("tiff-nodata.tif"));
        assert!(big.compression.is_some(), "{big:?}");
        assert_eq!(big.compression, ordinary.compression);
    }

    /// A measurement raster, read out of a real TIFF: the value that stands
    /// for nothing measured belongs to the georeference, and the rest of its
    /// directory is not listed.
    #[test]
    fn a_rasters_own_facts_are_its_georeference() {
        let exif = Exif::read(&fixture("tiff-nodata.tif"));
        assert_eq!(
            exif.sections,
            [Section {
                group: Group::Georeference,
                entries: vec![Entry::new("No data", "-9999")],
            }],
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
        let bounded = Exif::read_with(&path, directory as u64);
        let whole = Exif::read_with(&path, u64::MAX);
        let _ = std::fs::remove_file(&path);
        assert_eq!(bounded.sections, whole.sections);
        assert!(!bounded.sections.is_empty());

        // And a prefix that stops short of it is a file with nothing to say,
        // rather than an error anything upstream has to handle.
        assert!(empty(&Exif::read_with(Path::new("/nonexistent.tif"), 8)));
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
