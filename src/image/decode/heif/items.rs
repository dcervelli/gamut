//! A HEIF's item tables, read by walking the boxes: what is in the file
//! beside the coded pictures, and what is said of each picture.
//!
//! Everything a HEIF says about its images is in the top-level `meta` box,
//! ahead of the pixels in `mdat`: `pitm` for which item is the picture,
//! `iinf` for what each item is, `iref` for how the items refer to one
//! another, `iloc` for where each item's payload is, `idat` for the small
//! payloads kept in `meta` itself, and `iprp` for the properties — a list of
//! them in `ipco`, and in `ipma` which belong to which item. The walk reads
//! that structure alone, as `libheif` does before it decodes anything,
//! seeking past the boxes around it.
//!
//! [`super::tmap`] finds the ISO 21496-1 item through it. A Mac reads the
//! pixels through ImageIO, which says nothing of the rest, so it reads the
//! rest here: the picture's size once cropped and turned, its color, its
//! channels, the EXIF and XMP payloads the picture carries, and the XMP of
//! its depth image.

// Everywhere but a Mac, only the `tmap` walk reads these tables.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::collections::HashMap;
use std::io::SeekFrom;

use anyhow::{Context, Result, anyhow, bail};

use crate::image::color::cicp::UNSPECIFIED;
use crate::image::decode::ReadSeek;
use crate::image::isobmff::{self, Fields};

/// The largest `meta` box that will be read: the item tables of a phone's
/// photograph run to a few tens of kilobytes, and one claiming more than
/// this is not a picture's.
const MAX_META: u64 = 16 << 20;

/// What `iinf` says of one item.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Item {
    pub id: u32,
    pub kind: [u8; 4],
    /// The MIME type of a `mime` item's payload: `application/rdf+xml` for
    /// an XMP packet.
    pub content_type: Option<String>,
}

/// Where an item's payload is: which of the three ways the offsets are
/// meant, and the extents.
#[derive(Debug, Default)]
struct Location {
    /// 0 for file offsets, 1 for offsets into `idat`, 2 for offsets into
    /// another item.
    construction: u8,
    extents: Vec<(u64, u64)>,
}

/// The items an item refers to, by the reference's type.
type References = HashMap<(u32, [u8; 4]), Vec<u32>>;

/// What an item's color property says: CICP codes, or an ICC profile.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Color {
    /// `nclx`: the primaries and the transfer function, as code points.
    Nclx { primaries: u8, transfer: u8 },
    /// `prof` or `rICC`: the profile itself.
    Icc(Vec<u8>),
}

/// One of the properties `ipco` lists, as far as anything here reads them.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Property {
    /// `ispe`: the size the coded picture is, before any crop or turn.
    Size {
        width: u32,
        height: u32,
    },
    /// `clap`: the crop, as the size it leaves.
    Crop {
        width: f64,
        height: f64,
    },
    /// `irot`: quarter turns counterclockwise.
    Rotation(u8),
    Color(Color),
    /// `pixi`: the bits of each channel, one entry a channel.
    Channels(Vec<u8>),
    /// `auxC`: what an auxiliary image is, as a URN.
    Auxiliary(String),
    Other,
}

/// What the item tables say.
#[derive(Debug, Default)]
pub(super) struct Meta {
    /// `meta`'s body, which `idat`'s range is of.
    body: Vec<u8>,
    /// The picture's item id, from `pitm`.
    pub primary: Option<u32>,
    pub items: Vec<Item>,
    references: References,
    locations: HashMap<u32, Location>,
    /// The `idat` box's body, as a range of `body`.
    idat: Option<(usize, usize)>,
    /// `ipco`'s properties, in order: `ipma`'s indices count from one.
    properties: Vec<Property>,
    /// Each item's properties, as indices into `properties`, in the order
    /// `ipma` lists them, which is the order the transformative ones apply.
    associations: HashMap<u32, Vec<usize>>,
}

impl Meta {
    /// The tables of the file `source` holds. `None` for a file with no
    /// `meta` before its `mdat`, or one whose tables do not add up.
    pub fn read(source: &mut dyn ReadSeek) -> Result<Option<Self>> {
        // Past any `mdat` ahead of it: an iPhone on iOS 17 writes a small
        // one before `meta`, so the walk seeks past the boxes it does not
        // want rather than stopping at the pixels.
        let Some(body) = isobmff::top_level(source, b"meta", None, MAX_META)? else {
            return Ok(None);
        };
        Ok(Self::parse(body))
    }

