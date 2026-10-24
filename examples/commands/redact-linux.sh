#!/bin/sh
# Redact: a copy of the picture on screen with the region marked in gamut
# blacked out, written beside it and then shown. Linux.
#
# In ~/.config/gamut/config.toml, the key above the first [command] table
# and the table below the rest:
#
#   keys.command.redact = "ctrl+shift+r"
#
#   [command.redact]
#   label = "Redact region"
#   run = "/path/to/redact-linux.sh"
#   needs = "region"
#
# Needs ImageMagick: `pacman -S imagemagick`, `apt install imagemagick` or
# `dnf install ImageMagick`. The region arrives in the file's own pixels,
# so no -auto-orient is wanted, and the copy keeps the photo's orientation
# and the rest of its metadata, GPS included.

set -eu

: "${GAMUT_FILE:?run this from gamut}"
: "${GAMUT_RESULT:?run this from gamut}"
: "${GAMUT_REGION_X:?mark a region first}"

say_error() {
    echo "error = $1" >> "$GAMUT_RESULT"
    echo "redact: $1" >&2
    exit 1
}

# ImageMagick 7 is `magick`; a distribution still on 6 has only `convert`.
if command -v magick > /dev/null 2>&1; then
    im=magick
elif command -v convert > /dev/null 2>&1; then
    im=convert
else
    say_error "ImageMagick is not installed"
fi

# The copy is written in the file's own format where ImageMagick writes it,
# and as a PNG where it does not: a raw, a HEIC.
ext=$(printf '%s' "${GAMUT_EXT:-}" | tr '[:upper:]' '[:lower:]')
case "$ext" in
    jpg | jpeg | png | tif | tiff | webp | bmp | gif) ;;
    *) ext=png ;;
esac

# Never over a file already there: redacted, then redacted-2, and so on.
out="$GAMUT_DIR/$GAMUT_STEM-redacted.$ext"
n=2
while [ -e "$out" ]; do
    out="$GAMUT_DIR/$GAMUT_STEM-redacted-$n.$ext"
    n=$((n + 1))
done

# ImageMagick's rectangle names its last pixel, not the edge past it.
right=$((GAMUT_REGION_X + GAMUT_REGION_WIDTH - 1))
bottom=$((GAMUT_REGION_Y + GAMUT_REGION_HEIGHT - 1))

"$im" "$GAMUT_FILE" -fill black -stroke none \
    -draw "rectangle $GAMUT_REGION_X,$GAMUT_REGION_Y $right,$bottom" "$out" ||
    say_error "ImageMagick could not write $(basename "$out")"

echo "open = $out" >> "$GAMUT_RESULT"
echo "message = Redacted ${GAMUT_REGION_WIDTH}×${GAMUT_REGION_HEIGHT} into $(basename "$out")" >> "$GAMUT_RESULT"
