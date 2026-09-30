//! Choosing a texture format for a decoded image, and getting the bytes there.
//!
//! The invariant this module exists to maintain:
//!
//! > **A sampled texel is always linear in the working space** — either
//! > because the data was linear already, or because the format's hardware
//! > decode produces it, or because we linearized on the way in.
//!
//! It matters because a format's own decode happens as part of reading a
//! texel, before anything is weighted against anything else, while a shader
//! decode necessarily happens after. Decoding in the shader would mean
//! resampling in encoded space, which is the classic gamma-incorrect
//! downscale — and this viewer minifies constantly, since fit is the default.
//! So the transfer function is resolved here, once per image, never per frame,
//! and every weighted sum in `image_layer` is over linear light.

use std::sync::{Arc, Mutex, PoisonError};

use half::f16;

use crate::image::{Channels, DecodedImage, Samples, Transfer};

/// How many components a texture carries. Never three: no graphics API has
/// a three-component sampled texture format, so RGB is stored as RGBA with
/// an opaque alpha.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Components {
    One,
    Two,
    Four,
}

impl Components {
    fn of(channels: Channels) -> Self {
        match channels {
            Channels::Gray => Components::One,
            Channels::GrayAlpha => Components::Two,
            Channels::Rgb | Channels::Rgba => Components::Four,
        }
    }

    pub fn count(self) -> usize {
        match self {
            Components::One => 1,
            Components::Two => 2,
            Components::Four => 4,
        }
    }
}

/// What each component of a texture is stored as.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Storage {
    /// A byte the hardware decodes from the sRGB curve as it reads it. Four
    /// components only: there is no one- or two-channel sRGB format.
    Srgb8,
    /// A byte, read as the fraction it is of 255.
    Unorm8,
    /// Two bytes, read as the fraction they are of 65535.
    Unorm16,
    Float16,
    Float32,
}

impl Storage {
    fn bytes(self) -> usize {
        match self {
            Storage::Srgb8 | Storage::Unorm8 => 1,
            Storage::Unorm16 | Storage::Float16 => 2,
            Storage::Float32 => 4,
        }
    }
}

/// A texture format as this tree chooses one: how many components, each
/// stored as what. The one table from a layout to a `wgpu::TextureFormat`,
/// so that the picture, its gain map and the coarse chain reduced from
/// either all go through the same door — a format added here is added for
/// all three, and one left out of the table does not compile.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Layout {
    pub components: Components,
    pub storage: Storage,
}

impl Layout {
    pub fn format(self) -> wgpu::TextureFormat {
        use Components::{Four, One, Two};
        use wgpu::TextureFormat as F;
        match (self.storage, self.components) {
            (Storage::Srgb8, Four) => F::Rgba8UnormSrgb,
            (Storage::Srgb8, One | Two) => {
                unreachable!(
                    "there is no one- or two-channel sRGB format, and plan() asks for none"
                )
            }
            (Storage::Unorm8, One) => F::R8Unorm,
            (Storage::Unorm8, Two) => F::Rg8Unorm,
            (Storage::Unorm8, Four) => F::Rgba8Unorm,
            (Storage::Unorm16, One) => F::R16Unorm,
            (Storage::Unorm16, Two) => F::Rg16Unorm,
            (Storage::Unorm16, Four) => F::Rgba16Unorm,
            (Storage::Float16, One) => F::R16Float,
            (Storage::Float16, Two) => F::Rg16Float,
            (Storage::Float16, Four) => F::Rgba16Float,
            (Storage::Float32, One) => F::R32Float,
            (Storage::Float32, Two) => F::Rg32Float,
            (Storage::Float32, Four) => F::Rgba32Float,
        }
    }

