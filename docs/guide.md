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

See [`examples/todo/templates/todo_list.html`](../examples/todo/templates/todo_list.html) for the full layout and CSS.

### 3. Running with a Backend

Each example includes a native runner supporting desktop (`WinitBackend`) and embedded Linux (`LinuxFbBackend`, `DrmBackend`).
See [`examples/todo/main.rs`](../examples/todo/main.rs) and [`examples/todo/todo_impl.rs`](../examples/todo/todo_impl.rs) for the runner setup:

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

---

## 4. Responsive Design

A single template adapts to any screen size using standard CSS building blocks, plus a small Rust-side API. See `examples/responsive` for a complete dashboard demo (resize the window to watch the layout switch).

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

---

## 7. Running on Embedded Linux (fbdev / DRM)

Configure `Cargo.toml` features to enable embedded backends:
```toml
[dependencies]
xerune = { version = "0.1", features = ["linuxfb", "drm", "evdev"] }
```

### Linux Framebuffer (`/dev/fb0`)
```rust
use xerune::backend::LinuxFbBackend;

let backend = LinuxFbBackend::new();
backend.run("Xerune App", 800, 480, runtime, render_callback, |_proxy| {})?;
```

### DRM/KMS (Direct Rendering Manager)
```rust
use xerune::backend::DrmBackend;

let backend = DrmBackend::new();
backend.run("Xerune App", 800, 480, runtime, render_callback, |_proxy| {})?;
```
To rotate the DRM output, export the `XERUNE_ROTATION` environment variable:
```bash
export XERUNE_ROTATION=90
```

---

## 8. WebAssembly & Browser Deployment

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
