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
//! milliseconds where developing the frame takes hundreds; and, when the
//! viewer asks for the camera's rendering rather than ours, the picture
//! itself. [`super::Decoder::camera_jpeg`] says whether there is one and
//! how large, from the JPEG's header, so that the window can offer it.
//!
//! The file is read whole and handed over as one buffer. LibRaw reads by
//! seeking about a stream, and a buffer is the one form of stream its C
//! interface takes; a raw is tens of megabytes, which is a few milliseconds
//! of copying beside the few hundred the demosaic takes. Read once per open
//! file, though: the handle over the buffer is kept with the source between
//! the questions a load asks — the size, the camera's JPEG, the panel's
//! facts — and only developing the frame spends it.

mod ffi;

use std::cell::Cell;
use std::ffi::CStr;
use std::os::raw::c_int;

use anyhow::{Result, anyhow, bail};

use super::{Kept, ReadSeek};
use crate::image::exif::Entry;
use crate::image::{
    AlphaMode, Channels, ColorSpace, DecodedImage, Primaries, Referred, Samples, Transfer,
};

pub struct Raw;

impl super::Decoder for Raw {
    fn name(&self) -> &'static str {
        "camera raw"
    }

    fn facts(&self, source: &mut dyn super::ReadSeek) -> Result<Option<Vec<Entry>>> {
        facts(source).map(Some)
    }

    /// A raw that is a TIFF at the front keeps its packet in the directory,
    /// as a TIFF does. The containers that are not — an ORF or RW2 under
    /// its own letters, a RAF, an MRW, a CR3 — are not walked for one.
    fn xmp(&self, source: &mut dyn super::ReadSeek) -> Result<Option<Vec<u8>>> {
        let mut head = [0u8; 4];
        source.rewind()?;
        if super::fill(source, &mut head)? < head.len()
            || crate::image::tiff::header(&head).is_none()
        {
            return Ok(None);
        }
        Ok(crate::image::directory::packet(source))
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

    /// Which camera format, where the leading bytes say: the container's
    /// own signature, or the make in a TIFF-shaped raw's first directory.
    fn format(&self, header: &[u8]) -> &'static str {
        raw_format(header).unwrap_or_else(|| self.name())
    }

    fn dimensions(&self, source: &mut dyn super::ReadSeek) -> Result<Option<(u32, u32)>> {
        with_handle(source, |handle| {
            // A camera whose photosites are not square has its picture
            // stretched to fix that on the way out, and the header does not
            // say by how much. Rare — a handful of cameras from the early
            // 2000s — and the window takes its default shape rather than a
            // wrong one.
            if handle.sizes().pixel_aspect != 1.0 {
                return Ok(None);
            }
            Ok(Some(handle.output_size()))
        })
    }

    fn preview(
        &self,
        source: &mut dyn super::ReadSeek,
        overrides: super::Overrides,
    ) -> Result<Option<DecodedImage>> {
        with_handle(source, |handle| {
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
            // The JPEG is stored as the sensor saw the scene, with the way
            // the camera was held beside it; the developed picture is turned
            // to match, so the preview is too — by this orientation alone,
            // since a preview that repeats the tag in an EXIF of its own, as
            // a RAF's does, would otherwise be turned twice.
            let image = crate::image::orient::apply(image, handle.orientation());
            Ok(Some(image))
        })
    }

    fn camera_jpeg(&self, source: &mut dyn super::ReadSeek) -> Result<super::CameraJpeg> {
        with_handle(source, |handle| {
            if !handle.unpack_thumbnail()? {
                return Ok(super::CameraJpeg::Missing);
            }
            // Copied out but not decoded: the JPEG's header says its size,
            // and a bitmap's is in the library's own header.
            let thumbnail = handle.make_thumbnail()?;
            let (width, height) = match thumbnail.kind() {
                ffi::IMAGE_JPEG => super::jpeg::stored_size(thumbnail.bytes())?,
                ffi::IMAGE_BITMAP => {
                    let header = thumbnail.header();
                    (u32::from(header.width), u32::from(header.height))
                }
                other => bail!("LibRaw handed back a preview of kind {other}"),
            };
            let (width, height) = crate::image::orient::size(width, height, handle.orientation());
            Ok(super::CameraJpeg::Present([width, height]))
        })
    }

    fn decode(
        &self,
        source: &mut dyn super::ReadSeek,
        _overrides: super::Overrides,
    ) -> Result<DecodedImage> {
        // Taken rather than borrowed: developing the frame rewrites the
        // handle's multipliers, so it could not answer for the camera's
        // balance again, and is spent here.
        let handle = take_handle(source)?;
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
/// camera and the exposure as the library parsed them from the maker's own
/// block, as the rows the panel's `Camera` and `Exposure` sections are made
/// of, for the files the EXIF reader gets nothing or too little out of — a
/// CRW has no EXIF at all, and a Phase One's names the camera and stops —
/// which the panel takes whatever the EXIF did not say from; and the color
/// temperature the camera balanced for, which only the library's reading
/// of the sensor can work out.
pub(super) fn facts(source: &mut dyn super::ReadSeek) -> Result<Vec<Entry>> {
    with_handle(source, |handle| {
        let mut rows = handle.camera();
        push(
            &mut rows,
            crate::image::exif::COLOR_TEMPERATURE,
            handle.color_temperature(),
        );
        Ok(rows)
    })
}

/// Asks `ask` of the LibRaw handle over `source`: the one kept with the
/// source by an earlier question, where the source keeps anything, or one
/// opened now — and left with the source afterwards, for the next question.
/// Opening reads the whole file, so a load that asks a raw three things
/// reads it once.
fn with_handle<T>(source: &mut dyn ReadSeek, ask: impl FnOnce(&Handle) -> Result<T>) -> Result<T> {
    let handle = take_handle(source)?;
    let answer = ask(&handle);
    if let Some(kept) = source.kept() {
        kept.put(handle);
    }
    answer
}

/// The handle over `source`, out of the source's keeping or opened now,
/// for a question that spends it.
fn take_handle(source: &mut dyn ReadSeek) -> Result<Handle> {
    match source.kept().and_then(Kept::take::<Handle>) {
        Some(handle) => Ok(handle),
        None => Handle::open(source),
    }
}

/// One LibRaw handle over one file's bytes. The bytes live here because the
/// handle reads them for as long as it is open, and it is closed on drop.
struct Handle {
    data: *mut ffi::Data,
    _bytes: Vec<u8>,
    /// What `unpack_thumb` answered, once it has: the library refuses to be
    /// asked twice, and a handle kept between questions is.
    thumbnail: Cell<Option<bool>>,
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
            thumbnail: Cell::new(None),
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
    /// `Camera` section for a file the EXIF reader found nothing in, in the
    /// EXIF reader's order and under its names, so that either fills in
    /// what the other left out a row at a time.
    fn camera(&self) -> Vec<Entry> {
        use crate::image::exif::{APERTURE, CAMERA, FOCAL_LENGTH, ISO, LENS, SHUTTER, TAKEN};
        let mut rows = Vec::new();
        let params = self.params();
        let other = self.other();
        if other.timestamp > 0 {
            // dcraw makes the camera's date a `time_t` as if it were in this
            // machine's zone, so the same zone gives the camera's fields back.
            let taken = crate::clock::local(
                std::time::UNIX_EPOCH + std::time::Duration::from_secs(other.timestamp as u64),
            );
            push(
                &mut rows,
                TAKEN,
                Some(format!(
                    "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                    taken.year, taken.month, taken.day, taken.hour, taken.minute, taken.second
                )),
            );
        }
        push(
            &mut rows,
            CAMERA,
            crate::image::exif::camera_name(text(&params.make), text(&params.model)),
        );
        push(&mut rows, LENS, text(&self.lens().lens));

        if other.shutter > 0.0 {
            let shutter = if other.shutter < 1.0 {
                format!("1/{} s", tidy((1.0 / other.shutter).round()))
            } else {
                format!("{} s", tidy(other.shutter))
            };
            push(&mut rows, SHUTTER, Some(shutter));
        }
        if other.aperture > 0.0 {
            // To a tenth, which is how an f-number is spoken; the maker's
            // block holds it to more places than the lens was ever set to.
            let aperture = format!("{:.1}", other.aperture);
            push(
                &mut rows,
                APERTURE,
                Some(format!("f/{}", aperture.trim_end_matches(".0"))),
            );
        }
        if other.iso_speed > 0.0 {
            push(&mut rows, ISO, Some(tidy(other.iso_speed)));
        }
        if other.focal_len > 0.0 {
            push(
                &mut rows,
                FOCAL_LENGTH,
                Some(format!("{} mm", tidy(other.focal_len))),
            );
        }
        rows
    }

    /// The color temperature the camera's balance is for, to the nearest
    /// 50 K: the light a gray surface would have been under for these
    /// multipliers to make it gray.
    ///
    /// A gray surface reads, on the sensor, as the inverse of the
    /// multipliers that balance it. LibRaw's matrix takes the sensor's
    /// values to sRGB once they are scaled by its own daylight multipliers,
    /// so that surface, scaled that way and put through it, is the
    /// illuminant's color in sRGB — and from there in CIE xy, where
    /// McCamy's cubic gives the correlated color temperature. Only for a
    /// sensor of three colors, and only where the answer lands on the part
    /// of the locus the cubic holds for.
    fn color_temperature(&self) -> Option<String> {
        if self.params().colors != 3 {
            return None;
        }
        // SAFETY: accessors on an open handle.
        let (camera, daylight, matrix) = unsafe {
            let each = |get: unsafe extern "C" fn(*mut ffi::Data, c_int) -> f32| -> [f64; 3] {
                [0, 1, 2].map(|index| f64::from(get(self.data, index)))
            };
            let mut matrix = [[0.0f64; 3]; 3];
            for (row, values) in matrix.iter_mut().enumerate() {
                for (column, value) in values.iter_mut().enumerate() {
                    *value = f64::from(ffi::libraw_get_rgb_cam(
                        self.data,
                        row as c_int,
                        column as c_int,
                    ));
                }
            }
            (
                each(ffi::libraw_get_cam_mul),
                each(ffi::libraw_get_pre_mul),
                matrix,
            )
        };
        let kelvin = correlated_temperature(camera, daylight, matrix)?;
        Some(format!("{} K", (kelvin / 50.0).round() as u32 * 50))
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
        let exif = match self.sizes().flip {
            3 => 3,
            5 => 8,
            6 => 6,
            _ => 1,
        };
        crate::image::orient::from_tag(Some(exif))
    }

    /// Reads the preview out of the file, saying whether there was one;
    /// asked again, says what it said.
    fn unpack_thumbnail(&self) -> Result<bool> {
        if let Some(answered) = self.thumbnail.get() {
            return Ok(answered);
        }
        // SAFETY: an open handle.
        let code = unsafe { ffi::libraw_unpack_thumb(self.data) };
        let answer = match code {
            ffi::SUCCESS => true,
            ffi::NO_THUMBNAIL | ffi::UNSUPPORTED_THUMBNAIL => false,
            other => return Err(anyhow!("reading the preview: {}", describe(other))),
        };
        self.thumbnail.set(Some(answer));
        Ok(answer)
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
            // White is the level the file states, never the brightest pixel
            // in this frame. LibRaw's default lowers it to that pixel where
            // it is within about 0.4 EV of the stated one, so a frame with no
            // highlight near clipping would come out brighter than the same
            // scene with one: a normalization of each frame to itself, which
            // is the brightening this development promises not to do.
            ffi::libraw_set_adjust_maximum_thr(self.data, 0.0);

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

/// The correlated color temperature of the light `camera`'s multipliers
/// balance for, given LibRaw's `daylight` multipliers and its matrix from
/// the sensor, scaled by them, to linear sRGB; see
/// [`Handle::color_temperature`]. `None` where the multipliers are missing
/// or the answer is off the stretch of the locus McCamy's cubic follows.
fn correlated_temperature(
    camera: [f64; 3],
    daylight: [f64; 3],
    matrix: [[f64; 3]; 3],
) -> Option<f64> {
    if camera
        .iter()
        .chain(&daylight)
        .any(|value| value.is_nan() || *value <= 0.0)
    {
        return None;
    }
    let gray: [f64; 3] = std::array::from_fn(|c| daylight[c] / camera[c]);
    let rgb: [f64; 3] = std::array::from_fn(|row| {
        (0..3)
            .map(|column| matrix[row][column] * gray[column])
            .sum()
    });
    // Linear sRGB to CIE XYZ, under D65.
    const XYZ: [[f64; 3]; 3] = [
        [0.4124, 0.3576, 0.1805],
        [0.2126, 0.7152, 0.0722],
        [0.0193, 0.1192, 0.9505],
    ];
    let [x, y, z]: [f64; 3] =
        std::array::from_fn(|row| (0..3).map(|column| XYZ[row][column] * rgb[column]).sum());
    let sum = x + y + z;
    if sum.is_nan() || sum <= 0.0 {
        return None;
    }
    let (x, y) = (x / sum, y / sum);
    let n = (x - 0.3320) / (0.1858 - y);
    let kelvin = 449.0 * n.powi(3) + 3525.0 * n.powi(2) + 6823.3 * n + 5520.33;
    (2000.0..=12500.0).contains(&kelvin).then_some(kelvin)
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

/// Every answer [`raw_format`] gives, and the decoder's own name for the
/// rest: what a file this decoder claims may be called.
#[cfg(test)]
const FORMATS: &[&str] = &[
    "camera raw",
    "cr2",
    "cr3",
    "crw",
    "orf",
    "rw2",
    "raf",
    "mrw",
    "iiq",
    "dng",
    "nef",
    "arw",
    "pef",
    "srw",
    "3fr",
    "dcr",
    "erf",
    "mos",
];

/// Which camera format the leading bytes say a raw is — the container's own
/// signature where it has one, then a DNG's tag, then the make written in a
/// TIFF-shaped raw's first directory, which is how NEF, ARW, PEF and SRW
/// tell themselves apart — or `None` where they say no more than "a raw".
/// Named by the extension the camera writes, in the case the info panel
/// and the file list use for every format.
fn raw_format(header: &[u8]) -> Option<&'static str> {
    let at = |offset: usize, magic: &[u8]| header.get(offset..offset + magic.len()) == Some(magic);
    if at(8, b"CR\x02") {
        return Some("cr2");
    }
    if at(4, b"ftyp") && at(8, b"crx ") {
        return Some("cr3");
    }
    if at(6, b"HEAPCCDR") {
        return Some("crw");
    }
    if at(0, b"IIRO") || at(0, b"IIRS") || at(0, b"MMOR") {
        return Some("orf");
    }
    if at(0, b"IIU\0") {
        return Some("rw2");
    }
    if at(0, b"FUJIFILMCCD-RAW") {
        return Some("raf");
    }
    if at(0, b"\0MRM") {
        return Some("mrw");
    }
    if at(8, b"IIII") || at(8, b"MMMM") {
        return Some("iiq");
    }
    let directory = Directory::first(header)?;
    if directory.has(0xC612) {
        return Some("dng");
    }
    let make = directory.ascii(0x010F)?.to_ascii_uppercase();
    Some(match make.as_str() {
        make if make.starts_with("NIKON") => "nef",
        make if make.starts_with("SONY") => "arw",
        make if make.starts_with("PENTAX") || make.starts_with("RICOH") => "pef",
        make if make.starts_with("SAMSUNG") => "srw",
        make if make.starts_with("HASSELBLAD") => "3fr",
        make if make.starts_with("KODAK") => "dcr",
        make if make.starts_with("EPSON") => "erf",
        make if make.starts_with("LEAF") || make.starts_with("MAMIYA") => "mos",
        _ => return None,
    })
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

    /// An `ASCII` entry's text — inline for four bytes or fewer, and at
    /// its offset otherwise — as far as the header reaches it. `None` for
    /// text the header does not reach.
    fn ascii(&self, tag: u16) -> Option<&'a str> {
        let (_, kind, count, at) = self.entries().find(|(found, ..)| *found == tag)?;
        if kind != 2 {
            return None;
        }
        let count = count as usize;
        let start = if count <= 4 {
            at
        } else {
            self.u32(at)? as usize
        };
        let text = std::str::from_utf8(self.header.get(start..start + count)?).ok()?;
        Some(text.trim_end_matches('\0').trim())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::decode::Decoder;

    /// A raw is named for what its leading bytes say it is: the
    /// container's signature, a DNG's tag, or the make in a TIFF-shaped
    /// raw's directory; and "camera raw" where they say no more than that.
    #[test]
    fn a_raw_is_named_by_its_signature_its_tag_or_its_make() {
        assert_eq!(raw_format(b"IIRO\x08\x00\x00\x00"), Some("orf"));
        assert_eq!(raw_format(b"FUJIFILMCCD-RAW 0201"), Some("raf"));
        assert_eq!(
            raw_format(b"II\x2a\x00\x10\x00\x00\x00CR\x02\x00"),
            Some("cr2")
        );
        assert_eq!(raw_format(&tiff(&[(0xC612, 1)])), Some("dng"));
        // A Make entry: ASCII, longer than four bytes, so at an offset —
        // right after the directory and its terminating offset.
        let mut nef = b"II\x2a\x00\x08\x00\x00\x00\x01\x00".to_vec();
        nef.extend(0x010Fu16.to_le_bytes());
        nef.extend(2u16.to_le_bytes());
        nef.extend(18u32.to_le_bytes());
        nef.extend(26u32.to_le_bytes());
        nef.extend(0u32.to_le_bytes());
        nef.extend(b"NIKON CORPORATION\0");
        assert_eq!(raw_format(&nef), Some("nef"));
        let mut other = nef.clone();
        other.splice(26.., b"CASIO COMPUTER CO.\0".iter().copied());
        assert_eq!(raw_format(&other), None);
        assert_eq!(Raw.format(&other), Raw.name());
        assert_eq!(
            raw_format(&tiff(&[(0x0106, 32803)])),
            None,
            "a CFA and nothing else"
        );
        for format in FORMATS {
            assert!(
                format
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == ' ')
            );
        }
    }

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

    /// The fixture's header, as the panel will read it: the camera the
    /// generator named, and the daylight its balance of ones is for; no
    /// exposure, since nothing took the picture.
    #[test]
    fn the_fixture_reports_its_camera() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_images")
            .join("dng-cfa.dng");
        let mut source = std::io::BufReader::new(std::fs::File::open(path).unwrap());
        let rows = facts(&mut source).unwrap();
        let rows: Vec<(&str, &str)> = rows
            .iter()
            .map(|entry| (entry.name.as_str(), entry.value.as_str()))
            .collect();
        // Balanced for the daylight the generator's neutral stands for.
        assert_eq!(
            rows,
            [("Camera", "gamut fixture"), ("Color temperature", "6500 K")]
        );
    }

    /// A frame whose brightest photosite is short of the stated white level
    /// develops with that photosite short of white, rather than stretched
    /// up to it: the fixture's counts turned down to nine tenths of white
    /// come out at nine tenths, where LibRaw's default would make them one.
    #[test]
    fn white_is_the_stated_level_not_the_brightest_pixel() {
        const WHITE: u16 = 4095;
        const DIM: u16 = 3686;
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_images")
            .join("dng-cfa.dng");
        let mut bytes = std::fs::read(fixture).unwrap();
        // The counts are the file's last 32 × 24 sixteen-bit words.
        let start = bytes.len() - 32 * 24 * 2;
        for word in bytes[start..].as_chunks_mut::<2>().0 {
            if u16::from_le_bytes(*word) == WHITE {
                *word = DIM.to_le_bytes();
            }
        }
        let path = std::env::temp_dir().join(format!("gamut-raw-dim-{}.dng", std::process::id()));
        std::fs::write(&path, &bytes).unwrap();
        let image = crate::image::decode::load(&path, super::super::Overrides::default());
        std::fs::remove_file(&path).unwrap();
        let image = image.unwrap();
        let Samples::U16 { data, .. } = &image.samples else {
            panic!("a raw develops to sixteen bits");
        };
        // The middle of the white quadrant, away from the edges where the
        // demosaic overshoots.
        let at = (18 * image.width as usize + 24) * 3;
        let expected = f32::from(DIM) / f32::from(WHITE);
        for sample in &data[at..at + 3] {
            let value = f32::from(*sample) / 65535.0;
            assert!(
                (value - expected).abs() < 0.01,
                "the white quadrant comes out at {value}, not {expected}"
            );
        }
    }

    /// The camera's balance at LibRaw's own daylight is daylight, D65, at
    /// about 6500 K; a balance with more red and less blue than that is for
    /// bluer light, a higher temperature; and multipliers that are missing
    /// are no temperature.
    #[test]
    fn multipliers_come_to_a_color_temperature() {
        let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let daylight = [2.0, 1.0, 1.5];
        let d65 = correlated_temperature(daylight, daylight, identity).unwrap();
        assert!((d65 - 6504.0).abs() < 10.0, "{d65}");
        let shade = correlated_temperature([2.4, 1.0, 1.2], daylight, identity).unwrap();
        assert!(shade > d65, "{shade}");
        let tungsten = correlated_temperature([1.2, 1.0, 2.6], daylight, identity).unwrap();
        assert!(tungsten < 4000.0, "{tungsten}");
        assert_eq!(
            correlated_temperature([0.0, 1.0, 1.0], daylight, identity),
            None
        );
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
            let format = reader(&path);
            assert!(
                format.is_some_and(|format| FORMATS.contains(&format)),
                "{name} is not recognized as a raw: {format:?}"
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
            // Said from the JPEG's header, it is the size the JPEG decodes to.
            assert_eq!(
                crate::image::decode::camera_jpeg(&path)
                    .unwrap_or_else(|error| panic!("{name}: {error:#}")),
                crate::image::decode::CameraJpeg::Present([small.width, small.height]),
                "{name}"
            );
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
                .map(|section| format!("{} ({})", section.group.name(), section.entries.len()))
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
                .filter(|section| section.group <= crate::image::exif::Group::Exposure)
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
