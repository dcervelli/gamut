//! The DNGs LibRaw cannot unpack, developed here: a picture stored as
//! Linear Raw — three values a pixel, demosaiced already — in tiles
//! compressed as JPEG XL, which is how an iPhone has written ProRAW since
//! DNG 1.7 gave the format that compression. LibRaw reads JPEG XL only
//! through Adobe's DNG SDK, which the system's library is not built with;
//! and there is little left to do once the tiles are decoded, since the
//! demosaic is the camera's own.
//!
//! What is done is what LibRaw does to a Linear Raw it can read, by the
//! specification's arithmetic: each tile decoded by `jxl-oxide`, each value
//! put through the linearization table, the black and white levels, and the
//! color matrix [`super::profile`] works out, into sixteen bits of linear
//! Rec. 2020 at the same 1.0 a developed raw has. The file's other
//! instructions about how it should look — its baseline exposure, its tone
//! curve, the gain table map that is Apple's local tone mapping — are left
//! alone, as LibRaw's development leaves a camera's curve alone.
//!
//! A file of this compression holding anything else — a mosaic, a picture
//! of other than three channels — is refused by name; where the file
//! carries the camera's JPEG, the loader shows that instead.

use std::sync::Mutex;

use anyhow::{Context, Result, anyhow, bail};
use jxl_oxide::image::BitDepth;
use jxl_oxide::{AllocTracker, JxlImage, PixelFormat};

use super::profile::{self, Matrix, Profile};
use crate::image::tiff::{self, Entry, Order};
use crate::image::{
    AlphaMode, Channels, ColorSpace, DecodedImage, Primaries, Referred, Samples, Transfer,
};

/// The tags read here, by the numbers the TIFF and DNG specifications
/// give them.
mod tag {
    pub const NEW_SUBFILE_TYPE: u16 = 254;
    pub const IMAGE_WIDTH: u16 = 256;
    pub const IMAGE_LENGTH: u16 = 257;
    pub const BITS_PER_SAMPLE: u16 = 258;
    pub const COMPRESSION: u16 = 259;
    pub const PHOTOMETRIC: u16 = 262;
    pub const STRIP_OFFSETS: u16 = 273;
    pub const SAMPLES_PER_PIXEL: u16 = 277;
    pub const ROWS_PER_STRIP: u16 = 278;
    pub const STRIP_BYTE_COUNTS: u16 = 279;
    pub const PLANAR_CONFIGURATION: u16 = 284;
    pub const TILE_WIDTH: u16 = 322;
    pub const TILE_LENGTH: u16 = 323;
    pub const TILE_OFFSETS: u16 = 324;
    pub const TILE_BYTE_COUNTS: u16 = 325;
    pub const DNG_VERSION: u16 = 50706;
    pub const LINEARIZATION_TABLE: u16 = 50712;
    pub const BLACK_LEVEL_REPEAT_DIM: u16 = 50713;
    pub const BLACK_LEVEL: u16 = 50714;
    pub const BLACK_LEVEL_DELTA_H: u16 = 50715;
    pub const BLACK_LEVEL_DELTA_V: u16 = 50716;
    pub const WHITE_LEVEL: u16 = 50717;
    pub const COLOR_MATRIX_1: u16 = 50721;
    pub const COLOR_MATRIX_2: u16 = 50722;
    pub const ANALOG_BALANCE: u16 = 50727;
    pub const AS_SHOT_NEUTRAL: u16 = 50728;
    pub const AS_SHOT_WHITE_XY: u16 = 50729;
    pub const CALIBRATION_ILLUMINANT_1: u16 = 50778;
    pub const CALIBRATION_ILLUMINANT_2: u16 = 50779;
    pub const FORWARD_MATRIX_1: u16 = 50964;
    pub const FORWARD_MATRIX_2: u16 = 50965;
}

/// The compression code DNG 1.7 gives JPEG XL.
const JPEG_XL: f64 = 52546.0;
/// The photometric interpretation of a picture demosaiced already.
const LINEAR_RAW: f64 = 34892.0;

/// The longest linearization table the specification allows: one entry for
/// every sixteen-bit value.
const TABLE_MAX: usize = 1 << 16;

