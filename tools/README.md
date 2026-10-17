# Tools

Measurement helpers for engine work. Measure before and after a change;
these make that cheap.

## Rendering cost

- `cargo run --release --example render_bench`: times full repaints and
  single interactions of the todo, calculator, showcase and music player
  UIs, the share of the screen an interaction repaints, and a hash of the
  pixels (compare hashes on one architecture; SSE and NEON round
  differently). `RENDER_BENCH_SCENES`, `RENDER_BENCH_PHASE=raster|frame`,
  `RENDER_BENCH_ITERS`, `RENDER_BENCH_SIZE`.
- `cargo run --release --example text_bench`: rebuild, layout and render
  of a text-heavy feed.
- `tools/arm-insns.sh`: per-iteration ARM instruction counts of
  `render_bench`, built with the boards' NEON flags and run under
  `qemu-arm` with the counting plugin in `qemu-insn/`. Exact and
  repeatable, so it catches regressions without the hardware, but it
  ignores caches, memory bandwidth and latency: confirm important wins on
  the board.

  ```sh
  tools/arm-insns.sh --save before    # on the old tree
  # ... change things ...
  tools/arm-insns.sh before current   # side by side
  ```

  Needs `qemu-user`, `cargo-zigbuild`, the `armv7-unknown-linux-musleabihf`
  Rust target, a C compiler and `glib2` headers (for the plugin).
- On x86, `perf stat -e instructions:u <binary>` gives the same kind of
  count natively, and `perf record` / `perf report` show where it goes.
  Headless snapshots (`XERUNE_SNAPSHOT`, see `docs/guide.md`) only render
  the final frame, so profile rebuild and layout with them, rendering
  with `render_bench` (or record frames with `XERUNE_SNAPSHOT_FRAMES`).
- `XERUNE_PROFILE=1` on any app, on the board too: once a second, the
  time per frame in each phase (update, view, layout, animate, paint,
  raster) and the renderer's time per kind of draw command (guide §7).

## Matching Chromium

- `tools/chromium-lines.sh PAGE.html [800x480]`: Chromium's line breaks
  for a page with the bundled fonts (Roboto, then the Hangul fallback), as
  JSON, via `tests/text/chromium_lines.js`. The expected lines of
  `text_layout::line_breaks_match_chromium` come from
  `tests/text/reference.html`, and those of
  `korean_line_breaks_match_chromium` from `tests/text/korean.html`.
- `wpt_runner` compares WPT reftests with Chromium screenshots (see
  `docs/guide.md` §13).

## Fonts

- `tools/subset-hangul.py SOURCE_HAN_SANS_KR_DIR` makes
  `resources/fonts/XeruneHangul-{Regular,Bold}.otf`, the examples' Korean
  fallback: Source Han Sans KR cut down to Hangul and CJK punctuation
  (~1.9 MB each instead of 4.6), renamed because its license (SIL OFL,
  `XeruneHangul-LICENSE.txt`) reserves the name "Source" for unmodified
  fonts. Needs fontTools.

## README demos

- `tools/record-demos.sh [NAME...]`: records the clips in `docs/img/*.webp`
  headlessly (`XERUNE_SNAPSHOT_FRAMES`, guide §10): each example plays a
  scripted message list on the simulated clock, so a clip only changes when
  the UI does. Animated WebP, 600 px wide at 20 fps, a few hundred KB each.
  Needs ffmpeg with libwebp. Re-record after visible UI changes; every
  recording adds its bytes to the history, so not after every tweak.
