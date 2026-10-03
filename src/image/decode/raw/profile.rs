//! The DNG specification's arithmetic from a camera's numbers to color, for
//! the raws developed here rather than by LibRaw: the color matrices the file
//! carries for one or two illuminants, blended for the light the picture was
//! balanced for; the camera's neutral carried to white; and the result in
//! Rec. 2020, the space every developed raw arrives in.
//!
//! It follows the specification's chapter on mapping camera color space to
//! CIE XYZ, with two things left out. The camera calibration matrices only
//! apply where the profile's signature matches the camera's, and a camera
//! writing its own profile — an iPhone — leaves them out or makes them the
//! identity; they are taken as the identity. And the white is carried to
//! D65 by Bradford's transform directly rather than to D50 and on, which
//! is the same transform: each is a scaling of the same cone responses, and
//! two scalings are one.

use anyhow::{Result, bail};

pub type Matrix = [[f64; 3]; 3];

/// What the file says about its camera's color, as the DNG tags hold it.
#[derive(Clone, Debug, Default)]
pub struct Profile {
    /// `ColorMatrix1` and `ColorMatrix2`, from XYZ to the camera's space,
    /// each with the color temperature of the illuminant it was measured
    /// under where the file names one this knows.
    pub color: Vec<(Matrix, Option<f64>)>,
    /// `ForwardMatrix1` and `ForwardMatrix2`, from the balanced camera's
    /// space to XYZ under D50, where the file carries them.
    pub forward: Vec<Matrix>,
    /// `AnalogBalance`: what the camera's amplifiers did to each channel.
    pub analog: Option<[f64; 3]>,
    /// `AsShotNeutral`: the camera's values for something white under the
    /// light the picture was taken in.
    pub neutral: Option<[f64; 3]>,
    /// `AsShotWhiteXY`: the same light named as a chromaticity instead.
    pub white_xy: Option<[f64; 2]>,
}

/// D50 and D65 as chromaticities: the specification's reference white, and
/// Rec. 2020's.
const D50: [f64; 2] = [0.3457, 0.3585];
const D65: [f64; 2] = [0.3127, 0.3290];

/// CIE XYZ to linear Rec. 2020, under D65.
const XYZ_TO_BT2020: Matrix = [
    [1.716_651_2, -0.355_670_8, -0.253_366_3],
    [-0.666_684_4, 1.616_481_2, 0.015_768_5],
    [0.017_639_9, -0.042_770_6, 0.942_103_1],
];

/// Bradford's cone responses, from XYZ.
const BRADFORD: Matrix = [
    [0.8951, 0.2664, -0.1614],
    [-0.7502, 1.7135, 0.0367],
    [0.0389, -0.0685, 1.0296],
];

impl Profile {
    /// The matrix from the camera's values — linearized, and scaled so
    /// that the sensor's saturation is 1 — to linear Rec. 2020 under D65,
    /// where the camera's neutral comes out white and the first of its
    /// channels to saturate at 1.
    pub fn to_bt2020(&self) -> Result<[[f32; 3]; 3]> {
        if self.color.is_empty() {
            bail!("the DNG carries no color matrix");
        }
        let analog = self.analog.unwrap_or([1.0; 3]);
        if analog
            .iter()
            .any(|value| !value.is_finite() || *value <= 0.0)
        {
            bail!("the DNG's analog balance is not a balance");
        }

        let (neutral, white) = self.white(analog)?;
        let weight = self.weight(temperature(white));
        let mut camera_to_xyz = match self.forward.as_slice() {
            // The forward matrix takes the balanced camera's values, which
            // are its own values divided by the neutral once the analog
            // balance is undone.
            [first, rest @ ..] => {
                let forward = blend(*first, rest.first().copied(), weight);
                let balanced: [f64; 3] = std::array::from_fn(|c| analog[c] / neutral[c]);
                multiply(forward, diagonal(balanced))
            }
            [] => invert(self.xyz_to_camera(analog, weight))?,
        };

        // The neutral at its brightest — its largest channel at 1, where
        // the sensor saturates — is white at a luminance of 1.
        let peak = neutral.iter().copied().fold(f64::MIN, f64::max);
        let scaled: [f64; 3] = std::array::from_fn(|c| neutral[c] / peak);
        let lit = apply(camera_to_xyz, scaled);
        if !lit[1].is_finite() || lit[1] <= 0.0 {
            bail!("the DNG's neutral is not a color the matrix can light");
        }
        for row in &mut camera_to_xyz {
            for value in row.iter_mut() {
                *value /= lit[1];
            }
        }
        let lit = apply(camera_to_xyz, scaled);
        let adapted = multiply(adaptation(lit, xyz(D65))?, camera_to_xyz);
        let total = multiply(XYZ_TO_BT2020, adapted);
        if total.iter().flatten().any(|value| !value.is_finite()) {
            bail!("the DNG's color matrices do not make a color");
        }
        Ok(total.map(|row| row.map(|value| value as f32)))
    }

