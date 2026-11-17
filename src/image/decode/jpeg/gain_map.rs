//! Ultra HDR: a JPEG that carries a second, smaller JPEG saying how much
//! brighter than SDR white each pixel really was.
//!
//! The base image is an ordinary graded photograph, so every viewer ever
//! written shows something sensible. The gain map beside it is a per-pixel
//! log2 multiplier which, applied in linear light, restores the highlights
//! that grading compressed. A viewer that ignores it sees only the SDR half
//! and has no way to know the rest was ever there.
//!
//! Two crates split the work. `ultrahdr-rs` walks the container (MPF, and the
//! XMP directory Google writes alongside it) and hands back the two JPEGs as
//! raw bytes, so `image` stays the only JPEG decoder in the build.
//! What the map means, and its application by the display, is
//! [`crate::image::gain_map`]'s, shared with the HEIF that carries the same
//! kind of map: what comes back from here is the base photograph with the
//! map beside it.

use std::sync::Arc;

use anyhow::{Context, Result, bail};

use ultrahdr_rs::{Decoder, GainMapMetadata};

use crate::image::apple_note;
use crate::image::gain_map::{APPLE_AUXILIARY, GainMap, Lift};
use crate::image::{AlphaMode, Channels, ColorSpace, DecodedImage, Samples};

/// A JPEG, examined for the things the `image` crate throws away: the ICC
/// profile, and a gain map if there is one.
pub(super) struct Container<'a> {
    bytes: &'a [u8],
    decoder: Decoder<'a>,
}

impl<'a> Container<'a> {
    /// `None` for anything that is not a JPEG at all. Every real JPEG opens,
    /// gain map or not.
    pub fn open(bytes: &'a [u8]) -> Option<Self> {
        Decoder::new(bytes)
            .ok()
            .map(|decoder| Self { bytes, decoder })
    }

    /// The photograph's own ICC profile, reassembled across the APP2 chunks
    /// it is split into.
    ///
    /// Read from the photograph's header alone. `ultrahdr-rs` 0.3.5 looks
    /// for the chunks byte by byte through the whole file, so the profile of
    /// a gain map after the photograph — Apple's, which states PQ — would
    /// otherwise be taken for the photograph's, or spliced into it.
    pub fn icc_profile(&self) -> Option<Vec<u8>> {
        const NAME: &[u8] = b"ICC_PROFILE\0";
        let mut chunks: Vec<(u8, &[u8])> = header(self.bytes)?
            .segments
            .into_iter()
            .filter(|&(marker, _)| marker == APP2)
            .filter_map(|(_, payload)| payload.strip_prefix(NAME))
            .filter_map(|chunk| Some((*chunk.first()?, chunk.get(2..)?)))
            .collect();
        if chunks.is_empty() {
            return None;
        }
        chunks.sort_by_key(|&(sequence, _)| sequence);
        Some(
            chunks
                .into_iter()
                .flat_map(|(_, chunk)| chunk.to_vec())
                .collect(),
        )
    }

