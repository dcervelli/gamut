//! An interface color, and its two ways into the pipeline.

/// An sRGB color with straight alpha, the way UI code likes to think about
/// color. Conversion to whatever the pipeline needs happens on the way to
/// the GPU.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 255)
    }

    pub fn with_alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }

    /// The interface color that shows a linear working-space value: the
    /// inverse of [`Color::to_linear`], for the readouts that have to put a
    /// piece of the display's own output on a panel.
    ///
    /// An SDR reading of it. On an HDR output the surface carries more range
    /// than a panel can show, and a swatch on a panel is the thing that has
    /// to sit beside the words without glowing.
    pub fn from_linear(color: [f32; 3]) -> Self {
        fn channel(value: f32) -> u8 {
            let v = value.clamp(0.0, 1.0);
            let encoded = if v <= 0.0031308 {
                v * 12.92
            } else {
                1.055 * v.powf(1.0 / 2.4) - 0.055
            };
            (encoded * 255.0).round() as u8
        }
        Self::rgb(channel(color[0]), channel(color[1]), channel(color[2]))
    }

    /// Linear components, straight alpha: what both the quad shader — which
    /// writes into an sRGB target and so re-encodes on write — and the
    /// compositor's backdrop want.
    pub(crate) fn to_linear(self) -> [f32; 4] {
        fn channel(value: u8) -> f32 {
            let v = value as f32 / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        }
        [
            channel(self.r),
            channel(self.g),
            channel(self.b),
            self.a as f32 / 255.0,
        ]
    }
}
