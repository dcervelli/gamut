//! Camera raw files — DNG, NEF, CR2, CR3, ARW, RAF, ORF, RW2, PEF and the
//! rest — developed by the system LibRaw.
//!
//! A raw file is not a picture but the sensor's counts, one color per
//! photosite, with the camera's white balance and color matrix beside them.
//! Turning that into a picture — demosaicing, balancing, converting from
//! the camera's primaries — is a job with three hundred cameras' worth of
//! special cases, and LibRaw is where those cases live: it is dcraw carried
//! on, and what every raw developer on the desktop reads its files with. As
//! with HEIF, that is the system's library, bound by hand in `ffi` and named
//! in the package's dependencies rather than carried as a crate.
//!
//! What LibRaw is asked for is the least developed picture it can make: the
//! camera's own white balance, no brightening, no curve, sixteen bits, in
//! Rec. 2020 — the widest space this program names, so that a saturated
//! flower that the sensor recorded clips less than it would in sRGB. The
//! result is linear light with 1.0 at the sensor's saturation point, which
//! is a photograph with a white, so it is shown display-referred: the
//! window opens at 0..1 rather than being stretched to whatever the frame
//! holds, and a dark frame arrives dark, as it was shot.
//!
//! Every raw carries the camera's own JPEG of the frame, and that is what
//! [`super::Decoder::preview`] hands back: turned the way the camera was
//! held, since the JPEG is stored as the sensor saw it and the orientation
//! beside it, and otherwise as the camera rendered it — its curve, its
//! balance. A likeness of the picture for a thumbnail, found in a few
//! milliseconds where developing the frame takes hundreds.
//!
//! The file is read whole and handed over as one buffer. LibRaw reads by
//! seeking about a stream, and a buffer is the one form of stream its C
//! interface takes; a raw is tens of megabytes, which is a few milliseconds
//! of copying beside the few hundred the demosaic takes.

mod ffi;

use std::ffi::CStr;
use std::os::raw::c_int;

use anyhow::{Result, anyhow, bail};

use crate::image::exif::{Entry, Section};
use crate::image::{
    AlphaMode, Channels, ColorSpace, DecodedImage, Primaries, Referred, Samples, Transfer,
};

pub struct Raw;

impl super::Decoder for Raw {
    fn name(&self) -> &'static str {
        "camera raw"
    }

    fn facts(&self, source: &mut dyn super::ReadSeek) -> Result<Option<(Vec<Entry>, Section)>> {
        facts(source).map(Some)
    }

    fn extensions(&self) -> &'static [&'static str] {
        &[
            "dng", "nef", "nrw", "cr2", "cr3", "crw", "arw", "srf", "sr2", "raf", "orf", "rw2",
            "rwl", "pef", "srw", "3fr", "fff", "iiq", "mef", "mos", "erf", "dcr", "kdc", "mrw",
        ]
    }

    fn sniff(&self, header: &[u8]) -> bool {
        has_signature(header) || tiff_holds_raw(header)
    }

    fn dimensions(&self, source: &mut dyn super::ReadSeek) -> Result<Option<(u32, u32)>> {
        let handle = Handle::open(source)?;
        // A camera whose photosites are not square has its picture stretched
        // to fix that on the way out, and the header does not say by how
        // much. Rare — a handful of cameras from the early 2000s — and the
        // window takes its default shape rather than a wrong one.
        if handle.sizes().pixel_aspect != 1.0 {
            return Ok(None);
        }
        Ok(Some(handle.output_size()))
    }

    fn preview(
        &self,
        source: &mut dyn super::ReadSeek,
        overrides: super::Overrides,
    ) -> Result<Option<DecodedImage>> {
        let handle = Handle::open(source)?;
        // A file with no preview in it is a file with none, not a failure.
        if !handle.unpack_thumbnail()? {
            return Ok(None);
        }
        let thumbnail = handle.make_thumbnail()?;
        let image = match thumbnail.kind() {
            ffi::IMAGE_JPEG => super::jpeg::decode_stored(thumbnail.bytes(), overrides)?,
            ffi::IMAGE_BITMAP => thumbnail.bitmap()?,
            other => bail!("LibRaw handed back a preview of kind {other}"),
        };
        // The JPEG is stored as the sensor saw the scene, with the way the
        // camera was held beside it; the developed picture is turned to
        // match, so the preview is too — by this orientation alone, since
        // a preview that repeats the tag in an EXIF of its own, as a RAF's
        // does, would otherwise be turned twice.
        let image = crate::image::orient::apply(image, handle.orientation());
        Ok(Some(image))
    }

    fn decode(
        &self,
        source: &mut dyn super::ReadSeek,
        _overrides: super::Overrides,
    ) -> Result<DecodedImage> {
        let handle = Handle::open(source)?;
        let (width, height) = handle.output_size();
        if width == 0 || height == 0 {
            bail!("raw image is {width}x{height}");
        }
        // Three channels is the most the developed picture can have, so this
        // is the ceiling whether the sensor turns out to be color or not.
        super::check_decoded_size(width, height, 3, 16)?;

        handle.configure();
        handle.call("unpacking", ffi::libraw_unpack)?;
        handle.call("developing", ffi::libraw_dcraw_process)?;
        let developed = handle.make_image()?;

        let (width, height, channels, data) = developed.take()?;
        let mut image = DecodedImage::new(
            width,
            height,
            Samples::U16 { channels, data },
            ColorSpace {
                transfer: Transfer::Linear,
                primaries: Primaries::Bt2020,
            },
            AlphaMode::Opaque,
        );
        // Linear, but graded: the camera's white balance has been applied
        // and 1.0 is where the sensor saturates, which is as much of a white
        // as a photograph states.
        image.referred = Referred::Display;
        Ok(image)
    }
}

