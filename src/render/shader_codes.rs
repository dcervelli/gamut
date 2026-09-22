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
    use std::collections::BTreeSet;

    use super::*;

    const TEXEL: &str = include_str!("shaders/texel.wgsl");
    const IMAGE: &str = include_str!("shaders/image.wgsl");
    const REDUCE: &str = include_str!("shaders/reduce.wgsl");
    const COMPOSITE: &str = include_str!("shaders/composite.wgsl");

    /// The `case Nu:` arms of every `switch <selector> {` in `source`, one
    /// set per switch in the order they appear. Each switch has a `default`
    /// arm besides, so what a set leaves out is what the default takes.
    fn cases(source: &str, selector: &str) -> Vec<BTreeSet<u32>> {
        let opening = format!("switch {selector} {{");
        let switches: Vec<_> = source
            .match_indices(&opening)
            .map(|(at, _)| {
                let body = &source[at + opening.len()..];
                let mut depth = 1;
                let end = body
                    .char_indices()
                    .find(|&(_, c)| {
                        match c {
                            '{' => depth += 1,
                            '}' => depth -= 1,
                            _ => {}
                        }
                        depth == 0
                    })
                    .map(|(at, _)| at)
                    .expect("the switch closes");
                body[..end]
                    .lines()
                    .filter_map(|line| {
                        let arm = line.trim().strip_prefix("case ")?;
                        let digits: String = arm.chars().take_while(char::is_ascii_digit).collect();
                        digits.parse().ok()
                    })
                    .collect()
            })
            .collect();
        assert!(!switches.is_empty(), "no `{opening}`");
        switches
    }

    fn set(codes: impl IntoIterator<Item = u32>) -> BTreeSet<u32> {
        codes.into_iter().collect()
    }

    /// The layout is switched on twice over: to premultiply, in the texel
    /// reading both image shaders share, where gray and RGB have no alpha
    /// and are left to the default; and to expand to RGBA in the image
    /// layer, where RGBA is the default.
    #[test]
    fn the_layouts_are_the_shaders_swizzle_arms() {
        let with_alpha = set([swizzle(Channels::GrayAlpha), swizzle(Channels::Rgba)]);
        let expanded = set([
            swizzle(Channels::Gray),
            swizzle(Channels::GrayAlpha),
            swizzle(Channels::Rgb),
        ]);
        assert_eq!(cases(TEXEL, "params.swizzle"), [with_alpha]);
        assert_eq!(cases(IMAGE, "params.swizzle"), [expanded]);
    }

    /// The texel reading is shared by being prepended, not copied: neither
    /// shader carries a reading of its own.
    #[test]
    fn the_texel_reading_is_in_neither_shader() {
        for source in [IMAGE, REDUCE] {
            for name in ["fn premultiplied", "fn gains", "fn gain", "fn load"] {
                assert!(!source.contains(name), "{name}");
            }
            assert!(source.contains("load("));
        }
    }

    /// Alpha is compared rather than switched on: straight alpha is the
    /// one mode that is multiplied through, and opaque the one the image
    /// layer does not divide back out.
    #[test]
    fn the_alpha_modes_are_the_shaders_comparisons() {
        let straight = format!("params.alpha_mode != {}u", alpha(AlphaMode::Straight));
        assert!(TEXEL.contains(&straight), "{straight}");
        let opaque = format!("params.alpha_mode == {}u", alpha(AlphaMode::Opaque));
        assert!(IMAGE.contains(&opaque), "{opaque}");
        assert_eq!(
            level_alpha(AlphaMode::Straight),
            alpha(AlphaMode::Premultiplied)
        );
    }

    /// Every ramp but gray is an arm; gray is the default.
    #[test]
    fn the_colormaps_are_the_shaders_arms() {
        let ramps = set(Colormap::ALL
            .iter()
            .filter(|map| **map != Colormap::Gray)
            .map(|map| colormap(*map)));
        assert_eq!(cases(IMAGE, "which"), [ramps]);
        assert_eq!(
            colormap(Colormap::Gray),
            0,
            "no false color is the default arm"
        );
    }

    /// The curve and the pass-through are arms; the clip is the default.
    #[test]
    fn the_tone_maps_are_the_compositors_arms() {
        let arms = set([
            tone_map(ToneMap::Neutral, Headroom::None),
            tone_map(ToneMap::None, Headroom::Above),
        ]);
        assert_eq!(cases(COMPOSITE, "params.tone_map"), [arms]);
        assert_eq!(
            tone_map(ToneMap::None, Headroom::None),
            0,
            "the clip is the default arm"
        );
        assert_eq!(
            tone_map(ToneMap::Neutral, Headroom::Above),
            tone_map(ToneMap::Neutral, Headroom::None),
            "the curve is itself whatever the surface"
        );
    }

    /// The two magnifiers are arms; the area filter is the default.
    #[test]
    fn the_magnifiers_are_the_shaders_arms() {
        let arms = set([
            resampler(2.0, Upscale::Nearest),
            resampler(2.0, Upscale::Bicubic),
        ]);
        assert_eq!(cases(IMAGE, "params.resampler"), [arms]);
        for upscale in [Upscale::Nearest, Upscale::Bicubic] {
            assert_eq!(resampler(0.5, upscale), 0, "minifying is the default arm");
        }
    }

    /// The two SDR-shaped encodings are arms; PQ is the default.
    #[test]
    fn the_encodings_are_the_compositors_arms() {
        let arms = set([encoding(Encoding::Srgb), encoding(Encoding::ScRgbLinear)]);
        assert_eq!(cases(COMPOSITE, "params.encoding"), [arms]);
        assert_eq!(encoding(Encoding::Pq), 2, "PQ is the default arm");
    }

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
