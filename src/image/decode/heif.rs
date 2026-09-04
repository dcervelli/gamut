//! HEIF and the codecs that ride in it: HEIC (HEVC), AVIF (AV1), and the rest
//! of what the installed `libheif` plugins can open.
//!
//! There is no usable pure-Rust HEVC decoder, so this one binds to the system
//! `libheif`. What is left to do here is the part `libheif` deliberately does
//! not do: say what the numbers mean. A HEIF file states its color space in
//! CICP codes (H.273) rather than by convention, so unlike PNG or TIFF this
//! decoder is *told* the transfer function and the primaries, and passes them
//! on rather than guessing. That is how a Display P3 phone photograph and a
//! BT.2100 PQ frame end up in the right working space without a flag.
//!
//! Depth is preserved: a 10- or 12-bit file comes back as `U16` rather than
//! being flattened to bytes, and a monochrome file stays one channel all the
//! way to the GPU rather than being tripled into RGB.
//!
//! `libheif` applies the container's own geometric transformations — `irot`,
//! `imir`, `clap` — while decoding, so a rotated phone photograph arrives
//! upright. (JPEG's EXIF orientation still does not; that is a separate tag in
//! a separate decoder.)

use std::io::SeekFrom;
use std::sync::OnceLock;

use anyhow::{Result, anyhow, bail};
use libheif_rs::{
    ColorSpace as HeifColorSpace, HeifContext, ImageHandle, LibHeif, Plane, RgbChroma,
    SecurityLimits, StreamReader,
};

use crate::image::{AlphaMode, Channels, ColorSpace, DecodedImage, Samples};

/// `libheif`'s global initialisation, done once.
///
/// `LibHeif::drop` calls `heif_deinit`, which unloads the codec plugins; doing
/// that per image would rescan the plugin directory every time the user
/// pressed `]`. The guard is deliberately never dropped.
fn lib_heif() -> &'static LibHeif {
    static LIB: OnceLock<LibHeif> = OnceLock::new();
    LIB.get_or_init(LibHeif::new)
}

pub struct Heif;

/// Reads the container's structure — its boxes, not its pixels — which is
/// enough to ask what the image is before deciding to decode it.
fn container<'a>(source: &'a mut dyn super::ReadSeek) -> Result<HeifContext<'a>> {
    let length = source.seek(SeekFrom::End(0))?;
    source.seek(SeekFrom::Start(0))?;

    // Raise the stock limits to the same ceiling the other decoders use, so
    // one format is not quietly stricter than another.
    let mut limits = SecurityLimits::new();
    limits.set_max_image_size_pixels(MAX_PIXELS);
    limits.set_max_memory_block_size(super::MAX_DECODED_BYTES);
    limits.set_max_total_memory(super::MAX_DECODED_BYTES);

    let mut context = HeifContext::new()?;
    context.set_security_limits(&limits)?;
    context.read_reader(Box::new(StreamReader::new(source, length)))?;
    Ok(context)
}

