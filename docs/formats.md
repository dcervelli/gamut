# Formats

| Format | Backend |
| --- | --- |
| PNG, JPEG, GIF, Radiance HDR, OpenEXR, BMP, netpbm | [`image`](https://crates.io/crates/image) |
| TIFF | [`tiff`](https://crates.io/crates/tiff) directly |
| HEIF — HEIC, AVIF | [`libheif-rs`](https://crates.io/crates/libheif-rs), onto the system `libheif` |
| Camera raw — DNG, NEF, CR2, CR3, ARW, RAF, ORF, RW2, PEF and the rest | `decode::raw::ffi`, a hand-written binding onto the system LibRaw |
| WebP — lossy, lossless, animated | [`image-webp`](https://crates.io/crates/image-webp) directly |
| GIF's frame count and loop extension | [`gif`](https://crates.io/crates/gif), which `image` already carries |
| JPEG XL — codestream and container | [`jxl-oxide`](https://crates.io/crates/jxl-oxide) |
| ICO | own directory reader, onto the PNG path and [`image`](https://crates.io/crates/image)'s bitmap one |
| Ultra HDR containers, ICC profiles | [`ultrahdr-rs`](https://crates.io/crates/ultrahdr-rs), [`moxcms`](https://crates.io/crates/moxcms) |
| PNG `cICP`, `iCCP`, `gAMA`, `cHRM` and `eXIf` chunks | [`png`](https://crates.io/crates/png), which `image` already carries |

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
three, so a further format that carries them is a matter of reading two
bytes. A `cICP` chunk is the whole of how a PNG says it is BT.2100 PQ or
HLG, and `image` surfaces nothing of it, which is why `png` is a direct
dependency for the header pass.

**ICC profiles** are the other form, and the one a phone JPEG uses — and the
only form WebP and TIFF have. Only what
this program's color model can act on is taken from a profile: the primaries,
matched against the five it can name by comparing colorants rather than by
reading description text, and a transfer function only where the profile
states a plain power law — in either of ICC's two spellings of one, the
parametric type 0 and the one-entry `curv` that Adobe's own profiles use
for Adobe RGB's 2.2 and ProPhoto's 1.8. Where a file carries both
vocabularies the code points win.

PNG has a third, older vocabulary, read by `decode::png::header` where a
file carries none of the above and no `sRGB` chunk either, which the
specification has win over it: `gAMA`, the encoding exponent, and `cHRM`,
the primaries as chromaticities. Two `gAMA` values are named rather than
taken as a power law — 1.0 is `Transfer::Linear`, and the reason a renderer
writes the chunk at all, and 0.45455 is the specification's own value for an
sRGB picture, which every encoder writes beside sRGB data and which the sRGB
curve, not a power of 2.2, describes. `cHRM` goes through
`Primaries::from_chromaticities`, the coordinates' counterpart of the
colorant match. ImageMagick writes a `cHRM` naming sRGB's own primaries on
every PNG it saves, so most of the fixtures carry one, and it has to read as
sRGB; `png-gama-linear.png` and `png-chrm-p3.png` carry the other answers.

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
only JPEG decoder in the build; `ultrahdr-core` builds the table that says
what gain each of the map's 256 values stands for. The walk over the pixels
— the base decoded to linear through a table, the map sampled bilinearly at
each pixel, the product written out — is `gain_map::reconstruct`, in bands
of rows across the thread pool. The crate has a walk of its own,
`apply_gainmap`, and it is what the tests check `reconstruct` against; it is
not what runs because it decodes sRGB with a `powf` per sample on one
thread, 460 ms of the 560 a 12-megapixel phone photograph took to open
against 60 for the JPEG decode. What comes out is linear light with 1.0 at
SDR reference white, which is already the working space, so past the
decoder an Ultra HDR photograph is simply an HDR image: tone mapped on an
SDR surface, sent out untouched on an HDR one.

The whole boost is applied rather than a share of it chosen for an assumed
display. This viewer has an exposure control and a choice of tone mapping
already, and guessing here how bright the monitor is would only take that
choice away. `--no-gain-map` shows the SDR base image instead, which is worth
having when the two need comparing.

The same pass reads the ICC profile, because a phone JPEG is Display P3 far
more often than it is sRGB, and P3 numbers shown as sRGB come out visibly
flat.

## Orientation

Every format that carries an orientation tag has it applied, and the
turn is one function, `decode::orient::apply`, whichever tag asked for it.
HEIF and JPEG XL keep their rotation in the container and their libraries
apply it while decoding; the other four keep EXIF's tag beside the pixels,
and each decoder reads it its own way:

- JPEG from `image`'s own `JpegDecoder`, built by hand over the bytes
  rather than reached through `ImageReader`: the decoder keeps the `APP1`
  segment it passed on the way to the pixels and answers `orientation()`
  from it, and `ImageReader` would hand back the pixels alone. An Ultra HDR
  file's tag is the primary image's, and the reconstruction is turned the
  same way.
- TIFF from the `Orientation` tag of the directory being read, so a page
  keeps its own.
- PNG from the `eXIf` chunk, on the same header pass that reads its color
  chunks; an animation's frames are each turned as the still is.
- WebP from its `EXIF` chunk, which its decoder opens beside the profile.

The turn is written here rather than borrowed from `image`'s
`apply_orientation` because that takes a `DynamicImage`, which has no
single-band floating-point layout, and a one-band elevation model is
exactly the TIFF that might carry the tag. `orient::tests` checks all eight
values against the crate's turn where both can do it, so the meaning of
each is the crate's — and the browsers' — rather than a reading of the
standard made here. Every decoder's `dimensions` reports the size after the
turn through `orient::size`, so the window opens in the shape the picture
arrives in: each of `jpeg-quarter-turn.jpg`, `png-quarter-turn.png`,
`tiff-quarter-turn.tif` and `jxl-quarter-turn.jxl` is 24×32 on disk and
32×24 in both answers.

The one picture decoded as stored is the JPEG a raw carries of itself.
LibRaw reads the camera's orientation from the raw's own header and
`decode::raw` turns the preview by that; a preview that repeats the tag in
an EXIF of its own — a RAF's does — would otherwise be turned twice, so it
goes through `jpeg::decode_stored`.

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

What the numbers mean is the one thing the container is bad at saying. An
embedded profile (`IccProfile`, tag 34675) settles it where there is one:
a measurement carries none, and a picture out of Lightroom or Photoshop
carries the one it was graded in, so `tiff_rs::color_space` reads it through
`icc::color_space` with sRGB assumed for whatever it does not state, at any
depth — `tiff-icc-p3-16.tif` is the 16-bit file that would otherwise be
taken for linear counts. Without a profile, depth is the best signal there
is: 8-bit files are overwhelmingly pictures and deeper ones overwhelmingly
measurements, and `--transfer` is the override for the 16-bit scan saved
without one.

A TIFF is a chain of directories, and where there is more than one they are
pages: `sequence` walks the chain reading directories only, and
`decode_page` seeks to one and reads it as `decode` reads the first. Nothing
tells a page from an overview or a thumbnail, so a pyramid's reduced copies
count as pages too. A transparency mask is told apart, by the
`NewSubfileType` bit GDAL sets on the internal masks it writes — one after
the picture, one after each reduced copy — and `tiff_rs::pages` leaves those
out of the count and the numbering, since a mask is the coverage of the
picture before it, not a picture, and at one bit a pixel not one the crate
would decode either. `tiff-mask.tif` is the fixture, a mask between two
pages. The mask is not applied as alpha.

The pixels are read a chunk at a time — a strip or a tile, each compressed
on its own — with the rows of chunks divided between rayon's threads, rather
than through the crate's `read_image`, which decodes them one after another:
a 14000×9600 LZW map took 1.7 s that way and takes 150 ms across 32 cores.
The crate's `Decoder` reads through one file position, so each band opens a
decoder of its own over the same file. That needs a reader per thread on one
descriptor, which is what `decode::Positioned` is: it keeps its position in
itself and reads with `pread`, so the duplicate descriptor `ReadSeek::share`
hands over — whose offset is shared with the original — is never seeked.
The file-backed case is the only one that divides; bytes held in memory, and
the rare planar layout that keeps each channel's chunks apart, go through
`read_image` on the loader's thread. `tiff-strips.tif` and
`tiff-tiled.tif` are the fixtures that read on more than one band.

A JPEG-compressed TIFF — what GDAL writes for a scanned map or an aerial
photograph with `COMPRESS=JPEG` — stores its pixels as YCbCr with the chroma
subsampled, and the crate hands them back that way: it has the JPEG decoder
upsample the chroma but not convert it, because the conversion is the
container's to define, through `YCbCrCoefficients` and `ReferenceBlackWhite`,
and the crate has no side channel to pass those tags out on. So
`tiff_rs::YCbCr` reads the two tags — with TIFF 6.0's defaults, BT.601's
weights and JPEG's own coding range, where a file leaves them out — and
converts each chunk as it is read, on whichever thread read it, with libtiff's
arithmetic: each channel's code mapped by the reference onto the full range,
then the luma equation undone. `tiff-jpeg.tif` is the fixture, tiled so that
the bottom row of tiles is clipped. A YCbCr file that is not JPEG-compressed
but subsampled all the same is refused by the crate, since nothing would
upsample it.

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

A phone's HEIC is a grid of tiles, and `libheif` decodes them on a thread
pool of its own that is four deep by default; the decoder sets it to the
machine's core count, which halved the decode of an iPhone's 24-megapixel
frame on 32 cores. Rows of samples are then packed out of `libheif`'s
buffer on one thread, which is a few percent of the whole.

`libheif` applies the container's own geometric properties — `irot`, `imir`,
`clap` — while decoding, so a rotated phone photograph arrives upright. That
is a property of the format rather than a choice made here; the formats that
keep the tag beside the pixels are turned by [`decode::orient`](#orientation).

## Camera raw

A raw file is the sensor's counts, one color per photosite under the
color filter array, with the camera's white balance and its color matrix
written beside them. Making a picture of that — demosaicing, balancing,
converting out of the camera's own primaries, and the special case every
camera model is — is what [LibRaw](https://www.libraw.org) does, and every
raw developer on the desktop reads its files through it. So `decode::raw`
binds the system library, the way HEIF does, and does not develop anything
itself. The library's C interface is a handle and free functions, and the
dozen this program calls are declared by hand in `decode::raw::ffi`: the two
bindings on crates.io were last published in 2015 and 2021, against
libraries whose structs have moved since. Two structs are transcribed, for
the two facts the C interface has no accessor for — the orientation the
camera recorded, which decides whether the picture is taller than it is
wide, and the color count. `build.rs` pins the library at 0.21 or newer,
where those structs took their present shape, and `Handle::check_layout`
compares the transcription against the accessors the library does have when
a file is opened, so a layout that has moved is an error rather than a
garbled size.

Every other decoder is pure Rust or, for HEIF, a crate's binding; this one
is a binding of its own. It is also the second system library the package
depends on, and it arrives under LGPL-2.1 or CDDL-1.0 at the taker's choice
— a dynamically linked library rather than a crate, so `about.toml`'s
allowlist, which is over the crate graph, has nothing to say about it, and
the PKGBUILD's `depends=()` is where it is recorded. The pure-Rust
alternatives do not serve: `rawler` and `rawloader` are LGPL-2.1 crates,
which the allowlist refuses on purpose, and `rawkit` reads one make of
camera.

What LibRaw is asked for is the least developed picture it can make. AHD
demosaic, dcraw's default and what every other developer is compared to;
the camera's own white balance, handed back as the user's multipliers
because the C interface has no switch for "use the camera's" and the
arithmetic is the same; no auto-brightening; gamma 1 with a toe slope of 1,
which is dcraw's `-g 1 1`; sixteen bits; and Rec. 2020 as the output space,
the widest this program names, so that a saturated flower the sensor
recorded clips less than it would in sRGB. The result is linear light with
1.0 at the sensor's saturation point. That is a photograph with a white, so
the decoder marks it display-referred: the window opens at 0..1 rather than
being stretched to whatever the frame holds, and an underexposed frame
arrives dark, as it was shot. `--transfer` and `--primaries` relabel it like
anything else.

**Recognition** is the part the library does not do. A CR3 is an ISO media
file of the `crx ` brand, and CRW, ORF, RW2, RAF, MRW and IIQ each start
with bytes of their own; those are read at their offsets. The rest — DNG,
NEF, NRW, ARW, PEF, SRW, 3FR and the older ones — wear TIFF's four-byte
header, and `decode::tiff_rs` asked first would show a NEF's 160×120
thumbnail, which is what its first directory holds. So `raw::Raw` comes
first in `DECODERS` and claims a TIFF only when its first directory says a
camera wrote it: a `DNGVersion` tag; a `CFA` or `LinearRaw` photometric
interpretation; a compression code that is one vendor's own; a first
directory that is a reduced copy pointing at sub-directories, which is
Nikon's and Sony's layout; or one that holds no picture at all, only the
camera's name and the sub-directories, which is Samsung's. A scan matches
none of these and goes to `tiff_rs`. Reading the directory is why
`decode::HEADER` is 4096 bytes: a camera writes its first directory at byte
8 with a few dozen entries. `raw::tests` covers each rule
with a directory built by hand, and
`samples_are_recognized_probed_and_developed` — ignored unless asked for —
runs real cameras' files through recognition, the probe, the develop, the
preview and the metadata. The files are not in the tree, being tens of
megabytes each: `test_images/raw-samples/fetch.sh` brings one of each
format down from raw.pixls.us, where photographers have put a file of
nearly every camera under CC0, into a directory git ignores. Fifteen
cameras' files pass it.

The file is read whole and handed to `libraw_open_buffer`, as JPEG is read
whole for its gain map: LibRaw reads by seeking about a stream, a buffer is
the one form of stream its C interface takes short of a path, and a raw is
tens of megabytes, which is a few milliseconds of copying beside the few
hundred the demosaic takes. The probe reads it whole too, for the header's
sake; the second read comes from the page cache.

**What it costs.** A 24-megapixel Bayer frame develops in 400–600 ms on
this machine, LibRaw's OpenMP threads doing the demosaic; an X-Trans frame
takes three times that, its interpolation being three passes rather than
one.

**The preview.** Every raw carries the camera's own JPEG of the frame,
which LibRaw copies out without decoding anything — `unpack_thumb` and
`make_mem_thumb` — and `Raw::preview` hands it back through the JPEG
decoder, turned by the orientation the header holds, since the JPEG is
stored as the sensor saw the scene. That is what `Decoder::preview` is, and
the thumbnailer asks every format for one before it decodes: a likeness is
all a thumbnail is, and a preview arrives in 3–90 ms against 200–1400 for
a develop. It is used only when its longer side reaches `thumbnail::SIDE`,
so the cache never holds something blurrier than the format can give; of
the fifteen cameras sampled the smallest preview is 644 pixels wide and
most are the full frame. A thumbnail of a raw therefore looks like the
camera's JPEG — its curve, its balance — rather than the flat linear
picture the viewer opens; for finding a file that is the better likeness.
`dynamic::reorient` is the turn.

**The metadata.** The panel reads a raw's EXIF where a TIFF-shaped one
keeps it, at the front, and most formats are TIFF-shaped. Five are not, and
`image/enclosed.rs` finds the block each keeps inside: an ORF or RW2 is a
TIFF under its own four bytes, a RAF names the offset of a JPEG whose
`APP1` is the EXIF, an MRW has a `TTW` block that is a TIFF, and a CR3 keeps
four one-directory TIFFs in boxes under Canon's `uuid` — which, read alone,
put Exif tags in the image's directory where they mean nothing, so three of
them are written back out as one TIFF with the offsets moved. A CRW has no
EXIF anywhere. What every raw has is LibRaw's own reading of its header,
and `raw::facts` turns that into the panel's `Sensor` section — the frame
and the picture inside it, the filter cell spelled from dcraw's bit
pattern, the white level, the as-shot and daylight balances, the camera
matrix, the DNG version — and into the entries of `Camera`, which
`Exif::read` takes whatever of from that the EXIF did not say: all of it
for a CRW, the exposure for a Phase One. Two structs more are transcribed
for it, `Other` and the front of `Lens`, both reached through accessors so
that only their leading fields have to be right.

`dng-cfa.dng` is the fixture: the one raw format anything but a camera can
write, mosaiced RGGB by a script in `generate.sh`, twelve-bit counts in
sixteen-bit words so that the white level has to be read, and a color
matrix that makes the camera's space Rec. 2020 exactly, so the developed
quadrants are the pattern with nothing to balance or convert. AHD's
interpolation is exact on a flat field; the tolerance is for LibRaw's
output matrices, which are written to four decimal places.
`bad-truncated.dng` is the same file cut inside its directory, claimed by
the entries that survive and then refused by the library.

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

**Color arrives in both vocabularies**, and both have a translation here.
`rendered_cicp` gives the code points for a file with an enum encoding,
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

`EXIF` carries the orientation, and it is applied — see
[orientation](#orientation).

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
both need a sniff written with more care than a signature usually asks for.

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
    fn preview(&self, source: &mut dyn ReadSeek, overrides: Overrides) -> Result<Option<DecodedImage>> { Ok(None) }
    fn frames(&self, source: BufReader<File>, overrides: Overrides) -> Result<Box<dyn FrameSource>>;
}
```

The last five have defaults — no size, one image, page zero is that image,
no preview, no frames — and a format that holds more overrides them:
`sequence` from the header, `decode_page` for a file of pages, `preview` for
a format that carries a smaller picture of itself, `frames` for an
animation. What `decode` returns has to be what `sequence` says is the
default page, and `dimensions` has to agree with both; the fixture tests
hold every decoder to that.

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

