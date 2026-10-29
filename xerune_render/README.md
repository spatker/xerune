# xerune_render

Software rasterizer for [xerune](https://crates.io/crates/xerune): draws
its draw commands (rects, rounded corners, borders, box shadows, linear,
radial and conic gradients, images and text) into a pixel buffer on the
CPU, with SSE2 and NEON fast paths. Made for boards without a GPU.

`SoftwareRenderer` is the `xerune::Renderer` and owns the glyph and
image caches; `Fonts` (TrueType/OpenType, shaped by harfrust and
rasterized by fontdue, or bitmap fonts without `std`) is both its glyph source and the runtime's
`xerune::TextMeasurer`, so text is measured and drawn with the same
fonts:

```rust
let regular = std::fs::read("Roboto-Regular.ttf").unwrap();
let fonts = xerune_render::Fonts::ttf(&regular, None).expect("a font");
let renderer = xerune_render::SoftwareRenderer::new(fonts.clone());
// xerune::Runtime::new(model, fonts) ... xerune::App::new(runtime).run(renderer)
```

Without the default `std` feature it builds for `no_std` targets with
bitmap fonts only (`Fonts::bitmap`).

MIT licensed. See the [xerune repository](https://github.com/spatker/xerune) for the engine, examples and docs.
