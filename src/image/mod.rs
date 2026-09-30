//! The image data model.
//!
//! Decoders describe what they found rather than normalizing it, so that a
//! 16-bit measurement scan and an HDR photograph both survive the trip to the
//! GPU intact. Turning that description into a texture is the renderer's job
//! (see `render::upload`).
//!
//! The pixel model is here; what the numbers mean is [`color`]; the decoders
//! that produce it are [`decode`].

pub mod auxiliary;
pub mod color;
pub mod decode;
pub mod depth;
pub mod directory;
pub mod display;
pub mod enclosed;
pub mod encode;
pub mod exif;
pub mod gain_map;
pub mod geo;
pub mod isobmff;
pub mod metadata_region;
pub mod orient;
pub mod region;
pub mod resample;
pub mod sequence;
pub mod stats;
pub mod tiff;
pub mod xmp;

pub use color::{ColorSpace, Primaries, Transfer};
pub use region::Region;
pub use stats::Stats;

/// How many components each pixel carries, and what they mean.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Channels {
    Gray,
    GrayAlpha,
    Rgb,
    Rgba,
}

impl Channels {
    /// The word for the layout, which the bottom bar and the info panel both
    /// write out.
    pub fn label(self) -> &'static str {
        match self {
            Channels::Gray => "gray",
            Channels::GrayAlpha => "gray+alpha",
            Channels::Rgb => "RGB",
            Channels::Rgba => "RGBA",
        }
    }

    /// The same layout in the shorthand the top bar is set in, where the
    /// space goes to the file's name and every fact about it is read at a
    /// glance rather than out loud. A single letter for gray, where the
    /// three-letter `RGB` is the name of the layout itself: `GRAY16` is the
    /// longest of the twelve and says the least, the depth being the half of
    /// it a reader is there for. Joined to the depth by
    /// [`Samples::short_label`]; the words are [`Channels::label`].
    pub fn code(self) -> &'static str {
        match self {
            Channels::Gray => "G",
            Channels::GrayAlpha => "GA",
            Channels::Rgb => "RGB",
            Channels::Rgba => "RGBA",
        }
    }

    pub fn count(self) -> usize {
        match self {
            Channels::Gray => 1,
            Channels::GrayAlpha => 2,
            Channels::Rgb => 3,
            Channels::Rgba => 4,
        }
    }

    /// Component index of alpha, if there is one.
    pub fn alpha_index(self) -> Option<usize> {
        match self {
            Channels::Gray | Channels::Rgb => None,
            Channels::GrayAlpha => Some(1),
            Channels::Rgba => Some(3),
        }
    }

    pub fn is_gray(self) -> bool {
        matches!(self, Channels::Gray | Channels::GrayAlpha)
    }

    /// How many components carry color, alpha aside: one for gray, three
    /// otherwise. Gray is replicated across the three on the way to the
    /// screen, so one value is the whole of what the file said.
    pub fn color_count(self) -> usize {
        if self.is_gray() { 1 } else { 3 }
    }
}

/// Pixel data, row-major from the top, tightly packed, in the component type
/// the file actually used.
///
/// Typed vectors rather than `Vec<u8>` plus a tag: alignment for
/// `bytemuck::cast_slice` on upload is then guaranteed, and CPU-side work such
/// as histogramming reads naturally. `f16` is deliberately absent — no decoder
/// produces it, and it only appears as an upload target.
#[derive(Clone, Debug)]
pub enum Samples {
    U8 { channels: Channels, data: Vec<u8> },
    U16 { channels: Channels, data: Vec<u16> },
    F32 { channels: Channels, data: Vec<f32> },
}

impl Samples {
    pub fn channels(&self) -> Channels {
        match *self {
            Samples::U8 { channels, .. }
            | Samples::U16 { channels, .. }
            | Samples::F32 { channels, .. } => channels,
        }
    }

