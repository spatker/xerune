# Xerune Quick Start Guide

This guide walks you through building your first application using Xerune, styling it with CSS, using custom canvases, and deploying to embedded backends.

---

## 1. Minimal Application Setup

Xerune applications follow the Elm/MVU (Model-View-Update) pattern. For a complete, runnable reference implementation, see [`examples/todo`](../examples/todo/).

A complete application consists of:

### 1. Application State & Messages

Define your model struct and message enum:

```rust
use xerune::{Context, Model, XeruneMessage, XeruneTemplate};

#[derive(Default, XeruneTemplate)]
#[template(path = "todo_list.html")]
pub struct TodoList {
    items: Vec<TodoItem>,
    active_item: usize,
    new_item_title: String,
}

#[derive(Debug, Clone, XeruneMessage)]
pub enum TodoMsg {
    Toggle(usize),
    Remove(usize),
    Add,
}

impl Model for TodoList {
    type Message = TodoMsg;

    fn update(&mut self, msg: Self::Message, _ctx: &mut Context) {
        match msg {
            TodoMsg::Toggle(index) => {
                if let Some(item) = self.items.get_mut(index) {
                    item.completed = !item.completed;
                }
            }
            TodoMsg::Remove(index) => {
                if index < self.items.len() {
                    self.items.remove(index);
                }
            }
            TodoMsg::Add => { /* ... */ }
        }
    }
}
```

### 2. Declarative Template Layout

Templates are precompiled at compile time via `#[derive(XeruneTemplate)]` and bind interaction handlers via `data-on-click`:

```html
<div data-on-click="add" class="add-btn">Add</div>

{% for item in items %}
<div class="todo-item" data-on-click="toggle:{{ loop.index0 }}">
    <span>{{ item.title }}</span>
    <div data-on-click="remove:{{ loop.index0 }}" class="remove-btn">x</div>
</div>
{% endfor %}
```

A message is named after its variant in snake_case: `add` for `Add`,
`toggle:` followed by the argument for `Toggle(usize)` (`#[xerune(rename =
"...")]` and `#[xerune(prefix = "...")]` change that). The template builds
the message itself, so `toggle:{{ loop.index0 }}` is type-checked: passing a
`String` where `Toggle` takes a `usize` is a compile error, and the argument
must have the field's exact type or be a reference to it (`&str` works for a
`String` field). Static arguments (`digit:7`) are parsed when the view is
built and an invalid one is logged then. Messages must be `Clone`: an
element keeps its message and sends a copy on each click. The runtime's own
messages (`keydown:Enter`, `<input id>:text:...`) and those from
`Messenger::send`, `XERUNE_SNAPSHOT_MESSAGES` or JavaScript are strings,
parsed with the derived `FromStr`.

To scroll an element into view, give it an `id` and call
`context.scroll_into_view("item-3")` from `update`:
`<div id="item-{{ loop.index0 }}" ...>`.

See [`examples/todo/templates/todo_list.html`](../examples/todo/templates/todo_list.html) for the full layout and CSS.

#### Template syntax

