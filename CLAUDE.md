# Xerune

CPU-only HTML/CSS renderer and Elm-style (MVU) UI framework for embedded
Linux without a GPU (ARMv7 boards, fbdev, DRM/KMS). Templates are HTML/CSS
compiled to Rust by a proc macro; layout is Taffy; drawing is `fast_renderer`.

User docs: `README.md`, `docs/guide.md` (API, templates, snapshots, embedded),
`docs/architecture.md` (pipeline). Read those for feature details; this file is
the working notes.

## Workspace

| Path | What |
|---|---|
| `src/` | `xerune` lib: runtime, UI tree, CSS, backends |
| `xerune_derive/` | `XeruneTemplate` / `XeruneMessage` macros. One big `lib.rs`: template parser, CSS splitting, codegen of `build_ui` |
| `fast_renderer/` | Software rasterizer (blits, rounded rects, gradients, glyphs). Has a `no_std` mode |
| `studio/` | WASM inspector (`wasm-pack build --target web`, `features = ["browser"]`) |
| `wpt_runner/` | Runs WPT-style reftests through the `dynamic-parser` path |
| `examples/<name>/` | `main.rs` + `<name>_impl.rs` + `templates/*.html`; shared render loop in `examples/support/` |
| `tests/` | Integration tests; HTML fixtures in `tests/*.html`, `tests/canvas/`, `tests/gaps/`; trybuild cases in `tests/template_errors/` |
| `build_arm.sh`, `qemu-arm/` | Local ARM cross-build/deploy helpers (untracked / user's own) |

## Pipeline

1. Input (`backend/` → `InputEvent`) → `Runtime` → `Model::update(msg, &mut Context)`.
2. Rebuild unless `Model::view_fingerprint` returns the same `Some(..)` and the
   viewport didn't change. Rebuild = generated `build_ui` → `UiBuilder` →
   style resolution → fresh Taffy tree.
3. Taffy layout → walk → absolute `DrawCommand`s (rects, text, images,
   checkbox/slider/progress, canvas, clip push/pop).
4. `Renderer` (normally `fast_renderer`) draws into a `u32` buffer; the backend presents it.

Modules:
- `model.rs`: `Model`, `InputEvent`, `hash_bytes` (FNV, for fingerprints).
- `runtime/`: tick loop, message queue, timers, animations, fingerprint gating.
- `ui/`: `builder` (tree building), `attributes`, `style_resolution` (+ style cache), `metadata` (`NodeMetadata`), `node_map`.
- `style.rs`: `ContainerStyle` and CSS enums. `graphics.rs`: `Color`, `Canvas`, `Context`, `DrawCommand`, `TextMeasurer`, `Renderer`.
- `css/`: values/properties parser, gradients (linear/radial/conic), `@keyframes`, `media` (`@media` split/match; `simplecss` drops at-rules).
- `screen.rs`: global viewport (two atomics) + `Breakpoint`. `Runtime::set_size` → `Model::on_resize` → rebuild.
- `backend/`: `winit_impl` (desktop), `linuxfb_impl`, `drm_impl`, `input/evdev`, `browser_impl`, `snapshot`, `common_loop`.
- `font.rs`, `roboto_*.rs`: embedded fonts (fontdue; single-line text only).

Templates: `{{ expr }}`, `{% if %}/{% elif %}/{% else %}`, `{% if let %}`,
`{% for x in xs %}` with `loop.index`/`loop.index0`, `data-on-click="msg:{{ arg }}"`
(message strings parsed by `XeruneMessage`). `tests/template_grammar.html` is
the grammar regression fixture; extend it when touching the template parser.

## Commands

The user's machine freezes under heavy builds: always use `nice` and `-j4`.

```sh
nice cargo build -j4
nice cargo test -j4                       # all tests
nice cargo test -j4 --test layout_tests   # one test binary
nice cargo test -j4 -- --ignored          # known engine gaps (expected to fail)
nice cargo run -j4 --release --example showcase
```

Look at UI changes headlessly instead of opening windows:

```sh
XERUNE_SNAPSHOT=/path/out.png [XERUNE_SNAPSHOT_SIZE=480x800] \
  [XERUNE_SNAPSHOT_MESSAGES="add;toggle:0"] [XERUNE_SNAPSHOT_TIME_MS=1500] \
  cargo run --example todo
```

Then Read the PNG. Write snapshots to the scratchpad, not the repo.
`animation` and `breakout` read `Instant::now()`, so their snapshots aren't
deterministic.

ARM target build (user's setup):
`cargo zigbuild --target armv7-unknown-linux-gnueabihf.2.39 --features "drm,evdev" --release --example X`

## Constraints

- Toolchain stays at rustc 1.85.1 (edition 2024), no nightly / build-std.
- Release profile keeps `opt-level = 3`; smaller opt-levels hurt the renderer on ARM.
- Keep the `http` feature (remote images via ureq) in `std`.
- Rendering (`fast_renderer`) is the bottleneck on target hardware, not layout.
  Benchmark changes to fill/blit paths.
- Don't touch `drm_impl.rs` / `linuxfb_impl.rs` behavior without hardware to test on.

## Conventions

- Fix the engine instead of routing around it in templates/examples. If a
  workaround is unavoidable, mark it `FIXME` naming the engine limitation and
  tell the user.
- Known engine gaps live as `#[ignore = "FIXME ..."]` tests in
  `tests/layout_tests.rs` (`mod known_gaps`, fixtures in `tests/gaps/`).
  Remove the `#[ignore]` in the same commit as the fix.
- Examples target 800x480 and have a portrait layout
  (`@media (orientation: portrait)`); check both sizes. No decorative emoji
  (they render as boxes without `--features emoji`; Roboto lacks arrows and ✓).
- Commits: kernel style, `subsystem: summary` (e.g. `xerune_derive: ...`,
  `examples: todo: ...`, `fast_renderer: ...`), short body explaining why.
  One work unit per commit, committed as each unit is done.
- Match the surrounding code; keep docs (`docs/guide.md`, this file) in sync
  when behavior changes.

## Known weak spots

- Text: no wrapping, single-line measurement only.
- `:first-child` / `+` selectors don't see through `{% for %}` / `{% if %}`.
- Same-named templates in different `examples/*/templates` resolve in
  `read_dir` order.
- Images: PNG only.
