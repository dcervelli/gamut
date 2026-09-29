//! A region a file's metadata marks out on its picture: the subject the
//! camera metered or focused on, a face a cataloging program named, a
//! barcode it read.
//!
//! Each source says the same few things in its own way — EXIF's
//! `SubjectArea` as pixels of the picture, the Metadata Working Group's
//! regions as shares of its sides, with a name and a kind — and
//! [`MetadataRegion`] is what they have in common: what to call the region,
//! the words it carries, and a [`Shape`] in the [`Units`] the source wrote
//! it in. Every source measures in the picture as stored, before the EXIF
//! orientation turns it; [`MetadataRegion::place`] carries a shape through
//! that turn and the one in force to the picture as shown, which is where
//! everything the user reads is.

use ::image::metadata::Orientation;

use super::orient::{self, Turn};
use super::xmp;

/// One region, from whichever source, ready to be written out.
#[derive(Clone, PartialEq, Debug)]
pub struct MetadataRegion {
    /// What the row is named by: what kind of region it is.
    pub label: String,
    /// Who or what is in it, where the source says.
    pub name: Option<String>,
    /// Everything else the source says about it, each already in words, in
    /// the order they are read after the name.
    pub details: Vec<String>,
    /// Where it is; `None` for a region that names something without saying
    /// where.
    pub shape: Option<Shape>,
    pub units: Units,
}

/// Where a region is, in the picture as stored, in its [`Units`]. A
/// diameter is measured as a width is.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Shape {
    Point { center: [f64; 2] },
    Circle { center: [f64; 2], diameter: f64 },
    Rectangle { center: [f64; 2], size: [f64; 2] },
}

/// What a shape's numbers are measured in.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Units {
    /// Shares of the stored picture's sides, 0 to 1.
    Shares,
    /// Pixels of a picture the size given, as stored — or, where the source
    /// does not say what size, of the picture itself.
    Pixels { of: Option<[f64; 2]> },
}

impl MetadataRegion {
    /// The regions of the Metadata Working Group's list, in its order.
    pub fn mwg(regions: &xmp::Regions) -> Vec<Self> {
        regions
            .list
            .iter()
            .map(|region| {
                let label = match region.kind.as_deref() {
                    None => "Region",
                    Some("BarCode") => "Barcode",
                    Some(kind) => kind,
                };
                let focus = region.focus_usage.as_deref().map(|usage| match usage {
                    "EvaluatedUsed" => "focused on".to_string(),
                    "EvaluatedNotUsed" => "considered, not focused on".to_string(),
                    "NotEvaluatedNotUsed" => "not considered".to_string(),
                    usage => usage.to_string(),
                });
                // Lightroom's, outside the schema, and written as zero.
                let rotation = region
                    .rotation
                    .filter(|degrees| *degrees != 0.0)
                    .map(|degrees| format!("rotated {degrees}\u{00b0}"));
                let details = [
                    region.description.clone(),
                    region.barcode.clone(),
                    focus,
                    rotation,
                ]
                .into_iter()
                .flatten()
                .collect();
                let (shape, units) = match region.area {
                    Some(area) => {
                        let center = [area.x, area.y];
                        let shape = match (area.w, area.h, area.d) {
                            (Some(w), Some(h), _) => Shape::Rectangle {
                                center,
                                size: [w, h],
                            },
                            (_, _, Some(diameter)) => Shape::Circle { center, diameter },
                            _ => Shape::Point { center },
                        };
                        let units = if area.normalized {
                            Units::Shares
                        } else {
                            Units::Pixels {
                                of: regions.applied_to.map(|(w, h)| [w, h]),
                            }
                        };
                        (Some(shape), units)
                    }
                    None => (None, Units::Shares),
                };
                Self {
                    label: label.to_string(),
                    name: region.name.clone(),
                    details,
                    shape,
                    units,
                }
            })
            .collect()
    }

    /// EXIF's `SubjectArea`: where the camera found the main subject, as a
    /// point, a circle or a rectangle by how many numbers it holds — the
    /// center, then a diameter, or a width and a height — in pixels of the
    /// picture `of` pixels across and down as stored, which is what the
    /// block's own `PixelXDimension` and `PixelYDimension` say where it has
    /// them. `None` for any other count, which is not a shape.
    pub fn subject_area(values: &[u32], of: Option<[f64; 2]>) -> Option<Self> {
        let values: Vec<f64> = values.iter().map(|&value| f64::from(value)).collect();
        let shape = match values[..] {
            [x, y] => Shape::Point { center: [x, y] },
            [x, y, diameter] => Shape::Circle {
                center: [x, y],
                diameter,
            },
            [x, y, w, h] => Shape::Rectangle {
                center: [x, y],
                size: [w, h],
            },
            _ => return None,
        };
        Some(Self {
            label: "Subject".to_string(),
            name: None,
            details: Vec::new(),
            shape: Some(shape),
            units: Units::Pixels { of },
        })
    }