impl super::Decoder for Heif {
    fn name(&self) -> &'static str {
        "heif/heic/avif"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["heic", "heif", "hif", "avif"]
    }

    fn sniff(&self, header: &[u8]) -> bool {
        is_heif(header)
    }

    fn dimensions(&self, source: &mut dyn super::ReadSeek) -> Result<Option<(u32, u32)>> {
        // The handle's size is the size after any rotation the container asks
        // for — `libheif` accounts for that itself, which is why nothing here
        // has to swap anything the way the WebP decoder does.
        let handle = container(source)?.primary_image_handle()?;
        Ok(Some((handle.width(), handle.height())))
    }

    fn decode(
        &self,
        source: &mut dyn super::ReadSeek,
        _overrides: super::Overrides,
    ) -> Result<DecodedImage> {
        // Before the context, so that any external codec plugins are loaded
        // by the time one is asked for.
        let lib = lib_heif();

        let context = container(source)?;
        let handle = context.primary_image_handle()?;
        let (width, height) = (handle.width(), handle.height());
        // Caught here rather than by `DecodedImage::validate`, because the
        // row arithmetic below runs first and would underflow on a zero.
        if width == 0 || height == 0 {
            bail!("HEIF image is {width}x{height}");
        }

        // Bit depth of the luma (or the sole grey) channel: 8 for an ordinary
        // photograph, 10 or 12 for HDR. `libheif` reports -1 for a handle it
        // cannot make sense of, which arrives here as 255.
        let depth = handle.luma_bits_per_pixel();
        if !(1..=16).contains(&depth) {
            bail!("HEIF image reports {depth} bits per channel");
        }
        let wide = depth > 8;

        let monochrome = matches!(
            handle.preferred_decoding_colorspace(),
            Ok(HeifColorSpace::Monochrome)
        );
        let channels = match (monochrome, handle.has_alpha_channel()) {
            (true, false) => Channels::Gray,
            (true, true) => Channels::GrayAlpha,
            (false, false) => Channels::Rgb,
            (false, true) => Channels::Rgba,
        };

        super::check_decoded_size(width, height, channels.count(), if wide { 16 } else { 8 })?;

        let requested = requested_color_space(channels, wide);
        let image = lib.decode(&handle, requested, None)?;
        if (image.width(), image.height()) != (width, height) {
            // A grid or derived image whose handle and pixels disagree would
            // otherwise read past the end of the buffer downstream.
            bail!(
                "HEIF handle says {width}x{height} but the decoded image is {}x{}",
                image.width(),
                image.height()
            );
        }

        let planes = image.planes();
        let samples = if monochrome {
            let grey = planes
                .y
                .ok_or_else(|| anyhow!("monochrome HEIF image decoded without a grey plane"))?;
            match channels {
                Channels::GrayAlpha => {
                    let alpha = planes.a.ok_or_else(|| {
                        anyhow!("HEIF image claims an alpha channel but decoded without one")
                    })?;
                    interleave_planes(&[grey, alpha], width, height)?
                }
                _ => interleave_planes(&[grey], width, height)?,
            }
        } else {
            let interleaved = planes
                .interleaved
                .ok_or_else(|| anyhow!("HEIF image decoded without an interleaved plane"))?;
            pack_interleaved(&interleaved, width, height, channels)?
        };

        Ok(DecodedImage::new(
            width,
            height,
            samples,
            color_space(&handle),
            AlphaMode::of(channels, handle.is_premultiplied_alpha()),
        ))
    }
}

/// The pixel-count ceiling, on the same reasoning as `MAX_DECODED_BYTES`:
/// `max_texture_dimension_2d` is 32768 on current hardware, so anything past
/// 32768 x 32768 could not be displayed even in principle.
const MAX_PIXELS: u64 = 32768 * 32768;

/// Is this the start of an ISO base media file whose brand says still image?
///
/// The brand list lives in `libheif` rather than here, so a format its plugins
/// learn to open is recognised without this file changing. `MayBe` means the
/// header did not reach the end of the brand list, which for a real HEIF is
/// worth handing on: `decode` gives a better message than the registry's
/// "unsupported image format" would.
fn is_heif(header: &[u8]) -> bool {
    use libheif_rs::FileTypeResult::*;
    // The call reads a length prefix out of the buffer, so it needs enough of
    // the `ftyp` box to be there at all.
    if header.len() < 12 {
        return false;
    }
    matches!(libheif_rs::check_file_type(header), Supported | MayBe)
}

/// What to ask `libheif` to hand back.
///
/// Grey stays grey — expanding a one-channel image to RGB would triple what
/// the GPU has to hold for nothing. Anything above 8 bits comes back as
/// little-endian 16-bit words; the `LE` in the name is the buffer's byte
/// order, not the host's, so reading it explicitly keeps this correct on a
/// big-endian machine.
fn requested_color_space(channels: Channels, wide: bool) -> HeifColorSpace {
    match (channels, wide) {
        (Channels::Gray | Channels::GrayAlpha, _) => HeifColorSpace::Monochrome,
        (Channels::Rgb, false) => HeifColorSpace::Rgb(RgbChroma::Rgb),
        (Channels::Rgb, true) => HeifColorSpace::Rgb(RgbChroma::HdrRgbLe),
        (Channels::Rgba, false) => HeifColorSpace::Rgb(RgbChroma::Rgba),
        (Channels::Rgba, true) => HeifColorSpace::Rgb(RgbChroma::HdrRgbaLe),
    }
}

