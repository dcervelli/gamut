//! What ImageIO makes of the same HEIF files `libheif` reads, side by side:
//! the measure of whether a Mac could read HEIC and AVIF through the
//! system's own decoder rather than a bundled `libheif`.
//!
//! Each file is decoded both ways — through [`super::Heif`], as the program
//! reads it now, and through `CGImageSource` — and the two are compared on
//! what the rest of the program reads of a decoded HEIF: the size once the
//! container's `irot` and `imir` are applied, the channels and the depth,
//! the alpha, every sample, the color space the pixels are in, the gain
//! map's codes and the depth map's, and how long each took.
//!
//! ImageIO hands its pixels back as stored, with the turn the container
//! asks for as an EXIF orientation beside them, where `libheif` applies it;
//! so ImageIO's are read through that orientation before they are
//! compared. Its auxiliary images come back as stored too.
//!
//! The fixtures are always compared. A folder of real files — or several,
//! `:`-separated, or single files — is compared as well where
//! `GAMUT_HEIF_CORPUS` names one, at most `GAMUT_HEIF_CORPUS_LIMIT` of them
//! (20 unless said), which is where phones' grids, gain maps and depth maps
//! are; run with `--nocapture` to read the report.
//!
//! ImageIO, Core Graphics and Core Foundation are bound here by hand, as
//! `finder.rs` once bound `LSRegisterURL`, so that the comparison costs the
//! tree no dependency.

use std::ffi::{CStr, c_char, c_void};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::image::color::ColorSpace;
use crate::image::decode::{Decoder, Overrides};
use crate::image::depth;
use crate::image::gain_map::Data;
use crate::image::xmp::Xmp;
use crate::image::{AlphaMode, DecodedImage, Samples};

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
    fn CGImageRelease(image: Ref);
    fn CGDataProviderCopyData(provider: Ref) -> Ref;
    fn CGColorSpaceGetNumberOfComponents(space: Ref) -> usize;
    fn CGColorSpaceCopyICCData(space: Ref) -> Ref;
    fn CGColorSpaceCopyName(space: Ref) -> Ref;
    fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits_per_component: usize,
        bytes_per_row: usize,
        space: Ref,
        info: u32,
    ) -> Ref;
    fn CGBitmapContextCreateImage(context: Ref) -> Ref;
    fn CGContextDrawImage(context: Ref, rect: Rect, image: Ref);
    fn CGContextRelease(context: Ref);
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

/// The image drawn into a bitmap of its own color space, sixteen bits a
/// component with alpha premultiplied where the space has room for it —
/// for an image whose own buffer cannot be copied out. Exact for an 8- or
/// 10-bit picture, the space being its own.
fn redrawn(image: Ref) -> Option<Ref> {
    // SAFETY: a live image, drawn into a context made for its size and
    // released once its image is made.
    unsafe {
        let (width, height) = (CGImageGetWidth(image), CGImageGetHeight(image));
        let space = CGImageGetColorSpace(image);
        let info = if CGColorSpaceGetNumberOfComponents(space) == 1 {
            ORDER_16_LITTLE
        } else {
            ORDER_16_LITTLE | 1
        };
        let context =
            CGBitmapContextCreate(std::ptr::null_mut(), width, height, 16, 0, space, info);
        if context.is_null() {
            return None;
        }
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: width as f64,
            height: height as f64,
        };
        CGContextDrawImage(context, rect, image);
        let drawn = CGBitmapContextCreateImage(context);
        CGContextRelease(context);
        (!drawn.is_null()).then_some(drawn)
    }
}

#[link(name = "ImageIO", kind = "framework")]
unsafe extern "C" {
    static kCGImageSourceShouldCacheImmediately: Ref;
    static kCGImagePropertyOrientation: Ref;
    static kCGImageAuxiliaryDataTypeHDRGainMap: Ref;
    static kCGImageAuxiliaryDataTypeISOGainMap: Ref;
    static kCGImageAuxiliaryDataTypeDepth: Ref;
    static kCGImageAuxiliaryDataTypeDisparity: Ref;
    static kCGImageAuxiliaryDataInfoData: Ref;
    static kCGImageAuxiliaryDataInfoDataDescription: Ref;
    static kCGImageAuxiliaryDataInfoMetadata: Ref;
    fn CGImageSourceCreateWithData(data: Ref, options: Ref) -> Ref;
    fn CGImageSourceGetPrimaryImageIndex(source: Ref) -> usize;
    fn CGImageSourceGetStatusAtIndex(source: Ref, index: usize) -> i32;
    fn CGImageSourceCopyPropertiesAtIndex(source: Ref, index: usize, options: Ref) -> Ref;
    fn CGImageSourceCreateImageAtIndex(source: Ref, index: usize, options: Ref) -> Ref;
    fn CGImageSourceCopyAuxiliaryDataInfoAtIndex(source: Ref, index: usize, kind: Ref) -> Ref;
    fn CGImageMetadataCreateXMPData(metadata: Ref, options: Ref) -> Ref;
}

const UTF8: u32 = 0x0800_0100;
const NUMBER_SINT64: isize = 4;

// Core Graphics' bitmap info: the alpha's place in the low five bits, the
// float flag, and the byte order.
const ALPHA_MASK: u32 = 0x1f;
const FLOAT: u32 = 1 << 8;
const ORDER_MASK: u32 = 0x7000;
const ORDER_16_LITTLE: u32 = 1 << 12;
const ORDER_32_LITTLE: u32 = 2 << 12;
const PIXEL_FORMAT_MASK: u32 = 0xf_0000;

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