    /// Number of components, i.e. `width * height * channels.count()`.
    pub fn len(&self) -> usize {
        match self {
            Samples::U8 { data, .. } => data.len(),
            Samples::U16 { data, .. } => data.len(),
            Samples::F32 { data, .. } => data.len(),
        }
    }

    /// What the buffer costs to hold, in bytes.
    pub fn byte_len(&self) -> usize {
        match self {
            Samples::U8 { data, .. } => data.len(),
            Samples::U16 { data, .. } => data.len() * 2,
            Samples::F32 { data, .. } => data.len() * 4,
        }
    }

    /// The value a fully bright component has before any transfer decode.
    /// Float samples are already in their final units, so they scale by one.
    pub fn full_scale(&self) -> f32 {
        match self {
            Samples::U8 { .. } => u8::MAX as f32,
            Samples::U16 { .. } => u16::MAX as f32,
            Samples::F32 { .. } => 1.0,
        }
    }

    pub fn component_name(&self) -> &'static str {
        match self {
            Samples::U8 { .. } => "8-bit",
            Samples::U16 { .. } => "16-bit",
            Samples::F32 { .. } => "32-bit float",
        }
    }

    /// Layout and depth as one token — `RGB8`, `RGBA16`, `G32F` — for the
    /// top bar, which has a name to fit beside it. The info panel writes the
    /// same two facts out in words instead, having the room.
    pub fn short_label(&self) -> String {
        let depth = match self {
            Samples::U8 { .. } => "8",
            Samples::U16 { .. } => "16",
            Samples::F32 { .. } => "32F",
        };
        format!("{}{depth}", self.channels().code())
    }

    /// The buffer with its data made anew by `rebuild`, at the width and
    /// in the layout it had: what a turn and a shrink both do to it, and
    /// the one match on the three widths they share.
    pub fn rebuilt(&self, rebuild: &impl Rebuild) -> Samples {
        match self {
            Samples::U8 { channels, data } => Samples::U8 {
                channels: *channels,
                data: rebuild.rebuild(data),
            },
            Samples::U16 { channels, data } => Samples::U16 {
                channels: *channels,
                data: rebuild.rebuild(data),
            },
            Samples::F32 { channels, data } => Samples::F32 {
                channels: *channels,
                data: rebuild.rebuild(data),
            },
        }
    }
}

/// One component of a sample, whichever of the three widths it is stored
/// at: read out to a double, and written back from one. What a filter
/// averages, and what a walk over a buffer is generic in.
pub trait Component: Copy {
    fn to_f64(self) -> f64;
    /// The component nearest `mean`: rounded and clamped to the width for
    /// an integer, as it is for a float.
    fn from_f64(mean: f64) -> Self;
}

impl Component for u8 {
    fn to_f64(self) -> f64 {
        f64::from(self)
    }
    fn from_f64(mean: f64) -> Self {
        mean.round().clamp(0.0, 255.0) as u8
    }
}

impl Component for u16 {
    fn to_f64(self) -> f64 {
        f64::from(self)
    }
    fn from_f64(mean: f64) -> Self {
        mean.round().clamp(0.0, 65535.0) as u16
    }
}

impl Component for f32 {
    fn to_f64(self) -> f64 {
        f64::from(self)
    }
    fn from_f64(mean: f64) -> Self {
        mean as f32
    }
}

/// A function of a buffer at any of the three widths, for
/// [`Samples::rebuilt`]: a trait rather than a closure, since a closure
/// cannot be generic over the component.
pub trait Rebuild {
    fn rebuild<T: Component>(&self, data: &[T]) -> Vec<T>;
}

/// Whether color components have already been multiplied by alpha. PNG says
/// straight, EXR usually says premultiplied, and blending them the wrong way
/// shows as haloing.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum AlphaMode {
    Opaque,
    Straight,
    Premultiplied,
}