    /// The format a coarse chain reduced from a texture of this layout is
    /// stored in — see `render::reduce`.
    ///
    /// Float, so that a level holds linear light with no transfer function
    /// to think about, and premultiplied color without an 8-bit floor under
    /// it. Half floats everywhere except under a 32-bit float source, where
    /// the range and the low bits are the point of the file.
    pub fn level_format(self) -> wgpu::TextureFormat {
        let storage = match self.storage {
            Storage::Float32 => Storage::Float32,
            Storage::Srgb8 | Storage::Unorm8 | Storage::Unorm16 | Storage::Float16 => {
                Storage::Float16
            }
        };
        Layout {
            components: self.components,
            storage,
        }
        .format()
    }

    fn bytes_per_texel(self) -> usize {
        self.components.count() * self.storage.bytes()
    }
}

/// The texel bytes for an upload.
///
/// Borrowed whenever the decoded samples are already in the layout the
/// texture wants, which is the common case for measurement rasters: a
/// single-band float DEM goes to the GPU without a second copy of what may be
/// hundreds of megabytes.
pub enum Pixels<'a> {
    Borrowed(&'a [u8]),
    U8(Vec<u8>),
    U16(Vec<u16>),
    F16(Vec<f16>),
    F32(Vec<f32>),
}

impl Pixels<'_> {
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Pixels::Borrowed(bytes) => bytes,
            Pixels::U8(values) => values,
            Pixels::U16(values) => bytemuck::cast_slice(values),
            Pixels::F16(values) => bytemuck::cast_slice(values),
            Pixels::F32(values) => bytemuck::cast_slice(values),
        }
    }
}

/// A texture layout and the bytes to fill it with.
pub struct Plan<'a> {
    pub layout: Layout,
    pub pixels: Pixels<'a>,
    pub bytes_per_row: u32,
    /// Set when we had to give up precision to get a filterable format, so
    /// the caller can say so.
    pub reduced: Option<Reduced>,
}

impl<'a> Plan<'a> {
    /// `pixels` laid out as `layout`, `width` texels to the row.
    fn new(layout: Layout, pixels: Pixels<'a>, width: u32, reduced: Option<Reduced>) -> Self {
        Self {
            layout,
            pixels,
            bytes_per_row: width * layout.bytes_per_texel() as u32,
            reduced,
        }
    }

    /// 8-bit samples stored as they are, a byte a component, RGB widened to
    /// RGBA: what the picture takes when its values are linear already, and
    /// what a gain map takes, whose values are the shader's to interpret.
    pub fn unorm8(data: &'a [u8], channels: Channels, width: u32) -> Self {
        let layout = Layout {
            components: Components::of(channels),
            storage: Storage::Unorm8,
        };
        let pixels = expand_u8(data, channels, layout.components.count(), u8::MAX);
        Self::new(layout, pixels, width, None)
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        self.layout.format()
    }
}

/// Precision a picture lost on its way to the device, which had no format
/// that would hold it and still be filtered. Both land in half floats, whose
/// 11-bit mantissa is a real loss against what the file holds.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Reduced {
    /// 16-bit data with no curve to take off, on a device with no 16-bit
    /// integer formats.
    NoNorm16,
    /// 32-bit floats, on a device that cannot filter a 32-bit texture.
    NoFloat32Filter,
}

impl Reduced {
    /// Why, in words: the panel's row and the toast both say this.
    pub fn reason(self) -> &'static str {
        match self {
            Reduced::NoNorm16 => "this GPU has no 16-bit integer textures",
            Reduced::NoFloat32Filter => "this GPU can't filter 32-bit float textures",
        }
    }
}

/// What the device can do, gathered once at start-up.
#[derive(Clone, Copy, Debug)]
pub struct Capabilities {
    /// `R16Unorm` and friends. Native-only; without it 16-bit data has to
    /// become float.
    pub norm16: bool,
    /// Whether a linear sampler may be used with 32-bit float textures.
    /// Without it, float images would be stuck on nearest-neighbor.
    pub float32_filterable: bool,
}

impl Capabilities {
    pub fn from_adapter(adapter: &wgpu::Adapter) -> Self {
        let features = adapter.features();
        Self {
            norm16: features.contains(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM),
            float32_filterable: features.contains(wgpu::Features::FLOAT32_FILTERABLE),
        }
    }

