#!/bin/sh
# What every screenshot script in this directory does the same way: open a
# gamut window of a known size where the compositor will leave it alone, drive
# it with keys, capture exactly its rectangle, and close it again.
#
# Only Hyprland is spoken here. Its Lua API places and focuses the window,
# `wtype` types into it, and `grim` captures the region it reports. The capture
# is in device pixels, so a monitor at scale 1.6 gives a 1200x800 window as a
# 1920x1280 image, which is what the README's pictures are.
#
# A script sources this file, then:
#
#   open 1200 800 ~/git/mora     the window, focused, with its first file shown
#   keys plus plus plus Up       the keys to press, one argument each
#   settle                       wait for a pan or zoom to land
#   shoot user-docs/screenshots/x.jpg
#   close
#
# or, for a recording, `record target/screenshots/x.mp4` and `cut` around the
# keys, and `gif x.mp4 user-docs/screenshots/x.gif` afterwards. The
# recording is gpu-screen-recorder's, and the GIF is ffmpeg's.
#
# The window opens on whatever workspace is active, so the script is for a
# desk someone is sitting at, not for CI. Keep your hands off the keyboard
# while it runs: the keys go to whichever window is focused.

set -eu

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
CLASS=com.dcervelli.gamut
GAMUT="$ROOT/target/release/gamut"

for tool in hyprctl jq wtype grim cargo gpu-screen-recorder ffmpeg python3; do
    command -v "$tool" >/dev/null || { echo "$tool is not installed" >&2; exit 1; }
done

# Whatever a script that stopped early left behind: a window it was
# driving, a recorder it had started.
cleanup() {
    [ -z "${RECORDER:-}" ] || kill -INT "$RECORDER" 2>/dev/null || true
    [ -z "${WINDOW:-}" ] || kill "$(window pid)" 2>/dev/null || true
}
trap cleanup EXIT

# Every gamut window the compositor knows, one address per line.
gamut_windows() {
    hyprctl clients -j | jq -r --arg class "$CLASS" '.[] | select(.class == $class) | .address'
}

# One field of the window this script opened: "title", "pid", or the
# geometry as "x y w h".
window() {
    hyprctl clients -j | jq -r --arg address "$WINDOW" "
        .[] | select(.address == \$address) | $(case $1 in
            geometry) echo '"\(.at[0]) \(.at[1]) \(.size[0]) \(.size[1])"' ;;
            *) echo ".$1" ;;
        esac)"
}

# Open gamut at W by H logical pixels on the rest of the arguments, floating
# and centered so that the layout neither tiles nor resizes it, at full
# opacity so that whatever is behind it stays out of the picture. Returns
# once the first file is on screen and the window has the keyboard.
open() {
    width=$1
    height=$2
    shift 2

    [ -x "$GAMUT" ] || cargo build --release --manifest-path "$ROOT/Cargo.toml" -q

    before=" $(gamut_windows | tr '\n' ' ') "
    command="$GAMUT --size $width $height"
    for path; do
        command="$command $(shell_quote "$path")"
    done
    hyprctl eval "hl.exec_cmd($(lua_quote "$command"), {
        float = true, size = \"$width $height\", center = true, opacity = \"1 1\",
    })" >/dev/null

    # The new window is the one that was not there before.
    WINDOW=
    for _ in $(seq 50); do
        WINDOW=$(gamut_windows | while read -r candidate; do
            case "$before" in *" $candidate "*) ;; *) echo "$candidate" ;; esac
        done | head -1)
        [ -n "$WINDOW" ] && break
        sleep 0.1
    done
    [ -n "$WINDOW" ] || { echo "gamut's window never appeared" >&2; exit 1; }

    loaded

    # Focus follows the pointer, so the pointer goes in first; the keys
    # would otherwise land in whatever it was over.
    set -- $(window geometry)
    cursor $(($1 + $3 / 2)) $(($2 + $4 / 2))
    hyprctl eval "hl.dispatch(hl.dsp.focus({ window = hl.get_window(\"address:$WINDOW\") }))" >/dev/null
    for _ in $(seq 20); do
        [ "$(hyprctl activewindow -j | jq -r .address)" = "$WINDOW" ] && break
        sleep 0.1
    done
    sleep 0.5
}