    fn parse(body: Vec<u8>) -> Option<Self> {
        let mut meta = Self::default();
        // `meta` is a full box: version and flags before the children.
        for (kind, start, end) in isobmff::boxes(&body, 4, body.len()) {
            let child = &body[start..end];
            match &kind {
                b"pitm" => {
                    let mut fields = Fields::new(child);
                    let version = fields.u8()?;
                    fields.skip(3);
                    meta.primary = Some(fields.id(version, 1)?);
                }
                b"iinf" => meta.items = iinf(child)?,
                b"iref" => meta.references = iref(child)?,
                b"iloc" => meta.locations = iloc(child)?,
                b"idat" => meta.idat = Some((start, end)),
                b"iprp" => (meta.properties, meta.associations) = iprp(child)?,
                _ => {}
            }
        }
        meta.body = body;
        Some(meta)
    }

    /// The items `from` refers to by a reference of type `kind`, in order.
    pub fn references(&self, from: u32, kind: &[u8; 4]) -> &[u32] {
        self.references
            .get(&(from, *kind))
            .map_or(&[], Vec::as_slice)
    }

    /// The items that refer to `to` by a reference of type `kind`: the
    /// metadata that describes it (`cdsc`), the auxiliary images that go
    /// with it (`auxl`).
    pub fn referring(&self, to: u32, kind: &[u8; 4]) -> Vec<u32> {
        let mut found: Vec<u32> = self
            .references
            .iter()
            .filter(|((_, k), targets)| k == kind && targets.contains(&to))
            .map(|((from, _), _)| *from)
            .collect();
        found.sort_unstable();
        found
    }

    pub fn item(&self, id: u32) -> Option<&Item> {
        self.items.iter().find(|item| item.id == id)
    }

    /// The properties of an item, in the order `ipma` lists them.
    pub fn properties(&self, id: u32) -> impl Iterator<Item = &Property> {
        self.associations
            .get(&id)
            .into_iter()
            .flatten()
            .filter_map(|&index| self.properties.get(index))
    }

    /// The properties of an item, or where it has none of the kind asked
    /// for, of the first image it is derived from: a grid's tiles carry
    /// the color and channels that its own entry may leave out, and
    /// `libheif` reads them there too.
    pub fn property<T>(&self, id: u32, pick: impl Fn(&Property) -> Option<T>) -> Option<T> {
        self.properties(id).find_map(&pick).or_else(|| {
            self.references(id, b"dimg")
                .first()
                .and_then(|&tile| self.properties(tile).find_map(&pick))
        })
    }

    /// The size the item is shown at: its coded size, cropped by `clap` and
    /// turned by `irot` in the order they apply. `None` without an `ispe`.
    pub fn shown_size(&self, id: u32) -> Option<(u32, u32)> {
        let (mut width, mut height) = self.properties(id).find_map(|p| match p {
            Property::Size { width, height } => Some((*width, *height)),
            _ => None,
        })?;
        for property in self.properties(id) {
            match property {
                Property::Crop {
                    width: w,
                    height: h,
                } => {
                    // A crop larger than the picture, or of nothing, is
                    // not one.
                    let (w, h) = (w.round(), h.round());
                    if w >= 1.0 && h >= 1.0 && w <= f64::from(width) && h <= f64::from(height) {
                        (width, height) = (w as u32, h as u32);
                    }
                }
                Property::Rotation(quarters) if quarters % 2 == 1 => {
                    (width, height) = (height, width);
                }
                _ => {}
            }
        }
        Some((width, height))
    }

    /// The color the item states, nclx before a profile where it states
    /// both, as `libheif` reads it.
    pub fn color(&self, id: u32) -> (Option<(u8, u8)>, Option<Vec<u8>>) {
        let nclx = self.property(id, |p| match p {
            Property::Color(Color::Nclx {
                primaries,
                transfer,
            }) => Some((*primaries, *transfer)),
            _ => None,
        });
        let icc = self.property(id, |p| match p {
            Property::Color(Color::Icc(profile)) => Some(profile.clone()),
            _ => None,
        });
        (nclx, icc)
    }

