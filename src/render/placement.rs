//! Where the image lands on the window, and how it is resampled getting
//! there: the two things the image layer needs to know about a draw.
//!
//! Defined here rather than in `view` because the renderer consumes them; the
//! view produces them, and a lower layer must not depend on a higher one.

/// How the image is resampled when it is shown larger than life.
///
/// Minification has one right answer — average what the pixel covers — but
/// magnification is a judgement about what the image is for, so it is the
/// user's to make. Neither is a smoothing filter in the ordinary sense: both
/// leave a texel centre exactly as it was found.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Upscale {
    /// Nearest neighbour, ramped across the single output pixel that straddles
    /// a texel edge. Shows the pixel grid a measurement image is read on, and
    /// unlike plain nearest it does not double columns unevenly at a zoom that
    /// is not a whole number.
    #[default]
    Nearest,
    /// Catmull-Rom. Smooth, noticeably sharper than bilinear, and worth having
    /// when the subject is a photograph rather than a grid of measurements.
    Bicubic,
}

impl Upscale {
    /// Every filter, in the order the interface offers them — the same order
    /// [`Upscale::next`] cycles through, so the key and the cells of the zoom
    /// menu agree about what comes after what.
    pub const ALL: [Upscale; 2] = [Upscale::Nearest, Upscale::Bicubic];

    /// What the interface calls this filter, for the cell that chooses it.
    ///
    /// Title case, like every other name the interface writes out and unlike
    /// the value `--upscale` is given on the command line; [`Upscale::parse`]
    /// folds case, so the two are still the same word.
    pub fn label(self) -> &'static str {
        match self {
            Upscale::Nearest => "Nearest",
            Upscale::Bicubic => "Bicubic",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.to_ascii_lowercase().as_str() {
            "nearest" | "point" | "pixel" => Upscale::Nearest,
            "bicubic" | "cubic" | "catmull" | "catmull-rom" => Upscale::Bicubic,
            _ => return None,
        })
    }

    pub(crate) fn next(self) -> Self {
        match self {
            Upscale::Nearest => Upscale::Bicubic,
            Upscale::Bicubic => Upscale::Nearest,
        }
    }
}

/// Where the image sits in the window, in physical pixels, origin top-left.
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub zoom: f32,
    pub upscale: Upscale,
}

impl Placement {
    /// Where `point`, in physical window pixels, falls on the image, in image
    /// pixels. Fractional, and not clamped: the caller knows whether it wants
    /// the texel the point lands in and whether being off the image matters.
    pub fn image_point(&self, point: [f32; 2]) -> [f32; 2] {
        [
            (point[0] - self.x) / self.zoom,
            (point[1] - self.y) / self.zoom,
        ]
    }
}