    /// The subset of the above that we should actually request of the device.
    pub fn required_features(&self) -> wgpu::Features {
        let mut features = wgpu::Features::empty();
        if self.norm16 {
            features |= wgpu::Features::TEXTURE_FORMAT_16BIT_NORM;
        }
        if self.float32_filterable {
            features |= wgpu::Features::FLOAT32_FILTERABLE;
        }
        features
    }
}

pub fn plan(image: &DecodedImage, capabilities: Capabilities) -> Plan<'_> {
    let channels = image.samples.channels();
    let components = Components::of(channels);
    let layout = |storage| Layout {
        components,
        storage,
    };
    let transfer = image.color.transfer;
    let width = image.width;

    // The one case where hardware does the transfer decode for us, and the
    // only case where 8-bit storage survives to the GPU.
    let hardware_srgb = transfer == Transfer::Srgb
        && matches!(image.samples, Samples::U8 { .. })
        && matches!(channels, Channels::Rgb | Channels::Rgba);

    match &image.samples {
        Samples::U8 { data, .. } if hardware_srgb => Plan::new(
            layout(Storage::Srgb8),
            expand_u8(data, channels, components.count(), u8::MAX),
            width,
            None,
        ),

        Samples::U8 { data, .. } if transfer.is_linear() => Plan::unorm8(data, channels, width),

        // Gray with a curve on it. There is no `R8UnormSrgb`, so the choice is
        // a 4x expansion to `Rgba8UnormSrgb` or a 2x one to half floats;
        // half floats also keep the single-channel path uniform.
        Samples::U8 { data, .. } => {
            let lut = lut(transfer, u8::MAX as u32);
            let values = map_to_f16(data, channels, components.count(), &lut, u8::MAX as f32);
            Plan::new(layout(Storage::Float16), Pixels::F16(values), width, None)
        }

        Samples::U16 { data, .. } if transfer.is_linear() && capabilities.norm16 => Plan::new(
            layout(Storage::Unorm16),
            expand_u16(data, channels, components.count(), u16::MAX),
            width,
            None,
        ),

        // Either the curve has to come off, or the device lacks 16-bit norm
        // formats. Half floats answer both. Their 11-bit mantissa is a real
        // loss against 16-bit integers, but only for linear data — and only
        // where the device forced it.
        Samples::U16 { data, .. } => {
            let lut = lut(transfer, u16::MAX as u32);
            let values = map_to_f16(data, channels, components.count(), &lut, u16::MAX as f32);
            Plan::new(
                layout(Storage::Float16),
                Pixels::F16(values),
                width,
                (transfer.is_linear() && !capabilities.norm16).then_some(Reduced::NoNorm16),
            )
        }

        Samples::F32 { data, .. } if capabilities.float32_filterable => Plan::new(
            layout(Storage::Float32),
            map_f32(data, channels, components.count(), transfer),
            width,
            None,
        ),

        // Without `FLOAT32_FILTERABLE` a 32-bit float texture can only be
        // point-sampled, which would alias badly at fit-to-window scales.
        // Half floats stay filterable.
        Samples::F32 { data, .. } => {
            let widened = map_f32(data, channels, components.count(), transfer);
            let values: Vec<f16> = bytemuck::cast_slice::<u8, f32>(widened.as_bytes())
                .iter()
                .map(|value| f16::from_f32(*value))
                .collect();
            Plan::new(
                layout(Storage::Float16),
                Pixels::F16(values),
                width,
                Some(Reduced::NoFloat32Filter),
            )
        }
    }
}

/// How many linearizing tables are kept at once. Four covers the curves a
/// session sees — sRGB at both depths, and a raw's or a TIFF's gamma or two
/// — at 128 KiB for a 16-bit table.
const KEPT_LUTS: usize = 4;

