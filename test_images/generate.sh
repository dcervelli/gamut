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
# Two files are deliberately spelled `.jpeg` and `.tiff` rather than `.jpg`
# and `.tif`, so that both extension spellings are exercised by the router.
#
# The point is coverage of decode paths, not pretty pictures: every pixel
# layout the `image` crate can hand back, plus the per-format encodings that
# have their own code path (bit depths, palettes, interlacing, progressive
# JPEG, TIFF compressions and byte orders).
set -euo pipefail

quad() { magick \( -size 16x12 "xc:$1" -size 16x12 "xc:$2" +append \) \
                \( -size 16x12 "xc:$3" -size 16x12 "xc:$4" +append \) -append "$5"; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Start clean, so a renamed fixture does not leave its predecessor behind.
rm -f ./*.png ./*.jpg ./*.jpeg ./*.tif ./*.tiff ./*.hdr ./*.exr ./*.gif

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

echo "generated $(ls -1 *.png *.jpg *.tif *.hdr *.exr *.gif | wc -l) fixtures"
