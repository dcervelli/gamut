//! A gain map: the second picture a phone's photograph carries, saying how
//! far above SDR white each pixel went, and how far it is applied.
//!
//! The base picture is the graded SDR photograph every viewer has always
//! shown, and the map beside it — usually a quarter of the base's size, one
//! or three 8-bit samples a pixel — is a per-pixel log2 multiplier that,
//! applied in linear light, puts back the highlights grading compressed.
//! Two containers carry one, an Ultra HDR JPEG and a HEIF, and each reads
//! its own metadata into a [`Lift`]: ISO 21496-1's, which Ultra HDR's XMP
//! and a HEIF's `tmap` item both spell, or Apple's headroom for the map in
//! an older iPhone's HEIC.
//!
//! The map is not applied when the file is decoded. What it is applied
//! *by* is the display: the standard's weight is where the display's own
//! headroom sits between the base's and the alternate's, so a monitor in
//! SDR mode, with no room above white, gets the base exactly as the phone
//! graded it, and a monitor with room gets as much of the lift as it has
//! room for. The base and the map go to the GPU as they are, and the lift
//! happens in the shader at the weight the surface asks for, which is what
//! lets the room switch without the file being read again — and what keeps
//! a 24-megapixel photograph at four bytes a pixel rather than sixteen.
//!
//! A camera raw's profile is a lift too, of another kind: the exposure, the
//! local tone map and the tone curve a DNG says the camera rendered its
//! picture with ([`Lift::Profile`]). It is worked out when the raw is
//! developed, a ratio a channel for every pixel of the picture
//! ([`Data::Ratios`]), and read through the same machinery — the shader,
//! the readout, the statistics and the copy — at a weight that is the
//! switch's, on or off, rather than the display's room.
//!
//! [`Table`] is what a weight makes of the map: the linear gain each of its
//! 256 values stands for. It is worked out once on the CPU, in
//! [`GainMap::table`], and goes to the shader as a lookup texture, so the
//! two sides cannot disagree about what a value means; and
//! [`GainMap::gain_at`] is the CPU twin of the shader's sampling of the
//! map, for the readout, the statistics and the copy, which have to say
//! what the screen shows.

use std::sync::Arc;

use half::f16;

use crate::image::color::{ColorSpace, Transfer};
use crate::image::display::AutoWindow;
use crate::image::isobmff::Fields;
use crate::image::stats::{BINS, COLOR, Plot};
use crate::image::{AlphaMode, Channels, DecodedImage, Referred, Samples};
use ultrahdr_rs::GainMapMetadata;
use ultrahdr_rs::gainmap::apply::GainMapLut;

/// The map, and what it means.
#[derive(Clone, Debug)]
pub struct GainMap {
    pub width: u32,
    pub height: u32,
    /// One for a luminance map, three for a map with a channel each.
    pub channels: u8,
    pub data: Data,
    pub lift: Lift,
}

/// The map's values: codes read through a [`Table`], as a file stores its
/// gain map, or the gains themselves, as a profile's lift is worked out.
#[derive(Clone, Debug)]
pub enum Data {
    /// Eight-bit codes, `channels` to a pixel, row by row.
    Codes(Vec<u8>),
    /// The linear gain itself, `channels` to a pixel, row by row: what the
    /// whole of the lift multiplies each channel by.
    Ratios(Vec<f16>),
}

