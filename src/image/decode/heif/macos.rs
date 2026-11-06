//! HEIF on a Mac, through ImageIO: the system's own decoder, with Apple's
//! codecs, so that nothing of `libheif`'s — the library, its HEVC and AV1
//! codecs and their licenses — has to travel inside the bundle.
//!
//! ImageIO hands over the pixels and nothing it says about them is taken on
//! its word where the file says it plainly: the size, the color and the
//! channels are read from the item tables by [`super::items`], as are the
//! EXIF and XMP payloads and the gain map's ISO metadata ([`super::tmap`]).
//! The pixels are what ImageIO is for, and the shapes it gives them in are
//! undone here:
//!
//! - the picture as stored, with the turn the container asks for beside it
//!   as an EXIF orientation, which [`orient::apply`] then makes — `libheif`
//!   turns while decoding, and the two arrive at the same picture;
//! - eight-bit components four to a pixel, with the alpha's place, its
//!   premultiplication and the byte order in the bitmap info;
//! - a ten-bit picture as three ten-bit components packed into a 32-bit
//!   word, lifted here to sixteen bits as `libheif`'s are;
//! - a gray picture as RGB, which goes back to one channel where `pixi`
//!   says the file holds one;
//! - a depth map as disparity already in the units its XMP gives, which
//!   goes back to the codes the file stored where the XMP says how, so the
//!   readout reads them as it does `libheif`'s.
//!
//! ImageIO decodes some files to nothing while reporting a complete image:
//! a gray picture with an alpha plane, and a few others (see
//! `docs/formats.md`). That arrives as a buffer it will not hand over, and
//! is refused in so many words rather than shown as a blank picture.

use std::ffi::{CStr, c_char, c_void};
use std::sync::Arc;

use anyhow::{Result, anyhow, bail};

use super::items::{Meta, Property};
use super::{EXTENSIONS, MAX_PIXELS, apple, brand, brands, tmap};
use crate::image::color::{Primaries, Transfer};
use crate::image::decode::{self as decode, ReadSeek};
use crate::image::depth::{self, DepthMap};
use crate::image::gain_map::{Data, GainMap, Lift};
use crate::image::orient;
use crate::image::xmp::Xmp;
use crate::image::{AlphaMode, Channels, ColorSpace, DecodedImage, Samples};

type Ref = *const c_void;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFTypeDictionaryKeyCallBacks: c_void;
    static kCFTypeDictionaryValueCallBacks: c_void;
    static kCFBooleanTrue: Ref;
    fn CFRelease(object: Ref);
    fn CFDataCreate(allocator: Ref, bytes: *const u8, length: isize) -> Ref;
    fn CFDataGetBytePtr(data: Ref) -> *const u8;
    fn CFDataGetLength(data: Ref) -> isize;
    fn CFDictionaryCreate(
        allocator: Ref,
        keys: *const Ref,
        values: *const Ref,
        count: isize,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> Ref;
    fn CFDictionaryGetValue(dictionary: Ref, key: Ref) -> Ref;
    fn CFNumberGetValue(number: Ref, kind: isize, value: *mut c_void) -> u8;
    fn CFStringCreateWithCString(allocator: Ref, string: *const c_char, encoding: u32) -> Ref;
    fn CFStringGetCString(string: Ref, buffer: *mut c_char, size: isize, encoding: u32) -> u8;
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGImageGetWidth(image: Ref) -> usize;
    fn CGImageGetHeight(image: Ref) -> usize;
    fn CGImageGetBitsPerComponent(image: Ref) -> usize;
    fn CGImageGetBitsPerPixel(image: Ref) -> usize;
    fn CGImageGetBytesPerRow(image: Ref) -> usize;
    fn CGImageGetBitmapInfo(image: Ref) -> u32;
    fn CGImageGetColorSpace(image: Ref) -> Ref;
    fn CGImageGetDataProvider(image: Ref) -> Ref;
    fn CGDataProviderCopyData(provider: Ref) -> Ref;
    fn CGColorSpaceGetNumberOfComponents(space: Ref) -> usize;
    fn CGColorSpaceCopyICCData(space: Ref) -> Ref;
    fn CGColorSpaceCopyName(space: Ref) -> Ref;
}