impl AlphaMode {
    /// The mode for a layout with or without an alpha channel: opaque where
    /// there is none, and otherwise whatever the file says about
    /// premultiplication.
    pub fn of(channels: Channels, premultiplied: bool) -> Self {
        match (channels.alpha_index(), premultiplied) {
            (None, _) => AlphaMode::Opaque,
            (Some(_), true) => AlphaMode::Premultiplied,
            (Some(_), false) => AlphaMode::Straight,
        }
    }
}

/// What the numbers stand for once decoded to linear light: light that was
/// graded for a display, light as a scene had it, or a measurement.
///
/// The distinction everything about the opening view rests on. A graded
/// file — sRGB, a gamma curve, PQ, HLG, or a JPEG with its gain map applied
/// — has a reference white: 1.0 is white, whoever made it put it there, and
/// whatever sits above it is the highlights they meant to keep. Scene light
/// — a render, a light probe, a merge of exposures — has no such point: the
/// numbers are in whatever units the file was made in, cd/m² or a
/// renderer's own, and spread over more stops than a surface has, so it is
/// exposed the way a meter would expose it, the bulk of its light put at
/// middle gray and the highlights left to the curve. A measurement has
/// neither a white nor a middle: a 12-bit scan in a 16-bit container
/// reaches a sixteenth of the way to 1.0, and an elevation model is not
/// light at all, so it is windowed to what it holds. The first is shown as
/// it is; the tone curve is a question the second raises on its own once
/// exposed, and the third only once a hand pushes it past white.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Referred {
    /// Graded: 1.0 is reference white, and the picture is meant as it is.
    Display,
    /// Light with no white stated, in the file's own scale: metered.
    Scene,
    /// Numbers that need not be light at all: no white, no middle gray, and
    /// the useful range has to be found in the pixels.
    Measured,
}

impl Referred {
    /// What the transfer function alone says. A curve is only ever applied to
    /// light that has been graded, so a curved file is display-referred; a
    /// linear one is taken as a measurement, which is the safe reading of
    /// numbers nobody has vouched for, unless its decoder knows better. The
    /// gain-map path does, since its base image was a graded photograph
    /// before the map lifted its highlights, and the Radiance and OpenEXR
    /// paths do, since those formats carry nothing but light.
    pub fn of(transfer: Transfer) -> Self {
        match transfer {
            Transfer::Linear => Referred::Measured,
            Transfer::Srgb
            | Transfer::Pq
            | Transfer::Hlg
            | Transfer::Bt709
            | Transfer::Gamma(_) => Referred::Display,
        }
    }

    /// The word for it, for the information panel.
    pub fn label(self) -> &'static str {
        match self {
            Referred::Display => "display (1.0 is white)",
            Referred::Scene => "scene (no white stated, metered)",
            Referred::Measured => "measurement (no white stated)",
        }
    }
}

/// One decoded image, described rather than normalized.
#[derive(Clone, Debug)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub samples: Samples,
    pub color: ColorSpace,
    pub alpha: AlphaMode,
    /// Whether 1.0 means reference white. Follows the transfer function
    /// unless the decoder says otherwise; see [`Referred`].
    pub referred: Referred,
    /// The multiplier the file says has already been applied to its values,
    /// where a format has a place to say it: Radiance's `EXPOSURE=` lines,
    /// multiplied together, which `pfilt` writes once it has scaled a
    /// picture to be looked at. A file that states one other than 1 has
    /// had its white put where it is by whoever wrote the line, so it is
    /// display-referred and opens as stored; one that states 1 — which
    /// Blender wrote on every picture — has been scaled by nothing and is
    /// scene light like a file that says nothing. The number itself is for
    /// the information panel, and for anyone wanting the physical units
    /// back.
    pub exposure: Option<f32>,
    /// The value standing in for "no measurement here". Elevation models use
    /// -9999 and similar sentinels, which would otherwise dominate the
    /// automatic window and squash the real data into a sliver.
    pub nodata: Option<f32>,
    /// The gain map a phone's photograph carries beside its SDR base, which
    /// `samples` then is: how far above white each pixel went, applied by
    /// the display at the weight the surface's room asks for. See
    /// [`gain_map`].
    pub gain_map: Option<gain_map::Shared>,
    /// How far from the camera each pixel was, where the file carries a
    /// depth map: read by the pointer's readout and by nothing else. See
    /// [`depth`].
    pub depth: Option<depth::Shared>,
}

