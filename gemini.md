# Xerune Architecture Guide

This file provides context about the internal workings of the Xerune library for developers and AI assistants.

## The MVU Paradigm

Xerune follows a strict Model-View-Update (Elm-style) architecture:
1. The developer defines a state structure implementing the `Model` trait.
2. The user interacts causing an `InputEvent` (such as click, touch, keyboard key, scroll, hover, or custom message).
3. The `Runtime` coordinates the inputs, updates the model state by calling `model.update(msg, &mut context)`, and triggers visual redraws. If the model overrides `Model::view_fingerprint` (returns `Some(..)`) and the fingerprint is unchanged with no viewport resize, the expensive DOM/style rebuild is skipped.
4. The template is defined via HTML/CSS structures parsed via `XeruneTemplate` procedural macro generating builder commands.
5. If the layout structure changes or animations trigger, the layout is resolved onto a Taffy tree which computes relative bounds.
6. The resolved Taffy layout tree and resolved styling bounds are walked to produce absolute screen-space `DrawCommand`s.

## Important Concepts
- **Taffy**: The underlying layout engine. CSS attributes are parsed and translated to Taffy's flexbox/grid layout inputs.
- **ContainerStyle**: The structure holding CSS styling properties like background color, gradients, borders, font configuration, animations, etc.
- **NodeMetadata**: Holds HTML-level attributes and states of processed nodes (tag name, class, id, checked/value state, children, etc.).
- **DrawCommand**: Hardware-agnostic drawing primitives (rectangles, text, images, checkboxes, sliders, progress bars, custom canvas viewports).
- **Renderer**: The trait implementing graphic rendering and text measurement. The workspace contains the native `fast_renderer` to draw these commands directly to pixel buffers.

- **Backends**: System/display integration layers (e.g., `WinitBackend` using softbuffer, or direct Linux hardware access via `LinuxFbBackend` and `DrmBackend` using evdev).

- **Screen / Viewport**: `screen.rs` holds the process-global viewport size (two atomics) plus `Breakpoint` helpers. The runtime keeps it in sync on resize (`Runtime::set_size` → `Model::on_resize` → rebuild). `css::media` matches `@media` queries against it, and `css::parse_px` resolves `vw`/`vh`/`vmin`/`vmax` from it.

## Module Responsibilities
- `graphics.rs`: Abstractions over drawing instructions. Contains `Color`, `Canvas`, `Context`, `DrawCommand`, `TextMeasurer`, and `Renderer`.
- `style.rs`: Holds formatting and layout types. Contains `ContainerStyle`, `CssJustifyContent`, `Display`, `Overflow`, etc.
- `ui/`: Processes HTML elements and styling. Contains `UiBuilder` to programmatically build trees, `attributes` to map attributes, and `style_resolution` to parse and override stylesheet styles.
- `model.rs`: Holds MVU user-space trait abstractions (`Model`, `InputEvent`) and the dependency-free `hash_bytes` FNV helper for `view_fingerprint`.
- `screen.rs`: Global viewport size and semantic breakpoint (`Mobile`/`Tablet`/`Desktop`) helpers, `no_std`-safe via atomics.
- `runtime/`: The orchestration engine managing ticks, event processing, frame pacing, custom message passing, and fingerprint-gated rebuilds.
- `backend/`: Platform-specific display and input event loops (Winit, Linux Framebuffer, DRM/KMS).
- `css/`: CSS parser logic for hex colors, dimensions (px/%/vw/vh/vmin/vmax), layout properties, keyframe animations, and `css::media` (`@media` split/parse/match/expand).
