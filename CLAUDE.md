# image-view

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
  histogram.rs / minimap.rs / grid.rs / buttons.rs   one widget each
  info.rs        the file's own facts, in a column that scrolls
  pixel.rs       the pointer's readout: coordinate, stored and mapped values, swatch
  menu.rs        Menu (which popup is open), the zoom menu's choices, and how its cells are drawn
  status.rs      the words in the top and bottom bars
theme/         palette.rs reads Omarchy's colors.toml and resolves its cascade; mod.rs derives Theme's colour roles
view.rs        zoom / pan / fit geometry, pure maths (View, Viewport, Fit)
loader.rs      the decode + upload thread; replies arrive as winit user events
watch.rs       polling a file for a settled change
clipboard.rs   putting text or a file: URI on the clipboard, in a process that outlives the window
timing.rs      startup instrumentation
image/         the data model, nothing GPU
  mod.rs         Channels, Samples, AlphaMode, DecodedImage, Sample (one pixel read back)
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
  image_layer.rs / reduce.rs / composite.rs / ui_layer/   the passes; ui_layer holds Rect, Color, UiFrame, and popup.rs (a grid of cells anchored to a corner)
  shader_codes.rs  every Rust<->WGSL integer code, one fn per shader switch
  gpu.rs         wgpu boilerplate helpers (layouts, uniform buffers, full-screen pipelines, GrowableBuffer)
  shaders/       WGSL; each Params struct is mirrored by a #[repr(C)] struct in the .rs file that loads it
```

## Where to make a change

| Change | Edit |
| --- | --- |
| A key binding | `app/input.rs`: one `KEYS` entry, with the `mods` it is held with, and one `perform` arm. `--help` follows. |
| A status-bar segment | `ui/status.rs`; the pointer's pixel readout is `ui/pixel.rs` |
| What a pixel reads as under the pointer | `image/mod.rs::sample` for what the file holds, `image/display.rs::map` for what the screen shows |
| A panel or overlay | a new `ui/<name>.rs` and one call in `ui/mod.rs::build_frame`; a new colour role goes in `theme/mod.rs` |
| What the info panel says about a file | `ui/info.rs` for the layout; the file's own facts are gathered in `app/mod.rs::file_facts`, its metadata in `image/exif.rs`, and its georeference in `image/geo.rs` |
| A popup menu | a `Menu` variant in `ui/menu.rs` with its choices, `items`/`grid`/`choose` arms and a `draw` arm; `Chrome::popup` places it, `App::press` opens it |
| A CLI flag | `cli.rs`, and the `Options` / `Startup` / `Overrides` field it sets |
| An image format | `image/decode/<fmt>.rs` implementing `Decoder`, one line in `DECODERS`, a fixture in `test_images/` (see its README and `generate.sh`) |
| What a copy of the image contains | `image/encode.rs`; the chord that asks for it is in `app/input.rs` |
| A colour-space source (a new tag a format carries) | `image/color/` |
| An upscale filter or tone map | the WGSL function, one arm in `render/shader_codes.rs`, one enum variant with its `label`/`parse`/`next` |
| A new render pass | build it from `render/gpu.rs`; add its target to `Renderer::render` |
| Something about the display window, exposure or false colour | `image/display.rs` (state) and `shaders/image.wgsl` / `composite.wgsl` (effect) |

## Conventions

- Decoders describe, they do not normalise: keep 16-bit and float data as it
  is and say what it means through `ColorSpace` and `AlphaMode`.
- A sampled texel is always linear in the working space. Transfer functions
  are resolved once at upload (`render/upload.rs`), never in a shader.
- The interface is laid out in logical pixels; the image is placed in
  physical ones. `FrameInput.scale` converts.
- Handlers return an `Effect` (`Redraw` / `Nothing` / `Quit`), never call
  `request_redraw` themselves.
- Docs: `user-docs/` is for users and has its own CLAUDE.md; implementation
  reasoning belongs in the README or in module docs.

## Checks

```sh
cargo test
cargo clippy --all-targets    # clean
cargo doc --no-deps           # no warnings
cargo run --release -- test_images/png-rgb8.png --histogram
cargo audit                   # no advisories against the pinned dependency versions
```

Decoders parse untrusted files, so treat a panic on a malformed input as a
bug: the loader turns one into a recoverable decode error, but every decode
path should be size-checked before it allocates, and a new format wants a
malformed fixture as well as a valid one.
