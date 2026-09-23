//! The turn a file asks for: EXIF's eight orientations, applied to a
//! decoded image of any layout — and [`Turn`], the quarter turns a user
//! asks for, which are not applied to the pixels at all but read through.
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

use std::sync::Arc;

use ::image::metadata::Orientation;

use crate::image::gain_map::GainMap;
use crate::image::{DecodedImage, Samples};

/// The size a picture has once it is turned the way `orientation` says: a
/// quarter turn swaps width and height. For a decoder's `dimensions`, which
/// has to report the size the pixels will arrive at, since the window opens
/// at it.
pub fn size(width: u32, height: u32, orientation: Orientation) -> (u32, u32) {
    if quarter_turn(orientation) {
        (height, width)
    } else {
        (width, height)
    }
}

/// The image turned the way `orientation` says, with everything else about
/// it kept. The common case, no turn asked for, hands the image straight
/// back.
///
/// A gain map turns with the picture: it is read by the base's own
/// coordinates, by [`DecodedImage::sample`] and by the shaders alike, so a
/// map left as stored under a turned base would lift the wrong pixels.
pub fn apply(image: DecodedImage, orientation: Orientation) -> DecodedImage {
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
        gain_map,
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
        gain_map: gain_map.map(|map| turn_map(&map, orientation)),
    }
}

/// A gain map turned the way its base was.
fn turn_map(map: &GainMap, orientation: Orientation) -> Arc<GainMap> {
    let (width, height) = size(map.width, map.height, orientation);
    Arc::new(GainMap {
        width,
        height,
        channels: map.channels,
        data: turn(
            &map.data,
            map.width,
            map.height,
            map.channels as usize,
            orientation,
        ),
        lift: map.lift.clone(),
    })
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
    let mut turned = Vec::with_capacity(data.len());
    for y in 0..turned_h {
        for x in 0..turned_w {
            let (sx, sy) = stored(orientation, x, y, w, h);
            let at = (sy * w + sx) * channels;
            turned.extend_from_slice(&data[at..at + channels]);
        }
    }
    turned
}

/// Where the stored pixel of the upright picture's `(x, y)` is, in a
/// stored picture `w` by `h`: the one reading of the tag that both the turn
/// of the pixels above and [`Turn::stored`] go through.
pub fn stored(orientation: Orientation, x: usize, y: usize, w: usize, h: usize) -> (usize, usize) {
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
}

/// How far the user has turned the picture on screen: a count of quarter
/// turns clockwise, taken round four.
///
/// Not applied to the pixels. The picture as decoded, and the texture it
/// was uploaded to, stay as the file holds them — the stored space — and
/// the turn is read through wherever a pixel is fetched: the vertex shader's
/// texture coordinate, and [`Turn::stored`] on the CPU. Everything the user
/// sees, marks or reads is in the turned space: the picture's size, the
/// region, the pointer's coordinate, the view.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Turn(u8);

impl Turn {
    pub const NONE: Turn = Turn(0);

    pub fn clockwise(self) -> Turn {
        Turn((self.0 + 1) % 4)
    }

    pub fn counterclockwise(self) -> Turn {
        Turn((self.0 + 3) % 4)
    }

    /// Whether the picture lies on its side: width and height swapped.
    pub fn is_quarter(self) -> bool {
        self.0 % 2 == 1
    }

    /// Quarter turns clockwise, 0 to 3: what the shader switches on.
    pub fn quarters(self) -> u32 {
        u32::from(self.0)
    }

    /// The same turn in EXIF's terms, whose `Rotate90` is a quarter
    /// clockwise.
    pub fn orientation(self) -> Orientation {
        match self.0 {
            0 => Orientation::NoTransforms,
            1 => Orientation::Rotate90,
            2 => Orientation::Rotate180,
            _ => Orientation::Rotate270,
        }
    }

    /// The size a picture `stored` pixels across and down is on screen.
    pub fn size<T: Copy>(self, stored: [T; 2]) -> [T; 2] {
        if self.is_quarter() {
            [stored[1], stored[0]]
        } else {
            stored
        }
    }

    /// The stored pixel shown at `turned`, for a picture `stored` pixels
    /// across and down as the file holds it. `turned` must be inside the
    /// turned picture.
    pub fn stored(self, turned: [u32; 2], stored: [u32; 2]) -> [u32; 2] {
        let (x, y) = self::stored(
            self.orientation(),
            turned[0] as usize,
            turned[1] as usize,
            stored[0] as usize,
            stored[1] as usize,
        );
        [x as u32, y as u32]
    }
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

    /// A gain map under a turned base lifts the pixels it lifted before the
    /// turn: the base's `(x, y)` is the turned picture's `(h-1-y, x)` under a
    /// quarter turn clockwise, and the lift there must be the same.
    #[test]
    fn a_turned_gain_map_lifts_the_same_pixels() {
        use crate::image::gain_map::{GainMap, Lift};
        let (width, height) = (4, 2);
        let data: Vec<u8> = (0..width * height * 3).map(|i| (i * 10) as u8).collect();
        let mut image = DecodedImage::new(
            width,
            height,
            Samples::U8 {
                channels: Channels::Rgb,
                data,
            },
            ColorSpace::SRGB,
            AlphaMode::Opaque,
        );
        image.gain_map = Some(Arc::new(GainMap {
            width: 2,
            height: 1,
            channels: 1,
            data: vec![0, 255],
            lift: Lift::Apple { headroom: 4.0 },
        }));
        let table = image.gain_map.as_ref().unwrap().table(1.0);
        let turned = apply(image.clone(), Orientation::Rotate90);
        for y in 0..height {
            for x in 0..width {
                let before = image.sample(x, y, Some(&table)).unwrap();
                let after = turned.sample(height - 1 - y, x, Some(&table)).unwrap();
                assert_eq!(before.stored(), after.stored(), "({x}, {y})");
                assert_eq!(before.linear(), after.linear(), "({x}, {y})");
            }
        }
    }

    #[test]
    fn four_quarter_turns_are_none() {
        let mut turn = Turn::NONE;
        for _ in 0..4 {
            turn = turn.clockwise();
        }
        assert_eq!(turn, Turn::NONE);
        assert_eq!(Turn::NONE.clockwise().counterclockwise(), Turn::NONE);
        assert_eq!(
            Turn::NONE.counterclockwise(),
            Turn::NONE.clockwise().clockwise().clockwise()
        );
    }

    /// What is shown at a turned pixel is the stored pixel `Turn::stored`
    /// names: read through, the turn agrees with the turn applied.
    #[test]
    fn a_turned_coordinate_is_where_the_turn_fetched_from() {
        let (width, height) = (3u32, 2u32);
        let data: Vec<u8> = (0..width * height).map(|i| i as u8).collect();
        let mut turn = Turn::NONE;
        for _ in 0..4 {
            let applied = super::turn(&data, width, height, 1, turn.orientation());
            let [tw, th] = turn.size([width, height]);
            for y in 0..th {
                for x in 0..tw {
                    let [sx, sy] = turn.stored([x, y], [width, height]);
                    assert_eq!(
                        applied[(y * tw + x) as usize],
                        data[(sy * width + sx) as usize],
                        "{turn:?} at ({x}, {y})"
                    );
                }
            }
            turn = turn.clockwise();
        }
    }
}
