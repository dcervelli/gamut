//! Making an image another size on the CPU: a box filter over the file's
//! own samples, for the thumbnails, and a resize of the picture as shown,
//! for an export.
//!
//! The GPU has its own coarse chain for minifying the picture on screen —
//! see `render/reduce.rs` — and this is not that. A thumbnail is made off
//! the event loop by a thread that holds no device, and it is made once and
//! kept, so what matters is that it is cheap in memory and exact in what it
//! averages, not that it is fast.
//!
//! The thumbnail's filter works in the file's own encoding, over the file's
//! own samples, and describes rather than normalizes: the small image it
//! hands back has the same sample type, color space, alpha mode and no-data
//! sentinel as the one it was given, so that everything downstream — the
//! statistics scan, the display window, the encode — reads it exactly as it
//! would have read the full image. Averaging encoded values rather than
//! linear light is the thumbnail's bargain: a thumbnail is a picture of the
//! picture, not a measurement of it.
//!
//! The export's [`resize`] makes the other bargain. It takes the picture
//! after the display pipeline has had it — the 8-bit sRGB [`Raster`] that
//! `encode::displayed` walks out — and resamples it the way the screen
//! resamples the texture: in linear light, with straight alpha multiplied
//! through first so a transparent pixel does not bleed its color into the
//! edge beside it, averaging what each output pixel covers where the
//! picture shrinks and Catmull-Rom where it grows. The shrink is the
//! `area` of `shaders/image.wgsl`, the growth its `bicubic`; nearest is
//! not offered, since a file enlarged by nearest is a file of blocks.

use std::collections::VecDeque;
use std::sync::OnceLock;

use super::encode::{self, Raster};
use super::{DecodedImage, Samples, Transfer};

/// `image` fitted inside `side` pixels on its longer side, its aspect kept,
/// each output pixel the mean of the block of source pixels it stands for.
/// An image already within `side` comes back as it is.
///
/// Output pixel `(ox, oy)` averages the source columns `ox·w/ow ..
/// (ox+1)·w/ow` and rows likewise, in integer division, so that the blocks
/// tile the source exactly and none is ever empty. A sample equal to the
/// image's no-data sentinel is left out of its block's mean, and a block
/// with nothing else in it writes the sentinel back, so that a masked sea
/// stays masked rather than bleeding its sentinel into the coast. Alpha is
/// averaged like any other channel, unweighted by itself.
pub fn downscale(image: &DecodedImage, side: u32) -> DecodedImage {
    let Some((width, height)) = fitted([image.width, image.height], side) else {
        return image.clone();
    };
    let (w, h) = (image.width as usize, image.height as usize);
    let (ow, oh) = (width as usize, height as usize);
    let samples = match &image.samples {
        Samples::U8 { channels, data } => Samples::U8 {
            channels: *channels,
            data: reduce(data, channels.count(), w, h, ow, oh, image.nodata),
        },
        Samples::U16 { channels, data } => Samples::U16 {
            channels: *channels,
            data: reduce(data, channels.count(), w, h, ow, oh, image.nodata),
        },
        Samples::F32 { channels, data } => Samples::F32 {
            channels: *channels,
            data: reduce(data, channels.count(), w, h, ow, oh, image.nodata),
        },
    };
    DecodedImage {
        width,
        height,
        samples,
        color: image.color,
        alpha: image.alpha,
        referred: image.referred,
        exposure: image.exposure,
        nodata: image.nodata,
        gain_map: None,
    }
}

/// Eight-bit interleaved pixels of `channels` components, fitted inside
/// `side` the same way. For the copy a thumbnail keeps for the screen,
/// which is already the bytes of a PNG.
pub fn downscale_bytes(
    width: u32,
    height: u32,
    channels: usize,
    data: &[u8],
    side: u32,
) -> (u32, u32, Vec<u8>) {
    let Some((ow, oh)) = fitted([width, height], side) else {
        return (width, height, data.to_vec());
    };
    let small = reduce(
        data,
        channels,
        width as usize,
        height as usize,
        ow as usize,
        oh as usize,
        None,
    );
    (ow, oh, small)
}

