//! Value statistics, used to choose a sensible window for images whose
//! numbers do not conveniently fill 0..1 — 12-bit sensor data stored in
//! 16-bit containers, or HDR frames with a few very bright highlights.

use super::{Channels, ColorSpace, DecodedImage, Sample, Samples, Transfer};

/// Bins are plenty for percentile work and cheap to keep around; the UI draws
/// this directly as a histogram.
pub const BINS: usize = 256;

/// At most this many pixels are examined; larger images are sampled on a
/// stride. Enough for stable percentiles, fast enough to run on every load.
const MAX_SAMPLED_PIXELS: usize = 1 << 21;

/// Number of colour components a pixel carries, alpha aside.
pub const COLOUR: usize = 3;

/// The histogram the interface draws.
///
/// Binned on the curve the file stores its samples with, rather than on the
/// linear values the rest of [`Stats`] is measured in. Two reasons, and the
/// first is a correctness one:
///
/// Uniform bins over decoded values cannot be filled evenly by a quantised
/// file. A code step near white is several times wider in linear terms than
/// one near black — for 8-bit sRGB, 0.0089 against a bin width of 0.0039 — so
/// the highlights come out as a comb of spikes with empty bins between them
/// while a dozen shadow codes pile into bin zero. On the storage curve the
/// codes land one to a bin, or denser, and the comb cannot arise.
///
/// The second is that the eye's response is close to the curve a
/// display-referred file is encoded with, so plotting against it gives equal
/// width to equal perceived steps: mid grey sits in the middle rather than a
/// fifth of the way along. Scene-referred files store linear samples, so
/// their plot stays linear, which is what measurement work wants.
///
/// The axis follows the same split. A curved file is display-referred, so it
/// spans the range such a file can hold — 0..1, widened by any over-range
/// float samples — which puts clipping at either end where you can see it and
/// lands 8-bit codes one to a bin. Linear samples have no nominal range to
/// speak of, since the case that matters is 12-bit data in a 16-bit
/// container, so their axis is the range actually measured.
#[derive(Clone, Debug)]
pub struct Plot {
    /// What the bins span, in the file's own encoding.
    pub min: f32,
    pub max: f32,
    /// Counts over `min..max` for the luminance of each sampled pixel.
    pub luma: [u32; BINS],
    /// Red, green and blue, for an image that carries colour. Their range is
    /// what sets the axis; luminance is a weighted average of them and so
    /// always falls inside it.
    pub colour: Option<[[u32; BINS]; COLOUR]>,
}

impl Plot {
    /// Which bin of the luminance plane one pixel read back out of `image`
    /// was counted in, so that a marker can point at the bar its own pixel is
    /// part of.
    ///
    /// The twin of the luminance pass in [`Stats::scan`], and it has to be
    /// worked out from the file's components rather than from
    /// [`Sample::color`]: the scan bins what the transfer function alone
    /// makes of them, where that colour has also been carried into the
    /// working space and had its premultiplication undone. On a P3 file, or
    /// one with premultiplied alpha, a bin taken from the colour would name a
    /// bar the pixel is not in.
    ///
    /// `None` where there is no bar to point at: an axis with no span, a
    /// value the scan would have thrown out as nodata, or one outside the
    /// range it measured — which a strided scan can miss, and which the plot
    /// therefore does not cover.
    pub fn bin_of(&self, image: &DecodedImage, sample: &Sample) -> Option<usize> {
        let transfer = image.color.transfer;
        let scale = 1.0 / image.samples.full_scale();
        let mut linear = [0.0f32; 4];
        for (slot, stored) in linear.iter_mut().zip(sample.stored()) {
            *slot = transfer.to_linear(stored * scale);
        }
        let channels = sample.channels;
        let value = luminance(&linear[..channels.count()], channels);
        if !value.is_finite() || image.nodata.is_some_and(|sentinel| value == sentinel) {
            return None;
        }
        self.bin(encode(transfer, value))
    }