impl Data {
    /// How many values the map holds, `channels` to a pixel.
    pub fn len(&self) -> usize {
        match self {
            Data::Codes(codes) => codes.len(),
            Data::Ratios(ratios) => ratios.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// How the map's values are to be read.
#[derive(Clone, Debug)]
pub enum Lift {
    /// ISO 21496-1's description, which Ultra HDR's XMP and a HEIF's
    /// `tmap` item both spell: the headroom of each rendition, and per
    /// channel the range of the map's log2 gains, its gamma and two
    /// offsets.
    Iso(GainMapMetadata),
    /// Apple's description of the map in its own auxiliary image: the map
    /// encoded with the Rec. 709 transfer function, and once linearized the
    /// share of the headroom's boost each pixel gets. `headroom` is the ratio
    /// of the picture's brightest white to SDR white, as the maker note
    /// works it out.
    Apple { headroom: f32 },
    /// A camera raw's profile: the rendering its DNG says the camera meant
    /// — a baseline exposure, a gain table that is a local tone map, and a
    /// tone curve — worked out per pixel as [`Data::Ratios`] when the raw
    /// was developed. Applied whole or not at all.
    Profile(Profile),
}

/// ISO 21496-1's gain map metadata in its binary form, from the minimum
/// version on: what a JPEG's APP2 carries after the standard's name, and a
/// HEIF's `tmap` item after its version byte. `None` for a payload cut
/// short.
pub fn iso_metadata(body: &[u8]) -> Option<GainMapMetadata> {
    let mut fields = Fields::new(body);
    let _minimum_version = fields.u16()?;
    let _writer_version = fields.u16()?;
    let flags = fields.u8()?;
    let multichannel = flags & 0x80 != 0;
    let use_base_color_space = flags & 0x40 != 0;
    let mut ratio = |signed: bool| -> Option<f64> {
        let numerator = if signed {
            f64::from(fields.i32()?)
        } else {
            f64::from(fields.u32()?)
        };
        let denominator = f64::from(fields.u32()?);
        (denominator != 0.0).then(|| numerator / denominator)
    };
    let base_hdr_headroom = ratio(false)?;
    let alternate_hdr_headroom = ratio(false)?;
    let channels = if multichannel { 3 } else { 1 };
    let mut gain_map_min = [0.0; 3];
    let mut gain_map_max = [0.0; 3];
    let mut gamma = [1.0; 3];
    let mut base_offset = [0.0; 3];
    let mut alternate_offset = [0.0; 3];
    for channel in 0..channels {
        gain_map_min[channel] = ratio(true)?;
        gain_map_max[channel] = ratio(true)?;
        gamma[channel] = ratio(false)?;
        base_offset[channel] = ratio(true)?;
        alternate_offset[channel] = ratio(true)?;
    }
    if !multichannel {
        for channel in 1..3 {
            gain_map_min[channel] = gain_map_min[0];
            gain_map_max[channel] = gain_map_max[0];
            gamma[channel] = gamma[0];
            base_offset[channel] = base_offset[0];
            alternate_offset[channel] = alternate_offset[0];
        }
    }
    // `GainMapMetadata` is non-exhaustive, so it is built by amending the
    // defaults rather than by naming every field.
    let mut metadata = GainMapMetadata::default();
    metadata.gain_map_min = gain_map_min;
    metadata.gain_map_max = gain_map_max;
    metadata.gamma = gamma;
    metadata.base_offset = base_offset;
    metadata.alternate_offset = alternate_offset;
    metadata.base_hdr_headroom = base_hdr_headroom;
    metadata.alternate_hdr_headroom = alternate_hdr_headroom;
    metadata.use_base_color_space = use_base_color_space;
    // The standard has no flag for the direction: the base is the SDR
    // rendition when its headroom is the smaller.
    metadata.backward_direction = base_hdr_headroom > alternate_hdr_headroom;
    Some(metadata)
}

/// The auxiliary type Apple gives its gain map: a HEIC's auxiliary image is
/// found by it, and a JPEG's second picture says it in its XMP.
pub const APPLE_AUXILIARY: &str = "urn:com:apple:photo:2020:aux:hdrgainmap";

/// What a raw's profile said, kept for the panels that say it: the map
/// itself is the ratios it made.
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    /// `BaselineExposure`, in stops.
    pub baseline: f32,
    /// The gain table, where the file has one: kept for the histogram,
    /// which draws it, and the information panel, which describes it.
    pub table: Option<Arc<GainTable>>,
    /// `ProfileToneCurve`, its points from black to white, where the file
    /// has one.
    pub curve: Option<Arc<Vec<[f32; 2]>>>,
    /// The least and the most any channel of any pixel is lifted by, in
    /// stops: found as the ratios were made, since a walk over a map the
    /// picture's size is too much for every frame that asks.
    pub range: [f32; 2],
    /// From the picture's own linear primaries into the space the curve is
    /// applied in, linear ProPhoto: where a pixel stands on the curve is
    /// its largest channel there.
    pub to_curve: [[f32; 3]; 3],
}

/// A raw profile's `ProfileGainTableMap`: a grid over the picture, and at
/// each of its points a table of gains by how bright a pixel is — Apple's
/// local tone mapping. Read from the file by the raw developer.
#[derive(Debug, PartialEq)]
pub struct GainTable {
    /// Points down and across.
    pub(crate) points: [usize; 2],
    /// The distance between points, down and across, as a share of the
    /// picture's height and width.
    pub(crate) spacing: [f64; 2],
    /// Where the first point is, down and across, in the same shares.
    pub(crate) origin: [f64; 2],
    /// Each point's table's length.
    pub(crate) steps: usize,
    /// What red, green, blue, the least of the three and the most of them
    /// are each weighted by in the brightness the table is indexed by.
    pub(crate) weights: [f32; 5],
    /// The tables, point by point across each row of the grid in turn.
    pub(crate) gains: Vec<f32>,
}

impl GainTable {
    /// The grid's points, across and down.
    pub fn points(&self) -> [usize; 2] {
        [self.points[1], self.points[0]]
    }

    /// The steps each point's table has.
    pub fn steps(&self) -> usize {
        self.steps
    }

    /// What the weights sum to: what the table's input is of the pixel's
    /// channels, taken as a mean of them. On an iPhone it is 2 to the
    /// minus baseline, which carries the exposure back out, so the table
    /// is in effect indexed by the developed pixel. 1 for a table whose
    /// weights say nothing.
    pub fn scale(&self) -> f32 {
        let sum: f32 = self.weights.iter().sum();
        if sum > 0.0 && sum.is_finite() {
            sum
        } else {
            1.0
        }
    }

    /// The brightness `color` indexes the table by, 0 to 1: the weighted
    /// sum of its channels, the least of them and the most. The weights'
    /// order — red, green, blue, the least, the most — is as the one open
    /// reading of the tag found has it; the specification was not to hand.
    pub fn input(&self, color: [f32; 3]) -> f32 {
        let [r, g, b] = color;
        let w = &self.weights;
        (w[0] * r + w[1] * g + w[2] * b + w[3] * r.min(g).min(b) + w[4] * r.max(g).max(b))
            .clamp(0.0, 1.0)
    }

    /// Where `input` falls along a point's table: the step before it, and
    /// how far toward the next. The index runs to `steps` rather than one
    /// short of it, clamped to the last step, as the same reading has it.
    fn along(&self, input: f32) -> (usize, f32) {
        let position = (input.clamp(0.0, 1.0) * self.steps as f32).min((self.steps - 1) as f32);
        let step = (position as usize).min(self.steps - 2);
        (step, position - step as f32)
    }

    /// Where `share` of the picture's height (`axis` 0) or width (1) falls
    /// on the grid: the points either side of it, the edge's held past it.
    /// Worked out once a row or a column by the developer, which reads the
    /// table at every pixel.
    pub fn tap(&self, axis: usize, share: f32) -> Tap {
        let at = ((f64::from(share) - self.origin[axis]) / self.spacing[axis])
            .clamp(0.0, (self.points[axis] - 1) as f64);
        let near = (at as usize).min(self.points[axis] - 1);
        Tap {
            near,
            far: (near + 1).min(self.points[axis] - 1),
            fraction: (at - near as f64) as f32,
        }
    }

    /// The gain at brightness `input` at `[down, across]`, each a share of
    /// the picture's height and width: [`GainTable::gain_between`] the
    /// taps of the place.
    pub fn gain_for(&self, input: f32, place: [f32; 2]) -> f32 {
        self.gain_between(input, self.tap(0, place[0]), self.tap(1, place[1]))
    }

    /// The gain at brightness `input` between the grid's points `down` and
    /// `across` tap: linear between the table's steps, and blended between
    /// the four points of the grid around the place.
    pub fn gain_between(&self, input: f32, down: Tap, across: Tap) -> f32 {
        let (step, along) = self.along(input);
        let point = |row: usize, column: usize| {
            let start = (row * self.points[1] + column) * self.steps + step;
            let pair = &self.gains[start..start + 2];
            pair[0] + (pair[1] - pair[0]) * along
        };
        let upper = point(down.near, across.near)
            + (point(down.near, across.far) - point(down.near, across.near)) * across.fraction;
        let lower = point(down.far, across.near)
            + (point(down.far, across.far) - point(down.far, across.near)) * across.fraction;
        upper + (lower - upper) * down.fraction
    }

    /// The least and the most gain at `input` over every point of the grid:
    /// the spread of the picture's tables at that brightness, read along
    /// the steps as [`GainTable::gain_for`] reads them.
    pub fn spread_at(&self, input: f32) -> [f32; 2] {
        let (step, along) = self.along(input);
        self.gains
            .chunks_exact(self.steps)
            .map(|table| table[step] + (table[step + 1] - table[step]) * along)
            .fold([f32::INFINITY, f32::NEG_INFINITY], |[low, high], gain| {
                [low.min(gain), high.max(gain)]
            })
    }
}

impl Profile {
    /// The tone curve at `input`, through its points, linear between them:
    /// what the histogram draws. The identity where there is no curve.
    /// The curve on a color, as the DNG SDK's `RGBTone` applies it: see
    /// [`rgb_tone`].
    pub fn tone(&self, color: [f32; 3]) -> [f32; 3] {
        rgb_tone(|value| self.curve_at(value), color)
    }

    pub fn curve_at(&self, input: f32) -> f32 {
        let Some(points) = self.curve.as_deref() else {
            return input;
        };
        curve_through(points, input)
    }
}

/// `points`, from black to white, read at `input` linearly between them,
/// held at the ends.
pub fn curve_through(points: &[[f32; 2]], input: f32) -> f32 {
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        return input;
    };
    if input <= first[0] {
        return first[1];
    }
    if input >= last[0] {
        return last[1];
    }
    let after = points.partition_point(|point| point[0] <= input);
    let ([x0, y0], [x1, y1]) = (points[after - 1], points[after]);
    if x1 <= x0 {
        return y1;
    }
    y0 + (y1 - y0) * (input - x0) / (x1 - x0)
}

impl GainMap {
    /// Log2 of the whole lift: how far above the base the alternate goes.
    pub fn stops(&self) -> f32 {
        match &self.lift {
            Lift::Iso(metadata) => {
                (metadata.alternate_hdr_headroom - metadata.base_hdr_headroom) as f32
            }
            Lift::Apple { headroom } => headroom.log2(),
            Lift::Profile(profile) => profile.baseline,
        }
    }