/// The bytes of a `CFData`, copied out.
fn bytes(data: Ref) -> Vec<u8> {
    // SAFETY: `data` is a live CFData; the pointer and length are its own.
    unsafe {
        let length = CFDataGetLength(data) as usize;
        if length == 0 {
            return Vec::new();
        }
        std::slice::from_raw_parts(CFDataGetBytePtr(data), length).to_vec()
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

fn string_key(text: &CStr) -> Owned {
    // SAFETY: a NUL-terminated UTF-8 string.
    Owned::new(unsafe { CFStringCreateWithCString(std::ptr::null(), text.as_ptr(), UTF8) })
        .expect("a CFString")
}

fn text(string: Ref) -> String {
    let mut buffer = [0 as c_char; 256];
    // SAFETY: a live CFString, written into a buffer of the size given.
    if unsafe { CFStringGetCString(string, buffer.as_mut_ptr(), buffer.len() as isize, UTF8) } == 0
    {
        return "?".into();
    }
    // SAFETY: CFStringGetCString NUL-terminated what it wrote.
    unsafe { CStr::from_ptr(buffer.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

/// A picture as both sides are compared in: every sample from 0 to 1 — or
/// past 1, for a float — row by row, `channels` to a pixel, alpha last and
/// not multiplied in.
#[derive(Clone)]
struct Plane {
    width: usize,
    height: usize,
    /// Color channels, not counting alpha.
    color: usize,
    alpha: bool,
    data: Vec<f32>,
}

impl Plane {
    fn channels(&self) -> usize {
        self.color + usize::from(self.alpha)
    }

    fn at(&self, x: usize, y: usize, channel: usize) -> f32 {
        self.data[(y * self.width + x) * self.channels() + channel]
    }

    /// Read through an EXIF orientation, as `libheif` reads through `irot`
    /// and `imir`: the picture as it is shown.
    fn oriented(&self, orientation: i64) -> Plane {
        let (w, h) = (self.width, self.height);
        let swapped = orientation >= 5;
        let (ow, oh) = if swapped { (h, w) } else { (w, h) };
        let n = self.channels();
        let mut data = Vec::with_capacity(self.data.len());
        for y in 0..oh {
            for x in 0..ow {
                let (sx, sy) = match orientation {
                    2 => (w - 1 - x, y),
                    3 => (w - 1 - x, h - 1 - y),
                    4 => (x, h - 1 - y),
                    5 => (y, x),
                    6 => (y, h - 1 - x),
                    7 => (w - 1 - y, h - 1 - x),
                    8 => (w - 1 - y, x),
                    _ => (x, y),
                };
                let at = (sy * w + sx) * n;
                data.extend_from_slice(&self.data[at..at + n]);
            }
        }
        Plane {
            width: ow,
            height: oh,
            data,
            ..*self
        }
    }
}

fn plane_of(samples: &Samples, width: u32, height: u32, alpha: AlphaMode) -> Plane {
    let channels = samples.channels();
    let has_alpha = channels.alpha_index().is_some();
    let mut data: Vec<f32> = match samples {
        Samples::U8 { data, .. } => data.iter().map(|&v| f32::from(v) / 255.0).collect(),
        Samples::U16 { data, .. } => data.iter().map(|&v| f32::from(v) / 65535.0).collect(),
        Samples::F32 { data, .. } => data.clone(),
    };
    let n = channels.count();
    if alpha == AlphaMode::Premultiplied {
        unpremultiply(&mut data, n);
    }
    Plane {
        width: width as usize,
        height: height as usize,
        color: n - usize::from(has_alpha),
        alpha: has_alpha,
        data,
    }
}

fn unpremultiply(data: &mut [f32], channels: usize) {
    for pixel in data.chunks_exact_mut(channels) {
        let (color, alpha) = pixel.split_at_mut(channels - 1);
        if alpha[0] > 0.0 {
            color.iter_mut().for_each(|v| *v /= alpha[0]);
        }
    }
}

/// The color space Core Graphics names, as the program models it: what a
/// reader of ImageIO's pictures would go by before the ICC profile, which
/// for the HDR spaces says less than the name does. `None` for a space
/// without a name or with one not listed.
fn named(name: &str) -> Option<ColorSpace> {
    use crate::image::color::{Primaries as P, Transfer as T};
    let (primaries, transfer) = match name.trim_end_matches(" (redrawn)") {
        "kCGColorSpaceSRGB" => (P::Bt709, T::Srgb),
        "kCGColorSpaceDisplayP3" => (P::DisplayP3, T::Srgb),
        "kCGColorSpaceITUR_2100_PQ" => (P::Bt2020, T::Pq),
        "kCGColorSpaceITUR_2100_HLG" => (P::Bt2020, T::Hlg),
        "kCGColorSpaceLinearSRGB" => (P::Bt709, T::Linear),
        "kCGColorSpaceLinearDisplayP3" => (P::DisplayP3, T::Linear),
        "kCGColorSpaceITUR_2020" => (P::Bt2020, T::Bt709),
        "kCGColorSpaceITUR_709" => (P::Bt709, T::Bt709),
        _ => return None,
    };
    Some(ColorSpace {
        transfer,
        primaries,
    })
}

/// How a `CGImage`'s buffer is laid out, said the way the report says it.
struct Layout {
    bits_per_component: usize,
    bits_per_pixel: usize,
    info: u32,
    color_space: String,
    icc: Option<Vec<u8>>,
}

impl Layout {
    fn describe(&self) -> String {
        let alpha = match self.info & ALPHA_MASK {
            0 => "none",
            1 => "premul-last",
            2 => "premul-first",
            3 => "last",
            4 => "first",
            5 => "skip-last",
            6 => "skip-first",
            7 => "only",
            _ => "?",
        };
        let order = match self.info & ORDER_MASK {
            0 => "default",
            ORDER_16_LITTLE => "16LE",
            ORDER_32_LITTLE => "32LE",
            0x3000 => "16BE",
            0x4000 => "32BE",
            _ => "?",
        };
        let packing = match (self.info & PIXEL_FORMAT_MASK) >> 16 {
            0 => "",
            1 => " rgb555",
            2 => " rgb565",
            3 => " rgb101010",
            4 => " rgbcif10",
            _ => " packed?",
        };
        format!(
            "{}bpc/{}bpp{packing} {}{} alpha={alpha} order={order} space={}",
            self.bits_per_component,
            self.bits_per_pixel,
            if self.info & FLOAT != 0 { "float " } else { "" },
            if self.icc.is_some() { "icc" } else { "no-icc" },
            self.color_space,
        )
    }
}

/// A `CGImage`'s pixels as a [`Plane`], as stored.
fn cg_plane(image: Ref) -> Option<(Plane, Layout)> {
    // SAFETY: `image` is a live CGImage; every getter reads it, and the
    // provider's data is copied before the image can go.
    unsafe {
        let width = CGImageGetWidth(image);
        let height = CGImageGetHeight(image);
        let bpc = CGImageGetBitsPerComponent(image);
        let bpp = CGImageGetBitsPerPixel(image);
        let row = CGImageGetBytesPerRow(image);
        let info = CGImageGetBitmapInfo(image);
        let space = CGImageGetColorSpace(image);
        let color = CGColorSpaceGetNumberOfComponents(space);
        let name = Owned::new(CGColorSpaceCopyName(space))
            .map_or_else(|| "unnamed".to_string(), |name| text(name.0));
        let icc = Owned::new(CGColorSpaceCopyICCData(space)).map(|data| bytes(data.0));
        let data = Owned::new(CGDataProviderCopyData(CGImageGetDataProvider(image)))?;
        let raw = bytes(data.0);

        let float = info & FLOAT != 0;
        let order = info & ORDER_MASK;
        let alpha_info = info & ALPHA_MASK;
        let components = bpp / bpc;
        // Where alpha sits among the components in their logical order, and
        // whether it is a channel at all or only padding.
        let (alpha_at, alpha_real, premultiplied) = match alpha_info {
            1 => (Some(components - 1), true, true),
            2 => (Some(0), true, true),
            3 => (Some(components - 1), true, false),
            4 => (Some(0), true, false),
            5 => (Some(components - 1), false, false),
            6 => (Some(0), false, false),
            _ => (None, false, false),
        };
        let little = match bpc {
            16 => order == ORDER_16_LITTLE || (order == 0 && float),
            32 => order == ORDER_32_LITTLE,
            _ => false,
        };
        // An 8-bit pixel in a 32-bit little-endian word is stored in the
        // reverse of its logical order.
        let reversed = bpc == 8 && order == ORDER_32_LITTLE;

        let out_channels = color + usize::from(alpha_real);
        let mut out = Vec::with_capacity(width * height * out_channels);
        // Three ten-bit components and two bits of padding in one 32-bit
        // word, the padding at the top for skip-first and at the bottom for
        // skip-last; CIF10 is the same words over an extended range, which
        // the report says rather than reads.
        if bpc == 10 && bpp == 32 {
            let shift = if alpha_info == 5 { 2 } else { 0 };
            for y in 0..height {
                let line = &raw[y * row..];
                for x in 0..width {
                    let b = &line[x * 4..][..4];
                    let word = if order == ORDER_32_LITTLE {
                        u32::from_le_bytes([b[0], b[1], b[2], b[3]])
                    } else {
                        u32::from_be_bytes([b[0], b[1], b[2], b[3]])
                    } >> shift;
                    for at in [20, 10, 0] {
                        out.push(((word >> at) & 0x3ff) as f32 / 1023.0);
                    }
                }
            }
            return Some((
                Plane {
                    width,
                    height,
                    color: 3,
                    alpha: false,
                    data: out,
                },
                Layout {
                    bits_per_component: bpc,
                    bits_per_pixel: bpp,
                    info,
                    color_space: name,
                    icc,
                },
            ));
        }
        let bytes_per = bpc / 8;
        let mut logical = vec![0f32; components];
        for y in 0..height {
            let line = &raw[y * row..];
            for x in 0..width {
                let pixel = &line[x * bpp / 8..][..bpp / 8];
                for c in 0..components {
                    let word = &pixel[c * bytes_per..][..bytes_per];
                    let value = match (bpc, float) {
                        (8, _) => f32::from(word[0]) / 255.0,
                        (16, false) => {
                            let v = if little {
                                u16::from_le_bytes([word[0], word[1]])
                            } else {
                                u16::from_be_bytes([word[0], word[1]])
                            };
                            f32::from(v) / 65535.0
                        }
                        (16, true) => {
                            let v = if little {
                                u16::from_le_bytes([word[0], word[1]])
                            } else {
                                u16::from_be_bytes([word[0], word[1]])
                            };
                            half::f16::from_bits(v).to_f32()
                        }
                        (32, true) => {
                            let b = [word[0], word[1], word[2], word[3]];
                            if little {
                                f32::from_le_bytes(b)
                            } else {
                                f32::from_be_bytes(b)
                            }
                        }
                        _ => f32::NAN,
                    };
                    let at = if reversed { components - 1 - c } else { c };
                    logical[at] = value;
                }
                for (c, value) in logical.iter().enumerate() {
                    if Some(c) == alpha_at {
                        continue;
                    }
                    out.push(*value);
                }
                if alpha_real {
                    out.push(logical[alpha_at.unwrap()]);
                }
            }
        }
        if premultiplied {
            unpremultiply(&mut out, out_channels);
        }
        Some((
            Plane {
                width,
                height,
                color,
                alpha: alpha_real,
                data: out,
            },
            Layout {
                bits_per_component: bpc,
                bits_per_pixel: bpp,
                info,
                color_space: name,
                icc,
            },
        ))
    }
}

/// An auxiliary image ImageIO found, as stored: its samples as a [`Plane`]
/// of one channel or three, the pixel format it came in, and its XMP.
struct Auxiliary {
    plane: Plane,
    format: String,
    xmp: Option<Vec<u8>>,
}

fn four_cc(code: i64) -> String {
    let code = code as u32;
    if code < 0x2020_2020 {
        return format!("0x{code:x}");
    }
    code.to_be_bytes().iter().map(|&b| b as char).collect()
}

/// Reads sample `x` of a row of an auxiliary image, as 0 to 1 or as the
/// float it is.
type Sample = fn(&[u8], usize) -> f32;

fn auxiliary(source: Ref, index: usize, kind: Ref) -> Option<Auxiliary> {
    // SAFETY: a live source; the dictionary is ImageIO's, its values the
    // types ImageIO documents for each key.
    unsafe {
        let info = Owned::new(CGImageSourceCopyAuxiliaryDataInfoAtIndex(
            source, index, kind,
        ))?;
        let data = CFDictionaryGetValue(info.0, kCGImageAuxiliaryDataInfoData);
        let description = CFDictionaryGetValue(info.0, kCGImageAuxiliaryDataInfoDataDescription);
        if data.is_null() || description.is_null() {
            return None;
        }
        let key = |name: &CStr| number(description, string_key(name).0);
        let width = key(c"Width")? as usize;
        let height = key(c"Height")? as usize;
        let row = key(c"BytesPerRow")? as usize;
        let format = key(c"PixelFormat")?;
        let raw = bytes(data);

        let metadata = CFDictionaryGetValue(info.0, kCGImageAuxiliaryDataInfoMetadata);
        let xmp = (!metadata.is_null())
            .then(|| Owned::new(CGImageMetadataCreateXMPData(metadata, std::ptr::null())))
            .flatten()
            .map(|data| bytes(data.0));

        // The formats Core Video names an auxiliary image's samples by.
        let (channels, read): (usize, Sample) = match &four_cc(format)[..] {
            "L008" => (1, |row, x| f32::from(row[x]) / 255.0),
            "L016" => (1, |row, x| {
                f32::from(u16::from_le_bytes([row[2 * x], row[2 * x + 1]])) / 65535.0
            }),
            "hdep" | "hdis" | "L00h" => (1, |row, x| {
                half::f16::from_bits(u16::from_le_bytes([row[2 * x], row[2 * x + 1]])).to_f32()
            }),
            "fdep" | "fdis" | "L00f" => (1, |row, x| {
                f32::from_le_bytes(row[4 * x..4 * x + 4].try_into().unwrap())
            }),
            "0x18" => (3, |row, x| f32::from(row[x]) / 255.0),
            "BGRA" => (3, |row, x| {
                // Three logical channels read out of four stored, B G R A.
                let (pixel, c) = (x / 3, x % 3);
                f32::from(row[pixel * 4 + 2 - c]) / 255.0
            }),
            _ => {
                return Some(Auxiliary {
                    plane: Plane {
                        width,
                        height,
                        color: 0,
                        alpha: false,
                        data: Vec::new(),
                    },
                    format: four_cc(format),
                    xmp,
                });
            }
        };
        let mut out = Vec::with_capacity(width * height * channels);
        for y in 0..height {
            let line = &raw[y * row..];
            for x in 0..width * channels {
                out.push(read(line, x));
            }
        }
        Some(Auxiliary {
            plane: Plane {
                width,
                height,
                color: channels,
                alpha: false,
                data: out,
            },
            format: four_cc(format),
            xmp,
        })
    }
}

/// Everything ImageIO said of a file.
struct Read {
    /// `CGImageSourceStatus` for the picture once decoded: 0 complete,
    /// -1 incomplete, -4 invalid data, -5 an unexpected end of file.
    status: i32,
    orientation: i64,
    plane: Plane,
    layout: Layout,
    apple_map: Option<Auxiliary>,
    iso_map: Option<Auxiliary>,
    depth: Option<Auxiliary>,
    disparity: Option<Auxiliary>,
}

/// How long ImageIO takes over what `libheif` is timed doing: the picture
/// decoded and its buffer copied out, and then the auxiliary images the
/// program reads — the gain map, Apple's or the ISO one, and the depth —
/// decoded as ImageIO hands them over. Nothing is converted, so this is
/// the system's own time and none of the comparison's.
fn timed(file: &[u8]) -> Option<(Duration, Duration)> {
    let started = Instant::now();
    // SAFETY: as in `imageio`.
    unsafe {
        let data = Owned::new(CFDataCreate(
            std::ptr::null(),
            file.as_ptr(),
            file.len() as isize,
        ))?;
        let source = Owned::new(CGImageSourceCreateWithData(data.0, std::ptr::null()))?;
        let index = CGImageSourceGetPrimaryImageIndex(source.0);
        let image = CGImageSourceCreateImageAtIndex(source.0, index, std::ptr::null());
        if image.is_null() {
            return None;
        }
        let pixels = Owned::new(CGDataProviderCopyData(CGImageGetDataProvider(image)));
        CGImageRelease(image);
        drop(pixels?);
        let picture = started.elapsed();
        let aux = |kind| {
            Owned::new(CGImageSourceCopyAuxiliaryDataInfoAtIndex(
                source.0, index, kind,
            ))
        };
        let map = aux(kCGImageAuxiliaryDataTypeISOGainMap)
            .or_else(|| aux(kCGImageAuxiliaryDataTypeHDRGainMap));
        let depth =
            aux(kCGImageAuxiliaryDataTypeDepth).or_else(|| aux(kCGImageAuxiliaryDataTypeDisparity));
        drop((map, depth));
        Some((picture, started.elapsed()))
    }
}

fn imageio(file: &[u8]) -> Option<Read> {
    // SAFETY: each call is handed objects that are live for its length;
    // the source and the image are released when their `Owned` goes.
    unsafe {
        let data = Owned::new(CFDataCreate(
            std::ptr::null(),
            file.as_ptr(),
            file.len() as isize,
        ))?;
        let source = Owned::new(CGImageSourceCreateWithData(data.0, std::ptr::null()))?;
        let index = CGImageSourceGetPrimaryImageIndex(source.0);
        let properties = Owned::new(CGImageSourceCopyPropertiesAtIndex(
            source.0,
            index,
            std::ptr::null(),
        ))?;
        let orientation = number(properties.0, kCGImagePropertyOrientation).unwrap_or(1);
        let keys = [kCGImageSourceShouldCacheImmediately];
        let values = [kCFBooleanTrue];
        let options = Owned::new(CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            &raw const kCFTypeDictionaryKeyCallBacks,
            &raw const kCFTypeDictionaryValueCallBacks,
        ))?;
        // Decoded at once where ImageIO can hand the pixels over that way,
        // and otherwise as it decodes by default, when they are asked for.
        let mut read = None;
        for options in [options.0, std::ptr::null()] {
            let image = CGImageSourceCreateImageAtIndex(source.0, index, options);
            if image.is_null() {
                return None;
            }
            read = cg_plane(image);
            if std::env::var("GAMUT_HEIF_REDRAW").is_ok() {
                read = None;
            }
            if read.is_none()
                && let Some(drawn) = redrawn(image)
            {
                read = cg_plane(drawn).map(|(plane, mut layout)| {
                    layout.color_space += " (redrawn)";
                    (plane, layout)
                });
                CGImageRelease(drawn);
            }
            CGImageRelease(image);
            if read.is_some() {
                break;
            }
        }
        let (plane, layout) = read?;
        let status = CGImageSourceGetStatusAtIndex(source.0, index);
        let iso_map = auxiliary(source.0, index, kCGImageAuxiliaryDataTypeISOGainMap);
        let apple_map = auxiliary(source.0, index, kCGImageAuxiliaryDataTypeHDRGainMap);
        let depth = auxiliary(source.0, index, kCGImageAuxiliaryDataTypeDepth);
        let disparity = auxiliary(source.0, index, kCGImageAuxiliaryDataTypeDisparity);
        Some(Read {
            status,
            orientation,
            plane,
            layout,
            apple_map,
            iso_map,
            depth,
            disparity,
        })
    }
}

/// How far apart two planes of one size are, over the channels both have.
struct Difference {
    max: f32,
    /// Where the largest difference is, as `(x, y, channel)`.
    worst: (usize, usize, usize),
    mean: f32,
    /// The share of samples more than two 8-bit codes apart.
    off: f32,
}

impl Difference {
    fn of(a: &Plane, b: &Plane) -> Option<Self> {
        if (a.width, a.height) != (b.width, b.height) {
            return None;
        }
        // A gray picture against an RGB one is gray against each of the
        // three, which is what a reader of the RGB one would have to check
        // before taking one channel back.
        let mut channels: Vec<(usize, usize)> = if a.color == 1 && b.color == 3 {
            vec![(0, 0), (0, 1), (0, 2)]
        } else {
            (0..a.color.min(b.color)).map(|c| (c, c)).collect()
        };
        if a.alpha && b.alpha {
            channels.push((a.color, b.color));
        }
        let (mut max, mut sum, mut off, mut count) = (0f32, 0f64, 0usize, 0usize);
        let mut worst = (0, 0, 0);
        for y in 0..a.height {
            for x in 0..a.width {
                for &(ca, cb) in &channels {
                    let d = (a.at(x, y, ca) - b.at(x, y, cb)).abs();
                    let d = if d.is_nan() { 1.0 } else { d };
                    if d > max {
                        max = d;
                        worst = (x, y, ca);
                    }
                    sum += f64::from(d);
                    off += usize::from(d > 2.0 / 255.0);
                    count += 1;
                }
            }
        }
        Some(Self {
            max,
            worst,
            mean: (sum / count.max(1) as f64) as f32,
            off: off as f32 / count.max(1) as f32,
        })
    }

    fn say(&self) -> String {
        let (x, y, c) = self.worst;
        format!(
            "max {:.2} (at {x},{y} channel {c}) mean {:.3} codes, {:.2}% > 2",
            self.max * 255.0,
            self.mean * 255.0,
            self.off * 100.0
        )
    }
}

/// The correlation of two one-channel planes of one size, for depth maps
/// whose codes and whose floats are the same distances in other units.
fn correlation(a: &Plane, b: &Plane) -> Option<f64> {
    if (a.width, a.height) != (b.width, b.height) || a.data.is_empty() {
        return None;
    }
    let n = a.data.len() as f64;
    let mean = |p: &Plane| p.data.iter().map(|&v| f64::from(v)).sum::<f64>() / n;
    let (ma, mb) = (mean(a), mean(b));
    let (mut cov, mut va, mut vb) = (0.0, 0.0, 0.0);
    for (&x, &y) in a.data.iter().zip(&b.data) {
        let (dx, dy) = (f64::from(x) - ma, f64::from(y) - mb);
        cov += dx * dy;
        va += dx * dx;
        vb += dy * dy;
    }
    Some(cov / (va * vb).sqrt())
}

/// Compared stored and turned alike: an auxiliary image's turn is its own
/// item's business, so whichever of the two lines up is the one said.
fn closest(libheif: &Plane, imageio: &Plane, orientation: i64) -> Option<(Difference, bool)> {
    let stored = Difference::of(libheif, imageio);
    let turned = Difference::of(libheif, &imageio.oriented(orientation));
    match (stored, turned) {
        (Some(s), Some(t)) if t.mean < s.mean => Some((t, true)),
        (Some(s), _) => Some((s, false)),
        (None, Some(t)) => Some((t, true)),
        (None, None) => None,
    }
}

/// The picture a test collection keeps beside a file to say what it should
/// decode to, where it keeps one: `kimono.crop.png` beside
/// `kimono.crop.avif`, or `fox.png` beside `fox.profile0.8bpc.yuv420.avif`
/// for a picture encoded several ways — as AOM's Link-U files are.
fn reference(path: &Path) -> Option<Plane> {
    let name = path.file_name()?.to_str()?;
    let stem = name.rsplit_once('.')?.0;
    let base = name.split('.').next()?;
    let png = [stem, base]
        .iter()
        .map(|s| path.with_file_name(format!("{s}.png")))
        .find(|p| p.exists())?;
    let decoded = ::image::open(png).ok()?;
    let (width, height) = (decoded.width() as usize, decoded.height() as usize);
    let (color, alpha, data): (usize, bool, Vec<f32>) = if decoded.color().has_color() {
        if decoded.color().has_alpha() {
            let rgba = decoded.to_rgba16();
            (
                3,
                true,
                rgba.into_raw()
                    .iter()
                    .map(|&v| f32::from(v) / 65535.0)
                    .collect(),
            )
        } else {
            let rgb = decoded.to_rgb16();
            (
                3,
                false,
                rgb.into_raw()
                    .iter()
                    .map(|&v| f32::from(v) / 65535.0)
                    .collect(),
            )
        }
    } else {
        let gray = decoded.to_luma16();
        (
            1,
            false,
            gray.into_raw()
                .iter()
                .map(|&v| f32::from(v) / 65535.0)
                .collect(),
        )
    };
    Some(Plane {
        width,
        height,
        color,
        alpha,
        data,
    })
}

/// One file compared, as the report says it and as the tests judge it.
struct Comparison {
    name: String,
    lines: Vec<String>,
    same_size: bool,
    same_color: bool,
    pixels: Option<Difference>,
    /// Whether ImageIO handed over a buffer of its own, rather than one
    /// drawn from an image it may not have decoded at all.
    buffered: bool,
    /// `None` where neither found a gain map; `Some(None)` where one did and
    /// the codes could not be compared.
    gain_map: Option<Option<Difference>>,
    /// The same, for the depth map, as the correlation of the two.
    depth: Option<Option<f64>>,
    /// Where the file comes with the picture it should decode to: whether
    /// ImageIO's is as close to it as `libheif`'s, near enough.
    closer: Option<bool>,
    libheif_time: Duration,
    /// `None` where ImageIO never handed a buffer over to be timed.
    imageio_time: Option<Duration>,
}

fn libheif(path: &Path) -> anyhow::Result<(DecodedImage, Duration)> {
    let started = Instant::now();
    let mut file = std::io::BufReader::new(std::fs::File::open(path)?);
    let image = super::Heif.decode(&mut file, Overrides::default())?;
    Ok((image, started.elapsed()))
}

fn compare(path: &Path) -> Comparison {
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    let file = std::fs::read(path).expect("the file");
    let (theirs, libheif_time) = match libheif(path) {
        Ok(read) => read,
        Err(error) => panic!("{name}: libheif refused it: {error:#}"),
    };
    let ours = imageio(&file).unwrap_or_else(|| panic!("{name}: ImageIO refused it"));
    // Each timed twice more, for the fastest of three: the first of either
    // pays for loading its codec.
    let libheif_time = (0..2).fold(libheif_time, |best, _| best.min(libheif(path).unwrap().1));
    let times = (0..3)
        .filter_map(|_| timed(&file))
        .reduce(|(p, a), (tp, ta)| (p.min(tp), a.min(ta)));

    let mut lines = Vec::new();
    let reference = plane_of(&theirs.samples, theirs.width, theirs.height, theirs.alpha);
    let shown = ours.plane.oriented(ours.orientation);

    let same_size = (reference.width, reference.height) == (shown.width, shown.height);
    lines.push(format!(
        "size       libheif {}x{}  imageio {}x{} stored, orientation {} -> {}x{}{}",
        reference.width,
        reference.height,
        ours.plane.width,
        ours.plane.height,
        ours.orientation,
        shown.width,
        shown.height,
        if same_size { "" } else { "  MISMATCH" }
    ));

    if ours.status != 0 || ours.layout.color_space.ends_with("(redrawn)") {
        lines.push(format!(
            "status     ImageIO says {} and handed over no buffer of its own",
            ours.status
        ));
    }
    let same_channels = (reference.color, reference.alpha) == (shown.color, shown.alpha);
    lines.push(format!(
        "layout     libheif {:?} {:?} {:?}  imageio {}{}",
        theirs.samples.channels(),
        match &theirs.samples {
            Samples::U8 { .. } => "u8",
            Samples::U16 { .. } => "u16",
            Samples::F32 { .. } => "f32",
        },
        theirs.alpha,
        ours.layout.describe(),
        if same_channels {
            ""
        } else {
            "  CHANNELS DIFFER"
        }
    ));

    let by_name = named(&ours.layout.color_space);
    let ours_color =
        by_name.unwrap_or_else(|| ColorSpace::stated(None, ours.layout.icc.as_deref()));
    let same_color = ours_color == theirs.color;
    lines.push(format!(
        "color      libheif {}  imageio {} (by its {}){}",
        theirs.color.label(),
        ours_color.label(),
        if by_name.is_some() { "name" } else { "ICC" },
        if same_color { "" } else { "  DIFFERS" }
    ));

    let pixels = Difference::of(&reference, &shown);
    let mut closer = None;
    if let Some(expected) = self::reference(path) {
        let theirs_off = Difference::of(&expected, &reference);
        let ours_off = Difference::of(&expected, &shown);
        let say = |d: &Option<Difference>| {
            d.as_ref().map_or("not compared".into(), |d| {
                format!("mean {:.3}", d.mean * 255.0)
            })
        };
        lines.push(format!(
            "reference  libheif {}  imageio {} codes from the file's own PNG",
            say(&theirs_off),
            say(&ours_off)
        ));
        if let (Some(t), Some(o)) = (&theirs_off, &ours_off) {
            closer = Some(o.mean <= t.mean * 1.05);
        }
    }
    if std::env::var("GAMUT_HEIF_DUMP").is_ok_and(|want| name.contains(&want)) {
        for (x, y) in [(0, 0), (5, 3), (16, 12), (31, 23), (10, 20)] {
            let px = |p: &Plane| {
                (0..p.channels())
                    .map(|c| format!("{:.0}", p.at(x, y, c) * 255.0))
                    .collect::<Vec<_>>()
                    .join(",")
            };
            eprintln!(
                "  dump {x},{y}: libheif [{}] imageio [{}]",
                px(&reference),
                px(&shown)
            );
        }
    }
    lines.push(match &pixels {
        Some(d) => format!("pixels     {}", d.say()),
        None => "pixels     not compared: the sizes differ".into(),
    });

    let theirs_map = theirs.gain_map.as_ref().map(|map| {
        let codes = match &map.data {
            Data::Codes(codes) => codes.iter().map(|&v| f32::from(v) / 255.0).collect(),
            Data::Ratios(ratios) => ratios.iter().map(|v| v.to_f32()).collect(),
        };
        let lift = match map.lift {
            crate::image::gain_map::Lift::Iso(_) => "ISO",
            crate::image::gain_map::Lift::Apple { .. } => "Apple",
            _ => "other",
        };
        (
            Plane {
                width: map.width as usize,
                height: map.height as usize,
                color: map.channels as usize,
                alpha: false,
                data: codes,
            },
            lift,
        )
    });
    let mut gain_map = None;
    if theirs_map.is_some() || ours.iso_map.is_some() || ours.apple_map.is_some() {
        gain_map = Some(None);
        let say = |aux: &Option<Auxiliary>| match aux {
            Some(a) => format!(
                "{}x{}x{} {}",
                a.plane.width, a.plane.height, a.plane.color, a.format
            ),
            None => "none".into(),
        };
        lines.push(format!(
            "gain map   libheif {}  imageio ISO {}, Apple {}",
            theirs_map
                .as_ref()
                .map_or("none".into(), |(p, lift)| format!(
                    "{}x{}x{} {lift}",
                    p.width, p.height, p.color
                )),
            say(&ours.iso_map),
            say(&ours.apple_map),
        ));
        if let Some((plane, lift)) = &theirs_map {
            let candidate = if *lift == "ISO" {
                ours.iso_map.as_ref().or(ours.apple_map.as_ref())
            } else {
                ours.apple_map.as_ref().or(ours.iso_map.as_ref())
            };
            let compared = candidate.and_then(|a| closest(plane, &a.plane, ours.orientation));
            lines.push(match &compared {
                Some((d, turned)) => format!(
                    "           codes {} ({})",
                    d.say(),
                    if *turned { "turned" } else { "as stored" }
                ),
                None => "           codes not compared".into(),
            });
            gain_map = Some(compared.map(|(d, _)| d));
        }
    }

    let mut depth_found = None;
    let ours_depth = ours.depth.as_ref().or(ours.disparity.as_ref());
    if theirs.depth.is_some() || ours_depth.is_some() {
        let theirs_depth = theirs
            .depth
            .as_ref()
            .map(|map| plane_of(&map.samples, map.width, map.height, AlphaMode::Opaque));
        lines.push(format!(
            "depth      libheif {}  imageio {}",
            theirs_depth
                .as_ref()
                .map_or("none".into(), |p| format!("{}x{}", p.width, p.height)),
            ours_depth.map_or("none".into(), |a| format!(
                "{}x{} {}{}",
                a.plane.width,
                a.plane.height,
                a.format,
                if ours.depth.is_some() {
                    " (depth)"
                } else {
                    " (disparity)"
                }
            )),
        ));
        depth_found = Some(None);
        if let (Some(theirs_plane), Some(aux)) = (&theirs_depth, ours_depth) {
            let r = correlation(theirs_plane, &aux.plane)
                .or_else(|| correlation(theirs_plane, &aux.plane.oriented(ours.orientation)));
            depth_found = Some(r);
            let theirs_scale = theirs.depth.as_ref().and_then(|d| d.scale);
            let ours_scale = aux
                .xmp
                .as_deref()
                .and_then(Xmp::parse)
                .and_then(|xmp| depth::scale(&xmp));
            let (low, high) = aux
                .plane
                .data
                .iter()
                .fold((f32::MAX, f32::MIN), |(l, h), &v| (l.min(v), h.max(v)));
            lines.push(format!(
                "           correlation {}  imageio's values {low:.4}..{high:.4}  \
                 scale libheif {:?}  imageio {:?}",
                r.map_or("n/a".into(), |r| format!("{r:.4}")),
                theirs_scale,
                ours_scale
            ));
        }
    }

    let ms = |d: Duration| d.as_secs_f64() * 1e3;
    lines.push(match times {
        Some((picture, all)) => format!(
            "time       libheif {:.1} ms  imageio {:.1} ms ({:.1} ms the picture alone)",
            ms(libheif_time),
            ms(all),
            ms(picture),
        ),
        None => format!(
            "time       libheif {:.1} ms  imageio not timed",
            ms(libheif_time)
        ),
    });

    Comparison {
        name,
        lines,
        same_size,
        same_color,
        pixels,
        buffered: !ours.layout.color_space.ends_with("(redrawn)"),
        gain_map,
        depth: depth_found,
        closer,
        libheif_time,
        imageio_time: times.map(|(_, all)| all),
    }
}

fn report(comparisons: &[Comparison]) {
    for c in comparisons {
        println!("{}", c.name);
        for line in &c.lines {
            println!("  {line}");
        }
    }
    // Over the files both were timed on.
    let timed: Vec<(Duration, Duration)> = comparisons
        .iter()
        .filter_map(|c| c.imageio_time.map(|ours| (c.libheif_time, ours)))
        .collect();
    let (theirs, ours) = timed.iter().fold((0.0, 0.0), |(a, b), (t, o)| {
        (a + t.as_secs_f64(), b + o.as_secs_f64())
    });
    println!(
        "total      libheif {:.0} ms  imageio {:.0} ms over the {} of {} files both were timed on",
        theirs * 1e3,
        ours * 1e3,
        timed.len(),
        comparisons.len()
    );
}

fn fixtures() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir("test_images")
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| matches!(e, "heic" | "heif" | "avif"))
        })
        .collect();
    paths.sort();
    paths
}

