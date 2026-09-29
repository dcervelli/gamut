//! A depth map beside the picture: how far from the camera each pixel was,
//! as the phone or the program that wrote the file measured or estimated it.
//!
//! It is carried with the picture the way a gain map is, at its own size — a
//! phone's depth map is a fraction of the photograph's — and turned with it,
//! read one pixel at a time by the readout under the pointer when the depth
//! is what was asked for, and drawn, as [`DepthMap::image`], in the
//! picture's place and stretched over it when the depth map is asked to be
//! shown.
//!
//! Every vendor stores a map the same way underneath: a gray image whose
//! codes are spread evenly over a range of some quantity, which is either
//! the distance itself or its inverse. Google's `RangeLinear` spreads them
//! over the distance between two planes; its `RangeInverse`, and Apple's
//! disparity, over the inverse, which gives the near things the finer steps.
//! [`Scale`] is that one shape, and each vendor's metadata is translated into
//! it by a module of its own — [`apple`], [`google`] — from the XMP the
//! container carries, so that a vendor new to this tree is a module and a
//! line in [`scale`], and the readout never learns whose map it is reading.
//!
//! Finding the map and its XMP is the container's business, and stays with
//! the decoder: a HEIF's is an auxiliary image with a packet of its own, a
//! JPEG's is base64 inside its XMP.

pub mod apple;
pub mod google;

use std::sync::Arc;

use super::xmp::Xmp;
use super::{AlphaMode, ColorSpace, DecodedImage, Referred, Samples};

/// The depth map, shared between the image and every copy of it.
pub type Shared = Arc<DepthMap>;

/// One channel of depth, row-major from the top, at its own size, and what
/// its numbers stand for.
#[derive(Clone, Debug)]
pub struct DepthMap {
    pub width: u32,
    pub height: u32,
    /// Gray, one component a pixel, in whatever width the file stored.
    pub samples: Samples,
    /// What the codes stand for, where the file says. `None` leaves them
    /// codes: nearer and farther, in an order the file does not state.
    pub scale: Option<Scale>,
}

/// How a code becomes a distance: the codes between `codes` spread evenly
/// over `values`, which are `quantity` in `unit`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale {
    /// The codes the range runs between, lowest first. `None` for the whole
    /// width of the samples: 0 to 255 for an 8-bit map.
    pub codes: Option<[f32; 2]>,
    /// What the first and the second of `codes` stand for. Either may be
    /// the larger: a disparity map puts its nearest at the top.
    pub values: [f32; 2],
    pub quantity: Quantity,
    pub unit: Unit,
    pub accuracy: Accuracy,
}

/// What the values of a [`Scale`] are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quantity {
    /// The distance from the camera.
    Distance,
    /// One over the distance: Apple's disparity, and Google's
    /// `RangeInverse`. In one over `unit`.
    Inverse,
}

/// The unit the distances are stated in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Meters,
    Millimeters,
    /// Distances were stated with no unit, or one this does not know.
    Unknown,
}

impl Unit {
    /// The unit a file's word names.
    pub fn parse(word: &str) -> Self {
        match word.trim() {
            "m" => Unit::Meters,
            "mm" => Unit::Millimeters,
            _ => Unit::Unknown,
        }
    }
}

/// How far the distances can be taken at their word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Accuracy {
    /// Distances as measured, or as the file states them with nothing said
    /// against them.
    Absolute,
    /// Right about what is nearer and what is farther, but only estimated
    /// in scale: Apple's word for a map from two cameras' disparity.
    Relative,
}

/// The map at one pixel of the picture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Depth {
    /// The code the map holds there.
    pub stored: f32,
    /// Whether that code is a float rather than a count.
    pub float: bool,
    /// The distance it stands for, where the file says how to get one.
    pub distance: Option<Distance>,
}

/// A distance from the camera, and how far it can be believed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Distance {
    pub value: f32,
    pub unit: Unit,
    pub accuracy: Accuracy,
}

/// What `xmp` says about the depth map it describes, in whichever vendor's
/// words it is written. `None` where no vendor here recognizes it.
pub fn scale(xmp: &Xmp) -> Option<Scale> {
    apple::scale(xmp).or_else(|| google::scale(xmp))
}

impl Scale {
    /// The distance a code stands for in a map of `samples`' width. `None`
    /// where the code lands on an inverse of zero, or anything not finite.
    pub fn distance(&self, code: f32, samples: &Samples) -> Option<Distance> {
        let [low, high] = self.codes.unwrap_or([0.0, samples.full_scale()]);
        if high == low {
            return None;
        }
        let along = (code - low) / (high - low);
        let value = self.values[0] + along * (self.values[1] - self.values[0]);
        let value = match self.quantity {
            Quantity::Distance => value,
            Quantity::Inverse => 1.0 / value,
        };
        (value.is_finite() && value >= 0.0).then_some(Distance {
            value,
            unit: self.unit,
            accuracy: self.accuracy,
        })
    }
}