    /// Whether the map is a raw's profile rather than a picture's lift
    /// above white.
    pub fn is_profile(&self) -> bool {
        matches!(self.lift, Lift::Profile(_))
    }

    /// The profile the map was made from, where it was made from one.
    pub fn profile(&self) -> Option<&Profile> {
        match &self.lift {
            Lift::Profile(profile) => Some(profile),
            _ => None,
        }
    }

    /// How much of the lift a display with `headroom` — the ratio of its
    /// peak to its white, 1 for a monitor in SDR mode — gets: the standard's
    /// weight, where the display's headroom sits between the base's and the
    /// alternate's, from none of it to all of it. An infinite `headroom`
    /// is a display whose room is not known, which gets all of it.
    ///
    /// A profile is the camera's rendering rather than room above white,
    /// and is the same on every display: all of it, whatever the room.
    pub fn weight(&self, headroom: f32) -> f32 {
        if self.is_profile() {
            return 1.0;
        }
        let stops = self.stops();
        if stops.is_nan() || stops <= 0.0 || headroom.is_nan() {
            return 0.0;
        }
        let base = match &self.lift {
            Lift::Iso(metadata) => metadata.base_hdr_headroom as f32,
            Lift::Apple { .. } | Lift::Profile(_) => 0.0,
        };
        ((headroom.max(1.0).log2() - base) / stops).clamp(0.0, 1.0)
    }

    /// What `weight` of the lift makes of each of the map's values.
    pub fn table(&self, weight: f32) -> Table {
        let weight = weight.clamp(0.0, 1.0);
        let mut gains = Box::new([0.0f32; 256 * 3]);
        let (base_offset, alternate_offset) = match &self.lift {
            Lift::Iso(metadata) => {
                // The crate's own table: a value is a place between the
                // map's stated minimum and maximum log2 gains, through the
                // map's gamma, and the weight scales the log2 gain.
                let lut = GainMapLut::new(metadata, weight);
                for channel in 0..3 {
                    for value in 0..256 {
                        gains[channel * 256 + value] = lut.lookup(value as u8, channel);
                    }
                }
                (
                    metadata.base_offset.map(|offset| offset as f32),
                    metadata.alternate_offset.map(|offset| offset as f32),
                )
            }
            Lift::Apple { headroom } => {
                // Apple's formula, `1 + (headroom - 1) * gain`, with the
                // headroom scaled the way the standard scales its log2
                // gains, so that a weight means the same thing either way.
                let headroom = headroom.max(1.0).powf(weight);
                for value in 0..256 {
                    let gain =
                        1.0 + (headroom - 1.0) * Transfer::Bt709.to_linear(value as f32 / 255.0);
                    for channel in 0..3 {
                        gains[channel * 256 + value] = gain;
                    }
                }
                ([0.0; 3], [0.0; 3])
            }
            // The ratios are the gains themselves, read without a table:
            // a gain of one throughout, which is also what the lookup
            // texture is written with, unread.
            Lift::Profile(_) => {
                gains.fill(1.0);
                ([0.0; 3], [0.0; 3])
            }
        };
        Table {
            weight,
            gains,
            base_offset,
            alternate_offset,
        }
    }

    /// The map as a picture of its own, to be drawn in the place of the one
    /// it belongs to: each value as the stops it lifts the base by at the
    /// whole of the lift, gray for a luminance map and a color for one with
    /// a channel each, read as a measurement — windowed to the range it
    /// spans — so that what is lifted most is brightest, and the readout
    /// under the pointer reads stops.
    pub fn image(&self) -> DecodedImage {
        let channels = self.channels.max(1) as usize;
        let data = match &self.data {
            Data::Codes(codes) => {
                let stops = self.stops_table();
                codes
                    .chunks_exact(channels)
                    .flat_map(|values| {
                        values
                            .iter()
                            .enumerate()
                            .map(|(channel, &value)| stops[channel.min(2) * 256 + value as usize])
                    })
                    .collect()
            }
            Data::Ratios(ratios) => ratios.iter().map(|ratio| ratio.to_f32().log2()).collect(),
        };
        let mut image = DecodedImage::new(
            self.width,
            self.height,
            Samples::F32 {
                channels: if channels == 1 {
                    Channels::Gray
                } else {
                    Channels::Rgb
                },
                data,
            },
            ColorSpace::LINEAR_BT709,
            AlphaMode::Opaque,
        );
        image.referred = Referred::Measured;
        image.opens_on = Some(AutoWindow::MinMax);
        image
    }

    /// The least and the most any of the map's pixels lifts the base by, in
    /// stops, at the whole of the lift: what this picture's map holds, which
    /// may be less than the headroom it is described for. `None` for a map
    /// with no pixels.
    pub fn lift_range(&self) -> Option<[f32; 2]> {
        let codes = match (&self.data, &self.lift) {
            (_, Lift::Profile(profile)) => {
                return (!self.data.is_empty()).then_some(profile.range);
            }
            (Data::Ratios(ratios), _) => return ratio_range(ratios),
            (Data::Codes(codes), _) => codes,
        };
        let stops = &self.stops_table();
        let channels = self.channels.max(1) as usize;
        // Which values each channel holds, so that a map of millions of
        // pixels is read through the table 256 times at most a channel.
        let mut held = [[false; 256]; 3];
        for values in codes.chunks_exact(channels) {
            for (channel, &value) in values.iter().enumerate().take(3) {
                held[channel][value as usize] = true;
            }
        }
        held.iter()
            .enumerate()
            .flat_map(|(channel, values)| {
                values
                    .iter()
                    .enumerate()
                    .filter(|(_, held)| **held)
                    .map(move |(value, _)| stops[channel * 256 + value])
            })
            .filter(|stops| stops.is_finite())
            .fold(None, |range: Option<[f32; 2]>, stops| {
                Some(range.map_or([stops, stops], |[low, high]| {
                    [low.min(stops), high.max(stops)]
                }))
            })
    }

