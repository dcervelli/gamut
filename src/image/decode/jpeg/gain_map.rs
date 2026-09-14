//! Ultra HDR: a JPEG that carries a second, smaller JPEG saying how much
//! brighter than SDR white each pixel really was.
//!
//! The base image is an ordinary graded photograph, so every viewer ever
//! written shows something sensible. The gain map beside it is a per-pixel
//! log2 multiplier which, applied in linear light, restores the highlights
//! that grading compressed. A viewer that ignores it — as this one did — sees
//! only the SDR half and has no way to know the rest was ever there.
//!
//! Two crates split the work. `ultrahdr-rs` walks the container (MPF, and the
//! XMP directory Google writes alongside it) and hands back the two JPEGs as
//! raw bytes, so `image` stays the only JPEG decoder in the build.
//! `ultrahdr-core` turns the metadata into the table that says what gain
//! each of the map's 256 values stands for. Walking the pixels is done here,
//! in bands of rows across the thread pool: the crate's own `apply_gainmap`
//! walks them on one thread and decodes sRGB with a `powf` per sample, which
//! took four hundred milliseconds on a twelve-megapixel phone photograph —
//! six times the JPEG decode itself.
//!
//! What comes out is linear light with 1.0 at SDR reference white, which is
//! the working space the rest of this program already speaks: past here an
//! Ultra HDR photograph is just an HDR image, tone mapped on an SDR surface
//! and sent out as-is on an HDR one.

use anyhow::{Context, Result, bail};

use ultrahdr_rs::gainmap::apply::GainMapLut;
use ultrahdr_rs::{Decoder, GainMap, GainMapMetadata};

use crate::image::{AlphaMode, Channels, ColorSpace, DecodedImage, Referred, Samples, Transfer};

/// A JPEG, examined for the things the `image` crate throws away: the ICC
/// profile, and a gain map if there is one.
pub struct Container<'a> {
    decoder: Decoder<'a>,
}

impl<'a> Container<'a> {
    /// `None` for anything that is not a JPEG at all. Every real JPEG opens,
    /// gain map or not.
    pub fn open(bytes: &'a [u8]) -> Option<Self> {
        Decoder::new(bytes).ok().map(|decoder| Self { decoder })
    }

    /// The embedded ICC profile, reassembled across the APP2 chunks it is
    /// split into.
    pub fn icc_profile(&self) -> Option<Vec<u8>> {
        self.decoder.icc_profile()
    }

    /// The reconstructed HDR image, or `None` when this JPEG has no gain map.
    ///
    /// `color` is what the base image's own numbers mean, which the caller
    /// has already worked out from the ICC profile. The gain map is applied
    /// in that space and the result stays in it, so only the transfer
    /// function changes on the way through.
    pub fn gain_mapped(&self, color: ColorSpace) -> Result<Option<DecodedImage>> {
        // `is_ultrahdr` is also set for a plain multi-picture JPEG — a stereo
        // pair, or a camera that stores two exposures — where the second
        // image is not a gain map and there is no metadata to describe one.
        // Both have to be present before any of this means anything.
        let (Some(metadata), Some(base_jpeg), Some(map_jpeg)) = (
            self.decoder.metadata(),
            self.decoder.primary_jpeg(),
            self.decoder.gainmap_jpeg(),
        ) else {
            return Ok(None);
        };
        if !self.decoder.is_ultrahdr() {
            return Ok(None);
        }

        // v1.1 allows the stored image to be the HDR one, with the map saying
        // how to get *down* to SDR. Nothing below implements that direction,
        // and quietly brightening an image that is already bright would be
        // worse than saying so.
        if metadata.backward_direction || states_hdr_base(map_jpeg) {
            bail!(
                "gain map runs the other way (the base image is the HDR one), \
                 which this build cannot apply — try --no-gain-map"
            );
        }

        let base = decode(base_jpeg)
            .context("decoding the base image")?
            .into_rgb8();
        let (width, height) = base.dimensions();
        // Four 32-bit components per pixel is what comes back below, and it
        // is four times the base, so this is the size worth checking.
        crate::image::decode::check_decoded_size(width, height, 4, 32)?;

        let map = decode(map_jpeg).context("decoding the gain map")?;
        let (map_width, map_height) = (map.width(), map.height());
        // The gain map's size is independent of the base's — a tiny picture
        // may carry a huge map — so it is checked in its own right rather than
        // trusted to be "about a quarter of the base". A zero dimension is
        // refused outright: `Tap::at` would otherwise compute `width - 1` and
        // index past the end of a zero-size buffer.
        if map_width == 0 || map_height == 0 {
            bail!("the gain map has a zero dimension");
        }
        let channels = if map.color().has_color() { 3 } else { 1 };
        crate::image::decode::check_decoded_size(map_width, map_height, channels as usize, 8)?;
        let data = if channels == 3 {
            map.to_rgb8().into_raw()
        } else {
            map.to_luma8().into_raw()
        };
        let map = GainMap {
            width: map_width,
            height: map_height,
            channels,
            data,
        };

        // How much of the boost to apply. The whole of it: this viewer has
        // an exposure control and a choice of tone mapping already, and
        // deciding here how bright the monitor is would only take that choice
        // away. On an SDR surface the tone map rolls the highlights off; on
        // an HDR one they go out at the brightness the photographer chose.
        // The spec's weight is where the display's headroom sits between the
        // base's and the alternate's, so the whole boost is a weight of one.
        let lut = GainMapLut::new(metadata, 1.0);
        let samples = reconstruct(&base, &map, &lut, metadata);

        let mut image = DecodedImage::new(
            width,
            height,
            Samples::F32 {
                // Four components rather than three, so that the upload path
                // can hand the buffer to the GPU without widening it first.
                channels: Channels::Rgba,
                data: samples,
            },
            ColorSpace {
                transfer: Transfer::Linear,
                primaries: color.primaries,
            },
            // The base is a JPEG, so there was never an alpha channel; the
            // fourth component is padding the shader must not read.
            AlphaMode::Opaque,
        );
        // The photograph was graded to sit in 0..1 and the gain map is what
        // puts the highlights above it. Saying so keeps the startup window
        // off the percentile stretch that linear float otherwise asks for,
        // which would undo the grading the moment it loaded.
        image.referred = Referred::Display;
        Ok(Some(image))
    }
}