/// The size an image of `size` comes out at inside `side`: the longer side
/// exactly `side`, the other in proportion and never less than one pixel.
/// `None` for an image that already fits, which is left alone.
fn fitted(size: [u32; 2], side: u32) -> Option<(u32, u32)> {
    let longest = size[0].max(size[1]);
    if longest <= side || size[0] == 0 || size[1] == 0 {
        return None;
    }
    let scale = f64::from(side) / f64::from(longest);
    let along = |length: u32| ((f64::from(length) * scale).round() as u32).max(1);
    Some(if size[0] >= size[1] {
        (side, along(size[1]))
    } else {
        (along(size[0]), side)
    })
}

/// `raster` — the picture as the screen shows it, 8-bit sRGB with straight
/// alpha where it has any — at `size`, its aspect the caller's to keep.
/// The raster itself where `size` is its own, and along an axis that stays
/// the same length the filter is an exact identity. An output pixel that
/// covers more than one source pixel is the average of what it covers,
/// each source pixel weighted by how much of it falls inside; one that
/// covers less is Catmull-Rom over the four source pixels around its
/// center, clamped to the picture's edge, so that a source pixel's center
/// comes through untouched and the negative lobes can undershoot no
/// further than black. Both work in linear light on premultiplied color,
/// as the shaders do, and come back out through the sRGB curve as
/// `encode` writes it.
pub fn resize(raster: Raster, size: [u32; 2]) -> Raster {
    let stride = size[0] as usize * raster.channels.count();
    resize_on(raster, size, encode::bands(stride, size[1] as usize))
}

/// [`resize`], its rows divided between `bands` threads — a choice
/// `resize` makes from the picture, and a test makes to hold a divided
/// walk to a plain one.
pub fn resize_on(raster: Raster, size: [u32; 2], bands: usize) -> Raster {
    let empty = raster.width == 0 || raster.height == 0 || size[0] == 0 || size[1] == 0;
    if [raster.width, raster.height] == size || empty {
        return raster;
    }
    let count = raster.channels.count();
    let columns = taps(raster.width as usize, size[0] as usize);
    let rows = taps(raster.height as usize, size[1] as usize);
    let stride = size[0] as usize * count;
    let height = size[1] as usize;
    let mut data = vec![0u8; stride * height];
    let plan = Plan {
        raster: &raster,
        columns: &columns,
        rows: &rows,
    };
    let bands = bands.clamp(1, height);
    if bands == 1 {
        plan.fill(&mut data, 0);
    } else {
        let per_band = height.div_ceil(bands);
        std::thread::scope(|scope| {
            for (index, band) in data.chunks_mut(stride * per_band).enumerate() {
                let plan = &plan;
                scope.spawn(move || plan.fill(band, index * per_band));
            }
        });
    }
    Raster {
        width: size[0],
        height: size[1],
        channels: raster.channels,
        data,
    }
}

/// The source pixels one output pixel reads along one axis, and what each
/// weighs: `weights[k]` is the weight of source index `start + k`.
#[derive(Clone, Debug, PartialEq)]
struct Taps {
    start: usize,
    weights: Vec<f32>,
}

/// The taps of every output index along an axis `from` long becoming `to`
/// long: an identity where the two are equal, area weights where it
/// shrinks, Catmull-Rom where it grows.
fn taps(from: usize, to: usize) -> Vec<Taps> {
    if to == from {
        return (0..from)
            .map(|start| Taps {
                start,
                weights: vec![1.0],
            })
            .collect();
    }
    let scale = from as f64 / to as f64;
    (0..to)
        .map(|at| {
            if to < from {
                area(at, scale, from)
            } else {
                cubic(at, scale, from)
            }
        })
        .collect()
}