#[link(name = "ImageIO", kind = "framework")]
unsafe extern "C" {
    static kCGImageSourceShouldCacheImmediately: Ref;
    static kCGImagePropertyOrientation: Ref;
    static kCGImagePropertyPixelWidth: Ref;
    static kCGImagePropertyPixelHeight: Ref;
    static kCGImagePropertyDepth: Ref;
    static kCGImageAuxiliaryDataTypeHDRGainMap: Ref;
    static kCGImageAuxiliaryDataTypeISOGainMap: Ref;
    static kCGImageAuxiliaryDataTypeDepth: Ref;
    static kCGImageAuxiliaryDataTypeDisparity: Ref;
    static kCGImageAuxiliaryDataInfoData: Ref;
    static kCGImageAuxiliaryDataInfoDataDescription: Ref;
    fn CGImageSourceCreateWithData(data: Ref, options: Ref) -> Ref;
    fn CGImageSourceGetPrimaryImageIndex(source: Ref) -> usize;
    fn CGImageSourceCopyPropertiesAtIndex(source: Ref, index: usize, options: Ref) -> Ref;
    fn CGImageSourceCreateImageAtIndex(source: Ref, index: usize, options: Ref) -> Ref;
    fn CGImageSourceCopyAuxiliaryDataInfoAtIndex(source: Ref, index: usize, kind: Ref) -> Ref;
}

const UTF8: u32 = 0x0800_0100;
const NUMBER_SINT64: isize = 4;

// Core Graphics' bitmap info: the alpha's place in the low five bits, the
// float flag, the byte order, and the packed formats.
const ALPHA_MASK: u32 = 0x1f;
const FLOAT: u32 = 1 << 8;
const ORDER_MASK: u32 = 0x7000;
const ORDER_16_LITTLE: u32 = 1 << 12;
const ORDER_32_LITTLE: u32 = 2 << 12;
const PIXEL_FORMAT_MASK: u32 = 0xf_0000;
const PIXEL_FORMAT_RGB101010: u32 = 3 << 16;

/// The `auxC` types a depth image is named by, as `libheif` finds one.
const DEPTH: &[&str] = &[
    "urn:mpeg:hevc:2015:auxid:2",
    "urn:mpeg:mpegB:cicp:systems:auxiliary:depth",
];

/// The most an EXIF item may run to: the block is at most 64 KB in a
/// JPEG, and a HEIF's is the same block.
const MAX_EXIF: usize = 1 << 20;

/// The most an XMP item may run to, on the reasoning `image::xmp` gives.
const MAX_XMP: usize = 4 << 20;

/// The brands a file is ours by: HEVC's, AV1's, and the container's own.
const BRANDS: &[&[u8]] = &[
    b"heic", b"heix", b"hevc", b"hevx", b"heim", b"heis", b"hevm", b"hevs", b"avif", b"avis",
    b"mif1", b"mif2", b"msf1",
];

pub struct Heif;

impl decode::Decoder for Heif {
    fn name(&self) -> &'static str {
        "heif/heic/avif"
    }

    fn extensions(&self) -> &'static [&'static str] {
        EXTENSIONS
    }

    fn sniff(&self, header: &[u8]) -> bool {
        brands(header).any(|brand| BRANDS.contains(&brand))
    }

    /// Which of the three, by the brands the `ftyp` box names: an AVIF
    /// names `avif` or `avis` among them, a HEIC one of HEVC's, and a
    /// file that names neither is the container itself.
    fn format(&self, header: &[u8]) -> &'static str {
        brand(header).unwrap_or_else(|| self.name())
    }

    /// The packet the picture carries: a `mime` item of the packet's
    /// content type that describes the primary item.
    fn xmp(&self, source: &mut dyn ReadSeek) -> Result<Option<Vec<u8>>> {
        let Some(meta) = Meta::read(source)? else {
            return Ok(None);
        };
        let Some(primary) = meta.primary else {
            return Ok(None);
        };
        xmp_of(&meta, source, primary)
    }

    /// The primary item's size once its crop and its turn are applied, from
    /// the tables alone, as `libheif`'s handle reports it.
    fn dimensions(&self, source: &mut dyn ReadSeek) -> Result<Option<(u32, u32)>> {
        Ok(Meta::read(source)?.and_then(|meta| meta.shown_size(meta.primary?)))
    }

    fn decode(
        &self,
        source: &mut dyn ReadSeek,
        overrides: decode::Overrides,
    ) -> Result<DecodedImage> {
        let meta = Meta::read(source)?;
        let primary = meta.as_ref().and_then(|meta| meta.primary);
        let tone_map = match (&meta, overrides.gain_map) {
            (Some(meta), true) => tmap::in_tables(meta, source)?,
            _ => None,
        };

        let length = source.seek(std::io::SeekFrom::End(0))?;
        if length > decode::MAX_DECODED_BYTES {
            bail!("the file is {length} bytes");
        }
        source.seek(std::io::SeekFrom::Start(0))?;
        let mut file = Vec::with_capacity(length as usize);
        source.read_to_end(&mut file)?;
        let reader = Reader::open(&file)?;

        let (width, height, bits) = reader.header()?;
        if width == 0 || height == 0 {
            bail!("HEIF image is {width}x{height}");
        }
        if u64::from(width) * u64::from(height) > MAX_PIXELS {
            bail!("HEIF image is {width}x{height}");
        }
        // As ImageIO hands it over: four components a pixel, at the width
        // the file's depth takes.
        decode::check_decoded_size(width, height, 4, if bits > 8 { 16 } else { 8 })?;

        // Where the tables say the file holds one channel, or one and
        // alpha, the picture is gray, whatever ImageIO makes of it.
        let gray = meta
            .as_ref()
            .zip(primary)
            .and_then(|(meta, id)| {
                meta.property(id, |p| match p {
                    Property::Channels(bits) => Some(bits.len()),
                    _ => None,
                })
            })
            .is_some_and(|count| count <= 2);
        let picture = reader.picture(gray)?;
        let wide = matches!(picture.samples, Samples::U16 { .. });
        let channels = picture.samples.channels();

        let color = match (&meta, primary) {
            (Some(meta), Some(id)) => {
                let (nclx, icc) = meta.color(id);
                ColorSpace::stated(nclx, icc.as_deref())
            }
            _ => picture.color,
        };
        let mut image = DecodedImage::new(
            picture.width,
            picture.height,
            picture.samples,
            color,
            picture.alpha,
        );

        if let (true, Some(meta), Some(primary)) = (overrides.gain_map, &meta, primary)
            && channels == Channels::Rgb
            && !wide
        {
            image.gain_map = gain_map(&reader, meta, source, primary, tone_map)?;
        }
        // A depth map that will not decode costs the readout its depth and
        // nothing else, so it is let go rather than refusing the picture.
        image.depth = depth_map(&reader, meta.as_ref(), primary, source)
            .ok()
            .flatten();

        Ok(orient::apply(
            image,
            orient::from_tag(u8::try_from(reader.orientation()).ok()),
        ))
    }
}