    /// The camera's neutral and the chromaticity it is white at, from
    /// whichever of the two the file states. Where it states neither, the
    /// light is daylight.
    fn white(&self, analog: [f64; 3]) -> Result<([f64; 3], [f64; 2])> {
        if let Some(neutral) = self.neutral {
            if neutral
                .iter()
                .any(|value| !value.is_finite() || *value <= 0.0)
            {
                bail!("the DNG's neutral is not a color");
            }
            // The matrices are blended for a light that is itself found
            // through them, so the two are settled together: a guess at
            // the light, the matrix for it, the light that matrix says
            // the neutral is, until the guess stops moving.
            let mut white = D50;
            for _ in 0..30 {
                let weight = self.weight(temperature(white));
                let found =
                    chromaticity(apply(invert(self.xyz_to_camera(analog, weight))?, neutral))?;
                let moved = (found[0] - white[0]).abs() + (found[1] - white[1]).abs();
                white = found;
                if moved < 1e-9 {
                    break;
                }
            }
            return Ok((neutral, white));
        }
        let white = self.white_xy.unwrap_or(D65);
        if white[1] <= 0.0 || !white.iter().all(|value| value.is_finite()) {
            bail!("the DNG's white is not a chromaticity");
        }
        let weight = self.weight(temperature(white));
        let neutral = apply(self.xyz_to_camera(analog, weight), xyz(white));
        if neutral
            .iter()
            .any(|value| !value.is_finite() || *value <= 0.0)
        {
            bail!("the DNG's white is not a color its camera sees");
        }
        Ok((neutral, white))
    }

    /// The analog balance applied after the color matrix blended at
    /// `weight`: XYZ to the values the camera wrote.
    fn xyz_to_camera(&self, analog: [f64; 3], weight: f64) -> Matrix {
        let first = self.color[0].0;
        let second = self.color.get(1).map(|(matrix, _)| *matrix);
        multiply(diagonal(analog), blend(first, second, weight))
    }

    /// How much of the first matrix a light of `kelvin` takes, the rest
    /// being the second's: in proportion along the inverse temperature
    /// between the two illuminants, and all of the nearer one past either.
    /// All of the first where there is no second, or the two illuminants
    /// cannot be placed.
    fn weight(&self, kelvin: f64) -> f64 {
        let (Some((_, Some(first))), Some((_, Some(second)))) =
            (self.color.first(), self.color.get(1))
        else {
            return 1.0;
        };
        if first == second {
            return 1.0;
        }
        ((1.0 / kelvin - 1.0 / second) / (1.0 / first - 1.0 / second)).clamp(0.0, 1.0)
    }
}