    /// How many stops of the whole lift `weight` of it is: what is on
    /// screen of it, which the information panel and the histogram both
    /// say.
    pub fn applied_stops(&self, weight: f32) -> f32 {
        weight.clamp(0.0, 1.0) * self.stops()
    }

    /// How far the map lifts the base pixel `(x, y)` of a `width` by
    /// `height` base at the whole of the lift, in stops, a channel each: the
    /// map read as [`GainMap::gain_at`] reads it for the screen. The same
    /// three for a luminance map.
    pub fn stops_at(&self, x: u32, y: u32, width: u32, height: u32) -> [f32; 3] {
        self.gain_at(&self.table(1.0), x, y, width, height)
            .map(f32::log2)
    }

    /// The map's histogram: how much of the picture is lifted how far, in
    /// stops at the whole of the lift, evenly spaced, over the range the
    /// map is described for — its lowest code to its highest
    /// ([`GainMap::described_range`]) — as a graded file is plotted over the
    /// range its codes can hold rather than stretched to what it uses. Where
    /// the picture does not reach an end of that range, the plot says so by
    /// leaving the stretch empty.
    ///
    /// Each code stands for a stretch of stops, from halfway to the code
    /// below to halfway to the code above, and its count is spread over the
    /// bins that stretch covers, in proportion. A code dropped into the one
    /// bin its middle falls in would comb: the map's 256 codes land unevenly
    /// on bins in stops wherever the map reads its codes through a curve, as
    /// a gamma other than one and Apple's Rec. 709 encoding do, or holds
    /// fewer of them than there are bins. Spread, each bin holds what the
    /// codes say of it, and the plot is the distribution the map describes.
    ///
    /// A map with a channel each has a plane each, and its luminance plane
    /// is the mean of the three, as a pixel's luminance is a mean of its
    /// channels. `None` for a map with no pixels or no lift.
    ///
    /// A walk over the map, on a stride past [`MAX_PLOTTED`] values; the
    /// caller keeps the answer.
    pub fn lift_plot(&self) -> Option<Plot> {
        if let Data::Ratios(ratios) = &self.data {
            return self.ratio_plot(ratios);
        }
        self.lift_range()?;
        let [low, high] = self.described_range()?;
        let span = high - low;
        if span.partial_cmp(&f32::MIN_POSITIVE) != Some(std::cmp::Ordering::Greater) {
            return None;
        }
        let Data::Codes(stored) = &self.data else {
            unreachable!("ratios are plotted above");
        };
        let stops = self.stops_table();
        let channels = self.channels.max(1) as usize;
        let color = channels >= COLOR;
        // The stretch of stops each code of each channel stands for.
        let stretch = |channel: usize, code: usize| -> (f32, f32) {
            let at = |code: usize| stops[channel * 256 + code];
            let below = if code == 0 {
                at(0)
            } else {
                (at(code - 1) + at(code)) / 2.0
            };
            let above = if code == 255 {
                at(255)
            } else {
                (at(code) + at(code + 1)) / 2.0
            };
            (below.min(above), below.max(above))
        };
        // Bins as the positions `Plot::bin_at` rounds to: bin `k` is the
        // stretch of the axis within half a bin of its own value.
        let scale = (BINS - 1) as f32 / span;
        let spread = |into: &mut [f64; BINS], (from, to): (f32, f32), count: f64| {
            let (from, to) = ((from - low) * scale + 0.5, (to - low) * scale + 0.5);
            let (from, to) = (from.clamp(0.0, BINS as f32), to.clamp(0.0, BINS as f32));
            let width = to - from;
            if width <= 1e-6 {
                into[(from as usize).min(BINS - 1)] += count;
                return;
            }
            let mut bin = from.floor() as usize;
            while bin < BINS && (bin as f32) < to {
                let overlap = to.min(bin as f32 + 1.0) - from.max(bin as f32);
                if overlap > 0.0 {
                    into[bin] += count * f64::from(overlap / width);
                }
                bin += 1;
            }
        };

        let pixels = stored.len() / channels;
        let stride = pixels.div_ceil(MAX_PLOTTED).max(1);
        // Counted by code first, so that a channel's spreading is done once
        // a code rather than once a pixel; the luminance of a map with three
        // is the one thing that has to be spread pixel by pixel.
        let mut codes = [[0u64; 256]; COLOR];
        let mut luma = [0f64; BINS];
        for values in stored.chunks_exact(channels).step_by(stride) {
            for (channel, &value) in values.iter().enumerate().take(COLOR) {
                codes[channel][value as usize] += 1;
            }
            if color {
                let mut mean = (0.0, 0.0);
                for (channel, &value) in values.iter().enumerate().take(COLOR) {
                    let (from, to) = stretch(channel, value as usize);
                    mean = (mean.0 + from / COLOR as f32, mean.1 + to / COLOR as f32);
                }
                spread(&mut luma, mean, 1.0);
            }
        }
        let plane = |channel: usize| {
            let mut bins = [0f64; BINS];
            for (code, &count) in codes[channel].iter().enumerate() {
                if count > 0 {
                    spread(&mut bins, stretch(channel, code), count as f64);
                }
            }
            bins
        };
        if !color {
            luma = plane(0);
        }
        // Back to whole counts, which is what a plot holds, in sixteenths of
        // a pixel, so that a code spread thin over several bins rounds to
        // something in each rather than vanishing. The plot is drawn by its
        // bins' heights against each other, which the unit leaves alone.
        let whole = |bins: [f64; BINS]| bins.map(|count| (count * SHARES).round() as u32);
        Some(Plot {
            min: low,
            max: high,
            luma: whole(luma),
            color: color.then(|| [whole(plane(0)), whole(plane(1)), whole(plane(2))]),
        })
    }

    /// The least and the most any code of the map can lift the base by, in
    /// stops, at the whole of the lift: the range the file describes its map
    /// over, from its lowest code to its highest, whatever this picture's
    /// map holds of it. `None` where no code says anything finite.
    ///
    /// A map of ratios describes nothing beyond what it holds: its range is
    /// [`GainMap::lift_range`]'s.
    pub fn described_range(&self) -> Option<[f32; 2]> {
        if matches!(self.data, Data::Ratios(_)) {
            return self.lift_range();
        }
        let channels = if self.channels >= 3 { COLOR } else { 1 };
        self.stops_table()[..channels * 256]
            .iter()
            .copied()
            .filter(|stops| stops.is_finite())
            .fold(None, |range: Option<[f32; 2]>, stops| {
                Some(range.map_or([stops, stops], |[low, high]| {
                    [low.min(stops), high.max(stops)]
                }))
            })
    }