/// A DNG whose picture is Linear Raw in JPEG XL, as its directories
/// describe it.
pub struct Linear {
    width: u32,
    height: u32,
    /// Each tile's size; a strip is a tile the width of the picture.
    tile: [u32; 2],
    /// Where each tile's codestream is in the file, row by row.
    tiles: Vec<std::ops::Range<usize>>,
    /// What each stored value means, where the file says.
    table: Option<Vec<f32>>,
    /// Each channel's black and white, in the linearized values.
    black: [f32; 3],
    white: [f32; 3],
    /// What a column's and a row's black differ from `black` by.
    black_across: Vec<f32>,
    black_down: Vec<f32>,
    /// From the camera's values, scaled to 0..1, to linear Rec. 2020.
    matrix: [[f32; 3]; 3],
}

/// One directory's entries, with the block they point into.
struct Directory<'a> {
    bytes: &'a [u8],
    order: Order,
    entries: Vec<Entry>,
}

impl<'a> Directory<'a> {
    fn numbers(&self, tag: u16) -> Option<Vec<f64>> {
        self.entries
            .iter()
            .find(|entry| entry.tag == tag)?
            .numbers(self.bytes, self.order)
    }

    fn number(&self, tag: u16) -> Option<f64> {
        self.numbers(tag)?.first().copied()
    }

    /// A count or a size, which a negative, fractional or vast number is
    /// not.
    fn whole(&self, tag: u16) -> Result<Option<u32>> {
        self.number(tag).map(|value| whole(value, tag)).transpose()
    }

    fn matrix(&self, tag: u16) -> Result<Option<Matrix>> {
        let Some(values) = self.numbers(tag) else {
            return Ok(None);
        };
        // Three rows of three: a camera of other than three channels has
        // no matrix that lands in a picture of three.
        if values.len() != 9 {
            bail!("tag {tag} holds {} numbers, not a 3x3 matrix", values.len());
        }
        Ok(Some(std::array::from_fn(|row| {
            std::array::from_fn(|column| values[row * 3 + column])
        })))
    }

    fn triple(&self, tag: u16) -> Result<Option<[f64; 3]>> {
        let Some(values) = self.numbers(tag) else {
            return Ok(None);
        };
        match values[..] {
            [a, b, c] => Ok(Some([a, b, c])),
            _ => bail!("tag {tag} holds {} numbers, not three", values.len()),
        }
    }
}

fn whole(value: f64, tag: u16) -> Result<u32> {
    if value.fract() != 0.0 || !(0.0..=f64::from(u32::MAX)).contains(&value) {
        bail!("tag {tag} holds {value}, which is not a count");
    }
    Ok(value as u32)
}

impl Linear {
    /// The file in `bytes`, where it is a DNG whose picture is compressed
    /// with JPEG XL: `None` for any other file, which is LibRaw's to read,
    /// and an error for one of these this cannot develop.
    pub fn read(bytes: &[u8]) -> Result<Option<Self>> {
        let Some(order) = Order::of(bytes) else {
            return Ok(None);
        };
        let Some(entries) = tiff::entries(bytes, order, &[]) else {
            return Ok(None);
        };
        let first = Directory {
            bytes,
            order,
            entries,
        };
        if first.number(tag::DNG_VERSION).is_none() {
            return Ok(None);
        }
        let Some(picture) = picture(&first) else {
            return Ok(None);
        };
        if picture.number(tag::COMPRESSION) != Some(JPEG_XL) {
            return Ok(None);
        }
        let photometric = picture.number(tag::PHOTOMETRIC);
        if photometric != Some(LINEAR_RAW) {
            bail!(
                "the DNG's JPEG XL picture is not Linear Raw (photometric {}), which is all \
                 that can be developed here",
                photometric.map_or("missing".to_string(), |code| code.to_string())
            );
        }
        Self::of(&first, &picture).map(Some)
    }

