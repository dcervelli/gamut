//! The false-color ramps: which there are, what the command line calls them,
//! and the color each gives a windowed value — the one definition of the
//! ramps, which the image layer writes to the device from.

use crate::image::Transfer;

/// False color for single-channel images. Ignored for color images.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Colormap {
    Gray,
    Viridis,
    Magma,
    Turbo,
}

impl Colormap {
    /// Every map, in the order the key cycles them — which is the order the
    /// buttons under the histogram's ramp are laid out in, so that the two
    /// ways of choosing one agree about what comes after what.
    pub const ALL: [Colormap; 4] = [
        Colormap::Gray,
        Colormap::Viridis,
        Colormap::Magma,
        Colormap::Turbo,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Colormap::Gray => "gray",
            Colormap::Viridis => "viridis",
            Colormap::Magma => "magma",
            Colormap::Turbo => "turbo",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.to_ascii_lowercase().as_str() {
            "gray" | "grey" | "none" => Colormap::Gray,
            "viridis" => Colormap::Viridis,
            "magma" => Colormap::Magma,
            "turbo" => Colormap::Turbo,
            _ => return None,
        })
    }

    pub(super) fn next(self) -> Self {
        match self {
            Colormap::Gray => Colormap::Viridis,
            Colormap::Viridis => Colormap::Magma,
            Colormap::Magma => Colormap::Turbo,
            Colormap::Turbo => Colormap::Gray,
        }
    }

    /// The color this map gives to a windowed value, in the linear working
    /// space. Out-of-window values take the end of the ramp, as they do on
    /// screen.
    ///
    /// The one place the ramps are defined: `render/image_layer.rs` samples
    /// this along each map's length into the texture `shaders/image.wgsl`
    /// reads, so the screen shows what the readout names by construction.
    /// The polynomials produce sRGB-encoded values, linearized after.
    pub fn color(self, value: f32) -> [f32; 3] {
        let t = value.clamp(0.0, 1.0);
        let encoded = match self {
            Colormap::Gray => [t; 3],
            Colormap::Viridis => ramp(&VIRIDIS, t),
            Colormap::Magma => ramp(&MAGMA, t),
            Colormap::Turbo => ramp(&TURBO, t),
        };
        encoded.map(|c| Transfer::Srgb.to_linear(c.clamp(0.0, 1.0)))
    }
}

/// A colormap as its coefficients: one RGB triple per power of the ramp
/// position, lowest first, evaluated by Horner's method.
fn ramp(coefficients: &[[f32; 3]], t: f32) -> [f32; 3] {
    let mut out = [0.0; 3];
    for triple in coefficients.iter().rev() {
        for (slot, coefficient) in out.iter_mut().zip(triple) {
            *slot = *slot * t + coefficient;
        }
    }
    out
}

// Viridis and magma are Matt Zucker's polynomial fits to matplotlib's
// colormaps, from https://www.shadertoy.com/view/WlfXRN, dedicated to the
// public domain under CC0; the colormap data he fitted was CC0 as well. What
// is borrowed is the fit, not the colormap, which is why the license recorded
// in `REUSE.toml` is the fit's.
//
// Written out to the digit as published; f32 keeps rather fewer of them.
#[allow(clippy::excessive_precision)]
const VIRIDIS: [[f32; 3]; 7] = [
    [0.2777273, 0.00540734, 0.33409980],
    [0.10509304, 1.40461353, 1.38459016],
    [-0.33086183, 0.21484756, 0.09509516],
    [-4.63423050, -5.79910097, -19.33244096],
    [6.22826994, 14.17993337, 56.69055260],
    [4.77638500, -13.74514538, -65.35303263],
    [-5.43545586, 4.64585261, 26.31241433],
];

#[allow(clippy::excessive_precision)]
const MAGMA: [[f32; 3]; 7] = [
    [-0.00213649, -0.00074966, -0.00538613],
    [0.25166054, 0.67752324, 2.49402660],
    [8.35371728, -3.57771951, 0.31446790],
    [-27.66873309, 14.26473078, -13.64921319],
    [52.17613981, -27.94360607, 12.94416944],
    [-50.76852536, 29.04658282, 4.23415299],
    [18.65570507, -11.48977352, -5.60196151],
];

/// Turbo's fit is Google's own rather than a third party's: the colormap is
/// Anton Mikhailov's and the approximation Ruofei Du's, published together at
/// <https://gist.github.com/mikhailov-work/0d177465a8151eb6ede1768d51d476c7>
/// under Apache-2.0. `REUSE.toml` records it.
///
/// Published as two dot products per channel; this is the same degree-five
/// polynomial, transposed to a triple per power.
#[allow(clippy::excessive_precision)]
const TURBO: [[f32; 3]; 6] = [
    [0.13572138, 0.09140261, 0.10667330],
    [4.61539260, 2.19418839, 12.64194608],
    [-42.66032258, 4.84296658, -60.58204836],
    [132.13108234, -14.18503333, 110.36276771],
    [-152.94239396, 4.27729857, -89.90310912],
    [59.28637943, 2.82956604, 27.34824973],
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The key and the row of buttons offer the same maps in the same order.
    #[test]
    fn cycling_the_false_color_walks_the_row_of_them() {
        let mut map = Colormap::ALL[0];
        for expected in Colormap::ALL.into_iter().skip(1) {
            map = map.next();
            assert_eq!(map, expected);
        }
        assert_eq!(map.next(), Colormap::ALL[0], "and round again");
    }

    /// The fits are transcribed from `shaders/image.wgsl`, where a mistyped
    /// coefficient would be invisible; against matplotlib's own colors they
    /// are not. Loose, because a seven-term fit is an approximation of a
    /// 256-entry table, but nowhere near loose enough to hide a typo.
    #[test]
    fn the_colormaps_land_on_the_colors_they_are_named_after() {
        let encoded = |map: Colormap, t: f32| {
            map.color(t)
                .map(|channel| Transfer::Srgb.to_encoded(channel))
        };
        let close =
            |got: [f32; 3], want: [f32; 3]| got.iter().zip(want).all(|(a, b)| (a - b).abs() < 0.06);

        // matplotlib: viridis runs #440154 -> #21918c -> #fde725.
        assert!(close(
            encoded(Colormap::Viridis, 0.0),
            [0.267, 0.005, 0.329]
        ));
        assert!(close(
            encoded(Colormap::Viridis, 0.5),
            [0.129, 0.569, 0.549]
        ));
        assert!(close(
            encoded(Colormap::Viridis, 1.0),
            [0.993, 0.906, 0.144]
        ));

        // magma runs #000004 -> #b5367a -> #fcfdbf.
        assert!(close(encoded(Colormap::Magma, 0.0), [0.001, 0.000, 0.014]));
        assert!(close(encoded(Colormap::Magma, 0.5), [0.716, 0.215, 0.475]));
        assert!(close(encoded(Colormap::Magma, 1.0), [0.987, 0.991, 0.749]));

        // turbo runs dark blue -> green -> dark red.
        let middle = encoded(Colormap::Turbo, 0.5);
        assert!(middle[1] > middle[0] && middle[1] > middle[2], "{middle:?}");
        let top = encoded(Colormap::Turbo, 1.0);
        assert!(top[0] > 0.4 && top[2] < 0.2, "{top:?}");

        // Past either end of the window the ramp stops rather than running on
        // into whatever the polynomial does out there.
        assert_eq!(Colormap::Viridis.color(-3.0), Colormap::Viridis.color(0.0));
        assert_eq!(Colormap::Viridis.color(9.0), Colormap::Viridis.color(1.0));
    }
}