/// Every HEIF under `directory`, however deep.
fn walk(directory: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk(&path, found);
        } else if path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "heic" | "heif" | "hif" | "avif"
            )
        }) {
            found.push(path);
        }
    }
}

fn corpus() -> Vec<PathBuf> {
    let Ok(named) = std::env::var("GAMUT_HEIF_CORPUS") else {
        return Vec::new();
    };
    let limit = std::env::var("GAMUT_HEIF_CORPUS_LIMIT")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(20);
    let mut paths = Vec::new();
    for part in named.split(':').map(PathBuf::from) {
        if part.is_dir() {
            let mut found = Vec::new();
            walk(&part, &mut found);
            found.sort();
            paths.extend(found);
        } else {
            paths.push(part);
        }
    }
    paths.truncate(limit);
    paths
}

impl Comparison {
    /// What keeps ImageIO from standing in for `libheif` on this file, in
    /// words; empty where nothing does. `tolerance` is the mean difference
    /// in 8-bit codes the pixels may show, which is none for a lossless
    /// file and something for a lossy one, whose chroma the two readers
    /// reconstruct differently at edges.
    fn problems(&self, tolerance: f32) -> Vec<String> {
        let mut problems = Vec::new();
        if !self.buffered {
            problems.push("ImageIO decoded nothing it would hand over".into());
        }
        if !self.same_size {
            problems.push("the sizes differ".into());
        }
        if !self.same_color {
            problems.push("the color spaces differ".into());
        }
        // A gray picture coming back as RGB is a difference of layout
        // only, which the pixels then judge: each channel against the gray.
        // Where the file says what it should look like, being as close to
        // that as `libheif` is is agreement enough.
        match &self.pixels {
            _ if self.closer == Some(true) => {}
            Some(d) if d.mean * 255.0 > tolerance => problems.push(format!(
                "the pixels are {:.3} codes apart on average",
                d.mean * 255.0
            )),
            None => problems.push("the pixels could not be compared".into()),
            _ => {}
        }
        match &self.gain_map {
            Some(Some(d)) if d.max > 0.0 => problems.push("the gain maps differ".into()),
            Some(None) => problems.push("only one reader found the gain map".into()),
            _ => {}
        }
        match self.depth {
            Some(Some(r)) if r < 0.9999 => {
                problems.push(format!("the depth maps correlate at only {r:.4}"))
            }
            Some(None) => problems.push("only one reader found the depth map".into()),
            _ => {}
        }
        problems
    }
}