/// Copies one row-strided plane per component into a tightly packed buffer.
///
/// Used for monochrome, where grey and alpha arrive as separate planes and
/// have to be woven together; the interleaved case has its own path because
/// its components are already adjacent.
fn interleave_planes(planes: &[Plane<&[u8]>], width: u32, height: u32) -> Result<Samples> {
    let channels = match planes.len() {
        1 => Channels::Gray,
        _ => Channels::GrayAlpha,
    };
    let count = planes.len();
    let mut layouts = Vec::with_capacity(count);
    for plane in planes {
        layouts.push(Layout::of(plane, width, height, 1)?);
    }
    let wide = layouts.iter().any(|layout| layout.wide);

    if wide {
        let mut data = vec![0u16; width as usize * height as usize * count];
        for (component, (plane, layout)) in planes.iter().zip(&layouts).enumerate() {
            for y in 0..height as usize {
                let row = &plane.data[y * plane.stride..];
                for x in 0..width as usize {
                    data[(y * width as usize + x) * count + component] = layout.sample(row, x);
                }
            }
        }
        Ok(Samples::U16 { channels, data })
    } else {
        let mut data = vec![0u8; width as usize * height as usize * count];
        for (component, plane) in planes.iter().enumerate() {
            for y in 0..height as usize {
                let row = &plane.data[y * plane.stride..];
                for x in 0..width as usize {
                    data[(y * width as usize + x) * count + component] = row[x];
                }
            }
        }
        Ok(Samples::U8 { channels, data })
    }
}

/// Drops the row padding from an already-interleaved plane, and lifts a 10- or
/// 12-bit image up to the full 16-bit range the rest of the program expects.
fn pack_interleaved(
    plane: &Plane<&[u8]>,
    width: u32,
    height: u32,
    channels: Channels,
) -> Result<Samples> {
    let count = channels.count();
    let layout = Layout::of(plane, width, height, count)?;
    let row_components = width as usize * count;

    if layout.wide {
        let mut data = vec![0u16; row_components * height as usize];
        for y in 0..height as usize {
            let row = &plane.data[y * plane.stride..];
            let out = &mut data[y * row_components..][..row_components];
            for (component, slot) in out.iter_mut().enumerate() {
                *slot = layout.sample(row, component);
            }
        }
        Ok(Samples::U16 { channels, data })
    } else {
        let mut data = Vec::with_capacity(row_components * height as usize);
        for y in 0..height as usize {
            data.extend_from_slice(&plane.data[y * plane.stride..][..row_components]);
        }
        Ok(Samples::U8 { channels, data })
    }
}

/// How one decoded plane is laid out in memory, and how to read a sample from
/// it.
///
/// The two bit counts `libheif` reports mean different things and are easy to
/// confuse: `bits_per_pixel` is the range one component occupies — 8, 10, 12 —
/// while `storage_bits_per_pixel` counts the bits a whole pixel takes up, so
/// an interleaved 8-bit RGBA plane reports 32. Reading the second where the
/// first was meant makes every ordinary photograph look like HDR data.
#[derive(Clone, Copy)]
struct Layout {
    /// Two bytes per component rather than one.
    wide: bool,
    scale: Scale,
}

