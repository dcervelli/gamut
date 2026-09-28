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

//!
//! The link lines are printed here rather than by `pkg-config` itself, for
//! one word in them: Homebrew's `libraw_r.pc` names the GNU C++ library,
//! `stdc++`, which a Mac does not have — its C++ library is `c++`.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let library = pkg_config::Config::new()
        .atleast_version("0.21")
        .cargo_metadata(false)
        .probe("libraw_r")
        .expect("LibRaw 0.21 or newer, found through pkg-config as libraw_r");
    let mac = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "macos");
    for path in &library.link_paths {
        println!("cargo:rustc-link-search=native={}", path.display());
    }
    for name in &library.libs {
        let name = if mac && name == "stdc++" { "c++" } else { name };
        println!("cargo:rustc-link-lib={name}");
    }
}