/// The XMP packet `id` carries: the `mime` item of the packet's content
/// type that describes it.
fn xmp_of(meta: &Meta, source: &mut dyn ReadSeek, id: u32) -> Result<Option<Vec<u8>>> {
    match meta.describing(id, b"mime", Some("application/rdf+xml")) {
        Some(item) => meta.payload(source, item, MAX_XMP),
        None => Ok(None),
    }
}

/// The picture's gain map, where the file has one this reader can apply:
/// an ISO 21496-1 tone map whose base is this picture, or Apple's
/// auxiliary image with a maker note to say how far it lifts — the same
/// choice `libheif`'s reader makes, of the map ImageIO decodes.
fn gain_map(
    reader: &Reader,
    meta: &Meta,
    source: &mut dyn ReadSeek,
    primary: u32,
    tone_map: Option<tmap::ToneMap>,
) -> Result<Option<Arc<GainMap>>> {
    let (map, lift) = if let Some(tone_map) = tone_map {
        if tone_map.base != primary {
            return Ok(None);
        }
        // v1.1 allows the stored image to be the HDR one, with the map
        // saying how to get *down* to SDR. Nothing below implements that
        // direction, and quietly brightening an image that is already
        // bright would be worse than saying so.
        if tone_map.metadata.backward_direction {
            bail!(
                "gain map runs the other way (the base image is the HDR one), \
                 which this build cannot apply — try --no-gain-map"
            );
        }
        // SAFETY: a constant ImageIO exports.
        let Some(map) = reader.auxiliary(unsafe { kCGImageAuxiliaryDataTypeISOGainMap }) else {
            return Ok(None);
        };
        (map, Lift::Iso(tone_map.metadata))
    } else {
        // SAFETY: a constant ImageIO exports.
        let Some(map) = reader.auxiliary(unsafe { kCGImageAuxiliaryDataTypeHDRGainMap }) else {
            return Ok(None);
        };
        let Some(exif) = meta.describing(primary, b"Exif", None) else {
            return Ok(None);
        };
        let Some(headroom) = meta
            .payload(source, exif, MAX_EXIF)?
            .and_then(|exif| apple::headroom(&exif))
        else {
            return Ok(None);
        };
        (map, Lift::Apple { headroom })
    };
    let (channels, codes) = match map.values {
        Values::Codes { channels, codes } => (channels, codes),
        Values::Floats(_) => {
            bail!("the gain map is not 8-bit, which this build cannot apply — try --no-gain-map")
        }
    };
    Ok(Some(Arc::new(GainMap {
        width: map.width,
        height: map.height,
        channels,
        data: Data::Codes(codes),
        lift,
    })))
}