    /// The photograph with its gain map beside it, or `None` when this JPEG
    /// has no gain map.
    ///
    /// `color` is what the base image's own numbers mean, which the caller
    /// has already worked out from the ICC profile. The gain map is applied
    /// in that space and the result stays in it, so only the transfer
    /// function changes on the way through.
    ///
    /// What the map means is looked for in three places, the standard's
    /// first: the ISO 21496-1 block in the map's own APP2, which libultrahdr,
    /// Android and Apple's own export all write; then the `hdrgm` XMP Google
    /// and Adobe write, which `ultrahdr-rs` reads; then Apple's auxiliary
    /// type in the map's XMP, whose headroom is the photograph's maker
    /// note's. A JPEG out of Apple's export carries the first and the third
    /// and no `hdrgm` at all.
    pub fn gain_mapped(&self, color: ColorSpace) -> Result<Option<DecodedImage>> {
        let (Some(base_jpeg), Some(map_jpeg)) =
            (self.decoder.primary_jpeg(), self.decoder.gainmap_jpeg())
        else {
            return Ok(None);
        };
        if !self.split_holds(base_jpeg, map_jpeg) {
            return Ok(None);
        }
        // `is_ultrahdr` is also set for a plain multi-picture JPEG — a stereo
        // pair, or a camera that stores two exposures — where the second
        // image is not a gain map, and the crate's metadata is only read from
        // an `hdrgm` block, so the second picture is a gain map only where
        // one of the three says so.
        let xmp = || {
            self.decoder
                .metadata()
                .filter(|_| self.decoder.is_ultrahdr())
                .cloned()
        };
        let lift = match iso_metadata(map_jpeg).or_else(xmp) {
            Some(metadata) => {
                // v1.1 allows the stored image to be the HDR one, with the
                // map saying how to get *down* to SDR. Nothing below
                // implements that direction, and quietly brightening an image
                // that is already bright would be worse than saying so.
                if metadata.backward_direction || states_hdr_base(map_jpeg) {
                    bail!(
                        "gain map runs the other way (the base image is the HDR one), \
                         which this build cannot apply — try --no-gain-map"
                    );
                }
                Lift::Iso(metadata)
            }
            None => match apple_headroom(base_jpeg, map_jpeg) {
                Some(headroom) => Lift::Apple { headroom },
                None => return Ok(None),
            },
        };

        let base = decode(base_jpeg)
            .context("decoding the base image")?
            .into_rgb8();
        let (width, height) = base.dimensions();

        let map = decode(map_jpeg).context("decoding the gain map")?;
        let (map_width, map_height) = (map.width(), map.height());
        // The gain map's size is independent of the base's — a tiny picture
        // may carry a huge map — so it is checked in its own right rather than
        // trusted to be "about a quarter of the base". A zero dimension is
        // refused outright: the taps would otherwise compute `width - 1` and
        // index past the end of a zero-size buffer.
        if map_width == 0 || map_height == 0 {
            bail!("the gain map has a zero dimension");
        }
        let channels = if map.color().has_color() { 3 } else { 1 };
        let data = if channels == 3 {
            map.into_rgb8().into_raw()
        } else {
            map.into_luma8().into_raw()
        };

        // The base as it decoded, with the map beside it: the lift is the
        // display's to apply, at the weight its room asks for.
        let mut image = DecodedImage::new(
            width,
            height,
            Samples::U8 {
                channels: Channels::Rgb,
                data: base.into_raw(),
            },
            color,
            AlphaMode::Opaque,
        );
        image.gain_map = Some(Arc::new(GainMap {
            width: map_width,
            height: map_height,
            channels,
            data: crate::image::gain_map::Data::Codes(data),
            lift,
        }));
        Ok(Some(image))
    }

    /// Whether the two JPEGs the crate split the file into are the
    /// photograph and an image after it.
    ///
    /// Without an MPF directory, `ultrahdr-rs` 0.3.5 falls back to pairing
    /// each `FFD8` in the file with the next `FFD9`, and the thumbnails that
    /// EXIF and Photoshop's APP13 carry are JPEGs of their own. A file whose
    /// XMP still names a gain map it no longer has — Lightroom's export, put
    /// through something that dropped the second image — then comes back as a
    /// "base" that is the headers up to the EXIF thumbnail's end, and a "map"
    /// that is the next thumbnail. The photograph's own scan is the test: the
    /// base has to run from the start of the file through it, and the map has
    /// to begin after the base ends. Anything else is a plain JPEG.
    fn split_holds(&self, base: &[u8], map: &[u8]) -> bool {
        let Some(scan) = header(self.bytes).map(|header| header.scan) else {
            return false;
        };
        let offset = |part: &[u8]| part.as_ptr() as usize - self.bytes.as_ptr() as usize;
        let base_end = offset(base) + base.len();
        offset(base) == 0 && base_end > scan && offset(map) >= base_end
    }
}

/// The APP2 segment holding ICC profile chunks.
const APP2: u8 = 0xE2;

/// A JPEG's header: the marker segments before its first scan, each its
/// marker and its payload, and where the scan's SOS marker is.
struct Header<'a> {
    segments: Vec<(u8, &'a [u8])>,
    scan: usize,
}