    fn of(first: &Directory, picture: &Directory) -> Result<Self> {
        let width = picture
            .whole(tag::IMAGE_WIDTH)?
            .context("the DNG's picture has no width")?;
        let height = picture
            .whole(tag::IMAGE_LENGTH)?
            .context("the DNG's picture has no height")?;
        if width == 0 || height == 0 {
            bail!("the DNG's picture is {width}x{height}");
        }
        crate::image::decode::check_decoded_size(width, height, 3, 16)?;
        let samples = picture.whole(tag::SAMPLES_PER_PIXEL)?.unwrap_or(1);
        if samples != 3 {
            bail!("the DNG's Linear Raw has {samples} samples a pixel, not three");
        }
        if picture.whole(tag::PLANAR_CONFIGURATION)?.unwrap_or(1) != 1 {
            bail!("the DNG's Linear Raw is stored a plane at a time");
        }

        // Tiles, or strips: a strip is a tile as wide as the picture.
        let (tile, offsets, counts) = match picture.whole(tag::TILE_WIDTH)? {
            Some(across) => (
                [
                    across,
                    picture
                        .whole(tag::TILE_LENGTH)?
                        .context("the DNG's tiles have no height")?,
                ],
                tag::TILE_OFFSETS,
                tag::TILE_BYTE_COUNTS,
            ),
            None => (
                [
                    width,
                    picture
                        .whole(tag::ROWS_PER_STRIP)?
                        .unwrap_or(height)
                        .min(height),
                ],
                tag::STRIP_OFFSETS,
                tag::STRIP_BYTE_COUNTS,
            ),
        };
        if tile[0] == 0 || tile[1] == 0 {
            bail!("the DNG's tiles are {}x{}", tile[0], tile[1]);
        }
        // A tile is decoded whole, so it is held to the same ceiling as
        // the picture: one claiming to be vast is refused before the
        // decoder is asked for it.
        crate::image::decode::check_decoded_size(tile[0], tile[1], 3, 16)?;
        let expected = width.div_ceil(tile[0]) as usize * height.div_ceil(tile[1]) as usize;
        let offsets = picture.numbers(offsets).unwrap_or_default();
        let counts = picture.numbers(counts).unwrap_or_default();
        if offsets.len() != expected || counts.len() != expected {
            bail!(
                "the DNG's picture is {expected} tiles, and it says where {} of them are",
                offsets.len().min(counts.len())
            );
        }
        let tiles = offsets
            .iter()
            .zip(&counts)
            .map(|(&offset, &count)| {
                let start = whole(offset, tag::TILE_OFFSETS)? as usize;
                let end = start + whole(count, tag::TILE_BYTE_COUNTS)? as usize;
                if end > first.bytes.len() {
                    bail!("a tile of the DNG's picture runs past the end of the file");
                }
                Ok(start..end)
            })
            .collect::<Result<Vec<_>>>()?;

        let table = match picture.numbers(tag::LINEARIZATION_TABLE) {
            Some(table) if table.is_empty() || table.len() > TABLE_MAX => {
                bail!("the DNG's linearization table has {} entries", table.len())
            }
            Some(table) => Some(table.into_iter().map(|value| value as f32).collect()),
            None => None,
        };

        let repeat = picture
            .numbers(tag::BLACK_LEVEL_REPEAT_DIM)
            .unwrap_or_else(|| vec![1.0, 1.0]);
        if repeat != [1.0, 1.0] {
            bail!("the DNG's black level repeats over a pattern, which Linear Raw has none of");
        }
        let black = per_channel(picture.numbers(tag::BLACK_LEVEL), 0.0, "black level")?;
        let bits = picture
            .number(tag::BITS_PER_SAMPLE)
            .unwrap_or(16.0)
            .clamp(1.0, 32.0);
        let full = (2.0f64.powf(bits) - 1.0) as f32;
        let white = per_channel(picture.numbers(tag::WHITE_LEVEL), full, "white level")?;
        if (0..3).any(|c| white[c] <= black[c]) {
            bail!("the DNG's white level is not above its black level");
        }
        let delta = |tag: u16, length: u32| -> Result<Vec<f32>> {
            match picture.numbers(tag) {
                Some(values) if values.len() != length as usize => {
                    bail!(
                        "tag {tag} holds {} numbers, for {length} lines",
                        values.len()
                    )
                }
                Some(values) => Ok(values.into_iter().map(|value| value as f32).collect()),
                None => Ok(vec![0.0; length as usize]),
            }
        };
        let black_across = delta(tag::BLACK_LEVEL_DELTA_H, width)?;
        let black_down = delta(tag::BLACK_LEVEL_DELTA_V, height)?;

        let matrix = color(first)?.to_bt2020()?;
        Ok(Self {
            width,
            height,
            tile,
            tiles,
            table,
            black,
            white,
            black_across,
            black_down,
            matrix,
        })
    }

