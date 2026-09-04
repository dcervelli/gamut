# Image formats

What `gamut` opens, what it understands about each file beyond the
pixels, and where each format will surprise you.

| Format | Extensions | Depth kept | What the file can tell us |
| --- | --- | --- | --- |
| PNG | `.png` | 8 and 16-bit | Colour space, including HDR; ICC profile |
| JPEG | `.jpg` `.jpeg` `.jpe` `.jfif` | 8-bit | ICC profile; HDR gain map |
| GIF | `.gif` | 8-bit | Nothing — always sRGB |
| TIFF | `.tif` `.tiff` | 8 to 64-bit, integer or float | Nothing — inferred from depth |
| WebP | `.webp` | 8-bit | ICC profile; orientation |
| HEIF | `.heic` `.heif` `.hif` `.avif` | 8, 10 and 12-bit | Colour space, including HDR; ICC profile; orientation |
| ICO | `.ico` | Whatever the chosen icon holds | ICC profile, for the larger icons |
| BMP | `.bmp` | 8-bit | Nothing — always sRGB |
| Netpbm | `.pnm` `.pbm` `.pgm` `.ppm` `.pam` | 8 and 16-bit | Nothing — always sRGB |
| Radiance HDR | `.hdr` | 32-bit float | Nothing — linear by definition |
| OpenEXR | `.exr` | 32-bit float | Nothing — linear by definition |

Anything else is refused, with a message listing the extensions above. There
is no support for camera raw or DNG, SVG, PSD, JPEG 2000, JPEG XL, TGA, DDS
or ICNS.

## True of every format

**The contents decide, not the name.** A JPEG saved as `photo.png` opens as a
JPEG, and a file with no extension at all is fine. The extension is a fallback
for the one format whose first bytes are not distinctive, which is ICO.

**A directory stands for the images in it.** Name a directory instead of a
file and everything in it that looks like an image joins the list, in name
order, ready to step through. Only that directory is read, not the
directories inside it, and here the name is all there is to go on: a file is
taken for an image if its extension is one of those above, so a JPEG saved as
`photo.txt` is passed over where naming it yourself would have opened it.

**One picture per file.** Nothing here shows more than one:

- an animated GIF or WebP shows its first frame and stops — there is no
  playback and no way to step through the frames;
- a multi-page TIFF shows its first page;
- a HEIF holding several images, such as a burst or a Live Photo, shows the
  one it marks as primary;
- an ICO shows its largest icon;
- an EXR shows its first layer.

**Size limits.** An image may be at most 4 GB once decoded, and neither side
may be longer than 32768 pixels. That is the largest picture current graphics
hardware can hold at all. Too large is refused straight away, naming the size
it would have needed; a long, thin image can pass the first limit and fail the
second. Everything under them is allowed, including survey-scale rasters that
most viewers refuse.

**Loading is not incremental.** The window waits for the whole file, which for
a few hundred megabytes of compressed raster is a few seconds, and the image
is then held whole in memory and in graphics memory. Expect a large file to
cost somewhat more than its decoded size while it loads.

**An untagged file is treated as sRGB**, which is what every format here has
always meant in the absence of anything better. Where a file states its colour
space, that is believed instead; where it states it twice and the two
disagree, the more precise statement wins.

**Only part of an ICC profile is used.** The primaries are recognised if they
are sRGB, Display P3, BT.2020 or Adobe RGB, and the tone response is taken
only where the profile states a simple power law. A profile that describes
some other gamut, or that describes itself with lookup tables, or that is for
CMYK or another non-RGB space, leaves the file shown as sRGB rather than
approximated.

**You can override the assumption.** `--transfer linear|srgb|pq|hlg|gamma:N`
and `--primaries bt709|p3|bt2020|adobe` replace whatever the file said or was
assumed to mean, for every file opened in that run. This matters most for
TIFF, which says nothing at all.

**Depth and channels survive.** A 16-bit scan stays 16-bit, floating-point
data stays floating point, and a greyscale image stays single-channel rather
than being expanded to colour — which is a quarter of the memory on a large
scan. The formats that cannot preserve them say so below.

**How a file opens follows from what it is.** Content already graded for a
display — sRGB, gamma, PQ, HLG, a JPEG with its gain map applied — opens
untouched at 0–1, because second-guessing the grade would be wrong.
Measurement data and other scene-referred content opens with an automatic
99.8% window, because values occupying a fraction of the nominal range
otherwise show as a black rectangle. Where that leaves highlights above white
— a PQ frame, a gain-mapped photograph — the neutral tone map is on from the
start rather than clipping, unless the picture is going out to an HDR surface,
where the highlights have somewhere to go and no curve is applied at all. All
of it is adjustable: `e` cycles the automatic window, `t` the tone map, `o`
the room above white, `z` resets. The information panel's "Referred to" line says
which kind of light a file was taken for.