# Wait for the file on its way to be on screen: the title says "loading"
# until it is. For after a step to another file, as well as for `open`.
loaded() {
    for _ in $(seq 100); do
        case "$(window title)" in loading*) sleep 0.1 ;; *) break ;; esac
    done
}

# Warp the pointer to a point in the compositor's layout, in logical pixels.
cursor() {
    hyprctl eval "hl.dispatch(hl.dsp.cursor.move({ x = $1, y = $2 }))" >/dev/null
}

# Where in the layout image pixel X, Y of a W by H image is while the image
# is fitted to the window: the picture is centered in the area the four
# bars leave, at whichever of the two scales fits all of it in.
#
#   cursor $(fitted 6016 3384 960 2150)
fitted() {
    set -- "$1" "$2" "$3" "$4" $(window geometry) $(scale)
    awk -v W="$1" -v H="$2" -v X="$3" -v Y="$4" \
        -v wx="$5" -v wy="$6" -v ww="$7" -v wh="$8" -v scale="$9" -v bar=30 'BEGIN {
        vw = (ww - 2 * bar) * scale; vh = (wh - 2 * bar) * scale
        zoom = vw / W; if (vh / H < zoom) zoom = vh / H
        x = (vw - W * zoom) / 2 + X * zoom; y = (vh - H * zoom) / 2 + Y * zoom
        printf "%d %d\n", wx + bar + x / scale, wy + bar + y / scale
    }'
}

# Device pixels to the logical one on the monitor the window is on.
scale() {
    monitor=$(window monitor)
    hyprctl monitors -j | jq -r --argjson id "$monitor" '.[] | select(.id == $id) | .scale'
}

# Turn the wheel N notches under the pointer, positive away from the hand,
# which zooms in about it; and drag the left button DX, DY logical pixels
# from where the pointer is. Both are a device of our own: see device.py.
wheel() {
    python3 "$ROOT/bin/screenshots/device.py" wheel "$1"
}

drag() {
    python3 "$ROOT/bin/screenshots/device.py" drag "$1" "$2"
}

# Press a key by its position rather than by what it says — `shift+2` for
# 50%, `ctrl+shift+c` — through the same device, for the bindings that are
# matched on the key's position and that wtype's own keymap cannot reach.
press() {
    for chord; do
        python3 "$ROOT/bin/screenshots/device.py" key "$chord"
    done
}

# Press keys, one argument each, named as xkb names them: `plus`, `Up`,
# `space`, `bracketright`, `c`, and `C` for the capital. Modifiers go in
# front, joined with dashes: `ctrl-c`, `ctrl-shift-period`.
#
# One wtype call per key, because wtype grows its keymap as it meets new
# keysyms and sends the compositor each revision, and a key pressed under a
# keymap the window has not read yet lands as whatever that keycode was
# under the last one.
keys() {
    for key; do
        mods=
        while :; do
            case $key in
                ctrl-*|shift-*|alt-*) mods="$mods ${key%%-*}"; key=${key#*-} ;;
                *) break ;;
            esac
        done
        held=
        released=
        for mod in $mods; do
            held="$held -M $mod"
            released="-m $mod $released"
        done
        wtype $held -k "$key" $released
        sleep 0.05
    done
}

# Long enough for a pan or a zoom to reach where it was going: a move takes
# motion::DURATION, 200 ms, and the frame after it is what is wanted.
settle() {
    sleep 0.5
}

