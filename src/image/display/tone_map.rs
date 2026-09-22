//! The curve the highlights go out through, and the room a surface has for
//! them: `ToneMap` is what the viewer asks for, `Headroom` what the output
//! turned out to have, and which arm applies is a question of both.

/// What to do with values that are still above 1.0 once windowed.
///
/// A curve is something added: it exists to fit values above white into a
/// surface that stops there. So there is the one curve, and `None` — which
/// is not a second curve but the absence of one, and means whatever the
/// surface makes of the highlights on its own: an SDR surface clamps them at
/// white, and an HDR surface shows them at the brightness they were graded
/// to. Which of the two is [`Headroom`]'s to say, and the surface's; whether
/// to add the curve is the viewer's.
///
/// One curve, because a viewer wants exactly one thing of it: the highlights
/// brought back under white with everything else left alone. A curve that
/// re-grades the in-range picture to make room for them — Reinhard's
/// `c / (c + 1)` sends white to a half, and changes every pixel of an 8-bit
/// file at 0 EV — is editing, which this panel does not do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToneMap {
    /// No curve. Clipping on an SDR surface, which is correct for measurement
    /// work where clipping is a thing to be seen; the highlights as they are
    /// on an HDR one, which is the whole point of asking for one.
    None,
    /// Khronos PBR Neutral: a shoulder that rolls the highlights off under
    /// white, a toe that takes a small offset out of the shadows, and a
    /// desaturation toward the peak that holds hue where a channel would
    /// otherwise clip first. Below the shoulder a value comes out as itself.
    Neutral,
}

impl ToneMap {
    /// Both choices, in the order the histogram panel's row of them is drawn
    /// in; the key toggles between them.
    pub const ALL: [ToneMap; 2] = [ToneMap::None, ToneMap::Neutral];

    pub fn label(self) -> &'static str {
        match self {
            ToneMap::None => "none",
            ToneMap::Neutral => "neutral",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.to_ascii_lowercase().as_str() {
            "none" => ToneMap::None,
            "neutral" => ToneMap::Neutral,
            _ => return None,
        })
    }

    pub(super) fn next(self) -> Self {
        match self {
            ToneMap::None => ToneMap::Neutral,
            ToneMap::Neutral => ToneMap::None,
        }
    }

    /// The curve a picture gets when nothing has asked for one: none at all
    /// where the surface has room for the highlights, and otherwise a
    /// roll-off where there are highlights above white to roll off — and
    /// none, again, where there are not, since a curve on a picture that
    /// never reaches white is a bend in it for no reason.
    ///
    /// `above_white` is the picture as the display has it: not what kind of
    /// file it is, but whether anything in it comes out past white once the
    /// window is on it. See [`super::Display::exceeds_white`].
    ///
    /// Held apart from [`super::Display::for_image_with`] because the surface can
    /// change under a picture — it is settled after the first file is
    /// decoded, and it is switched — so the answer has to be asked for again
    /// whenever it does.
    pub fn default_for(headroom: Headroom, above_white: bool) -> Self {
        match (headroom, above_white) {
            (Headroom::Above, _) | (Headroom::None, false) => ToneMap::None,
            (Headroom::None, true) => ToneMap::Neutral,
        }
    }

    /// The curve itself, applied to one linear color, on a surface with
    /// `headroom`.
    ///
    /// The GPU runs this on every pixel of every frame as `tone_map` in
    /// `shaders/composite.wgsl`, with `shader_codes::tone_map` choosing the
    /// arm the way the match below does; this is the same arithmetic for the
    /// one pixel a readout has to describe. A readback test in
    /// `render/filter_tests.rs` holds the device to this over every arm —
    /// the point of a readout is that it agrees with the screen.
    pub fn apply(self, color: [f32; 3], headroom: Headroom) -> [f32; 3] {
        match (self, headroom) {
            // The hardware clamps on the way into an SDR surface.
            (ToneMap::None, Headroom::None) => color.map(|c| c.clamp(0.0, 1.0)),
            // The negatives go, as they do under every other curve here:
            // undershoot from a bicubic lobe is not light.
            (ToneMap::None, Headroom::Above) => color.map(|c| c.max(0.0)),
            (ToneMap::Neutral, _) => neutral(color.map(|c| c.max(0.0))),
        }
    }
}