    /// The picture, as stored: the turn the file asks for is the caller's.
    pub fn develop(&self, bytes: &[u8]) -> Result<DecodedImage> {
        let (width, height) = (self.width as usize, self.height as usize);
        let output = Mutex::new(vec![0u16; width * height * 3]);
        let across = self.width.div_ceil(self.tile[0]) as usize;
        let mut failures: Vec<Option<anyhow::Error>> =
            (0..self.tiles.len()).map(|_| None).collect();
        rayon_core::scope(|scope| {
            for (index, failure) in failures.iter_mut().enumerate() {
                let output = &output;
                scope.spawn(move |_| {
                    let at = [index % across, index / across];
                    if let Err(error) = self.tile(bytes, index, at, output) {
                        *failure = Some(error);
                    }
                });
            }
        });
        if let Some((index, error)) = failures
            .into_iter()
            .enumerate()
            .find_map(|(index, failure)| Some((index, failure?)))
        {
            return Err(error.context(format!("decoding tile {index} of the DNG's picture")));
        }
        let data = output
            .into_inner()
            .map_err(|_| anyhow!("a tile's thread panicked"))?;
        let mut image = DecodedImage::new(
            self.width,
            self.height,
            Samples::U16 {
                channels: Channels::Rgb,
                data,
            },
            ColorSpace {
                transfer: Transfer::Linear,
                primaries: Primaries::Bt2020,
            },
            AlphaMode::Opaque,
        );
        // As LibRaw's development is: linear, balanced, and 1.0 where the
        // sensor saturated.
        image.referred = Referred::Display;
        Ok(image)
    }

    /// Decodes tile `index`, the `at`th across and down, develops it in
    /// place, and copies the part of it inside the picture into `output`.
    fn tile(
        &self,
        bytes: &[u8],
        index: usize,
        at: [usize; 2],
        output: &Mutex<Vec<u16>>,
    ) -> Result<()> {
        let [tile_width, tile_height] = self.tile.map(|side| side as usize);
        let mut values = decode_tile(&bytes[self.tiles[index].clone()], self.tile)?;
        let left = at[0] * tile_width;
        let top = at[1] * tile_height;
        // A tile at the right or the bottom is padded past the picture.
        let columns = tile_width.min(self.width as usize - left);
        let rows = tile_height.min(self.height as usize - top);
        for row in 0..rows {
            let y = top + row;
            for column in 0..columns {
                let x = left + column;
                let pixel = (row * tile_width + column) * 3;
                let camera: [f32; 3] = std::array::from_fn(|c| {
                    let stored = values[pixel + c];
                    let linear = match &self.table {
                        Some(table) => table[usize::from(stored).min(table.len() - 1)],
                        None => f32::from(stored),
                    };
                    let black = self.black[c] + self.black_across[x] + self.black_down[y];
                    ((linear - black) / (self.white[c] - black)).clamp(0.0, 1.0)
                });
                for (c, row) in self.matrix.iter().enumerate() {
                    let value = row[0] * camera[0] + row[1] * camera[1] + row[2] * camera[2];
                    values[pixel + c] = (value.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16;
                }
            }
        }
        let width = self.width as usize;
        let mut output = output
            .lock()
            .map_err(|_| anyhow!("a tile's thread panicked"))?;
        for row in 0..rows {
            let from = row * tile_width * 3;
            let to = ((top + row) * width + left) * 3;
            output[to..to + columns * 3].copy_from_slice(&values[from..from + columns * 3]);
        }
        Ok(())
    }
}

/// The tile's values as stored, three to a pixel, `tile` in size.
fn decode_tile(codestream: &[u8], tile: [u32; 2]) -> Result<Vec<u16>> {
    let tracker = AllocTracker::with_limit(crate::image::decode::MAX_DECODED_BYTES as usize);
    let image = JxlImage::builder()
        .alloc_tracker(tracker)
        .read(codestream)
        .map_err(|error| anyhow!("{error}"))?;
    if [image.width(), image.height()] != tile {
        bail!(
            "the tile is {}x{}, not the {}x{} the DNG says",
            image.width(),
            image.height(),
            tile[0],
            tile[1]
        );
    }
    if image.pixel_format() != PixelFormat::Rgb {
        bail!("the tile is {:?}, not three channels", image.pixel_format());
    }
    // An integer of at most sixteen bits, so that `u16` holds the values
    // as they were stored: `write_to_buffer` scales to the type's full
    // range from the depth the codestream states, which is then the
    // stored integer itself.
    match image.image_header().metadata.bit_depth {
        BitDepth::IntegerSample {
            bits_per_sample: 16,
        } => {}
        other => bail!("the tile's samples are {other:?}, not sixteen-bit integers"),
    }
    let render = image.render_frame(0).map_err(|error| anyhow!("{error}"))?;
    let mut stream = render.stream();
    let mut values = vec![0u16; tile[0] as usize * tile[1] as usize * 3];
    let written = stream.write_to_buffer(&mut values);
    if written != values.len() {
        bail!("the tile gave {written} values, not {}", values.len());
    }
    Ok(values)
}

/// The directory the picture itself is in: the first, or among its
/// SubIFDs, whichever is marked the full-size picture rather than a
/// preview of it.
fn picture<'a>(first: &Directory<'a>) -> Option<Directory<'a>> {
    let full_size =
        |directory: &Directory| directory.number(tag::NEW_SUBFILE_TYPE).unwrap_or(0.0) == 0.0;
    if full_size(first) {
        return Some(Directory {
            entries: tiff::entries(first.bytes, first.order, &[])?,
            ..*first
        });
    }
    first
        .numbers(tiff::tag::SUB_IFDS)?
        .into_iter()
        .filter_map(|offset| {
            let entries = tiff::entries_at(first.bytes, first.order, offset as usize, &[])?;
            Some(Directory { entries, ..*first })
        })
        .find(full_size)
}

