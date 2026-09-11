//! Making an image smaller on the CPU: a box filter, for the thumbnails.
//!
//! The GPU has its own coarse chain for minifying the picture on screen —
//! see `render/reduce.rs` — and this is not that. A thumbnail is made off
//! the event loop by a thread that holds no device, and it is made once and
//! kept, so what matters is that it is cheap in memory and exact in what it
//! averages, not that it is fast.
//!
//! The filter works in the file's own encoding, over the file's own
//! samples, and describes rather than normalizes: the small image it hands
//! back has the same sample type, color space, alpha mode and no-data
//! sentinel as the one it was given, so that everything downstream — the
//! statistics scan, the display window, the encode — reads it exactly as it
//! would have read the full image. Averaging encoded values rather than
//! linear light is the thumbnail's bargain: a thumbnail is a picture of the
//! picture, not a measurement of it.

use super::{DecodedImage, Samples};

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
        nodata: image.nodata,
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