/// The fixtures, read both ways. Every one agrees exactly, but for two
/// whose differences are known: `heic-icc-p3.heic`, the one fixture coded
/// lossy at 4:2:0, whose chroma the readers reconstruct differently along
/// its hard edges; and `heic-gray-alpha8.heic`, a gray picture with an
/// alpha plane, which ImageIO does not decode at all — nor does `sips`,
/// nor Preview. Should either change, this says so.
#[test]
fn imageio_reads_the_fixtures_as_libheif_does() {
    let comparisons: Vec<Comparison> = fixtures().iter().map(|path| compare(path)).collect();
    report(&comparisons);
    for c in &comparisons {
        let tolerance = if c.name == "heic-icc-p3.heic" {
            6.0
        } else {
            0.0
        };
        let problems = c.problems(tolerance);
        if c.name == "heic-gray-alpha8.heic" {
            assert!(
                !c.buffered,
                "ImageIO now decodes a gray HEIC with alpha: {problems:?}"
            );
            continue;
        }
        assert!(problems.is_empty(), "{}: {}", c.name, problems.join("; "));
        if c.name != "heic-icc-p3.heic" {
            assert_eq!(
                c.pixels.as_ref().map(|d| d.max),
                Some(0.0),
                "{}: a lossless file decodes differently",
                c.name
            );
        }
    }
}