impl DecodedImage {
    /// An image with nothing stated beyond its pixels and its color space:
    /// what the numbers mean follows from the transfer function, and there is
    /// no no-data sentinel, which is what most formats can say.
    pub fn new(
        width: u32,
        height: u32,
        samples: Samples,
        color: ColorSpace,
        alpha: AlphaMode,
    ) -> Self {
        Self {
            width,
            height,
            samples,
            color,
            alpha,
            referred: Referred::of(color.transfer),
            exposure: None,
            nodata: None,
            gain_map: None,
            depth: None,
        }
    }

    pub fn channels(&self) -> Channels {
        self.samples.channels()
    }

    pub fn is_gray(&self) -> bool {
        self.channels().is_gray()
    }

    /// The pixel at `(x, y)`, or `None` when that is outside the image.
    ///
    /// Reads one pixel the way `render::upload` and `shaders/image.wgsl`
    /// between them read every pixel — the transfer curve resolved, the
    /// gain map applied at the weight `lift` was made at, any
    /// premultiplication divided back out, the primaries taken to the working
    /// space — so that what comes back is the value the display window acts
    /// on. Alpha never gets the curve, here or there. `lift` is the table
    /// the screen is drawn through, where the picture has a gain map; with
    /// none, or with no table, the base is what is read.
    ///
    /// One pixel at a time: this is for a readout following the pointer.
    /// Anything that walks the image sets up a [`Reader`] once and reads
    /// every pixel through it, which is the same arithmetic.
    pub fn sample(&self, x: u32, y: u32, lift: Option<&gain_map::Table>) -> Option<Sample> {
        Reader::new(self, lift).read(x, y)
    }

    /// Sanity check used by the loader, so a broken decoder fails loudly
    /// rather than reading past the end of a buffer on the upload path.
    pub fn validate(&self) -> Result<(), String> {
        if self.width == 0 || self.height == 0 {
            return Err("image has zero size".into());
        }
        // Checked, since this is the last guard before the sample count is
        // trusted by the upload path; a wrapping product could make a short
        // buffer look the right length. Nothing that reaches here has dodged
        // the size ceiling, so the overflow is a belt-and-braces case.
        let expected = (self.width as usize)
            .checked_mul(self.height as usize)
            .and_then(|pixels| pixels.checked_mul(self.samples.channels().count()));
        let Some(expected) = expected else {
            return Err(format!(
                "{}x{} {:?} overflows the addressable range",
                self.width,
                self.height,
                self.samples.channels(),
            ));
        };
        if self.samples.len() != expected {
            return Err(format!(
                "decoder produced {} components, expected {expected} for {}x{} {:?}",
                self.samples.len(),
                self.width,
                self.height,
                self.samples.channels(),
            ));
        }
        Ok(())
    }
}

/// One pixel read back out of an image, in both the terms it can be read in.
///
/// The distinction is the one the whole model rests on: what the file holds
/// is a measurement, and what the screen shows is that measurement decoded,
/// converted and windowed. A readout wants to say both, so this carries the
/// file's own numbers alongside the values the display pipeline starts from.
/// [`display::Display::map`] takes it the rest of the way.
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub channels: Channels,
    stored: [f32; 4],
    /// Every component decoded to linear, lifted and carried into the
    /// working space's primaries, alpha included where there is one, before
    /// the premultiplication is undone: what the statistics bin, so that
    /// the marker on the histogram lands on the bar the scan counted the
    /// pixel in.
    linear: [f32; 4],
    color: [f32; 3],
    /// Coverage as a fraction; 1.0 where the image has no alpha channel.
    pub alpha: f32,
}

