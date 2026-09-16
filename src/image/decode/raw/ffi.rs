//! The part of LibRaw's C API this program calls, declared by hand.
//!
//! LibRaw is C++ with a C interface over it, and the interface is a handle
//! and a few dozen free functions. What is declared here is the dozen or so
//! the decoder uses: open, unpack, develop, read the result back, and the
//! settings that make the result linear. Everything else — the metadata,
//! the thumbnails, the callbacks — is left undeclared until something wants
//! it, since an `extern` block only has to say what it calls.
//!
//! Two structs are transcribed, because two facts have no accessor: the
//! orientation the camera recorded, which decides whether the picture is
//! taller than it is wide, and the color count. They sit at the front of
//! [`Data`], behind one pointer, and their layout is what `build.rs` pins the
//! library version for. [`super::Raw`] checks the transcription against the
//! accessors at run time as well, so a library whose layout has moved is an
//! error rather than a garbled size.
//!
//! `libraw_r` is the reentrant build: nothing global, so two threads can each
//! hold a handle.

#![allow(non_camel_case_types)]

use std::ffi::{c_char, c_float, c_int, c_uint, c_void};

/// `libraw_image_sizes_t`: the geometry `identify` works out from the
/// header. `flip` is the camera's orientation as dcraw spells it, and bit 2
/// of it means a quarter turn, which swaps the sides of the output.
#[repr(C)]
pub struct Sizes {
    pub raw_height: u16,
    pub raw_width: u16,
    pub height: u16,
    pub width: u16,
    pub top_margin: u16,
    pub left_margin: u16,
    pub iheight: u16,
    pub iwidth: u16,
    pub raw_pitch: c_uint,
    pub pixel_aspect: f64,
    pub flip: c_int,
    pub mask: [[c_int; 4]; 8],
    pub raw_aspect: u16,
    pub raw_inset_crops: [InsetCrop; 2],
}

/// `libraw_raw_inset_crop_t`.
#[repr(C)]
pub struct InsetCrop {
    pub cleft: u16,
    pub ctop: u16,
    pub cwidth: u16,
    pub cheight: u16,
}

/// `libraw_iparams_t`: what the header says the camera is.
#[repr(C)]
pub struct Params {
    pub guard: [c_char; 4],
    pub make: [c_char; 64],
    pub model: [c_char; 64],
    pub software: [c_char; 64],
    pub normalized_make: [c_char; 64],
    pub normalized_model: [c_char; 64],
    pub maker_index: c_uint,
    pub raw_count: c_uint,
    pub dng_version: c_uint,
    pub is_foveon: c_uint,
    pub colors: c_int,
    pub filters: c_uint,
    pub xtrans: [[c_char; 6]; 6],
    pub xtrans_abs: [[c_char; 6]; 6],
    pub cdesc: [c_char; 5],
    pub xmplen: c_uint,
    pub xmpdata: *mut c_char,
}

/// The front of `libraw_data_t`, as far as the two structs above. The real
/// struct goes on for kilobytes past this, so one of these is only ever
/// looked at through the pointer [`libraw_init`] hands back, never made.
#[repr(C)]
pub struct Data {
    pub image: *mut [u16; 4],
    pub sizes: Sizes,
    pub idata: Params,
}

/// `libraw_processed_image_t`: what `dcraw_make_mem_image` hands back. The
/// pixels follow `data_size` in the same allocation, `data` being the first
/// byte of them.
#[repr(C)]
pub struct Processed {
    pub kind: c_int,
    pub height: u16,
    pub width: u16,
    pub colors: u16,
    pub bits: u16,
    pub data_size: c_uint,
    pub data: [u8; 1],
}

/// `LIBRAW_IMAGE_BITMAP`: [`Processed`] holds pixels rather than a JPEG.
pub const IMAGE_BITMAP: c_int = 2;

/// `LIBRAW_SUCCESS`.
pub const SUCCESS: c_int = 0;

/// The demosaic `user_qual` codes: AHD is dcraw's default and the one every
/// other developer compares itself to.
pub const DEMOSAIC_AHD: c_int = 3;

/// `output_color`, dcraw's `-o` numbering: 0 is the camera's own space, 1
/// sRGB, 2 Adobe RGB, 3 Wide Gamut, 4 ProPhoto, 5 XYZ, 6 ACES, 7 DCI-P3, 8
/// Rec. 2020.
pub const OUTPUT_REC2020: c_int = 8;

#[link(name = "raw_r")]
unsafe extern "C" {
    pub fn libraw_init(flags: c_uint) -> *mut Data;
    pub fn libraw_close(data: *mut Data);
    pub fn libraw_strerror(code: c_int) -> *const c_char;

    pub fn libraw_open_buffer(data: *mut Data, buffer: *const c_void, size: usize) -> c_int;
    pub fn libraw_unpack(data: *mut Data) -> c_int;
    pub fn libraw_dcraw_process(data: *mut Data) -> c_int;
    pub fn libraw_dcraw_make_mem_image(data: *mut Data, code: *mut c_int) -> *mut Processed;
    pub fn libraw_dcraw_clear_mem(image: *mut Processed);

    pub fn libraw_set_demosaic(data: *mut Data, value: c_int);
    pub fn libraw_set_output_color(data: *mut Data, value: c_int);
    pub fn libraw_set_output_bps(data: *mut Data, value: c_int);
    pub fn libraw_set_gamma(data: *mut Data, index: c_int, value: c_float);
    pub fn libraw_set_no_auto_bright(data: *mut Data, value: c_int);
    pub fn libraw_set_user_mul(data: *mut Data, index: c_int, value: c_float);

    pub fn libraw_get_iwidth(data: *mut Data) -> c_int;
    pub fn libraw_get_iheight(data: *mut Data) -> c_int;
    pub fn libraw_get_cam_mul(data: *mut Data, index: c_int) -> c_float;
    pub fn libraw_get_iparams(data: *mut Data) -> *mut Params;
}
