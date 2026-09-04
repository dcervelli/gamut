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
/// it, and the two curves are themselves whatever the surface.
pub fn tone_map(map: ToneMap, headroom: Headroom) -> u32 {
    match (map, headroom) {
        (ToneMap::None, Headroom::None) => 0,
        (ToneMap::Reinhard, _) => 1,
        (ToneMap::Neutral, _) => 2,
        (ToneMap::None, Headroom::Above) => 3,
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

/// Matches `encoding` in `shaders/composite.wgsl`.
pub fn encoding(encoding: Encoding) -> u32 {
    match encoding {
        Encoding::Srgb => 0,
        Encoding::ScRgbLinear => 1,
        Encoding::Pq => 2,
    }
}