/// The picture's depth map, where it has one: disparity or depth as
/// ImageIO decodes it, turned back into the codes the file stored where the
/// depth image's own XMP says how they map onto distances, so that it
/// reads as `libheif`'s does; where it says nothing, kept as ImageIO's.
fn depth_map(
    reader: &Reader,
    meta: Option<&Meta>,
    primary: Option<u32>,
    source: &mut dyn ReadSeek,
) -> Result<Option<Arc<DepthMap>>> {
    // SAFETY: constants ImageIO exports.
    let Some(map) = reader
        .auxiliary(unsafe { kCGImageAuxiliaryDataTypeDepth })
        .or_else(|| reader.auxiliary(unsafe { kCGImageAuxiliaryDataTypeDisparity }))
    else {
        return Ok(None);
    };
    let scale = match (meta, primary) {
        (Some(meta), Some(primary)) => match meta.auxiliary(primary, DEPTH) {
            Some(item) => xmp_of(meta, source, item)?
                .and_then(|packet| Xmp::parse(&packet))
                .and_then(|xmp| depth::scale(&xmp)),
            None => None,
        },
        _ => None,
    };
    let samples = match (map.values, scale) {
        (Values::Codes { codes, .. }, _) => Samples::U8 {
            channels: Channels::Gray,
            data: codes,
        },
        // The scale's codes spread evenly over its values, so a value goes
        // back to the code it was made from by the same line.
        (Values::Floats(values), Some(scale)) => {
            let [low, high] = scale.codes.unwrap_or([0.0, 255.0]);
            let [from, to] = scale.values;
            if from == to {
                bail!("the depth map's scale runs nowhere");
            }
            Samples::U8 {
                channels: Channels::Gray,
                data: values
                    .iter()
                    .map(|&v| {
                        let code = low + (v - from) / (to - from) * (high - low);
                        code.round().clamp(0.0, 255.0) as u8
                    })
                    .collect(),
            }
        }
        (Values::Floats(values), None) => Samples::F32 {
            channels: Channels::Gray,
            data: values,
        },
    };
    Ok(Some(Arc::new(DepthMap {
        width: map.width,
        height: map.height,
        samples,
        scale,
    })))
}

/// A Core Foundation object released when it goes, `None` for the null a
/// copy or create returns when there is nothing to hand back.
struct Owned(Ref);

impl Owned {
    fn new(object: Ref) -> Option<Self> {
        (!object.is_null()).then(|| Self(object))
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: the object was handed over by a Create or Copy call, so
        // this is the one reference to give back.
        unsafe { CFRelease(self.0) }
    }
}

/// The bytes of a `CFData`.
fn bytes(data: &Owned) -> &[u8] {
    // SAFETY: `data` is a live CFData; the pointer and length are its own,
    // and the slice lives no longer than the borrow of it.
    unsafe {
        let length = CFDataGetLength(data.0) as usize;
        if length == 0 {
            return &[];
        }
        std::slice::from_raw_parts(CFDataGetBytePtr(data.0), length)
    }
}

fn number(dictionary: Ref, key: Ref) -> Option<i64> {
    // SAFETY: a live dictionary, and a value that is a CFNumber where the
    // key is one ImageIO documents as a number.
    unsafe {
        let value = CFDictionaryGetValue(dictionary, key);
        if value.is_null() {
            return None;
        }
        let mut out = 0i64;
        (CFNumberGetValue(value, NUMBER_SINT64, (&raw mut out).cast()) != 0).then_some(out)
    }
}

fn string_key(text: &CStr) -> Option<Owned> {
    // SAFETY: a NUL-terminated UTF-8 string.
    Owned::new(unsafe { CFStringCreateWithCString(std::ptr::null(), text.as_ptr(), UTF8) })
}

fn text(string: &Owned) -> Option<String> {
    let mut buffer = [0 as c_char; 256];
    // SAFETY: a live CFString, written into a buffer of the size given,
    // which the call NUL-terminates where it succeeds.
    unsafe {
        (CFStringGetCString(string.0, buffer.as_mut_ptr(), buffer.len() as isize, UTF8) != 0).then(
            || {
                CStr::from_ptr(buffer.as_ptr())
                    .to_string_lossy()
                    .into_owned()
            },
        )
    }
}

/// A file opened with ImageIO: the source, and the index of its primary
/// image.
struct Reader {
    source: Owned,
    index: usize,
    properties: Owned,
}

/// The picture as ImageIO decoded it, in the program's terms but as stored.
struct Picture {
    width: u32,
    height: u32,
    samples: Samples,
    alpha: AlphaMode,
    /// What ImageIO's color space says, for a file with no tables to say
    /// it instead.
    color: ColorSpace,
}

