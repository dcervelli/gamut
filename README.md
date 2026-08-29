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

The first file is shown, stretched to fit the space the interface panels
leave in the middle; the window opens at the image's own size plus that
chrome, shrunk to fit the monitor. `` ` `` hides the panels and gives the
image the whole window, re-fitting it as it goes.

Everything is pure Rust except HEIF, which links the system `libheif` (1.20 or
newer) — `libheif-dev` on Debian, `libheif` on Arch, `brew install libheif` on
macOS. Which HEIF *codecs* work then depends on that installation's plugins:
HEVC (`.heic`) needs libde265 or ffmpeg, AV1 (`.avif`) needs dav1d or aom.
Both ship as standard on the distributions above.

## Keys

| Key | Action |
| --- | --- |
| `q`, `Esc` | Quit |
| `+`, `=` / `-`, `_` | Zoom in / out |
| Wheel | Zoom about the pointer |
| `0` | Actual size (100%) |
| Arrows | Pan |
| Drag | Pan, with the image following the pointer |
| `f` | Cycle fit → fit width → fit height |
| `u` | Cycle the filter used above 100%: nearest → bicubic |
| `n`, `p` | Next / previous file |
| `e`, `E` | Exposure down / up, half a stop |
| `a` | Cycle the automatic window: unit → min/max → 99.8% |
| `[`, `]` | Slide the window down / up |
| `,` `.` | Narrow / widen the window |
| `t` | Cycle tone mapping: clip → reinhard → neutral |
| `c` | Cycle false colour (single-channel images) |
| `r` | Reset display settings |
| `h` | Toggle the histogram |
| `m` | Toggle the minimap |
| `` ` `` | Toggle the interface panels |