/// The linear light of every value an integer sample can take, `0..=full_scale`,
/// through `transfer`, as the half floats a linearized picture is uploaded
/// as. Kept once made: the 16-bit table is a power per entry, sixty-five
/// thousand of them, and every 16-bit picture with a curve on it would pay
/// it again. A small cache rather than a table per curve, since
/// `Transfer::Gamma` carries its exponent and so has no fixed number of
/// values; the oldest goes when it is full.
fn lut(transfer: Transfer, full_scale: u32) -> Arc<[f16]> {
    /// A table kept: the curve and the full scale it was made for.
    type Kept = (Transfer, u32, Arc<[f16]>);
    static LUTS: Mutex<Vec<Kept>> = Mutex::new(Vec::new());
    let mut luts = LUTS.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((_, _, lut)) = luts
        .iter()
        .find(|(kept, scale, _)| *kept == transfer && *scale == full_scale)
    {
        return Arc::clone(lut);
    }
    let lut: Arc<[f16]> = (0..=full_scale)
        .map(|value| f16::from_f32(transfer.to_linear(value as f32 / full_scale as f32)))
        .collect();
    if luts.len() >= KEPT_LUTS {
        luts.remove(0);
    }
    luts.push((transfer, full_scale, Arc::clone(&lut)));
    lut
}

/// Pixels below which a repack is not worth dividing: handing bands to the
/// pool costs more than a picture this small takes to walk.
const PARALLEL_FROM: usize = 1 << 16;

/// The texels of an upload, `components` to a pixel, built from `source`
/// samples to a pixel of `data`: `pixel` writes each from its samples, and
/// any component past the source's — an alpha the texture has and the file
/// does not — reads as `fill`.
///
/// Divided by pixels between rayon's threads. A 134-megapixel RGB file is
/// 400 MB widened to 540, and one thread took a quarter of a second over it
/// — most of what the upload cost, and more than the GPU's own copy.
fn repack<S: Sync, T: bytemuck::Zeroable + Copy + Send + Sync>(
    data: &[S],
    source: usize,
    components: usize,
    fill: T,
    pixel: impl Fn(&[S], &mut [T]) + Sync,
) -> Vec<T> {
    let pixels = data.len() / source;
    // Zeroed rather than filled: a zeroed allocation is pages the kernel
    // hands out on first touch, so the memory is first written by the
    // threads below, in parallel, rather than by a fill here on one.
    let mut out = vec![T::zeroed(); pixels * components];
    let bands = if pixels < PARALLEL_FROM {
        1
    } else {
        rayon_core::current_num_threads().clamp(1, pixels)
    };
    let per_band = pixels.div_ceil(bands);

    let walk = |index: usize, band: &mut [T]| {
        let from = index * per_band * source;
        for (samples, texel) in data[from..]
            .chunks_exact(source)
            .zip(band.chunks_exact_mut(components))
        {
            texel[source..].fill(fill);
            pixel(samples, &mut texel[..source]);
        }
    };
    if bands == 1 {
        walk(0, &mut out);
    } else {
        rayon_core::scope(|scope| {
            for (index, band) in out.chunks_mut(per_band * components).enumerate() {
                let walk = &walk;
                scope.spawn(move |_| walk(index, band));
            }
        });
    }
    out
}

/// Widens each pixel to `components`, filling any added alpha with `opaque`.
/// Only ever grows 3 to 4; 1 and 2 stay as they are, and `None` says the
/// source can be uploaded as it is.
fn expand<T: bytemuck::Zeroable + Copy + Send + Sync>(
    data: &[T],
    channels: Channels,
    components: usize,
    opaque: T,
) -> Option<Vec<T>> {
    let source = channels.count();
    if source == components {
        return None;
    }
    Some(repack(
        data,
        source,
        components,
        opaque,
        |samples, texel| {
            texel.copy_from_slice(samples);
        },
    ))
}

