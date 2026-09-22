//! The turn a file asks for: EXIF's eight orientations, applied to a
//! decoded image of any layout.
//!
//! Four formats here carry the tag — JPEG and TIFF in their own headers,
//! WebP in a chunk, PNG in an `eXIf` chunk — and a raw's preview is turned
//! by the orientation LibRaw read from the raw. Which tag says to turn is
//! each decoder's business; the turn itself is one thing, and it is done
//! here rather than by `image`'s `apply_orientation` because that takes a
//! `DynamicImage`, which has no single-channel floating-point layout, and a
//! one-band elevation model is exactly the TIFF that might carry the tag.
//! The eight cases are checked against `image`'s where both can turn the
//! same buffer, so the meaning of each value is the crate's — and the
//! browsers' — rather than a reading of the standard made here.

use ::image::metadata::Orientation;

use crate::image::{DecodedImage, Samples};

/// The size a picture has once it is turned the way `orientation` says: a
/// quarter turn swaps width and height. For a decoder's `dimensions`, which
/// has to report the size the pixels will arrive at, since the window opens
/// at it.
pub(super) fn size(width: u32, height: u32, orientation: Orientation) -> (u32, u32) {
    if quarter_turn(orientation) {
        (height, width)
    } else {
        (width, height)
    }
}

/// The image turned the way `orientation` says, with everything else about
/// it kept. The common case, no turn asked for, hands the image straight
/// back.
pub(super) fn apply(image: DecodedImage, orientation: Orientation) -> DecodedImage {
    if orientation == Orientation::NoTransforms {
        return image;
    }
    let DecodedImage {
        width,
        height,
        samples,
        color,
        alpha,
        referred,
        exposure,
        nodata,
    } = image;
    let samples = match samples {
        Samples::U8 { channels, data } => Samples::U8 {
            channels,
            data: turn(&data, width, height, channels.count(), orientation),
        },
        Samples::U16 { channels, data } => Samples::U16 {
            channels,
            data: turn(&data, width, height, channels.count(), orientation),
        },
        Samples::F32 { channels, data } => Samples::F32 {
            channels,
            data: turn(&data, width, height, channels.count(), orientation),
        },
    };
    let (width, height) = size(width, height, orientation);
    DecodedImage {
        width,
        height,
        samples,
        color,
        alpha,
        referred,
        exposure,
        nodata,
    }
}

/// Whether the turn puts the picture on its side.
fn quarter_turn(orientation: Orientation) -> bool {
    matches!(
        orientation,
        Orientation::Rotate90
            | Orientation::Rotate270
            | Orientation::Rotate90FlipH
            | Orientation::Rotate270FlipH
    )
}

