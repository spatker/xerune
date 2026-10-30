# Xerune

Xerune is a lightweight, CPU-only native HTML/CSS rendering engine and UI framework designed for embedded Linux environments (like Raspberry Pi, QEMU, or custom board systems) without GPU/OpenGLES acceleration.

## Demos

| Showcase | Music player |
| :---: | :---: |
| ![Showcase: system, vehicle, monitor and controls tabs](docs/img/showcase.webp) | ![Music player: library, sliding player view, visualizer](docs/img/music_player.webp) |
| **Todo** | **CSS keyframes** |
| ![Todo: typing a task, toggling, English, German and Korean](docs/img/todo.webp) | ![CSS @keyframes gallery with no animation code in Rust](docs/img/animation_css.webp) |
| **Breakout** | **Animation stress test** |
| ![Breakout game](docs/img/breakout.webp) | ![100 moving elements rebuilt every frame](docs/img/animation.webp) |

Every example targets an 800x480 panel and has a portrait layout. The clips
are rendered headlessly by Xerune itself, frame by frame on a simulated clock
(`tools/record-demos.sh`), with the same renderer the boards use.

### In a game, through the C API

![A web shop drawn by Xerune on the screen of an in-game PC](docs/img/pc.webp)

A C program shows a web shop on an in-game PC. Xerune renders the page into
a texture and reports which part changed. The crosshair's hit on the screen
becomes pointer input, and Buy sends `buy:<id>` back to the program, which
updates its JSON model. The room is drawn by [raylib](https://www.raylib.com)
on the GPU; the screen's content is Xerune. Source:
[`xerune_capi/examples/raylib`](xerune_capi/examples/raylib/pc.c).

## Features

- **Model-View-Update (MVU) Architecture**: Elm-style design providing predictable state transitions and unidirectional data flow.
- **Compile-time template verification**: Native type-safe data binding and layout generation.
- **No GPU required**: High-performance CPU-only rendering.
- **Embedded hardware-ready**: Dedicated backends for Linux Framebuffer (`/dev/fb0`) and DRM/KMS with double buffering.
- **Input integration**: Built-in support for mouse, keyboard, and `evdev` touch input bounds calibration.
- **CSS Stylesheets, Animations & Transitions**: Parse inline styles, global styles, classes, IDs, gradients, borders, font weights, keyframe animations and transitions.
- **Text like Chromium**: Line breaking (UAX #14) checked against Chromium's breaks, `white-space`, `overflow-wrap`, `word-break`, `text-overflow: ellipsis`, `line-height`, and glyphs from fallback fonts (Hangul).
- **Interaction**: `:hover`, `:active`, `:checked`, form controls drawn like Chromium (`accent-color`, `appearance: none`, `::placeholder`), and element keys that keep animation and transition state across rebuilds.
- **On-screen keyboard**: A full-screen keyboard for touch or D-pad devices, with English, German and Korean (Hangul composition) layouts.
- **Translations**: Per-language string catalogs, with the language picked from the POSIX locale.
- **Responsive Layout**: `@media` queries (width/height/orientation), `vw`/`vh`/`vmin`/`vmax` viewport units, automatic `Model::on_resize` notification, and breakpoint helpers — compiled into cheap static guards for embedded targets.
- **Fingerprint Gating**: Opt-in `Model::view_fingerprint` lets the runtime skip full view rebuilds when a message changes no rendered state.
- **Canvas APIs**: Support for custom user-drawn Canvas pixel buffers.
- **C API**: Templates read at run time, models as JSON and messages as strings, drawn into the caller's pixel buffer; for C, C++, Zig and anything that calls C (guide §14).
- **Headless snapshots and recordings**: Any app renders to PNGs instead of a window (`XERUNE_SNAPSHOT`), driven by scripted messages, for tests, CI and AI agents.

## Documentation
- [Quick Start Guide](docs/guide.md)
- [Architecture Overview](docs/architecture.md)
- [AI Assistant Context (CLAUDE.md)](CLAUDE.md)

## Architecture

Xerune is built around the **Model-View-Update (MVU)** pattern:
- **Model (`src/model.rs`)**: Owns the application state.
- **View (`Model::view`)**: Takes the model and outputs declarative UI templates.
- **Update (`Model::update`)**: Mutates the model state based on `Message` intents.
- **UI Layout & Style Engine (`src/ui/`)**: Resolves HTML elements and CSS properties onto a Taffy Flexbox tree and generates `DrawCommand`s.
- **Runtime (`src/runtime/`)**: Runs updates, timers, subscriptions and frames (`Runtime::poll`), and animations.

## Dependencies

- **[taffy](https://crates.io/crates/taffy)**: Flexbox & grid layout engine.
- **[html5ever](https://crates.io/crates/html5ever)**: HTML parsing.
- **[winit](https://crates.io/crates/winit)** & **[softbuffer](https://crates.io/crates/softbuffer)**: Desktop windowing backend.
- **[evdev](https://crates.io/crates/evdev)**: Linux touch & key input handling.
- **[drm](https://crates.io/crates/drm)**: Linux Direct Rendering Manager control.

## Getting Started

### Minimal Example

A counter, complete. Templates are compiled into the program: their CSS
selectors are matched and their `data-on-click` messages checked against the
message enum when it builds. `Cargo.toml`:

```toml
[dependencies]
xerune = "0.1"
xerune_render = "0.1"
```

`templates/counter.html` (paths are from the crate root):

```html
<style>
    body { margin: 0; }
    .app { display: flex; flex-direction: column; align-items: center; justify-content: center;
           gap: 16px; width: 100vw; height: 100vh; background-color: #1e1b2e; color: #ffffff; }
    .count { font-size: 64px; font-weight: bold; }
    .row { display: flex; gap: 12px; }
    .button { padding: 12px 28px; border-radius: 10px; font-size: 24px; background-color: #7c3aed; }
    .button:active { background-color: #5b21b6; }
</style>
<div class="app">
    <div class="count">{{ count }}</div>
    <div class="row">
        <div class="button" data-on-click="add:-1">-</div>
        <div class="button" data-on-click="add:1">+</div>
        <div class="button" data-on-click="reset">Reset</div>
    </div>
</div>
```

`src/main.rs`:

```rust
use xerune::{Context, Model, XeruneMessage, XeruneTemplate};

// The state, and the template that shows it (a path from the crate root).
#[derive(Default, XeruneTemplate)]
#[template(path = "templates/counter.html")]
struct Counter {
    count: i64,
}

// What the template's `data-on-click` strings turn into: `add:1` is
// `Msg::Add(1)`, checked against this enum at compile time.
#[derive(Debug, Clone, XeruneMessage)]
enum Msg {
    Add(i64),
    Reset,
}

impl Model for Counter {
    type Message = Msg;

    fn update(&mut self, msg: Msg, _ctx: &mut Context<Msg>) {
        match msg {
            Msg::Add(n) => self.count += n,
            Msg::Reset => self.count = 0,
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The crates embed no fonts: bring your own TTF/OTF (regular, optional bold).
    let fonts = xerune_render::Fonts::ttf(
        include_bytes!("../fonts/Roboto-Regular.ttf"),
        Some(include_bytes!("../fonts/Roboto-Bold.ttf")),
    )?;
    let runtime = xerune::Runtime::new(Counter::default(), fonts.clone());
    // A window on the desktop; DRM/KMS or /dev/fb0 on a board (features `drm`, `linuxfb`).
    xerune::App::new(runtime).title("Counter").run(xerune_render::SoftwareRenderer::new(fonts))?;
    Ok(())
}
```

The crates embed no fonts; this one loads Roboto from `fonts/` (take it from
[`resources/fonts`](resources/fonts), or any TTF/OTF). `cargo run` opens a
window; `XERUNE_SNAPSHOT=out.png cargo run` writes a PNG instead. On a board,
turn on `drm`, `linuxfb` and `evdev` (guide §11). A larger app with lists,
text input and translations is the [Todo example](examples/todo/); the
[guide](docs/guide.md) covers the rest.

### Running Examples

> **Note**: For best performance, please run all native examples with the `--release` flag.

```bash
cargo run --release --example todo
cargo run --release --example music_player
cargo run --release --example showcase       # resize the window for the portrait layout
cargo run --release --example animation_css
cargo run --release --example breakout       # arrow keys
cargo run --release --example animation
cargo run --release --example calculator
```

### Running in WebBrowser (WASM & Xerune Studio)

Compile the WebAssembly studio package:

```bash
cd studio
wasm-pack build --target web
cd ..
```

Serve the repository with Python's HTTP server:

```bash
python3 -m http.server 8000
```

Open your browser to:
- **Xerune Studio (Inspector & Controls)**: [http://localhost:8000/studio/index.html](http://localhost:8000/studio/index.html)
- **Standalone Web Runner**: [http://localhost:8000/examples/standalone.html?app=music_player](http://localhost:8000/examples/standalone.html?app=music_player)  
  *(Available apps: `music_player`, `todo`, `calculator`, `breakout`, `animation`, `animation_css`, `showcase`)*

The page interprets the templates from disk: edit one and save, and the view
updates within a second, without rebuilding the wasm (see `docs/guide.md`, section 12).

## License

The code is licensed under the MIT License (`LICENSE`).

The fonts are under the SIL Open Font License 1.1, which lets you bundle and
embed them in any software, closed-source and commercial included, and on
locked-down devices; a product that ships one ships its copyright notice and
license (e.g. on a third-party licenses screen), and the font can't be sold on
its own:

| Font | Where | License |
|---|---|---|
| Roboto Regular/Bold | the examples, tests | `resources/fonts/Roboto-LICENSE.txt` |
| Xerune Hangul (Source Han Sans KR subset) | the examples (on 32-bit ARM with `--features hangul`) | `resources/fonts/XeruneHangul-LICENSE.txt` |
| Noto Color Emoji | the examples with `--features emoji` | `resources/fonts/NotoColorEmoji-LICENSE.txt` |

The crates embed no fonts: an app ships the fonts it loads.