**Rotation is usually ignored.** An image tagged with an orientation is shown
the way its pixels are stored, except in WebP and HEIF. If a JPEG from a phone
appears on its side, that is why.

**Failures are reported in the terminal.** Given several files, the first one
that opens is shown and the ones that did not are named on the way past. `]`
and `[` step over a file that cannot be decoded, so one bad file in a
directory cannot trap you.

**Live reload works for every format.** The file on screen is re-read within
about half a second of anything writing to it, keeping your pan, zoom and
display settings.

## PNG

Everything the format holds: 8 and 16-bit, greyscale or colour, with or
without alpha, indexed, interlaced, and the sub-byte depths, which arrive
expanded. Transparency on an indexed image becomes a real alpha channel.

PNG is one of only two formats here that can state outright that it is HDR,
and when it does — BT.2100 PQ or HLG — that is read and honoured. An ICC
profile is read where there is no such statement, so a Display P3 PNG shows as
Display P3.

An animated PNG shows its default image, the still picture that any
non-animated reader sees.

## JPEG

Baseline and progressive, 8-bit, greyscale or colour, at any chroma
subsampling. Arithmetic-coded and lossless JPEG do not open, nor do 12-bit
files; all three are rare and none is produced by a camera.

**The ICC profile is read**, which matters more here than anywhere else: a
photograph from a phone is Display P3 far more often than it is sRGB, and P3
numbers shown as sRGB come out visibly flat.

**Gain maps are applied.** A JPEG from a recent phone is two pictures: the
ordinary graded photograph every viewer has always shown, and a smaller *gain
map* recording how much brighter than white each pixel really was. Both are
read and recombined, so the file arrives as a genuine HDR image — tone mapped
on an ordinary display, sent out at full brightness on a monitor in HDR mode
(see `o` in KEYS.md). The whole boost is applied rather than a share of it
guessed for your monitor; exposure (`d`, `f`) and the tone map (`t`) are where
you decide what to do with it.

- `--no-gain-map` shows the SDR photograph instead, which is the picture every
  other viewer shows and what you want when the two need comparing.
- Reconstruction is expensive in memory: a 12-megapixel photograph becomes
  roughly 200 MB where the photograph alone was 12 MB.
- A gain map that runs the other way — where the stored picture is the HDR one
  and the map describes the way down — is refused rather than applied
  backwards. The message suggests `--no-gain-map`.
- A JPEG carrying a second picture that is not a gain map, such as one half of
  a stereo pair, opens as an ordinary JPEG.

**CMYK JPEGs open**, but the conversion to RGB does not use the file's CMYK
profile, so the colours are approximate.

## GIF

Opens, and there is little to say: the format carries no colour information,
no orientation and no depth beyond 8-bit, and its palette is sRGB by
definition. Interlaced files are fine.

Transparency in a GIF is one palette entry that is a hole — all or nothing,
with no partial transparency anywhere in the format — and the pixel behind the
hole has no colour of its own, unlike a transparent pixel in a PNG.

An animated GIF shows its first frame on the full canvas the file declares, so
a first frame stored as a small patch still arrives at the right size rather
than cropped.

## TIFF

The widest range of any format here, and the least to say about itself.
Classic TIFF and BigTIFF, either byte order, striped or tiled, uncompressed or
compressed with LZW, Deflate, PackBits or CCITT Group 4:

- 8, 16 and 32-bit unsigned integers;
- signed integers and 64-bit values, widened to float with their signs intact;
- 16, 32 and 64-bit floating point;
- greyscale or colour, with or without alpha, and indexed colour.

Single-band floating-point rasters — which is what an elevation model or a
scientific image usually is — stay single-band all the way to the screen, at a
quarter of the memory that expanding them to colour would cost.

**Values are never rescaled.** An elevation model holds metres, and −86 metres
at the Dead Sea is a real reading, not something to normalise away. The
display window (`e`, `a`, `s`, `A`, `S`) is what brings a range into view, and
its bounds are reported in the file's own units.

**A no-data value is honoured** where the file records one, and kept out of
the statistics — so a clipped raster's −9999 fill cannot set the bottom of the
automatic window and squash the real terrain into a sliver.

Caveats:

- **A TIFF says nothing about colour.** No profile is read, and the tone
  response is inferred from depth: 8-bit is taken as sRGB, anything deeper as
  linear measurement data. That is right nearly always and wrong for a 16-bit
  *scanned photograph*, which looks washed out until you pass `--transfer
  srgb`.
- CMYK, YCbCr and Lab TIFFs are refused, naming the colour type. This takes
  JPEG-compressed TIFFs with it, since they are almost always stored as
  YCbCr; ZSTD- and WebP-compressed TIFFs do not open either.
- Multi-page files show the first page.
- The orientation tag is not applied.

## WebP

Both kinds and the container that holds either: lossy, lossless, and
transparency in either.

