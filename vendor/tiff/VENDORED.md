# tiff 0.11.3, vendored

The `tiff` crate as published on crates.io at 0.11.3 —
<https://github.com/image-rs/image-tiff>, MIT, `LICENSE` beside this file —
with one change: `Cargo.toml` depends on `weezl` 0.2.1 rather than 0.1.10,
as upstream's commit `0fdf1e0` does on its main branch. Its `src/` is
unchanged. The examples, tests and benches are left out, and their entries
with them.

Why, and when it goes: see the `[patch.crates-io]` entry in the
workspace's `Cargo.toml`.
