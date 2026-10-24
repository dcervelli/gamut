#!/bin/sh
# Redact in place: the region marked in gamut blacked out of the picture on
# screen itself, rather than of a copy. The original goes to the Trash, and
# gamut's undo puts it back. macOS.
#
# In ~/.config/gamut/config.toml, the key above the first [command] table
# and the table below the rest:
#
#   keys.command.redact-in-place = "cmd+alt+r"
#
#   [command.redact-in-place]
#   label = "Redact in place"
#   run = "/path/to/redact-in-place-macos.sh"
#   needs = "region"
#   output = "replace"
#
# Needs ImageMagick: `brew install imagemagick`, or MacPorts' imagemagick.
# The region arrives in the file's own pixels, so no -auto-orient is wanted,
# and the file keeps the photo's orientation and the rest of its metadata,
# GPS included. A JPEG is compressed again, at the quality ImageMagick reads
# off the original.
#
# The script never writes over the file: it writes to $GAMUT_OUTPUT, and
# gamut puts that in the file's place once it has exited cleanly.

set -eu

: "${GAMUT_FILE:?run this from gamut}"
: "${GAMUT_RESULT:?run this from gamut}"
: "${GAMUT_OUTPUT:?give the command output = \"replace\"}"
: "${GAMUT_REGION_X:?mark a region first}"

say_error() {
    echo "error = $1" >> "$GAMUT_RESULT"
    echo "redact-in-place: $1" >&2
    exit 1
}

# Started from the Dock, the shell has none of your startup files; gamut adds
# Homebrew's and MacPorts' directories to the path, and they are looked in
# by name here as well, should it be run some other way.
im=""
for candidate in magick /opt/homebrew/bin/magick /usr/local/bin/magick /opt/local/bin/magick; do
    if command -v "$candidate" > /dev/null 2>&1; then
        im=$candidate
        break
    fi
done
[ -n "$im" ] || say_error "ImageMagick is not installed: brew install imagemagick"

# Only a format ImageMagick writes back as itself can be redacted in place:
# a raw or a HEIC from the iPhone would come back as something else under
# its old name.
ext=$(printf '%s' "${GAMUT_EXT:-}" | tr '[:upper:]' '[:lower:]')
case "$ext" in
    jpg | jpeg | png | tif | tiff | webp | bmp | gif) ;;
    *) say_error "Cannot redact a .${GAMUT_EXT:-} file in place; redact a copy instead" ;;
esac

# ImageMagick's rectangle names its last pixel, not the edge past it.
right=$((GAMUT_REGION_X + GAMUT_REGION_WIDTH - 1))
bottom=$((GAMUT_REGION_Y + GAMUT_REGION_HEIGHT - 1))

# $GAMUT_OUTPUT has the file's own name, so ImageMagick writes the format
# the file is already in.
"$im" "$GAMUT_FILE" -fill black -stroke none \
    -draw "rectangle $GAMUT_REGION_X,$GAMUT_REGION_Y $right,$bottom" "$GAMUT_OUTPUT" ||
    say_error "ImageMagick could not redact $GAMUT_NAME"
