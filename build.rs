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
//!
//! Where `GAMUT_LIBRAW_PREFIX` names a prefix `bin/macos-deps` built, the
//! library is found there alone and linked statically, as a Mac release is:
//! each archive the prefix holds goes into the binary, and only the
//! system's own libraries — the C++ library, zlib — are linked as they are.
//!
//! Beside the link, one C file is compiled against the library's own
//! header: `decode/raw/shim.c`, three accessors for the fields of
//! `libraw_data_t` that the C interface has none for and that sit too deep
//! in the struct to transcribe — the mosaic, and the black levels. Built
//! with the header the link found, so the offsets are the installed
//! version's whatever it is.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=GAMUT_LIBRAW_PREFIX");
    let prefix = std::env::var_os("GAMUT_LIBRAW_PREFIX").map(std::path::PathBuf::from);
    if let Some(prefix) = &prefix {
        // SAFETY: a build script is one thread, and nothing else reads the
        // environment while it is set.
        unsafe { std::env::set_var("PKG_CONFIG_LIBDIR", prefix.join("lib/pkgconfig")) };
    }
    let library = pkg_config::Config::new()
        .atleast_version("0.21")
        .statik(prefix.is_some())
        .cargo_metadata(false)
        .probe("libraw_r")
        .expect("LibRaw 0.21 or newer, found through pkg-config as libraw_r");
    let mac = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "macos");
    for path in &library.link_paths {
        println!("cargo:rustc-link-search=native={}", path.display());
    }
    for name in &library.libs {
        let name = if mac && name == "stdc++" { "c++" } else { name };
        // An archive the prefix built goes into the binary; anything else
        // is the system's, linked as it is.
        let archive = prefix.is_some()
            && library
                .link_paths
                .iter()
                .any(|path| path.join(format!("lib{name}.a")).exists());
        if archive {
            println!("cargo:rustc-link-lib=static={name}");
        } else {
            println!("cargo:rustc-link-lib={name}");
        }
    }
    // LibRaw reads a deflated DNG through zlib, which its static archive
    // needs and its pkg-config file does not name; every Mac has it.
    if prefix.is_some() {
        println!("cargo:rustc-link-lib=z");
    }
    println!("cargo:rerun-if-changed=src/image/decode/raw/shim.c");
    cc::Build::new()
        .file("src/image/decode/raw/shim.c")
        .includes(&library.include_paths)
        .warnings(true)
        // makepkg puts -flto in CFLAGS, which would leave only GCC's bytecode
        // in the archive, and Rust's linker, lld, reads none of it: the
        // accessors would go missing at the link. CFLAGS comes after the
        // flags given here, so -fno-lto would lose to it; fat objects carry
        // machine code beside the bytecode whichever order the two are in.
        .flag_if_supported("-ffat-lto-objects")
        .compile("gamut_libraw_shim");
}
