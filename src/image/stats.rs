//! Value statistics, used to choose a sensible window for images whose
//! numbers do not conveniently fill 0..1 — 12-bit sensor data stored in
//! 16-bit containers, or HDR frames with a few very bright highlights.

use super::{Channels, ColorSpace, DecodedImage, Samples};

/// Bins are plenty for percentile work and cheap to keep around; the UI draws
/// this directly as a histogram.
pub const BINS: usize = 256;

/// At most this many pixels are examined; larger images are sampled on a
/// stride. Enough for stable percentiles, fast enough to run on every load.
const MAX_SAMPLED_PIXELS: usize = 1 << 21;

/// Red, green, blue and luminance counts for a colour image, all binned over
/// one shared axis so the four can be drawn on top of each other.
///
/// This is a separate scan from [`Stats::histogram`], which stays on the
/// luminance range because that is what the exposure controls read. Colour
/// channels routinely run past it — a saturated red pixel is 1.0 in red but
/// only 0.21 in luminance — so they need an axis wide enough to hold them.
#[derive(Clone, Debug)]
pub struct ChannelStats {
    /// Smallest and largest value seen in any of the three colour channels.
    /// Luminance is a weighted average of them and so always falls inside.
    pub min: f32,
    pub max: f32,
    /// Counts over `min..max`: red, green, blue, then luminance.
    pub bins: [[u32; BINS]; 4],
}

impl ChannelStats {
    /// Index of the luminance plane in [`ChannelStats::bins`].
    pub const LUMA: usize = 3;
}

#[derive(Clone, Debug)]
pub struct Stats {
    /// Smallest and largest linear value seen, in working-space units.
    pub min: f32,
    pub max: f32,
    /// Counts over `min..max`, for percentiles and for drawing.
    pub histogram: [u32; BINS],
    pub counted: u32,
    /// Per-channel counts, for images that carry colour.
    pub channels: Option<ChannelStats>,
}

impl Stats {
    /// Scans `image`, converting to the linear working space as it goes so
    /// that the numbers line up with what the shader will sample.
    ///
    /// Grey images are measured on their single channel; colour images on
    /// relative luminance, which is what an exposure control should track.
    /// Colour images additionally get per-channel counts, which the UI draws
    /// as a four-channel histogram.
    pub fn scan(image: &DecodedImage) -> Self {
        let channels = image.samples.channels();
        let stride = Self::stride(image);
        let values = Values::new(image, channels, stride);
        let nodata = image.nodata;
        let is_data =
            move |value: f32| value.is_finite() && nodata.is_none_or(|sentinel| value != sentinel);

        let colour = !channels.is_gray();
        let (mut min, mut max) = (f32::INFINITY, f32::NEG_INFINITY);
        let (mut channel_min, mut channel_max) = (f32::INFINITY, f32::NEG_INFINITY);
        values.clone().for_each(|pixel| {
            let value = luminance(pixel, channels);
            if is_data(value) {
                min = min.min(value);
                max = max.max(value);
            }
            if colour {
                for &component in &pixel[..COLOUR] {
                    if is_data(component) {
                        channel_min = channel_min.min(component);
                        channel_max = channel_max.max(component);
                    }
                }
            }
        });

        if !min.is_finite() || !max.is_finite() {
            // Nothing measurable at all; fall back to unit range so
            // the rest of the pipeline still has something usable.
            return Self {
                min: 0.0,
                max: 1.0,
                histogram: [0; BINS],
                counted: 0,
                channels: None,
            };
        }

        let mut histogram = [0u32; BINS];
        let mut counted = 0u32;
        let span = max - min;
        let scale = (span > 0.0).then(|| (BINS - 1) as f32 / span);

        let channel_span = channel_max - channel_min;
        let mut channel_stats = if colour && channel_span > 0.0 {
            Some(ChannelStats {
                min: channel_min,
                max: channel_max,
                bins: [[0; BINS]; 4],
            })
        } else {
            None
        };
        let channel_scale = (BINS - 1) as f32 / channel_span;

        if scale.is_some() || channel_stats.is_some() {
            values.for_each(|pixel| {
                let value = luminance(pixel, channels);
                let counts = is_data(value);
                if let Some(scale) = scale
                    && counts
                {
                    let bin = ((value - min) * scale) as usize;
                    histogram[bin.min(BINS - 1)] += 1;
                    counted += 1;
                }
                if let Some(stats) = channel_stats.as_mut() {
                    let mut bin = |plane: usize, value: f32| {
                        let index = ((value - channel_min) * channel_scale) as usize;
                        stats.bins[plane][index.min(BINS - 1)] += 1;
                    };
                    for (plane, &component) in pixel[..COLOUR].iter().enumerate() {
                        if is_data(component) {
                            bin(plane, component);
                        }
                    }
                    if counts {
                        bin(ChannelStats::LUMA, value);
                    }
                }
            });
        }

        Self {
            min,
            max,
            histogram,
            counted,
            channels: channel_stats,
        }
    }

