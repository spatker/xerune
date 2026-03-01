# Xerune

Xerune is a lightweight, CPU-only native HTML renderer designed for embedded Linux environments without GPU support.

## Demos

| Music Player | Showcase | Animation |
| :---: | :---: | :---: |
| ![Music Player](docs/img/music_player.gif) | ![Showcase](docs/img/showcase.gif) | ![Animation](docs/img/animation.gif) |

High quality videos: [Music Player](docs/img/music_player.mkv), [Showcase](docs/img/showcase.mkv), [Animation](docs/img/animation.mkv)

## Features

- **Compile time checked templates**: Uses [askama](https://github.com/djc/askama) for safe data bindings.
- **HTML support**: Renders standard HTML elements.
- **No GPU required**: Runs entirely on the CPU with decent performance.
- **Click handling**: Native support for interactive elements.
- **Custom callbacks**: Define logic for interactions.
- **Layout and text rendering**: Built-in support for complex layouts and text.

## Architecture

Xerune is built around the **Model-View-Update (MVU)** architecture (similar to Elm), designed for highly decoupled, efficient updates:

- **Model (`src/model.rs`)**: Owns the application state and the `Message` routing.
- **View (`Model::view`)**: Purely declarative. Takes the model and outputs raw HTML/CSS strings.
- **Update (`Model::update`)**: Mutates the state based on `Message` intents. 
- **UI & Layout Engine (`src/ui.rs`)**: Parses the raw HTML into a Taffy Flexbox tree and processes styling, isolating the view from the backend renderer.
- **Runtime (`src/runtime.rs`)**: The engine loop that ties MVU to the underlying event system.

This modular separation makes it very easy to create custom components simply by emitting predictable HTML, whilst keeping hardware-specific drawing instructions sequestered in different backends.

## Roadmap

- [x] Dirty region handling
- [x] Performance improvements
- [ ] ARM Linux as first class citizen
- [ ] Animations
- [x] Performance and resource optimization (CPU, RAM, storage)
- [ ] More CSS and HTML tags

## Dependencies

Xerune relies on a few key libraries to provide its functionality:

- **[askama](https://crates.io/crates/askama)**: Template rendering engine.
- **[taffy](https://crates.io/crates/taffy)**: Flexbox layout engine.
- **[html5ever](https://crates.io/crates/html5ever)**: HTML parsing.
- **[csscolorparser](https://crates.io/crates/csscolorparser)**: CSS color parsing.
- **[tiny-skia](https://crates.io/crates/tiny-skia)**: Software rendering.
- **[winit](https://crates.io/crates/winit)** & **[softbuffer](https://crates.io/crates/softbuffer)**: Window creation and buffer management (for desktop examples).

## Getting Started

Check out the `examples/` directory to see how to use the library.

### Running Examples

> **Note**: For best performance, please run all examples with the `--release` flag.

```bash
cargo run --release --example music_player
cargo run --release --example todo
cargo run --release --example showcase
```
