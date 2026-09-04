# gamut

A GPU image viewer: Rust, `winit`, `wgpu`, `glyphon`. The README explains the
design and the reasoning behind it; this file is the map for making changes.

## Layout

Dependencies point downward only. Nothing in a lower layer imports a higher one.

```
main.rs        mod decls, event-loop bootstrap
cli.rs         argument parsing; usage() renders the key sections from app::input::KEYS
app/           the event loop's state and winit handlers
  mod.rs         App: window, renderer, loader, apply/deliver/redraw, ApplicationHandler impl
  files.rs       Files: the file list, the read in flight, walks past broken files (pure, tested)
  input.rs       Action, KEYS table, Effect; perform() is where every key's action happens; pointer handling
  window.rs      opening size, titles
ui/            builds each frame's display list; no wgpu or winit imports
  mod.rs         Current, Panels, FrameInput, build_frame(), backdrop()
  chrome.rs      the four panels and their buttons; content_area(), image_viewport()
  layers.rs      which layer the pointer is on: Hit, hit() — one answer for hover, press, wheel and readout
  histogram.rs / minimap.rs / grid.rs / buttons.rs   one widget each
  icon.rs        the marks a button wears: Lucide's geometry on its own
                 24-unit grid, sized and placed in whole device pixels so
                 strokes stay sharp and evenly spaced marks stay even
  info.rs        the file's own facts, in a column that scrolls
  pixel.rs       the pointer's readout: coordinate, stored and mapped values, swatch
  menu.rs        Menu (which popup is open), the zoom menu's choices, and how its cells are drawn
  tooltip.rs     the label naming what the pointer is resting on: Tip is what can
                 have one, Tooltips is when it opens, Tips is where it goes —
                 in the content area, on the layer nothing covers
  status.rs      the words in the top and bottom bars; top_bar() lays the top one
                 out for the frame builder and for the pointer alike
theme/         palette.rs reads Omarchy's colors.toml and resolves its cascade; mod.rs derives Theme's color roles
view.rs        zoom / pan / fit geometry, pure maths (View, Viewport, Fit)
listing.rs     what a path on the command line stands for: a directory is the
               images inside it, read again while the program runs
loader.rs      the decode + upload thread; replies arrive as winit user events; a
               paste is fetched here too, on its way to being read
watch.rs       polling a file — or a directory — for a settled change
monitor.rs     what the compositor says each monitor is in, SDR or HDR, over a Wayland connection of its own
clipboard.rs   putting text or a file: URI on the clipboard, in a process that outlives
               the window; and reading a pasted picture off it
pasted.rs      where a pasted picture is written and what it is called: the
               XDG pictures directory, and a name nothing else holds
clock.rs       a moment as a date and time — UTC, or the zone the system's own
               compiled zone file says it is in
timing.rs      startup instrumentation
image/         the data model, nothing GPU
  mod.rs         Channels, Samples, AlphaMode, Referred (graded or measured light), DecodedImage,
                 Sample (one pixel read back)
  color/         Transfer, Primaries, ColorSpace; icc.rs and cicp.rs translate what files say into them
  stats.rs       the scan an image gets on load: min/max, histogram, plot
  encode.rs      the displayed image walked back out to an 8-bit sRGB PNG, for the clipboard
  exif.rs        the file's own metadata, read and rendered for the info panel
  geo.rs         GeoTIFF's keys: where a raster's pixels are on the ground
  directory.rs   a TIFF directory the metadata reader cannot reach, rewritten
                 as a block it can: BigTIFF, or a directory past the prefix
  display.rs     Display: window, exposure, tone map, colormap — uniform state, never re-decodes;
                 map() and the CPU twins of the shaders' tone curves and colormaps
  decode/        Decoder trait + DECODERS registry in mod.rs; one file per format; limits.rs the size ceiling;
                 dynamic.rs the shared DynamicImage bridge; fixture_tests.rs runs every file in test_images/
render/        the GPU
  mod.rs         Renderer: surface, device, the three passes; Scene is what a frame draws; TextMeasure trait
  placement.rs   Placement (where the image lands) and Upscale (the magnification filter)
  upload.rs      texture format choice and the transfer-function LUTs; the "sampled texel is linear" invariant
  image_layer.rs / reduce.rs / composite.rs / ui_layer/   the passes; ui_layer holds Rect, Color, UiFrame — its
               quads fill or stroke a rounded box at any angle, and snap/stroke_center_in_device put one on the device grid —
               and popup.rs (sections of cells anchored to a corner)
  shader_codes.rs  every Rust<->WGSL integer code, one fn per shader switch
  gpu.rs         wgpu boilerplate helpers (layouts, uniform buffers, full-screen pipelines, GrowableBuffer)
  shaders/       WGSL; each Params struct is mirrored by a #[repr(C)] struct in the .rs file that loads it
packaging/     what an Arch package is built from: PKGBUILD, the .desktop entry,
               the icon, and hand-written shell completions
bin/           release, and pkgbuild-sha which points the PKGBUILD at a published tag
REUSE.toml     which file in the tree is under what license; LICENSES/ holds the
               texts it names, and `reuse lint` checks the two agree
about.toml     which licenses a dependency may arrive under; packaging/about.hbs
               renders THIRD-PARTY-NOTICES, the notices of every linked crate,
               which bin/release regenerates and stamps with the lock it read
```