/// The header of the JPEG `bytes` begins with, found by walking the segments
/// by their lengths, so that a JPEG carried inside an APP segment is stepped
/// over whole rather than mistaken for the file's own. `None` where a
/// segment runs past the end, or something that is not a marker stands
/// where one should.
fn header(bytes: &[u8]) -> Option<Header<'_>> {
    let mut segments = Vec::new();
    let mut at = 2;
    loop {
        let marker = bytes.get(at..at + 2)?;
        if marker[0] != 0xFF {
            return None;
        }
        match marker[1] {
            0xDA => return Some(Header { segments, scan: at }),
            // Fill bytes before a marker.
            0xFF => at += 1,
            // The markers that stand alone, with no length after them.
            0x01 | 0xD0..=0xD8 => at += 2,
            kind => {
                let length = bytes.get(at + 2..at + 4)?;
                let length = u16::from_be_bytes([length[0], length[1]]) as usize;
                segments.push((kind, bytes.get(at + 4..at + 2 + length.max(2))?));
                at += 2 + length;
            }
        }
    }
}

/// The ISO 21496-1 metadata in the gain map's own APP2, where the
/// standard puts it, under its name. The photograph may carry a block of
/// its own that holds only the versions, which says a map follows and
/// nothing about it, so the map's is the one read.
fn iso_metadata(gain_map: &[u8]) -> Option<GainMapMetadata> {
    const NAME: &[u8] = b"urn:iso:std:iso:ts:21496:-1\0";
    header(gain_map)?
        .segments
        .into_iter()
        .filter(|&(marker, _)| marker == APP2)
        .find_map(|(_, payload)| payload.strip_prefix(NAME))
        .and_then(crate::image::gain_map::iso_metadata)
}