/// The color temperature, in kelvin, of the illuminant an EXIF light source
/// code names, as Adobe's DNG SDK reckons them: the fluorescent lamps at the
/// middle of their range. `None` for a code naming no one light.
pub fn illuminant(code: u16) -> Option<f64> {
    Some(match code {
        // Standard light A, tungsten.
        17 | 3 => 2850.0,
        // ISO studio tungsten.
        24 => 3200.0,
        23 => 5000.0,
        // D55, daylight, fine weather, flash, standard light B.
        20 | 1 | 9 | 4 | 18 => 5500.0,
        // D65, standard light C, cloudy weather.
        21 | 19 | 10 => 6500.0,
        // D75, shade.
        22 | 11 => 7500.0,
        // Daylight, day white, cool white or plain, white and warm white
        // fluorescent.
        12 => 6400.0,
        13 => 5050.0,
        14 | 2 => 4150.0,
        15 => 3525.0,
        16 => 2925.0,
        _ => return None,
    })
}

/// McCamy's cubic for the correlated color temperature of a chromaticity:
/// close along the stretch of the locus a camera's light is ever on, and
/// all a blend between two matrices needs. Kept to that stretch.
pub fn temperature([x, y]: [f64; 2]) -> f64 {
    let n = (x - 0.3320) / (0.1858 - y);
    let kelvin = 449.0 * n.powi(3) + 3525.0 * n.powi(2) + 6823.3 * n + 5520.33;
    kelvin.clamp(2000.0, 12500.0)
}

/// The share `weight` of `first` and the rest of `second`, or `first`
/// where there is no second.
fn blend(first: Matrix, second: Option<Matrix>, weight: f64) -> Matrix {
    let Some(second) = second else {
        return first;
    };
    std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            weight * first[row][column] + (1.0 - weight) * second[row][column]
        })
    })
}

/// Bradford's transform from a white of `from` to one of `to`, both XYZ.
fn adaptation(from: [f64; 3], to: [f64; 3]) -> Result<Matrix> {
    let (from, to) = (apply(BRADFORD, from), apply(BRADFORD, to));
    if from.iter().any(|value| *value <= 0.0 || !value.is_finite()) {
        bail!("the DNG's white is not a color the eye adapts to");
    }
    let scale: [f64; 3] = std::array::from_fn(|c| to[c] / from[c]);
    Ok(multiply(
        invert(BRADFORD)?,
        multiply(diagonal(scale), BRADFORD),
    ))
}

/// XYZ at a luminance of 1 for the chromaticity `[x, y]`.
fn xyz([x, y]: [f64; 2]) -> [f64; 3] {
    [x / y, 1.0, (1.0 - x - y) / y]
}

fn chromaticity([x, y, z]: [f64; 3]) -> Result<[f64; 2]> {
    let sum = x + y + z;
    if !sum.is_finite() || sum <= 0.0 {
        bail!("the DNG's neutral is not a color the matrix can place");
    }
    Ok([x / sum, y / sum])
}

fn diagonal(values: [f64; 3]) -> Matrix {
    std::array::from_fn(|row| {
        std::array::from_fn(|column| if row == column { values[row] } else { 0.0 })
    })
}

fn multiply(a: Matrix, b: Matrix) -> Matrix {
    std::array::from_fn(|row| {
        std::array::from_fn(|column| (0..3).map(|k| a[row][k] * b[k][column]).sum())
    })
}

fn apply(matrix: Matrix, vector: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|row| {
        (0..3)
            .map(|column| matrix[row][column] * vector[column])
            .sum()
    })
}