impl Sample {
    /// Every component the file carries, alpha included, in the units it
    /// stores them in: counts for integer samples, the value itself for float
    /// ones.
    pub fn stored(&self) -> &[f32] {
        &self.stored[..self.channels.count()]
    }

    /// Every component decoded to linear, lifted where the picture has a
    /// gain map, and carried into the working space's primaries, with any
    /// premultiplication still in: what [`Stats`] measured the pixel as.
    pub fn linear(&self) -> &[f32] {
        &self.linear[..self.channels.count()]
    }

    /// The color, in the linear BT.709 working space with premultiplication
    /// undone: one component for gray, three for color. This is what
    /// `shaders/image.wgsl` has in hand at the moment it applies the window.
    pub fn color(&self) -> &[f32] {
        &self.color[..self.channels.color_count()]
    }
}

/// The pipeline a pixel of one image is read through, set up once for the
/// image: the curve, as a table over every code where the reader is for a
/// walk over integer samples; the matrix carrying the file's primaries
/// into the working space's, only where the two differ and the file has
/// color to carry; the gain map and the table it is lifted through; the
/// scale and the alpha. [`DecodedImage::sample`] reads one pixel through
/// one, and the statistics scan and the encoder hold one for a walk over
/// every pixel, so that the work done per pixel is the pixel's alone. The
/// arithmetic is the same however the reader was set up: a code looked up
/// in the table is the curve run on that code, and a matrix left out is
/// the identity, so a pixel reads the same through any of them.
pub struct Reader<'a> {
    image: &'a DecodedImage,
    channels: Channels,
    count: usize,
    curve: Curve,
    /// What an integer sample is multiplied by to be a fraction of full
    /// scale: one for float samples.
    scale: f32,
    lift: Option<(&'a gain_map::GainMap, &'a gain_map::Table)>,
    matrix: Option<[[f32; 3]; 3]>,
}

/// The transfer curve as a reader runs it: on each component as it comes,
/// or looked up in a table over every code the samples can hold.
enum Curve {
    Direct(Transfer),
    Table(Vec<f32>),
}

impl<'a> Reader<'a> {
    /// A reader for one pixel at a time — the readout under the pointer —
    /// which runs the curve on the components it reads rather than tabulating
    /// it first.
    pub fn new(image: &'a DecodedImage, lift: Option<&'a gain_map::Table>) -> Self {
        Self::with_curve(image, lift, Curve::Direct(image.color.transfer))
    }

    /// A reader for a walk over the picture, the curve tabulated over every
    /// code an integer sample can hold: 256 entries for bytes, 65536 for
    /// words, each the curve run on that code, so that the walk looks the
    /// answer up rather than working it out again a million times over. A
    /// float sample has no codes to tabulate and runs the curve.
    pub fn tabulated(image: &'a DecodedImage, lift: Option<&'a gain_map::Table>) -> Self {
        let transfer = image.color.transfer;
        let scale = 1.0 / image.samples.full_scale();
        let table = |codes: usize| {
            Curve::Table(
                (0..codes)
                    .map(|code| transfer.to_linear(code as f32 * scale))
                    .collect(),
            )
        };
        let curve = match image.samples {
            Samples::U8 { .. } => table(1 << 8),
            Samples::U16 { .. } => table(1 << 16),
            Samples::F32 { .. } => Curve::Direct(transfer),
        };
        Self::with_curve(image, lift, curve)
    }

    fn with_curve(
        image: &'a DecodedImage,
        lift: Option<&'a gain_map::Table>,
        curve: Curve,
    ) -> Self {
        let channels = image.channels();
        let primaries = image.color.primaries;
        Self {
            image,
            channels,
            count: channels.count(),
            curve,
            scale: 1.0 / image.samples.full_scale(),
            lift: lift.and_then(|table| Some((image.gain_map.as_deref()?, table))),
            // A gray file has one channel and no primaries to speak of, and
            // a BT.709 file's matrix is the identity: left out, so that its
            // codes come through exactly as stored.
            matrix: (!channels.is_gray() && primaries != Primaries::Bt709)
                .then(|| primaries.to_bt709()),
        }
    }

