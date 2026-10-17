#!/usr/bin/env bash
# Regenerate the sidebar thumbnails in assets/thumbs/ for every item in
# _data/gallery.yml that has a src, from a gamut-scripting checkout: the first
# frame of a GIF/MP4, or the still itself, 320px wide. The GIFs themselves are
# megabytes each, too heavy to load twenty at once.
#   MEDIA_DIR=/path/to/gamut-scripting ./make-thumbs.sh
set -euo pipefail
cd "$(dirname "$0")"

MEDIA_DIR="${MEDIA_DIR:-../gamut-scripting}"
out=assets/thumbs
mkdir -p "$out"

sed -n 's/^ *src: *//p' _data/gallery.yml | while read -r src; do
  f="$MEDIA_DIR/$src"
  name=$(basename "$f")  # keep the extension: info.gif and info.jpg both exist
  case "$f" in
    *.mp4) ffmpeg -loglevel error -y -i "$f" -frames:v 1 -vf scale=320:-2 -q:v 4 "$out/$name.jpg" </dev/null ;;
    *)     magick "$f[0]" -resize 320x -strip -quality 80 "$out/$name.jpg" ;;
  esac
done