# Put the pointer where it shows in nothing: the middle of the top bar,
# between the title and the readout. Over the picture it would put a pixel
# readout in the bottom bar, over a button a tooltip, and outside the
# window it would take the keyboard with it, since focus follows it.
#
# It goes out of the window and back in rather than straight there: a warp
# inside the window sends the window no motion, and the readout of wherever
# the pointer last was over the picture would stay in the bar. Leaving and
# entering are events.
park() {
    set -- $(window geometry)
    cursor $(($1 - 8)) $(($2 - 8))
    sleep 0.1
    cursor $(($1 + $3 / 2)) $(($2 + 15))
    sleep 0.2
}

# Capture the window's rectangle to the path given, as a JPEG.
shoot() {
    park
    set -- "$1" $(window geometry)
    grim -g "$2,$3 ${4}x$5" -t jpeg -q 92 "$1"
    echo "$1"
}

# Start recording the window's rectangle to the path given, an MP4 at 60
# frames a second, and return once the first frame is down. `cut` stops it.
# The pointer is left out of the film unless a second argument says
# `cursor`, for a recording of something the pointer does.
#
# The region is handed over in logical pixels, as `hyprctl clients` reports
# it; the recorder scales to the monitor's own pixels itself.
record() {
    park
    set -- "$1" "${2:-no}" $(window geometry)
    mkdir -p "$(dirname "$1")"
    rm -f "$1" "$1.ts"
    case $2 in cursor) shown=yes ;; *) shown=no ;; esac
    gpu-screen-recorder -w "${5}x$6+$3+$4" -f 60 -fm cfr -k h264 -cursor "$shown" \
        -fallback-cpu-encoding yes -write-first-frame-ts yes -o "$1" 2>"$1.log" &
    RECORDER=$!
    RECORDING=$1
    # The .ts file is written with the first frame, and says when that was.
    for _ in $(seq 100); do
        [ -s "$1.ts" ] && break
        kill -0 "$RECORDER" 2>/dev/null || { cat "$1.log" >&2; exit 1; }
        sleep 0.1
    done
    [ -s "$1.ts" ] || { echo "the recorder never wrote a frame" >&2; exit 1; }
    RECORDING_STARTED=$(awk 'NR == 2 { printf "%.6f\n", $2 / 1000000 }' "$1.ts")
}

# Seconds since the recording's first frame, for cutting the film to what
# happened after it.
since_record() {
    awk -v now="$(date +%s.%N)" -v then="$RECORDING_STARTED" 'BEGIN { printf "%.3f\n", now - then }'
}

# Stop the recording. SIGINT is how the recorder is told to finish the file
# rather than abandon it.
cut() {
    kill -INT "$RECORDER"
    wait "$RECORDER" 2>/dev/null || true
    RECORDER=
    rm -f "$RECORDING.log" "$RECORDING.ts"
    echo "$RECORDING"
}

# The recording, or the part of it from START for LENGTH seconds, as a GIF
# WIDTH pixels wide at 25 frames a second: a palette for the whole clip
# first, then the frames dithered against it.
#
#   gif film.mp4 out.gif [WIDTH] [START] [LENGTH]
gif() {
    film=$1
    out=$2
    width=${3:-800}
    window=""
    [ -n "${4:-}" ] && window="-ss $4"
    [ -n "${5:-}" ] && window="$window -t $5"
    filters="fps=25,scale=$width:-1:flags=lanczos"
    ffmpeg -v error -y $window -i "$film" \
        -filter_complex "$filters,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle" \
        -loop 0 "$out"
    echo "$out"
}

# Close the window, and wait until the compositor has forgotten it: the next
# window opened can be given the same address, and would then look like one
# that was already there.
close() {
    kill "$(window pid)" 2>/dev/null || true
    for _ in $(seq 50); do
        [ -z "$(window pid)" ] && break
        sleep 0.1
    done
}

# A string as a Lua literal, for the arguments hyprctl eval is handed.
lua_quote() {
    printf '"%s"' "$(printf '%s' "$1" | sed 's/[\\"]/\\&/g')"
}

# A string as one word of the shell command line Hyprland runs.
shell_quote() {
    printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"
}
