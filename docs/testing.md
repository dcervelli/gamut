# Tests

```sh
cargo test
```

The unit tests cover the transfer functions and primaries matrices, texture format
selection (including the device-capability fallbacks), the statistics and
window logic, the pixel readout's two halves and the colormaps behind its
swatch, the decoder registry, the CICP translation, ICC profile recognition,
a gain map's weight and table, the view geometry, the key table, the words the
interface says, the reload watch's idea of when a write has finished, and
the file chooser: its ranking over a matcher of the tests' own, the shared
directory the rows are named relative to, the cursor's clamping, the
thumbnail cache's URI spelling against GLib's and its MD5 against RFC
1321's vectors, a thumbnail written and found again in a cache directory of
the test's own, and the box filter's means.

The interface is tested by driving it. `src/ui/driven.rs` lays the whole of
it out headless with `egui_kittest`, presses a control by the name it gives
the accessibility tree, and reads off the commands the pass handed back: that
a toggle comes back as its press and keeps no keyboard focus, that a toggle
whose panel the window cannot take is dead and refuses the press, that the
paste button and the step buttons are there only when they would do
something, that each menu opens and each of its cells chooses what it says,
and that the file chooser takes the keyboard while it is up — what is typed,
the arrows and `Enter` come back as commands rather than reaching the window
— and hands it back when `Esc` closes it. `src/app/mod.rs` drives the same
interface over the application itself, with no window behind it — the frame
laid out from `App::frame_input`, the click fed to `App::act` — so that a
press is followed the whole way through to what it changes.
Nothing about pixels: what the frame looks like is looked at, and what it
does is tested. The geometry the panels are placed by — the four bars, the
content area, where the histogram and the information column go and when
there is no room for them — is pure and tested on its own.

The gain map tests build an Ultra HDR file rather than checking one in: a flat
base image and a half-size map that leaves one half alone and asks the other
for two stops, assembled with the same crate that reads it back, so the round
trip is exercised without a binary fixture; the one-pixel read the readout
uses is held, over every pixel, to the crate's own reconstruction. Two of
the GPU tests draw a picture with a map and hold the device to that read,
texel for texel, at three weights and through the coarse chain. A HEIF's `tmap` item is tested
the same way, over a `meta` box assembled in the test around the 62 bytes
of metadata an iPhone wrote, since no encoder in reach writes one; the
picture itself is not, and an iPhone's own file is what checks the whole.

The GPU tests in `render/filter_tests.rs` run the real pipeline on a real
adapter — a headless device, no window — and check what the shaders and the
passes actually produce against arithmetic done on the CPU. Every place the
CPU keeps a twin of a shader is held to the device this way: the false-color
ramps, the tone curves on both kinds of surface and the clip a false color
holds them at, the PQ curve an HDR10 surface takes, and the gain map's lift.
Of the filters: that minification is the exact mean of the texels a pixel
covers, that
two levels of the coarse chain plus the draw's own filter come to the same
number as averaging the source directly, that antialiased nearest is exactly
nearest at a whole-number zoom, that Catmull-Rom passes texel centers through
untouched, that a transparent texel does not bleed its color into its
neighbor, that the minimap's thumbnail lands beside the view as a second
draw of the same texture — building the coarse chain the view itself had no
use for — and that a frame written into the texture the last one occupies
is what the next draw shows, through a chain built again from it. Where no
adapter can be had they report success rather than failing for a reason that
has nothing to do with the code; on a machine that has one, setting
`GAMUT_REQUIRE_GPU` makes a run that could not open it fail instead, which is
how a run proves they ran.

`test_images/` holds a hundred real fixtures — see its README — covering every pixel
layout the decoder can produce and every per-format encoding with its own code
path: PNG bit depths, palettes and interlacing; progressive and subsampled
JPEG; TIFF compressions, byte orders, tiling, BigTIFF, the floating-point
predictor, signed samples and no-data; Radiance RGBE; EXR associated alpha;
HEIC monochrome, 10-bit, `irot` and its color tags, and the same container
with AV1 inside; WebP in both bitstreams, with and without alpha, tagged,
rotated and animated; GIF interlaced, transparent and animated; an animated
PNG and a two-page TIFF. Every animated fixture is the pattern followed by
the pattern upside down, so that a frame read past the first is visibly not
the first, and every one is walked to its end and rewound. Some exist for the
color tags in particular: a PNG carrying `cICP` for BT.2100 PQ, and a PNG, a
HEIF, a JPEG XL, a WebP and an ICO's PNG entry each carrying an ICC profile
for Display P3. Each is checked for
dimensions, channel layout, sample type, color space, alpha mode and actual
pixel values, and then pushed through the upload planner under both GPU
capability sets. A test asserts the directory and the fixture table stay in
step, so a file cannot be added without a test.

Regenerate them with `test_images/generate.sh`, which needs ImageMagick,
`heif-enc`, GDAL and Python. Neither `cICP` nor `iCCP` is a chunk ImageMagick
will write, so those two are spliced in afterwards with their CRCs computed,
and its WebP writer emits neither an `EXIF` chunk nor an animation, so those
two fixtures are assembled around the bitstreams it did write; its APNG
writer is a video delegate that lands a code value off, so that one is
assembled from two stills' chunks as well.
`display-p3.icc` sits beside the fixtures as an input rather than an output;
`examples/make-icc.rs` is what produced it.

The one raw fixture is a DNG, because only a camera can write any other raw
format; a script in `generate.sh` mosaics the pattern and writes the matrix
that makes the camera's space Rec. 2020. Real cameras' files are tested too,
but not from the tree: `test_images/raw-samples/fetch.sh` brings one file of
each format down from raw.pixls.us into a directory git ignores, and
`decode::raw`'s `samples_are_recognized_probed_and_developed`, ignored
unless asked for, runs each through recognition, the probe, the develop,
the preview and the metadata. See [formats](formats.md#camera-raw).

The session bus is the other thing a test machine need not have.
`dbus.rs`'s tests marshal and parse messages against bytes built in the
test, in both byte orders; `talks_to_the_live_session_bus`, ignored unless
asked for, then authenticates to the real bus, says `Hello`, calls
`ListNames`, and checks that an unknown method comes back as the named
error it should. The dialog itself is not driven by a test, since it would
put a window on whoever's desk ran it; `portal.rs` tests what goes into the
call and what is made of the answer.