    pub fn channels(&self) -> Channels {
        self.channels
    }

    /// What an integer sample is multiplied by to be a fraction of full
    /// scale.
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// Whether the reader moves a color from where the curve alone would
    /// put it: a lift, or a matrix.
    pub fn moves_color(&self) -> bool {
        self.lift.is_some() || self.matrix.is_some()
    }

    /// One pixel's components twice over: as stored — counts for integer
    /// samples, the value itself for floats — and decoded to the linear
    /// working space, lifted and carried into its primaries, alpha included
    /// where the image has one. `(x, y)` must be inside the image.
    pub fn decode(&self, x: u32, y: u32) -> ([f32; 4], [f32; 4]) {
        let image = self.image;
        let count = self.count;
        let base = (y as usize * image.width as usize + x as usize) * count;
        let mut stored = [0.0f32; 4];
        let mut linear = [0.0f32; 4];
        match (&image.samples, &self.curve) {
            (Samples::U8 { data, .. }, Curve::Table(table)) => {
                for (index, raw) in data[base..base + count].iter().enumerate() {
                    stored[index] = *raw as f32;
                    linear[index] = table[usize::from(*raw)];
                }
            }
            (Samples::U8 { data, .. }, Curve::Direct(transfer)) => {
                for (index, raw) in data[base..base + count].iter().enumerate() {
                    stored[index] = *raw as f32;
                    linear[index] = transfer.to_linear(stored[index] * self.scale);
                }
            }
            (Samples::U16 { data, .. }, Curve::Table(table)) => {
                for (index, raw) in data[base..base + count].iter().enumerate() {
                    stored[index] = *raw as f32;
                    linear[index] = table[usize::from(*raw)];
                }
            }
            (Samples::U16 { data, .. }, Curve::Direct(transfer)) => {
                for (index, raw) in data[base..base + count].iter().enumerate() {
                    stored[index] = *raw as f32;
                    linear[index] = transfer.to_linear(stored[index] * self.scale);
                }
            }
            (Samples::F32 { data, .. }, _) => {
                let transfer = image.color.transfer;
                for (index, raw) in data[base..base + count].iter().enumerate() {
                    stored[index] = *raw;
                    linear[index] = transfer.to_linear(*raw);
                }
            }
        }
        // The lift, in the base's own color space, before anything else:
        // the map multiplies light, and the shader multiplies the texel it
        // loaded before it filters, unpremultiplies or converts it.
        if let Some((map, table)) = self.lift {
            let gain = map.gain_at(table, x, y, image.width, image.height);
            table.apply(&mut linear[..self.channels.color_count()], gain);
        }
        // Then the primaries, on the color as it still is, premultiplied
        // and all: the matrix is linear, so the coverage divides out of the
        // converted color as it would have out of the file's. The CPU-side
        // twin of the `primaries` multiply in `shaders/image.wgsl`.
        if let Some(matrix) = self.matrix {
            let color = [linear[0], linear[1], linear[2]];
            for (slot, row) in linear.iter_mut().zip(matrix) {
                *slot = row[0] * color[0] + row[1] * color[1] + row[2] * color[2];
            }
        }
        (stored, linear)
    }

