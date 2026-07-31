//! Xerune is a lightweight, CPU-only native HTML/CSS rendering library designed
//! for embedded Linux environments, following the Model-View-Update (MVU) pattern.

#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]

#[cfg(not(feature = "std"))]
extern crate alloc;

/// Internal prelude for allocator-agnostic collections and types in `no_std` environments.
pub mod alloc_prelude {
    #[cfg(not(feature = "std"))]
    pub use alloc::{
        string::{String, ToString},
        vec::Vec,
        boxed::Box,
        sync::Arc,
        borrow::Cow,
        rc::Rc,
        format,
        vec,
    };
    #[cfg(not(feature = "std"))]
    pub use hashbrown::{HashMap, HashSet};

    #[cfg(feature = "std")]
    pub use std::{
        string::{String, ToString},
        vec::Vec,
        boxed::Box,
        sync::Arc,
        borrow::Cow,
        rc::Rc,
        collections::{HashMap, HashSet},
        format,
        vec,
    };
}

/// Graphics types, color definitions, draw commands, and canvas abstractions.
pub mod graphics;
/// Layout and styling properties, including the core styling system.
pub mod style;
/// Application state abstractions for MVU (Model-View-Update).
pub mod model;
/// UI engine, document tree creation, layout builders, and style resolvers.
pub mod ui;
/// Elm/MVU runtime engine that coordinates input events, updates, and rendering.
pub mod runtime;

/// CSS parsers and animation utilities.
pub mod css;
/// Default styles for standard HTML element tags.
pub mod defaults;
/// Font rasterizer representation and precompiled bitmap fonts.
pub mod font;

/// System-specific backends for windowing and hardware displays.
#[cfg(any(feature = "winit", feature = "linuxfb", feature = "drm", feature = "browser"))]
pub mod backend;

pub use graphics::{Color, LinearGradient, Rect, Canvas, Context, DrawCommand, TextMeasurer, Renderer};
pub use style::{Overflow, ContainerStyle, RenderData, Display, TextAlign, Direction, WritingMode, FlexDirection, FlexWrap, AlignContent, AlignItems, CssJustifyContent, Position, BoxSizing};
pub use model::{Model, InputEvent, XeruneMessage, NoMessage};
pub use ui::{Interaction, Ui, TemplateLayout, UiBuilder};
pub use runtime::Runtime;
pub use font::{BitmapGlyph, BitmapFont};
pub use xerune_derive::{XeruneTemplate, XeruneMessage};