fn expand_u8(data: &[u8], channels: Channels, components: usize, opaque: u8) -> Pixels<'_> {
    match expand(data, channels, components, opaque) {
        Some(out) => Pixels::U8(out),
        None => Pixels::Borrowed(data),
    }
}

fn expand_u16(data: &[u16], channels: Channels, components: usize, opaque: u16) -> Pixels<'_> {
    match expand(data, channels, components, opaque) {
        Some(out) => Pixels::U16(out),
        None => Pixels::Borrowed(bytemuck::cast_slice(data)),
    }
}

/// Linearizes integer samples through `lut`, widening to `components`.
/// Alpha is a coverage fraction, never a light measurement, so it is scaled
/// by `full_scale` and never put through the curve.
fn map_to_f16<T: Copy + Into<u32> + Sync>(
    data: &[T],
    channels: Channels,
    components: usize,
    lut: &[f16],
    full_scale: f32,
) -> Vec<f16> {
    let source = channels.count();
    let alpha = channels.alpha_index();
    repack(data, source, components, f16::ONE, |samples, texel| {
        for (index, (raw, slot)) in samples.iter().zip(texel).enumerate() {
            let raw: u32 = (*raw).into();
            *slot = if Some(index) == alpha {
                f16::from_f32(raw as f32 / full_scale)
            } else {
                lut[raw as usize]
            };
        }
    })
}

