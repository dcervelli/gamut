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
//! `ultrahdr-core` does the arithmetic, including the upsample from a gain map
//! that is typically a quarter of the base's size.
//!
//! What comes out is linear light with 1.0 at SDR reference white, which is
//! the working space the rest of this program already speaks: past here an
//! Ultra HDR photograph is just an HDR image, tone mapped on an SDR surface
//! and sent out as-is on an HDR one.

use anyhow::{Context, Result, anyhow, bail};

use ultrahdr_rs::gainmap::apply::{HdrOutputFormat, apply_gainmap};
use ultrahdr_rs::{
    ColorGamut, ColorTransfer, Decoder, GainMap, PixelFormat, RawImage, Unstoppable,
};

use crate::image::{AlphaMode, Channels, ColorSpace, DecodedImage, Primaries, Samples, Transfer};

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

        let base = decode(base_jpeg).context("decoding the base image")?;
        let (width, height) = (base.width(), base.height());
        // Four 32-bit components per pixel is what comes back below, and it
        // is four times the base, so this is the size worth checking.
        crate::image::decode::check_decoded_size(width, height, 4, 32)?;

        let base = RawImage {
            width,
            height,
            format: PixelFormat::Rgba8,
            gamut: gamut(color.primaries),
            transfer: ColorTransfer::Srgb,
            stride: width * 4,
            data: base.to_rgba8().into_raw(),
        };

        let map = decode(map_jpeg).context("decoding the gain map")?;
        let (map_width, map_height) = (map.width(), map.height());
        // The gain map's size is independent of the base's — a tiny picture
        // may carry a huge map — so it is checked in its own right rather than
        // trusted to be "about a quarter of the base". A zero dimension is
        // refused outright: `GainMap` is built here by struct literal, which
        // skips the library's own constructor, and `apply_gainmap` would then
        // compute `width - 1` and index past the end of a zero-size buffer.
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

        // How much of the boost to apply, expressed as the headroom of the
        // display it is being applied for. The whole of it: this viewer has
        // an exposure control and a choice of tone mapping already, and
        // deciding here how bright the monitor is would only take that choice
        // away. On an SDR surface the tone map rolls the highlights off; on
        // an HDR one they go out at the brightness the photographer chose.
        let boost = metadata.alternate_hdr_headroom.exp2().max(1.0) as f32;

        let hdr = apply_gainmap(
            &base,
            &map,
            metadata,
            boost,
            HdrOutputFormat::LinearFloat,
            Unstoppable,
        )
        .map_err(|problem| anyhow!("applying the gain map: {problem}"))?;

        // A copy, not a reinterpret: `hdr.data` is a `Vec<u8>` (alignment 1)
        // and the samples are `f32` (alignment 4), so `bytemuck` cannot reuse
        // the allocation. Both are bounded by the RGBA-f32 ceiling checked
        // above, so the transient second buffer is bounded too.
        let samples: Vec<f32> = bytemuck::cast_slice(&hdr.data).to_vec();
        drop(hdr);

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
        image.value_range = Some((0.0, 1.0));
        Ok(Some(image))
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

/// The gain map's vocabulary for primaries, which is narrower than ours.
///
/// It only labels the result — this file's metadata says the map is applied
/// in the base image's own colour space, and we keep our own answer for what
/// that space is — so the one gamut with no equivalent costs nothing.
fn gamut(primaries: Primaries) -> ColorGamut {
    match primaries {
        Primaries::DisplayP3 => ColorGamut::DisplayP3,
        Primaries::Bt2020 => ColorGamut::Bt2020,
        Primaries::Bt709 | Primaries::AdobeRgb => ColorGamut::Bt709,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::Stats;
    use crate::image::display::{AutoWindow, Display, Headroom, Startup, ToneMap};

    use ultrahdr_rs::{GainMapMetadata, encode_ultrahdr};

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
    const GREY: u8 = 230;

    /// An Ultra HDR file built here rather than checked in: 16x16 of one flat
    /// tone, with a half-size gain map that leaves the left half alone and
    /// asks the right half for two stops more.
    fn sample() -> Vec<u8> {
        const BASE: u32 = 16;
        const MAP: u32 = 8;

        let base = jpeg(
            &[GREY; (BASE * BASE * 3) as usize],
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
        let expected = Transfer::Srgb.to_linear(GREY as f32 / 255.0);
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
        assert!(image.is_high_dynamic_range());
        image.validate().expect("a well-formed buffer");
    }

    /// A photograph has already been graded, and linear float is otherwise
    /// the signature of sensor data — which the startup window stretches. The
    /// stated range is what keeps the grading intact, so it is worth pinning
    /// against the display logic rather than asserting the field alone.
    #[test]
    fn it_opens_windowed_to_the_base_rendition_rather_than_stretched() {
        let image = reconstructed();
        let stats = Stats::scan(&image);
        let display = Display::for_image_with(&image, &stats, Startup::default(), Headroom::None);

        assert_eq!(display.auto, AutoWindow::Manual);
        assert_eq!(display.low, 0.0);
        assert_eq!(display.high, 1.0);
        // And the highlights above that window get rolled off rather than cut.
        assert_eq!(display.tone_map, ToneMap::Neutral);
        assert!(stats.max > 1.0, "the scan has to see the boost too");
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
