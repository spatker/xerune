# Xerune

CPU-only HTML/CSS renderer and Elm-style (MVU) UI framework for embedded
Linux without a GPU (ARMv7 boards, fbdev, DRM/KMS). Templates are HTML/CSS
compiled to Rust by a proc macro; layout is Taffy; drawing is `xerune_render`.

User docs: `README.md`, `docs/guide.md` (API, templates, snapshots, embedded),
`docs/architecture.md` (pipeline). Read those for feature details; this file is
the working notes.

## Workspace

| Path | What |
|---|---|
| `src/` | `xerune` lib: runtime, UI tree, CSS, backends |
| `xerune_template/` | The template language: includes, Askama parse, validation (the supported subset is what `lower.rs` accepts), html5ever, giving one `Template` tree of elements and template blocks. Shared by the derive and, with feature `interpret`, the studio's interpreter (`render_html`), so they can't drift |
| `xerune_derive/` | `XeruneTemplate` / `XeruneMessage` macros. `lib.rs` compiles a `xerune_template::Template` to `build_ui`: compile-time selector matching, CSS splitting, codegen |
| `xerune_css/` | The CSS parser both of them use (`no_std`): stylesheets and declarations (`sheet`), selectors with specificity and matching over an `Element` trait (`selector`), `@media` blocks and queries, comment/string-aware scanning. Change CSS reading here, not in either crate |
| `xerune_render/` | Software rasterizer (blits, rounded rects, gradients, glyphs). Has a `no_std` mode |
| `studio/` | WASM inspector (`wasm-pack build --target web`, `features = ["browser"]`): the model runs in wasm, the view is the template interpreted live (`xerune.js` polls the files; no rebuild for template edits, one for newly called model methods) |
| `wpt_runner/` | WPT reftests vs Chromium screenshots; scope in `suites.txt`, known failures in `expectations.txt` (`docs/guide.md` §13) |
| `examples/<name>/` | `main.rs` (runtime + `xerune::App`) + `<name>_impl.rs` (model; also used by tests, benches, studio) + `templates/*.html` (+ `i18n/*.txt` strings in todo and showcase, English and German; `tests/i18n_tests.rs` checks every language has every key); `examples/support/` loads the bundled fonts |
| `tests/` | Integration tests; HTML fixtures in `tests/*.html`, `tests/canvas/`, `tests/gaps/`; trybuild cases in `tests/template_errors/` |
| `tools/` | Measurement scripts: ARM instruction counts under qemu, Chromium line breaks (`tools/README.md`) |
| `build_arm.sh`, `qemu-arm/` | Local ARM cross-build/deploy helpers (untracked / user's own) |

## Pipeline

1. Input (`backend/` → `InputEvent`) → `Runtime` → `Model::update(msg, &mut Context<Msg>)`.
2. Rebuild unless `Model::view_fingerprint` returns the same `Some(..)` and the
   viewport didn't change. Rebuild = generated `build_ui` → `UiBuilder` →
   style resolution → fresh Taffy tree.
3. Taffy layout → walk → absolute `DrawCommand`s (rects, text, images,
   check marks, canvas, clip push/pop).
4. `Renderer` (normally `xerune_render`) draws into a `u32` buffer; the backend presents it.

Modules:
- `model.rs`: `Model`, `InputEvent`, `hash_bytes` (FNV, for fingerprints).
- `runtime/`: `poll` (timers, subscriptions, frames at `target_fps`, returns `Wake`), `Clock`, keyframe animations (by key), transitions (by key + property, `runtime/transition.rs`), fingerprint gating.
- `ui/`: `builder` (tree building), `attributes`, `style_resolution` (+ style cache), `metadata` (`NodeMetadata`), `node_map`.
- `style.rs`: `ContainerStyle` and CSS enums. `graphics.rs`: `Color`, `Canvas`, `Context`, `DrawCommand`, `TextMeasurer`, `Renderer`, `Surface` (pixels, stride, rotation, origin: a band or the whole screen), `Damage`.
- `xerune_render`: `SoftwareRenderer` (owns glyph/image caches) and `Fonts` (TTF via fontdue or bitmap fonts; the runtime's `TextMeasurer`).
- `css/`: values/properties parser, gradients (linear/radial/conic), `@keyframes`, `transition`, `media` (`@media` evaluation; `xerune_css::sheet` records which blocks each rule is in).
- `screen.rs`: global viewport (two atomics) + `Breakpoint`. `Runtime::set_size` → `Model::on_resize` → rebuild.
- `keyboard/`: on-screen keyboard (`Context::edit_text(TextRequest, on_done)`). A derived template inside the crate (`extern crate self as xerune`); while open the runtime builds its tree instead of the model's (`KeyboardSession`), routes clicks/keys/`kb:*` messages to it, and restores the model's scroll offsets on close. Opt-in `Runtime::set_input_keyboard` (`XERUNE_INPUT_KEYBOARD=1`) opens it on tapping a text input and sends `<id>:value:<text>`. `keys.rs`: `KeyMap` (backend key names → `KeyAction`). Keyboard languages are `i18n` catalogs (`src/keyboard/en.txt`, `de.txt`: letter rows + labels), picked by `TextRequest::lang` or the input's inherited `lang` attribute (`Ui::lang`). Korean (`ko.txt`, `compose = hangul`, `shift_rowN`) composes jamo into syllables (`keyboard/hangul.rs`).
- `i18n.rs`: `Catalog` (`key = value` text) and `Translations` (`t(lang, key)`, fallback to the first catalog). The language is a plain model field; templates call a model method `self.t("key")` (guide §9).
- `backend/`: `winit_impl` (desktop), `linuxfb_impl`, `drm_impl`, `input/evdev`, `browser_impl`, `snapshot`, `common_loop`.
- `text.rs`: `ShapedText` (glyphs, advances, UAX #14 break segments via `unicode-linebreak`), line breaking, `GlyphRun`. Shaping is `TextMeasurer::shape` (fontdue advances in `xerune_render::Fonts`, cached per font set); layout measures text leaves with a taffy measure function (`Ui::compute_layout`).
- `font.rs`, `roboto_*.rs`: embedded bitmap fonts (`no_std` path).

Templates: `{{ expr }}`, `{% if %}/{% elif %}/{% else %}`, `{% if let %}`,
`{% for x in xs %}` with `loop.index`/`loop.index0`, `data-on-click="msg:{{ arg }}"`
(message strings parsed by `XeruneMessage`), also `data-on-hover(-out)` and
`data-on-transition{run,start,end,cancel}`. Selectors are matched when the
derive runs; classes set by `{% if %}` (on the element or an ancestor) become
runtime conditions. `tests/template_grammar.html` is
the grammar regression fixture; extend it when touching the template parser
(`xerune_template/tests/parse.rs` parses every template in the repo).

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

Text must match Chromium: `tests/text/reference.html` lists Chromium's line
breaks in `text_layout::line_breaks_match_chromium`. To compare a page,
`tools/chromium-lines.sh page.html` prints Chromium's lines with the bundled
Roboto, or take screenshots. `cargo run --release --example text_bench` times rebuild,
layout and render of a text-heavy view.
`cargo run --release --example render_bench` times full repaints and single
interactions of the example UIs; `tools/arm-insns.sh` counts its ARM
instructions under qemu-arm (no board needed). See `tools/README.md`.
WPT: `WPT_DIR=~/data/code/js/wpt cargo run --release -p wpt_runner [-- FILTER] [--images] [--update]`
(user's checkout path). Commit the `--update`d `expectations.txt` with each engine fix.
Snapshots run on a `ManualClock`, so timed examples (`animation`, `breakout`)
are deterministic too.

CI (`.github/workflows/ci.yml`, rustc 1.85.1): workspace tests + examples,
the armv7 zigbuild below (with `drm,linuxfb,evdev`), the studio for wasm32,
and a no_std check of `xerune` + `xerune_render` for `thumbv7em-none-eabihf`
(`--no-default-features`; runtime CSS, image loading and TrueType are std-only).

ARM target build (user's setup; use it for every ARM check, locally and in CI,
not gcc-arm or `cargo check`: it links against the boards' glibc 2.39):
`RUSTFLAGS="-C target-feature=+neon,+vfp4,+d32" cargo zigbuild --target armv7-unknown-linux-gnueabihf.2.39 --features "drm,evdev" --release --example X`
(the default armv7 target has no NEON; without the flags `blitter.rs` falls back to scalar loops)

## Constraints

- Toolchain stays at rustc 1.85.1 (edition 2024), no nightly / build-std.
- Release profile keeps `opt-level = 3`; smaller opt-levels hurt the renderer on ARM.
- Keep the `http` feature (remote images via ureq) in `std`.
- Rendering (`xerune_render`) is the bottleneck on target hardware, not layout.
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
  `examples: todo: ...`, `xerune_render: ...`), short body explaining why.
  One work unit per commit, committed as each unit is done.
- Match the surrounding code; keep docs (`docs/guide.md`, this file) in sync
  when behavior changes.

## Known weak spots

- Text: inline elements (`<b>`, `<span>`) are separate flex items, not one inline formatting context (`known_gaps::text_flows_around_inline_elements`). No OpenType shaping: fontdue ignores GPOS kerning (`known_gaps::gpos_kerning_narrows_lines`) and font features (`proportional-nums`; the calculator's right-aligned tabular digits shift as in Chromium), no complex scripts or bidi; `harfrust` is the planned shaper behind `TextMeasurer::shape`. Thai/Lao/Khmer/Myanmar break only at spaces.
- Sibling and structural selectors (`+`, `~`, `:nth-child`, `:last-child`) don't see through `{% for %}` / `{% if %}` in templates, so they never match inside a loop body.
- Images: PNG only.
- Borders: per-side width/color/style, but every style is drawn solid (no
  dashed/dotted/double/3D); `z-index` stacking is simplified (see
  `paint_layer` in `src/ui/mod.rs`).