    /// The first item of `kind` that describes `id` (by `cdsc`), and where
    /// `content_type` is given, whose payload is of that type.
    pub fn describing(&self, id: u32, kind: &[u8; 4], content_type: Option<&str>) -> Option<u32> {
        self.referring(id, b"cdsc").into_iter().find(|&from| {
            self.item(from).is_some_and(|item| {
                &item.kind == kind
                    && content_type.is_none_or(|want| item.content_type.as_deref() == Some(want))
            })
        })
    }

    /// The auxiliary image of `id` whose `auxC` names one of `types`.
    pub fn auxiliary(&self, id: u32, types: &[&str]) -> Option<u32> {
        self.referring(id, b"auxl").into_iter().find(|&aux| {
            self.properties(aux).any(|p| match p {
                Property::Auxiliary(urn) => types.contains(&urn.as_str()),
                _ => false,
            })
        })
    }

    /// An item's payload, gathered from its extents. `None` where it is
    /// kept somewhere this does not follow — in another item — and an error
    /// where it would run past `max`, which is refused before it is read.
    pub fn payload(
        &self,
        source: &mut dyn ReadSeek,
        id: u32,
        max: usize,
    ) -> Result<Option<Vec<u8>>> {
        let Some(location) = self.locations.get(&id) else {
            return Ok(None);
        };
        let mut payload = Vec::new();
        for &(offset, length) in &location.extents {
            let length = usize::try_from(length).map_err(|_| anyhow!("item {id} too long"))?;
            let total = payload.len().saturating_add(length);
            if total > max {
                bail!("item {id} claims {total} bytes");
            }
            match location.construction {
                // In the file itself.
                0 => {
                    source.seek(SeekFrom::Start(offset))?;
                    let mut extent = vec![0; length];
                    source
                        .read_exact(&mut extent)
                        .with_context(|| format!("reading item {id}"))?;
                    payload.extend_from_slice(&extent);
                }
                // In `meta`'s own `idat`, where the offsets are from its start.
                1 => {
                    let Some(idat) = self.idat else {
                        return Ok(None);
                    };
                    let from = usize::try_from(offset)
                        .ok()
                        .and_then(|o| o.checked_add(idat.0));
                    let Some(extent) = from.and_then(|from| {
                        let to = from.checked_add(length)?;
                        (to <= idat.1).then(|| self.body.get(from..to)).flatten()
                    }) else {
                        return Ok(None);
                    };
                    payload.extend_from_slice(extent);
                }
                _ => return Ok(None),
            }
        }
        Ok(Some(payload))
    }
}

/// A NUL-terminated string at the head of `fields`, and past it.
fn string(fields: &mut Fields) -> Option<String> {
    let mut out = Vec::new();
    loop {
        match fields.u8() {
            Some(0) | None => break,
            Some(byte) => out.push(byte),
        }
    }
    String::from_utf8(out).ok()
}

/// `iinf`: the items, from the `infe` boxes inside.
fn iinf(body: &[u8]) -> Option<Vec<Item>> {
    let mut fields = Fields::new(body);
    let version = fields.u8()?;
    fields.skip(3);
    let count = if version == 0 {
        fields.u16()? as usize
    } else {
        fields.u32()? as usize
    };
    let mut items = Vec::with_capacity(count.min(1024));
    for (kind, start, end) in isobmff::boxes(body, fields.at(), body.len()) {
        if &kind != b"infe" {
            continue;
        }
        let mut fields = Fields::new(&body[start..end]);
        let version = fields.u8()?;
        fields.skip(3);
        // Versions 0 and 1 name no type, and no item of theirs is one
        // anything here reads.
        if version < 2 {
            continue;
        }
        let id = fields.id(version, 3)?;
        fields.skip(2); // protection index
        let kind: [u8; 4] = fields.bytes(4)?.try_into().ok()?;
        let _name = string(&mut fields);
        let content_type = (&kind == b"mime").then(|| string(&mut fields)).flatten();
        items.push(Item {
            id,
            kind,
            content_type,
        });
    }
    Some(items)
}

/// `iref`: for each item, the items it refers to, by the reference's type.
fn iref(body: &[u8]) -> Option<References> {
    let mut references: References = HashMap::new();
    let version = *body.first()?;
    for (kind, start, end) in isobmff::boxes(body, 4, body.len()) {
        let mut fields = Fields::new(&body[start..end]);
        let from = fields.id(version, 1)?;
        let count = fields.u16()?;
        let mut to = Vec::with_capacity(usize::from(count).min(1024));
        for _ in 0..count {
            to.push(fields.id(version, 1)?);
        }
        references.entry((from, kind)).or_default().extend(to);
    }
    Some(references)
}