/// Output pixel `at` covers the source span `at·scale .. (at+1)·scale`;
/// each source pixel inside weighs the length of its overlap, as a share
/// of the span. Normalized at the end so that floating-point slack in the
/// span's ends cannot leave the weights short of one.
fn area(at: usize, scale: f64, from: usize) -> Taps {
    let left = at as f64 * scale;
    let right = ((at + 1) as f64 * scale).min(from as f64);
    let start = (left.floor() as usize).min(from - 1);
    let end = (right.ceil() as usize).clamp(start + 1, from);
    let mut weights: Vec<f64> = (start..end)
        .map(|index| (right.min((index + 1) as f64) - left.max(index as f64)).max(0.0))
        .collect();
    while weights.len() > 1 && weights.last() == Some(&0.0) {
        weights.pop();
    }
    let total: f64 = weights.iter().sum();
    Taps {
        start,
        weights: weights.iter().map(|w| (w / total) as f32).collect(),
    }
}

/// Output pixel `at`'s center, in source pixels, and Catmull-Rom over the
/// four source pixels around it; a tap past the picture's edge reads the
/// edge pixel instead, its weight folded onto it.
fn cubic(at: usize, scale: f64, from: usize) -> Taps {
    let center = (at as f64 + 0.5) * scale - 0.5;
    let base = center.floor();
    let weights = catmull_rom((center - base) as f32);
    let base = base as i64;
    let clamp = |index: i64| index.clamp(0, from as i64 - 1) as usize;
    let start = clamp(base - 1);
    let end = clamp(base + 2);
    let mut folded = vec![0.0f32; end - start + 1];
    for (tap, weight) in weights.into_iter().enumerate() {
        folded[clamp(base - 1 + tap as i64) - start] += weight;
    }
    Taps {
        start,
        weights: folded,
    }
}

/// Catmull-Rom's four weights at `offset` past the second tap — the twin of
/// `catmull_rom` in `shaders/image.wgsl`, the B = 0, C = 1/2 member of the
/// cubic family: interpolating, so a source pixel's center comes through
/// untouched, and sharper than bilinear at the cost of a little ringing
/// either side of a hard edge.
fn catmull_rom(offset: f32) -> [f32; 4] {
    let f2 = offset * offset;
    let f3 = f2 * offset;
    [
        (-f3 + 2.0 * f2 - offset) * 0.5,
        (3.0 * f3 - 5.0 * f2 + 2.0) * 0.5,
        (-3.0 * f3 + 4.0 * f2 + offset) * 0.5,
        (f3 - f2) * 0.5,
    ]
}

/// The sRGB curve read the other way from `encode::levels`: what each of
/// the 256 codes is in linear light.
fn linear_of_code() -> &'static [f32; 256] {
    static LINEAR: OnceLock<[f32; 256]> = OnceLock::new();
    LINEAR.get_or_init(|| std::array::from_fn(|code| Transfer::Srgb.to_linear(code as f32 / 255.0)))
}

/// What every band of a resize reads: the raster and the taps of both
/// axes.
struct Plan<'a> {
    raster: &'a Raster,
    columns: &'a [Taps],
    rows: &'a [Taps],
}

