# image-view

A GPU-accelerated image previewer with real colour management: Rust, `winit`
for the window, `wgpu` for the drawing, `glyphon` for the text.

Handles ordinary photographs, 16-bit and floating-point measurement data, and
HDR frames through one pipeline, without flattening any of them to 8-bit sRGB
on the way in.

## Build and run

```sh
cargo run --release -- photo.jpg scan.tiff render.exr
```

The first file is shown, stretched to fit the window, which opens at the
image's own size shrunk to fit the monitor.

## Keys

| Key | Action |
| --- | --- |
| `q`, `Esc` | Quit |
| `+`, `=` / `-`, `_` | Zoom in / out |
| `0` | Actual size (100%) |
| Arrows | Pan |
| `f` | Cycle fit → fit width → fit height |
| `n`, `p` | Next / previous file |
| `e`, `E` | Exposure down / up, half a stop |
| `a` | Cycle the automatic window: unit → min/max → 99.8% |
| `[`, `]` | Slide the window down / up |
| `,` `.` | Narrow / widen the window |
| `t` | Cycle tone mapping: clip → reinhard → neutral |
| `c` | Cycle false colour (single-channel images) |
| `r` | Reset display settings |
| `h` / `i` | Toggle the histogram / the overlay |

Zooming leaves fit mode; panning does not, so `f` then Down scrolls through a
tall image at fit-width. Keys held with Ctrl, Alt or Super are ignored, so
window-manager chords such as `Super+0` do not disturb the view.

Every display control is also a start-up flag — `--colormap viridis`,
`--tone-map neutral`, `--window minmax`, `--exposure -1.5`, `--histogram` —
which is handy for scripting and for comparing two files side by side.

## Colour management

The one invariant everything else follows from:

> **A sampled texel is always linear in the working space** — because the data
> was linear already, because the format's hardware decode produces it, or
> because we linearised on the way in.

It matters because hardware sRGB decode happens *before* texture filtering
while a shader decode necessarily happens after. Decoding in the shader would
mean filtering in encoded space — the classic gamma-incorrect downscale — and
this viewer minifies constantly, since fit is the default. So the transfer
function is resolved once per image at upload, never per frame.

The working space is linear BT.709 in `Rgba16Float`, with room above 1.0 for
HDR content to survive until tone mapping.

| Source | Stored as |
| --- | --- |
| 8-bit sRGB colour | `Rgba8UnormSrgb` — hardware decodes, filtering stays correct |
| 8-bit sRGB grey | `R16Float` — there is no `R8UnormSrgb`, so a LUT linearises it |
| 16-bit linear | `R16Unorm` / `Rgba16Unorm`, or half float where unsupported |
| 16-bit encoded | half float via a 65536-entry LUT |
| 32-bit float | `R32Float` / `Rgba32Float`, or half float if the GPU cannot filter them |

Alpha is coverage, never light, so it is never put through a transfer
function. Three-channel data is expanded to four because no graphics API has a
three-component sampled texture; one- and two-channel data is *not* expanded,
so a 20000×20000 16-bit grey scan costs 800 MB rather than 3.2 GB.

### HDR output

`--hdr` requests an HDR surface when the driver offers one: scRGB
(`Rgba16Float` + `ExtendedSrgbLinear`) by preference, HDR10
(`Rgb10a2Unorm` + `Bt2100Pq`) otherwise. It is opt-in because a driver will
report an HDR colour space whether or not the monitor in front of you is HDR.
Without it, HDR content is tone mapped into an ordinary sRGB surface.

### Grey images

Both kinds are supported, and they want opposite defaults, so the choice is
made from the transfer function rather than from bit depth:

- **Display-referred** (sRGB, gamma) content has already been graded by
  whoever made it. Window stays at 0..1; touching it would second-guess them.
- **Scene-referred** (linear, PQ, HLG) content has not. It gets a 99.8%
  percentile window, because 12-bit data in a 16-bit container occupies a
  sixteenth of the nominal range and shows as a black rectangle otherwise.

`--transfer linear|srgb|pq|hlg|gamma:N` overrides the guess, which matters
most for TIFF: the same container carries scanned photographs and frames of
sensor counts, and the header does not distinguish them. Window bounds are
reported in source units for linear integer data, so a 12-bit scan reads
`0–4096` rather than `0.000–0.063`.

False colour (`c`, or `--colormap`) applies to single-channel images, and
suppresses tone mapping while active — a curve on top of a colormap would
distort the mapping you are reading values off.

## Architecture

Rendering is three separable layers:

1. **Image layer** → a linear working-space target. Swizzle, primaries
   matrix, display window, false colour.
2. **UI layer** → its own sRGB target. One instanced draw for every rectangle
   plus one text pass.
3. **Compositor** → the surface. Tone map, lay the UI over, encode for
   whatever the surface turned out to be.

Keeping the interface off the image's target is what lets UI code stay in
plain sRGB and logical pixels while the image beside it is extended-range
linear — and it is what makes a third-party text renderer usable at all, since
glyphon has no idea what an HDR surface is.

