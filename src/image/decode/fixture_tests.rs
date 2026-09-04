//! Decoder coverage against the files in `test_images/`.
//!
//! Round-tripping through the `image` crate's own encoder only proves the
//! crate agrees with itself. These are real files written by ImageMagick,
//! covering every pixel layout the decoder can hand back and every per-format
//! encoding that has its own code path — bit depths, palettes, interlacing,
//! progressive JPEG, TIFF compressions, byte orders and tiling.
//!
//! Every fixture is the same pattern of four quadrants, so one table of
//! expected values serves all of them. See `test_images/generate.sh`.

use std::path::PathBuf;

use super::{Overrides, load, probe, supported_extensions};
use crate::image::{AlphaMode, Channels, ColorSpace, DecodedImage, Samples};

/// Centre of each quadrant, in the order the expectation tables use.
const PROBES: [(u32, u32); 4] = [(8, 6), (24, 6), (8, 18), (24, 18)];

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Kind {
    U8,
    U16,
    F32,
}

/// What the quadrants hold, before alpha.
#[derive(Clone, Copy)]
enum Tone {
    /// Red, green, blue, white.
    Color,
    /// 0, 1/3, 2/3, 1.
    Gray,
    /// What a one-bit image can represent of the grey pattern: the two middle
    /// steps round to the ends.
    GrayBilevel,
    /// 0, 0.5, 1.0, 255/64 — the last one deliberately above SDR range.
    Float,
    /// As `Float`, but the first quadrant holds the no-data sentinel.
    FloatWithNodata,
    /// Signed 16-bit elevations, scaled to span negative and positive.
    Int16,
    /// As `Color`, but the last quadrant carries no color at all. A GIF's
    /// transparency is a palette index rather than a channel, so the pixel
    /// behind it has nothing to hold a color in and the decoder hands back
    /// four zeroes where that index sat. A PNG's `tRNS` keeps the color
    /// under the hole, which is why `Color` cannot serve both.
    ColorLastCleared,
}

/// What the alpha channel holds, where there is one.
#[derive(Clone, Copy)]
enum Coverage {
    Opaque,
    /// 1, 0.749, 0.502, 0.251.
    Ramp,
    /// A palette's `tRNS` is quantised to all-or-nothing by the encoder.
    BinaryLastTransparent,
}

struct Fixture {
    file: &'static str,
    /// The decode path this file exists to cover.
    covers: &'static str,
    channels: Channels,
    kind: Kind,
    color: ColorSpace,
    alpha: AlphaMode,
    tone: Tone,
    coverage: Coverage,
    /// The no-data sentinel the decoder should have picked up, if any.
    nodata: Option<f32>,
    tolerance: f32,
}

/// Exact for lossless integer formats: 85/255 and 21845/65535 are both a
/// third, so one table covers 8- and 16-bit alike.
const EXACT: f32 = 1e-5;
/// JPEG moves pure primaries by a code value or two.
const LOSSY: f32 = 0.02;
/// Radiance packs a shared exponent and an 8-bit mantissa.
const RGBE: f32 = 0.01;
/// HEVC at quality 100 is near-lossless rather than lossless, and lands a
/// code value away. Only the one fixture ImageMagick has to write, because
/// `heif-enc` cannot embed an ICC profile.
const NEAR_LOSSLESS: f32 = 0.01;

const SRGB: ColorSpace = ColorSpace::SRGB;
const LINEAR: ColorSpace = ColorSpace::LINEAR_BT709;
/// What a phone writes: sRGB's curve on the wider Display P3 primaries.
const P3: ColorSpace = ColorSpace {
    transfer: crate::image::Transfer::Srgb,
    primaries: crate::image::Primaries::DisplayP3,
};
/// BT.2100 HDR, stated outright in the file's CICP tags.
const PQ_2020: ColorSpace = ColorSpace {
    transfer: crate::image::Transfer::Pq,
    primaries: crate::image::Primaries::Bt2020,
};