/// `iloc`: where each item's payload is.
fn iloc(body: &[u8]) -> Option<HashMap<u32, Location>> {
    let mut fields = Fields::new(body);
    let version = fields.u8()?;
    fields.skip(3);
    let sizes = fields.u16()?;
    let offset_size = (sizes >> 12) as u8;
    let length_size = ((sizes >> 8) & 15) as u8;
    let base_offset_size = ((sizes >> 4) & 15) as u8;
    let index_size = if version >= 1 { (sizes & 15) as u8 } else { 0 };
    let count = if version < 2 {
        fields.u16()? as usize
    } else {
        fields.u32()? as usize
    };
    let mut locations = HashMap::new();
    for _ in 0..count {
        let id = fields.id(version, 2)?;
        let construction = if version >= 1 {
            (fields.u16()? & 15) as u8
        } else {
            0
        };
        fields.skip(2); // data reference index
        let base = fields.sized(base_offset_size)?;
        let extents = fields.u16()?;
        let mut location = Location {
            construction,
            extents: Vec::with_capacity(usize::from(extents).min(1024)),
        };
        for _ in 0..extents {
            let _index = fields.sized(index_size)?;
            let offset = fields.sized(offset_size)?;
            let length = fields.sized(length_size)?;
            location.extents.push((base.checked_add(offset)?, length));
        }
        locations.insert(id, location);
    }
    Some(locations)
}

/// `iprp`: the properties `ipco` lists, and `ipma`'s associations of them
/// to the items, as indices into the list.
type Properties = (Vec<Property>, HashMap<u32, Vec<usize>>);

