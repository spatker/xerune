# Xerune

Xerune is a lightweight, CPU-only native HTML renderer designed for embedded Linux environments without GPU support.

## Features

- **CPU-Only Logic**: Optimized for systems with no hardware acceleration.
- **Native HTML Rendering**: Renders HTML/CSS directly to a pixel buffer.
- **Jinja-style Templates**: Uses [Askama](https://github.com/djc/askama) for type-safe, compiled Jinja-like templates.
- **Data Bindings**: Supports reactive data updates.
- **Low Resource Impact**: Minimal footprint suitable for embedded devices.

## Getting Started

Check out the `examples/` directory to see how to use the library.

### Running Examples

```bash
cargo run --example music_player
cargo run --example todo
cargo run --example showcase
```