/// A row-major buffer of `channels` samples a pixel, turned.
///
/// Each pixel of the turned picture is fetched from where it was stored,
/// and EXIF says where in terms of the stored picture's first row and
/// first column: for value 6, say, the stored first row is the upright
/// picture's right-hand column and the stored first column its top row,
/// which is the picture turned a quarter clockwise. The formula for each
/// value is written out in those terms below.
fn turn<T: Copy>(
    data: &[T],
    width: u32,
    height: u32,
    channels: usize,
    orientation: Orientation,
) -> Vec<T> {
    let (w, h) = (width as usize, height as usize);
    let (turned_w, turned_h) = if quarter_turn(orientation) {
        (h, w)
    } else {
        (w, h)
    };
    // Where the stored pixel of an upright `(x, y)` is.
    let source = |x: usize, y: usize| -> (usize, usize) {
        match orientation {
            Orientation::NoTransforms => (x, y),
            // The first row on top, the first column on the right.
            Orientation::FlipHorizontal => (w - 1 - x, y),
            // The first row at the bottom, the first column on the right.
            Orientation::Rotate180 => (w - 1 - x, h - 1 - y),
            // The first row at the bottom, the first column on the left.
            Orientation::FlipVertical => (x, h - 1 - y),
            // The first row down the left, the first column along the top.
            Orientation::Rotate90FlipH => (y, x),
            // The first row down the right, the first column along the top.
            Orientation::Rotate90 => (y, h - 1 - x),
            // The first row down the right, the first column along the bottom.
            Orientation::Rotate270FlipH => (w - 1 - y, h - 1 - x),
            // The first row down the left, the first column along the bottom.
            Orientation::Rotate270 => (w - 1 - y, x),
        }
    };
    let mut turned = Vec::with_capacity(data.len());
    for y in 0..turned_h {
        for x in 0..turned_w {
            let (sx, sy) = source(x, y);
            let at = (sy * w + sx) * channels;
            turned.extend_from_slice(&data[at..at + channels]);
        }
    }
    turned
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::{AlphaMode, Channels, ColorSpace};

    const ALL: [Orientation; 8] = [
        Orientation::NoTransforms,
        Orientation::FlipHorizontal,
        Orientation::Rotate180,
        Orientation::FlipVertical,
        Orientation::Rotate90FlipH,
        Orientation::Rotate90,
        Orientation::Rotate270FlipH,
        Orientation::Rotate270,
    ];

    /// Every one of the eight, on a picture with no symmetry to hide a
    /// mistake behind, against `image`'s own turn — which is what the
    /// browsers agree with, and the reading this module has to keep.
    #[test]
    fn every_orientation_agrees_with_the_image_crate() {
        let (width, height) = (3, 2);
        let data: Vec<u8> = (0..width * height * 3).map(|i| i as u8).collect();
        for orientation in ALL {
            let mut theirs = ::image::DynamicImage::ImageRgb8(
                ::image::RgbImage::from_raw(width, height, data.clone()).unwrap(),
            );
            theirs.apply_orientation(orientation);
            let ours = turn(&data, width, height, 3, orientation);
            assert_eq!(
                (theirs.width(), theirs.height()),
                size(width, height, orientation),
                "{orientation:?}"
            );
            assert_eq!(ours, theirs.into_rgb8().into_raw(), "{orientation:?}");
        }
    }

    /// The exact reading of the tag, independent of the crate: EXIF value 6
    /// is a photograph taken with the camera turned so that the stored
    /// first row runs down the right-hand side.
    #[test]
    fn a_quarter_turn_puts_the_first_row_down_the_right() {
        // A 2x1 image: red then green.
        let data = [255, 0, 0, 0, 255, 0];
        let turned = turn(&data, 2, 1, 3, Orientation::Rotate90);
        // Red on top, green below: the row now runs down.
        assert_eq!(turned, [255, 0, 0, 0, 255, 0]);
        let turned = turn(&data, 2, 1, 3, Orientation::Rotate270);
        assert_eq!(turned, [0, 255, 0, 255, 0, 0]);
    }

    /// The layouts `image` has no variant for turn all the same, and carry
    /// everything else about the image with them.
    #[test]
    fn a_single_band_float_image_turns_and_keeps_its_facts() {
        let mut image = DecodedImage::new(
            2,
            1,
            Samples::F32 {
                channels: Channels::Gray,
                data: vec![1.5, -2.5],
            },
            ColorSpace::LINEAR_BT709,
            AlphaMode::Opaque,
        );
        image.nodata = Some(-9999.0);
        let turned = apply(image, Orientation::Rotate90);
        assert_eq!((turned.width, turned.height), (1, 2));
        assert_eq!(turned.nodata, Some(-9999.0));
        match turned.samples {
            Samples::F32 { data, .. } => assert_eq!(data, [1.5, -2.5]),
            other => panic!("the turn changed the sample type to {other:?}"),
        }
    }

    /// Alpha has to survive the turn as well as color.
    #[test]
    fn rotation_carries_the_alpha_channel() {
        let data = [1u8, 2, 3, 64, 5, 6, 7, 128];
        assert_eq!(
            turn(&data, 2, 1, 4, Orientation::Rotate180),
            [5, 6, 7, 128, 1, 2, 3, 64]
        );
    }
}
