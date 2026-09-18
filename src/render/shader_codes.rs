//! Every integer the shaders switch on, in one place.
//!
//! Each function here is one half of a contract whose other half is a
//! `switch` in a WGSL file. Keep the two in step: a value added on one side
//! and not the other fails silently, as the wrong branch rather than an error.

use crate::image::display::{Colormap, Headroom, ToneMap};
use crate::image::{AlphaMode, Channels};

use super::output::Encoding;
use super::placement::Upscale;

/// How the shader should expand the sampled components to RGBA.
/// Matches `swizzle` in `shaders/image.wgsl` and `shaders/reduce.wgsl`.
pub fn swizzle(channels: Channels) -> u32 {
    match channels {
        Channels::Gray => 0,
        Channels::GrayAlpha => 1,
        Channels::Rgb => 2,
        Channels::Rgba => 3,
    }
}

/// How the shader should treat the alpha channel it samples from the image
/// as uploaded. Matches `alpha_mode` in `shaders/image.wgsl` and
/// `shaders/reduce.wgsl`.
pub fn alpha(alpha: AlphaMode) -> u32 {
    match alpha {
        AlphaMode::Opaque => 0,
        AlphaMode::Straight => 1,
        AlphaMode::Premultiplied => 2,
    }
}

/// What a coarse level holds. Straight alpha has been multiplied through on
/// the way in; an image whose alpha channel is meaningless keeps it that way,
/// since dividing the color back out by it would be nonsense. Same switch as
/// [`alpha`].
pub fn level_alpha(alpha: AlphaMode) -> u32 {
    match alpha {
        AlphaMode::Opaque => 0,
        AlphaMode::Straight | AlphaMode::Premultiplied => 2,
    }
}

/// Matches `colormap` in `shaders/image.wgsl`.
pub fn colormap(map: Colormap) -> u32 {
    match map {
        Colormap::Gray => 0,
        Colormap::Viridis => 1,
        Colormap::Magma => 2,
        Colormap::Turbo => 3,
    }
}

/// Matches `tone_map` in `shaders/composite.wgsl`, and `ToneMap::apply` in
/// `image/display.rs`, which is the same match on the CPU: no curve is a
/// clip at white on an SDR surface and a pass-through on one with room above
/// it, and the curve is itself whatever the surface.
pub fn tone_map(map: ToneMap, headroom: Headroom) -> u32 {
    match (map, headroom) {
        (ToneMap::None, Headroom::None) => 0,
        (ToneMap::Neutral, _) => 1,
        (ToneMap::None, Headroom::Above) => 2,
    }
}

/// Which filter a draw at `zoom` runs. Matches `resampler` in
/// `shaders/image.wgsl`, where 0 is the area filter minification uses.
///
/// Minification is an area average; magnification is whichever of the two
/// the user asked for. At exactly 1:1 both come to the same thing, so the
/// boundary is not a visible one.
pub fn resampler(zoom: f32, upscale: Upscale) -> u32 {
    if zoom < 1.0 {
        return 0;
    }
    match upscale {
        Upscale::Nearest => 1,
        Upscale::Bicubic => 2,
    }
}

/// Which ends of the window the image shader paints its warning colors
/// over, as the bits of `marks` in `shaders/image.wgsl`: the pixels at or
/// below black, and the pixels at or above white. Nothing while the key for
/// them is up, which is the usual state; and white only where the surface
/// is actually clipping it — no curve on, and no room above white — since a
/// highlight rolled off by a curve or shown by an HDR surface is not lost.
pub fn marks(black: bool, white: bool) -> u32 {
    u32::from(black) | (u32::from(white) << 1)
}

/// The two warning colors, in linear light, as `MARK_WHITE` and `MARK_BLACK`
/// in `shaders/image.wgsl` have them: what a clipped highlight and a clipped
/// shadow are painted. Here so that a test can hold the shader to them.
#[cfg(test)]
pub const MARKS: [[f32; 3]; 2] = [[1.0, 0.02, 0.02], [0.02, 0.1, 1.0]];

/// Matches `encoding` in `shaders/composite.wgsl`.
pub fn encoding(encoding: Encoding) -> u32 {
    match encoding {
        Encoding::Srgb => 0,
        Encoding::ScRgbLinear => 1,
        Encoding::Pq => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two bits are the two ends, and nothing is marked while the key
    /// is up.
    #[test]
    fn the_marks_are_one_bit_an_end() {
        assert_eq!(marks(false, false), 0);
        assert_eq!(marks(true, false), 1);
        assert_eq!(marks(false, true), 2);
        assert_eq!(marks(true, true), 3);
    }

    /// The shader paints the two colors [`MARKS`] says it does: a line of
    /// the WGSL source is held to each, so that a change to either side
    /// without the other fails here rather than on screen.
    #[test]
    fn the_shader_paints_the_marks_the_codes_name() {
        let source = include_str!("shaders/image.wgsl");
        for (name, color) in [("MARK_WHITE", MARKS[0]), ("MARK_BLACK", MARKS[1])] {
            let [r, g, b] = color.map(|channel| format!("{channel:?}"));
            let line = format!("const {name}: vec3<f32> = vec3<f32>({r}, {g}, {b});");
            assert!(source.contains(&line), "{line}");
        }
    }
}
