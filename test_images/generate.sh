#!/bin/bash
# Regenerates the decoder fixtures. Run from this directory.
#
# Every image is the same 32x24 pattern of four 16x12 quadrants, so a test can
# probe the center of each quadrant and know what it should find:
#
#   color:  top-left red     top-right green    bottom-left blue   bottom-right white
#   gray:    top-left 0       top-right 85       bottom-left 170    bottom-right 255
#   alpha:   top-left 255     top-right 191      bottom-left 128    bottom-right 64
#   float:   top-left 0.0     top-right 0.5      bottom-left 1.0    bottom-right 3.984
#
# Three files are deliberately spelled `.jpeg`, `.tiff` and `.heif` rather
# than `.jpg`, `.tif` and `.heic`, so that both extension spellings are
# exercised by the router.
#
# The point is coverage of decode paths, not pretty pictures: every pixel
# layout a decoder can hand back, plus the per-format encodings that have
# their own code path (bit depths, palettes, interlacing, progressive JPEG,
# TIFF compressions and byte orders, HEIF color tags and transformations).
#
# `display-p3.icc` is an input rather than an output: it is checked in beside
# the fixtures and is not regenerated here. See `examples/make-icc.rs`.
set -euo pipefail

quad() { magick \( -size 16x12 "xc:$1" -size 16x12 "xc:$2" +append \) \
                \( -size 16x12 "xc:$3" -size 16x12 "xc:$4" +append \) -append "$5"; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Start clean, so a renamed fixture does not leave its predecessor behind.
rm -f ./*.png ./*.jpg ./*.jpeg ./*.tif ./*.tiff ./*.hdr ./*.exr ./*.gif \
      ./*.heic ./*.heif ./*.avif ./*.webp ./*.jxl ./*.ico ./*.bmp ./*.tga \
      ./*.pnm ./*.pbm ./*.pgm ./*.ppm ./*.pam ./*.dng

quad '#FF0000' '#00FF00' '#0000FF' '#FFFFFF' "$work/color.png"
quad '#000000' '#555555' '#AAAAAA' '#FFFFFF' "$work/gray.png"
quad '#FFFFFF' '#BFBFBF' '#808080' '#404040' "$work/alpha.png"
# 0, 0.5, 1.0, 4.0 once multiplied by four, with the values read as linear
# rather than converted from sRGB.
# 0x20 and 0x40 are exactly an eighth and a quarter of 0x100, so scaling by
# 255/64 lands on 0.5 and 1.0 exactly; the highlight ends up at 255/64.
quad '#000000' '#202020' '#404040' '#FFFFFF' "$work/float.png"

# `-set colorspace` rather than `-colorspace`: the mask's numbers are coverage
# already and must not be put through a luminance conversion.
with_alpha() {  # base -> base with the alpha quadrants applied
  magick "$1" \( "$work/alpha.png" -set colorspace sRGB -channel R -separate \) \
    -alpha off -compose CopyOpacity -composite "$2"
}
with_alpha "$work/color.png" "$work/color-alpha.png"
with_alpha "$work/gray.png"  "$work/gray-alpha.png"

# The pattern the wrong way up, for the fixtures that hold it and claim
# otherwise — a HEIF `irot`, a WebP `EXIF` orientation — and for the second
# frame of every animation and the second page of the paged TIFF, so that a
# frame or page read past the first is visibly not the first.
magick "$work/color.png" -rotate 180 "$work/color-upside-down.png"

# ---------------------------------------------------------------- PNG
magick "$work/gray.png"        -depth 8  -define png:color-type=0 png-gray8.png
magick "$work/gray-alpha.png"  -depth 8  -define png:color-type=4 png-gray-alpha8.png
magick "$work/color.png"       -depth 8  -define png:color-type=2 png-rgb8.png
magick "$work/color-alpha.png" -depth 8  -define png:color-type=6 png-rgba8.png
magick "$work/gray.png"        -depth 16 -define png:bit-depth=16 -define png:color-type=0 png-gray16.png
magick "$work/gray-alpha.png"  -depth 16 -define png:bit-depth=16 -define png:color-type=4 png-gray-alpha16.png
magick "$work/color.png"       -depth 16 -define png:bit-depth=16 -define png:color-type=2 png-rgb16.png
magick "$work/color-alpha.png" -depth 16 -define png:bit-depth=16 -define png:color-type=6 png-rgba16.png
magick "$work/color.png"       -define png:color-type=3 png-palette.png
# PNG8: keeps the palette and writes the transparency as a tRNS chunk.
magick "$work/color-alpha.png" PNG8:png-palette-alpha.png
magick "$work/gray.png" -depth 1 -define png:bit-depth=1 -define png:color-type=0 png-gray1.png
magick "$work/gray.png" -depth 4 -define png:bit-depth=4 -define png:color-type=0 png-gray4.png
magick "$work/color.png" -interlace PNG png-interlaced.png
# An animation, assembled from two stills. ImageMagick's own APNG writer is
# a video delegate and lands a code value off, so the chunks are written
# here: `acTL` looping for ever, then each still's `IDAT` stream behind an
# `fcTL` of a tenth of a second, the second as `fdAT`. The first frame is
# also the default image, so a reader with no notion of animation sees the
# pattern.
png_anim() {  # dst first second
  python3 - "$@" <<'ANIM'
import struct, sys, zlib

dst, first, second = sys.argv[1:4]

def chunks(path):
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", path
    offset = 8
    while offset < len(data):
        (length,) = struct.unpack_from(">I", data, offset)
        yield data[offset + 4 : offset + 8], data[offset + 8 : offset + 8 + length]
        offset += 12 + length

def chunk(kind, body):
    return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)

def fctl(sequence, width, height):
    # Full canvas, no offset, 1/10 s, no disposal, source blending.
    return chunk(b"fcTL", struct.pack(">IIIIIHHBB", sequence, width, height, 0, 0, 1, 10, 0, 0))

heads = [dict(chunks(path))[b"IHDR"] for path in (first, second)]
assert heads[0] == heads[1], "the two stills differ in layout"
width, height = struct.unpack_from(">II", heads[0])
streams = [b"".join(body for kind, body in chunks(path) if kind == b"IDAT") for path in (first, second)]

out = bytearray(b"\x89PNG\r\n\x1a\n")
out += chunk(b"IHDR", heads[0])
out += chunk(b"acTL", struct.pack(">II", 2, 0))
out += fctl(0, width, height)
out += chunk(b"IDAT", streams[0])
out += fctl(1, width, height)
out += chunk(b"fdAT", struct.pack(">I", 2) + streams[1])
out += chunk(b"IEND", b"")
open(dst, "wb").write(bytes(out))
ANIM
}
magick "$work/color-upside-down.png" -depth 8 -define png:color-type=2 "$work/upside-down.png"
png_anim png-animated.png png-rgb8.png "$work/upside-down.png"

# Neither `cICP` nor `iCCP` is a chunk ImageMagick will write, and between
# them they are the whole of how a PNG says what its numbers mean, so both are
# spliced in after `IHDR` with their CRCs computed.
png_tag() {  # cicp src dst p t m r  |  iccp src dst profile
  python3 - "$@" <<'TAG'
import struct, sys, zlib

mode, src, dst = sys.argv[1:4]
if mode == "cicp":
    # Color primaries, transfer function, matrix coefficients, full range.
    kind, body = b"cICP", bytes(int(value) for value in sys.argv[4:8])
else:
    profile = open(sys.argv[4], "rb").read()
    kind = b"iCCP"
    # Profile name, its terminator, the compression method, then the stream.
    body = b"ICC profile\x00\x00" + zlib.compress(profile, 9)

data = open(src, "rb").read()
assert data[:8] == b"\x89PNG\r\n\x1a\n", src
out, offset, done = bytearray(data[:8]), 8, False
while offset < len(data):
    (length,) = struct.unpack_from(">I", data, offset)
    chunk = data[offset : offset + 12 + length]
    out += chunk
    offset += 12 + length
    if chunk[4:8] == b"IHDR":
        out += struct.pack(">I", len(body)) + kind + body
        out += struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)
        done = True
assert done, "no IHDR"
open(dst, "wb").write(bytes(out))
TAG
}

# 16-bit tagged BT.2100 PQ on BT.2020 primaries: the HDR path for PNG, and the
# only way a PNG has of saying so.
png_tag cicp png-rgb16.png png-cicp-pq.png 9 16 0 1
# Display P3 with the sRGB curve, stated by a profile rather than by code
# points: what a phone writes, and what the ICC reader is for.
png_tag iccp png-rgb8.png png-icc-p3.png display-p3.icc

# ---------------------------------------------------------------- JPEG
magick "$work/color.png" -quality 95 -sampling-factor 4:4:4 jpeg-rgb.jpg
magick "$work/gray.png"  -colorspace gray -quality 95 jpeg-gray.jpg
magick "$work/color.png" -quality 95 -interlace JPEG jpeg-progressive.jpeg
magick "$work/color.png" -quality 95 -sampling-factor 4:2:0 jpeg-subsampled.jpg

# ----------------------------------------------------------------- GIF
# Always a palette, always 8-bit, and always RGBA once decoded: the crate's
# GIF decoder has one output layout and the transparent index has to go
# somewhere. The four here are the encodings with their own code path.
magick "$work/color.png" gif-palette.gif
magick "$work/color.png" -interlace GIF gif-interlaced.gif
# Transparency is a palette index rather than a channel, so it is all or
# nothing and the pixel behind it carries no color: white is named as the
# transparent one, and the quadrant comes back as four zeroes.
magick "$work/color.png" -transparent white gif-transparent.gif
# Two frames, the pattern first and the upside-down one second, so a decoder
# that ran the animation to its end would fail the table the rest pass.
magick -delay 10 -loop 0 \
  "$work/color.png" "$work/color-upside-down.png" gif-animated.gif

# ---------------------------------------------------------------- TIFF
magick "$work/gray.png"        -depth 8  -type Grayscale -compress None tiff-gray8.tif
magick "$work/color.png"       -depth 8  -type TrueColor -compress None tiff-rgb8.tif
magick "$work/color-alpha.png" -depth 8  -type TrueColorAlpha -compress None tiff-rgba8.tif
magick "$work/gray.png"        -depth 16 -type Grayscale -compress None tiff-gray16.tif
magick "$work/color.png"       -depth 16 -type TrueColor -compress None tiff-rgb16.tif
magick "$work/float.png" -set colorspace RGB -evaluate multiply 3.984375 \
  -depth 32 -type TrueColor -define quantum:format=floating-point -compress None tiff-float32.tif
magick "$work/color.png" -depth 8 -type TrueColor -compress LZW      tiff-lzw.tif
magick "$work/color.png" -depth 8 -type TrueColor -compress Zip      tiff-deflate.tiff
magick "$work/color.png" -depth 8 -type TrueColor -compress RLE      tiff-packbits.tif
magick "$work/color.png" -depth 8 -type TrueColor -compress None -define tiff:endian=msb tiff-bigendian.tif
magick "$work/color.png" -depth 8 -type TrueColor -compress None -define tiff:tile-geometry=16x16 tiff-tiled.tif
# Five strips of five rows, the last of four: more rows of chunks than the
# decoder has bands for them, and an edge that clips one.
magick "$work/color.png" -depth 8 -type TrueColor -compress LZW -define tiff:rows-per-strip=5 tiff-strips.tif
# Two directories, the pattern first and the upside-down one second: pages,
# with no clock between them.
magick "$work/color.png" "$work/color-upside-down.png" -depth 8 -type TrueColor -compress None tiff-pages.tif

# ---------------------------------------------------------------- Radiance
magick "$work/float.png" -set colorspace RGB -evaluate multiply 3.984375 hdr-rgbe.hdr
# The same picture with a `VIEW=` line ahead of the signature, as Debevec's
# memorial.hdr has: Radiance's own reader takes any text line first, and
# readers that insist on the signature in the first ten bytes refuse it.
{ echo 'VIEW= -vtv -vh 90 -vv 150'; cat hdr-rgbe.hdr; } > hdr-view-line.hdr
# The same picture with `EXPOSURE=` lines, as `pfilt -e` leaves one once it
# has scaled the picture to be looked at. Every line multiplies in: these
# two say 2. A picture that states its exposure has been given its white,
# and opens as stored rather than metered.
{ head -n 1 hdr-rgbe.hdr; echo 'EXPOSURE=4'; echo 'EXPOSURE=0.5'; tail -n +2 hdr-rgbe.hdr; } > hdr-exposure.hdr

# ---------------------------------------------------------------- OpenEXR
magick "$work/float.png" -set colorspace RGB -evaluate multiply 3.984375 exr-rgb.exr
# Composite while both are still sRGB-tagged so the coverage values pass
# through untouched, then reinterpret as linear and scale only the color.
magick "$work/float.png" \( "$work/alpha.png" -set colorspace sRGB -channel R -separate \) \
  -alpha off -compose CopyOpacity -composite \
  -set colorspace RGB -channel RGB -evaluate multiply 3.984375 +channel exr-rgba.exr
magick "$work/float.png" -set colorspace RGB -evaluate multiply 3.984375 \
  -define exr:compression=zip exr-zip.exr

# ---------------------------------------------------------------- HEIF
# `-L` is lossless, so the quadrants come back exactly and the same table of
# expected values serves these as serves PNG. The nclx tags are what makes
# this family different from every other format here: a HEIF file *states* its
# transfer function and primaries in CICP codes rather than leaving them to
# convention, so the color-space fixtures below are testing a translation
# rather than a guess.
magick "$work/color.png"       -depth 16 -define png:bit-depth=16 "$work/color16.png"

heif-enc -L --hevc -o heic-rgb8.heic        "$work/color.png"       > /dev/null
heif-enc -L --hevc -o heic-rgba8.heic       "$work/color-alpha.png" > /dev/null
# A grayscale input encodes as a monochrome image, and must stay one channel.
heif-enc -L --hevc -o heic-gray8.heic       "$work/gray.png"        > /dev/null
heif-enc -L --hevc -o heic-gray-alpha8.heic "$work/gray-alpha.png"  > /dev/null
# 10-bit, tagged BT.2100 PQ on BT.2020 primaries: the HDR path, and the one
# that has to be lifted from 0..1023 to the full 16-bit range on the way in.
heif-enc -L --hevc -b 10 --color_primaries 9 --transfer_characteristic 16 \
  -o heic-pq10.heic "$work/color16.png" > /dev/null
# Display P3 primaries (EG 432-1) with the sRGB curve: what a phone writes.
heif-enc -L --hevc --color_primaries 12 --transfer_characteristic 13 \
  -o heif-p3.heif "$work/color.png" > /dev/null
# An upside-down image that says it is upside down. `--rotate-cw` writes an
# `irot` property rather than turning the pixels, so this decodes back to the
# ordinary pattern only if the transformation is applied on the way out.
heif-enc -L --hevc --rotate-cw 180 -o heic-rotated.heic \
  "$work/color-upside-down.png" > /dev/null
# The same container with AV1 inside instead of HEVC.
heif-enc -L -A -o avif-rgb8.avif "$work/color.png" > /dev/null
# A HEIF that states its color space with an ICC profile and no `nclx` box,
# which is what some cameras write. `heif-enc` has no way to embed a profile,
# but ImageMagick carries the source PNG's through untouched — the pixels are
# not converted, because there is no target profile to convert to. HEVC at
# quality 100 is near-lossless rather than lossless, hence the tolerance on
# this one fixture.
magick png-icc-p3.png -quality 100 heic-icc-p3.heic

# ------------------------------------------------------------- JPEG XL
# `-d 0` is lossless, so the quadrants come back exactly and the same table of
# expected values serves these as serves PNG. Like the HEIF family, a JPEG XL
# *states* what its numbers mean rather than leaving them to convention — as
# enum codes that translate to CICP, or as an ICC profile — so those two
# fixtures test a translation rather than a guess.
#
# The `-x color_space` hint only takes on a raw format such as PPM: a PNG
# carries color information of its own, and the hint is passed over for it.

cjxl -d 0 "$work/gray.png"        jxl-gray8.jxl        > /dev/null 2>&1
cjxl -d 0 "$work/gray-alpha.png"  jxl-gray-alpha8.jxl  > /dev/null 2>&1
cjxl -d 0 "$work/color.png"       jxl-rgb8.jxl         > /dev/null 2>&1
cjxl -d 0 "$work/color-alpha.png" jxl-rgba8.jxl        > /dev/null 2>&1
cjxl -d 0 "$work/color16.png"     jxl-rgb16.jxl        > /dev/null 2>&1
# The alpha the file declares, rather than the alpha it holds: `--premultiply`
# sets the flag and leaves the samples alone, so the quadrants are the
# ordinary ones and only the label differs. That is what is being tested, and
# it is how `exr-rgba.exr` stands in the same table.
cjxl -d 0 --premultiply=1 "$work/color-alpha.png" jxl-premultiplied.jxl > /dev/null 2>&1
# VarDCT rather than Modular, which is the other half of the format and a
# different decode path entirely. Lossy, hence the tolerance on it.
cjxl -d 1 "$work/color.png" jxl-lossy.jxl > /dev/null 2>&1
# The ISOBMFF spelling. A bare codestream starts `ff 0a` and this starts with
# a `JXL ` box, so the two are recognized by different signatures.
cjxl -d 0 --container=1 "$work/color.png" jxl-container.jxl > /dev/null 2>&1
# BT.2100 PQ on BT.2020 primaries, stated as enum codes. As with `heic-pq10`
# the samples are not converted, only labeled, so the quadrants still hold the
# ordinary pattern.
magick "$work/color16.png" -depth 16 "$work/color16.ppm"
cjxl -d 0 -x color_space=Rec2100PQ "$work/color16.ppm" jxl-cicp-pq.jxl > /dev/null 2>&1
# Display P3 by ICC profile and no enum codes, which is the other way a file
# can say it: carried through from the tagged PNG, lossless, so it stays exact.
cjxl -d 0 png-icc-p3.png jxl-icc-p3.jxl > /dev/null 2>&1

# Orientation is a field in the codestream rather than a tag beside it, and
# `cjxl` fills it in from the input's EXIF. These two store the pattern the
# wrong way round and say so, so they come back the right way up only if the
# field is applied — and the quarter turn additionally swaps the size the
# header reports, which is what the window opens at.
png_exif() {  # src dst orientation
  python3 - "$@" <<'EXIF'
import struct, sys, zlib

src, dst, orientation = sys.argv[1], sys.argv[2], int(sys.argv[3])
# A bare TIFF header with one IFD entry: tag 0x0112, SHORT, the orientation.
tiff = (b"II\x2a\x00" + struct.pack("<I", 8) + struct.pack("<H", 1)
        + struct.pack("<HHIHH", 0x0112, 3, 1, orientation, 0) + struct.pack("<I", 0))
payload = struct.pack(">I", len(tiff)) + b"eXIf" + tiff
payload += struct.pack(">I", zlib.crc32(b"eXIf" + tiff) & 0xFFFFFFFF)
data = open(src, "rb").read()
# After the signature and `IHDR`, which is always the first chunk and always
# thirteen bytes of payload.
at = 8 + 25
open(dst, "wb").write(data[:at] + payload + data[at:])
EXIF
}

png_exif "$work/color-upside-down.png" "$work/upside-down-exif.png" 3
cjxl -d 0 "$work/upside-down-exif.png" jxl-rotated.jxl > /dev/null 2>&1
# Stored a quarter turn anticlockwise, tagged to be turned back: 24x32 on
# disk, 32x24 once the header is honored.
magick "$work/color.png" -rotate -90 "$work/color-quarter-turn.png"
png_exif "$work/color-quarter-turn.png" "$work/quarter-turn-exif.png" 6
cjxl -d 0 "$work/quarter-turn-exif.png" jxl-quarter-turn.jxl > /dev/null 2>&1

# Two frames, the pattern first and the upside-down one second, so a decoder
# that ran the animation to its end would fail the table the rest pass.
magick -delay 10 "$work/color.png" "$work/color-upside-down.png" "$work/animated.gif"
cjxl -d 0 "$work/animated.gif" jxl-animated.jxl > /dev/null 2>&1

# ---------------------------------------------------------------- WebP
# Both bitstreams, with and without alpha. VP8L is exact, so it shares the
# table of expected values with PNG; VP8 goes through YCbCr 4:2:0 and lands
# within a code value or two, like JPEG.
magick "$work/color.png"       -define webp:lossless=true webp-lossless-rgb8.webp
magick "$work/color-alpha.png" -define webp:lossless=true webp-lossless-rgba8.webp
magick "$work/color.png"       -quality 95 webp-lossy-rgb8.webp
# Lossy plus alpha is the one layout that needs the extended container: an
# `ALPH` chunk carrying the coverage beside a `VP8` chunk carrying the color.
magick "$work/color-alpha.png" -quality 95 webp-lossy-rgba8.webp
# Display P3 by ICC profile, carried through from the tagged PNG the same way
# `heic-icc-p3.heic` is. Lossless, so this one stays exact.
magick png-icc-p3.png -define webp:lossless=true webp-icc-p3.webp

# ImageMagick's WebP writer emits neither an `EXIF` chunk nor an animation, so
# the two container fixtures that need them are assembled here from bitstreams
# it did write — the same approach the PNG color tags above take.
webp_tag() {  # exif src dst orientation  |  anim dst frame...
  python3 - "$@" <<'TAG'
import struct, sys

def chunk(fourcc, payload):
    return fourcc + struct.pack("<I", len(payload)) + payload + (b"\0" if len(payload) & 1 else b"")

def riff(payload):
    return b"RIFF" + struct.pack("<I", len(payload) + 4) + b"WEBP" + payload

def chunks(data):
    assert data[:4] == b"RIFF" and data[8:12] == b"WEBP", "not a WebP"
    out, offset = [], 12
    while offset + 8 <= len(data):
        fourcc = data[offset : offset + 4]
        (size,) = struct.unpack_from("<I", data, offset + 4)
        out.append((fourcc, data[offset + 8 : offset + 8 + size]))
        offset += 8 + size + (size & 1)
    return out

def bitstream(path):
    """The coded picture of a still WebP, as whole chunks."""
    kept = [(fourcc, payload) for fourcc, payload in chunks(open(path, "rb").read())
            if fourcc in (b"VP8 ", b"VP8L", b"ALPH")]
    assert kept, path
    return b"".join(chunk(fourcc, payload) for fourcc, payload in kept)

def vp8x(flags, width, height):
    # Feature flags, three reserved bytes, then the canvas size less one.
    return chunk(b"VP8X", bytes([flags]) + b"\0\0\0" + three(width - 1) + three(height - 1))

def three(value):
    return value.to_bytes(3, "little")

EXIF, ANIM = 0x08, 0x02
WIDTH, HEIGHT = 32, 24

mode = sys.argv[1]
if mode == "exif":
    src, dst, orientation = sys.argv[2], sys.argv[3], int(sys.argv[4])
    # The chunk holds a bare TIFF header with one IFD entry: tag 0x0112,
    # SHORT, the orientation. Nothing else in EXIF is read.
    tiff = (b"II\x2a\x00" + struct.pack("<I", 8) + struct.pack("<H", 1)
            + struct.pack("<HHIHH", 0x0112, 3, 1, orientation, 0) + struct.pack("<I", 0))
    body = vp8x(EXIF, WIDTH, HEIGHT) + bitstream(src) + chunk(b"EXIF", tiff)
else:
    dst, frames = sys.argv[2], sys.argv[3:]
    # Transparent background, looping forever.
    body = vp8x(ANIM, WIDTH, HEIGHT) + chunk(b"ANIM", b"\0\0\0\0" + struct.pack("<H", 0))
    for frame in frames:
        # A full-canvas frame at the origin, 100 ms, no blending, no disposal.
        header = (three(0) + three(0) + three(WIDTH - 1) + three(HEIGHT - 1)
                  + three(100) + bytes([0b10]))
        body += chunk(b"ANMF", header + bitstream(frame))
open(dst, "wb").write(riff(body))
TAG
}

# The ordinary pattern stored upside down, with an `EXIF` chunk saying so.
# It decodes back to the ordinary pattern only if the tag is applied.
magick "$work/color-upside-down.png" -define webp:lossless=true "$work/upside-down.webp"
webp_tag exif "$work/upside-down.webp" webp-exif-rotated.webp 3
# Two frames, the pattern first and the upside-down one second, so a decoder
# that ran the animation to its end would fail the same table the rest pass.
# Both are full-canvas, which is what the `ANMF` headers written above say.
webp_tag anim webp-animated.webp webp-lossless-rgb8.webp "$work/upside-down.webp"

# ----------------------------------------------------------------- ICO
# A directory of icons rather than one image, in the two formats an entry can
# hold. `-type TrueColorAlpha` forces the 32-bit bitmap that carries the alpha
# ramp; left alone, ImageMagick quantizes the four-color pattern to a 4-bit
# palette, which is the other bitmap path and gets a fixture of its own.
magick "$work/color-alpha.png" -type TrueColorAlpha ico-bmp-rgba8.ico
magick "$work/color.png"                            ico-bmp-palette.ico

# ImageMagick writes neither a PNG-compressed entry — how every icon above 48
# pixels has been stored since Vista — nor a directory mixing depths, so those
# are packed here around files it did write. A `.png` source is embedded
# whole; a `.ico` source has its entries lifted across unchanged.
ico_pack() {  # dst src...
  python3 - "$@" <<'PACK'
import struct, sys

SIGNATURE = b"\x89PNG\r\n\x1a\n"

def entries(path):
    """Every image in a source file, as (directory bytes, payload)."""
    data = open(path, "rb").read()
    if data.startswith(SIGNATURE):
        width, height = struct.unpack_from(">II", data, 16)
        # A dimension of 256 is stored as 0: the field is one byte wide. The
        # stated depth is a convention only — a PNG entry states its own.
        return [(bytes([width % 256, height % 256, 0, 0])
                 + struct.pack("<HH", 1, 32), data)]

    reserved, kind, count = struct.unpack_from("<HHH", data, 0)
    assert reserved == 0 and kind == 1, f"{path} is not an icon"
    lifted = []
    for index in range(count):
        head = data[6 + 16 * index : 22 + 16 * index]
        size, offset = struct.unpack_from("<II", head, 8)
        lifted.append((head[:8], data[offset : offset + size]))
    return lifted

dst, sources = sys.argv[1], sys.argv[2:]
packed = [entry for source in sources for entry in entries(source)]

directory = b"\0\0\1\0" + struct.pack("<H", len(packed))
offset, body = 6 + 16 * len(packed), b""
for head, payload in packed:
    directory += head + struct.pack("<II", len(payload), offset)
    offset += len(payload)
    body += payload
open(dst, "wb").write(directory + body)
PACK
}

# The PNG entries. `png-gray8.png` is one `image`'s own ICO decoder refuses
# outright, on the strength of a note saying embedded PNGs must be 32-bit;
# `png-icc-p3.png` carries a profile that only survives if the entry goes
# through the PNG path rather than being flattened to icon pixels.
ico_pack ico-png-rgba8.ico  png-rgba8.png
ico_pack ico-png-gray8.ico  png-gray8.png
ico_pack ico-png-icc-p3.ico png-icc-p3.png

# The selection fixture: the picture is the largest entry and also the
# shallowest, so a decoder scoring depth before size shows the thumbnail.
magick "$work/color-alpha.png" -resize 16x12\! -type TrueColorAlpha "$work/thumbnail.ico"
ico_pack ico-multi.ico ico-bmp-palette.ico "$work/thumbnail.ico"

# ----------------------------------------------------------------- BMP
# The DIB the ICO fixtures carry, in a file of its own. `BMP3:` pins the
# ordinary `BITMAPINFOHEADER`; the alpha fixture is left to choose for itself,
# because carrying alpha is what forces the `BITMAPV5HEADER` and the bitfield
# masks that come with it. `-type Palette` on a four-color pattern lands on
# 4-bit, and adding RLE to it lands on 8-bit, so the two palette widths and
# both of the format's run-length codings are covered between them.
magick "$work/color.png"       -type TrueColor      BMP3:bmp-rgb8.bmp
magick "$work/color-alpha.png" -type TrueColorAlpha bmp-rgba8.bmp
magick "$work/color.png"       -type Palette        BMP3:bmp-palette.bmp
magick "$work/color.png" -depth 8 -type Palette -compress RLE BMP3:bmp-rle8.bmp

# A BMP stores its rows bottom-up unless its height is negative, and screen
# capture is where the other kind comes from. ImageMagick writes only the
# usual way round, so the rows are reversed here and the height negated to
# say so: it decodes to the ordinary pattern only if the sign is honored.
bmp_topdown() {  # src dst
  python3 - "$@" <<'FLIP'
import struct, sys

src, dst = sys.argv[1:3]
data = bytearray(open(src, "rb").read())
(start,) = struct.unpack_from("<I", data, 10)
width, height = struct.unpack_from("<ii", data, 18)
(bpp,) = struct.unpack_from("<H", data, 28)
assert height > 0, f"{src} is top-down already"

# Rows are padded out to a multiple of four bytes.
stride = ((width * bpp + 31) // 32) * 4
rows = [bytes(data[start + y * stride : start + (y + 1) * stride]) for y in range(height)]
data[start : start + stride * height] = b"".join(reversed(rows))
struct.pack_into("<i", data, 22, -height)
open(dst, "wb").write(bytes(data))
FLIP
}
bmp_topdown bmp-rgb8.bmp bmp-topdown.bmp

# -------------------------------------------------------------- netpbm
# `-depth` is not optional here. Left to itself ImageMagick picks the smallest
# MAXVAL that represents the colors present, so the four-color pattern comes
# out as `MAXVAL 3` — which is a fine netpbm file and a good demonstration of
# why the decoder reads MAXVAL, but not the fixture wanted at full range.
magick "$work/color.png" -depth 8  ppm:pnm-rgb8.ppm
magick "$work/gray.png"  -depth 8  -colorspace gray pgm:pnm-gray8.pgm
# 16-bit samples, which netpbm stores big-endian whatever the machine is.
magick "$work/color.png" -depth 16 ppm:pnm-rgb16.ppm
# The ASCII member of the family, under the extension that names no member in
# particular. `-compress None` is what selects `P3` over `P6`.
magick "$work/color.png" -depth 8 -compress None ppm:pnm-ascii.pnm
# The reason netpbm has a decoder of its own rather than joining the plain
# path: MAXVAL 1023 in a 16-bit word, which has to be lifted to full scale or
# the picture shows at a sixteenth of its brightness.
magick "$work/gray.png" -depth 10 -colorspace gray pgm:pnm-maxval1023.pgm
# MAXVAL 1, at the other end: one bit per pixel, and `+dither` so the two
# middle steps of the gray pattern round to the ends rather than stippling.
magick "$work/gray.png" -colorspace gray -threshold 50% +dither pbm:pnm-bilevel.pbm
# PAM, which generalizes the three above and is the only one of them that can
# carry alpha.
magick "$work/color-alpha.png" -depth 8 pam:pnm-rgba8.pam

# -------------------------------------------- TIFF as measurement rasters
# What elevation models and scientific output actually look like, and what
# `image` cannot read at all: single-band floats, BigTIFF, the floating-point
# predictor, signed integers, and a no-data sentinel. Needs GDAL; everything
# above needs only ImageMagick.
gdal_translate -q -b 1 -of GTiff -co COMPRESS=NONE tiff-float32.tif "$work/band1.tif"
gdal_translate -q -of GTiff -co BIGTIFF=YES -co COMPRESS=NONE \
  "$work/band1.tif" tiff-bigtiff.tif
gdal_translate -q -of GTiff -co COMPRESS=DEFLATE -co PREDICTOR=3 -co TILED=YES \
  -co BLOCKXSIZE=16 -co BLOCKYSIZE=16 "$work/band1.tif" tiff-float-predictor.tif
gdal_translate -q -ot Int16 -scale 0 3.984375 -1000 3000 \
  "$work/band1.tif" tiff-int16.tif
# A scanned map or an aerial photograph as GDAL writes one: JPEG-compressed,
# which stores the pixels as YCbCr with the chroma subsampled 2x2 and leaves
# the conversion back to the reader. Tiled, so that the bottom row of tiles
# is clipped.
gdal_translate -q -of GTiff -co COMPRESS=JPEG -co JPEG_QUALITY=100 -co PHOTOMETRIC=YCBCR \
  -co TILED=YES -co BLOCKXSIZE=16 -co BLOCKYSIZE=16 tiff-rgb8.tif tiff-jpeg.tif
# A picture with an internal mask, as GDAL writes one from an alpha band: a
# one-bit transparency-mask directory after the picture, which is not a page.
# The upside-down page is appended after it, so that reaching the second page
# means stepping over the mask.
gdal_translate -q -b 1 -b 2 -b 3 -mask 4 --config GDAL_TIFF_INTERNAL_MASK YES \
  -co COMPRESS=NONE tiff-rgba8.tif "$work/masked.tif"
tiffcp "$work/masked.tif" tiff-pages.tif,1 tiff-mask.tif

# A no-data sentinel with pixels actually set to it, built from raw floats so
# the quadrant values are exact.
python3 - "$work/nodata.raw" <<'RAW'
import struct, sys
values = [-9999.0, 0.5, 1.0, 255 / 64]
rows = [values[(0 if y < 12 else 2) + (0 if x < 16 else 1)]
        for y in range(24) for x in range(32)]
open(sys.argv[1], "wb").write(struct.pack(f"<{len(rows)}f", *rows))
RAW
cat > "$work/nodata.vrt" <<VRT
<VRTDataset rasterXSize="32" rasterYSize="24">
  <VRTRasterBand dataType="Float32" band="1" subClass="VRTRawRasterBand">
    <SourceFilename relativeToVRT="1">nodata.raw</SourceFilename>
    <ImageOffset>0</ImageOffset><PixelOffset>4</PixelOffset><LineOffset>128</LineOffset>
    <ByteOrder>LSB</ByteOrder>
  </VRTRasterBand>
</VRTDataset>
VRT
gdal_translate -q -of GTiff -a_nodata -9999 -co COMPRESS=NONE \
  "$work/nodata.vrt" tiff-nodata.tif

# ---------------------------------------------------------- camera raw
# What a camera writes: not a picture but one count per photosite, under a
# color filter, with the matrix that says what the counts mean. DNG is the
# one raw format anything but a camera can write, so it stands for all of
# them; LibRaw develops it through the same pipeline a NEF goes through.
# The pattern is mosaiced RGGB — a red quadrant is red counts at the red
# sites and nothing at the others — and the matrix makes the camera's space
# Rec. 2020 exactly, so the developed quadrants come out as the pattern with
# nothing to balance or convert. Twelve-bit counts in sixteen-bit words, so
# that the white level has to be read rather than assumed.
python3 - dng-cfa.dng <<'DNG'
import struct, sys

WIDTH, HEIGHT, WHITE = 32, 24, 4095

# The four quadrants' colors, mosaiced RGGB: each photosite keeps the one
# channel its filter passes.
def quadrant(x, y):
    return [(1, 0, 0), (0, 1, 0), (0, 0, 1), (1, 1, 1)][(2 if y >= 12 else 0) + (1 if x >= 16 else 0)]
counts = []
for y in range(HEIGHT):
    for x in range(WIDTH):
        r, g, b = quadrant(x, y)
        counts.append([r, g, g, b][(y % 2) * 2 + (x % 2)] * WHITE)
pixels = struct.pack(f"<{len(counts)}H", *counts)

# XYZ (D65) to Rec. 2020: with this as the camera's matrix, the camera's
# space is Rec. 2020 and a developed pixel is its counts, balanced by nothing.
matrix = [1716651, -355671, -253366, -666684, 1616481, 15769, 17640, -42771, 942103]

SHORT, LONG, RATIONAL, SRATIONAL, ASCII, BYTE = 3, 4, 5, 10, 2, 1
entries = [
    (254, LONG, [0]),                       # NewSubfileType: the picture itself
    (256, LONG, [WIDTH]), (257, LONG, [HEIGHT]),
    (258, SHORT, [16]), (259, SHORT, [1]),  # BitsPerSample, uncompressed
    (262, SHORT, [32803]),                  # PhotometricInterpretation: CFA
    (271, ASCII, b"gamut\0"), (272, ASCII, b"fixture\0"),
    (273, LONG, [0]),                       # StripOffsets, patched below
    (274, SHORT, [1]), (277, SHORT, [1]), (278, LONG, [HEIGHT]),
    (279, LONG, [len(pixels)]), (284, SHORT, [1]),
    (33421, SHORT, [2, 2]), (33422, BYTE, bytes([0, 1, 1, 2])),   # RGGB
    (50706, BYTE, bytes([1, 4, 0, 0])), (50707, BYTE, bytes([1, 1, 0, 0])),
    (50708, ASCII, b"gamut fixture\0"),
    (50717, LONG, [WHITE]),                                       # WhiteLevel
    (50721, SRATIONAL, [(m, 1000000) for m in matrix]),           # ColorMatrix1
    (50728, RATIONAL, [(1, 1)] * 3),                              # AsShotNeutral
    (50778, SHORT, [21]),                                         # D65
]
entries.sort()

def pack(kind, values):
    if kind in (ASCII, BYTE):
        return bytes(values), len(values)
    if kind == SHORT:
        return struct.pack(f"<{len(values)}H", *values), len(values)
    if kind == LONG:
        return struct.pack(f"<{len(values)}I", *values), len(values)
    code = "<II" if kind == RATIONAL else "<ii"
    return b"".join(struct.pack(code, *v) for v in values), len(values)

directory_at = 8
overflow_at = directory_at + 2 + 12 * len(entries) + 4
directory, overflow = b"", b""
for tag, kind, values in entries:
    raw, count = pack(kind, values)
    if tag == 273:
        directory += struct.pack("<HHI", tag, kind, count) + b"STRP"
    elif len(raw) <= 4:
        directory += struct.pack("<HHI", tag, kind, count) + raw.ljust(4, b"\0")
    else:
        directory += struct.pack("<HHI", tag, kind, count) + struct.pack("<I", overflow_at + len(overflow))
        overflow += raw + (b"\0" if len(raw) % 2 else b"")
strip_at = overflow_at + len(overflow)
directory = directory.replace(b"STRP", struct.pack("<I", strip_at))
out = b"II\x2a\x00" + struct.pack("<I", directory_at) + struct.pack("<H", len(entries)) + directory + struct.pack("<I", 0) + overflow + pixels
open(sys.argv[1], "wb").write(out)
DNG

# ------------------------------------------------------- negative fixtures
# A DNG cut off inside its directory: claimed by the raw decoder, since the
# entries that survive say what it is, then refused by LibRaw.
head -c 100 dng-cfa.dng > bad-truncated.dng
# A PNG header followed by rubbish: the decoder is chosen, then fails.
{ printf '\211PNG\r\n\032\n'; head -c 64 /dev/zero | tr '\0' 'X'; } > bad-truncated.png
# A real image in a format this build does not include. Targa has no decoder
# here, and its header is a length and two type codes with no signature in it
# for any sniff to claim, so it is turned away by the registry rather than by
# a backend.
magick "$work/color.png" unsupported.tga
# A PNG called a TIFF, to exercise the content-sniffing fallback.
cp png-rgb8.png mislabeled.tif

echo "generated $(ls -1 *.png *.jpg *.jpeg *.tif *.tiff *.hdr *.exr *.gif \
                    *.heic *.heif *.avif *.webp *.jxl *.ico *.bmp *.tga \
                    *.pnm *.pbm *.pgm *.ppm *.pam *.dng | wc -l) fixtures"
