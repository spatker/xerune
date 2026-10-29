#!/bin/sh
# Prints PAGE with the fonts the examples bundle (Roboto, and the Hangul
# fallback) forced on every element, for Chromium to load from a file:
# URL; then SCRIPT, if given, inline. Shared by chromium-lines.sh and
# chromium-shot.sh.
#
#   tools/chromium-page.sh PAGE.html [SCRIPT.js] > page.html
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
fonts=$root/resources/fonts

cat <<CSS
<style>
@font-face { font-family: XeruneRoboto; src: url("file://$fonts/Roboto-Regular.ttf"); font-weight: 400; }
@font-face { font-family: XeruneRoboto; src: url("file://$fonts/Roboto-Bold.ttf"); font-weight: 700; }
@font-face { font-family: XeruneHangul; src: url("file://$fonts/XeruneHangul-Regular.otf"); font-weight: 400; }
@font-face { font-family: XeruneHangul; src: url("file://$fonts/XeruneHangul-Bold.otf"); font-weight: 700; }
* { font-family: XeruneRoboto, XeruneHangul !important; }
</style>
CSS
cat "$1"
if [ $# -gt 1 ]; then
    printf '<script>\n'
    cat "$2"
    printf '</script>\n'
fi