- The **ICC profile** is read, and is the only thing a WebP can say about its
  own colour. Without one it means sRGB.
- The **orientation is applied**, one of only two places a rotation tag is
  honoured.
- An **animated** WebP shows its first frame on the full canvas, so a first
  frame stored as a partial patch arrives whole.

Nothing in WebP goes above 8 bits or outside three colour channels, so there
is no depth to preserve and no greyscale encoding: a grey WebP is a grey
colour WebP.

## HEIF — HEIC and AVIF

The one format that does not have to be guessed at. Where a 16-bit TIFF leaves
you to work out whether it is a photograph or a frame of sensor readings, a
HEIF file states its colour space outright, so a Display P3 photograph from a
phone and a BT.2100 PQ frame both land in the right space with no flag from
you. An ICC profile is read where a file carries one instead.

10 and 12-bit files stay wide rather than being flattened to 8-bit, and
monochrome files stay monochrome. Rotation, mirroring and cropping recorded in
the file are applied, so a photograph taken sideways arrives upright.

Caveats:

- **This format needs support installed on the system.** HEIF decoding uses
  the system HEIF library, version 1.20 or newer, and `gamut` will not
  start without it: `libheif-dev` on Debian and Ubuntu, `libheif` on Arch,
  `brew install libheif` on macOS. Which *codecs* work then depends on that
  installation's plugins — HEIC needs libde265 or ffmpeg, AVIF needs dav1d or
  aom. Both are standard on the systems above; on a stripped-down one, a file
  can fail with a codec error where another HEIF opens fine.
- **HDR photographs from an iPhone show their SDR version.** HEIF can carry a
  gain map alongside the picture, which is how those files store the bright
  half, and it is not read. The photograph is correct, just not the bright
  one. A gain map in a JPEG *is* applied.
- A file holding several pictures shows the primary one.

## ICO

An ICO is not one image but a folder of them — the same picture at 16, 32, 48
and 256 pixels, so Windows can pick the size that fits where it is drawing. A
viewer has no such slot, so it has to choose, and this one shows the largest
icon, breaking a tie on colour depth. The others are not reachable.

That is deliberately the opposite of the usual choice, which is to prefer the
deepest icon: in a file whose 256-pixel icon uses a palette and whose 16-pixel
icon is full colour, preferring depth shows you a thumbnail.

Both kinds of icon are read. The larger ones are stored as PNG and get
everything the PNG support offers, so an icon can be Display P3 and can be
greyscale. The smaller ones are bitmaps, and their transparency mask is
applied, which is how a 16-colour icon has a transparent background; a bitmap
icon always arrives with an alpha channel whatever its stored depth.

A cursor (`.cur`) uses the same container with different fields and is not
shown. One renamed to `.ico` is reported as a cursor rather than misread.

## BMP

Opens, whatever the variant: 1, 4, 8, 16, 24 and 32 bits per pixel, palettes,
run-length compression, and rows stored either way up. Files with an alpha
channel keep it.

The format carries no colour information this program reads, so a BMP is
always taken as sRGB. Some are written with a colour space recorded in the
header, and that is ignored; if you know a particular file means something
else, `--transfer` and `--primaries` are the way to say so.

## Netpbm — PBM, PGM, PPM and PAM

Opens in every member of the family, in both the ASCII and the binary
spellings, at 8 or 16 bits per sample, greyscale or colour. PAM files carrying
an alpha channel keep it; the other three have no alpha to carry.

Any brightness scale is honoured. Netpbm lets a file declare what a fully
bright sample is, and instrument output often says 1023 or 4095 rather than
the full width of the sample — a file like that is read at the brightness it
was meant to have rather than a fraction of it.

The format says nothing about colour, so its files are taken as sRGB. That is
what the specification calls for, but a pipeline writing linear measurements
into a greyscale file is common and looks no different from the inside: if a
`.pgm` opens looking washed out, `--transfer linear` is the correction.

## Radiance HDR

Read as scene-linear floating point. There is nothing in the file to configure
and nothing to override: the format is linear by definition, whatever else it
might claim.

Because the data is scene-referred, it opens on an automatic window rather
than at 0–1, and the tone map (`t`) is what brings the highlights back.

## OpenEXR

Half and full floating point, uncompressed or with the common compressions
(RLE, ZIP, PIZ, PXR24, B44), read as scene-linear. Alpha is taken as
premultiplied, which is the format's own convention.

The limits are worth knowing before reaching for EXR as a data container:

- **Colour channels only.** A file whose channels are a single luminance,
  a depth pass, motion vectors or any other arbitrary output will not open.
  For single-channel measurement data, TIFF is the format that works here.
- Only the first layer is shown, and the others are not reachable.
- Deep images do not open.
- DWAA and DWAB compression is not supported.
- Pixels outside the display window are dropped, and the metadata — including
  any range the file records for its channels — is not read; the window is
  derived by scanning the pixels instead.
