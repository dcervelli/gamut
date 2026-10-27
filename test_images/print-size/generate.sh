#!/bin/bash
# Regenerates ruler-150dpi.png, the picture the zoom menu's print size is
# checked against by eye. Run from this directory; needs ImageMagick.
#
# The file says 150 pixels to the inch in its pHYs chunk, and everything in it
# is drawn to that: an inch ruler to six inches in sixteenths, a millimeter
# ruler to the same six inches, and the outline of a credit card — ISO/IEC
# 7810 ID-1, 85.60 by 53.98 mm with corners of 3.18 mm. Shown at its print
# size, the rulers measure true against a real one, and a card laid on the
# glass covers the outline.
#
# pHYs counts pixels to the meter in whole numbers, and ImageMagick cuts
# 150 dpi down to 5905 of them: 149.99 dpi, which the rulers come out longer
# by a hundredth of a millimeter over their six inches.
set -euo pipefail

# The face the numbers are set in: whichever of these the system has, or
# FONT where it says.
for candidate in "${FONT:-}" \
  /usr/share/fonts/TTF/DejaVuSans.ttf \
  /usr/share/fonts/truetype/dejavu/DejaVuSans.ttf \
  /System/Library/Fonts/Supplemental/Arial.ttf; do
  if [ -n "$candidate" ] && [ -f "$candidate" ]; then
    font=$candidate
    break
  fi
done
: "${font:?no font found; set FONT to a TrueType file}"

# Every length below is in inches or millimeters, turned into pixels at the
# file's own density by awk, so the picture is its own statement of scale.
awk -v dpi=150 '
function inch(v) { return v * dpi }
function mm(v) { return v * dpi / 25.4 }
BEGIN {
  left = inch(0.5)
  printf "fill black stroke none font-size 22 text-anchor start\n"
  printf "text %.2f,%.2f \"150 dpi. At print size, these measure true.\"\n", left, inch(0.35)

  # The inch ruler, in sixteenths: ticks hang from its edge, longer as the
  # fraction is coarser, with each whole inch numbered under its tick.
  top = inch(0.6)
  printf "stroke black stroke-width 1.5 fill none\n"
  printf "line %.2f,%.2f %.2f,%.2f\n", left, top, left + inch(6), top
  for (i = 0; i <= 96; i++) {
    x = left + inch(i / 16)
    if (i % 16 == 0) len = 48
    else if (i % 8 == 0) len = 36
    else if (i % 4 == 0) len = 26
    else if (i % 2 == 0) len = 18
    else len = 11
    printf "line %.2f,%.2f %.2f,%.2f\n", x, top, x, top + len
  }
  printf "fill black stroke none font-size 20 text-anchor middle\n"
  for (i = 0; i <= 6; i++)
    printf "text %.2f,%.2f \"%d\"\n", left + inch(i), top + 72, i
  printf "text-anchor start\n"
  printf "text %.2f,%.2f \"in\"\n", left + inch(6) + 14, top + 22

  # The millimeter ruler, to the same six inches: a tick every millimeter,
  # longer at five and ten, numbered every ten.
  top = inch(1.45)
  printf "stroke black stroke-width 1 fill none\n"
  printf "line %.2f,%.2f %.2f,%.2f\n", left, top, left + mm(152.4), top
  for (i = 0; i <= 152; i++) {
    x = left + mm(i)
    len = i % 10 == 0 ? 30 : (i % 5 == 0 ? 20 : 12)
    printf "line %.2f,%.2f %.2f,%.2f\n", x, top, x, top + len
  }
  printf "fill black stroke none font-size 16 text-anchor middle\n"
  for (i = 0; i <= 150; i += 10)
    printf "text %.2f,%.2f \"%d\"\n", left + mm(i), top + 50, i
  printf "text-anchor start\n"
  printf "text %.2f,%.2f \"mm\"\n", left + mm(152.4) + 14, top + 16

  # The card, filled, with its size written inside it.
  top = inch(2.2)
  printf "fill \"#c8c8c8\" stroke none\n"
  printf "roundrectangle %.2f,%.2f %.2f,%.2f %.2f,%.2f\n", \
    left, top, left + mm(85.60), top + mm(53.98), mm(3.18), mm(3.18)
  printf "fill \"#505050\" font-size 18 text-anchor middle\n"
  printf "text %.2f,%.2f \"Credit card\"\n", left + mm(85.60) / 2, top + mm(53.98) / 2 - 6
  printf "text %.2f,%.2f \"85.60 x 53.98 mm\"\n", left + mm(85.60) / 2, top + mm(53.98) / 2 + 20
}' > ruler.mvg
trap 'rm -f ruler.mvg' EXIT

magick -size 1050x720 xc:white -font "$font" -draw @ruler.mvg \
  -colorspace Gray -depth 8 -define png:color-type=0 \
  -units PixelsPerInch -density 150 \
  -define png:exclude-chunks=date,time +set date:create +set date:modify \
  ruler-150dpi.png