/// What LibRaw read out of the header, for the information panel: the
/// sensor and how the file describes it, as one section, and the exposure
/// as the library parsed it from the maker's own block, as the entries the
/// panel's `Camera` section is made of. The second is for the files the
/// EXIF reader gets nothing or too little out of — a CRW has no EXIF at
/// all, and a Phase One's names the camera and stops — and the panel takes
/// from it whatever the EXIF did not say.
pub(super) fn facts(source: &mut dyn super::ReadSeek) -> Result<(Vec<Entry>, Section)> {
    let handle = Handle::open(source)?;
    Ok((
        handle.camera(),
        Section {
            name: "Sensor",
            entries: handle.sensor(),
        },
    ))
}

/// One LibRaw handle over one file's bytes. The bytes live here because the
/// handle reads them for as long as it is open, and it is closed on drop.
struct Handle {
    data: *mut ffi::Data,
    _bytes: Vec<u8>,
}

impl Handle {
    fn open(source: &mut dyn super::ReadSeek) -> Result<Self> {
        let mut bytes = Vec::new();
        source.read_to_end(&mut bytes)?;

        // SAFETY: `libraw_init` takes only flags, and 0 means the defaults.
        let data = unsafe { ffi::libraw_init(0) };
        if data.is_null() {
            bail!("LibRaw could not allocate a handle");
        }
        let handle = Self {
            data,
            _bytes: bytes,
        };
        // SAFETY: the buffer outlives the handle, being owned by it, and the
        // handle is a live one from `libraw_init`.
        let code = unsafe {
            ffi::libraw_open_buffer(
                handle.data,
                handle._bytes.as_ptr().cast(),
                handle._bytes.len(),
            )
        };
        handle.check("reading the header", code)?;
        handle.check_layout()?;
        Ok(handle)
    }

    /// The transcription in `ffi` against the library's own word. The two
    /// accessors read the same two fields the transcription reaches for, one
    /// at each end of the transcribed struct, so a layout that has moved is
    /// caught here rather than read as garbage.
    fn check_layout(&self) -> Result<()> {
        // SAFETY: the handle is open, so both accessors have a filled struct
        // to read; the transcribed fields are read through the same pointer.
        let agrees = unsafe {
            std::ptr::eq(ffi::libraw_get_iparams(self.data), &(*self.data).idata)
                && ffi::libraw_get_iwidth(self.data) == c_int::from((*self.data).sizes.iwidth)
                && ffi::libraw_get_iheight(self.data) == c_int::from((*self.data).sizes.iheight)
        };
        if !agrees {
            bail!(
                "the installed LibRaw lays its data out differently from the \
                 version this program was written against"
            );
        }
        Ok(())
    }

    fn sizes(&self) -> &ffi::Sizes {
        // SAFETY: an open handle; the layout was checked when it was opened.
        unsafe { &(*self.data).sizes }
    }

    fn params(&self) -> &ffi::Params {
        // SAFETY: as `sizes`.
        unsafe { &(*self.data).idata }
    }

    fn other(&self) -> &ffi::Other {
        // SAFETY: the accessor hands back a pointer into the open handle's
        // own struct, which lives as long as the handle.
        unsafe { &*ffi::libraw_get_imgother(self.data) }
    }

    fn lens(&self) -> &ffi::Lens {
        // SAFETY: as `other`; `Lens` is a prefix of the struct pointed at.
        unsafe { &*ffi::libraw_get_lensinfo(self.data) }
    }

