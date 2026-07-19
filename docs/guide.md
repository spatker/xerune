# Xerune Quick Start Guide

This guide walks you through building your first application using Xerune, styling it with CSS, using custom canvases, and deploying to embedded backends.

---

## 1. Minimal Application Setup

Ensure your `Cargo.toml` includes:
```toml
[dependencies]
xerune = { version = "0.1", features = ["winit"] } # Features: "linuxfb", "drm", "evdev", "std"
```

Create a new file `src/main.rs`:

```rust
use xerune::{Model, InputEvent, Runtime, Context, XeruneTemplate, backend::WinitBackend, backend::Backend};

// 1. Define application state
#[derive(Default)]
struct Counter {
    value: i32,
}

// 2. Define messages
#[derive(Debug)]
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

    fn update(&mut self, msg: Self::Message, _ctx: &mut Context) -> Option<Self::Message> {
        match msg {
            Message::Increment => self.value += 1,
            Message::Decrement => self.value -= 1,
        }
        None
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let model = Counter::default();
    
    // Create UI measures
    let font_regular = xerune::font::DEFAULT_ROBOTO_REGULAR;
    let font_bold = xerune::font::DEFAULT_ROBOTO_BOLD;
    let runtime = Runtime::new(model, font_regular, font_bold);
    
    // Run the desktop window backend
    let backend = WinitBackend::new();
    backend.run("Xerune Counter", 800, 600, runtime, |rt, buf, w, h| {
        // Direct blitting callback
        let canvases = rt.context.canvases();
        // Custom render loop calling direct rasterizer/renderer
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

## 4. Keyframe Animations

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

## 5. Running on Embedded Linux (fbdev / DRM)

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