`UiFrame` is a display list, not a widget toolkit:

```rust
frame.rounded_rect(Rect::new(x, y, w, h), 6.0, PANEL_BACKGROUND);
frame.text([x, y], 13.0, TEXT_PRIMARY, "hello");
let width = renderer.measure_text("hello", 13.0)[0];   // for layout
```

Adding panels, sliders or inspectors means emitting more primitives; the
renderer does not change, and it all batches into the same two draws. The
status bar and the histogram are the two clients that exist today.

| File | Role |
| --- | --- |
| `src/main.rs` | Argument parsing, event-loop setup |
| `src/app.rs` | Window lifecycle, key handling, building each frame's UI |
| `src/view.rs` | Zoom / pan / fit geometry — pure maths |
| `src/image/` | The data model: `Samples`, `ColorSpace`, stats, display state |
| `src/image/decode/` | The decoder trait and its registry |
| `src/render/` | Upload planning, the three layers, output selection |

## Formats

| Format | Backend |
| --- | --- |
| PNG, JPEG, Radiance HDR, OpenEXR | [`image`](https://crates.io/crates/image) |
| TIFF | [`tiff`](https://crates.io/crates/tiff) directly |

TIFF goes to the `tiff` crate rather than through `image` because `image`
cannot carry the format's full range. `DynamicImage` has no single-band
floating-point variant, so a one-band Float32 raster — essentially every DEM
and scientific image — fails outright, and `image`'s format sniffer does not
know BigTIFF's magic number at all. Going direct also covers the
floating-point predictor, signed and wide integer samples, and GDAL's no-data
tag, and keeps a single-band raster single-band all the way to the GPU: a
5500×4700 elevation model uploads as 103 MB of `R32Float` rather than 413 MB
of `Rgba32Float`.

Signed and wide integer rasters are widened to float rather than rescaled —
an elevation model holds metres, and −86 at the Dead Sea is a real value, not
something to normalise away. A no-data sentinel is read from the file and kept
out of the statistics, so a clipped DEM's −9999 fill cannot set the bottom of
the automatic window and squash the terrain into a sliver.

The decoder is chosen by content, falling back to the file extension for
formats without a recognisable header. Files are streamed rather than read
into memory whole, so opening a 600 MB raster does not begin by copying it.

### Size ceiling

Both backends ship conservative allocation limits — 256 MiB in `tiff`, 512 MiB
in `image` — which a survey-grade elevation model passes on the way out of the
door. Both are raised to 4 GiB, which is not an arbitrary number:
`max_texture_dimension_2d` is 32768 on current hardware, and 32768 × 32768 × 4
bytes is exactly 4 GiB, so the ceiling is the largest single-channel 32-bit
image that could be displayed even in principle. Going over it is refused from
the header alone, before anything is decoded, with a message naming the size.

### Adding a format

Decoders describe what they found rather than normalising it:

```rust
pub trait Decoder: Sync {
    fn name(&self) -> &'static str;
    fn extensions(&self) -> &'static [&'static str];
    fn sniff(&self, header: &[u8]) -> bool;
    fn decode(&self, bytes: &[u8]) -> Result<DecodedImage>;
}
```

`DecodedImage` carries `Samples` (U8/U16/F32 × gray/gray+alpha/rgb/rgba),
a `ColorSpace` (transfer function and primaries), an `AlphaMode`, and an
optional `value_range` for files that state their own.

To add one: write a module in `src/image/decode/` implementing `Decoder`, and
add it to the `DECODERS` slice — `src/image/decode/tiff_rs.rs` is a worked
example that reaches past `image` to a lower-level crate. Nothing else changes — the upload layer picks
a texture format from the description, `--help` and the error messages pick up
the new extensions, and a test guards against two decoders claiming the same
one.

## Known limits

Decoding is synchronous: the window does not appear until the first image is
ready, and switching files blocks until the next one is. Mount Rainier at 3 m
(18333 × 15667, 589 MB of Deflate) takes about six seconds and peaks around
1.4 GB of resident memory against a 1.15 GB decoded buffer. Nothing is
streamed to the GPU in tiles, so an image also has to fit in one texture.

EXIF orientation is not applied, so a rotated phone JPEG shows unrotated.

## Tests

```sh
cargo test
```

52 tests over the transfer functions and primaries matrices, texture format
selection (including the device-capability fallbacks), the statistics and
window logic, the decoder registry, and the view geometry. Rendering itself is
not covered; it needs a GPU and a window.

`test_images/` holds 39 real fixtures — see its README — covering every pixel
layout the decoder can produce and every per-format encoding with its own code
path: PNG bit depths, palettes and interlacing; progressive and subsampled
JPEG; TIFF compressions, byte orders, tiling, BigTIFF, the floating-point
predictor, signed samples and no-data; Radiance RGBE; EXR associated alpha. Each is checked for dimensions, channel layout, sample type, colour
space, alpha mode and actual pixel values, and then pushed through the upload
planner under both GPU capability sets. A test asserts the directory and the
fixture table stay in step, so a file cannot be added without a test.
Regenerate them with `test_images/generate.sh`.