    /// Which bin a value already on the plot's own axis falls in: the same
    /// arithmetic the scan bins with, so the two cannot drift apart.
    ///
    /// Out of range is `None` rather than the end bin the scan clamps to. The
    /// scan clamps because every value it sees is one the axis was measured
    /// from and a clamp there is only guarding the arithmetic; a marker asked
    /// about a value off the axis has genuinely nowhere to stand, and one
    /// held at the edge would claim a bar that is not the pixel's.
    fn bin(&self, stored: f32) -> Option<usize> {
        // Written so that a non-finite axis fails the test as well: every
        // comparison against a NaN is false, and the positive form would let
        // one through to divide by it.
        let span = self.max - self.min;
        if !span.is_finite() || span <= f32::MIN_POSITIVE {
            return None;
        }
        let bin = (stored - self.min) * ((BINS - 1) as f32 / span) + 0.5;
        (0.0..BINS as f32).contains(&bin).then_some(bin as usize)
    }

    fn empty() -> Self {
        Self {
            min: 0.0,
            max: 1.0,
            luma: [0; BINS],
            colour: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Stats {
    /// Smallest and largest linear value seen, in working-space units.
    pub min: f32,
    pub max: f32,
    /// Counts over `min..max`, in linear units, for percentiles.
    pub histogram: [u32; BINS],
    pub counted: u32,
    /// The same pixels binned for drawing. See [`Plot`] for why it is a
    /// second scan rather than the same one.
    pub plot: Plot,
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
        let transfer = image.color.transfer;
        let nodata = image.nodata;
        let is_data =
            move |value: f32| value.is_finite() && nodata.is_none_or(|sentinel| value != sentinel);

        let colour = !channels.is_gray();
        let plotted = if colour { COLOUR } else { 1 };
        let (mut min, mut max) = (f32::INFINITY, f32::NEG_INFINITY);
        let (mut axis_min, mut axis_max) = if transfer.is_linear() {
            (f32::INFINITY, f32::NEG_INFINITY)
        } else {
            (0.0, 1.0)
        };
        values.clone().for_each(|encoded, linear| {
            let value = luminance(linear, channels);
            if is_data(value) {
                min = min.min(value);
                max = max.max(value);
            }
            // The axis spans the channels that get plotted, in the units they
            // are plotted in.
            for (stored, decoded) in encoded[..plotted].iter().zip(linear) {
                if is_data(*decoded) {
                    axis_min = axis_min.min(*stored);
                    axis_max = axis_max.max(*stored);
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
                plot: Plot::empty(),
            };
        }

        let mut histogram = [0u32; BINS];
        let mut counted = 0u32;
        let span = max - min;
        // A span at or below the smallest normal float is treated as flat:
        // dividing by a subnormal gives an infinite scale, which then bins
        // every sample into the two ends. The clamps downstream keep it from
        // going out of range, but the plot it draws is a lie.
        let scale = (span > f32::MIN_POSITIVE).then(|| (BINS - 1) as f32 / span);

        let axis_span = axis_max - axis_min;
        let mut plot = Plot {
            min: axis_min,
            max: axis_max,
            luma: [0; BINS],
            colour: if colour {
                Some([[0; BINS]; COLOUR])
            } else {
                None
            },
        };
        // A flat image spans nothing to plot against; leaving the counts at
        // zero draws an empty panel rather than one misleading spike.
        let axis_scale = (axis_span > f32::MIN_POSITIVE).then(|| (BINS - 1) as f32 / axis_span);

        if scale.is_some() || axis_scale.is_some() {
            values.for_each(|encoded, linear| {
                let value = luminance(linear, channels);
                let counts = is_data(value);
                if let Some(scale) = scale
                    && counts
                {
                    // Rounded, not truncated: bins are sample points spread
                    // from `min` to `max`, which is how `percentile` reads
                    // them back, and what lands a code on its own bin
                    // without a rounding error stealing the boundary.
                    let bin = ((value - min) * scale + 0.5) as usize;
                    histogram[bin.min(BINS - 1)] += 1;
                    counted += 1;
                }
                if let Some(axis_scale) = axis_scale {
                    let bin = |counts: &mut [u32; BINS], stored: f32| {
                        let index = ((stored - axis_min) * axis_scale + 0.5) as usize;
                        counts[index.min(BINS - 1)] += 1;
                    };
                    if counts {
                        // Luminance is a linear quantity; it goes onto the
                        // axis the same way the samples themselves did.
                        bin(&mut plot.luma, encode(transfer, value));
                    }
                    if let Some(planes) = plot.colour.as_mut() {
                        for (plane, (stored, decoded)) in
                            encoded[..COLOUR].iter().zip(linear).enumerate()
                        {
                            if is_data(*decoded) {
                                bin(&mut planes[plane], *stored);
                            }
                        }
                    }
                }
            });
        }

        Self {
            min,
            max,
            histogram,
            counted,
            plot,
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

/// Grey uses its one channel; colour collapses to relative luminance,
/// weighted for the BT.709 primaries the working space uses.
fn luminance(pixel: &[f32], channels: Channels) -> f32 {
    if channels.is_gray() {
        pixel[0]
    } else {
        0.2126 * pixel[0] + 0.7152 * pixel[1] + 0.0722 * pixel[2]
    }
}

/// Puts a linear value back on the storage curve, for plotting alongside
/// samples that never left it.
fn encode(transfer: Transfer, value: f32) -> f32 {
    if transfer.is_linear() {
        value
    } else {
        transfer.to_encoded(value)
    }
}

/// Iterator over each sampled pixel, as stored and as decoded.
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

    /// Calls `visit` with one pixel's components twice over: first as the
    /// file holds them, normalised to 0..1 for integer samples, then decoded
    /// to the linear working space. Alpha is included where the image has
    /// one, since callers slice down to what they want.
    fn for_each(self, mut visit: impl FnMut(&[f32], &[f32])) {
        let count = self.channels.count();
        let transfer = self.color.transfer;
        let mut stored = [0.0f32; 4];
        let mut linear = [0.0f32; 4];

        match self.samples {
            Samples::U8 { data, .. } => {
                let scale = 1.0 / u8::MAX as f32;
                // Only 256 codes, and every one of them needs the curve
                // applied; a table beats calling it per component.
                let lut: Vec<f32> = (0..=u8::MAX)
                    .map(|v| transfer.to_linear(v as f32 * scale))
                    .collect();
                for chunk in data.chunks_exact(count).step_by(self.stride) {
                    for (index, raw) in chunk.iter().enumerate() {
                        stored[index] = *raw as f32 * scale;
                        linear[index] = lut[*raw as usize];
                    }
                    visit(&stored[..count], &linear[..count]);
                }
            }
            Samples::U16 { data, .. } => {
                let scale = 1.0 / u16::MAX as f32;
                for chunk in data.chunks_exact(count).step_by(self.stride) {
                    for (index, raw) in chunk.iter().enumerate() {
                        stored[index] = *raw as f32 * scale;
                        linear[index] = transfer.to_linear(stored[index]);
                    }
                    visit(&stored[..count], &linear[..count]);
                }
            }
            Samples::F32 { data, .. } => {
                for chunk in data.chunks_exact(count).step_by(self.stride) {
                    for (index, raw) in chunk.iter().enumerate() {
                        stored[index] = *raw;
                        linear[index] = if transfer.is_linear() {
                            *raw
                        } else {
                            transfer.to_linear(*raw)
                        };
                    }
                    visit(&stored[..count], &linear[..count]);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::{AlphaMode, ColorSpace, Primaries, Referred, Transfer};

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
            referred: Referred::Scene,
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

    /// Every pixel's marker lands on a bar that actually has the pixel in it.
    ///
    /// The one that would go wrong quietly: this image is P3 with
    /// premultiplied alpha, so the colour a readout gets back from
    /// [`DecodedImage::sample`] has been through a primaries matrix and a
    /// divide by alpha that the scan never applied. A bin taken from that
    /// would point at an empty bar beside the pixel's own.
    #[test]
    fn a_pixels_bin_is_the_bar_that_counted_it() {
        let image = DecodedImage {
            width: 4,
            height: 1,
            samples: Samples::U8 {
                channels: Channels::Rgba,
                data: vec![
                    10, 20, 30, 255, // opaque, so the divide is a no-op
                    9, 40, 60, 128, // and these three are not
                    60, 30, 15, 128, //
                    128, 128, 128, 128,
                ],
            },
            color: ColorSpace {
                transfer: Transfer::Srgb,
                primaries: Primaries::DisplayP3,
            },
            alpha: AlphaMode::Premultiplied,
            referred: Referred::Display,
            nodata: None,
        };
        let plot = Stats::scan(&image).plot;

        for x in 0..image.width {
            let sample = image.sample(x, 0).expect("inside the image");
            let bin = plot.bin_of(&image, &sample).expect("on the axis");
            assert!(
                plot.luma[bin] > 0,
                "pixel {x} marked at bin {bin}, which counted nothing"
            );
        }
    }

    /// A value the plot does not cover has no bar to stand on, and is told so
    /// rather than being pushed onto the bar at the end.
    #[test]
    fn a_value_off_the_axis_gets_no_bin() {
        let plot = Stats::scan(&linear_gray(vec![1000, 2000])).plot;
        assert_eq!(plot.bin(plot.min), Some(0));
        assert_eq!(plot.bin(plot.max), Some(BINS - 1));
        assert_eq!(plot.bin(plot.min - (plot.max - plot.min)), None);
        assert_eq!(plot.bin(plot.max + (plot.max - plot.min)), None);
        assert_eq!(plot.bin(f32::NAN), None);

        let flat = Stats::scan(&linear_gray(vec![500; 4])).plot;
        assert_eq!(flat.bin(500.0 / 65535.0), None, "no span, no bars");
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
            referred: Referred::Scene,
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
            referred: Referred::Scene,
            nodata: None,
        }
    }

    /// The point of the separate axis: saturated colour runs well past the
    /// luminance range, and binning it there would pile it into the last bin.
    #[test]
    fn colour_channels_are_binned_over_their_own_range() {
        let stats = Stats::scan(&rgb_f32(vec![1.0, 0.0, 0.0]));
        let plot = &stats.plot;
        let colour = plot.colour.expect("an rgb image has channel counts");

        assert!((stats.max - 0.2126).abs() < 1e-4, "luminance is unchanged");
        assert!(plot.min.abs() < 1e-6);
        assert!((plot.max - 1.0).abs() < 1e-6);
        // Red at the top of the axis, green and blue at the bottom.
        assert_eq!(colour[0][BINS - 1], 1);
        assert_eq!(colour[1][0], 1);
        assert_eq!(colour[2][0], 1);
        // Luminance sits between them, on the same axis as the rest.
        assert_eq!(plot.luma.iter().sum::<u32>(), 1);
        assert_eq!(plot.luma[(0.2126 * 255.0) as usize], 1);
    }

    #[test]
    fn every_plane_counts_every_pixel() {
        let stats = Stats::scan(&rgb_f32(vec![0.25, 0.5, 0.75, 0.0, 0.5, 1.0]));
        let colour = stats.plot.colour.expect("an rgb image has channel counts");
        for plane in &colour {
            assert_eq!(plane.iter().sum::<u32>(), 2);
        }
        assert_eq!(stats.plot.luma.iter().sum::<u32>(), 2);
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
        let with_alpha = Stats::scan(&image).plot;
        let opaque = opaque.plot;

        assert_eq!(with_alpha.max, opaque.max, "alpha would have widened this");
        assert_eq!(with_alpha.colour, opaque.colour);
    }

    #[test]
    fn grey_images_have_no_channel_histogram() {
        assert!(
            Stats::scan(&linear_gray(vec![0, 1000, 4095]))
                .plot
                .colour
                .is_none()
        );
    }

    /// A flat image spans nothing, and binning it would divide by a
    /// zero-width axis.
    #[test]
    fn a_flat_image_plots_nothing() {
        let plot = Stats::scan(&rgb_f32(vec![0.5, 0.5, 0.5])).plot;
        assert_eq!(plot.luma.iter().sum::<u32>(), 0);
        assert!(
            plot.colour
                .is_some_and(|planes| planes.iter().all(|plane| plane.iter().sum::<u32>() == 0))
        );
    }

    fn srgb_gray_u8(data: Vec<u8>) -> DecodedImage {
        DecodedImage {
            width: data.len() as u32,
            height: 1,
            samples: Samples::U8 {
                channels: Channels::Gray,
                data,
            },
            color: ColorSpace {
                transfer: Transfer::Srgb,
                ..ColorSpace::LINEAR_BT709
            },
            alpha: AlphaMode::Opaque,
            referred: Referred::Display,
            nodata: None,
        }
    }

    /// The reason the plot is binned on the storage curve. Decoding first and
    /// binning the linear values leaves two thirds of the highlight bins
    /// unreachable, which draws as a comb of spikes.
    #[test]
    fn consecutive_eight_bit_codes_land_in_consecutive_bins() {
        let codes: Vec<u8> = (200..=255).collect();
        let expected = codes.len();
        let plot = Stats::scan(&srgb_gray_u8(codes)).plot;

        let occupied = plot.luma.iter().filter(|count| **count > 0).count();
        assert_eq!(occupied, expected, "every code needs a bin of its own");
        // And they are contiguous: no empty bin anywhere between the ends.
        let first = plot.luma.iter().position(|count| *count > 0).unwrap();
        let last = plot.luma.iter().rposition(|count| *count > 0).unwrap();
        assert!(
            plot.luma[first..=last].iter().all(|count| *count > 0),
            "a gap between codes is the comb this binning exists to avoid"
        );
    }

    /// A display-referred file plots against the range it can hold, so that
    /// a low-contrast one reads as low contrast instead of being stretched
    /// across the panel — and so its codes stay one to a bin.
    #[test]
    fn display_referred_images_plot_against_the_nominal_range() {
        let plot = Stats::scan(&srgb_gray_u8(vec![200, 255])).plot;
        assert!(plot.min.abs() < 1e-6);
        assert!((plot.max - 1.0).abs() < 1e-6);
        assert_eq!(plot.luma[200], 1);
        assert_eq!(plot.luma[BINS - 1], 1);
    }

    /// Linear samples have no curve to plot against, so the axis stays in
    /// the units the file measured in.
    #[test]
    fn scene_linear_images_keep_a_linear_axis() {
        let stats = Stats::scan(&linear_gray(vec![0, 16384, 32768, 65535]));
        assert!(stats.plot.min.abs() < 1e-6);
        assert!((stats.plot.max - 1.0).abs() < 1e-6);
        // A quarter of full scale bins a quarter of the way along, which on
        // a curve it would not: sRGB would put it past the half way mark.
        let quarter = (0.25 * (BINS - 1) as f32).round() as usize;
        assert_eq!(stats.plot.luma[quarter], 1);
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
            referred: Referred::Scene,
            nodata: None,
        };
        let stats = Stats::scan(&image);
        assert!((stats.min - 0.25).abs() < 1e-6);
        assert!((stats.max - 0.75).abs() < 1e-6);
    }
}
