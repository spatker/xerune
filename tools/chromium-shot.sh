#!/bin/sh
# A screenshot of a page in Chromium, with the fonts the examples bundle
# (Roboto, and the Hangul fallback).
#
#   tools/chromium-shot.sh tests/text/glyphs.html tests/text/glyphs-chromium.png [800x480]
#
# The reference of text_raster::glyphs_match_chromium is made this way.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
page=$1
out=$(realpath "$2")
size=${3:-800x480}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

"$root/tools/chromium-page.sh" "$page" > "$tmp/page.html"

chromium --headless --disable-gpu --allow-file-access-from-files --hide-scrollbars \
    --window-size="$(echo "$size" | tr x ,)" --virtual-time-budget=5000 \
    --screenshot="$out" "file://$tmp/page.html" 2>/dev/null
echo "$out"