/// An auxiliary image's values, as stored.
struct Auxiliary {
    width: u32,
    height: u32,
    values: Values,
}

enum Values {
    /// Eight-bit codes, `channels` to a pixel.
    Codes { channels: u8, codes: Vec<u8> },
    /// One float a pixel: depth or disparity.
    Floats(Vec<f32>),
}

impl Reader {
    fn open(file: &[u8]) -> Result<Self> {
        // SAFETY: the data is copied by CFDataCreate, so the source owns
        // what it reads; each object is released when its `Owned` goes.
        unsafe {
            let data = Owned::new(CFDataCreate(
                std::ptr::null(),
                file.as_ptr(),
                file.len() as isize,
            ))
            .ok_or_else(|| anyhow!("could not hand the file to ImageIO"))?;
            let source = Owned::new(CGImageSourceCreateWithData(data.0, std::ptr::null()))
                .ok_or_else(|| anyhow!("ImageIO does not read this file"))?;
            let index = CGImageSourceGetPrimaryImageIndex(source.0);
            let properties = Owned::new(CGImageSourceCopyPropertiesAtIndex(
                source.0,
                index,
                std::ptr::null(),
            ))
            .ok_or_else(|| anyhow!("ImageIO finds no picture in this file"))?;
            Ok(Self {
                source,
                index,
                properties,
            })
        }
    }

    /// The stored size and the bits a component, from ImageIO's header.
    fn header(&self) -> Result<(u32, u32, u32)> {
        // SAFETY: constants ImageIO exports.
        let (w, h, d) = unsafe {
            (
                number(self.properties.0, kCGImagePropertyPixelWidth),
                number(self.properties.0, kCGImagePropertyPixelHeight),
                number(self.properties.0, kCGImagePropertyDepth),
            )
        };
        let (Some(w), Some(h)) = (w, h) else {
            bail!("ImageIO gives no size for this picture");
        };
        Ok((
            u32::try_from(w)?,
            u32::try_from(h)?,
            d.and_then(|d| u32::try_from(d).ok()).unwrap_or(8),
        ))
    }

    /// The EXIF orientation that turns the pixels as stored into the
    /// picture as shown: what the container's `irot` and `imir` say.
    fn orientation(&self) -> i64 {
        // SAFETY: a constant ImageIO exports.
        number(self.properties.0, unsafe { kCGImagePropertyOrientation }).unwrap_or(1)
    }

    /// The picture decoded, as stored. `gray` takes one channel of the RGB
    /// ImageIO makes of a gray picture.
    fn picture(&self, gray: bool) -> Result<Picture> {
        // SAFETY: the options dictionary and the image are released when
        // their `Owned` goes, and the buffer is copied out before then.
        unsafe {
            let keys = [kCGImageSourceShouldCacheImmediately];
            let values = [kCFBooleanTrue];
            let options = Owned::new(CFDictionaryCreate(
                std::ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                1,
                &raw const kCFTypeDictionaryKeyCallBacks,
                &raw const kCFTypeDictionaryValueCallBacks,
            ));
            let image = Owned::new(CGImageSourceCreateImageAtIndex(
                self.source.0,
                self.index,
                options.as_ref().map_or(std::ptr::null(), |o| o.0),
            ))
            .ok_or_else(|| anyhow!("ImageIO could not decode this picture"))?;
            let layout = Layout {
                width: CGImageGetWidth(image.0),
                height: CGImageGetHeight(image.0),
                bits_per_component: CGImageGetBitsPerComponent(image.0),
                bits_per_pixel: CGImageGetBitsPerPixel(image.0),
                bytes_per_row: CGImageGetBytesPerRow(image.0),
                info: CGImageGetBitmapInfo(image.0),
                components: CGColorSpaceGetNumberOfComponents(CGImageGetColorSpace(image.0)),
            };
            let space = CGImageGetColorSpace(image.0);
            let color = Owned::new(CGColorSpaceCopyName(space))
                .and_then(|name| text(&name))
                .and_then(|name| named(&name))
                .unwrap_or_else(|| {
                    let icc = Owned::new(CGColorSpaceCopyICCData(space));
                    ColorSpace::stated(None, icc.as_ref().map(bytes))
                });
            // Where ImageIO decoded nothing, there is no buffer to copy: a
            // file macOS cannot read, which Preview shows blank.
            let data = Owned::new(CGDataProviderCopyData(CGImageGetDataProvider(image.0)))
                .ok_or_else(|| {
                    anyhow!("macOS could not decode this picture: ImageIO decoded nothing")
                })?;
            let (samples, alpha) = layout.samples(bytes(&data), gray)?;
            Ok(Picture {
                width: u32::try_from(layout.width)?,
                height: u32::try_from(layout.height)?,
                samples,
                alpha,
                color,
            })
        }
    }