/// The headroom of Apple's gain map, where the second picture says in its
/// XMP that it is one, from the maker note in the photograph's EXIF. `None`
/// where either is missing: a map whose headroom is not known cannot be
/// applied.
fn apple_headroom(base: &[u8], gain_map: &[u8]) -> Option<f32> {
    const XMP: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
    const APP1: u8 = 0xE1;
    let kind = APPLE_AUXILIARY.as_bytes();
    let says_so = header(gain_map)?
        .segments
        .into_iter()
        .filter(|&(marker, _)| marker == APP1)
        .filter_map(|(_, payload)| payload.strip_prefix(XMP))
        .any(|packet| packet.windows(kind.len()).any(|window| window == kind));
    if !says_so {
        return None;
    }
    let exif = exif::Reader::new()
        .read_from_container(&mut std::io::Cursor::new(base))
        .ok()?;
    let field = exif.get_field(exif::Tag::MakerNote, exif::In::PRIMARY)?;
    match &field.value {
        exif::Value::Undefined(note, _) => apple_note::headroom(note),
        _ => None,
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

/// One of the two JPEGs decoded, under the ceilings every decoder here
/// works under, and checked against the byte ceiling from its header before
/// the crate allocates for it. The base and the map are each checked in
/// their own right: the map's size is independent of the base's, and a tiny
/// picture may carry a huge map.
fn decode(bytes: &[u8]) -> Result<::image::DynamicImage> {
    use ::image::ImageDecoder;
    let mut decoder = ::image::codecs::jpeg::JpegDecoder::new(std::io::Cursor::new(bytes))?;
    decoder.set_limits(super::super::dynamic::limits())?;
    let (width, height) = decoder.dimensions();
    let components = decoder.color_type().channel_count() as usize;
    crate::image::decode::check_decoded_size(width, height, components, 8)?;
    Ok(::image::DynamicImage::from_decoder(decoder)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::display::{AutoWindow, Display, Startup, ToneMap};
    use crate::image::gain_map::Table;
    use crate::image::{Primaries, Referred, Stats, Transfer};

    use ultrahdr_rs::{ColorGamut, GainMap as CrateMap, GainMapMetadata, encode_ultrahdr};

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

    fn metadata() -> GainMapMetadata {
        // `GainMapMetadata` is non-exhaustive, so it is built by amending the
        // defaults rather than by naming every field.
        let mut metadata = GainMapMetadata::default();
        metadata.gain_map_max = [STOPS; 3];
        metadata.gain_map_min = [0.0; 3];
        metadata.base_offset = [0.0; 3];
        metadata.alternate_offset = [0.0; 3];
        metadata.alternate_hdr_headroom = STOPS;
        metadata
    }

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

        encode_ultrahdr(&base, &map, &metadata(), ColorGamut::DisplayP3)
            .expect("the sample assembles")
    }

    /// The sample opened: the base with the map beside it.
    fn opened() -> DecodedImage {
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

    /// The whole lift, as the display applies it on a monitor with all the
    /// room the file asks for.
    fn whole(image: &DecodedImage) -> Table {
        image.gain_map.as_ref().expect("a gain map").table(1.0)
    }

    fn pixel(image: &DecodedImage, table: &Table, x: u32, y: u32) -> [f32; 3] {
        let sample = image.sample(x, y, Some(table)).expect("inside the image");
        let color = sample.color();
        [color[0], color[1], color[2]]
    }

    /// The whole point: the half the gain map marks comes back brighter than
    /// SDR white, by the number of stops the metadata asked for, while the
    /// half it leaves alone stays exactly where the base image put it.
    #[test]
    fn the_marked_half_comes_back_two_stops_brighter() {
        let image = opened();
        let table = whole(&image);

        let plain = pixel(&image, &table, 3, 8)[1];
        let boosted = pixel(&image, &table, 12, 8)[1];

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

    /// What comes back is the photograph as graded — 8-bit, on the sRGB
    /// curve, in the base's own primaries — with the map beside it saying
    /// how far it goes above white, rather than a picture already lifted.
    #[test]
    fn the_base_is_the_photograph_with_the_map_beside_it() {
        let image = opened();

        assert_eq!(image.color.transfer, Transfer::Srgb);
        assert_eq!(image.color.primaries, Primaries::DisplayP3);
        assert_eq!(image.channels(), Channels::Rgb);
        assert_eq!(image.alpha, AlphaMode::Opaque);
        assert_eq!(image.referred, Referred::Display);
        let map = image.gain_map.as_ref().expect("the map rides with it");
        assert_eq!((map.width, map.height, map.channels), (8, 8, 1));
        assert!((map.stops() - STOPS as f32).abs() < 1e-6);
        image.validate().expect("a well-formed buffer");
    }

    /// On a monitor with no room above white the picture is the base, and
    /// nothing in it is past white; the window is the base's. On one with
    /// the room the statistics see the lift, and the display reports what
    /// of it is past white — which the weight that follows the room keeps
    /// the surface from clipping. No curve comes on by itself either way.
    #[test]
    fn the_window_is_the_bases_and_the_statistics_see_what_is_lifted() {
        let image = opened();

        let base = Stats::scan(&image);
        assert!(base.max <= 1.0);
        let display = Display::for_image_with(&image, &base, Startup::default());
        assert_eq!(display.auto(), AutoWindow::Off);
        assert_eq!((display.window_low(), display.window_high()), (0.0, 1.0));
        assert_eq!(display.tone_map(), ToneMap::None);
        assert!(!display.exceeds_white(&base));

        let lifted = Stats::scan_with(&image, Some(&whole(&image)));
        assert!(lifted.max > 1.0, "the scan has to see the lift");
        let display = Display::for_image_with(&image, &lifted, Startup::default());
        assert_eq!((display.window_low(), display.window_high()), (0.0, 1.0));
        assert_eq!(display.tone_map(), ToneMap::None);
        assert!(display.exceeds_white(&lifted));

        // As the file stores it, the picture is the base, whatever the room:
        // what the histogram plots the file as.
        assert!(lifted.plot.max > 1.0);
        let stored = crate::image::stats::Stored::scan(&image);
        assert_eq!(stored.shared_range(), Some([0.0, 1.0]));
        assert!(stored.planes.iter().all(|plane| plane.max <= 1.0));
    }

    /// The crate's own `apply_gainmap` over the same base and map, as the
    /// floats it writes.
    fn reference(base: &::image::RgbImage, map: &CrateMap, metadata: &GainMapMetadata) -> Vec<f32> {
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

    /// Every pixel of `image` lifted the whole way, four floats a pixel as
    /// the crate writes them, through the one-pixel read the readout uses.
    fn lifted(image: &DecodedImage) -> Vec<f32> {
        let table = whole(image);
        let mut out = Vec::with_capacity((image.width * image.height * 4) as usize);
        for y in 0..image.height {
            for x in 0..image.width {
                out.extend_from_slice(&pixel(image, &table, x, y));
                out.push(1.0);
            }
        }
        out
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

    /// The read here stands in for the crate's own `apply_gainmap`, so it
    /// has to produce the same numbers: the sample's map has a hard edge
    /// down its middle, which the bilinear sampling blends across, and
    /// every pixel is checked, not just the flat halves.
    #[test]
    fn the_lift_agrees_with_the_crates_own() {
        let bytes = sample();
        let container = Container::open(&bytes).expect("a JPEG opens");
        let ours = container
            .gain_mapped(ColorSpace::SRGB)
            .expect("the gain map applies")
            .expect("the sample has a gain map");

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
        let map = CrateMap {
            width: map.width(),
            height: map.height(),
            channels: 1,
            data: map.into_raw(),
        };
        assert_agrees(&lifted(&ours), &reference(&base, &map, metadata));
    }

    /// The same, over a three-channel map whose size is no neat fraction of
    /// the base's so that every row and column lands between map samples;
    /// and with offsets, which the sample above leaves at zero.
    #[test]
    fn a_three_channel_map_with_offsets_agrees_with_the_crates_own() {
        let (width, height) = (100u32, 20u32);
        let base = ::image::RgbImage::from_fn(width, height, |x, y| {
            ::image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
        });
        let (map_width, map_height) = (31u32, 7u32);
        let data: Vec<u8> = (0..map_width * map_height * 3)
            .map(|index| (index * 7 % 256) as u8)
            .collect();
        let map = CrateMap {
            width: map_width,
            height: map_height,
            channels: 3,
            data: data.clone(),
        };
        let mut metadata = metadata();
        metadata.gain_map_min = [-0.5, 0.0, 0.25];
        metadata.gamma = [1.0, 1.5, 2.0];
        metadata.base_offset = [0.015625; 3];
        metadata.alternate_offset = [0.01, 0.02, 0.03];

        let mut ours = DecodedImage::new(
            width,
            height,
            Samples::U8 {
                channels: Channels::Rgb,
                data: base.as_raw().clone(),
            },
            ColorSpace::SRGB,
            AlphaMode::Opaque,
        );
        ours.gain_map = Some(Arc::new(GainMap {
            width: map_width,
            height: map_height,
            channels: 3,
            data: crate::image::gain_map::Data::Codes(data),
            lift: Lift::Iso(metadata.clone()),
        }));
        assert_agrees(&lifted(&ours), &reference(&base, &map, &metadata));
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

    /// One APP segment: its marker, its length and its bytes.
    fn segment(marker: u8, data: &[u8]) -> Vec<u8> {
        let mut segment = vec![0xFF, marker];
        segment.extend_from_slice(&(data.len() as u16 + 2).to_be_bytes());
        segment.extend_from_slice(data);
        segment
    }

    /// A file shaped as a Lightroom export whose gain map has been stripped:
    /// the XMP still names one, the EXIF and Photoshop's APP13 each carry a
    /// thumbnail, and there is no MPF and no second image. The crate's
    /// fallback splits it into two thumbnails, which have to be refused and
    /// the file read as the plain JPEG it is.
    #[test]
    fn a_gain_map_named_but_missing_is_a_plain_jpeg() {
        let photo = jpeg(
            &[GRAY; 16 * 16 * 3],
            16,
            16,
            ::image::ExtendedColorType::Rgb8,
        );
        let thumbnail = jpeg(&[GRAY; 4 * 4 * 3], 4, 4, ::image::ExtendedColorType::Rgb8);

        let mut exif = b"Exif\0\0".to_vec();
        exif.extend_from_slice(&thumbnail);
        let mut photoshop = b"Photoshop 3.0\0".to_vec();
        photoshop.extend_from_slice(&thumbnail);
        let mut xmp = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
        xmp.extend_from_slice(
            br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:hdrgm="http://ns.adobe.com/hdr-gain-map/1.0/" hdrgm:Version="1.0" hdrgm:GainMapMin="0" hdrgm:GainMapMax="1.5" hdrgm:HDRCapacityMin="0" hdrgm:HDRCapacityMax="1.5"/></rdf:RDF></x:xmpmeta>"#,
        );

        let mut bytes = photo[..2].to_vec();
        bytes.extend(segment(0xE1, &exif));
        bytes.extend(segment(0xED, &photoshop));
        bytes.extend(segment(0xE1, &xmp));
        bytes.extend_from_slice(&photo[2..]);

        let container = Container::open(&bytes).expect("a JPEG opens");
        // The crate's split, which is the thing being guarded against.
        assert!(container.decoder.is_ultrahdr());
        assert!(container.decoder.metadata().is_some());
        let base = container.decoder.primary_jpeg().expect("a base is found");
        let scan = header(&bytes).expect("the photograph has a scan").scan;
        assert!(
            base.len() <= scan,
            "the crate's base reaches the photograph's scan"
        );
        assert!(container.decoder.gainmap_jpeg().is_some());

        assert!(container.gain_mapped(ColorSpace::SRGB).unwrap().is_none());
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

    /// The ISO 21496-1 block an iPhone 16 Pro's photograph carried in its
    /// gain map's APP2 once through Apple's export, after the standard's
    /// name: one channel, in the base's color space, the alternate at
    /// 2.354 stops — 5.11 times SDR white, as its maker note works out too.
    const EXPORTED: [u8; 61] = [
        0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0f, 0x42, 0x40, 0x00, 0x23,
        0xe9, 0x6c, 0x00, 0x0f, 0x42, 0x40, 0xff, 0xf5, 0xbd, 0x95, 0x00, 0x0f, 0x42, 0x40, 0x00,
        0x23, 0xe9, 0x6c, 0x00, 0x0f, 0x42, 0x40, 0x00, 0x10, 0xfc, 0xc1, 0x00, 0x0f, 0x42, 0x40,
        0x00, 0x00, 0x00, 0x0a, 0x00, 0x0f, 0x42, 0x40, 0x00, 0x00, 0x00, 0x0a, 0x00, 0x0f, 0x42,
        0x40,
    ];

    /// `jpeg` with `segments` put in after its SOI marker.
    fn with_segments(jpeg: &[u8], segments: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = jpeg[..2].to_vec();
        bytes.extend(segments.concat());
        bytes.extend_from_slice(&jpeg[2..]);
        bytes
    }

    /// The gain map's ISO block, as an APP2 segment.
    fn iso_segment() -> Vec<u8> {
        segment(
            0xE2,
            &[&b"urn:iso:std:iso:ts:21496:-1\0"[..], &EXPORTED].concat(),
        )
    }

    /// The XMP Apple writes into its gain map, as an APP1 segment.
    fn apple_xmp() -> Vec<u8> {
        let packet = format!(
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:apdi="http://ns.apple.com/pixeldatainfo/1.0/" apdi:AuxiliaryImageType="{APPLE_AUXILIARY}"/></rdf:RDF></x:xmpmeta>"#
        );
        segment(
            0xE1,
            &[&b"http://ns.adobe.com/xap/1.0/\0"[..], packet.as_bytes()].concat(),
        )
    }

    /// An EXIF block, as an APP1 segment, holding an Apple maker note whose
    /// tags 33 and 48 are 1.01 and 0.0092, near the photograph's: 2.356
    /// stops above SDR white by Apple's formula.
    fn apple_exif() -> Vec<u8> {
        let note = crate::image::apple_note::tests::note(&[
            (33, 10, &[(101, 100)]),
            (48, 10, &[(92, 10000)]),
        ]);
        let field = exif::Field {
            tag: exif::Tag::MakerNote,
            ifd_num: exif::In::PRIMARY,
            value: exif::Value::Undefined(note, 0),
        };
        let mut writer = exif::experimental::Writer::new();
        writer.push_field(&field);
        let mut tiff = std::io::Cursor::new(Vec::new());
        writer
            .write(&mut tiff, false)
            .expect("the EXIF block writes");
        segment(0xE1, &[&b"Exif\0\0"[..], tiff.get_ref()].concat())
    }

    /// A photograph and a half-size map after it, as Apple's export lays
    /// them out but for the MPF directory, which the crate does without: the
    /// photograph's `photo` segments and the map's `map` segments.
    fn exported(photo: &[Vec<u8>], map: &[Vec<u8>]) -> Vec<u8> {
        let base = jpeg(
            &[GRAY; 16 * 16 * 3],
            16,
            16,
            ::image::ExtendedColorType::Rgb8,
        );
        let gain = jpeg(&[128; 8 * 8], 8, 8, ::image::ExtendedColorType::L8);
        [with_segments(&base, photo), with_segments(&gain, map)].concat()
    }

    fn lift_of(bytes: &[u8]) -> Option<Lift> {
        Container::open(bytes)
            .expect("a JPEG opens")
            .gain_mapped(ColorSpace::SRGB)
            .expect("the gain map reads")
            .map(|image| image.gain_map.expect("a map beside it").lift.clone())
    }

    /// The standard's block in the map is enough on its own, with no `hdrgm`
    /// anywhere in the file.
    #[test]
    fn the_maps_iso_block_is_read_without_hdrgm() {
        let Some(Lift::Iso(metadata)) = lift_of(&exported(&[], &[iso_segment()])) else {
            panic!("the ISO block is read");
        };
        assert!((metadata.alternate_hdr_headroom - 2.353516).abs() < 1e-6);
        assert!((metadata.gain_map_min[0] + 0.672363).abs() < 1e-6);
        assert!(!metadata.backward_direction);
    }

    /// A map that says in its XMP it is Apple's is lifted by the headroom
    /// the photograph's maker note works out.
    #[test]
    fn apples_map_is_lifted_by_the_maker_notes_headroom() {
        let Some(Lift::Apple { headroom }) = lift_of(&exported(&[apple_exif()], &[apple_xmp()]))
        else {
            panic!("Apple's map is read");
        };
        let stops = 3.0 - 70.0 * 0.0092_f32;
        assert!((headroom - stops.exp2()).abs() < 1e-4, "{headroom}");
    }

    /// Where the map carries both, the standard's description is the one
    /// read, as the file Apple's export writes does.
    #[test]
    fn the_iso_block_comes_before_apples() {
        let bytes = exported(&[apple_exif()], &[apple_xmp(), iso_segment()]);
        assert!(matches!(lift_of(&bytes), Some(Lift::Iso(_))));
    }

    /// Apple's map with no maker note to say its headroom has nothing to be
    /// applied by, and the photograph is read as a plain one; and a second
    /// picture that says nothing of being a map is not taken for one.
    #[test]
    fn a_map_nothing_describes_is_not_applied() {
        assert!(lift_of(&exported(&[], &[apple_xmp()])).is_none());
        assert!(lift_of(&exported(&[apple_exif()], &[])).is_none());
    }

    /// The photograph's profile is its own, and the map's — which Apple's
    /// export gives a PQ profile — is not taken for it where the photograph
    /// has none, or spliced into it where it has one.
    #[test]
    fn the_maps_icc_profile_is_not_the_photographs() {
        let chunk =
            |profile: &[u8]| segment(0xE2, &[&b"ICC_PROFILE\0\x01\x01"[..], profile].concat());
        let bytes = exported(&[], &[chunk(b"the map's")]);
        let container = Container::open(&bytes).expect("a JPEG opens");
        assert_eq!(container.icc_profile(), None);

        let bytes = exported(&[chunk(b"the photograph's")], &[chunk(b"the map's")]);
        let container = Container::open(&bytes).expect("a JPEG opens");
        assert_eq!(
            container.icc_profile().as_deref(),
            Some(&b"the photograph's"[..])
        );
    }
}