`main.rs::APP_ID` is the crate's own name, and everything outside the program
that has to agree on one — the Wayland app_id, the desktop entry, the icon,
the package — is named from it. `cli.rs`'s tests check that they still do, so
renaming the program is editing `Cargo.toml` and moving the files those tests
name.

## Where to make a change

| Change | Edit |
| --- | --- |
| A key binding | `app/input.rs`: one `KEYS` entry, with the `mods` it is held with, and one `perform` arm. `--help` follows. |
| A status-bar segment | `ui/status.rs`; the pointer's pixel readout is `ui/pixel.rs` |
| What a pixel reads as under the pointer | `image/mod.rs::sample` for what the file holds, `image/display.rs::map` for what the screen shows |
| What something is called when the pointer rests on it | a `Tip` variant in `ui/tooltip.rs` and one `tips.offer(tip, rect)` beside where it is drawn; `App::tooltip` composes the words, from `KEYS` by way of `action_of` wherever a key does the same job, so a tooltip and `--help` cannot disagree. Words of its own go in `ui/tooltip.rs::words`, and a menu cell's in `Menu::cell_tip`. A thing that wants its label somewhere other than under it offers with `offer_toward`: the histogram panel's toggles open `Opens::Right`, across the plot they act on. When it opens is `Tooltips`, held by `App` and asked in `update_hover` and `about_to_wait` |
| A button's icon | `ui/icon.rs`: one `&[Mark]` on the 24-unit grid, and one `icon::draw` call where the button is drawn. The caller sets aside a budget; whether the mark comes out sharp is `UiFrame::stroke_center_in_device`'s business and whether its spacing stays even is `icon::fit`'s |
| A panel or overlay | a new `ui/<name>.rs` and one call in `ui/mod.rs::build_frame`; if the pointer can be on it, a `Hit` variant and one test in `ui/layers.rs` at the same height in the stack it is drawn at; a new color role goes in `theme/mod.rs` |
| What the info panel says about a file | `ui/info.rs` for the layout; the file's own facts are gathered in `app/mod.rs::file_facts`, its metadata in `image/exif.rs`, and its georeference in `image/geo.rs` |
| A popup menu | a `Menu` variant in `ui/menu.rs` with its choices, `sections`/`grid`/`choose` arms and a `draw` arm; `Chrome::popup` places it, `App::press` opens it, and `ui/layers.rs` puts it over everything |
| A CLI flag | `cli.rs`, and the `Options` / `Startup` / `Overrides` field it sets |
| What a path on the command line stands for | `listing.rs`; `App::poll_directories` notices a named directory changing and `app/files.rs::relist` takes the new list in |
| Anything a user installs — the desktop entry, the icon, a completion, the man page | `packaging/`; the man page is generated by `cli.rs::man()` from the same `OPTIONS` and `KEYS` as `--help`, so it is never edited directly |
| An image format | `image/decode/<fmt>.rs` implementing `Decoder`, one line in `DECODERS`, a fixture in `test_images/` (see its README and `generate.sh`) |
| What a copy of the image contains | `image/encode.rs`; the chord that asks for it is in `app/input.rs` |
| What a paste accepts, or where it is written | `clipboard.rs::IMAGE_TYPES` for the MIME types and the extensions they are saved under, `pasted.rs` for the directory and the name; `App::paste` starts it and `app/files.rs::adopt` puts it in the list. Whether the button for it is on screen is `Panels::paste`, looked at by `App::poll_clipboard` |
| A color-space source (a new tag a format carries) | `image/color/` |
| Someone else's work brought into the tree | say where it came from beside the code that carries it, then one `[[annotations]]` entry in `REUSE.toml`; if its license is new to the tree, its text goes in `LICENSES/` named by SPDX identifier, and the PKGBUILD's `license=()` grows an entry |
| A dependency | `Cargo.toml`, then `bin/release` rewrites `THIRD-PARTY-NOTICES`. A license `about.toml` does not accept fails generation: add it there, in priority order, and its text to `LICENSES/`, or take the dependency instead |
| An upscale filter or tone map | the WGSL function, one arm in `render/shader_codes.rs`, one enum variant with its `label`/`parse`/`next`; a tone map's CPU twin is `ToneMap::apply`, which takes the surface's `Headroom` as the shader arm does |
| What the surface can be, SDR or HDR | `render/output.rs` chooses it; `monitor.rs` says what the monitor is in; `App::surface_hdr` and `App::headroom` put the two together, `App::sync_output` acts on them, and `App::toggle_hdr` is what the bar's `HDR` button and `o` both call |
| A new render pass | build it from `render/gpu.rs`; add its target to `Renderer::render` |
| Something about the display window, exposure or false color | `image/display.rs` (state) and `shaders/image.wgsl` / `composite.wgsl` (effect) |