    /// An auxiliary image of the type ImageIO names `kind`, as stored, in
    /// one of the layouts Core Video names an auxiliary image's samples
    /// by. `None` where there is none, or it is in a layout not read here.
    fn auxiliary(&self, kind: Ref) -> Option<Auxiliary> {
        // SAFETY: a live source; the dictionary is ImageIO's, its values
        // the types ImageIO documents for each key.
        unsafe {
            let info = Owned::new(CGImageSourceCopyAuxiliaryDataInfoAtIndex(
                self.source.0,
                self.index,
                kind,
            ))?;
            let data = CFDictionaryGetValue(info.0, kCGImageAuxiliaryDataInfoData);
            let description =
                CFDictionaryGetValue(info.0, kCGImageAuxiliaryDataInfoDataDescription);
            if data.is_null() || description.is_null() {
                return None;
            }
            let key = |name: &CStr| number(description, string_key(name)?.0);
            let width = usize::try_from(key(c"Width")?).ok()?;
            let height = usize::try_from(key(c"Height")?).ok()?;
            let row = usize::try_from(key(c"BytesPerRow")?).ok()?;
            let format = u32::try_from(key(c"PixelFormat")?).ok()?;
            decode::check_decoded_size(
                u32::try_from(width).ok()?,
                u32::try_from(height).ok()?,
                4,
                8,
            )
            .ok()?;
            let length = CFDataGetLength(data) as usize;
            let raw = std::slice::from_raw_parts(CFDataGetBytePtr(data), length);
            let rows = |bytes_per_pixel: usize| -> Option<Vec<&[u8]>> {
                (0..height)
                    .map(|y| raw.get(y * row..)?.get(..width * bytes_per_pixel))
                    .collect()
            };
            let half = |b: &[u8; 2]| half::f16::from_le_bytes(*b).to_f32();
            let single = |b: &[u8; 4]| f32::from_le_bytes(*b);
            let values = match &format.to_be_bytes() {
                b"L008" => Values::Codes {
                    channels: 1,
                    codes: rows(1)?.concat(),
                },
                // kCVPixelFormatType_24RGB.
                [0, 0, 0, 0x18] => Values::Codes {
                    channels: 3,
                    codes: rows(3)?.concat(),
                },
                b"BGRA" => Values::Codes {
                    channels: 3,
                    codes: rows(4)?
                        .iter()
                        .flat_map(|line| {
                            line.as_chunks::<4>()
                                .0
                                .iter()
                                .flat_map(|p| [p[2], p[1], p[0]])
                        })
                        .collect(),
                },
                b"hdep" | b"hdis" | b"L00h" => Values::Floats(
                    rows(2)?
                        .iter()
                        .flat_map(|line| line.as_chunks::<2>().0.iter().map(half))
                        .collect(),
                ),
                b"fdep" | b"fdis" | b"L00f" => Values::Floats(
                    rows(4)?
                        .iter()
                        .flat_map(|line| line.as_chunks::<4>().0.iter().map(single))
                        .collect(),
                ),
                _ => return None,
            };
            Some(Auxiliary {
                width: u32::try_from(width).ok()?,
                height: u32::try_from(height).ok()?,
                values,
            })
        }
    }
}

/// How a `CGImage`'s buffer is laid out.
struct Layout {
    width: usize,
    height: usize,
    bits_per_component: usize,
    bits_per_pixel: usize,
    bytes_per_row: usize,
    info: u32,
    /// The color space's components, not counting alpha.
    components: usize,
}

