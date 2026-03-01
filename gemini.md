# Xerune Architecture Guide

This file provides context about the internal workings of the Xerune library.

## The MVU Paradigm

Xerune follows a strict Model-View-Update (Elm) architecture.
1. The developer defines a structure implementing the `Model` trait.
2. The user interacts causing an `InputEvent`.
3. The `Runtime` hit-tests the Flexbox tree to produce a stringified `Message`.
4. The string is parsed into the user's explicit enum `Message`.
5. `model.update(msg)` mutates the state.
6. `model.view()` produces a new raw HTML string.
7. Only if the HTML string differs from exactly the previous frame, the `Ui` module destroys the previous DOM layout and rebuilds a brand new Taffy DOM tree.

## Important Concepts
- **Taffy**: The layout engine. We map `html5ever` DOM nodes directly into `Taffy` `NodeId`s.
- **RenderData**: The styling attached to a given `NodeId`.
- **DrawCommand**: Hardware agnostic primitives (rects, text, gradients). You process the Taffy tree (walking node by node) and convert relative coordinate layouts into absolute display coordinates packed into `DrawCommand`s.
- **TinySkia Backend**: The standard software renderer implementation that interprets `DrawCommand`s.

## Module Responsibilities
- `graphics.rs`: Abstractions over drawing instructions. Contains `Color`, `Canvas`, and `DrawCommand`.
- `style.rs`: Abstractions over layout instructions. Contains `RenderData` and `ContainerStyle`. 
- `ui.rs`: Parses HTML. Creates Taffy nodes. Converts Taffy layouts into global coordinates inside `DrawCommand` lists. Provides hit-testing and scrolling coordinate handling.
- `model.rs`: Holds purely user-space trait abstractions (`Model`, `InputEvent`).
- `runtime.rs`: The state machine connecting user-space MVU with the engine's `Renderer`.