impl DepthMap {
    /// The map as a picture of its own, to be drawn in the place of the one
    /// it belongs to: its codes as they are, gray, read as a measurement,
    /// which windows them to the range they span rather than to white.
    pub fn image(&self) -> DecodedImage {
        let mut image = DecodedImage::new(
            self.width,
            self.height,
            self.samples.clone(),
            ColorSpace::LINEAR_BT709,
            AlphaMode::Opaque,
        );
        image.referred = Referred::Measured;
        image
    }

    /// The map under the pixel `(x, y)` of a picture `width` by `height`,
    /// as it is stored: the map's own pixel covering the same fraction of
    /// the way across and down. `None` outside the picture.
    pub fn at(&self, x: u32, y: u32, width: u32, height: u32) -> Option<Depth> {
        if x >= width || y >= height || self.width == 0 || self.height == 0 {
            return None;
        }
        let mx = (u64::from(x) * u64::from(self.width) / u64::from(width)) as usize;
        let my = (u64::from(y) * u64::from(self.height) / u64::from(height)) as usize;
        let index = my * self.width as usize + mx;
        let (stored, float) = match &self.samples {
            Samples::U8 { data, .. } => (f32::from(*data.get(index)?), false),
            Samples::U16 { data, .. } => (f32::from(*data.get(index)?), false),
            Samples::F32 { data, .. } => (*data.get(index)?, true),
        };
        Some(Depth {
            stored,
            float,
            distance: self
                .scale
                .and_then(|scale| scale.distance(stored, &self.samples)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::Channels;

    fn map(data: Vec<u8>, width: u32, height: u32, scale: Option<Scale>) -> DepthMap {
        DepthMap {
            width,
            height,
            samples: Samples::U8 {
                channels: Channels::Gray,
                data,
            },
            scale,
        }
    }

    /// A map smaller than the picture is read at the pixel covering the same
    /// place, so a 2x1 map splits a 10-wide picture down the middle.
    #[test]
    fn a_smaller_map_is_read_where_it_covers_the_picture() {
        let depth = map(vec![10, 200], 2, 1, None);
        assert_eq!(depth.at(0, 0, 10, 4).map(|d| d.stored), Some(10.0));
        assert_eq!(depth.at(4, 3, 10, 4).map(|d| d.stored), Some(10.0));
        assert_eq!(depth.at(5, 0, 10, 4).map(|d| d.stored), Some(200.0));
        assert_eq!(depth.at(9, 3, 10, 4).map(|d| d.stored), Some(200.0));
        assert_eq!(depth.at(10, 0, 10, 4), None);
        assert_eq!(depth.at(0, 0, 10, 4).and_then(|d| d.distance), None);
    }

    fn distance(scale: Scale, code: u8) -> f32 {
        map(vec![code], 1, 1, Some(scale))
            .at(0, 0, 1, 1)
            .and_then(|d| d.distance)
            .map(|d| d.value)
            .expect("a distance")
    }

    /// Codes spread evenly over the distance land where a ruler would put
    /// them; spread over its inverse, they give the near half more of them.
    #[test]
    fn a_scale_turns_a_code_into_a_distance() {
        let linear = Scale {
            codes: None,
            values: [1.0, 5.0],
            quantity: Quantity::Distance,
            unit: Unit::Meters,
            accuracy: Accuracy::Absolute,
        };
        assert!((distance(linear, 0) - 1.0).abs() < 1e-6);
        assert!((distance(linear, 255) - 5.0).abs() < 1e-6);
        assert!((distance(linear, 51) - 1.8).abs() < 1e-5);

        let inverse = Scale {
            values: [1.0, 1.0 / 5.0],
            quantity: Quantity::Inverse,
            ..linear
        };
        assert!((distance(inverse, 0) - 1.0).abs() < 1e-6);
        assert!((distance(inverse, 255) - 5.0).abs() < 1e-5);
        assert!(distance(inverse, 128) < distance(linear, 128));
    }

    /// A range of codes narrower than the samples is stretched over the
    /// values, and a code outside it is taken on along the same line.
    #[test]
    fn a_stated_range_of_codes_is_the_one_used() {
        let scale = Scale {
            codes: Some([10.0, 110.0]),
            values: [2.0, 4.0],
            quantity: Quantity::Distance,
            unit: Unit::Meters,
            accuracy: Accuracy::Absolute,
        };
        assert!((distance(scale, 10) - 2.0).abs() < 1e-6);
        assert!((distance(scale, 60) - 3.0).abs() < 1e-6);
        assert!((distance(scale, 110) - 4.0).abs() < 1e-6);
    }

    /// An inverse of zero is infinitely far, which is no distance to write.
    #[test]
    fn an_inverse_of_zero_is_no_distance() {
        let scale = Scale {
            codes: None,
            values: [0.0, 1.0],
            quantity: Quantity::Inverse,
            unit: Unit::Meters,
            accuracy: Accuracy::Absolute,
        };
        let depth = map(vec![0], 1, 1, Some(scale)).at(0, 0, 1, 1);
        assert_eq!(depth.and_then(|d| d.distance), None);
    }
}
