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
| `q`, `Esc` | Quit — `Esc` first closes an open popup |
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

It only appears while some of the image is off screen. A view holding all of
it is already its own map, and the thumbnail would be a smaller copy of the
window laid over the corner of it, so the widget leaves and comes back on the
zoom that first cuts something off. The toggle keeps its state through that:
the button stays lit, and the minimap returns without being asked for again.

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

## Theme

The interface takes its colours from the desktop rather than carrying its own.
On [Omarchy](https://omarchy.org) the active theme is materialised as a
palette file, and `src/theme/` reads it, resolves it, and derives the
handful of roles the chrome actually needs — panel, hairline, primary and dim
text, accent, the menu panel, the floating panel and the ink on it, the
histogram's planes. Switching the desktop's theme is picked up on the same
250 ms poll as the file on screen, so an open window changes with everything
else rather than staying in the theme it opened under.

Off Omarchy there is nothing to read and nothing happens: the neutral dark set
the interface was designed in is used instead. The same set fills in for a
palette too sparse to build on, so a half-written theme degrades to something
wearable rather than to black on black.

The palette is not read literally. Omarchy resolves it through an alias and
derivation cascade before any consumer sees it — short names, ANSI `color0`
through `color15` in both directions, shades mixed out of the base colours —
and a theme is free to define only one side of any of those pairs.
`src/theme/palette.rs` reimplements that cascade rather than shelling out to
`omarchy-theme-color`, which would cost a process per read and is not there to
be called on a machine that has no Omarchy on it. Its tests check the result
against what that script prints for the same file, so the two cannot drift
apart quietly.

Two things resist being themed directly and are derived instead:

* **The floating histogram panel stays dark whichever way round the theme
  is.** The plot is drawn by screening the colour planes over one another, and
  screening only reads on a dark ground: ground under the plot is added to
  every plane, so a panel light enough to see lifts each plane's darkest
  channel several times over and three overlapping planes come out as three
  washes of the same pale colour. The panel is therefore the darkest colour
  the theme has — its `darker_background` where it is a dark theme, its *ink*
  where it is a light one, with the label on it drawn in the background — and
  that colour is then taken down in value until it is dark enough to screen
  onto. Scaled whole, so the theme's hue and saturation are exactly what they
  were, and only when it is above the ceiling, so a theme that has picked its
  own dark end keeps it: every dark theme tried against this passes through
  untouched, and what the ceiling catches is the light theme, whose darkest
  colour is nothing of the kind.

  A light panel with the plot *multiplied* onto it instead — the arithmetic
  dual, and the theme-compliant answer if it worked — was tried and does not:
  three subtractive inks that overlap in a neutral mid grey have to be pale
  ones, so the channels stop being tellable apart, and a near-white panel over
  a bright picture loses its own edges.
* **The colour planes are pulled towards their own primaries and then
  balanced.** A palette's red is a pastel with green and blue in it, and three
  pastels screened together climb towards white, which loses the overlaps the
  plot exists to show. Each plane is scaled — whole, so the theme's hue
  survives — until all three screened together land on a neutral mid grey. A
  theme that names no colours keeps the planes the interface was designed
  with, since one themed plane beside two default ones would read as three
  unrelated colours.

A popup's panel is not one of those two. Its cells are buttons, drawn in the
same ink as the toggles in the side panels, and that ink is made to read
against the bars — so the panel is the bars' own surface and follows the theme
either way round. It is held nearer to opaque than the floating panel is,
since the picture coming through a menu is what the choices on it compete
with.

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
frame.rounded_rect(Rect::new(x, y, w, h), 6.0, theme.panel_background);
frame.text([x, y], 13.0, theme.text_primary, "hello");
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
doing to the image. The left strip holds the minimap toggle, the right strip
the histogram toggle, and the end of the bottom bar the zoom readout — which
is a button: pressing it opens a menu of zooms in the lower right of the
content area, the ladder from 10% to 1600% and the three fits as icons. The
readout is a fixed width so that the click handler knows where it is without
measuring what it says, and so that it does not shuffle along the bar as the
zoom changes.

Popups are `render::ui_layer::Popup`: a panel of uniform cells anchored to a corner
of an area, which answers where the panel goes, where each cell landed, and
which cell a point is over. What a cell has in it and what pressing one does
stay with the caller (`src/ui/menu.rs`), so a second menu is a `Menu` variant,
a grid, and the code that draws its cells. Only one can be open, which is what makes
dismissing one unambiguous: an open menu takes every press before the chrome
and the image do, a press on a cell chooses and closes, and a press anywhere
off the panel is spent closing it. `Esc` closes it too, in front of the quit
it would otherwise be.

They are opaque, and the image is drawn in the `Viewport` they leave rather
than behind them: zoom, fit, pan limits and the wheel's anchor are all measured
against that rectangle. It is derived per frame from the window and whether the
panels are showing, never stored, so `` ` `` re-fits a fitted image without
anything having to notice that it should.

| File | Role |
| --- | --- |
| `src/main.rs` | Event-loop setup |
| `src/cli.rs` | Argument parsing and `--help`, whose key sections come from the keymap |
| `src/app/` | Window lifecycle and the event loop's state: `files.rs` is the file list and the read in flight, `input.rs` the keymap and pointer, `window.rs` titles and opening size |
| `src/ui/` | Building each frame's interface: `chrome.rs` the panels, one file per widget, `status.rs` the words in the bars |
| `src/view.rs` | Zoom / pan / fit geometry — pure maths |
| `src/watch.rs` | Noticing that the file on screen has been rewritten |
| `src/theme/` | `palette.rs` reads the desktop's palette; `mod.rs` derives the colours drawn from it |
| `src/image/` | The data model: `Samples`, `color/` (transfer functions, primaries, ICC and CICP), stats, display state |
| `src/image/decode/` | The decoder trait and its registry, one file per format |
| `src/render/` | Upload planning, the three layers, output selection; `shader_codes.rs` is every integer the shaders switch on |
| `src/render/ui_layer/popup.rs` | Where a popup menu's panel and cells go, and what a press lands on |
| `src/render/reduce.rs` | The coarse chain a minifying draw reads from |

## Formats

| Format | Backend |
| --- | --- |
| PNG, JPEG, GIF, Radiance HDR, OpenEXR | [`image`](https://crates.io/crates/image) |
| TIFF | [`tiff`](https://crates.io/crates/tiff) directly |
| HEIF — HEIC, AVIF | [`libheif-rs`](https://crates.io/crates/libheif-rs), onto the system `libheif` |
| WebP — lossy, lossless, animated | [`image-webp`](https://crates.io/crates/image-webp) directly |
| ICO | own directory reader, onto the PNG path and [`image`](https://crates.io/crates/image)'s bitmap one |
| Ultra HDR containers, ICC profiles | [`ultrahdr-rs`](https://crates.io/crates/ultrahdr-rs), [`moxcms`](https://crates.io/crates/moxcms) |
| PNG `cICP` and `iCCP` chunks | [`png`](https://crates.io/crates/png), which `image` already carries |

The decoder is chosen by content, falling back to the file extension for
formats without a recognisable header. Files are streamed rather than read
into memory whole, so opening a 600 MB raster does not begin by copying it —
JPEG excepted, because its gain map sits past the pixels and the reader that
finds it needs the file as one slice.

### Saying what the numbers mean

Four formats state their colour space rather than leaving it to convention,
and they do it in two vocabularies.

**CICP code points** — the small integers of ITU-T H.273 — are the precise
form, because they name a transfer function this program models exactly. HEIF
carries them in an `nclx` box, PNG in a `cICP` chunk; one translation in
`decode::cicp` serves both, so a third format that carries them is a matter of
reading two bytes. A `cICP` chunk is the whole of how a PNG says it is BT.2100
PQ or HLG, and `image` surfaces nothing of it, which is why `png` is a direct
dependency for the header pass.

**ICC profiles** are the other form, and the one a phone JPEG uses — and the
only form WebP has. Only what
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

### WebP

Both bitstreams, and the container that can hold either. VP8 is lossy, coded
as YCbCr 4:2:0 and upsampled on the way out; VP8L is lossless and exact. Alpha
arrives two ways — a bit in the VP8L header, or an `ALPH` chunk beside a lossy
frame — and is straight in both. Neither bitstream has anything above eight
bits or outside three colour channels, so a WebP is always `U8` and always
RGB or RGBA; there is no depth to preserve and no monochrome encoding to keep
one channel wide.

`image-webp` is a direct dependency rather than a feature of `image`, for the
same reason `png` is: everything worth having beyond the pixels lives in the
RIFF container, and `ImageReader` hands back only the pixels. Three chunks are
read.

`ICCP` is the only thing a WebP has to say about its own colour — the format
carries no CICP code points — and it goes through the same profile reader
JPEG, PNG and HEIF use. Without one the file means sRGB.

`EXIF` carries the orientation, and it is applied. This is the one place a
metadata tag is honoured rather than ignored, and it is a deliberate
exception: the tag sits in a chunk this decoder is already opening for the
profile, and reading it costs a rotation of a buffer that is already in hand.
JPEG's EXIF orientation still is not applied — same tag, different decoder,
and that one would have to grow a container pass to reach it.

`ANIM` and `ANMF` make the file an animation, and the first frame is what is
shown. That frame is not necessarily a picture: the format lets it be a patch
at an offset, composited onto a canvas the `ANIM` chunk colours, so it is
decoded through the animation path rather than read out directly and arrives
whole either way. The frames after it are not shown. Nothing downstream of the
decoder has a clock — an image is decoded once, uploaded once, and redrawn
only when the view changes — so playing them would be a change to the event
loop rather than to this decoder.

### GIF

GIF takes the plain route through `image`, because its container has nothing
to say that this program could act on: no profile, no code points, no
orientation, and a palette of sRGB bytes by definition. Every GIF comes back
RGBA whatever its palette holds — the crate's decoder has one output layout,
and the transparent index has to go somewhere. That index is also all the
transparency the format has: one palette entry is a hole, the rest are opaque,
and the pixel behind the hole carries no colour at all rather than a colour
with zero alpha the way a PNG's `tRNS` does.

An animated GIF shows its first frame, the same choice an animated WebP gets.
The crate composites that frame onto the logical screen the file declares, so
a first frame stored as a patch at an offset still arrives at the full size
rather than cropped to the patch.

### ICO

An ICO is not an image but a folder of them — the same picture at 16, 32, 48
and 256 pixels, so that Windows can pick the one that fits the slot it is
drawing into. A viewer has no slot, so it has to choose, and the choice is the
whole of what this decoder adds.

It picks the **largest** entry, breaking a tie on the stated depth. `image`'s
own ICO decoder scores the other way round, depth before size, which is right
for a toolkit asked for the best-quality rendition and wrong here: an icon
whose 256×256 entry is a palette image and whose 16×16 entry is 32-bit shows
as a thumbnail. `ico-multi.ico` is that file, and the test that it decodes to
the large entry is the one this module exists to pass.

Each entry is a whole file in its own right, in one of two formats, and they
take different routes:

**PNG**, which is how every entry above 48 pixels has been written since
Vista, goes through the same code a `.png` on disk does. So an `iCCP` chunk is
read — an icon can be Display P3 — and any pixel layout is kept, greyscale
included. `image` refuses anything but RGBA8 there, on the strength of a
Microsoft blog post saying embedded PNGs must be 32-bit; browsers display the
others, and so does this.

**BMP** is a headerless DIB with two Windows-specific quirks: the height in
its header counts the rows twice, and a 1-bit AND mask may follow the pixels
carrying transparency the colour data has no room for — which is how a 4-bit
palette icon has a transparent background. `image` handles both, but only from
inside its own ICO decoder, whose hooks are `pub(crate)`, so the chosen entry
is handed back to it wrapped in a 22-byte container holding nothing else.
Rebuilding the DIB reader to avoid that would be the worse trade: it is a
decade of Windows bitmap variants, already written and already tested. Every
bitmap entry comes back RGBA whatever its stored depth, because the mask has
nowhere else to go.

Only type 1, the icon, is claimed. A cursor is the same container under the
`.cur` extension, but its directory overloads the colour-plane and bit-depth
fields with the hotspot coordinates, so the numbers the selection sorts on
would mean something else entirely. One opened as `.ico` anyway is named as a
cursor rather than mis-sorted.

ICO also has no magic number worth the name — four bytes, three of them zero —
so the directory's own structure stands in for one when sniffing: a file
claiming entries it has no room for, or whose first entry starts inside the
directory that lists it, is not an ICO however its first four bytes read.

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
    fn decode(&self, source: &mut dyn ReadSeek, overrides: Overrides) -> Result<DecodedImage>;
    fn dimensions(&self, source: &mut dyn ReadSeek) -> Result<Option<(u32, u32)>> { Ok(None) }
}
```

`DecodedImage` carries `Samples` (U8/U16/F32 × gray/gray+alpha/rgb/rgba),
a `ColorSpace` (transfer function and primaries), an `AlphaMode`, and, for
files that state their own, an optional `value_range` and `nodata` sentinel.
`DecodedImage::new` fills in the two optionals and `AlphaMode::of` picks the
alpha mode from the channel layout, so a decoder only has to say what it found.

To add one: write a module in `src/image/decode/` implementing `Decoder`, and
add it to the `DECODERS` slice — `src/image/decode/tiff_rs.rs` is a worked
example that reaches past `image` to a lower-level crate, and
`src/image/decode/png.rs` one that goes through `image` with a container pass
of its own. Nothing else changes — the upload layer picks a texture format from
the description, the error messages pick up the new extensions, and a test
guards against two decoders claiming the same one. The fixture test will ask
for a file in `test_images/` exercising it.

## Known limits

Decoding is synchronous: the window does not appear until the first image is
ready, and switching files blocks until the next one is. Mount Rainier at 3 m
(18333 × 15667, 589 MB of Deflate) takes about six seconds and peaks around
1.4 GB of resident memory against a 1.15 GB decoded buffer. Nothing is
streamed to the GPU in tiles, so an image also has to fit in one texture.

EXIF orientation is not applied, so a rotated phone JPEG shows unrotated. HEIF
and WebP are the exceptions: HEIF's rotation lives in the container rather than
in a metadata tag, and WebP's tag sits in a chunk its decoder already opens for
the colour profile.

An ICO shows one entry of the several it holds — the largest — and the rest
are not reachable. Showing them side by side is what the comparison view is
for, but nothing below the decoder can return more than one image per file.

Animated WebP and animated GIF show their first frame and stop there. Playing
the rest needs a clock in the event loop, which nothing else here wants; the
frames themselves are already reachable through the decoders that read the
first one.

Embedded ICC profiles are read for JPEG, PNG, HEIF and WebP — and so for an
ICO whose entry is a PNG — which is every format here that can carry one
except TIFF, which can and does not. GIF has no way of carrying one.

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

182 tests over the transfer functions and primaries matrices, texture format
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

`test_images/` holds 67 real fixtures — see its README — covering every pixel
layout the decoder can produce and every per-format encoding with its own code
path: PNG bit depths, palettes and interlacing; progressive and subsampled
JPEG; TIFF compressions, byte orders, tiling, BigTIFF, the floating-point
predictor, signed samples and no-data; Radiance RGBE; EXR associated alpha;
HEIC monochrome, 10-bit, `irot` and its colour tags, and the same container
with AV1 inside; WebP in both bitstreams, with and without alpha, tagged,
rotated and animated; GIF interlaced, transparent and animated. Four of them
exist for the colour tags in particular: a
PNG carrying `cICP` for BT.2100 PQ, a PNG carrying `iCCP` for Display P3, a
HEIF tagged by ICC profile with no `nclx` box beside it, and a WebP carrying
`ICCP`. Each is checked for
dimensions, channel layout, sample type, colour space, alpha mode and actual
pixel values, and then pushed through the upload planner under both GPU
capability sets. A test asserts the directory and the fixture table stay in
step, so a file cannot be added without a test.

Regenerate them with `test_images/generate.sh`, which needs ImageMagick,
`heif-enc`, GDAL and Python. Neither `cICP` nor `iCCP` is a chunk ImageMagick
will write, so those two are spliced in afterwards with their CRCs computed,
and its WebP writer emits neither an `EXIF` chunk nor an animation, so those
two fixtures are assembled around the bitstreams it did write.
`display-p3.icc` sits beside the fixtures as an input rather than an output;
`examples/make-icc.rs` is what produced it.