fn invert(m: Matrix) -> Result<Matrix> {
    let cofactor = |row: usize, column: usize| {
        let (r0, r1) = ((row + 1) % 3, (row + 2) % 3);
        let (c0, c1) = ((column + 1) % 3, (column + 2) % 3);
        m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0]
    };
    let determinant: f64 = (0..3)
        .map(|column| m[0][column] * cofactor(0, column))
        .sum();
    if !determinant.is_finite() || determinant.abs() < 1e-12 {
        bail!("the DNG's color matrix cannot be undone");
    }
    // The inverse is the transposed cofactors over the determinant.
    Ok(std::array::from_fn(|row| {
        std::array::from_fn(|column| cofactor(column, row) / determinant)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(have: [[f32; 3]; 3], want: Matrix, within: f64) {
        for row in 0..3 {
            for column in 0..3 {
                let difference = (f64::from(have[row][column]) - want[row][column]).abs();
                assert!(difference < within, "{have:?} is not {want:?}");
            }
        }
    }

    const IDENTITY: Matrix = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

    /// A camera whose space is Rec. 2020, balanced for D65, is developed
    /// by nothing at all: the matrix is the identity.
    #[test]
    fn a_camera_whose_space_is_rec_2020_is_left_alone() {
        let profile = Profile {
            color: vec![(XYZ_TO_BT2020, illuminant(21))],
            neutral: Some([1.0; 3]),
            ..Profile::default()
        };
        near(profile.to_bt2020().unwrap(), IDENTITY, 1e-4);
    }

    /// The neutral comes out white, whatever it is, and its largest
    /// channel at 1 comes out at 1.
    #[test]
    fn the_neutral_comes_out_white() {
        let profile = Profile {
            color: vec![(XYZ_TO_BT2020, illuminant(21))],
            neutral: Some([0.5, 1.0, 0.8]),
            ..Profile::default()
        };
        let matrix = profile.to_bt2020().unwrap();
        let white: [f32; 3] =
            std::array::from_fn(|row| (0..3).map(|c| matrix[row][c] * [0.5, 1.0, 0.8][c]).sum());
        for value in white {
            assert!((value - 1.0).abs() < 1e-4, "{white:?}");
        }
    }

    /// A forward matrix takes the balanced camera to D50, which is then
    /// carried to D65: Rec. 2020's own matrix to D50 develops to the
    /// identity, just as the color matrix did.
    #[test]
    fn a_forward_matrix_is_read_through_the_neutral() {
        let to_d50 = multiply(
            adaptation(xyz(D65), xyz(D50)).unwrap(),
            invert(XYZ_TO_BT2020).unwrap(),
        );
        let profile = Profile {
            color: vec![(XYZ_TO_BT2020, illuminant(21))],
            forward: vec![to_d50],
            neutral: Some([1.0; 3]),
            ..Profile::default()
        };
        near(profile.to_bt2020().unwrap(), IDENTITY, 1e-4);
    }

    /// Two matrices are blended by the light the neutral says the picture
    /// was taken in: a daylight neutral takes the daylight matrix, and a
    /// tungsten one the tungsten matrix.
    #[test]
    fn two_matrices_are_blended_for_the_light() {
        let tungsten: Matrix = [[2.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.5]];
        let daylight = XYZ_TO_BT2020;
        let profile = Profile {
            color: vec![(tungsten, illuminant(17)), (daylight, illuminant(21))],
            ..Profile::default()
        };
        assert_eq!(profile.weight(6500.0), 0.0);
        assert_eq!(profile.weight(2850.0), 1.0);
        assert_eq!(profile.weight(10000.0), 0.0, "past the daylight one");
        let middle = profile.weight(4000.0);
        assert!(middle > 0.0 && middle < 1.0, "{middle}");
        // With no neutral, the light is D65, and so all daylight.
        near(profile.to_bt2020().unwrap(), IDENTITY, 1e-3);
    }

    #[test]
    fn mccamy_places_the_standard_illuminants() {
        assert!((temperature(D65) - 6504.0).abs() < 10.0);
        assert!((temperature(D50) - 5003.0).abs() < 10.0);
    }

    /// A file whose matrix cannot be undone, or whose neutral is no color,
    /// is refused rather than developed to nonsense.
    #[test]
    fn a_profile_that_makes_no_color_is_refused() {
        let flat = Profile {
            color: vec![([[0.0; 3]; 3], None)],
            ..Profile::default()
        };
        assert!(flat.to_bt2020().is_err());
        let dark = Profile {
            color: vec![(XYZ_TO_BT2020, None)],
            neutral: Some([0.0, 1.0, 1.0]),
            ..Profile::default()
        };
        assert!(dark.to_bt2020().is_err());
        assert!(Profile::default().to_bt2020().is_err());
    }
}