fn map_f32(data: &[f32], channels: Channels, components: usize, transfer: Transfer) -> Pixels<'_> {
    let source = channels.count();
    let alpha = channels.alpha_index();
    if source == components && transfer.is_linear() {
        return Pixels::Borrowed(bytemuck::cast_slice(data));
    }
    let out = repack(data, source, components, 1.0f32, |samples, texel| {
        for (index, (raw, slot)) in samples.iter().zip(texel).enumerate() {
            *slot = if Some(index) == alpha || transfer.is_linear() {
                *raw
            } else {
                transfer.to_linear(*raw)
            };
        }
    });
    Pixels::F32(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::{AlphaMode, ColorSpace, DecodedImage, Primaries, Referred, Samples};

    const FULL: Capabilities = Capabilities {
        norm16: true,
        float32_filterable: true,
    };
    const BARE: Capabilities = Capabilities {
        norm16: false,
        float32_filterable: false,
    };

    fn image(samples: Samples, transfer: Transfer) -> DecodedImage {
        let channels = samples.channels();
        let pixels = samples.len() / channels.count();
        DecodedImage {
            width: pixels as u32,
            height: 1,
            samples,
            color: ColorSpace {
                transfer,
                primaries: Primaries::Bt709,
            },
            alpha: AlphaMode::Opaque,
            referred: Referred::of(transfer),
            exposure: None,
            nodata: None,
            gain_map: None,
            depth: None,
        }
    }

    /// The one case where the hardware can do the transfer decode, and so the
    /// only one where filtering happens on properly linearized texels for
    /// free. Losing this would silently reintroduce gamma-incorrect scaling.
    #[test]
    fn eight_bit_srgb_color_keeps_the_hardware_srgb_format() {
        let decoded = image(
            Samples::U8 {
                channels: Channels::Rgb,
                data: vec![10, 20, 30, 40, 50, 60],
            },
            Transfer::Srgb,
        );
        let plan = plan(&decoded, FULL);
        assert_eq!(plan.format(), wgpu::TextureFormat::Rgba8UnormSrgb);
        // Three channels became four, with an opaque alpha.
        assert_eq!(plan.pixels.as_bytes().len(), 8);
        assert_eq!(plan.pixels.as_bytes()[3], u8::MAX);
        assert_eq!(plan.pixels.as_bytes()[4..7], [40, 50, 60]);
    }

    /// There is no `R8UnormSrgb`, so gray with a curve on it has to be
    /// linearized on the way in rather than left for the shader.
    #[test]
    fn eight_bit_srgb_gray_is_linearized_to_half_float() {
        let decoded = image(
            Samples::U8 {
                channels: Channels::Gray,
                data: vec![0, 128, 255],
            },
            Transfer::Srgb,
        );
        let plan = plan(&decoded, FULL);
        assert_eq!(plan.format(), wgpu::TextureFormat::R16Float);

        let values: &[f16] = bytemuck::cast_slice(plan.pixels.as_bytes());
        assert_eq!(values[0].to_f32(), 0.0);
        assert!((values[2].to_f32() - 1.0).abs() < 1e-3);
        // Mid sRGB gray is about 21% of the light, not 50%.
        assert!((values[1].to_f32() - 0.2158).abs() < 0.01, "{values:?}");
    }

    #[test]
    fn linear_sixteen_bit_stays_sixteen_bit_when_the_device_allows() {
        let measurement = image(
            Samples::U16 {
                channels: Channels::Gray,
                data: vec![0, 2048, 4095],
            },
            Transfer::Linear,
        );
        assert_eq!(
            plan(&measurement, FULL).format(),
            wgpu::TextureFormat::R16Unorm
        );

        // Without the feature it has to become float, and says so.
        let fallback = plan(&measurement, BARE);
        assert_eq!(fallback.format(), wgpu::TextureFormat::R16Float);
        assert_eq!(fallback.reduced, Some(Reduced::NoNorm16));
    }

    #[test]
    fn float_falls_back_to_half_when_it_cannot_be_filtered() {
        let scene = image(
            Samples::F32 {
                channels: Channels::Rgb,
                data: vec![0.5, 1.0, 4.0],
            },
            Transfer::Linear,
        );
        assert_eq!(
            plan(&scene, FULL).format(),
            wgpu::TextureFormat::Rgba32Float
        );

        let fallback = plan(&scene, BARE);
        assert_eq!(fallback.format(), wgpu::TextureFormat::Rgba16Float);
        assert_eq!(fallback.reduced, Some(Reduced::NoFloat32Filter));
    }

    /// Alpha is coverage, not light: running it through a transfer function
    /// would make edges wrong.
    #[test]
    fn alpha_is_never_linearized() {
        let decoded = image(
            Samples::U8 {
                channels: Channels::GrayAlpha,
                data: vec![128, 128],
            },
            Transfer::Srgb,
        );
        let plan = plan(&decoded, FULL);
        assert_eq!(plan.format(), wgpu::TextureFormat::Rg16Float);

        let values: &[f16] = bytemuck::cast_slice(plan.pixels.as_bytes());
        assert!(
            (values[0].to_f32() - 0.2158).abs() < 0.01,
            "color is decoded"
        );
        assert!(
            (values[1].to_f32() - 128.0 / 255.0).abs() < 1e-3,
            "alpha is untouched"
        );
    }

    /// A gain map is stored a byte a component like a linear 8-bit picture,
    /// its three channels widened to four, its one left alone.
    #[test]
    fn a_gain_map_is_stored_a_byte_a_component() {
        let three = Plan::unorm8(&[1, 2, 3, 4, 5, 6], Channels::Rgb, 2);
        assert_eq!(three.format(), wgpu::TextureFormat::Rgba8Unorm);
        assert_eq!(three.bytes_per_row, 8);
        assert_eq!(three.pixels.as_bytes(), [1, 2, 3, 255, 4, 5, 6, 255]);

        let one = Plan::unorm8(&[7, 8], Channels::Gray, 2);
        assert_eq!(one.format(), wgpu::TextureFormat::R8Unorm);
        assert_eq!(one.bytes_per_row, 2);
        assert!(matches!(one.pixels, Pixels::Borrowed([7, 8])));
    }

    /// The chain is stored in half floats under everything but a 32-bit
    /// float source, with the source's own component count: the table the
    /// reducer used to keep for itself, held here to what it said.
    #[test]
    fn a_coarse_level_keeps_the_components_and_floats_the_storage() {
        use wgpu::TextureFormat as F;
        let level = |components, storage| {
            Layout {
                components,
                storage,
            }
            .level_format()
        };
        for storage in [Storage::Unorm8, Storage::Unorm16, Storage::Float16] {
            assert_eq!(level(Components::One, storage), F::R16Float);
            assert_eq!(level(Components::Two, storage), F::Rg16Float);
            assert_eq!(level(Components::Four, storage), F::Rgba16Float);
        }
        assert_eq!(level(Components::Four, Storage::Srgb8), F::Rgba16Float);
        assert_eq!(level(Components::One, Storage::Float32), F::R32Float);
        assert_eq!(level(Components::Two, Storage::Float32), F::Rg32Float);
        assert_eq!(level(Components::Four, Storage::Float32), F::Rgba32Float);
    }

    /// The same curve at the same depth is one table, made once.
    #[test]
    fn a_linearizing_table_is_made_once() {
        let first = lut(Transfer::Gamma(2.2), u16::MAX as u32);
        let again = lut(Transfer::Gamma(2.2), u16::MAX as u32);
        assert!(Arc::ptr_eq(&first, &again));
        assert_eq!(first.len(), 65536);
        assert_eq!(first[0], f16::ZERO);
        assert_eq!(first[65535], f16::ONE);
        let other = lut(Transfer::Gamma(1.8), u16::MAX as u32);
        assert!(!Arc::ptr_eq(&first, &other));
    }

    /// Dividing a repack between threads must not move a sample: every
    /// pixel of a picture large enough to be divided lands where a plain
    /// walk would put it, the added alpha included, and the last band —
    /// which is short — with the rest.
    #[test]
    fn a_divided_repack_agrees_with_a_plain_one() {
        let pixels = PARALLEL_FROM + 1234;
        let data: Vec<u8> = (0..pixels * 3).map(|i| (i % 251) as u8).collect();
        let widened = repack(&data, 3, 4, 0xEE, |samples, texel: &mut [u8]| {
            texel.copy_from_slice(samples);
        });
        assert_eq!(widened.len(), pixels * 4);
        let (samples, _) = data.as_chunks::<3>();
        let (texels, _) = widened.as_chunks::<4>();
        for (pixel, (samples, texel)) in samples.iter().zip(texels).enumerate() {
            assert_eq!(&texel[..3], samples, "pixel {pixel}");
            assert_eq!(texel[3], 0xEE, "pixel {pixel}");
        }

        // The same walk with nothing added, which the helper takes as a
        // plain copy rather than a fill of nothing.
        let same = repack(&data, 3, 3, 0u8, |samples, texel: &mut [u8]| {
            texel.copy_from_slice(samples);
        });
        assert_eq!(same, data);
    }

    /// Whatever the path, the buffer must match what the texture expects, or
    /// `write_texture` reads past the end of it.
    #[test]
    fn every_plan_produces_a_consistent_buffer() {
        let cases = [
            (Channels::Gray, Transfer::Linear),
            (Channels::Gray, Transfer::Srgb),
            (Channels::GrayAlpha, Transfer::Linear),
            (Channels::Rgb, Transfer::Srgb),
            (Channels::Rgba, Transfer::Linear),
        ];
        for (channels, transfer) in cases {
            for capabilities in [FULL, BARE] {
                let pixels = 6;
                let decoded = DecodedImage {
                    width: 3,
                    height: 2,
                    samples: Samples::U16 {
                        channels,
                        data: vec![1234; pixels * channels.count()],
                    },
                    color: ColorSpace {
                        transfer,
                        primaries: Primaries::Bt709,
                    },
                    alpha: AlphaMode::Opaque,
                    referred: Referred::of(transfer),
                    exposure: None,
                    nodata: None,
                    gain_map: None,
                    depth: None,
                };
                let plan = plan(&decoded, capabilities);
                assert_eq!(
                    plan.pixels.as_bytes().len(),
                    plan.bytes_per_row as usize * 2,
                    "{channels:?} {transfer:?} {capabilities:?}"
                );
            }
        }
    }
}
