//! Choosing what the surface should be, and therefore how the compositor has
//! to encode its result.
//!
//! Availability is not the same as usefulness: a driver will happily report an
//! HDR colour space while the monitor in front of you is SDR, and picking one
//! then changes how everything looks for no benefit. So HDR output is
//! requested, not assumed — by the monitor, where the compositor says what it
//! is in (`monitor`), and otherwise by hand.

/// Which surface is wanted: what `--output` asked for at start-up, and what
/// the switch asks for after. What it comes to is decided in `App`, which
/// also has the monitor's word; here only [`HdrPreference::On`] asks for the
/// HDR surface.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum HdrPreference {
    /// Whatever the monitor is in, where the compositor says; SDR where
    /// nothing does.
    #[default]
    Follow,
    /// An SDR sRGB surface, with HDR content tone mapped into it.
    Off,
    /// An HDR surface, where the driver offers one — even for a monitor in
    /// SDR mode, which a compositor may answer by switching the monitor over.
    On,
}

impl HdrPreference {
    /// `sdr` or `hdr`, as the command line names them.
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.to_ascii_lowercase().as_str() {
            "sdr" => HdrPreference::Off,
            "hdr" => HdrPreference::On,
            _ => return None,
        })
    }
}

/// The transfer encoding the composite shader applies on the way out, to
/// match what the surface expects to receive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Encoding {
    /// The hardware encodes; the shader writes linear values.
    Srgb,
    /// scRGB: linear values straight out, 1.0 being SDR reference white.
    ScRgbLinear,
    /// HDR10: the shader applies the PQ curve itself.
    Pq,
}

/// The surface configuration, and the encoding the composite shader must
/// apply to match it.
#[derive(Clone, Copy, Debug)]
pub struct Output {
    pub format: wgpu::TextureFormat,
    pub color_space: wgpu::SurfaceColorSpace,
    pub encoding: Encoding,
    pub label: &'static str,
    pub is_hdr: bool,
}

impl Output {
    pub fn choose(
        capabilities: &wgpu::SurfaceCapabilities,
        preference: HdrPreference,
    ) -> Option<Self> {
        if preference == HdrPreference::On {
            if let Some(output) = Self::extended_linear(capabilities) {
                return Some(output);
            }
            if let Some(output) = Self::pq(capabilities) {
                return Some(output);
            }
        }
        Self::srgb(capabilities)
    }

    /// Whether an HDR surface is there to be asked for at all: the driver
    /// offers a colour space with room above white for this window. Not
    /// whether the monitor is HDR, which nothing on Linux will say.
    pub fn hdr_available(capabilities: &wgpu::SurfaceCapabilities) -> bool {
        Self::extended_linear(capabilities).is_some() || Self::pq(capabilities).is_some()
    }

    fn supports(
        capabilities: &wgpu::SurfaceCapabilities,
        format: wgpu::TextureFormat,
        space: wgpu::SurfaceColorSpaces,
    ) -> bool {
        capabilities
            .format_capabilities
            .iter()
            .any(|entry| entry.format == format && entry.color_spaces.contains(space))
    }

    /// scRGB: the shader writes linear values straight out, 1.0 being SDR
    /// reference white. The easiest HDR target to be correct on, because the
    /// working space already is linear.
    fn extended_linear(capabilities: &wgpu::SurfaceCapabilities) -> Option<Self> {
        let format = wgpu::TextureFormat::Rgba16Float;
        Self::supports(
            capabilities,
            format,
            wgpu::SurfaceColorSpaces::EXTENDED_SRGB_LINEAR,
        )
        .then_some(Self {
            format,
            color_space: wgpu::SurfaceColorSpace::ExtendedSrgbLinear,
            encoding: Encoding::ScRgbLinear,
            label: "scRGB linear (HDR)",
            is_hdr: true,
        })
    }

    /// HDR10. Ten bits per channel, so the shader has to apply the PQ curve
    /// itself and dithering would matter if we pushed it harder.
    fn pq(capabilities: &wgpu::SurfaceCapabilities) -> Option<Self> {
        let format = wgpu::TextureFormat::Rgb10a2Unorm;
        Self::supports(capabilities, format, wgpu::SurfaceColorSpaces::BT2100_PQ).then_some(Self {
            format,
            color_space: wgpu::SurfaceColorSpace::Bt2100Pq,
            encoding: Encoding::Pq,
            label: "BT.2100 PQ (HDR10)",
            is_hdr: true,
        })
    }

    /// The ordinary path: an sRGB surface whose hardware encodes the linear
    /// values the compositor writes.
    fn srgb(capabilities: &wgpu::SurfaceCapabilities) -> Option<Self> {
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| format.is_srgb())
            .or_else(|| capabilities.formats.first().copied())?;
        Some(Self {
            format,
            color_space: wgpu::SurfaceColorSpace::Srgb,
            encoding: Encoding::Srgb,
            label: "sRGB",
            is_hdr: false,
        })
    }
}