The panels are opaque and the image is fitted inside them rather than passing
behind them, so `` ` `` changes how much room a fitted image has and it re-fits
on the spot.

Zooming leaves fit mode; panning does not, so `f` then Down scrolls through a
tall image at fit-width. Keys held with Ctrl, Alt or Super are ignored, so
window-manager chords such as `Super+0` do not disturb the view.

`n` and `p` keep the pan and zoom when the file they land on is the same size
as the one on screen — a directory of frames or of exposures is a set to be
compared, and the comparison only works if the same detail stays under the
same pixels. A file of another size is a different picture, and is fitted.

Every display control is also a start-up flag — `--colormap viridis`,
`--tone-map neutral`, `--window minmax`, `--exposure -1.5`, `--histogram`,
`--minimap` — which is handy for scripting and for comparing two files side by
side.

## Minimap

`m`, or the button at the top of the left strip, puts a thumbnail of the whole
image in the top-left corner, with the part of it on screen picked out and
the rest washed over. It is the map to read while zoomed in far enough that
the image on screen no longer says where in the picture you are.

The thumbnail is not a separate rendering of the image: it is a second quad in
the image layer's pass, drawn from the same texture through the same shader as
the view itself, reading whichever coarse level suits the size it is drawn at.
Exposure, the display window, false colour and tone mapping therefore reach it
without any of that being reimplemented for a widget, and it costs one more
draw call and a second uniform. Only the border and the wash over what is off
screen belong to the interface, which is why both are drawn hollow or
translucent — the thumbnail underneath them is in the layer below.

## Live reload

The file on screen is watched, and a write to it by anything else — a render
finishing, a script rewriting its output, an editor saving — is picked up and
shown within about half a second. Nothing has to be pressed.

A reload keeps you where you were: the same pan and zoom, the same exposure
and tone map, with only an automatic window re-derived from the new pixels.
The point is watching one spot as the numbers under it change. A file that
comes back a different size is treated as a different picture and gets a fresh
fit. `n` and `p` move the watch along with the view.

It is a `stat` every 250 ms, not `inotify`. That costs nothing measurable, and
it is the version that works over NFS and SSHFS and that survives the way most
editors save — a temporary file renamed over the original, which leaves a
watch on the original inode looking at a file nobody will ever write to again.
A change is read only once the size and timestamp have held still for a whole
interval, so a file caught halfway through being written is waited out rather
than decoded and reported as corrupt.

## Colour management

The one invariant everything else follows from:

> **A sampled texel is always linear in the working space** — because the data
> was linear already, because the format's hardware decode produces it, or
> because we linearised on the way in.

It matters because a format's own decode — sRGB or otherwise — happens as part
of reading a texel, before anything is weighted against anything else, while a
decode written into the shader necessarily happens after. Decoding in the
shader would mean resampling in encoded space — the classic gamma-incorrect
downscale — and this viewer minifies constantly, since fit is the default. So
the transfer function is resolved once per image at upload, never per frame,
and every weighted sum in [Resampling](#resampling) is over linear light.

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

## Resampling

Below 100% an output pixel covers more than one texel, and the only right
answer is the average of what it actually covers — weighted by the fraction of
each edge texel the pixel overlaps, so that the result does not shimmer as the
zoom changes. Above 100% there is no right answer, only a question about what
the image is for, so it is a setting:

| Filter | What it is for |
| --- | --- |
| `nearest` (default) | Nearest neighbour, ramped across the single output pixel that straddles a texel edge. Shows the pixel grid a measurement image is read on. At a whole-number zoom it is exactly nearest neighbour; at 4.5:1 it resolves the half-covered pixel rather than doubling columns unevenly, which is what plain nearest does. |
| `bicubic` | Catmull-Rom. Interpolating, so texel centres come through untouched, and noticeably sharper than bilinear. What you want when the subject is a photograph. |

At and above 1:1 the quad is put on whole pixels, so a texel edge falls on a
pixel edge and `nearest` is exactly nearest. Centring an odd difference
otherwise leaves it half a pixel off the grid, which would put every texel edge
through the middle of a pixel and cost the 100% view its crispness.

All three filters are weighted sums of texel loads in `shaders/image.wgsl`
rather than sampler taps — one bilinear tap is neither of the two above, and it
covers four texels however far out the view is zoomed. Because the shader does
the weighting, it can also multiply straight alpha through first, so a
transparent texel no longer bleeds its colour into the edge beside it.

### What it costs, and the coarse chain

An exact area filter reads every source texel the window covers, which is a
constant — one pass over the visible image — whatever the zoom. That is
pleasant at 24 megapixels and gigabytes a frame at the texture size limit, and
with a wheel to zoom by it would be paid again on every notch.

So each image gets a chain of coarse levels, each an exact 4×4 area average of
the one above it. A draw starts from the level within a factor of four of the
size it wants, which bounds it at sixteen texels per output pixel however far
out the view is. Stepping by four per axis rather than a mipmap's two is what
makes that affordable: levels shrink by sixteen in area, so the whole chain is
a *fifteenth* of the image's own texture where a mip chain is a third of it.

It is built the first time a view zooms out past 4:1 — most never do — and it
is dropped with the image, so only one is ever alive. Levels are float, which
keeps them linear with no transfer function to think about and no 8-bit floor
under premultiplied colour: half floats, or 32-bit ones above a 32-bit float
source, where the range and the low bits are the point of the file.

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
renderer does not change, and it all batches into the same two draws.

The chrome is four panels: top and bottom bars spanning the full width, with
skinny left and right strips nested between them, so the corners belong to the
bars and the strips never reason about where one ends. `Chrome` derives all
four from the window size alone, which is what lets the frame builder and the
click handler agree on where a widget is without either of them owning it. The
top bar carries the file name and what the image is — its size, its pixels,
its colour space, all fixed for as long as the file is on screen — while the
bottom bar carries what changes: the pointer's position, and what the view is
doing to the image. The left strip holds the minimap toggle and the right
strip the histogram toggle.

They are opaque, and the image is drawn in the `Viewport` they leave rather
than behind them: zoom, fit, pan limits and the wheel's anchor are all measured
against that rectangle. It is derived per frame from the window and whether the
panels are showing, never stored, so `` ` `` re-fits a fitted image without
anything having to notice that it should.

| File | Role |
| --- | --- |
| `src/main.rs` | Argument parsing, event-loop setup |
| `src/app.rs` | Window lifecycle, key handling, building each frame's UI |
| `src/view.rs` | Zoom / pan / fit geometry — pure maths |
| `src/watch.rs` | Noticing that the file on screen has been rewritten |
| `src/image/` | The data model: `Samples`, `ColorSpace`, stats, display state |
| `src/image/decode/` | The decoder trait and its registry |
| `src/render/` | Upload planning, the three layers, output selection |
| `src/render/reduce.rs` | The coarse chain a minifying draw reads from |

## Formats