## Conventions

- Decoders describe, they do not normalize: keep 16-bit and float data as it
  is and say what it means through `ColorSpace` and `AlphaMode`.
- A sampled texel is always linear in the working space. Transfer functions
  are resolved once at upload (`render/upload.rs`), never in a shader.
- The interface is laid out in logical pixels; the image is placed in
  physical ones. `FrameInput.scale` converts. A display need not have a whole
  number of device pixels to the logical one, so anything thin — a rule, an
  icon's stroke — is put on the device's own grid before it is drawn
  (`UiFrame::line`, `snap`, `stroke_center_in_device`). Rounding to a whole logical
  pixel is not the same thing and is not enough.
- A label that sits beside a mark is leveled on its capitals
  (`TextMeasure::cap_center`), not on the box its line is laid out in — that
  box keeps room under the baseline for descenders the label may not have. A
  line of prose in a bar centers the box instead (`ui/mod.rs::text_baseline`),
  descenders being ordinary there.
- The interface's face has no U+2192, and the arrow the fallback supplies sits
  low, so a readout showing one thing become another uses `ui::BECOMES`.
- Handlers return an `Effect` (`Redraw` / `Nothing` / `Quit`), never call
  `request_redraw` themselves.
- Docs: `user-docs/` is for users and has its own CLAUDE.md; implementation
  reasoning belongs in the README or in module docs.
- American spelling throughout — code, comments, docs, and every word a user
  reads: `color`, `gray`, `center`, `normalize`, `license`, `behavior`,
  `canceled`. The tree holds no British spelling of any of them, so a search
  for one finds every use rather than half of them. Two words are kept in
  their British form on purpose, both of them things a user might type at us
  rather than read from us: `--colormap grey` is taken as `gray`, and the
  desktop entry's `Keywords=` lists `colour` beside `color` so that a search
  in either finds the program. The other exception is a name someone else
  chose, quoted as they wrote it: a spec's own wording, an external API, an
  SPDX license identifier.

## Checks

```sh
cargo test
cargo clippy --all-targets    # clean
cargo doc --no-deps           # no warnings
cargo run --release -- test_images/png-rgb8.png --histogram
cargo audit                   # no advisories against the pinned dependency versions
reuse lint                    # every file accounted for by REUSE.toml
cargo about generate --frozen packaging/about.hbs   # only after a dependency changes
```

Decoders parse untrusted files, so treat a panic on a malformed input as a
bug: the loader turns one into a recoverable decode error, but every decode
path should be size-checked before it allocates, and a new format wants a
malformed fixture as well as a valid one.
