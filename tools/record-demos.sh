#!/bin/sh
# Record the README demos headlessly into docs/img/*.webp.
#
# Each example runs in snapshot mode with XERUNE_SNAPSHOT_FRAMES, on the
# simulated clock, driven by a scripted message list (`@MS` waits), so the
# clips are deterministic and need no screen recorder. Needs ffmpeg with
# libwebp.
#
#   tools/record-demos.sh [NAME...]    # default: all demos
set -eu

cd "$(dirname "$0")/.."
OUT=docs/img
FPS=20
WIDTH=600 # px; the examples render at 800x480 and are scaled down
QUALITY=85

TMP=$(mktemp -d "${TMPDIR:-/tmp}/xerune-demos.XXXXXX")
trap 'rm -rf "$TMP"' EXIT

# Messages typing $2 into the todo input, one character every 90 ms from $1 ms
# (each sets the whole text, as the on-screen keyboard does).
type_todo() {
    t=$1 text=$2 i=1
    while [ "$i" -le "${#text}" ]; do
        printf '@%d;todo_input:value:%s;' "$t" "$(printf '%s' "$text" | cut -c1-"$i")"
        t=$((t + 90)) i=$((i + 1))
    done
}

# record NAME SECONDS MESSAGES [START_MS] [FPS] [QUALITY]
record() {
    name=$1 seconds=$2 messages=$3 start=${4:-0} fps=${5:-$FPS} quality=${6:-$QUALITY}
    echo "recording $name (${seconds}s)"
    rm -f "$TMP"/f_*.png
    XERUNE_SNAPSHOT="$TMP/f.png" XERUNE_SNAPSHOT_SIZE=800x480 \
        XERUNE_SNAPSHOT_FRAMES=$((seconds * fps)) XERUNE_SNAPSHOT_FPS=$fps \
        XERUNE_SNAPSHOT_MESSAGES="$messages" XERUNE_SNAPSHOT_TIME_MS=$start \
        "target/release/examples/$name" >/dev/null
    ffmpeg -v error -y -framerate "$fps" -i "$TMP/f_%04d.png" \
        -vf "scale=$WIDTH:-1:flags=lanczos" \
        -c:v libwebp_anim -lossless 0 -quality "$quality" -compression_level 6 -loop 0 \
        "$OUT/$name.webp"
    echo "  $OUT/$name.webp: $(du -k "$OUT/$name.webp" | cut -f1) KB"
}

demos=${*:-animation animation_css breakout music_player showcase todo}
# shellcheck disable=SC2086
nice cargo build -j4 --release $(for d in $demos; do printf -- '--example %s ' "$d"; done)

for demo in $demos; do
    case $demo in
    animation)
        # 100 moving boxes compress badly: shorter, slower and softer.
        # Starts after the first FPS reading.
        record animation 4 "" 1000 15 55 ;;
    animation_css) record animation_css 5 "" ;;
    breakout)
        # The paddle meets the ball; the key times were found by recording,
        # finding where the ball crossed the paddle line, and moving there.
        record breakout 8 "@905;keydown:ArrowRight;@1633;keyup:ArrowRight;@2951;keydown:ArrowLeft;@3183;keyup:ArrowLeft;@4105;keydown:ArrowLeft;@4750;keyup:ArrowLeft;@5686;keydown:ArrowRight;@6283;keyup:ArrowRight" ;;
    music_player)
        record music_player 9 "@600;hover_track:1;@1200;hover_track:3;@1800;select_track:3;@5000;next;@6800;back;@7600;hover_track:4" ;;
    showcase)
        record showcase 10 "@1000;increment_progress;@1700;toggle_maintenance;@2500;select_tab:2;@3200;increment_speed;@3500;increment_speed;@3800;increment_speed;@5000;select_tab:3;@7000;select_tab:4;@7700;toggle_wifi;@8300;toggle_bluetooth;@9200;select_tab:1" ;;
    todo)
        record todo 9 "$(type_todo 700 'Ship the demo clips')@2700;add;@3400;toggle:0;@4000;toggle:3;@4800;remove:2;@6000;lang:de;@7500;lang:ko" ;;
    *) echo "unknown demo $demo" >&2; exit 1 ;;
    esac
done
