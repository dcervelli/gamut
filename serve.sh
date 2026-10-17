#!/usr/bin/env bash
# Local preview: Jekyll on :4000, with media served from a gamut-scripting
# checkout on :4001 instead of dcervelli.github.io/gamut-scripting.
#   MEDIA_DIR=/path/to/gamut-scripting ./serve.sh
set -euo pipefail
cd "$(dirname "$0")"

MEDIA_DIR="${MEDIA_DIR:-../gamut-scripting}"
if [[ ! -d "$MEDIA_DIR" ]]; then
  echo "media checkout not found at $MEDIA_DIR (set MEDIA_DIR)" >&2
  exit 1
fi

python3 -m http.server 4001 --bind 127.0.0.1 --directory "$MEDIA_DIR" >/dev/null 2>&1 &
media_pid=$!
trap 'kill $media_pid' EXIT

export PATH="$(ruby -e 'print Gem.user_dir')/bin:$PATH"
bundle exec jekyll serve --livereload --config _config.yml,_config.local.yml "$@"
