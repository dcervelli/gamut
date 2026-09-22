//! A gain map applied: the base picture multiplied, in linear light,
//! through the map that says how far above SDR white each pixel went.
//!
//! Two containers carry one — an Ultra HDR JPEG, and a HEIF with an ISO
//! 21496-1 `tmap` item or Apple's auxiliary image — and each reads its
//! own metadata and hands over the same three things: the base picture as
//! 8-bit RGB, the map as 8-bit samples, and a [`Table`] saying what gain
//! each of the map's 256 values stands for. The walk over the pixels is
//! done here once: the base decoded to linear through a table, the map
//! sampled bilinearly at each pixel as the specification's reference
//! does, and the product written out, in bands of rows across the thread
//! pool. `ultrahdr-core`'s own `apply_gainmap` walks them on one thread
//! and decodes sRGB with a `powf` per sample, which took four hundred
//! milliseconds on a twelve-megapixel phone photograph — six times the
//! JPEG decode itself.
//!
//! What comes out is linear light with 1.0 at SDR reference white, which
//! is the working space the rest of this program already speaks: past
//! here a photograph with a gain map is just an HDR image, tone mapped on
//! an SDR surface and sent out as-is on an HDR one.

use ultrahdr_rs::GainMapMetadata;
use ultrahdr_rs::gainmap::apply::GainMapLut;

use crate::image::{
    AlphaMode, Channels, ColorSpace, DecodedImage, Primaries, Referred, Samples, Transfer,
};

/// The map: one or three 8-bit samples a pixel, usually at a fraction of
/// the base's size.
pub(super) struct Map {
    pub width: u32,
    pub height: u32,
    /// One for a luminance map, three for a map with a channel each.
    pub channels: u8,
    pub data: Vec<u8>,
}

impl From<ultrahdr_rs::GainMap> for Map {
    fn from(map: ultrahdr_rs::GainMap) -> Self {
        Self {
            width: map.width,
            height: map.height,
            channels: map.channels,
            data: map.data,
        }
    }
}

/// What each of the map's 256 values means: the linear gain it stands for,
/// per channel, and the offsets the formula adds and takes away around the
/// multiplication.
pub(super) struct Table {
    /// `[R0..R255, G0..G255, B0..B255]`.
    gains: Box<[f32; 256 * 3]>,
    base_offset: [f32; 3],
    alternate_offset: [f32; 3],
}

impl Table {
    /// The table ISO 21496-1 and Ultra HDR describe: a value is a place
    /// between the map's stated minimum and maximum log2 gains, through the
    /// map's own gamma. The whole boost is applied rather than a share of
    /// it chosen for an assumed display — this viewer has an exposure
    /// control and a choice of tone mapping already, and deciding here how
    /// bright the monitor is would only take that choice away. The
    /// specification's weight is where the display's headroom sits between
    /// the base's and the alternate's, so the whole boost is a weight of one.
    pub fn iso(metadata: &GainMapMetadata) -> Self {
        let lut = GainMapLut::new(metadata, 1.0);
        let mut gains = Box::new([0.0f32; 256 * 3]);
        for channel in 0..3 {
            for value in 0..256 {
                gains[channel * 256 + value] = lut.lookup(value as u8, channel);
            }
        }
        Self {
            gains,
            base_offset: metadata.base_offset.map(|offset| offset as f32),
            alternate_offset: metadata.alternate_offset.map(|offset| offset as f32),
        }
    }

    /// The table Apple describes for the map in its own auxiliary image, in
    /// "Applying Apple HDR effect to your photos": the map is encoded with
    /// the Rec. 709 transfer function, and once linearized is the share of
    /// the headroom's boost each pixel gets — `1 + (headroom - 1) * gain`.
    /// `headroom` is the ratio of the picture's brightest white to SDR
    /// white, as the maker note states it.
    pub fn apple(headroom: f32) -> Self {
        let mut gains = Box::new([0.0f32; 256 * 3]);
        for value in 0..256 {
            let gain = 1.0 + (headroom - 1.0) * bt709_to_linear(value as f32 / 255.0);
            for channel in 0..3 {
                gains[channel * 256 + value] = gain;
            }
        }
        Self {
            gains,
            base_offset: [0.0; 3],
            alternate_offset: [0.0; 3],
        }
    }

