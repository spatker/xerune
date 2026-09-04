# Xerune Quick Start Guide

This guide walks you through building your first application using Xerune, styling it with CSS, using custom canvases, and deploying to embedded backends.

---

## 1. Minimal Application Setup

Ensure your `Cargo.toml` includes:
```toml
[dependencies]
xerune = { version = "0.1", features = ["winit"] } # Features: "linuxfb", "drm", "evdev", "std"
fast_renderer = { version = "0.1" }
```

Create a new file `src/main.rs`:

```rust
use xerune::{Model, InputEvent, Runtime, Context, XeruneTemplate, XeruneMessage, backend::WinitBackend, backend::Backend};
use fast_renderer::{FastMeasurer, FastRenderer};

// 1. Define application state
#[derive(Default)]
struct Counter {
    value: i32,
}

// 2. Define messages
#[derive(Debug, Clone, XeruneMessage)]
enum Message {
    Increment,
    Decrement,
}

// 3. Define layout and logic
#[derive(XeruneTemplate)]
#[template(source = r#"
    <div style="flex-direction: column; align-items: center; justify-content: center; width: 100%; height: 100%; background-color: #222222; color: #ffffff;">
        <span style="font-size: 32px; font-weight: bold; margin-bottom: 20px;">
            Value: {{ value }}
        </span>
        <div style="flex-direction: row;">
            <button onclick="Decrement" style="background-color: #ff3b30; padding: 10px 20px; border-radius: 5px; margin-right: 10px;">-</button>
            <button onclick="Increment" style="background-color: #34c759; padding: 10px 20px; border-radius: 5px;">+</button>
        </div>
    </div>
"#, ext = "html")]
impl Model for Counter {
    type Message = Message;

    fn update(&mut self, msg: Self::Message, _ctx: &mut Context) {
        match msg {
            Message::Increment => self.value += 1,
            Message::Decrement => self.value -= 1,
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let model = Counter::default();
    
    // Create UI measures using the default precompiled bitmap fonts
    let fonts = vec![xerune::font::DEFAULT_ROBOTO_REGULAR, xerune::font::DEFAULT_ROBOTO_BOLD];
    let measurer = FastMeasurer { fonts: (&fonts).into() };
    let runtime = Runtime::new(model, measurer);
    
    // Setup rendering caches
    let mut image_cache = std::collections::HashMap::new();
    let mut glyph_cache = std::collections::HashMap::new();

    // Run the desktop window backend
    let backend = WinitBackend::new();
    backend.run("Xerune Counter", 800, 600, runtime, move |rt, buf, w, h| {
        let mut renderer = FastRenderer::new(
            buf,
            w,
            h,
            &fonts,
            &mut image_cache,
            &mut glyph_cache,
        );
        rt.render(&mut renderer);
    }, |_proxy| {})?;
    
    Ok(())
}
```

---

## 2. Using Custom Canvas Buffers

Custom canvas allows you to directly manipulate raw pixel colors:

```rust
// 1. Declare <canvas id="my_canvas"> in HTML template
// 2. Access and draw onto the canvas inside update loop:
fn update(&mut self, msg: Self::Message, ctx: &mut Context) -> Option<Self::Message> {
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
    None
}
```

---

## 3. Styling with CSS Stylesheets

You can supply global stylesheets to style elements by class, ID, or tag:

```rust
#[derive(XeruneTemplate)]
#[template(source = r#"
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
"#, ext = "html")]
impl Model for MyModel { ... }
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
