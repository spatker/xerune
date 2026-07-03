#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;

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

pub mod graphics;
pub mod style;
pub mod model;
pub mod ui;
pub mod runtime;

pub mod css;
pub mod defaults;

pub use graphics::{Color, LinearGradient, Rect, Canvas, Context, DrawCommand, TextMeasurer, Renderer};
pub use style::{Overflow, ContainerStyle, RenderData, Display, TextAlign, Direction, WritingMode, FlexDirection, FlexWrap, AlignContent, AlignItems, MyJustifyContent, Position, BoxSizing};
pub use model::{Model, InputEvent};
pub use ui::{Interaction, Ui, TemplateLayout, UiBuilder};
pub use runtime::Runtime;
pub use xerune_derive::XeruneTemplate;