impl Plan<'_> {
    /// Writes the output rows of `band`, the first of which is row `first`.
    ///
    /// Each source row is decoded and resampled across once, into a cache
    /// of rows the output rows still to come will read; a row no output
    /// row below it reads is let go. The rows an output row reads run
    /// upward with it, so the cache is a short contiguous run, four rows
    /// deep for a growth and a span's worth for a shrink, never the
    /// picture.
    fn fill(&self, band: &mut [u8], first: usize) {
        let count = self.raster.channels.count();
        let stride = self.columns.len() * count;
        let mut cache: VecDeque<(usize, Vec<f32>)> = VecDeque::new();
        let mut summed = vec![0.0f32; stride];
        for (offset, row) in band.chunks_exact_mut(stride).enumerate() {
            let taps = &self.rows[first + offset];
            while cache.front().is_some_and(|(y, _)| *y < taps.start) {
                cache.pop_front();
            }
            let next = cache.back().map_or(taps.start, |(y, _)| y + 1);
            for y in next..taps.start + taps.weights.len() {
                cache.push_back((y, self.across(y)));
            }
            summed.fill(0.0);
            for (k, weight) in taps.weights.iter().enumerate() {
                let (_, across) = &cache[k];
                for (sum, value) in summed.iter_mut().zip(across) {
                    *sum += weight * value;
                }
            }
            self.encode(&summed, row);
        }
    }

    /// Source row `y` in linear light, premultiplied, resampled across to
    /// the output width.
    fn across(&self, y: usize) -> Vec<f32> {
        let count = self.raster.channels.count();
        let alpha = self.raster.channels.alpha_index();
        let width = self.raster.width as usize;
        let linear = linear_of_code();
        let source = &self.raster.data[y * width * count..(y + 1) * width * count];
        let mut decoded = vec![0.0f32; width * count];
        for (pixel, out) in source
            .chunks_exact(count)
            .zip(decoded.chunks_exact_mut(count))
        {
            let coverage = alpha.map_or(1.0, |index| f32::from(pixel[index]) / 255.0);
            for (channel, (code, slot)) in pixel.iter().zip(out.iter_mut()).enumerate() {
                *slot = if Some(channel) == alpha {
                    coverage
                } else {
                    linear[usize::from(*code)] * coverage
                };
            }
        }
        let mut out = vec![0.0f32; self.columns.len() * count];
        for (taps, pixel) in self.columns.iter().zip(out.chunks_exact_mut(count)) {
            for (k, weight) in taps.weights.iter().enumerate() {
                let x = taps.start + k;
                for (slot, value) in pixel.iter_mut().zip(&decoded[x * count..(x + 1) * count]) {
                    *slot += weight * value;
                }
            }
        }
        out
    }

    /// One resampled row, premultiplied and linear, as the bytes the
    /// raster stores: the color divided back out by its coverage and taken
    /// through the sRGB curve, the coverage written as it is. A pixel with
    /// no coverage has no color to speak of and is written black.
    fn encode(&self, summed: &[f32], row: &mut [u8]) {
        let count = self.raster.channels.count();
        let alpha = self.raster.channels.alpha_index();
        let levels = encode::levels();
        for (pixel, out) in summed.chunks_exact(count).zip(row.chunks_exact_mut(count)) {
            let coverage = alpha.map_or(1.0, |index| pixel[index].clamp(0.0, 1.0));
            for (channel, (value, slot)) in pixel.iter().zip(out.iter_mut()).enumerate() {
                *slot = if Some(channel) == alpha {
                    encode::byte(coverage)
                } else if coverage > 0.0 {
                    encode::quantize(value / coverage, levels)
                } else {
                    0
                };
            }
        }
    }
}

/// A component the filter can average: read out to a double, and written
/// back from one.
trait Component: Copy {
    fn to_f64(self) -> f64;
    fn from_f64(mean: f64) -> Self;
}

impl Component for u8 {
    fn to_f64(self) -> f64 {
        f64::from(self)
    }
    fn from_f64(mean: f64) -> Self {
        mean.round().clamp(0.0, 255.0) as u8
    }
}

impl Component for u16 {
    fn to_f64(self) -> f64 {
        f64::from(self)
    }
    fn from_f64(mean: f64) -> Self {
        mean.round().clamp(0.0, 65535.0) as u16
    }
}

impl Component for f32 {
    fn to_f64(self) -> f64 {
        f64::from(self)
    }
    fn from_f64(mean: f64) -> Self {
        mean as f32
    }
}

/// The block of the source axis `full` long that output cell `at` of `out`
/// stands for.
fn block(at: usize, full: usize, out: usize) -> std::ops::Range<usize> {
    (at * full / out)..((at + 1) * full / out)
}