/// Khronos PBR Neutral, the twin of `neutral()` in `shaders/composite.wgsl`.
fn neutral(color: [f32; 3]) -> [f32; 3] {
    const START_COMPRESSION: f32 = 0.8 - 0.04;
    const DESATURATION: f32 = 0.15;

    let darkest = color[0].min(color[1]).min(color[2]);
    let offset = if darkest < 0.08 {
        darkest - 6.25 * darkest * darkest
    } else {
        0.04
    };
    let color = color.map(|c| c - offset);

    let peak = color[0].max(color[1]).max(color[2]);
    if peak < START_COMPRESSION {
        return color;
    }

    let d = 1.0 - START_COMPRESSION;
    let new_peak = 1.0 - d * d / (peak + d - START_COMPRESSION);
    let color = color.map(|c| c * (new_peak / peak));

    // Highlights desaturate towards the peak rather than clipping a channel
    // at a time, which is what keeps the hue.
    let g = 1.0 - 1.0 / (DESATURATION * (peak - new_peak) + 1.0);
    color.map(|c| c + (new_peak - c) * g)
}

/// Whether the surface being drawn to has room above SDR white.
///
/// It is what decides what becomes of values over 1.0 when no curve is on,
/// so it belongs to the output rather than to the image or to the user, and
/// is passed to everything here that has to say what the screen shows rather
/// than kept in [`super::Display`]. `render` resolves it from the surface it managed
/// to get; this layer only has to know which of the two it is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Headroom {
    /// An SDR surface: everything above 1.0 is clamped at white on the way in.
    #[default]
    None,
    /// An HDR surface: the highlights go out at the brightness they were
    /// graded to, and tone mapping is something the viewer asks for rather
    /// than something the output imposes.
    Above,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirrors of shader code are worth pinning to their anchors: clip is a
    /// clamp, and the neutral curve leaves ordinary values where they are
    /// before rolling off the top.
    #[test]
    fn the_tone_curves_match_what_the_compositor_does() {
        assert_eq!(
            ToneMap::None.apply([-1.0, 0.5, 2.0], Headroom::None),
            [0.0, 0.5, 1.0]
        );

        let neutral = ToneMap::Neutral.apply([0.2, 0.4, 0.6], Headroom::None);
        for (got, want) in neutral.iter().zip([0.2, 0.4, 0.6]) {
            assert!((got - want).abs() < 0.05, "{neutral:?}");
        }
        // Everything above the shoulder stays inside the display's range.
        for value in [1.0, 4.0, 100.0] {
            let peak = ToneMap::Neutral.apply([value; 3], Headroom::None)[0];
            assert!((0.8..=1.0).contains(&peak), "{value} -> {peak}");
        }
    }

    /// No curve means whatever the surface does: an SDR surface clamps at
    /// white, and an HDR one passes the highlights through and clips nothing
    /// but the light that is not there — the shader's arms 0 and 2.
    #[test]
    fn no_curve_is_a_clip_on_sdr_and_a_pass_through_on_hdr() {
        let color = [-0.25, 0.5, 6.31];
        assert_eq!(ToneMap::None.apply(color, Headroom::None), [0.0, 0.5, 1.0]);
        assert_eq!(
            ToneMap::None.apply(color, Headroom::Above),
            [0.0, 0.5, 6.31]
        );
        // The curve is the curve whatever the surface.
        assert_eq!(
            ToneMap::Neutral.apply(color, Headroom::None),
            ToneMap::Neutral.apply(color, Headroom::Above)
        );
    }

    /// Every choice has to be reachable from every other one, or a viewer on
    /// an HDR surface who presses `t` to see the SDR rendering has no way
    /// back to the one the surface was asked for.
    #[test]
    fn cycling_the_tone_map_returns_to_where_it_started() {
        let mut map = ToneMap::None;
        let mut seen = vec![map];
        for _ in 1..ToneMap::ALL.len() {
            map = map.next();
            assert!(!seen.contains(&map), "{map:?} came round twice");
            seen.push(map);
        }
        assert_eq!(map.next(), ToneMap::None);
    }
}