    fn stride(image: &DecodedImage) -> usize {
        let pixels = image.width as usize * image.height as usize;
        pixels.div_ceil(MAX_SAMPLED_PIXELS).max(1)
    }

    /// The value below which `fraction` of the samples fall, `fraction` in
    /// 0..=1. Returns `min`/`max` at the extremes.
    pub fn percentile(&self, fraction: f32) -> f32 {
        if self.counted == 0 || self.max <= self.min {
            return if fraction <= 0.0 { self.min } else { self.max };
        }
        let target = (fraction.clamp(0.0, 1.0) * self.counted as f32) as u32;
        let mut running = 0u32;
        for (index, count) in self.histogram.iter().enumerate() {
            running += count;
            if running >= target {
                let position = index as f32 / (BINS - 1) as f32;
                return self.min + position * (self.max - self.min);
            }
        }
        self.max
    }
}

/// Number of colour components a pixel carries, alpha aside.
const COLOUR: usize = 3;

/// Grey uses its one channel; colour collapses to relative luminance,
/// weighted for the BT.709 primaries the working space uses.
fn luminance(pixel: &[f32], channels: Channels) -> f32 {
    if channels.is_gray() {
        pixel[0]
    } else {
        0.2126 * pixel[0] + 0.7152 * pixel[1] + 0.0722 * pixel[2]
    }
}

/// Iterator over the linear components of each sampled pixel.
#[derive(Clone)]
struct Values<'a> {
    samples: &'a Samples,
    color: ColorSpace,
    channels: Channels,
    stride: usize,
}

impl<'a> Values<'a> {
    fn new(image: &'a DecodedImage, channels: Channels, stride: usize) -> Self {
        Self {
            samples: &image.samples,
            color: image.color,
            channels,
            stride,
        }
    }

