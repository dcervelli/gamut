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

use half::f16;

use crate::image::{Channels, DecodedImage, Samples, Transfer};

/// How many components the texture carries. Never three: no graphics API has
/// a three-component sampled texture format.
fn components_for(channels: Channels) -> usize {
    match channels {
        Channels::Gray => 1,
        Channels::GrayAlpha => 2,
        Channels::Rgb | Channels::Rgba => 4,
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

/// A texture format and the bytes to fill it with.
pub struct Plan<'a> {
    pub format: wgpu::TextureFormat,
    pub pixels: Pixels<'a>,
    pub bytes_per_row: u32,
    /// Set when we had to give up precision to get a filterable format, so
    /// the caller can say so.
    pub precision_note: Option<&'static str>,
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
    let components = components_for(channels);
    let transfer = image.color.transfer;
    let width = image.width as usize;

    // The one case where hardware does the transfer decode for us, and the
    // only case where 8-bit storage survives to the GPU.
    let hardware_srgb = transfer == Transfer::Srgb
        && matches!(image.samples, Samples::U8 { .. })
        && matches!(channels, Channels::Rgb | Channels::Rgba);

    match &image.samples {
        Samples::U8 { data, .. } if hardware_srgb => Plan {
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            pixels: expand_u8(data, channels, components, u8::MAX),
            bytes_per_row: (width * components) as u32,
            precision_note: None,
        },

        Samples::U8 { data, .. } if transfer.is_linear() => Plan {
            format: match components {
                1 => wgpu::TextureFormat::R8Unorm,
                2 => wgpu::TextureFormat::Rg8Unorm,
                _ => wgpu::TextureFormat::Rgba8Unorm,
            },
            pixels: expand_u8(data, channels, components, u8::MAX),
            bytes_per_row: (width * components) as u32,
            precision_note: None,
        },

        // Gray with a curve on it. There is no `R8UnormSrgb`, so the choice is
        // a 4x expansion to `Rgba8UnormSrgb` or a 2x one to half floats;
        // half floats also keep the single-channel path uniform.
        Samples::U8 { data, .. } => {
            let lut: Vec<f16> = (0..=u8::MAX)
                .map(|v| f16::from_f32(transfer.to_linear(v as f32 / u8::MAX as f32)))
                .collect();
            let values = map_to_f16(data, channels, components, &lut, u8::MAX as f32);
            Plan {
                format: float16_format(components),
                pixels: Pixels::F16(values),
                bytes_per_row: (width * components * 2) as u32,
                precision_note: None,
            }
        }

        Samples::U16 { data, .. } if transfer.is_linear() && capabilities.norm16 => Plan {
            format: match components {
                1 => wgpu::TextureFormat::R16Unorm,
                2 => wgpu::TextureFormat::Rg16Unorm,
                _ => wgpu::TextureFormat::Rgba16Unorm,
            },
            pixels: expand_u16(data, channels, components, u16::MAX),
            bytes_per_row: (width * components * 2) as u32,
            precision_note: None,
        },

        // Either the curve has to come off, or the device lacks 16-bit norm
        // formats. Half floats answer both. Their 11-bit mantissa is a real
        // loss against 16-bit integers, but only for linear data — and only
        // where the device forced it.
        Samples::U16 { data, .. } => {
            let lut: Vec<f16> = (0..=u16::MAX)
                .map(|v| f16::from_f32(transfer.to_linear(v as f32 / u16::MAX as f32)))
                .collect();
            let values = map_to_f16(data, channels, components, &lut, u16::MAX as f32);
            Plan {
                format: float16_format(components),
                pixels: Pixels::F16(values),
                bytes_per_row: (width * components * 2) as u32,
                precision_note: (transfer.is_linear() && !capabilities.norm16).then_some(
                    "16-bit data stored as half float: this GPU has no 16-bit norm formats",
                ),
            }
        }

        Samples::F32 { data, .. } if capabilities.float32_filterable => Plan {
            format: match components {
                1 => wgpu::TextureFormat::R32Float,
                2 => wgpu::TextureFormat::Rg32Float,
                _ => wgpu::TextureFormat::Rgba32Float,
            },
            pixels: map_f32(data, channels, components, transfer),
            bytes_per_row: (width * components * 4) as u32,
            precision_note: None,
        },

        // Without `FLOAT32_FILTERABLE` a 32-bit float texture can only be
        // point-sampled, which would alias badly at fit-to-window scales.
        // Half floats stay filterable.
        Samples::F32 { data, .. } => {
            let widened = map_f32(data, channels, components, transfer);
            let values: Vec<f16> = bytemuck::cast_slice::<u8, f32>(widened.as_bytes())
                .iter()
                .map(|value| f16::from_f32(*value))
                .collect();
            Plan {
                format: float16_format(components),
                pixels: Pixels::F16(values),
                bytes_per_row: (width * components * 2) as u32,
                precision_note: Some(
                    "float data stored as half float: this GPU cannot filter 32-bit textures",
                ),
            }
        }
    }
}

fn float16_format(components: usize) -> wgpu::TextureFormat {
    match components {
        1 => wgpu::TextureFormat::R16Float,
        2 => wgpu::TextureFormat::Rg16Float,
        _ => wgpu::TextureFormat::Rgba16Float,
    }
}

/// Widens each pixel to `components`, filling any added alpha with `opaque`.
/// Only ever grows 3 to 4; 1 and 2 stay as they are, and `None` says the
/// source can be uploaded as it is.
fn expand<T: Copy>(data: &[T], channels: Channels, components: usize, opaque: T) -> Option<Vec<T>> {
    let source = channels.count();
    if source == components {
        return None;
    }
    let mut out = vec![opaque; data.len() / source * components];
    for (pixel, chunk) in data.chunks_exact(source).enumerate() {
        out[pixel * components..pixel * components + source].copy_from_slice(chunk);
    }
    Some(out)
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
fn map_to_f16<T: Copy + Into<u32>>(
    data: &[T],
    channels: Channels,
    components: usize,
    lut: &[f16],
    full_scale: f32,
) -> Vec<f16> {
    let source = channels.count();
    let alpha = channels.alpha_index();
    let mut out = vec![f16::ONE; data.len() / source * components];
    for (pixel, chunk) in data.chunks_exact(source).enumerate() {
        for (index, raw) in chunk.iter().enumerate() {
            let raw: u32 = (*raw).into();
            out[pixel * components + index] = if Some(index) == alpha {
                f16::from_f32(raw as f32 / full_scale)
            } else {
                lut[raw as usize]
            };
        }
    }
    out
}

fn map_f32(data: &[f32], channels: Channels, components: usize, transfer: Transfer) -> Pixels<'_> {
    let source = channels.count();
    let alpha = channels.alpha_index();
    if source == components && transfer.is_linear() {
        return Pixels::Borrowed(bytemuck::cast_slice(data));
    }
    let mut out = vec![1.0f32; data.len() / source * components];
    for (pixel, chunk) in data.chunks_exact(source).enumerate() {
        for (index, raw) in chunk.iter().enumerate() {
            out[pixel * components + index] = if Some(index) == alpha || transfer.is_linear() {
                *raw
            } else {
                transfer.to_linear(*raw)
            };
        }
    }
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
            nodata: None,
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
        assert_eq!(plan.format, wgpu::TextureFormat::Rgba8UnormSrgb);
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
        assert_eq!(plan.format, wgpu::TextureFormat::R16Float);

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
            plan(&measurement, FULL).format,
            wgpu::TextureFormat::R16Unorm
        );

        // Without the feature it has to become float, and says so.
        let fallback = plan(&measurement, BARE);
        assert_eq!(fallback.format, wgpu::TextureFormat::R16Float);
        assert!(fallback.precision_note.is_some());
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
        assert_eq!(plan(&scene, FULL).format, wgpu::TextureFormat::Rgba32Float);

        let fallback = plan(&scene, BARE);
        assert_eq!(fallback.format, wgpu::TextureFormat::Rgba16Float);
        assert!(fallback.precision_note.is_some());
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
        assert_eq!(plan.format, wgpu::TextureFormat::Rg16Float);

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
                    nodata: None,
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