    /// Where the region is in the picture as shown, `shown` pixels across
    /// and down: a rectangle's size and top left corner, as a marked region
    /// and the export dialog write them; a circle's diameter and center; a
    /// point. The shape is carried through `orientation` — the EXIF tag's
    /// turn, which it was measured before — and then `turn`, and taken as
    /// shares of the picture's sides, so that a picture made smaller since
    /// the region was measured still has it in the right place.
    pub fn place(&self, orientation: Orientation, turn: Turn, shown: [u32; 2]) -> Option<String> {
        let shape = self.shape?;
        let shown = shown.map(f64::from);
        // The picture as stored: the one shown with the turn taken back off,
        // and then the orientation. Either is a swap of the sides or none.
        let upright = turn.size(shown);
        let stored = if orient::quarter_turn(orientation) {
            [upright[1], upright[0]]
        } else {
            upright
        };
        // What the numbers are measured against.
        let basis = match self.units {
            Units::Shares => [1.0, 1.0],
            Units::Pixels { of: Some(of) } => of,
            Units::Pixels { of: None } => stored,
        };
        if basis[0] <= 0.0 || basis[1] <= 0.0 {
            return None;
        }
        let at = |center: [f64; 2]| {
            let [x, y] = orient::upright(
                turn.orientation(),
                orient::upright(orientation, [center[0] / basis[0], center[1] / basis[1]]),
            );
            [x * shown[0], y * shown[1]]
        };
        let swapped = orient::quarter_turn(orientation) != turn.is_quarter();
        // Adding nothing turns an edge a hair past the picture's from -0
        // into 0.
        let pixels = |value: f64| value.round() + 0.0;
        Some(match shape {
            Shape::Rectangle { center, size } => {
                let mut size = [size[0] / basis[0], size[1] / basis[1]];
                if swapped {
                    size.swap(0, 1);
                }
                let size = [size[0] * shown[0], size[1] * shown[1]];
                let [x, y] = at(center);
                format!(
                    "{} \u{00d7} {} at {}, {}",
                    pixels(size[0]),
                    pixels(size[1]),
                    pixels(x - size[0] / 2.0),
                    pixels(y - size[1] / 2.0),
                )
            }
            Shape::Circle { center, diameter } => {
                let [x, y] = at(center);
                format!(
                    "{} across at {}, {}",
                    pixels(diameter / basis[0] * stored[0]),
                    pixels(x),
                    pixels(y),
                )
            }
            Shape::Point { center } => {
                let [x, y] = at(center);
                format!("{}, {}", pixels(x), pixels(y))
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two numbers are a point, three a circle, four a rectangle, each
    /// measured in pixels of the size the block says; any other count is
    /// nothing.
    #[test]
    fn a_subject_area_is_shaped_by_how_many_numbers_it_holds() {
        let of = Some([4000.0, 3000.0]);
        let shape = |values: &[u32]| MetadataRegion::subject_area(values, of).and_then(|r| r.shape);
        assert_eq!(
            shape(&[10, 20]),
            Some(Shape::Point {
                center: [10.0, 20.0]
            })
        );
        assert_eq!(
            shape(&[10, 20, 30]),
            Some(Shape::Circle {
                center: [10.0, 20.0],
                diameter: 30.0
            })
        );
        assert_eq!(
            shape(&[10, 20, 30, 40]),
            Some(Shape::Rectangle {
                center: [10.0, 20.0],
                size: [30.0, 40.0]
            })
        );
        assert_eq!(shape(&[]), None);
        assert_eq!(shape(&[1]), None);
        assert_eq!(shape(&[1, 2, 3, 4, 5]), None);
        assert_eq!(
            MetadataRegion::subject_area(&[1, 2], of).map(|r| r.units),
            Some(Units::Pixels { of })
        );
    }

    /// A size of nothing to measure against places nothing, rather than
    /// dividing by it.
    #[test]
    fn a_region_measured_against_nothing_is_not_placed() {
        let region = MetadataRegion::subject_area(&[1, 2], Some([0.0, 3000.0])).unwrap();
        assert_eq!(
            region.place(Orientation::NoTransforms, Turn::NONE, [40, 30]),
            None
        );
    }
}