const FIXTURES: &[Fixture] = &[
    // ------------------------------------------------------------- PNG
    Fixture {
        file: "png-gray8.png",
        covers: "PNG greyscale, 8-bit",
        channels: Channels::Gray,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Gray,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "png-gray-alpha8.png",
        covers: "PNG greyscale plus alpha, 8-bit",
        channels: Channels::GrayAlpha,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Straight,
        tone: Tone::Gray,
        coverage: Coverage::Ramp,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "png-rgb8.png",
        covers: "PNG truecolor, 8-bit",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "png-rgba8.png",
        covers: "PNG truecolor plus alpha, 8-bit",
        channels: Channels::Rgba,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Straight,
        tone: Tone::Color,
        coverage: Coverage::Ramp,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "png-gray16.png",
        covers: "PNG greyscale, 16-bit",
        channels: Channels::Gray,
        kind: Kind::U16,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Gray,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "png-gray-alpha16.png",
        covers: "PNG greyscale plus alpha, 16-bit",
        channels: Channels::GrayAlpha,
        kind: Kind::U16,
        color: SRGB,
        alpha: AlphaMode::Straight,
        tone: Tone::Gray,
        coverage: Coverage::Ramp,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "png-rgb16.png",
        covers: "PNG truecolor, 16-bit",
        channels: Channels::Rgb,
        kind: Kind::U16,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "png-rgba16.png",
        covers: "PNG truecolor plus alpha, 16-bit",
        channels: Channels::Rgba,
        kind: Kind::U16,
        color: SRGB,
        alpha: AlphaMode::Straight,
        tone: Tone::Color,
        coverage: Coverage::Ramp,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "png-gray1.png",
        covers: "PNG sub-byte bit depth, 1-bit",
        channels: Channels::Gray,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::GrayBilevel,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "png-gray4.png",
        covers: "PNG sub-byte bit depth, 4-bit",
        channels: Channels::Gray,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Gray,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "png-palette.png",
        covers: "PNG indexed color, expanded on decode",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "png-palette-alpha.png",
        covers: "PNG indexed color with a tRNS chunk",
        channels: Channels::Rgba,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Straight,
        tone: Tone::Color,
        coverage: Coverage::BinaryLastTransparent,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "png-interlaced.png",
        covers: "PNG Adam7 interlacing",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    // A PNG says it is HDR with a `cICP` chunk and nothing else, so this is
    // the fixture standing between the HDR PNG path and silence.
    Fixture {
        file: "png-cicp-pq.png",
        covers: "PNG `cICP`: BT.2100 PQ on BT.2020 primaries",
        channels: Channels::Rgb,
        kind: Kind::U16,
        color: PQ_2020,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "png-icc-p3.png",
        covers: "PNG `iCCP`: Display P3 stated by profile rather than code points",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: P3,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    // ------------------------------------------------------------ JPEG
    Fixture {
        file: "jpeg-rgb.jpg",
        covers: "JPEG baseline, no chroma subsampling",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: LOSSY,
    },
    Fixture {
        file: "jpeg-gray.jpg",
        covers: "JPEG single-component greyscale",
        channels: Channels::Gray,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Gray,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: LOSSY,
    },
    Fixture {
        file: "jpeg-progressive.jpeg",
        covers: "JPEG progressive scan order, and the `.jpeg` extension",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: LOSSY,
    },
    Fixture {
        file: "jpeg-subsampled.jpg",
        covers: "JPEG 4:2:0 chroma subsampling",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: LOSSY,
    },
    // ------------------------------------------------------------- GIF
    // Always a palette and always 8-bit, and always RGBA once decoded: the
    // crate's GIF decoder has one output layout, and the transparent index
    // has to go somewhere.
    Fixture {
        file: "gif-palette.gif",
        covers: "GIF palette, no transparent index",
        channels: Channels::Rgba,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Straight,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    // The rows stored in four passes rather than in order, GIF's counterpart
    // of Adam7.
    Fixture {
        file: "gif-interlaced.gif",
        covers: "GIF interlaced row order",
        channels: Channels::Rgba,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Straight,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    // White is the transparent index here, so the last quadrant comes back
    // cleared rather than white behind a hole: see `Tone::ColorLastCleared`.
    Fixture {
        file: "gif-transparent.gif",
        covers: "GIF transparent palette index",
        channels: Channels::Rgba,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Straight,
        tone: Tone::ColorLastCleared,
        coverage: Coverage::BinaryLastTransparent,
        nodata: None,
        tolerance: EXACT,
    },
    // Two frames, the pattern first and an upside-down one second. Passing
    // this table means the first frame is the one shown.
    Fixture {
        file: "gif-animated.gif",
        covers: "animated GIF, first frame onto the logical screen",
        channels: Channels::Rgba,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Straight,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    // ------------------------------------------------------------ TIFF
    Fixture {
        file: "tiff-gray8.tif",
        covers: "TIFF greyscale, 8-bit — guessed display-referred",
        channels: Channels::Gray,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Gray,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "tiff-rgb8.tif",
        covers: "TIFF truecolor, 8-bit",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "tiff-rgba8.tif",
        covers: "TIFF truecolor plus alpha, 8-bit",
        channels: Channels::Rgba,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Straight,
        tone: Tone::Color,
        coverage: Coverage::Ramp,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "tiff-gray16.tif",
        covers: "TIFF greyscale, 16-bit — guessed scene-referred",
        channels: Channels::Gray,
        kind: Kind::U16,
        color: LINEAR,
        alpha: AlphaMode::Opaque,
        tone: Tone::Gray,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "tiff-rgb16.tif",
        covers: "TIFF truecolor, 16-bit",
        channels: Channels::Rgb,
        kind: Kind::U16,
        color: LINEAR,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "tiff-float32.tif",
        covers: "TIFF 32-bit floating point samples",
        channels: Channels::Rgb,
        kind: Kind::F32,
        color: LINEAR,
        alpha: AlphaMode::Opaque,
        tone: Tone::Float,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "tiff-lzw.tif",
        covers: "TIFF LZW compression",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "tiff-deflate.tiff",
        covers: "TIFF Deflate compression, and the `.tiff` extension",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "tiff-packbits.tif",
        covers: "TIFF PackBits compression",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "tiff-bigendian.tif",
        covers: "TIFF big-endian byte order",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "tiff-tiled.tif",
        covers: "TIFF tiled rather than stripped layout",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    // TIFF as it turns up in mapping and science: single band, floating
    // point, and encodings `image` cannot read at all.
    Fixture {
        file: "tiff-bigtiff.tif",
        covers: "BigTIFF — a different magic number, invisible to image's sniffer",
        channels: Channels::Gray,
        kind: Kind::F32,
        color: LINEAR,
        alpha: AlphaMode::Opaque,
        tone: Tone::Float,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "tiff-float-predictor.tif",
        covers: "Deflate with the floating-point predictor, tiled — how DEMs ship",
        channels: Channels::Gray,
        kind: Kind::F32,
        color: LINEAR,
        alpha: AlphaMode::Opaque,
        tone: Tone::Float,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "tiff-int16.tif",
        covers: "signed 16-bit samples, widened to float with negatives intact",
        channels: Channels::Gray,
        kind: Kind::F32,
        color: LINEAR,
        alpha: AlphaMode::Opaque,
        tone: Tone::Int16,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "tiff-nodata.tif",
        covers: "a GDAL no-data sentinel, which must not reach the display window",
        channels: Channels::Gray,
        kind: Kind::F32,
        color: LINEAR,
        alpha: AlphaMode::Opaque,
        tone: Tone::FloatWithNodata,
        coverage: Coverage::Opaque,
        nodata: Some(-9999.0),
        tolerance: EXACT,
    },
    // -------------------------------------------------------- Radiance
    Fixture {
        file: "hdr-rgbe.hdr",
        covers: "Radiance RGBE, shared exponent",
        channels: Channels::Rgb,
        kind: Kind::F32,
        color: LINEAR,
        alpha: AlphaMode::Opaque,
        tone: Tone::Float,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: RGBE,
    },
    // --------------------------------------------------------- OpenEXR
    Fixture {
        file: "exr-rgb.exr",
        covers: "OpenEXR without alpha",
        channels: Channels::Rgb,
        kind: Kind::F32,
        color: LINEAR,
        alpha: AlphaMode::Opaque,
        tone: Tone::Float,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "exr-rgba.exr",
        covers: "OpenEXR with associated (premultiplied) alpha",
        channels: Channels::Rgba,
        kind: Kind::F32,
        color: LINEAR,
        alpha: AlphaMode::Premultiplied,
        tone: Tone::Float,
        coverage: Coverage::Ramp,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "exr-zip.exr",
        covers: "OpenEXR zip compression",
        channels: Channels::Rgb,
        kind: Kind::F32,
        color: LINEAR,
        alpha: AlphaMode::Opaque,
        tone: Tone::Float,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    // ------------------------------------------------------------ HEIF
    Fixture {
        file: "heic-rgb8.heic",
        covers: "HEIC truecolor, 8-bit",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "heic-rgba8.heic",
        covers: "HEIC truecolor plus alpha, 8-bit",
        channels: Channels::Rgba,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Straight,
        tone: Tone::Color,
        coverage: Coverage::Ramp,
        nodata: None,
        tolerance: EXACT,
    },
    // Monochrome HEIF decodes through its own path, and has to stay one
    // channel rather than being tripled into RGB on the way to the GPU.
    Fixture {
        file: "heic-gray8.heic",
        covers: "HEIC monochrome, 8-bit",
        channels: Channels::Gray,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Gray,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    // Grey and alpha arrive as two separate planes here, not interleaved.
    Fixture {
        file: "heic-gray-alpha8.heic",
        covers: "HEIC monochrome plus a separate alpha plane, 8-bit",
        channels: Channels::GrayAlpha,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Straight,
        tone: Tone::Gray,
        coverage: Coverage::Ramp,
        nodata: None,
        tolerance: EXACT,
    },
    // 10-bit samples arrive as 0..1023 in a 16-bit word and have to be lifted
    // to full scale, or the picture displays a sixteenth as bright as it is.
    Fixture {
        file: "heic-pq10.heic",
        covers: "HEIC 10-bit, tagged BT.2100 PQ on BT.2020 primaries",
        channels: Channels::Rgb,
        kind: Kind::U16,
        color: PQ_2020,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "heif-p3.heif",
        covers: "HEIF Display P3 primaries, and the `.heif` spelling",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: P3,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    // The same color space said the other way: an ICC profile with no
    // `nclx` box beside it, which is what some cameras write and what used to
    // read as plain sRGB.
    Fixture {
        file: "heic-icc-p3.heic",
        covers: "HEIF tagged by ICC profile rather than by `nclx`",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: P3,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: NEAR_LOSSLESS,
    },
    // Stored upside down with an `irot` property saying so. It reads as the
    // ordinary pattern only because the transformation is applied on decode.
    Fixture {
        file: "heic-rotated.heic",
        covers: "HEIC `irot` applied while decoding",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "avif-rgb8.avif",
        covers: "the same container with AV1 inside instead of HEVC",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    // ------------------------------------------------------------ WebP
    Fixture {
        file: "webp-lossless-rgb8.webp",
        covers: "WebP lossless (VP8L), no alpha",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    // The alpha here is stated by a bit in the VP8L header rather than by an
    // extended container, which is the one place WebP hides it.
    Fixture {
        file: "webp-lossless-rgba8.webp",
        covers: "WebP lossless with alpha, no `VP8X`",
        channels: Channels::Rgba,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Straight,
        tone: Tone::Color,
        coverage: Coverage::Ramp,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "webp-lossy-rgb8.webp",
        covers: "WebP lossy (VP8), through YCbCr 4:2:0",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: LOSSY,
    },
    // Lossy plus alpha is two bitstreams: an `ALPH` chunk for the coverage
    // and a `VP8` chunk for the color, which only the extended container can
    // hold together.
    Fixture {
        file: "webp-lossy-rgba8.webp",
        covers: "WebP lossy with an `ALPH` chunk beside it",
        channels: Channels::Rgba,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Straight,
        tone: Tone::Color,
        coverage: Coverage::Ramp,
        nodata: None,
        tolerance: LOSSY,
    },
    // The only thing a WebP has to say about its own color.
    Fixture {
        file: "webp-icc-p3.webp",
        covers: "WebP `ICCP` chunk naming Display P3",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: P3,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    // Stored upside down with an `EXIF` chunk saying so, the WebP counterpart
    // of `heic-rotated.heic`.
    Fixture {
        file: "webp-exif-rotated.webp",
        covers: "WebP EXIF orientation applied while decoding",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    // Two frames, the pattern first and a rotated one second. Passing this
    // table means the first frame was the one composited onto the canvas.
    Fixture {
        file: "webp-animated.webp",
        covers: "animated WebP, first frame onto the `ANIM` canvas",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    // ------------------------------------------------------------- ICO
    // The two formats an entry can hold, and what each one costs. A bitmap
    // entry always comes back RGBA whatever its stored depth, because the
    // AND mask that carries an icon's transparency has nowhere else to go.
    Fixture {
        file: "ico-bmp-rgba8.ico",
        covers: "ICO bitmap entry, 32-bit with alpha",
        channels: Channels::Rgba,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Straight,
        tone: Tone::Color,
        coverage: Coverage::Ramp,
        nodata: None,
        tolerance: EXACT,
    },
    // A 4-bit palette with a fully opaque AND mask beside it: the shallow
    // bitmap path, and the one an icon written before 32-bit color takes.
    Fixture {
        file: "ico-bmp-palette.ico",
        covers: "ICO bitmap entry, 4-bit palette plus AND mask",
        channels: Channels::Rgba,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Straight,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    // How every entry above 48 pixels has been stored since Vista.
    Fixture {
        file: "ico-png-rgba8.ico",
        covers: "ICO PNG entry",
        channels: Channels::Rgba,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Straight,
        tone: Tone::Color,
        coverage: Coverage::Ramp,
        nodata: None,
        tolerance: EXACT,
    },
    // A layout `image`'s own ICO decoder refuses outright, on the strength of
    // a note saying embedded PNGs must be 32-bit. Passing as `Gray` means the
    // entry went through the PNG path whole rather than being taken for
    // icon pixels.
    Fixture {
        file: "ico-png-gray8.ico",
        covers: "ICO PNG entry that is not RGBA",
        channels: Channels::Gray,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Gray,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    // The same path's other payoff: a PNG entry's `iCCP` chunk is read, so an
    // icon can say it is Display P3 like any other PNG.
    Fixture {
        file: "ico-png-icc-p3.ico",
        covers: "ICO PNG entry carrying an `iCCP` profile",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: P3,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    // Two entries: the pattern at 32x24 in 4 bits, and a 16x12 thumbnail in
    // 32. Passing this table at all means the larger one was chosen — see
    // `the_largest_ico_entry_is_the_one_shown`.
    Fixture {
        file: "ico-multi.ico",
        covers: "ICO directory of several sizes",
        channels: Channels::Rgba,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Straight,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    // ------------------------------------------------------------- BMP
    Fixture {
        file: "bmp-rgb8.bmp",
        covers: "BMP 24-bit, bottom-up rows",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    // Carrying alpha is what forces the `BITMAPV5HEADER` and the bitfield
    // masks that describe where each channel sits in the word.
    Fixture {
        file: "bmp-rgba8.bmp",
        covers: "BMP 32-bit with bitfield masks and alpha",
        channels: Channels::Rgba,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Straight,
        tone: Tone::Color,
        coverage: Coverage::Ramp,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "bmp-palette.bmp",
        covers: "BMP 4-bit palette",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    // The other palette width, and with it the run-length coding that only a
    // palette can use.
    Fixture {
        file: "bmp-rle8.bmp",
        covers: "BMP 8-bit palette, `BI_RLE8` runs",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    // The same picture with its rows reversed and a negative height saying
    // so, which is how screen capture writes one. Passing this table means
    // the sign was honoured; ignoring it would turn the picture upside down.
    Fixture {
        file: "bmp-topdown.bmp",
        covers: "BMP top-down row order",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    // ---------------------------------------------------------- netpbm
    Fixture {
        file: "pnm-rgb8.ppm",
        covers: "netpbm binary PPM, 8-bit",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    Fixture {
        file: "pnm-gray8.pgm",
        covers: "netpbm binary PGM, 8-bit",
        channels: Channels::Gray,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Gray,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    // 16-bit samples, which netpbm stores big-endian whatever wrote them.
    Fixture {
        file: "pnm-rgb16.ppm",
        covers: "netpbm binary PPM, 16-bit big-endian samples",
        channels: Channels::Rgb,
        kind: Kind::U16,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    // The ASCII member of the family, under the extension that names no
    // member in particular.
    Fixture {
        file: "pnm-ascii.pnm",
        covers: "netpbm ASCII PPM, and the generic `.pnm` extension",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    // `MAXVAL 1023` in a 16-bit word, which has to be lifted to 65535 or the
    // picture shows at a sixteenth of its brightness. Passing the ordinary
    // grey table is what says it was.
    Fixture {
        file: "pnm-maxval1023.pgm",
        covers: "netpbm PGM whose MAXVAL is not the sample width",
        channels: Channels::Gray,
        kind: Kind::U16,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Gray,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    // `MAXVAL 1` at the other end, where a sample is one bit and white is
    // whatever the lift makes of it.
    Fixture {
        file: "pnm-bilevel.pbm",
        covers: "netpbm bitmap, one bit per pixel",
        channels: Channels::Gray,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::GrayBilevel,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
    // PAM generalises the three above, states its fields as keyword lines
    // rather than bare numbers, and is the only one that carries alpha.
    Fixture {
        file: "pnm-rgba8.pam",
        covers: "netpbm PAM with an alpha channel",
        channels: Channels::Rgba,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Straight,
        tone: Tone::Color,
        coverage: Coverage::Ramp,
        nodata: None,
        tolerance: EXACT,
    },
    // A PNG under a TIFF name, decoded by sniffing rather than extension.
    Fixture {
        file: "mislabelled.tif",
        covers: "content sniffing when the extension lies",
        channels: Channels::Rgb,
        kind: Kind::U8,
        color: SRGB,
        alpha: AlphaMode::Opaque,
        tone: Tone::Color,
        coverage: Coverage::Opaque,
        nodata: None,
        tolerance: EXACT,
    },
];

/// Files that are meant to fail, and the phrase the failure should contain.
const REJECTED: &[(&str, &str)] = &[
    ("unsupported.tga", "unsupported image format"),
    ("bad-truncated.png", "decoding"),
];

/// Extensions the registry advertises that share a decode path with another
/// fixture and so do not need one of their own.
const ALIASES: &[&str] = &["jpe", "jfif", "hif"];

fn directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_images")
}

fn kind_of(samples: &Samples) -> Kind {
    match samples {
        Samples::U8 { .. } => Kind::U8,
        Samples::U16 { .. } => Kind::U16,
        Samples::F32 { .. } => Kind::F32,
    }
}

/// One pixel's components, integers normalised to 0..1 so that 8- and 16-bit
/// fixtures can share a table of expected values.
fn pixel(image: &DecodedImage, x: u32, y: u32) -> Vec<f32> {
    let count = image.samples.channels().count();
    let start = (y as usize * image.width as usize + x as usize) * count;
    let scale = 1.0 / image.samples.full_scale();
    match &image.samples {
        Samples::U8 { data, .. } => data[start..start + count]
            .iter()
            .map(|value| *value as f32 * scale)
            .collect(),
        Samples::U16 { data, .. } => data[start..start + count]
            .iter()
            .map(|value| *value as f32 * scale)
            .collect(),
        Samples::F32 { data, .. } => data[start..start + count].to_vec(),
    }
}

/// Expected RGBA for each quadrant. Grey fixtures put their value in the red
/// slot, which is where the comparison looks for them.
fn expected(tone: Tone, coverage: Coverage) -> [[f32; 4]; 4] {
    let third = 1.0 / 3.0;
    let mut quadrants = match tone {
        Tone::Color => [
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 1.0],
            [0.0, 0.0, 1.0, 1.0],
            [1.0, 1.0, 1.0, 1.0],
        ],
        Tone::Gray => [[0.0; 4], [third; 4], [2.0 * third; 4], [1.0; 4]],
        Tone::GrayBilevel => [[0.0; 4], [0.0; 4], [1.0; 4], [1.0; 4]],
        Tone::Float => {
            let peak = 255.0 / 64.0;
            [[0.0; 4], [0.5; 4], [1.0; 4], [peak; 4]]
        }
        Tone::FloatWithNodata => {
            let peak = 255.0 / 64.0;
            [[-9999.0; 4], [0.5; 4], [1.0; 4], [peak; 4]]
        }
        Tone::Int16 => [[-1000.0; 4], [-498.0; 4], [4.0; 4], [3000.0; 4]],
        Tone::ColorLastCleared => [
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 1.0],
            [0.0, 0.0, 1.0, 1.0],
            [0.0, 0.0, 0.0, 0.0],
        ],
    };

    let alphas = match coverage {
        Coverage::Opaque => [1.0; 4],
        Coverage::Ramp => [1.0, 191.0 / 255.0, 128.0 / 255.0, 64.0 / 255.0],
        Coverage::BinaryLastTransparent => [1.0, 1.0, 1.0, 0.0],
    };
    for (quadrant, alpha) in quadrants.iter_mut().zip(alphas) {
        quadrant[3] = alpha;
    }
    quadrants
}

#[test]
/// The window opens at the size the probe reports, before a single pixel has
/// been decoded — so a probe that disagrees with its own decoder opens the
/// window in the wrong shape. Anything the header will not say is `None` and
/// falls back to a default, which is fine; saying the wrong thing is not.
fn every_fixture_probes_to_the_size_it_decodes_to() {
    for fixture in FIXTURES {
        let path = directory().join(fixture.file);
        let image = load(&path, Overrides::default())
            .unwrap_or_else(|error| panic!("{}: {error:#}", fixture.file));
        let probed = probe(&path).unwrap_or_else(|error| panic!("{}: {error:#}", fixture.file));

        if let Some(size) = probed {
            assert_eq!(
                size,
                (image.width, image.height),
                "{} ({}) probes to a different size than it decodes to",
                fixture.file,
                fixture.covers
            );
        }
    }
}

#[test]
fn every_fixture_decodes_to_what_it_says_it_does() {
    for fixture in FIXTURES {
        let path = directory().join(fixture.file);
        let image = load(&path, Overrides::default())
            .unwrap_or_else(|error| panic!("{}: {error:#}", fixture.file));

        let name = format!("{} ({})", fixture.file, fixture.covers);
        assert_eq!((image.width, image.height), (32, 24), "{name}");
        assert_eq!(image.channels(), fixture.channels, "{name}");
        assert_eq!(kind_of(&image.samples), fixture.kind, "{name}");
        assert_eq!(image.color.transfer, fixture.color.transfer, "{name}");
        assert_eq!(image.color.primaries, fixture.color.primaries, "{name}");
        assert_eq!(image.alpha, fixture.alpha, "{name}");
        assert_eq!(image.nodata, fixture.nodata, "{name}");
        image
            .validate()
            .unwrap_or_else(|problem| panic!("{name}: {problem}"));

        let table = expected(fixture.tone, fixture.coverage);
        for (index, (x, y)) in PROBES.iter().copied().enumerate() {
            let found = pixel(&image, x, y);
            let want = table[index];

            // Grey keeps its value in the red slot; alpha, where present, is
            // always the last component.
            let color_slots = if fixture.channels.is_gray() { 1 } else { 3 };
            for slot in 0..color_slots {
                assert!(
                    (found[slot] - want[slot]).abs() <= fixture.tolerance,
                    "{name} quadrant {index} channel {slot}: got {found:?}, want {want:?}"
                );
            }
            if let Some(alpha) = fixture.channels.alpha_index() {
                assert!(
                    (found[alpha] - want[3]).abs() <= fixture.tolerance,
                    "{name} quadrant {index} alpha: got {found:?}, want {want:?}"
                );
            }
        }
    }
}

#[test]
fn rejected_fixtures_fail_with_a_useful_message() {
    for (file, phrase) in REJECTED {
        let path = directory().join(file);
        match load(&path, Overrides::default()) {
            Ok(_) => panic!("{file} should not have decoded"),
            Err(error) => {
                let message = format!("{error:#}");
                assert!(
                    message.contains(phrase),
                    "{file}: expected `{phrase}` in `{message}`"
                );
            }
        }
    }
}

/// Guards against a fixture being added without a test, or a test outliving
/// its fixture.
#[test]
fn the_fixture_directory_and_the_table_agree() {
    let mut on_disk: Vec<String> = std::fs::read_dir(directory())
        .expect("test_images/ is missing")
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        // `.icc` is an input to the generator, not a fixture in its own right.
        .filter(|name| !name.ends_with(".sh") && !name.ends_with(".md") && !name.ends_with(".icc"))
        .collect();
    on_disk.sort();

    let mut claimed: Vec<String> = FIXTURES
        .iter()
        .map(|fixture| fixture.file.to_string())
        .chain(REJECTED.iter().map(|(file, _)| file.to_string()))
        .collect();
    claimed.sort();

    assert_eq!(
        on_disk, claimed,
        "test_images/ and the fixture tables have drifted apart"
    );
}

/// Every advertised extension is either exercised by a fixture or explicitly
/// noted as sharing another one's path, so adding a format cannot quietly go
/// untested.
#[test]
fn every_advertised_extension_is_covered() {
    for extension in supported_extensions() {
        let covered = FIXTURES.iter().any(|fixture| {
            std::path::Path::new(fixture.file)
                .extension()
                .and_then(|value| value.to_str())
                == Some(extension)
        });
        assert!(
            covered || ALIASES.contains(&extension),
            "`{extension}` has no fixture and is not listed as an alias"
        );
    }
}

/// The fixtures between them must reach every branch of the data model, or
/// the upload layer has untested inputs.
#[test]
fn the_fixtures_reach_every_pixel_layout() {
    let mut channels = std::collections::HashSet::new();
    let mut kinds = std::collections::HashSet::new();
    let mut alphas = std::collections::HashSet::new();
    for fixture in FIXTURES {
        channels.insert(fixture.channels);
        kinds.insert(fixture.kind);
        alphas.insert(fixture.alpha);
    }

    for wanted in [
        Channels::Gray,
        Channels::GrayAlpha,
        Channels::Rgb,
        Channels::Rgba,
    ] {
        assert!(channels.contains(&wanted), "no fixture is {wanted:?}");
    }
    for wanted in [Kind::U8, Kind::U16, Kind::F32] {
        assert!(kinds.contains(&wanted), "no fixture is {wanted:?}");
    }
    for wanted in [
        AlphaMode::Opaque,
        AlphaMode::Straight,
        AlphaMode::Premultiplied,
    ] {
        assert!(alphas.contains(&wanted), "no fixture is {wanted:?}");
    }
}

/// A no-data sentinel is not a measurement. Left in, it would set the bottom
/// of the automatic window and squash the real data into a sliver at the top —
/// which is how a clipped elevation model comes out looking blank.
#[test]
fn no_data_pixels_are_kept_out_of_the_statistics() {
    let image = load(&directory().join("tiff-nodata.tif"), Overrides::default()).unwrap();
    assert_eq!(image.nodata, Some(-9999.0));

    // The sentinel is still in the pixels, where the shader will clamp it.
    assert_eq!(pixel(&image, 8, 6)[0], -9999.0);

    // But the range comes from the quadrants that hold real values.
    let stats = crate::image::Stats::scan(&image);
    assert!((stats.min - 0.5).abs() < 1e-5, "min was {}", stats.min);
    assert!(
        (stats.max - 255.0 / 64.0).abs() < 1e-5,
        "max was {}",
        stats.max
    );
}

/// A 16-bit TIFF is guessed to be measurement data. When the guess is wrong —
/// a scanned photograph in the same container — `--transfer` is the way out,
/// and it has to reach the decoded image.
#[test]
fn command_line_overrides_replace_the_guess() {
    let path = directory().join("tiff-gray16.tif");

    let guessed = load(&path, Overrides::default()).unwrap();
    assert_eq!(guessed.color.transfer, crate::image::Transfer::Linear);
    assert_eq!(guessed.color.primaries, crate::image::Primaries::Bt709);

    let overridden = load(
        &path,
        Overrides {
            transfer: Some(crate::image::Transfer::Srgb),
            primaries: Some(crate::image::Primaries::DisplayP3),
            ..Overrides::default()
        },
    )
    .unwrap();
    assert_eq!(overridden.color.transfer, crate::image::Transfer::Srgb);
    assert_eq!(
        overridden.color.primaries,
        crate::image::Primaries::DisplayP3
    );

    // The pixels are untouched: an override changes interpretation only.
    assert_eq!(pixel(&guessed, 24, 6), pixel(&overridden, 24, 6));
}

/// Whatever the source layout, the upload layer has to produce a buffer the
/// texture can accept. This runs the real fixtures through it.
#[test]
fn every_fixture_produces_a_valid_upload_plan() {
    use crate::render::upload::{Capabilities, plan};

    for capabilities in [
        Capabilities {
            norm16: true,
            float32_filterable: true,
        },
        Capabilities {
            norm16: false,
            float32_filterable: false,
        },
    ] {
        for fixture in FIXTURES {
            let image = load(&directory().join(fixture.file), Overrides::default()).unwrap();
            let plan = plan(&image, capabilities);
            assert_eq!(
                plan.pixels.as_bytes().len(),
                plan.bytes_per_row as usize * image.height as usize,
                "{} {capabilities:?}",
                fixture.file
            );
        }
    }
}

/// HEIF stores rotation and mirroring as container properties rather than in
/// the coded picture, so a phone photograph is upright only if they are
/// applied on the way out. `heic-rotated.heic` holds the ordinary pattern
/// upside down with an `irot` saying as much, so it should decode to exactly
/// what the unrotated fixture does.
#[test]
fn heif_container_transformations_are_applied_on_decode() {
    let upright = load(&directory().join("heic-rgb8.heic"), Overrides::default()).unwrap();
    let rotated = load(&directory().join("heic-rotated.heic"), Overrides::default()).unwrap();

    assert_eq!(
        (rotated.width, rotated.height),
        (upright.width, upright.height)
    );
    for (x, y) in PROBES {
        assert_eq!(
            pixel(&rotated, x, y),
            pixel(&upright, x, y),
            "at {x},{y}: the `irot` property was not applied"
        );
    }
}

/// WebP's rotation lives in an `EXIF` chunk rather than in the container
/// proper, so nothing below this decoder would apply it. `webp-exif-rotated`
/// holds the ordinary pattern upside down with a tag saying so, and should
/// decode to exactly what the untagged fixture does.
#[test]
fn webp_exif_orientation_is_applied_on_decode() {
    let upright = load(
        &directory().join("webp-lossless-rgb8.webp"),
        Overrides::default(),
    )
    .unwrap();
    let rotated = load(
        &directory().join("webp-exif-rotated.webp"),
        Overrides::default(),
    )
    .unwrap();

    assert_eq!(
        (rotated.width, rotated.height),
        (upright.width, upright.height)
    );
    for (x, y) in PROBES {
        assert_eq!(
            pixel(&rotated, x, y),
            pixel(&upright, x, y),
            "at {x},{y}: the EXIF orientation was not applied"
        );
    }
}

/// An animated WebP's frames are patches composited onto a canvas, and the
/// one worth showing is the first. `webp-animated` puts the ordinary pattern
/// there and a quarter-turned one after it, so running the animation to its
/// end would be visible rather than silent.
#[test]
fn an_animated_webp_shows_its_first_frame() {
    let animated = load(
        &directory().join("webp-animated.webp"),
        Overrides::default(),
    )
    .unwrap();
    let still = load(
        &directory().join("webp-lossless-rgb8.webp"),
        Overrides::default(),
    )
    .unwrap();

    assert_eq!(
        (animated.width, animated.height),
        (still.width, still.height)
    );
    for (x, y) in PROBES {
        assert_eq!(pixel(&animated, x, y), pixel(&still, x, y), "at {x},{y}");
    }
}

/// An animated GIF is shown as its first frame, the same choice an animated
/// WebP gets and for the same reason: nothing below the decoder has a clock.
/// `gif-animated` puts the ordinary pattern first and a half-turned one after
/// it, so running the animation to its end would be visible rather than
/// silent.
#[test]
fn an_animated_gif_shows_its_first_frame() {
    let animated = load(&directory().join("gif-animated.gif"), Overrides::default()).unwrap();
    let still = load(&directory().join("gif-palette.gif"), Overrides::default()).unwrap();

    assert_eq!(
        (animated.width, animated.height),
        (still.width, still.height)
    );
    for (x, y) in PROBES {
        assert_eq!(pixel(&animated, x, y), pixel(&still, x, y), "at {x},{y}");
    }
}

/// An ICO is a folder of the same picture at several sizes, and a viewer has
/// to pick one. `image` scores depth before size and answers with the 16x12
/// thumbnail here; what someone opening an icon wants to see is the biggest
/// picture in it, which `ico-multi.ico` deliberately makes the shallowest.
#[test]
fn the_largest_ico_entry_is_the_one_shown() {
    let chosen = load(&directory().join("ico-multi.ico"), Overrides::default()).unwrap();
    let expected = load(
        &directory().join("ico-bmp-palette.ico"),
        Overrides::default(),
    )
    .unwrap();

    assert_eq!((chosen.width, chosen.height), (32, 24));
    assert_eq!(
        (chosen.width, chosen.height),
        (expected.width, expected.height)
    );
    for (x, y) in PROBES {
        assert_eq!(pixel(&chosen, x, y), pixel(&expected, x, y), "at {x},{y}");
    }
}

/// A 10-bit HEIF holds 0..1023 in a 16-bit word, but `Samples::full_scale`
/// says a `U16` image's white is 65535. Handed on unscaled, every 10-bit
/// photograph would display at a sixteenth of its intended brightness.
#[test]
fn ten_bit_heif_samples_are_lifted_to_full_scale() {
    let image = load(&directory().join("heic-pq10.heic"), Overrides::default()).unwrap();
    let Samples::U16 { data, .. } = &image.samples else {
        panic!(
            "expected 16-bit samples, got {}",
            image.samples.component_name()
        );
    };
    assert_eq!(data.iter().copied().max(), Some(u16::MAX));
}