impl Layout {
    /// The buffer as the program's samples: eight bits stay eight, anything
    /// wider becomes sixteen at full scale, and gray where `gray` says so.
    fn samples(&self, raw: &[u8], gray: bool) -> Result<(Samples, AlphaMode)> {
        let (bpc, bpp) = (self.bits_per_component, self.bits_per_pixel);
        let order = self.info & ORDER_MASK;
        if self.info & FLOAT != 0 {
            bail!("ImageIO decoded this picture to floats, which this build does not read");
        }
        let row_bytes = self.width * bpp / 8;
        if self.bytes_per_row < row_bytes
            || raw.len() < self.bytes_per_row * (self.height - 1) + row_bytes
        {
            bail!("ImageIO's buffer is shorter than its picture");
        }
        let rows = (0..self.height).map(|y| &raw[y * self.bytes_per_row..][..row_bytes]);

        // Three ten-bit components and two bits of padding in one 32-bit
        // word, the padding at the top for skip-first and at the bottom for
        // skip-last, lifted to sixteen bits as `libheif`'s are.
        if bpc == 10 && bpp == 32 {
            if self.info & PIXEL_FORMAT_MASK != PIXEL_FORMAT_RGB101010 {
                bail!("ImageIO decoded this picture to a ten-bit layout this build does not read");
            }
            let shift = if self.info & ALPHA_MASK == 5 { 2 } else { 0 };
            let lift = |v: u32| ((v * 65535 + 511) / 1023) as u16;
            let mut data = Vec::with_capacity(self.width * self.height * 3);
            for line in rows {
                for &word in line.as_chunks::<4>().0 {
                    let word = if order == ORDER_32_LITTLE {
                        u32::from_le_bytes(word)
                    } else {
                        u32::from_be_bytes(word)
                    } >> shift;
                    let rgb = [(word >> 20) & 0x3ff, (word >> 10) & 0x3ff, word & 0x3ff];
                    if gray {
                        data.push(lift(rgb[0]));
                    } else {
                        data.extend(rgb.map(lift));
                    }
                }
            }
            let channels = if gray { Channels::Gray } else { Channels::Rgb };
            return Ok((Samples::U16 { channels, data }, AlphaMode::Opaque));
        }

        if bpc != 8 && bpc != 16 {
            bail!(
                "ImageIO decoded this picture to {bpc} bits a component, which this build does not read"
            );
        }
        let count = bpp / bpc;
        let color = self.components;
        // Where alpha sits among the components in their logical order,
        // whether it is a channel or only padding, and whether the color
        // has been multiplied by it.
        let (alpha_at, real, premultiplied) = match self.info & ALPHA_MASK {
            1 => (Some(count - 1), true, true),
            2 => (Some(0), true, true),
            3 => (Some(count - 1), true, false),
            4 => (Some(0), true, false),
            5 => (Some(count - 1), false, false),
            6 => (Some(0), false, false),
            _ => (None, false, false),
        };
        if count != color + usize::from(alpha_at.is_some()) || !(color == 1 || color == 3) {
            bail!("ImageIO decoded this picture to a layout this build does not read");
        }
        // An 8-bit pixel in a 32-bit little-endian word is stored in the
        // reverse of its logical order.
        let reversed = bpc == 8 && order == ORDER_32_LITTLE;
        let little = bpc == 16 && order == ORDER_16_LITTLE;
        // The components kept, in their logical order: one of the three
        // for a gray picture, then alpha.
        let colors: Vec<usize> = (0..count).filter(|&c| Some(c) != alpha_at).collect();
        let mut kept: Vec<usize> = if gray || color == 1 {
            vec![colors[0]]
        } else {
            colors
        };
        if real {
            kept.push(alpha_at.unwrap_or(0));
        }
        let channels = match (kept.len(), real) {
            (1, _) => Channels::Gray,
            (2, _) => Channels::GrayAlpha,
            (3, _) => Channels::Rgb,
            _ => Channels::Rgba,
        };
        let alpha = AlphaMode::of(channels, premultiplied);
        let at = |c: usize| if reversed { count - 1 - c } else { c };
        let samples = if bpc == 8 {
            let mut data = Vec::with_capacity(self.width * self.height * kept.len());
            for line in rows {
                for pixel in line.chunks_exact(count) {
                    data.extend(kept.iter().map(|&c| pixel[at(c)]));
                }
            }
            Samples::U8 { channels, data }
        } else {
            let mut data = Vec::with_capacity(self.width * self.height * kept.len());
            for line in rows {
                for pixel in line.chunks_exact(count * 2) {
                    data.extend(kept.iter().map(|&c| {
                        let b = [pixel[2 * c], pixel[2 * c + 1]];
                        if little {
                            u16::from_le_bytes(b)
                        } else {
                            u16::from_be_bytes(b)
                        }
                    }));
                }
            }
            Samples::U16 { channels, data }
        };
        Ok((samples, alpha))
    }
}