    /// Calls `visit` with one pixel's worth of linear components, alpha
    /// included where the image has one.
    fn for_each(self, mut visit: impl FnMut(&[f32])) {
        let count = self.channels.count();
        let transfer = self.color.transfer;

        match self.samples {
            Samples::U8 { data, .. } => {
                let lut: Vec<f32> = (0..=u8::MAX)
                    .map(|v| transfer.to_linear(v as f32 / u8::MAX as f32))
                    .collect();
                let mut pixel = [0.0f32; 4];
                for chunk in data.chunks_exact(count).step_by(self.stride) {
                    for (slot, raw) in pixel.iter_mut().zip(chunk) {
                        *slot = lut[*raw as usize];
                    }
                    visit(&pixel[..count]);
                }
            }
            Samples::U16 { data, .. } => {
                let scale = 1.0 / u16::MAX as f32;
                let mut pixel = [0.0f32; 4];
                for chunk in data.chunks_exact(count).step_by(self.stride) {
                    for (slot, raw) in pixel.iter_mut().zip(chunk) {
                        *slot = transfer.to_linear(*raw as f32 * scale);
                    }
                    visit(&pixel[..count]);
                }
            }
            Samples::F32 { data, .. } => {
                let mut pixel = [0.0f32; 4];
                for chunk in data.chunks_exact(count).step_by(self.stride) {
                    for (slot, raw) in pixel.iter_mut().zip(chunk) {
                        *slot = if transfer.is_linear() {
                            *raw
                        } else {
                            transfer.to_linear(*raw)
                        };
                    }
                    visit(&pixel[..count]);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::{AlphaMode, ColorSpace, Transfer};

    fn linear_gray(data: Vec<u16>) -> DecodedImage {
        DecodedImage {
            width: data.len() as u32,
            height: 1,
            samples: Samples::U16 {
                channels: Channels::Gray,
                data,
            },
            color: ColorSpace::LINEAR_BT709,
            alpha: AlphaMode::Opaque,
            value_range: None,
            nodata: None,
        }
    }

    /// The case the whole windowing feature exists for: 12-bit data in a
    /// 16-bit container occupies a sixteenth of the nominal range, so showing
    /// it unwindowed would be a nearly black rectangle.
    #[test]
    fn finds_the_range_of_twelve_bit_data_in_a_sixteen_bit_container() {
        let stats = Stats::scan(&linear_gray(vec![0, 1000, 2000, 4095]));
        assert!(stats.min.abs() < 1e-6);
        assert!((stats.max - 4095.0 / 65535.0).abs() < 1e-5, "{}", stats.max);
        assert!(stats.max < 0.07);
    }

    #[test]
    fn percentiles_ignore_a_lone_hot_pixel() {
        // A thousand samples near the bottom plus one at full scale.
        let mut data = vec![100u16; 1000];
        data.push(u16::MAX);
        let stats = Stats::scan(&linear_gray(data));

        assert!((stats.max - 1.0).abs() < 1e-6, "min/max still sees it");
        // The upper percentile does not, which is the point.
        assert!(stats.percentile(0.999) < 0.5, "{}", stats.percentile(0.999));
    }

    #[test]
    fn a_flat_image_gives_a_degenerate_but_usable_range() {
        let stats = Stats::scan(&linear_gray(vec![500; 16]));
        assert!((stats.min - stats.max).abs() < 1e-9);
        // Percentiles must not divide by the zero-width span.
        assert!(stats.percentile(0.5).is_finite());
    }

    #[test]
    fn statistics_are_gathered_after_the_transfer_decode() {
        let mut image = linear_gray(vec![u16::MAX / 2]);
        let linear = Stats::scan(&image).max;

        image.color.transfer = Transfer::Srgb;
        let decoded = Stats::scan(&image).max;

        assert!((linear - 0.5).abs() < 1e-3, "{linear}");
        assert!((decoded - 0.2140).abs() < 1e-3, "{decoded}");
    }

    #[test]
    fn colour_images_are_measured_on_luminance() {
        let green = DecodedImage {
            width: 1,
            height: 1,
            samples: Samples::F32 {
                channels: Channels::Rgb,
                data: vec![0.0, 1.0, 0.0],
            },
            color: ColorSpace::LINEAR_BT709,
            alpha: AlphaMode::Opaque,
            value_range: None,
            nodata: None,
        };
        // BT.709 luminance weights green at 0.7152.
        assert!((Stats::scan(&green).max - 0.7152).abs() < 1e-4);
    }

    fn rgb_f32(data: Vec<f32>) -> DecodedImage {
        DecodedImage {
            width: (data.len() / 3) as u32,
            height: 1,
            samples: Samples::F32 {
                channels: Channels::Rgb,
                data,
            },
            color: ColorSpace::LINEAR_BT709,
            alpha: AlphaMode::Opaque,
            value_range: None,
            nodata: None,
        }
    }

    /// The point of the separate axis: saturated colour runs well past the
    /// luminance range, and binning it there would pile it into the last bin.
    #[test]
    fn colour_channels_are_binned_over_their_own_range() {
        let stats = Stats::scan(&rgb_f32(vec![1.0, 0.0, 0.0]));
        let channels = stats.channels.expect("an rgb image has channel counts");

        assert!((stats.max - 0.2126).abs() < 1e-4, "luminance is unchanged");
        assert!(channels.min.abs() < 1e-6);
        assert!((channels.max - 1.0).abs() < 1e-6);
        // Red at the top of the axis, green and blue at the bottom.
        assert_eq!(channels.bins[0][BINS - 1], 1);
        assert_eq!(channels.bins[1][0], 1);
        assert_eq!(channels.bins[2][0], 1);
        // Luminance sits between them, on the same axis as the rest.
        assert_eq!(channels.bins[ChannelStats::LUMA].iter().sum::<u32>(), 1);
        assert_eq!(
            channels.bins[ChannelStats::LUMA][(0.2126 * 255.0) as usize],
            1
        );
    }

    #[test]
    fn every_plane_counts_every_pixel() {
        let stats = Stats::scan(&rgb_f32(vec![0.25, 0.5, 0.75, 0.0, 0.5, 1.0]));
        let channels = stats.channels.expect("an rgb image has channel counts");
        for plane in &channels.bins {
            assert_eq!(plane.iter().sum::<u32>(), 2);
        }
        assert_eq!(stats.counted, 2);
    }

    #[test]
    fn alpha_is_left_out_of_the_channel_histogram() {
        let opaque = Stats::scan(&rgb_f32(vec![0.25, 0.5, 0.75]));
        let image = DecodedImage {
            samples: Samples::F32 {
                channels: Channels::Rgba,
                data: vec![0.25, 0.5, 0.75, 1.0],
            },
            width: 1,
            ..rgb_f32(vec![0.0, 0.0, 0.0])
        };
        let with_alpha = Stats::scan(&image).channels.expect("rgba is colour");
        let opaque = opaque.channels.expect("rgb is colour");

        assert_eq!(with_alpha.max, opaque.max, "alpha would have widened this");
        assert_eq!(with_alpha.bins, opaque.bins);
    }

    #[test]
    fn grey_images_have_no_channel_histogram() {
        assert!(
            Stats::scan(&linear_gray(vec![0, 1000, 4095]))
                .channels
                .is_none()
        );
    }

    /// A flat colour image has nothing to plot per channel, and binning it
    /// would divide by a zero-width span.
    #[test]
    fn a_flat_colour_image_gets_no_channel_histogram() {
        assert!(
            Stats::scan(&rgb_f32(vec![0.5, 0.5, 0.5]))
                .channels
                .is_none()
        );
    }

    #[test]
    fn non_finite_samples_do_not_poison_the_range() {
        let image = DecodedImage {
            width: 4,
            height: 1,
            samples: Samples::F32 {
                channels: Channels::Gray,
                data: vec![0.25, f32::NAN, f32::INFINITY, 0.75],
            },
            color: ColorSpace::LINEAR_BT709,
            alpha: AlphaMode::Opaque,
            value_range: None,
            nodata: None,
        };
        let stats = Stats::scan(&image);
        assert!((stats.min - 0.25).abs() < 1e-6);
        assert!((stats.max - 0.75).abs() < 1e-6);
    }
}
