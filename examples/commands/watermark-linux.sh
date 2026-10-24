#!/bin/sh
# Watermark: a copy of the picture on screen with a line of text laid over
# one corner or the middle, mixed into the pixels under it rather than
# painted over them, written beside it and then shown. Linux.
#
# In ~/.config/gamut/config.toml, the key above the first [command] table
# and the table below the rest:
#
#   keys.command.watermark = "ctrl+shift+w"
#
#   [command.watermark]
#   label = "Watermark"
#   run = "/path/to/watermark-linux.sh"
#   input = [
#     { name = "text", label = "Text", kind = "string", default = "© 2026" },
#     { name = "size", label = "Size, % of height", kind = "slider", min = 1, max = 25, default = 6 },
#     { name = "position", label = "Position", kind = "radio", choices = ["UL", "UR", "C", "LL", "LR"], default = "LR" },
#   ]
#
# Needs ImageMagick: `pacman -S imagemagick`, `apt install imagemagick` or
# `dnf install ImageMagick`. The corner is the one you see: the copy is
# stood upright by its EXIF orientation, which is then written as 1, and
# turned as gamut shows it while the text goes on, then turned back. The
# rest of the photo's metadata is kept.
#
# The text is mixed by ImageMagick's Overlay: light letters in a dark
# outline, each at part strength, lighten and darken what is under them,
# so the picture's own texture shows through.

set -eu

: "${GAMUT_FILE:?run this from gamut}"
: "${GAMUT_RESULT:?run this from gamut}"
: "${GAMUT_INPUT_TEXT:?give the command a text input}"

say_error() {
    echo "error = $1" >> "$GAMUT_RESULT"
    echo "watermark: $1" >&2
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

[ -n "$GAMUT_INPUT_TEXT" ] || say_error "No text to write"

case "${GAMUT_INPUT_POSITION:-LR}" in
    UL) gravity=NorthWest ;;
    UR) gravity=NorthEast ;;
    C) gravity=Center ;;
    LL) gravity=SouthWest ;;
    LR) gravity=SouthEast ;;
    *) say_error "Unknown position ${GAMUT_INPUT_POSITION}" ;;
esac

# The height the text is a share of is the picture's as seen: across a
# quarter turn of the file's orientation or of gamut's, width and height
# trade places.
width=${GAMUT_WIDTH:?}
height=${GAMUT_HEIGHT:?}
case "${GAMUT_ORIENTATION:-1}" in
    5 | 6 | 7 | 8) t=$width; width=$height; height=$t ;;
esac
turn=${GAMUT_TURN:-0}
case "$turn" in
    90 | 270) t=$width; width=$height; height=$t ;;
esac

# The slider's number may arrive as 6 or 6.5; the point size is whole.
points=$(awk -v h="$height" -v s="${GAMUT_INPUT_SIZE:-6}" 'BEGIN { p = int(h * s / 100 + 0.5); print (p < 4 ? 4 : p) }')
margin=$((points / 2))
outline=$((points / 16 + 1))
[ "$gravity" = Center ] && margin=0

# The copy is written in the file's own format where ImageMagick writes it,
# and as a PNG where it does not: a raw, a HEIC.
ext=$(printf '%s' "${GAMUT_EXT:-}" | tr '[:upper:]' '[:lower:]')
case "$ext" in
    jpg | jpeg | png | tif | tiff | webp | bmp) ;;
    *) ext=png ;;
esac

# Never over a file already there: watermarked, then watermarked-2, and so on.
out="$GAMUT_DIR/$GAMUT_STEM-watermarked.$ext"
n=2
while [ -e "$out" ]; do
    out="$GAMUT_DIR/$GAMUT_STEM-watermarked-$n.$ext"
    n=$((n + 1))
done

# Fonts are named differently on every system: fontconfig is asked for the
# desktop's bold sans by its file, and ImageMagick's default kept without one.
font=$(fc-match -f '%{file}' 'sans:bold' 2> /dev/null || true)
set --
[ -n "$font" ] && [ -f "$font" ] && set -- -font "$font"

# The text goes through a file rather than the command line, so that a %
# or a backslash in it is written as typed rather than read as an escape.
text_file=$(mktemp)
trap 'rm -f "$text_file"' EXIT
printf '%s' "$GAMUT_INPUT_TEXT" > "$text_file"

# The layer is the picture's size, clear but for the text: a dark outline,
# the light letters over it. Overlay then lightens the picture under the
# letters and darkens it around them.
"$im" "$GAMUT_FILE[0]" -auto-orient -rotate "$turn" \
    \( +clone -alpha transparent -background none \
       -gravity "$gravity" -pointsize "$points" "$@" \
       -fill none -stroke "rgba(0,0,0,0.6)" -strokewidth "$outline" \
       -annotate "+$margin+$margin" "@$text_file" \
       -fill "rgba(255,255,255,0.8)" -stroke none \
       -annotate "+$margin+$margin" "@$text_file" \) \
    -compose Overlay -composite \
    -rotate "-$turn" "$out" ||
    say_error "ImageMagick could not write $(basename "$out")"

echo "open = $out" >> "$GAMUT_RESULT"
echo "message = Watermarked into $(basename "$out")" >> "$GAMUT_RESULT"