impl Layout {
    fn of(plane: &Plane<&[u8]>, width: u32, height: u32, components: usize) -> Result<Self> {
        if plane.width < width || plane.height < height {
            bail!(
                "HEIF plane is {}x{}, smaller than the {width}x{height} image",
                plane.width,
                plane.height
            );
        }

        // A narrow plane is copied a byte at a time and left unscaled, which
        // is only right if the byte *is* the full range. Nothing HEIF can
        // hold breaks that, but it is what the copy quietly assumes.
        let wide = plane.bits_per_pixel > 8;
        if !wide && plane.bits_per_pixel != 8 {
            bail!("HEIF plane is {} bits in a byte", plane.bits_per_pixel);
        }
        let bytes_per_component = usize::from(wide) + 1;
        let expected_storage = components * bytes_per_component * 8;
        if usize::from(plane.storage_bits_per_pixel) != expected_storage {
            bail!(
                "HEIF plane stores {} bits per pixel, expected {expected_storage} for \
                 {components} components of {} bits",
                plane.storage_bits_per_pixel,
                plane.bits_per_pixel,
            );
        }

        let row_bytes = width as usize * components * bytes_per_component;
        let needed = plane.stride * (height as usize - 1) + row_bytes;
        if plane.stride < row_bytes || plane.data.len() < needed {
            bail!(
                "HEIF plane has a {}-byte stride and {} bytes, needs {row_bytes} per row \
                 and {needed} in total",
                plane.stride,
                plane.data.len(),
            );
        }

        Ok(Self {
            wide,
            scale: Scale::new(plane.bits_per_pixel),
        })
    }

    /// Reads component `index` of a row as a full-range 16-bit value.
    ///
    /// A narrow plane widens on the way through, which is what a monochrome
    /// image with 10-bit grey and an 8-bit alpha plane needs: the two arrive
    /// at different widths and have to leave at the same one.
    ///
    /// The `LE` in `libheif`'s chroma names is the buffer's byte order, not
    /// the host's, so the two bytes are combined explicitly rather than
    /// transmuted.
    fn sample(self, row: &[u8], index: usize) -> u16 {
        let raw = if self.wide {
            u16::from_le_bytes([row[index * 2], row[index * 2 + 1]])
        } else {
            u16::from(row[index])
        };
        self.scale.to_full(raw)
    }
}

/// Lifts a value stored in `bits` of a 16-bit word up to the full range.
///
/// A 10-bit sample is 0..1023 in a `u16`, and `Samples::full_scale` says a
/// `U16` image's white is 65535. Left unscaled, a 10-bit photograph would
/// display as a sixteenth of its intended brightness.
#[derive(Clone, Copy)]
struct Scale {
    max: u32,
}

impl Scale {
    fn new(bits: u8) -> Self {
        let bits = bits.clamp(1, 16);
        Self {
            max: (1u32 << bits) - 1,
        }
    }

    fn to_full(self, value: u16) -> u16 {
        if self.max == u16::MAX as u32 {
            return value;
        }
        let value = u32::from(value).min(self.max);
        ((value * 65535 + self.max / 2) / self.max) as u16
    }
}

/// What the file says its numbers mean.
///
/// HEIF states this outright rather than leaving it to convention, in one of
/// two ways. `nclx` is the usual one and the more precise, being CICP codes
/// that name a transfer function this program models exactly. Some cameras
/// write an ICC profile instead, which says the same thing less directly, and
/// is read only when there is no `nclx` to prefer.
///
/// A file carrying neither falls back to sRGB, which is what an untagged
/// still image means by convention and what every other decoder here assumes.
fn color_space(handle: &ImageHandle) -> ColorSpace {
    if let Some(nclx) = handle.color_profile_nclx() {
        return crate::image::color::cicp::color_space(
            code(nclx.color_primaries() as i32),
            code(nclx.transfer_characteristics() as i32),
        );
    }
    match handle.color_profile_raw() {
        Some(profile) => crate::image::color::icc::color_space(&profile.data, ColorSpace::SRGB),
        None => ColorSpace::SRGB,
    }
}

/// `libheif-rs` gives back C enums whose discriminants are the CICP code
/// points themselves, except for the `Unknown` variant it invents when
/// `libheif` reports a number it does not know. That one has no code point,
/// so it becomes the one H.273 reserves for saying nothing.
fn code(discriminant: i32) -> u8 {
    u8::try_from(discriminant).unwrap_or(crate::image::color::cicp::UNSPECIFIED)
}

