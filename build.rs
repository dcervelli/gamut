//! Finds the system LibRaw, the one library `decode::raw` links.
//!
//! `libraw_r` is the reentrant build, in which every call works on the
//! handle it is given and nothing is global: the loader and the thumbnailer
//! each decode on a thread of their own, at the same time.
//!
//! The floor is 0.21, where `libraw_image_sizes_t` took the shape
//! `decode::raw::ffi` transcribes. An older library would still link and
//! then be read through the wrong layout, which is exactly what a build-time
//! check is for. `decode::raw` checks the layout again at run time, against
//! what the library's own accessors say.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    pkg_config::Config::new()
        .atleast_version("0.21")
        .probe("libraw_r")
        .expect("LibRaw 0.21 or newer, found through pkg-config as libraw_r");
}