    /// The histogram of a map of ratios: each value is a gain of its own
    /// rather than a code standing for a stretch, so each pixel's stops
    /// fall in the one bin they round to, a channel each and their mean for
    /// the luminance.
    ///
    /// Over the stops all but a thousandth of the pixels hold at either
    /// end, the rest counted in the end bins: a profile's tone curve takes
    /// its deepest shadows to black, and the handful of pixels it does that
    /// to are lifted by nothing a ratio can say, which would otherwise
    /// stretch the axis a dozen stops past where the picture is. `None`
    /// for a map that lifts everything alike.
    fn ratio_plot(&self, ratios: &[f16]) -> Option<Plot> {
        let channels = self.channels.max(1) as usize;
        let color = channels >= COLOR;
        let pixels = ratios.len() / channels;
        let stride = pixels.div_ceil(MAX_PLOTTED).max(1);
        let stops: Vec<[f32; COLOR]> = ratios
            .chunks_exact(channels)
            .step_by(stride)
            .map(|values| {
                std::array::from_fn(|channel| values[channel.min(values.len() - 1)].to_f32().log2())
            })
            .filter(|stops: &[f32; COLOR]| stops.iter().all(|stops| stops.is_finite()))
            .collect();
        let mut sorted: Vec<f32> = stops.iter().flatten().copied().collect();
        if sorted.is_empty() {
            return None;
        }
        sorted.sort_unstable_by(f32::total_cmp);
        let at = |share: f64| sorted[((sorted.len() - 1) as f64 * share).round() as usize];
        let (low, high) = (at(TRIMMED), at(1.0 - TRIMMED));
        if high - low <= f32::MIN_POSITIVE {
            return None;
        }
        let scale = (BINS - 1) as f32 / (high - low);
        let bin = |stops: f32| {
            let at = ((stops - low) * scale + 0.5).clamp(0.0, (BINS - 1) as f32);
            at as usize
        };
        let share = SHARES as u32;
        let mut luma = [0u32; BINS];
        let mut planes = [[0u32; BINS]; COLOR];
        for pixel in &stops {
            if color {
                for (plane, stops) in planes.iter_mut().zip(pixel) {
                    plane[bin(*stops)] += share;
                }
                luma[bin(pixel.iter().sum::<f32>() / COLOR as f32)] += share;
            } else {
                luma[bin(pixel[0])] += share;
            }
        }
        Some(Plot {
            min: low,
            max: high,
            luma,
            color: color.then_some(planes),
        })
    }

    /// Each value of each channel as the stops it lifts by at the whole of
    /// the lift, laid out as [`Table`]'s gains are.
    fn stops_table(&self) -> Vec<f32> {
        let table = self.table(1.0);
        (0..3)
            .flat_map(|channel| (0..=255u8).map(move |value| (channel, value)))
            .map(|(channel, value)| table.gain(value, channel).log2())
            .collect()
    }

    /// The gain at base pixel `(x, y)` of a `width` by `height` base,
    /// through `table`: the map's four surrounding values, each through the
    /// table, blended by the pixel's distance from them, as the standard's
    /// reference samples it. The twin of `gain` in `shaders/image.wgsl`
    /// and `shaders/reduce.wgsl`; keep the three in step.
    ///
    /// A map of ratios holds the gains themselves, which the table's
    /// weight raises to its power: none of the lift at no weight, all of it
    /// at one. A map the base's own size is read at the pixel itself, which
    /// is where the blend lands anyway.
    pub fn gain_at(&self, table: &Table, x: u32, y: u32, width: u32, height: u32) -> [f32; 3] {
        let codes = match &self.data {
            Data::Codes(codes) => codes,
            Data::Ratios(ratios) => {
                return self.ratio_at(ratios, table.weight, x, y, width, height);
            }
        };
        let column = Tap::at(x, width, self.width);
        let row = Tap::at(y, height, self.height);
        let stride = self.width as usize;
        let corner = |x: usize, y: usize| (y * stride + x) * self.channels as usize;
        let (c00, c10, c01, c11) = (
            corner(column.near, row.near),
            corner(column.far, row.near),
            corner(column.near, row.far),
            corner(column.far, row.far),
        );
        let blend = |channel: usize| {
            let gain = |corner: usize| table.gain(codes[corner + channel], channel);
            let top = gain(c00) * (1.0 - column.fraction) + gain(c10) * column.fraction;
            let bottom = gain(c01) * (1.0 - column.fraction) + gain(c11) * column.fraction;
            top * (1.0 - row.fraction) + bottom * row.fraction
        };
        if self.channels == 1 {
            let gain = blend(0);
            [gain, gain, gain]
        } else {
            [blend(0), blend(1), blend(2)]
        }
    }

    /// [`GainMap::gain_at`] for a map of ratios, at `weight` of the lift.
    fn ratio_at(
        &self,
        ratios: &[f16],
        weight: f32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> [f32; 3] {
        if weight <= 0.0 {
            return [1.0; 3];
        }
        let channels = self.channels.max(1) as usize;
        let ratio = |channel: usize| -> f32 {
            let read = |x: usize, y: usize| {
                ratios
                    .get((y * self.width as usize + x) * channels + channel.min(channels - 1))
                    .map_or(1.0, |ratio| ratio.to_f32())
            };
            if (width, height) == (self.width, self.height) {
                return read(x as usize, y as usize);
            }
            let column = Tap::at(x, width, self.width);
            let row = Tap::at(y, height, self.height);
            let top = read(column.near, row.near) * (1.0 - column.fraction)
                + read(column.far, row.near) * column.fraction;
            let bottom = read(column.near, row.far) * (1.0 - column.fraction)
                + read(column.far, row.far) * column.fraction;
            top * (1.0 - row.fraction) + bottom * row.fraction
        };
        let gains = [ratio(0), ratio(1), ratio(2)];
        if weight >= 1.0 {
            gains
        } else {
            gains.map(|gain| gain.powf(weight))
        }
    }
}

/// The least and the most of `ratios` in stops, where any is finite.
fn ratio_range(ratios: &[f16]) -> Option<[f32; 2]> {
    ratios
        .iter()
        .map(|ratio| ratio.to_f32().log2())
        .filter(|stops| stops.is_finite())
        .fold(None, |range: Option<[f32; 2]>, stops| {
            Some(range.map_or([stops, stops], |[low, high]| {
                [low.min(stops), high.max(stops)]
            }))
        })
}

/// What each of the map's 256 values means at one weight: the linear gain
/// it stands for, per channel, and the offsets the formula adds and takes
/// away around the multiplication.
#[derive(Clone, Debug)]
pub struct Table {
    weight: f32,
    /// `[R0..R255, G0..G255, B0..B255]`.
    gains: Box<[f32; 256 * 3]>,
    base_offset: [f32; 3],
    alternate_offset: [f32; 3],
}

impl Table {
    /// The weight this table was made at.
    pub fn weight(&self) -> f32 {
        self.weight
    }