/// The mean of every block, channel by channel. Sums are doubles, which
/// hold an integer sum exactly up to 2⁵³ — more than any block of 16-bit
/// samples this program will ever hold comes to.
fn reduce<T: Component>(
    data: &[T],
    channels: usize,
    w: usize,
    h: usize,
    ow: usize,
    oh: usize,
    nodata: Option<f32>,
) -> Vec<T> {
    let nodata = nodata.map(f64::from);
    let mut out = Vec::with_capacity(ow * oh * channels);
    let mut sums = vec![0.0f64; channels];
    let mut counts = vec![0usize; channels];
    for oy in 0..oh {
        let rows = block(oy, h, oh);
        for ox in 0..ow {
            let columns = block(ox, w, ow);
            sums.fill(0.0);
            counts.fill(0);
            for y in rows.clone() {
                let row =
                    &data[(y * w + columns.start) * channels..(y * w + columns.end) * channels];
                for pixel in row.chunks_exact(channels) {
                    for (channel, sample) in pixel.iter().enumerate() {
                        let value = sample.to_f64();
                        if nodata.is_some_and(|sentinel| value == sentinel) {
                            continue;
                        }
                        sums[channel] += value;
                        counts[channel] += 1;
                    }
                }
            }
            for channel in 0..channels {
                out.push(match (counts[channel], nodata) {
                    (0, Some(sentinel)) => T::from_f64(sentinel),
                    (0, None) => T::from_f64(0.0),
                    (count, _) => T::from_f64(sums[channel] / count as f64),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::{AlphaMode, Channels, ColorSpace, Referred};

    /// The bytes `image` would be exported as, unchanged by the display:
    /// what the resize is handed.
    fn raster(channels: Channels, width: u32, height: u32, data: Vec<u8>) -> Raster {
        assert_eq!(
            data.len(),
            width as usize * height as usize * channels.count()
        );
        Raster {
            width,
            height,
            channels,
            data,
        }
    }

    /// The code a linear value is written as.
    fn code(linear: f32) -> u8 {
        encode::quantize(linear, encode::levels())
    }

    /// A raster asked for at its own size is handed back as it is, and an
    /// axis that keeps its length is an exact identity even while the
    /// other changes.
    #[test]
    fn a_resize_to_the_same_size_is_the_raster_itself() {
        let same = resize(raster(Channels::Gray, 3, 2, vec![1, 2, 3, 4, 5, 6]), [3, 2]);
        assert_eq!(same.data, vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(
            taps(3, 3),
            vec![
                Taps {
                    start: 0,
                    weights: vec![1.0]
                },
                Taps {
                    start: 1,
                    weights: vec![1.0]
                },
                Taps {
                    start: 2,
                    weights: vec![1.0]
                },
            ]
        );
        // Halved in height alone: each column is the mean of its two rows,
        // and the columns are as they were.
        let halved = resize(
            raster(Channels::Gray, 3, 2, vec![10, 20, 30, 10, 20, 30]),
            [3, 1],
        );
        assert_eq!((halved.width, halved.height), (3, 1));
        assert_eq!(halved.data, vec![10, 20, 30]);
    }

    /// Shrinking averages in linear light: the mean of black and white is
    /// the code of half the light, not the middle code.
    #[test]
    fn a_shrink_averages_what_each_pixel_covers_in_linear_light() {
        let small = resize(raster(Channels::Gray, 4, 1, vec![0, 255, 0, 255]), [2, 1]);
        assert_eq!(small.data, vec![code(0.5), code(0.5)]);
        assert_eq!(code(0.5), 188, "not 128");
        // Three into two: the middle pixel is shared, a third of each
        // output pixel.
        let across = taps(3, 2);
        assert_eq!(across[0].start, 0);
        assert_eq!(across[1].start, 1);
        for (weights, expected) in [
            (&across[0].weights, [2.0 / 3.0, 1.0 / 3.0]),
            (&across[1].weights, [1.0 / 3.0, 2.0 / 3.0]),
        ] {
            assert_eq!(weights.len(), 2);
            for (weight, expected) in weights.iter().zip(expected) {
                assert!((weight - expected).abs() < 1e-6, "{weights:?}");
            }
        }
        let uneven = resize(raster(Channels::Gray, 3, 1, vec![0, 0, 255]), [2, 1]);
        assert_eq!(uneven.data, vec![0, code(2.0 / 3.0)]);
    }

    /// Growing is Catmull-Rom: a source pixel whose center an output pixel
    /// lands on comes through untouched, and the ends of the picture read
    /// the edge pixel rather than nothing.
    #[test]
    fn a_growth_keeps_the_source_centers() {
        // Tripled, output pixels 1 and 4 sit on the two source centers.
        let big = resize(raster(Channels::Gray, 2, 1, vec![40, 200]), [6, 1]);
        assert_eq!((big.width, big.height), (6, 1));
        assert_eq!(big.data[1], 40);
        assert_eq!(big.data[4], 200);
        let across = taps(2, 6);
        assert_eq!(across[1].weights.iter().sum::<f32>(), 1.0);
        assert!(
            across
                .iter()
                .all(|taps| taps.start + taps.weights.len() <= 2),
            "clamped to the edge: {across:?}"
        );
        // A flat picture stays flat however it grows: the negative lobes
        // cancel exactly.
        let flat = resize(raster(Channels::Rgb, 2, 2, vec![90; 12]), [7, 5]);
        assert!(flat.data.iter().all(|&code| code == 90), "{:?}", flat.data);
        // An undershoot past black is clamped, not wrapped.
        let edge = resize(raster(Channels::Gray, 4, 1, vec![0, 0, 255, 255]), [8, 1]);
        assert_eq!(edge.data[..3], [0, 0, 0]);
        assert!(edge.data[3] < edge.data[4]);
        assert_eq!(catmull_rom(0.0), [0.0, 1.0, 0.0, 0.0]);
    }

    /// Color is premultiplied before it is filtered: a transparent black
    /// pixel beside an opaque red one does not darken the red, and the
    /// coverage averages on its own.
    #[test]
    fn transparent_pixels_do_not_bleed_their_color() {
        let mixed = resize(
            raster(Channels::Rgba, 2, 1, vec![255, 0, 0, 255, 0, 0, 0, 0]),
            [1, 1],
        );
        assert_eq!(mixed.data, vec![255, 0, 0, 128]);
        let gray = resize(
            raster(Channels::GrayAlpha, 2, 1, vec![200, 255, 0, 0]),
            [1, 1],
        );
        assert_eq!(gray.data, vec![200, 128]);
        // No coverage at all: black, and no division by nothing.
        let none = resize(
            raster(
                Channels::Rgba,
                2,
                1,
                vec![255, 255, 255, 0, 255, 255, 255, 0],
            ),
            [1, 1],
        );
        assert_eq!(none.data, vec![0, 0, 0, 0]);
    }

    /// The rows divided between threads come out as one thread would have
    /// written them: the split is over who does the work, not what it is.
    #[test]
    fn a_divided_resize_agrees_with_a_plain_one() {
        let (width, height) = (37u32, 29u32);
        let data: Vec<u8> = (0..width * height * 3)
            .map(|i| (i * 7 % 251) as u8)
            .collect();
        for size in [[11, 8], [80, 61], [37, 50], [50, 29]] {
            let plain = resize_on(raster(Channels::Rgb, width, height, data.clone()), size, 1);
            let divided = resize_on(raster(Channels::Rgb, width, height, data.clone()), size, 5);
            assert_eq!(plain.data, divided.data, "{size:?}");
            assert_eq!((plain.width, plain.height), (size[0], size[1]));
        }
    }

    fn gray_u8(width: u32, height: u32, data: Vec<u8>) -> DecodedImage {
        DecodedImage::new(
            width,
            height,
            Samples::U8 {
                channels: Channels::Gray,
                data,
            },
            ColorSpace::SRGB,
            AlphaMode::Opaque,
        )
    }

    /// Every output pixel is the mean of its block, rounded, and the
    /// description travels with the pixels.
    #[test]
    fn a_block_comes_out_as_its_mean() {
        let image = gray_u8(
            4,
            4,
            vec![
                0, 10, 100, 110, //
                20, 30, 120, 130, //
                200, 210, 40, 50, //
                220, 230, 60, 70,
            ],
        );
        let small = downscale(&image, 2);
        assert_eq!((small.width, small.height), (2, 2));
        assert_eq!(small.color, image.color);
        assert_eq!(small.referred, Referred::Display);
        match small.samples {
            Samples::U8 { channels, data } => {
                assert_eq!(channels, Channels::Gray);
                assert_eq!(data, vec![15, 115, 215, 55]);
            }
            other => panic!("{other:?}"),
        }
    }

    /// A sentinel is left out of the mean, and a block that is nothing but
    /// sentinel stays sentinel.
    #[test]
    fn nodata_is_skipped_and_kept() {
        let mut image = DecodedImage::new(
            4,
            2,
            Samples::F32 {
                channels: Channels::Gray,
                data: vec![
                    1.0, -9999.0, -9999.0, -9999.0, //
                    3.0, 5.0, -9999.0, -9999.0,
                ],
            },
            ColorSpace {
                transfer: crate::image::Transfer::Linear,
                primaries: crate::image::Primaries::Bt709,
            },
            AlphaMode::Opaque,
        );
        image.nodata = Some(-9999.0);
        let small = downscale(&image, 2);
        assert_eq!((small.width, small.height), (2, 1));
        assert_eq!(small.nodata, Some(-9999.0));
        match small.samples {
            Samples::F32 { data, .. } => assert_eq!(data, vec![3.0, -9999.0]),
            other => panic!("{other:?}"),
        }
    }

    /// An image that already fits is handed back as it is, never enlarged.
    #[test]
    fn a_small_image_is_left_alone() {
        let image = gray_u8(3, 2, vec![1, 2, 3, 4, 5, 6]);
        let same = downscale(&image, 512);
        assert_eq!((same.width, same.height), (3, 2));
        assert_eq!(same.samples.len(), 6);
        assert_eq!(fitted([3, 2], 512), None);
        assert_eq!(fitted([1024, 768], 512), Some((512, 384)));
        assert_eq!(fitted([100, 4000], 512), Some((13, 512)));
        assert_eq!(fitted([4000, 1], 512), Some((512, 1)));
    }

    /// Blocks that do not divide evenly tile the source without a gap: one
    /// block takes two columns and the other one, and every column is in
    /// exactly one of them.
    #[test]
    fn uneven_blocks_tile_the_source() {
        assert_eq!(block(0, 3, 2), 0..1);
        assert_eq!(block(1, 3, 2), 1..3);
        let image = gray_u8(3, 1, vec![10, 20, 40]);
        let (w, h, data) = downscale_bytes(3, 1, 1, &[10, 20, 40], 2);
        assert_eq!((w, h), (2, 1));
        assert_eq!(data, vec![10, 30]);
        let small = downscale(&image, 2);
        assert!(matches!(small.samples, Samples::U8 { ref data, .. } if *data == vec![10, 30]));
    }

    /// Interleaved channels are averaged each with its own kind, and a
    /// 16-bit mean rounds to the nearest code.
    #[test]
    fn channels_are_averaged_apart() {
        let image = DecodedImage::new(
            2,
            1,
            Samples::U16 {
                channels: Channels::GrayAlpha,
                data: vec![1000, 65535, 1001, 0],
            },
            ColorSpace::SRGB,
            AlphaMode::Straight,
        );
        let small = downscale(&image, 1);
        match small.samples {
            Samples::U16 { data, .. } => assert_eq!(data, vec![1001, 32768]),
            other => panic!("{other:?}"),
        }
        assert_eq!(small.alpha, AlphaMode::Straight);
    }
}