Templates are parsed with [Askama](https://askama.readthedocs.io/)'s parser, so expressions are **Rust**, not Jinja. Everything below is checked at compile time; anything unsupported is a compile error pointing at the template line.

| Construct | Example | Notes |
|---|---|---|
| Expression | `{{ name }}`, `{{ self.total() }}`, `{{ item.title }}` | Bare names are model fields (`self.name`) unless bound by `for` / `if let`. |
| Formatting | `{{ "{:.1}"\|format(volume) }}` | `format` is the **only** filter. For anything else, add a method to your model. |
| Conditionals | `{% if a %}…{% elif b %}…{% else %}…{% endif %}` | |
| Pattern match | `{% if let Some(c) = cover %}{{ c }}{% endif %}` | The value is borrowed; `c` is a reference. |
| Loops | `{% for item in items %}`, `{% for (k, v) in pairs %}`, `{% for i in 0..n %}` | `loop.index` (1-based) and `loop.index0` are available. `{% break %}` / `{% continue %}` work. |
| Includes | `{% include "header.html" %}` | Resolved relative to the including file. |
| Comments | `{# … #}` | |

Operators are Rust's: `&&`, `||`, `!`, `==`, `!=`, `<`, … (**not** `and` / `or` / `not`).

Not supported: other filters (`upper`, `safe`, …), `{% match %}`, `{% let %}` / `{% set %}`, `{% macro %}`, `{% block %}` / `{% extends %}`, `{% for … if cond %}`, and `{% else %}` inside `{% for %}`.

**Inside tags**, template code may only appear *inside a quoted attribute value*:

```html
<!-- yes -->
<div class="row {% if selected %}active{% else %}idle{% endif %}">
<img src="{{ cover_url }}">
<p class='{% if title != "" %}has-title{% endif %}'>  <!-- either quote style works -->

<!-- no: compile error -->
<div {% if selected %}class="active"{% endif %}>
```

The one exception is `<input type="checkbox" {% if done %}checked{% endif %}>`.

**Class names must be literals.** CSS is matched against classes at compile time, so `class="{{ name }}"` is rejected; switch between literal classes with `{% if %}` instead.

`#[template(path = "...")]` is looked up, in order: in the crate root, in
its `templates/` directory, and in the workspace root (so crates of a
workspace can share templates). `{% include "..." %}` looks next to the
including template first. A missing template lists every place tried.

For small components and tests, a template can be inline instead of a file: `#[template(source = r#"<p>{{ name }}</p>"#)]`.

### 3. Running the App

`xerune_render` provides the fonts (which the runtime measures text with) and
the software renderer; `App` runs both on a window or a display:

```rust
use xerune::{App, Runtime};
use xerune_render::{Fonts, SoftwareRenderer};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fonts = Fonts::ttf(include_bytes!("Roboto-Regular.ttf"), Some(include_bytes!("Roboto-Bold.ttf")))?;
    let runtime = Runtime::new(TodoList::default(), fonts.clone());
    App::new(runtime).title("Todo").size(800, 480).run(SoftwareRenderer::new(fonts))?;
    Ok(())
}
```

`App::run` picks the backend when the app starts, among those compiled in:
a window (`winit`) in a desktop session, else DRM/KMS, else the fbdev
framebuffer; `XERUNE_BACKEND=winit|drm|fbdev` overrides it. So one binary
built with `winit,drm,evdev` runs on a desktop and on the device.
`App::on_start` receives a `Messenger` for sending messages from other
threads. `Fonts::bitmap(&xerune::font::DEFAULT_ROBOTO)` uses the bundled
prerasterized fonts instead of TrueType.

```bash
cargo run --release --example todo
```

---

## 2. Using Custom Canvas Buffers

Custom canvas allows you to directly manipulate raw pixel colors:

```rust
// 1. Declare <canvas id="my_canvas"> in HTML template
// 2. Access and draw onto the canvas inside update loop:
fn update(&mut self, msg: Self::Message, ctx: &mut Context) {
    if let Some(canvas) = ctx.get_canvas_mut("my_canvas", 200, 200) {
        // Clear canvas with white color
        canvas.data.fill(255);
        // Paint a blue pixel at (50, 50)
        let idx = (50 * 200 + 50) * 4;
        canvas.data[idx] = 0;     // Red
        canvas.data[idx + 1] = 0; // Green
        canvas.data[idx + 2] = 255; // Blue
        canvas.data[idx + 3] = 255; // Alpha
        canvas.dirty = true;
    }
}
```

---

## 3. Styling with CSS Stylesheets

Templates can embed global or component stylesheets inside `<style>` blocks:

```html
<style>
    .card {
        background-color: #333333;
        border-radius: 8px;
        padding: 20px;
    }
    #title {
        color: #ffcc00;
        font-size: 24px;
    }
</style>
<div class="card">
    <span id="title">Hello World</span>
</div>
```

```rust
#[derive(XeruneTemplate)]
#[template(path = "my_template.html")]
pub struct MyModel {
    // ...
}
```

CSS is resolved at **compile time**: selectors are matched against the template and each element gets the declarations that apply to it, guarded by any `{% if %}` conditions on its classes. Unsupported properties are ignored with a one-time warning at runtime.

**Selectors:** type (`p`), class (`.card`), id (`#title`), attribute (`[type=checkbox]`), universal (`*`), descendant (`.list p`), child (`.list > p`), adjacent sibling (`h1 + p`), `:first-child`, and `@media` queries (see below). Descendant and child selectors see through `{% for %}` / `{% if %}`. Sibling selectors and `:first-child` do not: which iteration or branch comes first is only known at runtime, so they never match across a loop or conditional. No `:hover` / `:focus` / `:nth-child`.

**Properties:**

- Box: `width`, `height`, `min-/max-width`, `min-/max-height`, logical sizes (`inline-size`, `block-size` and their `min-`/`max-` forms), `margin*`, `padding*`, `box-sizing`
- Layout: `display` (`block`, `inline-block`, `inline`, `flex`, `none`), `flex`, `flex-direction`, `flex-wrap`, `flex-flow`, `flex-grow`, `flex-shrink`, `flex-basis`, `justify-content`, `align-items`, `align-self`, `align-content`, `gap`, `row-gap`, `column-gap`, `order`, `position` (`static`, `relative`, `absolute`), `top`/`right`/`bottom`/`left`, `float`, `overflow`, `direction`, `writing-mode`
- Visual: `color`, `background`, `background-color`, `background-image`, `border*` (width, color, style, per side), `border-radius`, `box-shadow`
- Gradients: `linear-gradient`, `radial-gradient`, `conic-gradient` and the `repeating-` radial/conic forms, with any CSS color, hard stops (`red 0deg 90deg`) and `from <angle>` / `at <position>`. Stop positions take `%` (and angles for conic); `px` stops are not supported. Conic gradients plus `border-radius: 50%` make rings, arcs and dials without a canvas — see the vehicle tab of `examples/showcase`.
- Text: `font-size`, `font-weight`, `font` (size, weight and `/line-height`), `line-height`, `text-align`, `white-space` (`normal`, `nowrap`, `pre`, `pre-wrap`, `pre-line`), `overflow-wrap` / `word-wrap` (`break-word`, `anywhere`), `word-break` (`break-all`), `text-overflow` (`ellipsis`, with `overflow` other than `visible`)
- Animation: `animation` and all `animation-*` longhands, with `@keyframes`

Not supported, among others: `font-family` (fonts are compiled bitmap fonts), `opacity`, `transform`, `transition`, `display: grid`, `z-index`.

**Text layout.** Text wraps to its container like in a browser: lines break at Unicode (UAX #14) break opportunities, so spaces, hyphens and between CJK characters; `text-align` aligns each line. A run of text and `{{ }}` values (`Hello {{ name }}, you have {{ n }} messages`) is one paragraph and wraps as a whole. Limits: inline elements (`<b>`, `<span>`, `<a>`) are separate boxes rather than part of the surrounding paragraph, so text after one starts a new box; there is no OpenType shaping (Roboto's kerning, ligatures, font features such as `font-variant-numeric: proportional-nums`, Arabic or Indic scripts). Roboto's digits are tabular, so right-aligned numbers move as digits with different side bearings appear (a lone `1` ends short of the edge), exactly as in Chromium; Thai, Lao, Khmer and Myanmar break only at spaces. For truncated single lines, use `white-space: nowrap; overflow: hidden; text-overflow: ellipsis` on the element holding the text.

**Page background.** As in browsers, the background of `<html>` (or, if it has none, of `<body>`) paints the whole viewport, behind everything. Without one, the viewport is white, or `#121212` with `html { color-scheme: dark; }`. Every frame repaints from this canvas, so a translucent background never shows the previous frame. If an opaque, square-cornered box already covers the viewport (a full-screen `.app` div, as in the examples), no canvas fill is drawn at all.

As in browsers, `box-sizing` defaults to `content-box`: `width: 800px; padding: 10px` takes up 820px. Add `* { box-sizing: border-box; }` if you prefer the other model.

Unsupported **tags** fall back to a generic inline-block box (with a one-time warning). Elements with dedicated behaviour: `div`, `p`, `pre`, `span`, `h1`–`h6`, `ul`/`li`, `table`/`tr`/`td`/`th` (+ `thead`/`tbody`/`tfoot`), `strong`/`b`, `img`, `button`, `input` (`type="checkbox"` / text), `progress`, `canvas`, `br`.

---

## 4. Responsive Design

A single template adapts to any screen size using standard CSS building blocks, plus a small Rust-side API. Every example targets an 800x480 panel and switches to a portrait (480x800) layout with an `orientation` media query. Several also use `on_resize`: `showcase` picks a canvas size, `music_player` slides its views by the viewport width, and `breakout` and `animation` lay out their playfield from it. Resize the window, or render headlessly with `XERUNE_SNAPSHOT=out.png XERUNE_SNAPSHOT_SIZE=480x800`, to see the switch.

### Media Queries

`@media` blocks are resolved against the current viewport every time the UI tree is built. Supported features: `min-width`, `max-width`, `min-height`, `max-height` (and `device-*` aliases), `orientation: portrait|landscape`. Media types `all`/`screen` pass, `print` never matches, `not (feature)` is supported. Unknown features never match (CSS spec behavior).

```css
.card { width: 46%; }
@media (min-width: 1024px) { .card { width: 29%; } }
@media (orientation: landscape) { .toolbar { height: 48px; } }
```

For compiled templates the queries are parsed **at macro time**: each rule inside an `@media` block is inlined into the generated build code behind a `xerune::css::media::matches_query(&[...])` predicate built from static feature constants — matching costs two atomic reads plus a tiny slice scan, no runtime CSS parsing.

### Viewport Units

`vw`, `vh`, `vmin` and `vmax` work anywhere a length is accepted (sizes, padding, gaps, `font-size`, ...):

```css
.hero { width: 80vw; height: 25vh; font-size: 3vh; }
```

### Resize Notification

Backends report viewport changes automatically. On a real size change the runtime updates the global viewport, calls `Model::on_resize`, and rebuilds the tree so media queries and `vw`/`vh` values re-resolve. `on_resize` also fires once with the initial size:

```rust
impl Model for MyModel {
    fn on_resize(&mut self, width: f32, height: f32, _ctx: &mut Context) {
        self.width = width as u32;
        self.height = height as u32;
    }
}
```

Overriding `on_resize` is only needed when the *structure* of the view changes; pure-CSS adaptation via `@media`/`vw` requires no model code at all.

### Breakpoint Helpers

- `xerune::screen::width()` / `height()` / `size()` — current viewport, readable anywhere (including template expressions like `{{ xerune::screen::width() }}`).
- `xerune::screen::breakpoint()` → `Breakpoint::Mobile` (`<600px`), `Tablet` (`<1024px`), `Desktop`, plus `is_mobile()` / `is_tablet()` / `is_desktop()`. Thresholds are plain constants (`screen::MOBILE_MAX`, `screen::TABLET_MAX`).
- `Context::screen_size()` / `Context::breakpoint()` for use inside `update`/`on_resize`.

---

## 5. Skipping Redundant Rebuilds (view fingerprint)

The classic MVU loop rebuilds the whole UI tree after every message. For embedded CPU savings a model may opt into **fingerprint gating** by overriding `Model::view_fingerprint`:

```rust
fn view_fingerprint(&self) -> Option<u64> {
    // hash everything the view actually renders (e.g. via `xerune::model::hash_bytes`)
    Some(xerune::model::hash_bytes(self.rendered_state.as_bytes()))
}
```

When two consecutive message-driven syncs see an unchanged fingerprint (and no viewport change happened), the runtime skips template re-instantiation, style resolution and text measurement — a big win for messages that only touch state the view does not display (bookkeeping, hidden timers). Canvas pixels remain safe: dirty canvas flags still force a redraw even when the rebuild is skipped, and viewport changes always rebuild.

The default implementation returns `None` (no fingerprinting) and keeps the always-rebuild behavior. **The runtime trusts your fingerprint completely** — if it misses state the view reads, the UI will go stale. Keep it a function of exactly the view-relevant fields.

---

## 6. Keyframe Animations

Animations are declared directly in CSS style blocks:

```html
<style>
    @keyframes pulse {
        0% { background-color: #ff0000; }
        50% { background-color: #00ff00; }
        100% { background-color: #ff0000; }
    }
    .pulsing-box {
        width: 100px;
        height: 100px;
        animation-name: pulse;
        animation-duration: 2s;
        animation-iteration-count: infinite;
    }
</style>
<div class="pulsing-box"></div>
```

Animatable properties: `color`, `background-color`, `border-color`, `border-width`, `border-radius`, `font-size`, `width`, `height`, `margin-*`, `padding-*`, `top`/`right`/`bottom`/`left`, and `background` gradients. Two gradients of the same kind with the same number of stops interpolate their angle, center, colors and stop positions, so a spinner is just:

```css
@keyframes spin {
    from { background: conic-gradient(from 0deg, transparent, #22d3ee); }
    to { background: conic-gradient(from 360deg, transparent, #22d3ee); }
}
```

See `examples/animation_css` for a gallery.

---

## 7. Headless Snapshots

Any app can render a single frame to a PNG instead of opening a window, which makes the UI easy to check from tests, CI or an AI agent. Every built-in backend honours these variables, so no code changes are needed:

```bash
XERUNE_SNAPSHOT=todo.png cargo run --example todo
# Reach a specific state, at a specific size, after 1.5 s of simulated time:
XERUNE_SNAPSHOT=todo.png \
XERUNE_SNAPSHOT_SIZE=800x480 \
XERUNE_SNAPSHOT_MESSAGES="remove:0;toggle:0" \
XERUNE_SNAPSHOT_TIME_MS=1500 \
cargo run --example todo
```

| Variable | Meaning |
|---|---|
| `XERUNE_SNAPSHOT` | Output PNG path; enables snapshot mode |
| `XERUNE_SNAPSHOT_SIZE` | `WIDTHxHEIGHT`; defaults to the app's window size |
| `XERUNE_SNAPSHOT_MESSAGES` | `;`-separated messages dispatched in order (same strings as `data-on-click`); an unknown message is an error |
| `XERUNE_SNAPSHOT_TIME_MS` | Simulated time to run before rendering, so timers fire and animations progress (default 0) |

The frame goes through the app's own renderer, so fonts and drawing match the real app. Pending images are awaited (up to 10 s). `App::on_start` is not run, since background producers would make the output nondeterministic; drive state with messages instead. To snapshot from code, use `xerune::backend::SnapshotBackend` directly.

---

## 8. Running on Embedded Linux (fbdev / DRM)

Configure `Cargo.toml` features to enable embedded backends:
```toml
[dependencies]
xerune = { version = "0.1", features = ["linuxfb", "drm", "evdev"] }
```

`App::run` (see section 1) uses DRM/KMS or the fbdev framebuffer
`/dev/fb0` outside a desktop session; `XERUNE_BACKEND=drm` or `=fbdev`
forces one. To run a specific backend directly, `App::run_on(BackendKind::Drm, renderer)`,
or `DrmBackend::new().run(...)` / `LinuxFbBackend::new().run(...)`.

The display's size comes from its mode. For a display mounted rotated,
export `XERUNE_ROTATION=90` (or 180, 270); fbdev also rotates a portrait
framebuffer by itself. Touch input is mapped to the rotated UI.

---

## 9. WebAssembly & Browser Deployment

Xerune applications can compile to WebAssembly to run directly in web browsers or inside **Xerune Studio** (a web-based live inspector).

### 1. Build WASM Bindings
```bash
cd studio
wasm-pack build --target web
```

### 2. Run Local Web Server
```bash
# From workspace root
python3 -m http.server 8000
```

### 3. Open in Browser
- **Studio with live state inspector**: `http://localhost:8000/studio/index.html`
- **Standalone full-screen web runner**: `http://localhost:8000/examples/standalone.html?app=music_player`

---

## 10. WPT Reftests

`wpt_runner` checks Xerune against the [web-platform-tests](https://github.com/web-platform-tests/wpt) reftests of the CSS features it implements. Each test is rendered by Xerune and compared pixel for pixel with Chromium's screenshot of the test's reference.

```bash
WPT_DIR=~/path/to/wpt cargo run --release -p wpt_runner             # all suites
WPT_DIR=~/path/to/wpt cargo run --release -p wpt_runner -- flexbox  # paths containing "flexbox"
WPT_DIR=~/path/to/wpt cargo run --release -p wpt_runner -- flexbox --images
WPT_DIR=~/path/to/wpt cargo run --release -p wpt_runner -- --update # record new results
```

Needs Node 22+ and `chromium` (or `$CHROMIUM`). How it works:

- **Scope**: `wpt_runner/suites.txt` lists the WPT directories that count. Grid, transforms, tables, floats, multi-column, writing modes and similar are left out.
- **Comparison**: images must be identical, unless the test's `<meta name=fuzzy>` allows a difference. At least one `match` reference must match and every `mismatch` reference must differ, as in WPT. The one exception is anti-aliasing, because two rasterizers never agree on it exactly (Chromium hints Ahem's glyph boxes onto the pixel grid, fontdue doesn't): a render also matches if every differing pixel is off by at most 64/255 and lies on an edge of the reference. Missing, moved or recolored content differs by far more. The summary's `(aa)` column counts these passes; `--images` saves them for inspection.
- **References**: rendered by Chromium, not Xerune, so a test can't pass by being wrong the same way as its reference. `wpt_runner/chromium_screenshots.mjs` serves `WPT_DIR` over HTTP and screenshots each file at 800x600. Screenshots are cached under `target/wpt/chromium/<wpt commit>-<chromium version>/` and taken again when either changes.
- **Font**: Ahem only, in both Xerune and Chromium (a private fontconfig maps every family to it). Every Ahem glyph is a box, so text compares exactly. Tests that then no longer match their own reference in Chromium are skipped ("fails in Chromium").
- **Skipped**: tests that need JavaScript, `reftest-wait`, iframes, SVG, canvas, video, animations or paging. They are reported but not part of the pass rate.
- **Expectations**: `wpt_runner/expectations.txt` lists the tests that fail; every other test that runs is expected to pass, and its header has the pass rate per suite. A run exits with an error when a test fails that isn't listed, and lists tests that newly pass. After an engine fix, run with `--update` and commit the file along with the fix.
- **Images**: `--images` writes Xerune's render, Chromium's reference and a diff for each failure to `target/wpt/images/failures/<test>/`, and for each anti-aliasing pass to `target/wpt/images/antialiasing/<test>/`.
