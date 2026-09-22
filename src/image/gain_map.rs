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
//! [`Table`] is what a weight makes of the map: the linear gain each of its
//! 256 values stands for. It is worked out once on the CPU, in
//! [`GainMap::table`], and goes to the shader as a lookup texture, so the
//! two sides cannot disagree about what a value means; and
//! [`GainMap::gain_at`] is the CPU twin of the shader's sampling of the
//! map, for the readout, the statistics and the copy, which have to say
//! what the screen shows.

use std::sync::Arc;

use crate::image::color::Transfer;
use ultrahdr_rs::GainMapMetadata;
use ultrahdr_rs::gainmap::apply::GainMapLut;

/// The map, and what it means.
#[derive(Clone, Debug)]
pub struct GainMap {
    pub width: u32,
    pub height: u32,
    /// One for a luminance map, three for a map with a channel each.
    pub channels: u8,
    pub data: Vec<u8>,
    pub lift: Lift,
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
}

impl GainMap {
    /// Log2 of the whole lift: how far above the base the alternate goes.
    pub fn stops(&self) -> f32 {
        match &self.lift {
            Lift::Iso(metadata) => {
                (metadata.alternate_hdr_headroom - metadata.base_hdr_headroom) as f32
            }
            Lift::Apple { headroom } => headroom.log2(),
        }
    }

    /// How much of the lift a display with `headroom` — the ratio of its
    /// peak to its white, 1 for a monitor in SDR mode — gets: the standard's
    /// weight, where the display's headroom sits between the base's and the
    /// alternate's, from none of it to all of it.
    pub fn weight(&self, headroom: f32) -> f32 {
        let stops = self.stops();
        if stops.is_nan() || stops <= 0.0 || !headroom.is_finite() {
            return 0.0;
        }
        let base = match &self.lift {
            Lift::Iso(metadata) => metadata.base_hdr_headroom as f32,
            Lift::Apple { .. } => 0.0,
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
        };
        Table {
            weight,
            gains,
            base_offset,
            alternate_offset,
        }
    }

    /// The gain at base pixel `(x, y)` of a `width` by `height` base,
    /// through `table`: the map's four surrounding values, each through the
    /// table, blended by the pixel's distance from them, as the standard's
    /// reference samples it. The twin of `gain` in `shaders/image.wgsl`
    /// and `shaders/reduce.wgsl`; keep the three in step.
    pub fn gain_at(&self, table: &Table, x: u32, y: u32, width: u32, height: u32) -> [f32; 3] {
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
            let gain = |corner: usize| table.gain(self.data[corner + channel], channel);
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

/// A gain map shared between the picture, the GPU's copy of it and the
/// threads that read it.
pub type Shared = Arc<GainMap>;

/// Where a base pixel's row or column falls on the map: the two map rows
/// (or columns) either side of it and how far it is from the first.
struct Tap {
    near: usize,
    far: usize,
    fraction: f32,
}

impl Tap {
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
            data: vec![0, 255],
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
}
