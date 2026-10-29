#!/bin/sh
# Chromium's line breaks for a page, with the fonts the examples bundle
# (Roboto, and the Hangul fallback).
#
#   tools/chromium-lines.sh tests/text/reference.html [800x480]
#
# Prints the text of every line box in document order as JSON (via
# tests/text/chromium_lines.js); paste it into the expected lines of
# text_layout::line_breaks_match_chromium or compare with xerune's output.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
page=$1
size=${2:-800x480}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

"$root/tools/chromium-page.sh" "$page" "$root/tests/text/chromium_lines.js" > "$tmp/page.html"

chromium --headless --disable-gpu --allow-file-access-from-files \
    --window-size="$(echo "$size" | tr x ,)" --virtual-time-budget=5000 \
    --dump-dom "file://$tmp/page.html" 2>/dev/null |
    sed -n 's/.*<pre id="lines">\(.*\)<\/pre>.*/\1/p' |
    sed 's/&quot;/"/g; s/&lt;/</g; s/&gt;/>/g; s/&amp;/\&/g'
