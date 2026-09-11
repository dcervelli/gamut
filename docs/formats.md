# Formats

| Format | Backend |
| --- | --- |
| PNG, JPEG, GIF, Radiance HDR, OpenEXR, BMP, netpbm | [`image`](https://crates.io/crates/image) |
| TIFF | [`tiff`](https://crates.io/crates/tiff) directly |
| HEIF — HEIC, AVIF | [`libheif-rs`](https://crates.io/crates/libheif-rs), onto the system `libheif` |
| WebP — lossy, lossless, animated | [`image-webp`](https://crates.io/crates/image-webp) directly |
| GIF's frame count and loop extension | [`gif`](https://crates.io/crates/gif), which `image` already carries |
| JPEG XL — codestream and container | [`jxl-oxide`](https://crates.io/crates/jxl-oxide) |
| ICO | own directory reader, onto the PNG path and [`image`](https://crates.io/crates/image)'s bitmap one |
| Ultra HDR containers, ICC profiles | [`ultrahdr-rs`](https://crates.io/crates/ultrahdr-rs), [`moxcms`](https://crates.io/crates/moxcms) |
| PNG `cICP` and `iCCP` chunks | [`png`](https://crates.io/crates/png), which `image` already carries |

The decoder is chosen by content, falling back to the file extension for
formats without a recognizable header. Files are streamed rather than read
into memory whole, so opening a 600 MB raster does not begin by copying it —
JPEG excepted, because its gain map sits past the pixels and the reader that
finds it needs the file as one slice.

## Saying what the numbers mean

Five formats state their color space rather than leaving it to convention,
and they do it in two vocabularies.

**CICP code points** — the small integers of ITU-T H.273 — are the precise
form, because they name a transfer function this program models exactly. HEIF
carries them in an `nclx` box, PNG in a `cICP` chunk, and JPEG XL derives them
from its enum color encoding; one translation in `decode::cicp` serves all
three, which is what made the third of them a matter of reading two bytes. A `cICP` chunk is the whole of how a PNG says it is BT.2100
PQ or HLG, and `image` surfaces nothing of it, which is why `png` is a direct
dependency for the header pass.

**ICC profiles** are the other form, and the one a phone JPEG uses — and the
only form WebP has. Only what
this program's color model can act on is taken from a profile: the primaries,
matched against the four it can name by comparing colorants rather than by
reading description text, and a transfer function only where the profile
states a plain power law. Where a file carries both vocabularies the code
points win.

PQ and HLG are treated as display-referred alongside sRGB. They are absolute
curves — 1.0 is reference white and the headroom above it was put there on
purpose — so the startup window stays at 0..1 and the tone map deals with what
is above, rather than stretching each frame's observed range and undoing the
grading.

## JPEG: gain maps

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

## TIFF

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
an elevation model holds meters, and −86 at the Dead Sea is a real value, not
something to normalize away. A no-data sentinel is read from the file and kept
out of the statistics, so a clipped DEM's −9999 fill cannot set the bottom of
the automatic window and squash the terrain into a sliver.

A TIFF is a chain of directories, and where there is more than one they are
pages: `sequence` walks the chain reading directories only, and
`decode_page` seeks to one and reads it as `decode` reads the first. Nothing
tells a page from an overview or a thumbnail, so a pyramid's reduced copies
count as pages too.

## HEIF

The only decoder that is not pure Rust, because there is no usable pure-Rust
HEVC decoder to bind to instead. `libheif` also gets AVIF and whatever else
its plugins can open for free, since HEIC and AVIF differ only in the codec
inside the same container.

It is the system's `libheif`, and it has to be 1.23 or newer: the binding is
built against the `v1_23` security-limits ABI, so an older library is a link
error rather than a degraded build. Which codecs then work is a property of
that installation's plugins and not of this tree — HEVC wants libde265 or
ffmpeg, AV1 wants dav1d or aom.

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

## JPEG XL

Pure Rust, which is what separates it from the other modern container here: no
shared library for the package to depend on and no C toolchain for the build.
`jxl-oxide` renders into whatever color encoding the file declares and hands
the samples over without converting them, which is the same division of labor
`decode` is built around — so `decode::jxl` spends its length describing what
came back rather than fixing it.

**Depth follows what the file was authored at, not how it is coded.** JPEG XL
is float from end to end internally, so taking the internal form at face value
would quadruple what an ordinary photograph costs on the way to the GPU.
`ImageHeader::metadata::bit_depth` says what was meant, and `depth_of` turns it
into one of the three sample types: 8 bits or fewer to `U8`, 9 to 16 to `U16`,
and a float file — or an integer one wider than a `u16` holds — to `F32`, where
the highlights above 1.0 that are the point of it survive.
`ImageStream::write_to_buffer` scales into the full range of whichever type it
is handed, which is exactly what `Samples::full_scale` downstream assumes.

**Color arrives in both vocabularies**, and both already had a translation
here. `rendered_cicp` gives the code points for a file with an enum encoding,
and `original_icc` the profile for one without; the CICP form wins where there
is one, as it does for HEIF. Both are asked of the *rendered* encoding rather
than the stored one, so what is described is what the buffer actually holds.

**Reading is split in two.** `Reading::header` feeds the source only as far as
the image header and stops; the size ceiling is applied against what that
header claims; and only then does `Reading::finish` hand over the rest of the
file. `jxl-oxide`'s own `JxlImage::builder().read()` does both at once, which
would mean deciding an image was too large after having taken it in. The same
split is what lets `dimensions` answer from the header without decoding, and
because `JxlImage::width` reports the size with the orientation already
applied, the size the window opens at and the size the pixels arrive at cannot
disagree — `jxl-quarter-turn.jxl` is 24×32 on disk and 32×24 in both answers.

**One thing the format can hold is deliberately not taken.** A CMYK file is
refused by name: separating it needs an output profile this program does not
have, and `Channels` has nowhere to put four color components, so a guess would
read as a decode. An animation is its keyframes rendered one at a time, each
whole by construction; the decoder keeps every frame it has read, so going
back to the start is an index reset and no reading. The header states the
tick rate and the loop count, and a frame's duration is a count of ticks —
see [animation.md](animation.md) for what is done with them.

The two signatures are unrelated: a bare codestream opens `ff 0a`, and the
container opens with a `JXL ` box. That box is not `ftyp`, so the HEIF family
cannot claim a JPEG XL and vice versa — which is worth a test, the two
decoders being neighbors in `DECODERS`.

## WebP

Both bitstreams, and the container that can hold either. VP8 is lossy, coded
as YCbCr 4:2:0 and upsampled on the way out; VP8L is lossless and exact. Alpha
arrives two ways — a bit in the VP8L header, or an `ALPH` chunk beside a lossy
frame — and is straight in both. Neither bitstream has anything above eight
bits or outside three color channels, so a WebP is always `U8` and always
RGB or RGBA; there is no depth to preserve and no monochrome encoding to keep
one channel wide.

`image-webp` is a direct dependency rather than a feature of `image`, for the
same reason `png` is: everything worth having beyond the pixels lives in the
RIFF container, and `ImageReader` hands back only the pixels. Three chunks are
read.

`ICCP` is the only thing a WebP has to say about its own color — the format
carries no CICP code points — and it goes through the same profile reader
JPEG, PNG and HEIF use. Without one the file means sRGB.

`EXIF` carries the orientation, and it is applied. This is the one place a
metadata tag is honored rather than ignored, and it is a deliberate
exception: the tag sits in a chunk this decoder is already opening for the
profile, and reading it costs a rotation of a buffer that is already in hand.
JPEG's EXIF orientation still is not applied — same tag, different decoder,
and that one would have to grow a container pass to reach it.

`ANIM` and `ANMF` make the file an animation. A frame is not necessarily a
picture: the format lets it be a patch at an offset, blended onto a canvas
with the rectangle the last frame asked cleared cleared, so every frame is
read through `read_frame`, which composites it, and arrives whole. `decode`
shows the first that way; `frames` walks the rest for the player, and
`reset_animation` takes the decoder back to the start without the file being
opened again. The container states the frame count and the loop count, so
`sequence` answers from the header. The `ANIM` chunk also names a background
color, which browsers ignore and so does this: the canvas starts clear.

## GIF

GIF takes the plain route through `image`, because its container has nothing
to say that this program could act on: no profile, no code points, no
orientation, and a palette of sRGB bytes by definition. Every GIF comes back
RGBA whatever its palette holds — the crate's decoder has one output layout,
and the transparent index has to go somewhere. That index is also all the
transparency the format has: one palette entry is a hole, the rest are opaque,
and the pixel behind the hole carries no color at all rather than a color
with zero alpha the way a PNG's `tRNS` does.

The crate's `AnimationDecoder` composites every frame onto the logical screen
the file declares — disposal, transparency and offsets resolved — so a frame
stored as a patch at an offset arrives at the full size rather than cropped
to the patch. `decode` takes the first; `frames` walks them all. The crate's
iterator takes the decoder and the decoder takes the reader, so a rewind is a
fresh decoder over the same open file, seeked back to its start.

A GIF's header says nothing about how many frames follow, so `sequence`
counts them with the `gif` crate directly, decoding switched off: every
frame's bytes are read past and none is decoded, which is a read of the file
and no more. The same walk reads the loop extension, which `image` reports
wrongly — a file with no extension comes back as looping for ever, where
browsers play it once — and which counts *repeats*, so a file saying 1 plays
twice.

## BMP and netpbm

Both take the plain route through `image`, for the same reason GIF does, and
both needed a sniff written with more care than a signature usually asks for.

BMP's magic number is the two letters `BM`, which plain English wears often
enough to matter; netpbm's is `P` and a digit. Neither is worth trusting on
its own, so each is checked against what has to follow it — for BMP the size
of the DIB header, a small number from a known set, and for netpbm the
whitespace that has to separate the magic number from the width. Everything
else the two formats vary — bit depths, palettes, run-length codings, bitfield
masks, rows stored bottom-up or top-down, ASCII and binary spellings — the
crate resolves before it answers.

Netpbm's header does state one thing worth acting on: `MAXVAL`, the value a
fully bright sample has, which need not be the full width of the sample.
Instrument pipelines write 1023 or 4095, and a bitmap writes 1. That is the
same problem a 10-bit HEIF poses — `Samples::full_scale` says a `U16` image's
white is 65535, so a raster stated against 1023 would show at a sixteenth of
its brightness — but here the crate already rescales every sample before
handing the buffer over, so there is nothing to add beyond a fixture that
keeps it true. A `BITMAPV5HEADER`'s color space is the one thing genuinely
left on the floor; the crate does not surface it, and BMPs that carry one are
rare enough that reading the header a second time to find it would be work
spent on almost nothing.

## ICO

An ICO is not an image but a folder of them — the same picture at 16, 32, 48
and 256 pixels, so that Windows can pick the one that fits the slot it is
drawing into. A viewer has no slot, so it has to choose, and the choice is the
whole of what this decoder adds.

Every entry is a page, reachable by number through `decode_page`, and the
choice is which page `decode` shows first — `sequence` names it as the
default, so that the window opens at the size `dimensions` reported.

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
read — an icon can be Display P3 — and any pixel layout is kept, grayscale
included. `image` refuses anything but RGBA8 there, on the strength of a
Microsoft blog post saying embedded PNGs must be 32-bit; browsers display the
others, and so does this.

**BMP** is a headerless DIB with two Windows-specific quirks: the height in
its header counts the rows twice, and a 1-bit AND mask may follow the pixels
carrying transparency the color data has no room for — which is how a 4-bit
palette icon has a transparent background. `image` handles both, but only from
inside its own ICO decoder, whose hooks are `pub(crate)`, so the chosen entry
is handed back to it wrapped in a 22-byte container holding nothing else.
Rebuilding the DIB reader to avoid that would be the worse trade: it is a
decade of Windows bitmap variants, already written and already tested. Every
bitmap entry comes back RGBA whatever its stored depth, because the mask has
nowhere else to go.

Only type 1, the icon, is claimed. A cursor is the same container under the
`.cur` extension, but its directory overloads the color-plane and bit-depth
fields with the hotspot coordinates, so the numbers the selection sorts on
would mean something else entirely. One opened as `.ico` anyway is named as a
cursor rather than mis-sorted.

ICO also has no magic number worth the name — four bytes, three of them zero —
so the directory's own structure stands in for one when sniffing: a file
claiming entries it has no room for, or whose first entry starts inside the
directory that lists it, is not an ICO however its first four bytes read.

## Size ceiling

Every backend ships conservative allocation limits — 256 MiB in `tiff`,
512 MiB in `image`, `libheif`'s own security limits, and `jxl-oxide`'s
`AllocTracker` — which a survey-grade elevation model passes on the way out of
the door. All are raised to 4 GiB,
which is not an arbitrary number:
`max_texture_dimension_2d` is 32768 on current hardware, and 32768 × 32768 × 4
bytes is exactly 4 GiB, so the ceiling is the largest single-channel 32-bit
image that could be displayed even in principle. Going over it is refused from
the header alone, before anything is decoded, with a message naming the size.

## Adding a format

Decoders describe what they found rather than normalizing it:

```rust
pub trait Decoder: Sync {
    fn name(&self) -> &'static str;
    fn extensions(&self) -> &'static [&'static str];
    fn sniff(&self, header: &[u8]) -> bool;
    fn decode(&self, source: &mut dyn ReadSeek, overrides: Overrides) -> Result<DecodedImage>;
    fn dimensions(&self, source: &mut dyn ReadSeek) -> Result<Option<(u32, u32)>> { Ok(None) }
    fn sequence(&self, source: &mut dyn ReadSeek) -> Result<Sequence> { Ok(Sequence::Still) }
    fn decode_page(&self, source: &mut dyn ReadSeek, overrides: Overrides, page: usize) -> Result<DecodedImage>;
    fn frames(&self, source: BufReader<File>, overrides: Overrides) -> Result<Box<dyn FrameSource>>;
}
```

The last three have defaults — one image, page zero is that image, no frames
— and a format that holds more overrides them: `sequence` from the header,
`decode_page` for a file of pages, `frames` for an animation. What `decode`
returns has to be what `sequence` says is the default page, and
`dimensions` has to agree with both; the fixture tests hold every decoder to
that.

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

