# Licensing and provenance

Dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](../LICENSE-APACHE))
- MIT license ([LICENSE-MIT](../LICENSE-MIT))

at your option. Unless you state otherwise, any contribution you intentionally
submit for inclusion in this work shall be dual-licensed as above, without any
additional terms or conditions.

Two things in the tree are someone else's work, and are licensed as they say
rather than as above:

- The marks the buttons wear are [Lucide](https://lucide.dev)'s geometry,
  redescribed in `src/ui/icon.rs` as a table of strokes on Lucide's own
  24-unit grid. Lucide is ISC-licensed.
- The false-color ramps are polynomial fits rather than sampled tables, so
  what is borrowed is the fit. Viridis and magma are
  [Matt Zucker's](https://www.shadertoy.com/view/WlfXRN) fits to matplotlib's,
  under CC0; turbo's is
  [Google's](https://gist.github.com/mikhailov-work/0d177465a8151eb6ede1768d51d476c7)
  fit to its own, under Apache-2.0. Both appear twice, in
  `src/image/display.rs` and `src/render/shaders/image.wgsl`, because the
  readout and the screen have to agree.

Which is recorded file by file in [REUSE.toml](../REUSE.toml), in the form the
[REUSE](https://reuse.software) specification defines, with the license texts
it names in [LICENSES/](../LICENSES). `reuse lint` checks that nothing in the
tree is unaccounted for, and CI runs it.

The crates linked into the binary are the third part, and they are not in the
tree at all — `Cargo.lock` only names them. Their notices are collected in
[THIRD-PARTY-NOTICES](../THIRD-PARTY-NOTICES), generated from that lock by
`cargo about` and rewritten by `bin/release`, because a statically linked
binary carries its dependencies' code and owes their notices with it. Which
licenses may turn up there is not left open: `about.toml` lists the ones this
project accepts, in priority order, and a crate arriving under anything else
fails generation rather than being written out quietly — `cargo audit` for
licenses instead of advisories. The file is stamped with the lock it was
generated from, and CI fails if that is not the lock in the tree. It is what
keeps egui's bundled fonts out of the binary: they arrive under font licenses
the list does not carry, so the feature that would compile them in is left
off and the interface is set in the desktop's own faces instead, which it
would have been anyway.

It is a generated file kept in the tree, which is the one place this project
does that, so the reason is worth saying. Generation is offline and
deterministic — `--frozen` implies `--offline`, every crate in the graph
ships its own license text, and the same lock gives the same bytes — so the
PKGBUILD could perfectly well produce it during the build, as it already does
the manual page. What stops it is that `cargo about` is packaged neither in
Arch's repositories nor in the AUR: building it there would mean fetching the
tool from crates.io first, unpinned and over the network, inside a build that
is otherwise `--frozen`. Committing the file is what keeps the package
buildable with cargo and nothing else. Being deterministic, it can be
verified rather than trusted:

```sh
cargo about generate --frozen packaging/about.hbs | diff - <(tail -n +3 THIRD-PARTY-NOTICES)
```

Between the three, `LICENSES/` ends up holding every license anything in the
distribution is under, and `reuse lint` reports none of them unused.