    pub fn gain(&self, value: u8, channel: usize) -> f32 {
        self.gains[channel * 256 + value as usize]
    }

    /// The offset added to the base before the gain, and the one taken
    /// from the product after.
    pub fn offsets(&self) -> ([f32; 3], [f32; 3]) {
        (self.base_offset, self.alternate_offset)
    }

    /// One linear color of the base, lifted by `gain`: the standard's
    /// `(base + offset) * gain - offset`.
    pub fn apply(&self, color: &mut [f32], gain: [f32; 3]) {
        for (channel, value) in color.iter_mut().enumerate().take(3) {
            *value = (*value + self.base_offset[channel]) * gain[channel]
                - self.alternate_offset[channel];
        }
    }

    /// The table as the shader's lookup texture holds it: one texel a
    /// value, the three channels' gains in its color.
    pub fn texels(&self) -> Vec<[f32; 4]> {
        (0..256)
            .map(|value| {
                [
                    self.gain(value as u8, 0),
                    self.gain(value as u8, 1),
                    self.gain(value as u8, 2),
                    0.0,
                ]
            })
            .collect()
    }
}

/// The most of a map's pixels [`GainMap::lift_plot`] counts before it walks
/// the map on a stride, as the picture's own scan does.
const MAX_PLOTTED: usize = 1 << 21;

/// The share of a map of ratios' values left off either end of its plot's
/// axis, and counted in the end bins.
const TRIMMED: f64 = 0.001;

/// What [`GainMap::lift_plot`] counts a pixel as: sixteen shares, so that
/// a code spread over several bins keeps a share in each.
const SHARES: f64 = 16.0;

/// A gain map shared between the picture, the GPU's copy of it and the
/// threads that read it.
pub type Shared = Arc<GainMap>;

/// Where a place falls along one axis of a grid — a gain map's pixels over
/// the base's, or a [`GainTable`]'s points over the picture: the two rows
/// (or columns) either side of it and how far it is from the first toward
/// the other.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tap {
    near: usize,
    far: usize,
    fraction: f32,
}

impl Tap {
    /// Where the base's row or column `at` of `base` falls on `map` rows
    /// or columns.
    fn at(at: u32, base: u32, map: u32) -> Self {
        let position = (at as f32 / base as f32) * map as f32;
        let last = map.saturating_sub(1) as usize;
        let near = (position.floor() as usize).min(last);
        Self {
            near,
            far: (near + 1).min(last),
            fraction: position - position.floor(),
        }
    }
}

/// A tone curve applied to a color as the DNG SDK's `RGBTone` applies a
/// profile's: the curve on the largest channel and the smallest, and the
/// middle one put where it stood between them, so that the hue holds
/// rather than sliding toward a primary as a curve on each channel would.
/// The channels are taken to 0..1 first, the curve's own domain. Which
/// channel is which is [`tone_order`]'s.
pub fn rgb_tone(curve: impl Fn(f32) -> f32, color: [f32; 3]) -> [f32; 3] {
    let color = color.map(|value| value.clamp(0.0, 1.0));
    let [high, middle, low] = tone_order(color);
    let (top, bottom) = (curve(color[high]), curve(color[low]));
    let mut out = [0.0; 3];
    out[high] = top;
    out[low] = bottom;
    let span = color[high] - color[low];
    out[middle] = if span > 0.0 {
        bottom + (top - bottom) * (color[middle] - color[low]) / span
    } else {
        top
    };
    out
}

/// The channels of `color` from the largest to the smallest, by index: the
/// two the curve is applied to at either end, and the one placed between
/// them in the middle.
/// Run at every pixel of a raw as it develops, so the three comparisons
/// are written out rather than handed to a sort; a tie keeps the channels'
/// own order, as a stable sort would.
pub fn tone_order(color: [f32; 3]) -> [usize; 3] {
    let mut order = [0, 1, 2];
    if color[order[1]] > color[order[0]] {
        order.swap(0, 1);
    }
    if color[order[2]] > color[order[1]] {
        order.swap(1, 2);
    }
    if color[order[1]] > color[order[0]] {
        order.swap(0, 1);
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iso(stops: f64) -> GainMapMetadata {
        // `GainMapMetadata` is non-exhaustive, so it is built by amending the
        // defaults rather than by naming every field.
        let mut metadata = GainMapMetadata::default();
        metadata.gain_map_max = [stops; 3];
        metadata.gain_map_min = [0.0; 3];
        metadata.base_offset = [0.0; 3];
        metadata.alternate_offset = [0.0; 3];
        metadata.alternate_hdr_headroom = stops;
        metadata
    }

    fn map(lift: Lift) -> GainMap {
        GainMap {
            width: 2,
            height: 1,
            channels: 1,
            data: Data::Codes(vec![0, 255]),
            lift,
        }
    }

    /// The standard's weight: nothing for a monitor with no room above
    /// white, all of it for one with as much room as the file asks for, and
    /// a proportion between — in stops, not in ratios.
    #[test]
    fn the_weight_is_where_the_display_sits_between_the_renditions() {
        let two_stops = map(Lift::Iso(iso(2.0)));
        assert_eq!(two_stops.weight(1.0), 0.0);
        assert_eq!(two_stops.weight(0.5), 0.0);
        assert!((two_stops.weight(2.0) - 0.5).abs() < 1e-6);
        assert_eq!(two_stops.weight(4.0), 1.0);
        assert_eq!(two_stops.weight(16.0), 1.0);
        assert_eq!(two_stops.weight(f32::NAN), 0.0);
        assert_eq!(two_stops.weight(f32::INFINITY), 1.0, "room not known");

        let apple = map(Lift::Apple { headroom: 4.0 });
        assert_eq!(apple.weight(1.0), 0.0);
        assert!((apple.weight(2.0) - 0.5).abs() < 1e-6);
        assert_eq!(apple.weight(4.0), 1.0);

        // A map that lifts nothing weighs nothing, rather than dividing by
        // zero.
        assert_eq!(map(Lift::Iso(iso(0.0))).weight(4.0), 0.0);
    }

    /// At no weight every value is a gain of one, and at full weight the
    /// top value is the whole lift, whichever description the map came
    /// with. Half the weight is half the stops.
    #[test]
    fn the_table_runs_from_no_lift_to_the_whole_of_it() {
        for lift in [Lift::Iso(iso(2.0)), Lift::Apple { headroom: 4.0 }] {
            let map = map(lift);
            let none = map.table(0.0);
            let all = map.table(1.0);
            let half = map.table(0.5);
            for channel in 0..3 {
                for value in 0..=255u8 {
                    assert!((none.gain(value, channel) - 1.0).abs() < 1e-6);
                }
                assert!((all.gain(0, channel) - 1.0).abs() < 1e-6);
                assert!((all.gain(255, channel) - 4.0).abs() < 1e-4);
                assert!((half.gain(255, channel) - 2.0).abs() < 1e-4);
            }
        }
    }

    /// The ISO table is the crate's own, read out once rather than looked
    /// up per pixel, and the offsets ride with it.
    #[test]
    fn the_iso_table_is_the_crates() {
        let mut metadata = iso(2.0);
        metadata.gain_map_min = [-1.0, 0.0, 0.5];
        metadata.gamma = [1.0, 1.5, 2.0];
        metadata.base_offset = [0.015625; 3];
        metadata.alternate_offset = [0.01, 0.02, 0.03];
        let table = map(Lift::Iso(metadata.clone())).table(0.7);
        let lut = GainMapLut::new(&metadata, 0.7);
        for channel in 0..3 {
            for value in 0..=255u8 {
                assert_eq!(table.gain(value, channel), lut.lookup(value, channel));
            }
        }
        assert_eq!(table.offsets(), ([0.015625; 3], [0.01, 0.02, 0.03]));
        let mut color = [0.5, 0.5, 0.5];
        table.apply(&mut color, [2.0, 2.0, 2.0]);
        assert!((color[0] - (0.515625 * 2.0 - 0.01)).abs() < 1e-6);
    }

    /// Apple's map is encoded with Rec. 709's curve, and the curve is
    /// undone on the way: the middle code is well under half the boost, as
    /// a gamma-encoded value is.
    #[test]
    fn apples_map_is_linearized_through_rec_709() {
        let table = map(Lift::Apple { headroom: 4.0 }).table(1.0);
        let middle = table.gain(128, 1);
        assert!(middle > 1.0 && middle < 2.5, "{middle}");
        assert!((Transfer::Bt709.to_linear(0.5) - 0.2596).abs() < 0.001);
        assert!((Transfer::Bt709.to_linear(1.0) - 1.0).abs() < 1e-5);
        assert_eq!(Transfer::Bt709.to_linear(0.0), 0.0);
    }

    /// Shown in the picture's place, the map reads in stops, one channel
    /// gray and three in color, and its range is what its pixels hold.
    #[test]
    fn the_map_shown_reads_in_stops() {
        let map = map(Lift::Iso(iso(2.0)));
        let image = map.image();
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!(image.referred, Referred::Measured);
        let Samples::F32 { channels, data } = &image.samples else {
            panic!("float samples");
        };
        assert_eq!(*channels, Channels::Gray);
        assert!(
            data[0].abs() < 1e-5 && (data[1] - 2.0).abs() < 1e-3,
            "{data:?}"
        );
        let [low, high] = map.lift_range().expect("a range");
        assert!(low.abs() < 1e-5 && (high - 2.0).abs() < 1e-3);

        let color = GainMap {
            width: 1,
            height: 1,
            channels: 3,
            data: Data::Codes(vec![0, 128, 255]),
            lift: Lift::Apple { headroom: 8.0 },
        };
        let Samples::F32 { channels, data } = color.image().samples else {
            panic!("float samples");
        };
        assert_eq!(channels, Channels::Rgb);
        assert_eq!(data.len(), 3);
        assert!((data[2] - 3.0).abs() < 1e-3);
        let [low, high] = color.lift_range().expect("a range");
        assert!(low.abs() < 1e-5 && (high - 3.0).abs() < 1e-3);
    }

    /// Shown, the map opens on its full range, and a reset puts it back
    /// there: its strongest lift is a handful of pixels, which a trim would
    /// clip to the top of the window with the lift under them.
    #[test]
    fn the_map_shown_opens_on_its_full_range() {
        use crate::image::display::{Display, Startup};
        use crate::image::stats::Stats;
        let mut data = vec![0; 2000];
        data.push(255);
        let map = GainMap {
            width: 2001,
            height: 1,
            channels: 1,
            data: Data::Codes(data),
            lift: Lift::Iso(iso(2.0)),
        };
        let image = map.image();
        let stats = Stats::scan(&image);
        let mut display = Display::for_image_with(&image, &stats, Startup::default());
        assert_eq!(display.auto(), AutoWindow::MinMax);
        assert!((display.window_high() - 2.0).abs() < 1e-3);
        display.put_white(1.0);
        display.reset(&stats, &image);
        assert_eq!(display.auto(), AutoWindow::MinMax);
        assert!((display.window_high() - 2.0).abs() < 1e-3);
    }

    /// The lift's histogram counts the unlifted pixel at nothing and the
    /// lifted one at the whole of the lift, and a map with a channel each
    /// has a plane each, its luminance their mean. A pixel's own lift reads
    /// the same stops.
    #[test]
    fn the_lift_is_plotted_and_read_in_stops() {
        let gray = map(Lift::Iso(iso(2.0)));
        let plot = gray.lift_plot().expect("a lift to plot");
        assert_eq!(plot.min, 0.0);
        assert!((plot.max - 2.0).abs() < 1e-3, "{}", plot.max);
        // Each end code stands for half a step, spread inside the axis, and
        // is counted in sixteenths.
        assert_eq!(plot.luma.iter().sum::<u32>(), 2 * SHARES as u32);
        assert!(plot.luma[0] > 0);
        assert!(plot.luma[BINS - 1] > 0);
        assert!(plot.color.is_none());

        // The base is twice the map's width: its first two pixels read the
        // map's first value, and its last its second.
        assert!(gray.stops_at(0, 0, 4, 1)[0].abs() < 1e-5);
        assert!((gray.stops_at(3, 0, 4, 1)[0] - 2.0).abs() < 1e-3);
        assert!((gray.applied_stops(0.5) - 1.0).abs() < 1e-6);
        assert_eq!(gray.applied_stops(2.0), gray.stops());

        let color = GainMap {
            width: 1,
            height: 1,
            channels: 3,
            data: Data::Codes(vec![0, 0, 255]),
            lift: Lift::Iso(iso(3.0)),
        };
        let plot = color.lift_plot().expect("a lift to plot");
        let planes = plot.color.expect("a plane a channel");
        assert!(planes[0][0] > 0);
        assert!(planes[1][0] > 0);
        assert!(planes[2][BINS - 1] > 0);
        // The mean of 0, 0 and 3 stops is one, a third of the way along.
        assert!(plot.luma[plot.bin_at(1.0).expect("on the axis")] > 0);

        // The axis runs over the range the map is described for, whatever of
        // it the picture's map holds.
        let mut darkening = iso(2.0);
        darkening.gain_map_min = [-1.0; 3];
        let partway = GainMap {
            width: 3,
            height: 1,
            channels: 1,
            data: Data::Codes(vec![200, 220, 255]),
            lift: Lift::Iso(darkening),
        };
        let plot = partway.lift_plot().expect("a lift to plot");
        assert!(
            (plot.min + 1.0).abs() < 1e-3,
            "the darkening its codes describe, which this map does not use: {}",
            plot.min
        );
        assert!((plot.max - 2.0).abs() < 1e-3);
        let low = plot.bin_at(1.0).expect("on the axis");
        assert!(
            plot.luma[..low].iter().all(|count| *count == 0),
            "nothing below the codes the map holds"
        );

        // Every code of a map read through a curve still fills the stretch
        // it stands for: no empty bin between two the map holds, the comb
        // a bin to a code would leave on an axis in stops.
        let apple = GainMap {
            width: 256,
            height: 1,
            channels: 1,
            data: Data::Codes((0..=255).collect()),
            lift: Lift::Apple { headroom: 4.5 },
        };
        let plot = apple.lift_plot().expect("a lift to plot");
        let first = plot.luma.iter().position(|count| *count > 0).unwrap();
        let last = plot.luma.iter().rposition(|count| *count > 0).unwrap();
        assert!(
            plot.luma[first..=last].iter().all(|count| *count > 0),
            "{:?}",
            &plot.luma[..]
        );

        // A map that lifts nothing has nothing to plot.
        assert!(map(Lift::Iso(iso(0.0))).lift_plot().is_none());
    }

    /// The map is sampled bilinearly at the base pixel's place in it, as
    /// the reference does: a base twice the map's width reads the map's two
    /// values at its ends and a blend of them between.
    #[test]
    fn the_gain_is_blended_between_the_maps_values() {
        let map = map(Lift::Iso(iso(1.0)));
        let table = map.table(1.0);
        let gain = |x: u32| map.gain_at(&table, x, 0, 4, 1)[0];
        assert!((gain(0) - 1.0).abs() < 1e-6);
        // Pixel 2 of 4 is at position 1.0 on a map of 2: the second value.
        assert!((gain(2) - 2.0).abs() < 1e-6);
        // Pixel 1 is half way: half way between the two gains.
        assert!((gain(1) - 1.5).abs() < 1e-6);
        assert!((gain(3) - 2.0).abs() < 1e-6);
    }

    fn profile(baseline: f32, ratios: &[f32], width: u32) -> GainMap {
        let data: Vec<f16> = ratios.iter().copied().map(f16::from_f32).collect();
        let range = ratio_range(&data).unwrap_or([0.0; 2]);
        GainMap {
            width,
            height: (data.len() / 3) as u32 / width,
            channels: 3,
            data: Data::Ratios(data),
            lift: Lift::Profile(Profile {
                baseline,
                table: None,
                curve: None,
                range,
                to_curve: IDENTITY,
            }),
        }
    }

    const IDENTITY: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

    /// A profile's ratios are the gains themselves: all of each at the
    /// whole of the lift, none at no weight, and a channel the developed
    /// picture held nothing of is left as it was.
    #[test]
    fn a_ratio_map_reads_its_ratios_directly() {
        let map = profile(2.0, &[4.0, 0.5, 1.0, 2.0, 2.0, 2.0], 2);
        let on = map.table(map.weight(1.0));
        let off = map.table(0.0);
        assert_eq!(map.gain_at(&on, 0, 0, 2, 1), [4.0, 0.5, 1.0]);
        assert_eq!(map.gain_at(&on, 1, 0, 2, 1), [2.0, 2.0, 2.0]);
        assert_eq!(map.gain_at(&off, 0, 0, 2, 1), [1.0; 3]);
        let mut color = [0.25, 0.5, 0.0];
        on.apply(&mut color, map.gain_at(&on, 0, 0, 2, 1));
        assert_eq!(color, [1.0, 0.25, 0.0]);
        assert_eq!(map.stops_at(0, 0, 2, 1), [2.0, -1.0, 0.0]);
    }

    /// A profile is the camera's rendering, the same on every display:
    /// all of it whatever the room, and whichever way its baseline goes.
    #[test]
    fn the_profile_weight_ignores_the_room() {
        for baseline in [4.12, 0.0, -1.27] {
            let map = profile(baseline, &[1.0, 1.0, 1.0], 1);
            for room in [1.0, 2.0, f32::INFINITY, f32::NAN] {
                assert_eq!(map.weight(room), 1.0, "{baseline} at {room}");
            }
            assert_eq!(map.stops(), baseline);
        }
    }

    /// A map of ratios is plotted over the stops it holds, a pixel to the
    /// bin its stops round to, a plane a channel and their mean for the
    /// luminance; shown, it reads in stops.
    #[test]
    fn a_ratio_map_is_plotted_and_shown_in_stops() {
        let map = profile(0.0, &[0.25, 0.25, 0.25, 4.0, 4.0, 4.0, 1.0, 1.0, 1.0], 3);
        assert_eq!(map.lift_range(), Some([-2.0, 2.0]));
        assert_eq!(map.described_range(), Some([-2.0, 2.0]));
        let plot = map.lift_plot().expect("a lift to plot");
        assert_eq!((plot.min, plot.max), (-2.0, 2.0));
        assert!(plot.luma[0] > 0 && plot.luma[BINS - 1] > 0);
        assert!(plot.luma[plot.bin_at(0.0).expect("on the axis")] > 0);
        assert_eq!(plot.luma.iter().sum::<u32>(), 3 * SHARES as u32);
        let planes = plot.color.expect("a plane a channel");
        assert_eq!(planes[1][0], SHARES as u32);

        let Samples::F32 { channels, data } = map.image().samples else {
            panic!("float samples");
        };
        assert_eq!(channels, Channels::Rgb);
        assert_eq!(data[..3], [-2.0; 3]);
        assert_eq!(data[3..6], [2.0; 3]);
    }

    /// The curve is read linearly between its points and held at its
    /// ends; a profile without one reads the identity.
    #[test]
    fn the_tone_curve_is_read_between_its_points() {
        let points = [[0.0, 0.0], [0.5, 0.25], [1.0, 1.0]];
        assert_eq!(curve_through(&points, 0.5), 0.25);
        assert_eq!(curve_through(&points, 0.25), 0.125);
        assert_eq!(curve_through(&points, 0.75), 0.625);
        assert_eq!(curve_through(&points, 2.0), 1.0);
        assert_eq!(curve_through(&points, -1.0), 0.0);
        let none = profile(0.0, &[1.0; 3], 1);
        assert_eq!(none.profile().expect("a profile").curve_at(0.3), 0.3);
    }
}