    fn gain(&self, value: u8, channel: usize) -> f32 {
        self.gains[channel * 256 + value as usize]
    }
}

/// The inverse of Rec. 709's opto-electronic transfer function: the curve
/// on its own, without the display's 2.4 that BT.1886 puts in its place.
fn bt709_to_linear(value: f32) -> f32 {
    if value < 0.081 {
        value / 4.5
    } else {
        ((value + 0.099) / 1.099).powf(1.0 / 0.45)
    }
}

/// The base picture, `width` by `height` of 8-bit RGB encoded by `transfer`,
/// multiplied through the map: linear light, four floats to a pixel with the
/// fourth left at one.
///
/// Each pixel of the base is decoded to linear through a table, and the map
/// — usually a quarter of the base's size in each direction — is sampled
/// bilinearly at the pixel's position, as the specification's reference does.
/// The rows are cut into one band per thread; a pixel depends on nothing but
/// itself and the map, so the split changes no result.
pub(super) fn reconstruct(
    base: &[u8],
    width: u32,
    height: u32,
    transfer: Transfer,
    map: &Map,
    table: &Table,
) -> Vec<f32> {
    let (width, height) = (width as usize, height as usize);
    let pixels = width * height;
    let decode: [f32; 256] = std::array::from_fn(|value| transfer.to_linear(value as f32 / 255.0));
    let columns: Vec<Tap> = (0..width).map(|x| Tap::at(x, width, map.width)).collect();

    // Zeroed rather than filled: the pages are first touched by the band
    // that writes them, in parallel.
    let mut out = vec![0.0f32; pixels * 4];
    if pixels == 0 {
        return out;
    }
    let bands = if pixels < PARALLEL_FROM {
        1
    } else {
        rayon_core::current_num_threads().clamp(1, height)
    };
    let per_band = height.div_ceil(bands);

    let walk = |index: usize, band: &mut [f32]| {
        let rows = base[index * per_band * width * 3..]
            .chunks_exact(width * 3)
            .zip(band.chunks_exact_mut(width * 4));
        for (y, (row, out)) in rows.enumerate() {
            let row_tap = Tap::at(index * per_band + y, height, map.height);
            let (samples, _) = row.as_chunks::<3>();
            let (pixels, _) = out.as_chunks_mut::<4>();
            for ((sample, out), column) in samples.iter().zip(pixels).zip(&columns) {
                let gain = sample_gain(map, table, column, &row_tap);
                for channel in 0..3 {
                    out[channel] = (decode[sample[channel] as usize] + table.base_offset[channel])
                        * gain[channel]
                        - table.alternate_offset[channel];
                }
                out[3] = 1.0;
            }
        }
    };
    if bands == 1 {
        walk(0, &mut out);
    } else {
        rayon_core::scope(|scope| {
            for (index, band) in out.chunks_mut(per_band * width * 4).enumerate() {
                let walk = &walk;
                scope.spawn(move |_| walk(index, band));
            }
        });
    }
    out
}

/// The reconstruction as the image it is: linear light on the base's own
/// primaries, four components so that the upload path can hand the buffer
/// to the GPU without widening it first, and no alpha — the fourth is
/// padding the shader must not read.
pub(super) fn image(
    samples: Vec<f32>,
    width: u32,
    height: u32,
    primaries: Primaries,
) -> DecodedImage {
    let mut image = DecodedImage::new(
        width,
        height,
        Samples::F32 {
            channels: Channels::Rgba,
            data: samples,
        },
        ColorSpace {
            transfer: Transfer::Linear,
            primaries,
        },
        AlphaMode::Opaque,
    );
    // The photograph was graded to sit in 0..1 and the gain map is what
    // puts the highlights above it. Saying so keeps the startup window
    // off the percentile stretch that linear float otherwise asks for,
    // which would undo the grading the moment it loaded.
    image.referred = Referred::Display;
    image
}