/// The base image, as it decoded, multiplied through the gain map:
/// linear light, four floats to a pixel with the fourth left at one.
///
/// Each pixel of the base is decoded to linear through a table, and the map
/// — usually a quarter of the base's size in each direction — is sampled
/// bilinearly at the pixel's position, as the specification's reference does.
/// The rows are cut into one band per thread; a pixel depends on nothing but
/// itself and the map, so the split changes no result.
fn reconstruct(
    base: &::image::RgbImage,
    map: &GainMap,
    lut: &GainMapLut,
    metadata: &GainMapMetadata,
) -> Vec<f32> {
    let (width, height) = (base.width() as usize, base.height() as usize);
    let pixels = width * height;
    let srgb: [f32; 256] =
        std::array::from_fn(|value| Transfer::Srgb.to_linear(value as f32 / 255.0));
    let base_offset = metadata.base_offset.map(|offset| offset as f32);
    let alternate_offset = metadata.alternate_offset.map(|offset| offset as f32);
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
        let rows = base.as_raw()[index * per_band * width * 3..]
            .chunks_exact(width * 3)
            .zip(band.chunks_exact_mut(width * 4));
        for (y, (row, out)) in rows.enumerate() {
            let row_tap = Tap::at(index * per_band + y, height, map.height);
            let (samples, _) = row.as_chunks::<3>();
            let (pixels, _) = out.as_chunks_mut::<4>();
            for ((sample, out), column) in samples.iter().zip(pixels).zip(&columns) {
                let gain = sample_gain(map, lut, column, &row_tap);
                for channel in 0..3 {
                    out[channel] = (srgb[sample[channel] as usize] + base_offset[channel])
                        * gain[channel]
                        - alternate_offset[channel];
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

/// Below this many pixels the reconstruction stays on one thread.
const PARALLEL_FROM: usize = 1 << 16;

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
fn sample_gain(map: &GainMap, lut: &GainMapLut, column: &Tap, row: &Tap) -> [f32; 3] {
    let stride = map.width as usize;
    let corner = |x: usize, y: usize| (y * stride + x) * map.channels as usize;
    let (c00, c10, c01, c11) = (
        corner(column.near, row.near),
        corner(column.far, row.near),
        corner(column.near, row.far),
        corner(column.far, row.far),
    );
    let blend = |channel: usize| {
        let gain = |corner: usize| lut.lookup(map.data[corner + channel], channel);
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

/// Whether the file says its *base* image is the HDR one, so that the gain
/// map describes the way down to SDR rather than up.
///
/// `hdrgm:BaseRenditionIsHDR` is the attribute that says so, and the metadata
/// parser in `ultrahdr-rs` 0.3.5 does not read it: `backward_direction` comes
/// back false for every file, whatever the file says. Applying a boost to the
/// image that was already the bright one would be silently, badly wrong, so
/// the attribute is looked for here instead of trusted to the parser.
///
/// It is looked for in the gain map's own bytes, which is where the modern
/// layout keeps the `hdrgm` block and which is small enough to scan whole.
/// Searching the bytes rather than the XMP segment alone means trusting that
/// entropy-coded image data will not spell out an eighteen-character
/// attribute name followed by `="true"`, which it will not.
fn states_hdr_base(gain_map: &[u8]) -> bool {
    const ATTRIBUTE: &[u8] = b"BaseRenditionIsHDR=";
    let Some(at) = gain_map
        .windows(ATTRIBUTE.len())
        .position(|window| window == ATTRIBUTE)
    else {
        return false;
    };

    // Past the attribute name and the opening quote, whichever quote the
    // writer chose.
    let value: Vec<u8> = gain_map[at + ATTRIBUTE.len()..]
        .iter()
        .skip(1)
        .take(4)
        .map(u8::to_ascii_lowercase)
        .collect();
    value == b"true"
}

fn decode(bytes: &[u8]) -> Result<::image::DynamicImage> {
    let mut reader =
        ::image::ImageReader::with_format(std::io::Cursor::new(bytes), ::image::ImageFormat::Jpeg);
    let mut limits = ::image::Limits::default();
    limits.max_alloc = Some(crate::image::decode::MAX_DECODED_BYTES);
    reader.limits(limits);
    Ok(reader.decode()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::display::{AutoWindow, Display, Headroom, Startup, ToneMap};
    use crate::image::{Primaries, Stats};

    use ultrahdr_rs::{ColorGamut, encode_ultrahdr};

    /// Log2 of the boost the brightest half of the test gain map asks for.
    /// Two stops: enough to be unmistakable against JPEG's own error.
    const STOPS: f64 = 2.0;

    fn jpeg(data: &[u8], width: u32, height: u32, kind: ::image::ExtendedColorType) -> Vec<u8> {
        let mut encoded = Vec::new();
        ::image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, 100)
            .encode(data, width, height, kind)
            .expect("the test images encode");
        encoded
    }

    /// How bright the flat base image is. High enough that two stops of boost
    /// clears SDR white, which is the thing worth asserting.
    const GRAY: u8 = 230;

    /// An Ultra HDR file built here rather than checked in: 16x16 of one flat
    /// tone, with a half-size gain map that leaves the left half alone and
    /// asks the right half for two stops more.
    fn sample() -> Vec<u8> {
        const BASE: u32 = 16;
        const MAP: u32 = 8;

        let base = jpeg(
            &[GRAY; (BASE * BASE * 3) as usize],
            BASE,
            BASE,
            ::image::ExtendedColorType::Rgb8,
        );

        let map: Vec<u8> = (0..MAP * MAP)
            .map(|index| if index % MAP < MAP / 2 { 0 } else { 255 })
            .collect();
        let map = jpeg(&map, MAP, MAP, ::image::ExtendedColorType::L8);

        // `GainMapMetadata` is non-exhaustive, so it is built by amending the
        // defaults rather than by naming every field.
        let mut metadata = GainMapMetadata::default();
        metadata.gain_map_max = [STOPS; 3];
        metadata.gain_map_min = [0.0; 3];
        metadata.base_offset = [0.0; 3];
        metadata.alternate_offset = [0.0; 3];
        metadata.alternate_hdr_headroom = STOPS;
        encode_ultrahdr(&base, &map, &metadata, ColorGamut::DisplayP3)
            .expect("the sample assembles")
    }

    fn reconstructed() -> DecodedImage {
        let bytes = sample();
        Container::open(&bytes)
            .expect("a JPEG opens")
            .gain_mapped(ColorSpace {
                transfer: Transfer::Srgb,
                primaries: Primaries::DisplayP3,
            })
            .expect("the gain map applies")
            .expect("the sample has a gain map")
    }

    fn pixel(image: &DecodedImage, x: u32, y: u32) -> [f32; 3] {
        let Samples::F32 { data, .. } = &image.samples else {
            panic!("reconstruction is always float");
        };
        let base = ((y * image.width + x) * 4) as usize;
        [data[base], data[base + 1], data[base + 2]]
    }

    /// The whole point: the half the gain map marks comes back brighter than
    /// SDR white, by the number of stops the metadata asked for, while the
    /// half it leaves alone stays exactly where the base image put it.
    #[test]
    fn the_marked_half_comes_back_two_stops_brighter() {
        let image = reconstructed();

        let plain = pixel(&image, 3, 8)[1];
        let boosted = pixel(&image, 12, 8)[1];

        // The base tone through the sRGB curve, which is what the untouched
        // half must still be. JPEG is lossy even at quality 100, hence the
        // slack.
        let expected = Transfer::Srgb.to_linear(GRAY as f32 / 255.0);
        assert!(
            (plain - expected).abs() < 0.02,
            "unmarked half moved: {plain} against {expected}"
        );

        let ratio = boosted / plain;
        assert!(
            (ratio - STOPS.exp2() as f32).abs() < 0.2,
            "marked half was boosted {ratio}x, expected {}x",
            STOPS.exp2()
        );
        assert!(
            boosted > 1.0,
            "the boost has to clear SDR white, got {boosted}"
        );
    }

    /// The reconstruction has to describe itself as linear light in the base
    /// image's own primaries, or the shader converts from the wrong space and
    /// the renderer windows it as though it were sensor data.
    #[test]
    fn the_reconstruction_describes_itself_as_linear_light() {
        let image = reconstructed();

        assert_eq!(image.color.transfer, Transfer::Linear);
        assert_eq!(image.color.primaries, Primaries::DisplayP3);
        assert_eq!(image.channels(), Channels::Rgba);
        assert_eq!(image.alpha, AlphaMode::Opaque);
        assert_eq!(image.referred, Referred::Display);
        image.validate().expect("a well-formed buffer");
    }

    /// A photograph has already been graded, and linear float is otherwise
    /// the signature of sensor data — which the startup window stretches.
    /// Saying the light is display-referred is what keeps the grading intact,
    /// so it is worth pinning against the display logic rather than asserting
    /// the field alone.
    #[test]
    fn it_opens_windowed_to_the_base_rendition_rather_than_stretched() {
        let image = reconstructed();
        let stats = Stats::scan(&image);
        let display = Display::for_image_with(&image, &stats, Startup::default(), Headroom::None);

        assert_eq!(display.auto, AutoWindow::Off);
        assert_eq!(display.low, 0.0);
        assert_eq!(display.high, 1.0);
        // And the highlights above that window get rolled off rather than cut.
        assert_eq!(display.tone_map, ToneMap::Neutral);
        assert!(stats.max > 1.0, "the scan has to see the boost too");
    }

    /// The crate's own `apply_gainmap` over the same base and map, as the
    /// floats it writes.
    fn reference(base: &::image::RgbImage, map: &GainMap, metadata: &GainMapMetadata) -> Vec<f32> {
        use ultrahdr_rs::gainmap::apply::{HdrOutputFormat, apply_gainmap};
        use ultrahdr_rs::{ColorTransfer, PixelFormat, RawImage, Unstoppable};

        let base = RawImage {
            width: base.width(),
            height: base.height(),
            format: PixelFormat::Rgb8,
            gamut: ColorGamut::Bt709,
            transfer: ColorTransfer::Srgb,
            stride: base.width() * 3,
            data: base.as_raw().clone(),
        };
        let hdr = apply_gainmap(
            &base,
            map,
            metadata,
            metadata.alternate_hdr_headroom.exp2() as f32,
            HdrOutputFormat::LinearFloat,
            Unstoppable,
        )
        .expect("the reference applies");
        bytemuck::cast_slice(&hdr.data).to_vec()
    }

    fn assert_agrees(ours: &[f32], theirs: &[f32]) {
        assert_eq!(ours.len(), theirs.len());
        for (index, (a, b)) in ours.iter().zip(theirs).enumerate() {
            // The two decode sRGB by different arithmetic, exact to a few
            // parts in a million of each other.
            assert!(
                (a - b).abs() <= 1e-4 * b.abs().max(1.0),
                "sample {index}: {a} here against {b} from the crate"
            );
        }
    }

    /// The banded walk here stands in for the crate's own `apply_gainmap`,
    /// so it has to produce the same numbers: the sample's map has a hard
    /// edge down its middle, which the bilinear sampling blends across, and
    /// every pixel is checked, not just the flat halves.
    #[test]
    fn the_reconstruction_agrees_with_the_crates_own() {
        let bytes = sample();
        let container = Container::open(&bytes).expect("a JPEG opens");
        let ours = container
            .gain_mapped(ColorSpace::SRGB)
            .expect("the gain map applies")
            .expect("the sample has a gain map");
        let Samples::F32 { data: ours, .. } = &ours.samples else {
            panic!("reconstruction is always float");
        };

        let metadata = container
            .decoder
            .metadata()
            .expect("the sample has metadata");
        let base = decode(container.decoder.primary_jpeg().unwrap())
            .unwrap()
            .into_rgb8();
        let map = decode(container.decoder.gainmap_jpeg().unwrap())
            .unwrap()
            .into_luma8();
        let map = GainMap {
            width: map.width(),
            height: map.height(),
            channels: 1,
            data: map.into_raw(),
        };
        assert_agrees(ours, &reference(&base, &map, metadata));
    }

    /// The same, over a picture large enough to be cut into bands, with a
    /// three-channel map whose size is no neat fraction of the base's so
    /// that every row and column lands between map samples; and with
    /// offsets, which the sample above leaves at zero.
    #[test]
    fn a_banded_reconstruction_agrees_with_the_crates_own() {
        let (width, height) = (1000u32, 200u32);
        assert!((width * height) as usize > PARALLEL_FROM);
        let base = ::image::RgbImage::from_fn(width, height, |x, y| {
            ::image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
        });
        let (map_width, map_height) = (301u32, 67u32);
        let map = GainMap {
            width: map_width,
            height: map_height,
            channels: 3,
            data: (0..map_width * map_height * 3)
                .map(|index| (index * 7 % 256) as u8)
                .collect(),
        };
        let mut metadata = GainMapMetadata::default();
        metadata.gain_map_max = [STOPS; 3];
        metadata.gain_map_min = [-0.5, 0.0, 0.25];
        metadata.gamma = [1.0, 1.5, 2.0];
        metadata.base_offset = [0.015625; 3];
        metadata.alternate_offset = [0.01, 0.02, 0.03];
        metadata.alternate_hdr_headroom = STOPS;

        let lut = GainMapLut::new(&metadata, 1.0);
        let ours = reconstruct(&base, &map, &lut, &metadata);
        assert_agrees(&ours, &reference(&base, &map, &metadata));
    }

    /// Every ordinary JPEG goes through the same call, and must come back
    /// untouched rather than be mistaken for a container.
    #[test]
    fn an_ordinary_jpeg_has_no_gain_map() {
        let plain = jpeg(&[200u8; 48], 4, 4, ::image::ExtendedColorType::Rgb8);
        let opened = Container::open(&plain).expect("a JPEG opens");
        assert!(opened.gain_mapped(ColorSpace::SRGB).unwrap().is_none());
        assert!(opened.icc_profile().is_none());
    }

    /// A base image that is already HDR needs the map run the other way,
    /// which nothing here does. Saying so beats brightening it twice.
    ///
    /// The encoder always writes `BaseRenditionIsHDR="False"`, so the file is
    /// made by rewriting that attribute in place — `"True" ` is the same
    /// length as `"False"`, which keeps every JPEG segment length correct.
    #[test]
    fn a_backward_gain_map_is_refused_rather_than_applied() {
        let bytes = sample();
        let forward = &b"BaseRenditionIsHDR=\"False\""[..];
        let backward = &b"BaseRenditionIsHDR=\"True\" "[..];
        assert_eq!(forward.len(), backward.len());

        let at = bytes
            .windows(forward.len())
            .position(|window| window == forward)
            .expect("the encoder states the direction");
        let mut bytes = bytes;
        bytes[at..at + backward.len()].copy_from_slice(backward);

        let error = Container::open(&bytes)
            .expect("a JPEG opens")
            .gain_mapped(ColorSpace::SRGB)
            .expect_err("a backward gain map is refused");
        assert!(format!("{error:#}").contains("--no-gain-map"), "{error:#}");
    }

    /// And the same file, unedited, must not trip that check — otherwise
    /// every ordinary Ultra HDR photograph would be refused.
    #[test]
    fn a_forward_gain_map_is_not_mistaken_for_a_backward_one() {
        let bytes = sample();
        assert!(!states_hdr_base(&bytes));
        assert!(
            Container::open(&bytes)
                .expect("a JPEG opens")
                .gain_mapped(ColorSpace::SRGB)
                .expect("a forward gain map applies")
                .is_some()
        );
    }
}