#[cfg(test)]
mod tests {
    use super::*;

    use libheif_rs::{ColorPrimaries, TransferCharacteristics};

    use crate::image::{Primaries, Transfer};

    #[test]
    fn ten_bit_white_reaches_full_scale() {
        let scale = Scale::new(10);
        assert_eq!(scale.to_full(0), 0);
        assert_eq!(scale.to_full(1023), u16::MAX);
        // And mid grey stays mid grey rather than drifting a code value.
        assert_eq!(scale.to_full(512), 32800);
    }

    #[test]
    fn sixteen_bit_samples_pass_through_untouched() {
        let scale = Scale::new(16);
        for value in [0, 1, 12345, u16::MAX] {
            assert_eq!(scale.to_full(value), value);
        }
    }

    /// A sample wider than its declared depth is a broken file, not a reason
    /// to hand the upload layer something over full scale.
    #[test]
    fn an_out_of_range_sample_is_clamped() {
        assert_eq!(Scale::new(8).to_full(4000), u16::MAX);
    }

    /// The enum discriminants have to *be* the CICP code points, or the
    /// shared translation is handed the wrong numbers and every tagged HEIF
    /// silently reads as sRGB.
    #[test]
    fn the_enum_discriminants_are_the_cicp_code_points() {
        use ColorPrimaries as P;
        use TransferCharacteristics as T;

        assert_eq!(code(T::IEC_61966_2_1 as i32), 13);
        assert_eq!(code(T::ITU_R_BT_2100_0_PQ as i32), 16);
        assert_eq!(code(T::ITU_R_BT_2100_0_HLG as i32), 18);
        assert_eq!(code(T::Linear as i32), 8);
        assert_eq!(code(P::ITU_R_BT_709_5 as i32), 1);
        assert_eq!(code(P::ITU_R_BT_2020_2_and_2100_0 as i32), 9);
        assert_eq!(code(P::SMPTE_EG_432_1 as i32), 12);

        // And the invented variants, which stand for no code point at all,
        // must land on "unspecified" rather than on a real space.
        assert_eq!(
            crate::image::color::cicp::transfer(code(T::Unknown as i32)),
            Transfer::Srgb
        );
        assert_eq!(
            crate::image::color::cicp::primaries(code(P::Unknown as i32)),
            Primaries::Bt709
        );
    }

    /// Grey must not be widened to RGB, and a deep file must not be asked for
    /// as bytes — either would be a silent loss on the way to the GPU.
    #[test]
    fn the_requested_layout_preserves_channels_and_depth() {
        assert_eq!(
            requested_color_space(Channels::Gray, false),
            HeifColorSpace::Monochrome
        );
        assert_eq!(
            requested_color_space(Channels::Gray, true),
            HeifColorSpace::Monochrome
        );
        assert_eq!(
            requested_color_space(Channels::Rgb, false),
            HeifColorSpace::Rgb(RgbChroma::Rgb)
        );
        assert_eq!(
            requested_color_space(Channels::Rgba, true),
            HeifColorSpace::Rgb(RgbChroma::HdrRgbaLe)
        );
    }

    /// The `ftyp` brand, not the extension, is what makes a file ours — and a
    /// file that merely starts with a box header is not.
    #[test]
    fn brands_are_recognised_and_other_containers_are_not() {
        let heic = b"\x00\x00\x00\x18ftypheic\x00\x00\x00\x00mif1heic";
        assert!(is_heif(heic));
        let avif = b"\x00\x00\x00\x1cftypavif\x00\x00\x00\x00avifmif1miaf";
        assert!(is_heif(avif));

        assert!(!is_heif(
            b"\x00\x00\x00\x18ftypmp42\x00\x00\x00\x00mp42isom"
        ));
        assert!(!is_heif(b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR"));
        assert!(!is_heif(b"\xff\xd8\xff\xe0\x00\x10JFIF\x00\x01"));
        // Too short to hold a brand at all.
        assert!(!is_heif(b"\x00\x00\x00\x18ftyp"));
    }
}