fn iprp(body: &[u8]) -> Option<Properties> {
    let mut properties = Vec::new();
    let mut associations: HashMap<u32, Vec<usize>> = HashMap::new();
    for (kind, start, end) in isobmff::boxes(body, 0, body.len()) {
        let child = &body[start..end];
        match &kind {
            b"ipco" => {
                properties = isobmff::boxes(child, 0, child.len())
                    .into_iter()
                    .map(|(kind, start, end)| property(&kind, &child[start..end]))
                    .collect();
            }
            b"ipma" => {
                let mut fields = Fields::new(child);
                let version = fields.u8()?;
                let flags = fields.bytes(3)?[2];
                let count = fields.u32()?;
                for _ in 0..count {
                    let id = fields.id(version, 1)?;
                    let n = fields.u8()?;
                    let list = associations.entry(id).or_default();
                    for _ in 0..n {
                        // The top bit says whether the property is
                        // essential; the rest is its index, from one.
                        let index = if flags & 1 != 0 {
                            usize::from(fields.u16()? & 0x7fff)
                        } else {
                            usize::from(fields.u8()? & 0x7f)
                        };
                        if index > 0 {
                            list.push(index - 1);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Some((properties, associations))
}

fn property(kind: &[u8; 4], body: &[u8]) -> Property {
    let mut fields = Fields::new(body);
    let mut read = || -> Option<Property> {
        match kind {
            b"ispe" => {
                fields.skip(4);
                Some(Property::Size {
                    width: fields.u32()?,
                    height: fields.u32()?,
                })
            }
            b"clap" => {
                let mut ratio = || -> Option<f64> {
                    let numerator = f64::from(fields.u32()?);
                    let denominator = f64::from(fields.u32()?);
                    (denominator != 0.0).then(|| numerator / denominator)
                };
                Some(Property::Crop {
                    width: ratio()?,
                    height: ratio()?,
                })
            }
            b"irot" => Some(Property::Rotation(fields.u8()? & 3)),
            b"colr" => match fields.bytes(4)? {
                // A code past what H.273 defines says nothing, as
                // `libheif`'s reader takes it.
                b"nclx" => Some(Property::Color(Color::Nclx {
                    primaries: u8::try_from(fields.u16()?).unwrap_or(UNSPECIFIED),
                    transfer: u8::try_from(fields.u16()?).unwrap_or(UNSPECIFIED),
                })),
                b"prof" | b"rICC" => Some(Property::Color(Color::Icc(body.get(4..)?.to_vec()))),
                _ => None,
            },
            b"pixi" => {
                fields.skip(4);
                let count = fields.u8()?;
                Some(Property::Channels(
                    fields.bytes(usize::from(count))?.to_vec(),
                ))
            }
            b"auxC" => {
                fields.skip(4);
                Some(Property::Auxiliary(string(&mut fields)?))
            }
            _ => None,
        }
    };
    read().unwrap_or(Property::Other)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_box(kind: &[u8; 4], version: u8, body: &[u8]) -> Vec<u8> {
        plain_box(kind, &[&[version, 0, 0, 0][..], body].concat())
    }

    fn plain_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out
    }

    /// The fixtures' own tables, read here and by `libheif` alike: the
    /// size once turned, and the color each states.
    #[test]
    fn the_fixtures_tables_say_what_libheif_reads() {
        let read = |name: &str| {
            let mut file = std::io::BufReader::new(
                std::fs::File::open(format!("test_images/{name}")).unwrap(),
            );
            Meta::read(&mut file).unwrap().expect("the tables")
        };
        let rotated = read("heic-rotated.heic");
        let primary = rotated.primary.expect("a primary item");
        assert_eq!(rotated.shown_size(primary), Some((32, 24)));
        assert!(
            rotated
                .properties(primary)
                .any(|p| *p == Property::Rotation(2))
        );

        let pq = read("heic-pq10.heic");
        assert_eq!(pq.color(pq.primary.unwrap()).0, Some((9, 16)));

        let icc = read("heic-icc-p3.heic");
        let (nclx, profile) = icc.color(icc.primary.unwrap());
        assert_eq!(nclx, None);
        assert!(profile.is_some_and(|p| p.len() > 128));

        let gray = read("heic-gray8.heic");
        let channels = gray.property(gray.primary.unwrap(), |p| match p {
            Property::Channels(bits) => Some(bits.clone()),
            _ => None,
        });
        assert_eq!(channels, Some(vec![8]));
    }

    /// A crop and a turn apply in the order `ipma` lists them, and a mime
    /// item that describes the picture is found by its content type.
    #[test]
    fn the_shown_size_and_the_describing_item_come_from_the_tables() {
        let infe = |id: u16, kind: &[u8; 4], extra: &[u8]| {
            full_box(
                b"infe",
                2,
                &[&id.to_be_bytes()[..], &[0, 0], kind, b"\0", extra].concat(),
            )
        };
        let iinf = full_box(
            b"iinf",
            0,
            &[
                &2u16.to_be_bytes()[..],
                &infe(1, b"hvc1", b""),
                &infe(2, b"mime", b"application/rdf+xml\0"),
            ]
            .concat(),
        );
        let pitm = full_box(b"pitm", 0, &1u16.to_be_bytes());
        let iref = full_box(
            b"iref",
            0,
            &plain_box(
                b"cdsc",
                &[
                    &2u16.to_be_bytes()[..],
                    &1u16.to_be_bytes(),
                    &1u16.to_be_bytes(),
                ]
                .concat(),
            ),
        );
        let ispe = full_box(
            b"ispe",
            0,
            &[&64u32.to_be_bytes()[..], &48u32.to_be_bytes()].concat(),
        );
        let clap = plain_box(
            b"clap",
            &[
                &40u32.to_be_bytes()[..],
                &1u32.to_be_bytes(),
                &30u32.to_be_bytes(),
                &1u32.to_be_bytes(),
                &[0; 16],
            ]
            .concat(),
        );
        let irot = plain_box(b"irot", &[1]);
        let ipco = plain_box(b"ipco", &[ispe, clap, irot].concat());
        let ipma = full_box(
            b"ipma",
            0,
            &[&1u32.to_be_bytes()[..], &1u16.to_be_bytes(), &[3, 1, 2, 3]].concat(),
        );
        let iprp = plain_box(b"iprp", &[ipco, ipma].concat());
        let body = [&[0u8, 0, 0, 0][..], &pitm, &iinf, &iref, &iprp].concat();
        let meta = Meta::parse(body).expect("the tables parse");
        assert_eq!(meta.primary, Some(1));
        assert_eq!(meta.shown_size(1), Some((30, 40)));
        assert_eq!(
            meta.describing(1, b"mime", Some("application/rdf+xml")),
            Some(2)
        );
        assert_eq!(meta.describing(1, b"Exif", None), None);
    }
}