/// Real files, where `GAMUT_HEIF_CORPUS` names some: every one compared,
/// and every one where ImageIO could not stand in for `libheif` named at
/// the end, with why. A phone's photograph is lossy at 4:2:0, so its pixels
/// may differ by up to `GAMUT_HEIF_TOLERANCE` codes on average (1.5 unless
/// said) where the chroma's edges are reconstructed differently.
#[test]
fn imageio_reads_the_corpus_as_libheif_does() {
    let paths = corpus();
    if paths.is_empty() {
        return;
    }
    let tolerance = std::env::var("GAMUT_HEIF_TOLERANCE")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(1.5);
    // A file either reader refuses, or that this comparison cannot read
    // ImageIO's buffer of, is a finding rather than the end of the run.
    let mut refused = Vec::new();
    let comparisons: Vec<Comparison> = paths
        .iter()
        .filter_map(|path| {
            std::panic::catch_unwind(|| compare(path))
                .map_err(|panic| {
                    let said = panic
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_default();
                    refused.push(format!("{}: {said}", path.display()));
                })
                .ok()
        })
        .collect();
    report(&comparisons);
    let failing: Vec<String> = comparisons
        .iter()
        .filter_map(|c| {
            let problems = c.problems(tolerance);
            (!problems.is_empty()).then(|| format!("{}: {}", c.name, problems.join("; ")))
        })
        .chain(refused)
        .collect();
    assert!(
        failing.is_empty(),
        "{} of {} files:\n{}",
        failing.len(),
        paths.len(),
        failing.join("\n")
    );
}
