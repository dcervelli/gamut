//! A depth map beside the picture: how far from the camera each pixel was,
//! as the phone or the program that wrote the file measured or estimated it.
//!
//! Nothing on screen is drawn from it. It is carried with the picture the
//! way a gain map is, at its own size — a phone's depth map is a fraction of
//! the photograph's — and turned with it, and read one pixel at a time by the
//! readout under the pointer when the depth is what was asked for.
//!
//! Two containers carry one here. A HEIF names an auxiliary image as depth
//! by MPEG's own type, which `libheif` recognizes; that says nothing of
//! units, so its values are read as the codes they are. A JPEG carries
//! Google's `GDepth` block in its XMP — the map itself, base64 in the
//! extended packet, and the near and far planes it was normalized between —
//! so its values become distances.

use std::sync::Arc;

use super::Samples;

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
    pub range: Range,
}

/// What a code in the map means.
#[derive(Clone, Debug, PartialEq)]
pub enum Range {
    /// Nothing is said: the codes are all there is.
    Unstated,
    /// Normalized between two planes, the code rising in step with the
    /// distance: Google's `RangeLinear`.
    Linear { near: f32, far: f32, unit: Unit },
    /// Normalized between two planes, the code rising in step with the
    /// inverse of the distance, so that near things get the finer steps:
    /// Google's `RangeInverse`.
    Inverse { near: f32, far: f32, unit: Unit },
}

/// The unit the planes, and so the distances, are stated in.
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

/// The map at one pixel of the picture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Depth {
    /// The code the map holds there.
    pub stored: f32,
    /// Whether that code is a float rather than a count.
    pub float: bool,
    /// The distance it stands for, where the file says how to get one.
    pub distance: Option<(f32, Unit)>,
}

impl DepthMap {
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
        let normalized = stored / self.samples.full_scale();
        let distance = match self.range {
            Range::Unstated => None,
            Range::Linear { near, far, unit } => Some((near + normalized * (far - near), unit)),
            Range::Inverse { near, far, unit } => {
                let denominator = far - normalized * (far - near);
                (denominator != 0.0).then(|| (far * near / denominator, unit))
            }
        };
        Some(Depth {
            stored,
            float,
            distance: distance.filter(|(value, _)| value.is_finite()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::Channels;

    fn map(data: Vec<u8>, width: u32, height: u32, range: Range) -> DepthMap {
        DepthMap {
            width,
            height,
            samples: Samples::U8 {
                channels: Channels::Gray,
                data,
            },
            range,
        }
    }

    /// A map smaller than the picture is read at the pixel covering the same
    /// place, so a 2x1 map splits a 10-wide picture down the middle.
    #[test]
    fn a_smaller_map_is_read_where_it_covers_the_picture() {
        let depth = map(vec![10, 200], 2, 1, Range::Unstated);
        assert_eq!(depth.at(0, 0, 10, 4).map(|d| d.stored), Some(10.0));
        assert_eq!(depth.at(4, 3, 10, 4).map(|d| d.stored), Some(10.0));
        assert_eq!(depth.at(5, 0, 10, 4).map(|d| d.stored), Some(200.0));
        assert_eq!(depth.at(9, 3, 10, 4).map(|d| d.stored), Some(200.0));
        assert_eq!(depth.at(10, 0, 10, 4), None);
        assert_eq!(depth.at(0, 0, 10, 4).and_then(|d| d.distance), None);
    }

    /// Both of Google's normalizations reach the near plane at a code of
    /// zero and the far one at full scale; between them, the inverse one
    /// spends its codes on what is close.
    #[test]
    fn the_planes_turn_a_code_into_a_distance() {
        let linear = Range::Linear {
            near: 1.0,
            far: 5.0,
            unit: Unit::Meters,
        };
        let inverse = Range::Inverse {
            near: 1.0,
            far: 5.0,
            unit: Unit::Meters,
        };
        let distance = |range: &Range, code: u8| {
            map(vec![code], 1, 1, range.clone())
                .at(0, 0, 1, 1)
                .and_then(|d| d.distance)
                .map(|(value, _)| value)
                .expect("a distance")
        };
        assert!((distance(&linear, 0) - 1.0).abs() < 1e-6);
        assert!((distance(&linear, 255) - 5.0).abs() < 1e-6);
        assert!((distance(&linear, 51) - 1.8).abs() < 1e-5);
        assert!((distance(&inverse, 0) - 1.0).abs() < 1e-6);
        assert!((distance(&inverse, 255) - 5.0).abs() < 1e-5);
        assert!(distance(&inverse, 128) < distance(&linear, 128));
    }
}