| Format | Backend |
| --- | --- |
| PNG, JPEG, Radiance HDR, OpenEXR | [`image`](https://crates.io/crates/image) |
| TIFF | [`tiff`](https://crates.io/crates/tiff) directly |
| HEIF — HEIC, AVIF | [`libheif-rs`](https://crates.io/crates/libheif-rs), onto the system `libheif` |
| Ultra HDR containers, ICC profiles | [`ultrahdr-rs`](https://crates.io/crates/ultrahdr-rs), [`moxcms`](https://crates.io/crates/moxcms) |
| PNG `cICP` and `iCCP` chunks | [`png`](https://crates.io/crates/png), which `image` already carries |

The decoder is chosen by content, falling back to the file extension for
formats without a recognisable header. Files are streamed rather than read
into memory whole, so opening a 600 MB raster does not begin by copying it —
JPEG excepted, because its gain map sits past the pixels and the reader that
finds it needs the file as one slice.

### Saying what the numbers mean

Three formats state their colour space rather than leaving it to convention,
and they do it in two vocabularies.

**CICP code points** — the small integers of ITU-T H.273 — are the precise
form, because they name a transfer function this program models exactly. HEIF
carries them in an `nclx` box, PNG in a `cICP` chunk; one translation in
`decode::cicp` serves both, so a third format that carries them is a matter of
reading two bytes. A `cICP` chunk is the whole of how a PNG says it is BT.2100
PQ or HLG, and `image` surfaces nothing of it, which is why `png` is a direct
dependency for the header pass.

**ICC profiles** are the other form, and the one a phone JPEG uses. Only what
this program's colour model can act on is taken from a profile: the primaries,
matched against the four it can name by comparing colorants rather than by
reading description text, and a transfer function only where the profile
states a plain power law. Where a file carries both vocabularies the code
points win.

PQ and HLG are treated as display-referred alongside sRGB. They are absolute
curves — 1.0 is reference white and the headroom above it was put there on
purpose — so the startup window stays at 0..1 and the tone map deals with what
is above, rather than stretching each frame's observed range and undoing the
grading.

### JPEG: gain maps

A JPEG from a recent phone is two images. The first is the ordinary graded
photograph every viewer has always shown; the second, typically a quarter of
its size, is a **gain map** — a per-pixel log2 multiplier that, applied in
linear light, puts back the highlights grading compressed. Ignore it and the
HDR half of the file is invisible with nothing to say it was ever there.

`ultrahdr-rs` walks the container — MPF and the XMP directory Google writes
beside it — and hands back the two JPEGs as raw bytes, so `image` remains the
only JPEG decoder in the build; `ultrahdr-core` does the arithmetic and the
upsample. What comes out is linear light with 1.0 at SDR reference white,
which is already the working space, so past the decoder an Ultra HDR
photograph is simply an HDR image: tone mapped on an SDR surface, sent out
untouched on an HDR one.

The whole boost is applied rather than a share of it chosen for an assumed
display. This viewer has an exposure control and a choice of tone mapping
already, and guessing here how bright the monitor is would only take that
choice away. `--no-gain-map` shows the SDR base image instead, which is worth
having when the two need comparing.

The same pass reads the ICC profile, because a phone JPEG is Display P3 far
more often than it is sRGB, and P3 numbers shown as sRGB come out visibly
flat.

### TIFF

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

### HEIF

The only decoder that is not pure Rust, because there is no usable pure-Rust
HEVC decoder to bind to instead. `libheif` also gets AVIF and whatever else
its plugins can open for free, since HEIC and AVIF differ only in the codec
inside the same container.

HEIF is the one format here that does not have to be guessed at. Where a
16-bit TIFF leaves you to work out whether it is a photograph or a frame of
sensor counts, a HEIF file *states* its transfer function and primaries in
CICP codes (H.273), so an iPhone's Display P3 photograph and a BT.2100 PQ
frame both land in the right working space with no flag from the user. The
decoder translates those codes and passes them on:

| CICP | Read as |
| --- | --- |
| Transfer 13 (IEC 61966-2-1) | sRGB |
| Transfer 1, 6, 14, 15 (BT.709 / 601 / 2020 OETF) | sRGB — not literally the same curve, but the display the content was graded for |
| Transfer 16 / 18 | PQ / HLG |
| Transfer 8 | Linear |
| Primaries 9 | BT.2020 |
| Primaries 11, 12 (DCI-P3, Display P3) | Display P3 |
| Anything else, or nothing | sRGB on BT.709 |

Depth and channel count survive the same way they do elsewhere: a 10- or
12-bit file arrives as 16-bit samples lifted to full scale rather than
flattened to bytes, and a monochrome file stays one channel all the way to the
GPU rather than being tripled into RGB.

`libheif` applies the container's own geometric properties — `irot`, `imir`,
`clap` — while decoding, so a rotated phone photograph arrives upright. That
is a property of the format, not of this program: JPEG's EXIF orientation is a
separate tag in a separate decoder, and is still ignored.

### Size ceiling

Every backend ships conservative allocation limits — 256 MiB in `tiff`,
512 MiB in `image`, and `libheif`'s own security limits — which a survey-grade
elevation model passes on the way out of the door. All are raised to 4 GiB,
which is not an arbitrary number:
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

EXIF orientation is not applied, so a rotated phone JPEG shows unrotated. HEIF
is the exception, and only because its rotation lives in the container rather
than in a metadata tag.

Embedded ICC profiles are read for JPEG, PNG and HEIF, which is every format
here that can carry one. TIFF can too, and does not.

Gain maps are read for JPEG only. HEIF can carry one as an auxiliary image,
which is how Apple stores HDR photographs, and that is not implemented; such a
file shows its SDR base. The obstacle is not the arithmetic, which is the same
code the JPEG path already runs, but the metadata: `libheif` 1.23 exposes no
gain map API at all, so reaching it would mean either walking the ISOBMFF
boxes for an ISO 21496-1 `tmap` item or reverse-engineering Apple's maker
note. Worth revisiting when `libheif` exposes it. A gain map running the other
way — where the stored image is the HDR one — is refused rather than applied,
since applying it backwards would brighten what was already bright.

Reconstruction costs memory: the result is four 32-bit floats per pixel, so a
12-megapixel photograph is a 200 MB buffer where the base image alone was 12
MB, with a transient copy of the same size on the way out of the gain map
crate.

## Tests

```sh
cargo test
```

122 tests over the transfer functions and primaries matrices, texture format
selection (including the device-capability fallbacks), the statistics and
window logic, the decoder registry, the CICP translation, ICC profile
recognition, gain map reconstruction, the view geometry, and the reload
watch's idea of when a write has finished.

The gain map tests build an Ultra HDR file rather than checking one in: a flat
base image and a half-size map that leaves one half alone and asks the other
for two stops, assembled with the same crate that reads it back, so the round
trip is exercised without a binary fixture.

Seven of them run the real image pipeline on a real adapter — a headless
device, no window — and check what the shader and the passes actually produce
against arithmetic done on the CPU: that minification is the exact mean of the
texels a pixel covers, that
two levels of the coarse chain plus the draw's own filter come to the same
number as averaging the source directly, that antialiased nearest is exactly
nearest at a whole-number zoom, that Catmull-Rom passes texel centres through
untouched, that a transparent texel does not bleed its colour into its
neighbour, and that the minimap's thumbnail lands beside the view as a second
draw of the same texture — building the coarse chain the view itself had no
use for. Where no adapter can be had they report success rather than failing
for a reason that has nothing to do with the code.

`test_images/` holds 50 real fixtures — see its README — covering every pixel
layout the decoder can produce and every per-format encoding with its own code
path: PNG bit depths, palettes and interlacing; progressive and subsampled
JPEG; TIFF compressions, byte orders, tiling, BigTIFF, the floating-point
predictor, signed samples and no-data; Radiance RGBE; EXR associated alpha;
HEIC monochrome, 10-bit, `irot` and its colour tags, and the same container
with AV1 inside. Three of them exist for the colour tags in particular: a PNG
carrying `cICP` for BT.2100 PQ, a PNG carrying `iCCP` for Display P3, and a
HEIF tagged by ICC profile with no `nclx` box beside it. Each is checked for
dimensions, channel layout, sample type, colour space, alpha mode and actual
pixel values, and then pushed through the upload planner under both GPU
capability sets. A test asserts the directory and the fixture table stay in
step, so a file cannot be added without a test.

Regenerate them with `test_images/generate.sh`, which needs ImageMagick,
`heif-enc`, GDAL and Python. Neither `cICP` nor `iCCP` is a chunk ImageMagick
will write, so those two are spliced in afterwards with their CRCs computed.
`display-p3.icc` sits beside the fixtures as an input rather than an output;
`examples/make-icc.rs` is what produced it.