    /// The pixel at `(x, y)` — see [`DecodedImage::sample`] — or `None`
    /// when that is outside the image.
    pub fn read(&self, x: u32, y: u32) -> Option<Sample> {
        let image = self.image;
        if x >= image.width || y >= image.height {
            return None;
        }
        let channels = self.channels;
        let (stored, linear) = self.decode(x, y);
        let mut color = [0.0f32; 3];
        color.copy_from_slice(&linear[..3]);
        let alpha = match channels.alpha_index() {
            Some(index) => (stored[index] * self.scale).clamp(0.0, 1.0),
            None => 1.0,
        };
        if image.alpha == AlphaMode::Premultiplied {
            // The shader's threshold as well as its division: a texel that has
            // resolved to nearly nothing is nothing, rather than a wild color
            // divided out of it.
            if alpha > 1e-4 {
                for value in &mut color {
                    *value /= alpha;
                }
            } else {
                color = [0.0; 3];
            }
        }
        Some(Sample {
            channels,
            stored,
            linear,
            color,
            alpha,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bar's shorthand: layout then depth, one token, no two of the
    /// twelve alike — a reader who has learned `RGB8` has learned the rest.
    #[test]
    fn every_layout_and_depth_has_its_own_shorthand() {
        let layouts = [
            Channels::Gray,
            Channels::GrayAlpha,
            Channels::Rgb,
            Channels::Rgba,
        ];
        let mut seen = Vec::new();
        for channels in layouts {
            for samples in [
                Samples::U8 {
                    channels,
                    data: Vec::new(),
                },
                Samples::U16 {
                    channels,
                    data: Vec::new(),
                },
                Samples::F32 {
                    channels,
                    data: Vec::new(),
                },
            ] {
                seen.push(samples.short_label());
            }
        }
        assert_eq!(seen[0], "G8");
        assert_eq!(seen[5], "GA32F");
        assert_eq!(seen[6], "RGB8");
        assert_eq!(seen[10], "RGBA16");
        let mut unique = seen.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), seen.len(), "{seen:?}");
    }

    #[test]
    fn channel_layout_is_self_consistent() {
        for channels in [
            Channels::Gray,
            Channels::GrayAlpha,
            Channels::Rgb,
            Channels::Rgba,
        ] {
            if let Some(index) = channels.alpha_index() {
                assert_eq!(index, channels.count() - 1, "alpha is the last component");
            }
        }
        assert!(Channels::Gray.is_gray());
        assert!(Channels::GrayAlpha.is_gray());
        assert!(!Channels::Rgb.is_gray());
    }

    fn gray16(data: Vec<u16>, width: u32, height: u32) -> DecodedImage {
        DecodedImage {
            width,
            height,
            samples: Samples::U16 {
                channels: Channels::Gray,
                data,
            },
            color: ColorSpace::LINEAR_BT709,
            alpha: AlphaMode::Opaque,
            referred: Referred::Measured,
            exposure: None,
            nodata: None,
            gain_map: None,
            depth: None,
        }
    }

    /// The readout the pointer drives has to say two things at once: what the
    /// file holds, in the units the file holds it in, and what the pipeline
    /// will make of that. So a sample carries both, and the file's own numbers
    /// come back unscaled — a 16-bit count reads as a count.
    #[test]
    fn a_sample_reports_the_file_s_own_numbers_and_the_decoded_ones() {
        let image = gray16(vec![0, 1000, 2000, 3000, 4000, 5000], 3, 2);
        let sample = image.sample(1, 1, None).expect("inside the image");

        assert_eq!(
            sample.stored(),
            [4000.0],
            "the count, not a fraction of one"
        );
        assert!((sample.color()[0] - 4000.0 / 65535.0).abs() < 1e-6);
        assert_eq!(sample.alpha, 1.0, "an image with no alpha is opaque");

        // Row-major from the top, so the last pixel is the bottom right one.
        assert_eq!(image.sample(2, 1, None).expect("inside").stored(), [5000.0]);
        assert!(image.sample(3, 1, None).is_none());
        assert!(image.sample(0, 2, None).is_none());
    }

    #[test]
    fn a_sample_decodes_the_transfer_curve_but_never_the_alpha() {
        let image = DecodedImage::new(
            1,
            1,
            Samples::U8 {
                channels: Channels::GrayAlpha,
                data: vec![128, 128],
            },
            ColorSpace::SRGB,
            AlphaMode::Straight,
        );

        let sample = image.sample(0, 0, None).expect("inside the image");
        assert_eq!(sample.stored(), [128.0, 128.0]);
        // The same code in both components, and only one of them curved.
        assert!(
            (sample.color()[0] - 0.2158).abs() < 1e-3,
            "{:?}",
            sample.color()
        );
        assert!((sample.alpha - 128.0 / 255.0).abs() < 1e-6);
    }

    /// What the shader has in hand when it applies the window is the straight
    /// color, so that is what a sample reports — while `stored` keeps the
    /// faded numbers the file actually contains.
    #[test]
    fn a_premultiplied_sample_is_divided_back_out_the_way_the_shader_does_it() {
        let image = DecodedImage {
            width: 2,
            height: 1,
            samples: Samples::F32 {
                channels: Channels::Rgba,
                data: vec![0.25, 0.5, 0.75, 0.5, 0.0, 0.0, 0.0, 0.0],
            },
            color: ColorSpace::LINEAR_BT709,
            alpha: AlphaMode::Premultiplied,
            referred: Referred::Scene,
            exposure: None,
            nodata: None,
            gain_map: None,
            depth: None,
        };

        let sample = image.sample(0, 0, None).expect("inside the image");
        assert_eq!(sample.stored(), [0.25, 0.5, 0.75, 0.5]);
        assert_eq!(sample.color(), [0.5, 1.0, 1.5]);

        // A texel that has resolved to nothing is nothing, rather than a wild
        // color divided out of an alpha of zero.
        let empty = image.sample(1, 0, None).expect("inside the image");
        assert_eq!(empty.color(), [0.0, 0.0, 0.0]);
        assert_eq!(empty.alpha, 0.0);
    }

    /// Color comes back in the working space, since that is where the window
    /// and everything after it happens. Gray has no primaries to convert.
    #[test]
    fn a_sample_is_taken_to_the_working_space() {
        let mut image = DecodedImage::new(
            1,
            1,
            Samples::F32 {
                channels: Channels::Rgb,
                data: vec![0.0, 1.0, 0.0],
            },
            ColorSpace::LINEAR_BT709,
            AlphaMode::Opaque,
        );
        assert_eq!(
            image.sample(0, 0, None).expect("inside").color(),
            [0.0, 1.0, 0.0]
        );

        image.color.primaries = Primaries::DisplayP3;
        let converted = image.sample(0, 0, None).expect("inside").color().to_vec();
        // P3 green is outside BT.709, which shows as a negative red.
        assert!(converted[0] < 0.0, "{converted:?}");
        assert!(converted[1] > 1.0, "{converted:?}");
    }

    #[test]
    fn validate_rejects_a_buffer_of_the_wrong_length() {
        assert!(gray16(vec![0; 6], 3, 2).validate().is_ok());
        assert!(gray16(vec![0; 5], 3, 2).validate().is_err());
        assert!(gray16(vec![], 0, 2).validate().is_err());
    }

    /// A curve is only ever put on graded light, so the curve says which kind
    /// of light it is; linear is taken as a measurement until a decoder says
    /// otherwise, since that is the safe reading of numbers nobody has
    /// vouched for.
    #[test]
    fn what_the_light_is_referred_to_follows_from_the_curve() {
        for transfer in [
            Transfer::Srgb,
            Transfer::Pq,
            Transfer::Hlg,
            Transfer::Bt709,
            Transfer::Gamma(2.2),
        ] {
            assert_eq!(Referred::of(transfer), Referred::Display, "{transfer:?}");
        }
        assert_eq!(Referred::of(Transfer::Linear), Referred::Measured);

        let image = gray16(vec![0; 6], 3, 2);
        assert_eq!(image.referred, Referred::Measured);
        let photograph = DecodedImage::new(
            1,
            1,
            Samples::U8 {
                channels: Channels::Gray,
                data: vec![0],
            },
            ColorSpace::SRGB,
            AlphaMode::Opaque,
        );
        assert_eq!(photograph.referred, Referred::Display);
    }
}