/// The color space Core Graphics names, as the program models it, for a
/// file whose tables say nothing; `None` for a name not listed, and the
/// space's ICC profile is read instead. The HDR spaces say less through
/// their profile than through their name.
fn named(name: &str) -> Option<ColorSpace> {
    use Primaries as P;
    use Transfer as T;
    let (primaries, transfer) = match name {
        "kCGColorSpaceSRGB" => (P::Bt709, T::Srgb),
        "kCGColorSpaceDisplayP3" => (P::DisplayP3, T::Srgb),
        "kCGColorSpaceITUR_2100_PQ" => (P::Bt2020, T::Pq),
        "kCGColorSpaceITUR_2100_HLG" => (P::Bt2020, T::Hlg),
        "kCGColorSpaceLinearSRGB" => (P::Bt709, T::Linear),
        "kCGColorSpaceLinearDisplayP3" => (P::DisplayP3, T::Linear),
        // CICP's BT.709 and BT.2020 curves read as sRGB here, as they do
        // in `color::cicp`: the display the content was graded for.
        "kCGColorSpaceITUR_2020" => (P::Bt2020, T::Srgb),
        "kCGColorSpaceITUR_709" => (P::Bt709, T::Srgb),
        _ => return None,
    };
    Some(ColorSpace {
        transfer,
        primaries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::decode::Decoder;

    /// The brands make a file ours, and a JPEG XL container, a Canon CR3
    /// and an MP4 are not.
    #[test]
    fn the_brands_make_a_file_ours() {
        let heif = Heif;
        assert!(heif.sniff(b"\x00\x00\x00\x18ftypheic\x00\x00\x00\x00mif1heic"));
        assert!(heif.sniff(b"\x00\x00\x00\x1cftypavif\x00\x00\x00\x00avifmif1miaf"));
        assert!(heif.sniff(b"\x00\x00\x00\x14ftypmif1\x00\x00\x00\x00mif1"));
        assert!(!heif.sniff(b"\x00\x00\x00\x18ftypmp42\x00\x00\x00\x00mp42isom"));
        assert!(!heif.sniff(b"\x00\x00\x00\x18ftypcrx \x00\x00\x00\x01crx isom"));
        assert!(!heif.sniff(b"\x00\x00\x00\x0cJXL \r\n\x87\n\x00\x00\x00\x14ftypjxl "));
        assert!(!heif.sniff(b"\x00\x00\x00\x18ftyp"));
        assert!(!heif.sniff(b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR"));
    }

    /// A ten-bit word unpacks to three components lifted to full scale, in
    /// either padding's place.
    #[test]
    fn a_packed_ten_bit_word_unpacks() {
        let layout = |alpha: u32| Layout {
            width: 1,
            height: 1,
            bits_per_component: 10,
            bits_per_pixel: 32,
            bytes_per_row: 4,
            info: PIXEL_FORMAT_RGB101010 | ORDER_32_LITTLE | alpha,
            components: 3,
        };
        // Skip-first: red 1023, green 0, blue 512.
        let word: u32 = (1023 << 20) | 512;
        let (samples, alpha) = layout(6).samples(&word.to_le_bytes(), false).unwrap();
        assert_eq!(alpha, AlphaMode::Opaque);
        let Samples::U16 { data, .. } = samples else {
            panic!("sixteen bits")
        };
        assert_eq!(data, [65535, 0, 32800]);
        // Skip-last: the same three two bits up.
        let (samples, _) = layout(5).samples(&(word << 2).to_le_bytes(), true).unwrap();
        let Samples::U16 { data, channels } = samples else {
            panic!("sixteen bits")
        };
        assert_eq!((channels, data), (Channels::Gray, vec![65535]));
    }

    /// Eight-bit pixels in either byte order and with alpha in either
    /// place come out RGBA in that order, straight or premultiplied as
    /// the bitmap says.
    #[test]
    fn eight_bit_pixels_come_out_in_logical_order() {
        let layout = |info: u32| Layout {
            width: 1,
            height: 1,
            bits_per_component: 8,
            bits_per_pixel: 32,
            bytes_per_row: 4,
            info,
            components: 3,
        };
        // Alpha last, big-endian: R G B A as stored.
        let (samples, alpha) = layout(3).samples(&[10, 20, 30, 40], false).unwrap();
        assert_eq!(alpha, AlphaMode::Straight);
        assert!(
            matches!(samples, Samples::U8 { channels: Channels::Rgba, ref data } if data == &[10, 20, 30, 40])
        );
        // Premultiplied first in a little-endian word: B G R A as stored.
        let (samples, alpha) = layout(2 | ORDER_32_LITTLE)
            .samples(&[30, 20, 10, 40], false)
            .unwrap();
        assert_eq!(alpha, AlphaMode::Premultiplied);
        assert!(
            matches!(samples, Samples::U8 { channels: Channels::Rgba, ref data } if data == &[10, 20, 30, 40])
        );
        // Padding last, gray asked for: the first color alone.
        let (samples, alpha) = layout(5).samples(&[7, 7, 7, 0], true).unwrap();
        assert_eq!(alpha, AlphaMode::Opaque);
        assert!(
            matches!(samples, Samples::U8 { channels: Channels::Gray, ref data } if data == &[7])
        );
    }
}