    /// What took the picture and how, as LibRaw parsed it: the panel's
    /// `Camera` section for a file the EXIF reader found nothing in.
    fn camera(&self) -> Vec<Entry> {
        let mut rows = Vec::new();
        let params = self.params();
        let make = text(&params.make);
        let model = text(&params.model);
        let camera = match (make, model) {
            (Some(make), Some(model)) if model.starts_with(&make) => Some(model),
            (Some(make), Some(model)) => Some(format!("{make} {model}")),
            (some, None) | (None, some) => some,
        };
        push(&mut rows, "Camera", camera);
        push(&mut rows, "Lens", text(&self.lens().lens));

        let other = self.other();
        if other.timestamp > 0 {
            // dcraw makes the camera's date a `time_t` as if it were in this
            // machine's zone, so the same zone gives the camera's fields back.
            let taken = crate::clock::local(
                std::time::UNIX_EPOCH + std::time::Duration::from_secs(other.timestamp as u64),
            );
            push(
                &mut rows,
                "Taken",
                Some(format!(
                    "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                    taken.year, taken.month, taken.day, taken.hour, taken.minute, taken.second
                )),
            );
        }
        let mut exposure = Vec::new();
        if other.shutter > 0.0 {
            exposure.push(if other.shutter < 1.0 {
                format!("1/{} s", tidy((1.0 / other.shutter).round()))
            } else {
                format!("{} s", tidy(other.shutter))
            });
        }
        if other.aperture > 0.0 {
            // To a tenth, which is how an f-number is spoken; the maker's
            // block holds it to more places than the lens was ever set to.
            let aperture = format!("{:.1}", other.aperture);
            exposure.push(format!("f/{}", aperture.trim_end_matches(".0")));
        }
        if other.iso_speed > 0.0 {
            exposure.push(format!("ISO {}", tidy(other.iso_speed)));
        }
        push(&mut rows, "Exposure", join(&exposure));
        if other.focal_len > 0.0 {
            push(
                &mut rows,
                "Focal length",
                Some(format!("{} mm", tidy(other.focal_len))),
            );
        }
        rows
    }

