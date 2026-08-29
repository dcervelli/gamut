#!/bin/bash
# Regenerates the decoder fixtures. Run from this directory.
#
# Every image is the same 32x24 pattern of four 16x12 quadrants, so a test can
# probe the centre of each quadrant and know what it should find:
#
#   colour:  top-left red     top-right green    bottom-left blue   bottom-right white
#   grey:    top-left 0       top-right 85       bottom-left 170    bottom-right 255
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
# TIFF compressions and byte orders, HEIF colour tags and transformations).
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
      ./*.heic ./*.heif ./*.avif

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

# Neither `cICP` nor `iCCP` is a chunk ImageMagick will write, and between
# them they are the whole of how a PNG says what its numbers mean, so both are
# spliced in after `IHDR` with their CRCs computed.
png_tag() {  # cicp src dst p t m r  |  iccp src dst profile
  python3 - "$@" <<'TAG'
import struct, sys, zlib

mode, src, dst = sys.argv[1:4]
if mode == "cicp":
    # Colour primaries, transfer function, matrix coefficients, full range.
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

# ---------------------------------------------------------------- Radiance
magick "$work/float.png" -set colorspace RGB -evaluate multiply 3.984375 hdr-rgbe.hdr

# ---------------------------------------------------------------- OpenEXR
magick "$work/float.png" -set colorspace RGB -evaluate multiply 3.984375 exr-rgb.exr
# Composite while both are still sRGB-tagged so the coverage values pass
# through untouched, then reinterpret as linear and scale only the colour.
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
# convention, so the colour-space fixtures below are testing a translation
# rather than a guess.
magick "$work/color.png"       -depth 16 -define png:bit-depth=16 "$work/color16.png"

heif-enc -L --hevc -o heic-rgb8.heic        "$work/color.png"       > /dev/null
heif-enc -L --hevc -o heic-rgba8.heic       "$work/color-alpha.png" > /dev/null
# A greyscale input encodes as a monochrome image, and must stay one channel.
heif-enc -L --hevc -o heic-gray8.heic       "$work/gray.png"        > /dev/null
heif-enc -L --hevc -o heic-gray-alpha8.heic "$work/gray-alpha.png"  > /dev/null
# 10-bit, tagged BT.2100 PQ on BT.2020 primaries: the HDR path, and the one
# that has to be lifted from 0..1023 to the full 16-bit range on the way in.
heif-enc -L --hevc -b 10 --colour_primaries 9 --transfer_characteristic 16 \
  -o heic-pq10.heic "$work/color16.png" > /dev/null
# Display P3 primaries (EG 432-1) with the sRGB curve: what a phone writes.
heif-enc -L --hevc --colour_primaries 12 --transfer_characteristic 13 \
  -o heif-p3.heif "$work/color.png" > /dev/null
# An upside-down image that says it is upside down. `--rotate-cw` writes an
# `irot` property rather than turning the pixels, so this decodes back to the
# ordinary pattern only if the transformation is applied on the way out.
magick "$work/color.png" -rotate 180 "$work/color-upside-down.png"
heif-enc -L --hevc --rotate-cw 180 -o heic-rotated.heic \
  "$work/color-upside-down.png" > /dev/null
# The same container with AV1 inside instead of HEVC.
heif-enc -L -A -o avif-rgb8.avif "$work/color.png" > /dev/null
# A HEIF that states its colour space with an ICC profile and no `nclx` box,
# which is what some cameras write. `heif-enc` has no way to embed a profile,
# but ImageMagick carries the source PNG's through untouched — the pixels are
# not converted, because there is no target profile to convert to. HEVC at
# quality 100 is near-lossless rather than lossless, hence the tolerance on
# this one fixture.
magick png-icc-p3.png -quality 100 heic-icc-p3.heic

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

# ------------------------------------------------------- negative fixtures
# A PNG header followed by rubbish: the decoder is chosen, then fails.
{ printf '\211PNG\r\n\032\n'; head -c 64 /dev/zero | tr '\0' 'X'; } > bad-truncated.png
# A real image in a format this build does not include.
magick "$work/color.png" unsupported.gif
# A PNG called a TIFF, to exercise the content-sniffing fallback.
cp png-rgb8.png mislabelled.tif

echo "generated $(ls -1 *.png *.jpg *.jpeg *.tif *.tiff *.hdr *.exr *.gif \
                    *.heic *.heif *.avif | wc -l) fixtures"