/// Below this many pixels the reconstruction stays on one thread.
pub(super) const PARALLEL_FROM: usize = 1 << 16;

/// Where a base pixel's row or column falls on the map: the two map rows
/// (or columns) either side of it and how far it is from the first.
struct Tap {
    near: usize,
    far: usize,
    fraction: f32,
}

impl Tap {
    fn at(at: usize, base: usize, map: u32) -> Self {
        let position = (at as f32 / base as f32) * map as f32;
        let last = (map - 1) as usize;
        let near = (position.floor() as usize).min(last);
        Self {
            near,
            far: (near + 1).min(last),
            fraction: position - position.floor(),
        }
    }
}

/// The gain at one base pixel: the map's four surrounding values, each
/// through the table, blended by the pixel's distance from them.
fn sample_gain(map: &Map, table: &Table, column: &Tap, row: &Tap) -> [f32; 3] {
    let stride = map.width as usize;
    let corner = |x: usize, y: usize| (y * stride + x) * map.channels as usize;
    let (c00, c10, c01, c11) = (
        corner(column.near, row.near),
        corner(column.far, row.near),
        corner(column.near, row.far),
        corner(column.far, row.far),
    );
    let blend = |channel: usize| {
        let gain = |corner: usize| table.gain(map.data[corner + channel], channel);
        let top = gain(c00) * (1.0 - column.fraction) + gain(c10) * column.fraction;
        let bottom = gain(c01) * (1.0 - column.fraction) + gain(c11) * column.fraction;
        top * (1.0 - row.fraction) + bottom * row.fraction
    };
    if map.channels == 1 {
        let gain = blend(0);
        [gain, gain, gain]
    } else {
        [blend(0), blend(1), blend(2)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Apple's table runs from no boost at all to the whole headroom, and
    /// the map's Rec. 709 curve is undone on the way: the middle code is
    /// well under half the boost, as a gamma-encoded value is.
    #[test]
    fn apples_table_spans_one_to_the_headroom() {
        let table = Table::apple(4.0);
        assert_eq!(table.gain(0, 0), 1.0);
        assert!((table.gain(255, 2) - 4.0).abs() < 1e-5);
        let middle = table.gain(128, 1);
        assert!(middle > 1.0 && middle < 2.5, "{middle}");
        // Linearizing 0.5 through Rec. 709 gives 0.26; through sRGB it
        // would give 0.21, and through nothing 0.5.
        assert!((bt709_to_linear(0.5) - 0.2596).abs() < 0.001);
        assert!((bt709_to_linear(1.0) - 1.0).abs() < 1e-5);
        assert_eq!(bt709_to_linear(0.0), 0.0);
    }

    /// The ISO table is the crate's own, read out once rather than looked
    /// up per pixel, and the offsets ride with it.
    #[test]
    fn the_iso_table_is_the_crates() {
        let mut metadata = GainMapMetadata::default();
        metadata.gain_map_max = [2.0; 3];
        metadata.gain_map_min = [-1.0, 0.0, 0.5];
        metadata.gamma = [1.0, 1.5, 2.0];
        metadata.base_offset = [0.015625; 3];
        metadata.alternate_offset = [0.01, 0.02, 0.03];
        let table = Table::iso(&metadata);
        let lut = GainMapLut::new(&metadata, 1.0);
        for channel in 0..3 {
            for value in 0..=255u8 {
                assert_eq!(table.gain(value, channel), lut.lookup(value, channel));
            }
        }
        assert_eq!(table.base_offset, [0.015625; 3]);
        assert_eq!(table.alternate_offset, [0.01, 0.02, 0.03]);
    }
}