    /// The sensor and what the file says about it: the frame's size and the
    /// picture's inside it, the filter pattern, the white level, the
    /// balances, the matrix.
    fn sensor(&self) -> Vec<Entry> {
        let mut rows = Vec::new();
        let sizes = self.sizes();
        let params = self.params();
        push(
            &mut rows,
            "Sensor",
            Some(format!("{} × {}", sizes.raw_width, sizes.raw_height)),
        );
        let (width, height) = (u32::from(sizes.width), u32::from(sizes.height));
        if (width, height) != (u32::from(sizes.raw_width), u32::from(sizes.raw_height)) {
            let at = match (sizes.left_margin, sizes.top_margin) {
                (0, 0) => String::new(),
                (left, top) => format!(" at {left}, {top}"),
            };
            push(
                &mut rows,
                "Picture",
                Some(format!("{width} × {height}{at}")),
            );
        }
        push(&mut rows, "Filter pattern", Some(pattern(params)));
        if params.colors > 0 {
            push(&mut rows, "Colors", Some(params.colors.to_string()));
        }
        // SAFETY: accessors on an open handle.
        let (maximum, camera, daylight, matrix) = unsafe {
            let each = |get: unsafe extern "C" fn(*mut ffi::Data, c_int) -> f32| -> Vec<f32> {
                (0..4).map(|index| get(self.data, index)).collect()
            };
            let mut matrix = [[0.0f32; 4]; 3];
            for (row, values) in matrix.iter_mut().enumerate() {
                for (column, value) in values.iter_mut().enumerate() {
                    *value = ffi::libraw_get_rgb_cam(self.data, row as c_int, column as c_int);
                }
            }
            (
                ffi::libraw_get_color_maximum(self.data),
                each(ffi::libraw_get_cam_mul),
                each(ffi::libraw_get_pre_mul),
                matrix,
            )
        };
        if maximum > 0 {
            push(&mut rows, "White level", Some(maximum.to_string()));
        }
        push(&mut rows, "White balance", multipliers(&camera));
        push(&mut rows, "Daylight balance", multipliers(&daylight));
        // Camera to sRGB, three rows of as many columns as the sensor has
        // colors: what the library will develop through.
        let columns = params.colors.clamp(3, 4) as usize;
        if matrix.iter().flatten().any(|value| *value != 0.0) {
            let rows_text: Vec<String> = matrix
                .iter()
                .map(|row| {
                    row[..columns]
                        .iter()
                        .map(|value| format!("{value:.4}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect();
            push(&mut rows, "Matrix to sRGB", Some(rows_text.join(" / ")));
        }
        if params.dng_version != 0 {
            let version = params.dng_version;
            push(
                &mut rows,
                "DNG version",
                Some(format!(
                    "{}.{}.{}.{}",
                    version >> 24,
                    (version >> 16) & 0xff,
                    (version >> 8) & 0xff,
                    version & 0xff
                )),
            );
        }
        if params.raw_count > 1 {
            push(&mut rows, "Frames", Some(params.raw_count.to_string()));
        }
        if params.is_foveon != 0 {
            push(&mut rows, "Sensor type", Some("Foveon".to_string()));
        }
        rows
    }

    /// The developed picture's size: the cropped sensor, turned the way the
    /// camera was held.
    fn output_size(&self) -> (u32, u32) {
        let sizes = self.sizes();
        let (width, height) = (u32::from(sizes.iwidth), u32::from(sizes.iheight));
        // dcraw's `flip`: 3 is upside down, 5 and 6 a quarter turn either
        // way, so bit 2 is the one that swaps the sides.
        if sizes.flip & 4 != 0 {
            (height, width)
        } else {
            (width, height)
        }
    }

    /// The way the camera was held, as `image` spells it. dcraw's `flip` is
    /// its own numbering — 3 upside down, 5 a quarter turn one way, 6 the
    /// other — and each of those is one of EXIF's, which `image` reads.
    fn orientation(&self) -> ::image::metadata::Orientation {
        use ::image::metadata::Orientation;
        let exif = match self.sizes().flip {
            3 => 3,
            5 => 8,
            6 => 6,
            _ => 1,
        };
        Orientation::from_exif(exif).unwrap_or(Orientation::NoTransforms)
    }

    /// Reads the preview out of the file, saying whether there was one.
    fn unpack_thumbnail(&self) -> Result<bool> {
        // SAFETY: an open handle.
        let code = unsafe { ffi::libraw_unpack_thumb(self.data) };
        match code {
            ffi::SUCCESS => Ok(true),
            ffi::NO_THUMBNAIL | ffi::UNSUPPORTED_THUMBNAIL => Ok(false),
            other => Err(anyhow!("reading the preview: {}", describe(other))),
        }
    }

    fn make_thumbnail(&self) -> Result<Developed> {
        let mut code = ffi::SUCCESS;
        // SAFETY: the preview has been unpacked; the code is written through
        // the pointer given.
        let image = unsafe { ffi::libraw_dcraw_make_mem_thumb(self.data, &mut code) };
        if image.is_null() {
            bail!("copying the preview out: {}", describe(code));
        }
        Ok(Developed { image })
    }

    /// The least a raw can be developed: linear, unbrightened, sixteen bits,
    /// the camera's own balance, in Rec. 2020.
    fn configure(&self) {
        // SAFETY: setters on an open handle; each writes one field of the
        // output parameters.
        unsafe {
            ffi::libraw_set_demosaic(self.data, ffi::DEMOSAIC_AHD);
            ffi::libraw_set_output_color(self.data, ffi::OUTPUT_REC2020);
            ffi::libraw_set_output_bps(self.data, 16);
            // The curve's power and its toe slope, both 1: dcraw's `-g 1 1`.
            ffi::libraw_set_gamma(self.data, 0, 1.0);
            ffi::libraw_set_gamma(self.data, 1, 1.0);
            ffi::libraw_set_no_auto_bright(self.data, 1);

            // The white balance the camera recorded, where it recorded one.
            // The C interface has no switch for "use the camera's", so the
            // camera's multipliers are handed back as the user's, which is
            // the same arithmetic. A file with none — a synthetic DNG, an
            // old scan — keeps the daylight balance its matrix implies.
            let camera: Vec<f32> = (0..4)
                .map(|index| ffi::libraw_get_cam_mul(self.data, index))
                .collect();
            if camera[0] > 0.0 && camera[2] > 0.0 {
                for (index, multiplier) in camera.into_iter().enumerate() {
                    ffi::libraw_set_user_mul(self.data, index as c_int, multiplier);
                }
            }
        }
    }

    /// One step of the pipeline, by the error it would raise.
    fn call(&self, doing: &str, step: unsafe extern "C" fn(*mut ffi::Data) -> c_int) -> Result<()> {
        // SAFETY: each step takes the open handle and nothing else.
        let code = unsafe { step(self.data) };
        self.check(doing, code)
    }

    fn check(&self, doing: &str, code: c_int) -> Result<()> {
        if code == ffi::SUCCESS {
            return Ok(());
        }
        Err(anyhow!("{doing}: {}", describe(code)))
    }

    fn make_image(&self) -> Result<Developed> {
        let mut code = ffi::SUCCESS;
        // SAFETY: the handle has been through `dcraw_process`; the code is
        // written through the pointer given.
        let image = unsafe { ffi::libraw_dcraw_make_mem_image(self.data, &mut code) };
        if image.is_null() {
            bail!("reading the developed image back: {}", describe(code));
        }
        Ok(Developed { image })
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: closes the handle `libraw_init` made, once.
        unsafe { ffi::libraw_close(self.data) };
    }
}

/// A C string field as text, or nothing for an empty one.
fn text(field: &[std::ffi::c_char]) -> Option<String> {
    let bytes: Vec<u8> = field
        .iter()
        .map(|byte| *byte as u8)
        .take_while(|byte| *byte != 0)
        .collect();
    let text = String::from_utf8_lossy(&bytes).trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// The color filter array as the letters of its repeating cell: `RGGB` for
/// the common Bayer layouts, X-Trans by name, and none for a sensor that
/// reads every color at every site.
fn pattern(params: &ffi::Params) -> String {
    match params.filters {
        0 => "none".to_string(),
        9 => "X-Trans".to_string(),
        filters if params.colors > 0 => {
            // dcraw's `FC`: two bits per site of a 2×2 cell, an index into
            // the color names the camera lists.
            let names: Vec<u8> = params.cdesc.iter().map(|byte| *byte as u8).collect();
            let cell: String = (0..2)
                .flat_map(|row| (0..2).map(move |column| (row, column)))
                .map(|(row, column): (u32, u32)| {
                    let index = (filters >> (((row << 1 & 14) + (column & 1)) << 1)) & 3;
                    names.get(index as usize).copied().unwrap_or(b'?') as char
                })
                .collect();
            cell
        }
        _ => "unknown".to_string(),
    }
}

/// Multipliers as the panel writes them, green held at 1: what the balance
/// does to red and blue against it, which is how a photographer reads one.
fn multipliers(values: &[f32]) -> Option<String> {
    let (red, green, blue) = (values[0], values[1], values[2]);
    if red <= 0.0 || green <= 0.0 || blue <= 0.0 {
        return None;
    }
    Some(format!(
        "R {} · G 1 · B {}",
        tidy(red / green),
        tidy(blue / green)
    ))
}

/// A number to a few decimals, without the trailing zeros.
fn tidy(value: f32) -> String {
    let text = format!("{value:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn push(rows: &mut Vec<Entry>, name: &str, value: Option<String>) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        rows.push(Entry::new(name, value));
    }
}

fn join(parts: &[String]) -> Option<String> {
    (!parts.is_empty()).then(|| parts.join(" \u{00b7} "))
}

/// What LibRaw says a code means.
fn describe(code: c_int) -> String {
    // SAFETY: `libraw_strerror` returns a static string for any code.
    let message = unsafe { CStr::from_ptr(ffi::libraw_strerror(code)) };
    message.to_string_lossy().into_owned()
}

/// A picture LibRaw handed back — the developed frame, or the preview — in
/// its own allocation, freed on drop.
struct Developed {
    image: *mut ffi::Processed,
}

impl Developed {
    fn header(&self) -> &ffi::Processed {
        // SAFETY: a non-null result of `dcraw_make_mem_image` or
        // `dcraw_make_mem_thumb`, whose header fields say how many bytes
        // follow it.
        unsafe { &*self.image }
    }

    fn kind(&self) -> c_int {
        self.header().kind
    }

    /// Everything after the header: the JPEG, or the pixels.
    fn bytes(&self) -> &[u8] {
        let header = self.header();
        // SAFETY: `data_size` bytes follow `data` in the allocation, which
        // is what the library says it allocated.
        unsafe { std::slice::from_raw_parts(header.data.as_ptr(), header.data_size as usize) }
    }

    /// The pixels of a bitmap, as `width`, `height`, channels and the
    /// samples of `bits` each, checked against the byte count.
    fn pixels(&self, bits: u16) -> Result<(u32, u32, Channels, &[u8])> {
        let header = self.header();
        if header.kind != ffi::IMAGE_BITMAP {
            bail!("LibRaw handed back something other than a bitmap");
        }
        if header.bits != bits {
            bail!(
                "LibRaw handed back {} bits per sample, not {bits}",
                header.bits
            );
        }
        let channels = match header.colors {
            1 => Channels::Gray,
            3 => Channels::Rgb,
            other => bail!("LibRaw handed back {other} colors per pixel"),
        };
        let (width, height) = (u32::from(header.width), u32::from(header.height));
        let samples = width as usize * height as usize * channels.count();
        if header.data_size as usize != samples * usize::from(bits / 8) {
            bail!(
                "LibRaw handed back {width}x{height} with {} colors but {} bytes",
                header.colors,
                header.data_size
            );
        }
        Ok((width, height, channels, self.bytes()))
    }

    /// A preview kept as pixels rather than as a JPEG, which is what a few
    /// makes write: eight-bit sRGB, the way a JPEG would decode.
    fn bitmap(&self) -> Result<DecodedImage> {
        let (width, height, channels, bytes) = self.pixels(8)?;
        Ok(DecodedImage::new(
            width,
            height,
            Samples::U8 {
                channels,
                data: bytes.to_vec(),
            },
            ColorSpace::SRGB,
            AlphaMode::Opaque,
        ))
    }

    /// The developed frame's pixels, copied out as this program holds them.
    fn take(&self) -> Result<(u32, u32, Channels, Vec<u16>)> {
        let (width, height, channels, bytes) = self.pixels(16)?;
        let data = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_ne_bytes(*pair))
            .collect();
        Ok((width, height, channels, data))
    }
}

impl Drop for Developed {
    fn drop(&mut self) {
        // SAFETY: frees what `dcraw_make_mem_image` allocated, once.
        unsafe { ffi::libraw_dcraw_clear_mem(self.image) };
    }
}

/// The formats whose first bytes are their own: everything that is not a
/// TIFF in disguise.
fn has_signature(header: &[u8]) -> bool {
    let at = |offset: usize, magic: &[u8]| header.get(offset..offset + magic.len()) == Some(magic);
    // Canon CR2: a TIFF header that names itself at byte 8.
    at(8, b"CR\x02")
        // Canon CR3: an ISO base media file of the `crx ` brand.
        || (at(4, b"ftyp") && at(8, b"crx "))
        // Canon CRW, the format before CR2.
        || at(6, b"HEAPCCDR")
        // Olympus, either byte order.
        || at(0, b"IIRO") || at(0, b"IIRS") || at(0, b"MMOR")
        // Panasonic and Leica.
        || at(0, b"IIU\0")
        // Fujifilm.
        || at(0, b"FUJIFILMCCD-RAW")
        // Minolta.
        || at(0, b"\0MRM")
        // Phase One: a TIFF header, then its own block at byte 8, whose
        // directory is at the far end of the file where no header reaches.
        || at(8, b"IIII") || at(8, b"MMMM")
}

/// Whether a file with TIFF's header holds a camera's raw data rather than a
/// picture: DNG, NEF, ARW, PEF, SRW and the rest wear the same four bytes
/// as a scan, and `decode::tiff_rs` would show a NEF's thumbnail rather than
/// its picture. The first directory says which it is, when it says anything.
fn tiff_holds_raw(header: &[u8]) -> bool {
    let Some(directory) = Directory::first(header) else {
        return false;
    };
    // A DNG says so.
    directory.has(0xC612)
        // A directory of sensor counts: `CFA` or `LinearRaw`.
        || matches!(directory.value(0x0106), Some(32803 | 34892))
        // A vendor's own compression code: Sony, Nikon, Samsung, Pentax and
        // Kodak each write one that no other TIFF writer does.
        || matches!(
            directory.value(0x0103),
            Some(32767 | 32769 | 32770 | 32772 | 34713 | 65000 | 65535)
        )
        // The first directory is a reduced copy and the picture is in a
        // sub-directory, which is how NEF and ARW are laid out: a plain
        // TIFF's first directory is its picture.
        || (directory.value(0x00FE).is_some_and(|kind| kind & 1 != 0) && directory.has(0x014A))
        // Or holds no picture at all, only the camera's name and the
        // sub-directories, which is Samsung's layout.
        || (directory.has(0x014A) && !directory.has(0x0100))
}

/// The entries of a TIFF's first directory, as far as the header reaches.
struct Directory<'a> {
    header: &'a [u8],
    little_endian: bool,
    entries: usize,
    start: usize,
}

impl<'a> Directory<'a> {
    fn first(header: &'a [u8]) -> Option<Self> {
        let little_endian = match header.get(..4)? {
            b"II\x2a\x00" => true,
            b"MM\x00\x2a" => false,
            _ => return None,
        };
        let mut directory = Self {
            header,
            little_endian,
            entries: 0,
            start: 0,
        };
        let offset = directory.u32(4)? as usize;
        directory.entries = directory.u16(offset)? as usize;
        directory.start = offset + 2;
        Some(directory)
    }

    fn u16(&self, at: usize) -> Option<u16> {
        let bytes: [u8; 2] = self.header.get(at..at + 2)?.try_into().ok()?;
        Some(match self.little_endian {
            true => u16::from_le_bytes(bytes),
            false => u16::from_be_bytes(bytes),
        })
    }

    fn u32(&self, at: usize) -> Option<u32> {
        let bytes: [u8; 4] = self.header.get(at..at + 4)?.try_into().ok()?;
        Some(match self.little_endian {
            true => u32::from_le_bytes(bytes),
            false => u32::from_be_bytes(bytes),
        })
    }

    /// The entries the header holds, as (tag, type, count, value-or-offset).
    /// A directory that runs past the header is read as far as it goes,
    /// whole entries only.
    fn entries(&self) -> impl Iterator<Item = (u16, u16, u32, usize)> + '_ {
        (0..self.entries).map_while(move |index| {
            let at = self.start + index * 12;
            if at + 12 > self.header.len() {
                return None;
            }
            Some((self.u16(at)?, self.u16(at + 2)?, self.u32(at + 4)?, at + 8))
        })
    }

    fn has(&self, tag: u16) -> bool {
        self.entries().any(|(found, ..)| found == tag)
    }

    /// A single `SHORT` or `LONG` entry's value, which TIFF keeps inline.
    fn value(&self, tag: u16) -> Option<u32> {
        let (_, kind, count, at) = self.entries().find(|(found, ..)| *found == tag)?;
        match (kind, count) {
            (3, 1) => self.u16(at).map(u32::from),
            (4, 1) => self.u32(at),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A little-endian TIFF header whose first directory holds the given
    /// `SHORT` or `LONG` entries.
    fn tiff(entries: &[(u16, u32)]) -> Vec<u8> {
        let mut bytes = b"II\x2a\x00\x08\x00\x00\x00".to_vec();
        bytes.extend((entries.len() as u16).to_le_bytes());
        for (tag, value) in entries {
            bytes.extend(tag.to_le_bytes());
            bytes.extend(4u16.to_le_bytes());
            bytes.extend(1u32.to_le_bytes());
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(0u32.to_le_bytes());
        bytes
    }

    #[test]
    fn a_plain_tiff_is_left_alone() {
        // A picture: full resolution, uncompressed RGB.
        let plain = tiff(&[(0x00FE, 0), (0x0100, 32), (0x0103, 1), (0x0106, 2)]);
        assert!(!tiff_holds_raw(&plain));
        // A pyramid whose overviews hang off the picture is still a
        // picture first.
        let pyramid = tiff(&[(0x00FE, 0), (0x0100, 32), (0x0106, 2), (0x014A, 1000)]);
        assert!(!tiff_holds_raw(&pyramid));
        // Not a TIFF at all.
        assert!(!tiff_holds_raw(b"\x89PNG\r\n\x1a\n"));
        assert!(!tiff_holds_raw(b"II"));
    }

    #[test]
    fn a_raw_in_tiff_clothing_is_claimed() {
        // DNG, by its version tag.
        assert!(tiff_holds_raw(&tiff(&[(0x0106, 2), (0xC612, 0x01040000)])));
        // Pentax: the sensor's counts in the first directory.
        assert!(tiff_holds_raw(&tiff(&[(0x0106, 32803)])));
        // Nikon and Sony: a thumbnail first, the picture in a sub-directory.
        assert!(tiff_holds_raw(&tiff(&[(0x00FE, 1), (0x014A, 149006)])));
        // Nikon's own compression code, wherever it turns up.
        assert!(tiff_holds_raw(&tiff(&[(0x0103, 34713)])));
        // Samsung: a first directory with nothing in it but the camera's
        // name and the way to the sub-directories.
        assert!(tiff_holds_raw(&tiff(&[
            (0x010F, 86),
            (0x8769, 154),
            (0x014A, 126)
        ])));
    }

    #[test]
    fn a_directory_cut_short_is_read_as_far_as_it_goes() {
        let whole = tiff(&[(0x0106, 2), (0xC612, 0x01040000)]);
        // Cut inside the second entry: the first is still readable, the
        // second is not, and nothing panics.
        assert!(!tiff_holds_raw(&whole[..8 + 2 + 12 + 6]));
        // Cut after the second entry's tag and type but before its value.
        assert!(!tiff_holds_raw(&whole[..8 + 2 + 12 + 8]));
        // The whole second entry present, the terminator not.
        assert!(tiff_holds_raw(&whole[..8 + 2 + 24]));
    }

    /// The fixture's header, as the panel will read it: the sensor's size,
    /// the filter cell the generator laid out, the white level it wrote in
    /// twelve bits, and the balance of ones its neutral asks for. No
    /// exposure, since nothing took the picture.
    #[test]
    fn the_fixture_reports_its_sensor() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_images")
            .join("dng-cfa.dng");
        let mut source = std::io::BufReader::new(std::fs::File::open(path).unwrap());
        let (camera, sensor) = facts(&mut source).unwrap();

        let names: Vec<&str> = camera.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["Camera"]);
        assert_eq!(camera[0].value, "gamut fixture");

        assert_eq!(sensor.name, "Sensor");
        let find = |name: &str| {
            sensor
                .entries
                .iter()
                .find(|entry| entry.name == name)
                .map(|entry| entry.value.as_str())
        };
        assert_eq!(find("Sensor"), Some("32 × 24"));
        assert_eq!(find("Picture"), None, "the whole sensor is the picture");
        assert_eq!(find("Filter pattern"), Some("RGGB"));
        assert_eq!(find("White level"), Some("4095"));
        assert_eq!(find("White balance"), Some("R 1 · G 1 · B 1"));
        assert_eq!(find("DNG version"), Some("1.4.0.0"));
        assert!(find("Matrix to sRGB").is_some());
    }

    #[test]
    fn a_filter_cell_is_spelled_from_the_bit_pattern() {
        let mut params: ffi::Params = unsafe { std::mem::zeroed() };
        params.colors = 3;
        for (index, byte) in b"RGBG".iter().enumerate() {
            params.cdesc[index] = *byte as std::ffi::c_char;
        }
        // dcraw's code for RGGB, and X-Trans and none by their codes.
        params.filters = 0x94949494;
        assert_eq!(pattern(&params), "RGGB");
        params.filters = 0x16161616;
        assert_eq!(pattern(&params), "BGGR");
        params.filters = 9;
        assert_eq!(pattern(&params), "X-Trans");
        params.filters = 0;
        assert_eq!(pattern(&params), "none");
    }

    /// Real cameras' files, one of each format: each has to be recognized
    /// as a raw rather than as the TIFF or the ISO media file it is dressed
    /// as, probe to the size it develops to, develop, hand back its preview
    /// the right way up and large enough to thumbnail from, and give up its
    /// metadata. The files are tens of megabytes each and are not in the
    /// tree: `test_images/raw-samples/fetch.sh` brings them down from
    /// raw.pixls.us, and `GAMUT_RAW_SAMPLES` names another directory
    /// instead.
    ///
    /// ```sh
    /// test_images/raw-samples/fetch.sh
    /// cargo test --release raw::tests::samples -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "needs the camera files test_images/raw-samples/fetch.sh brings down"]
    fn samples_are_recognized_probed_and_developed() {
        use crate::image::decode::{Overrides, load_timed, preview, probe, reader};

        let directory = std::env::var("GAMUT_RAW_SAMPLES")
            .unwrap_or_else(|_| format!("{}/test_images/raw-samples", env!("CARGO_MANIFEST_DIR")));
        let mut paths: Vec<_> = std::fs::read_dir(&directory)
            .expect("the samples directory")
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|extension| extension != "sh"))
            .collect();
        paths.sort();
        assert!(
            !paths.is_empty(),
            "{directory} holds no camera files; run test_images/raw-samples/fetch.sh"
        );

        for path in paths {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            assert_eq!(
                reader(&path),
                Some("camera raw"),
                "{name} is not recognized as a raw"
            );
            let probed = probe(&path).unwrap_or_else(|error| panic!("{name}: {error:#}"));
            let (image, took) = load_timed(&path, Overrides::default(), None)
                .unwrap_or_else(|error| panic!("{name}: {error:#}"));
            if let Some(size) = probed {
                assert_eq!(
                    size,
                    (image.width, image.height),
                    "{name} probes to the wrong size"
                );
            }
            assert_eq!(image.referred, Referred::Display, "{name}");

            // The preview: present, turned the way the picture is, and
            // large enough to thumbnail from.
            let started = std::time::Instant::now();
            let small = preview(&path, Overrides::default())
                .unwrap_or_else(|error| panic!("{name}: {error:#}"))
                .unwrap_or_else(|| panic!("{name} carries no preview"));
            let preview_took = started.elapsed();
            let landscape = |width: u32, height: u32| width >= height;
            assert_eq!(
                landscape(small.width, small.height),
                landscape(image.width, image.height),
                "{name}: the preview is {}x{} but the picture is {}x{}",
                small.width,
                small.height,
                image.width,
                image.height
            );
            assert!(
                small.width.max(small.height) >= crate::thumbnail::SIDE,
                "{name}: the preview is only {}x{}",
                small.width,
                small.height
            );

            let exif = crate::image::exif::Exif::read(&path);
            let sections: Vec<String> = exif
                .sections
                .iter()
                .map(|section| format!("{} ({})", section.name, section.entries.len()))
                .collect();
            println!(
                "{name}: {}x{} {:?} in {} ms; preview {}x{} in {} ms; metadata: {}",
                image.width,
                image.height,
                image.channels(),
                took.as_millis(),
                small.width,
                small.height,
                preview_took.as_millis(),
                sections.join(", ")
            );
            for section in exif
                .sections
                .iter()
                .filter(|section| ["Camera", "Sensor"].contains(&section.name))
            {
                for entry in &section.entries {
                    println!("    {}: {}", entry.name, entry.value);
                }
            }
        }
    }

    #[test]
    fn signatures_are_read_at_their_offsets() {
        assert!(has_signature(b"II\x2a\x00\x10\x00\x00\x00CR\x02\x00"));
        assert!(has_signature(b"\0\0\0\x18ftypcrx \0\0\0\x01crx isom"));
        assert!(has_signature(b"FUJIFILMCCD-RAW 0201FF393103"));
        assert!(has_signature(b"IIU\0\x18\0\0\0"));
        assert!(has_signature(b"IIRO\x08\0\0\0"));
        assert!(has_signature(b"II\x2a\x00\xbc\x0aN\x01IIIICwaR"));
        assert!(!has_signature(b"II\x2a\x00\x08\x00\x00\x00"));
        assert!(!has_signature(b"\0\0\0\x18ftypheic"));
        assert!(!has_signature(b""));
    }
}