/// What the first directory says about the camera's color.
fn color(first: &Directory) -> Result<Profile> {
    let light = |tag: u16| {
        first
            .number(tag)
            .and_then(|code| profile::illuminant(code as u16))
    };
    let mut profile = Profile::default();
    for (matrix, illuminant) in [
        (tag::COLOR_MATRIX_1, tag::CALIBRATION_ILLUMINANT_1),
        (tag::COLOR_MATRIX_2, tag::CALIBRATION_ILLUMINANT_2),
    ] {
        if let Some(matrix) = first.matrix(matrix)? {
            profile.color.push((matrix, light(illuminant)));
        }
    }
    for tag in [tag::FORWARD_MATRIX_1, tag::FORWARD_MATRIX_2] {
        if let Some(matrix) = first.matrix(tag)? {
            profile.forward.push(matrix);
        }
    }
    // A forward matrix stands beside a color matrix, one for one; a file
    // with them unpaired is read by its color matrices alone.
    if profile.forward.len() != profile.color.len() {
        profile.forward.clear();
    }
    profile.analog = first.triple(tag::ANALOG_BALANCE)?;
    profile.neutral = first.triple(tag::AS_SHOT_NEUTRAL)?;
    profile.white_xy = match first.numbers(tag::AS_SHOT_WHITE_XY).as_deref() {
        Some(&[x, y]) => Some([x, y]),
        _ => None,
    };
    Ok(profile)
}

/// A level given once for every channel or once for each, as its tag
/// allows; `missing` where the file gives none.
fn per_channel(values: Option<Vec<f64>>, missing: f32, name: &str) -> Result<[f32; 3]> {
    Ok(match values.as_deref() {
        None => [missing; 3],
        Some(&[all]) => [all as f32; 3],
        Some(&[a, b, c]) => [a as f32, b as f32, c as f32],
        Some(other) => bail!("the DNG's {name} holds {} numbers", other.len()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test_images")
            .join(name);
        std::fs::read(path).expect("the fixture is there")
    }

    /// Only a DNG whose picture is in JPEG XL is taken: a mosaiced DNG,
    /// and anything that is not a DNG, are LibRaw's.
    #[test]
    fn only_a_jpeg_xl_dng_is_taken() {
        assert!(Linear::read(&fixture("dng-jxl.dng")).unwrap().is_some());
        assert!(Linear::read(&fixture("dng-cfa.dng")).unwrap().is_none());
        assert!(Linear::read(&fixture("tiff-rgb8.tif")).unwrap().is_none());
        assert!(Linear::read(b"not a tiff").unwrap().is_none());
    }

    /// A JPEG XL picture that is a mosaic rather than Linear Raw is
    /// refused by name, rather than handed to LibRaw to fail at.
    #[test]
    fn a_mosaic_in_jpeg_xl_is_refused_by_name() {
        let mut bytes = fixture("dng-jxl.dng");
        // The SubIFD's PhotometricInterpretation, Linear Raw, made CFA.
        let linear_raw = [0x06, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x4c, 0x88];
        let at = bytes
            .windows(linear_raw.len())
            .position(|window| window == linear_raw)
            .expect("the fixture's picture is Linear Raw");
        bytes[at + 8..at + 10].copy_from_slice(&32803u16.to_le_bytes());
        let error = Linear::read(&bytes).err().expect("refused");
        assert!(format!("{error:#}").contains("not Linear Raw"), "{error:#}");
    }
}
