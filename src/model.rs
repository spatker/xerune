use crate::graphics::Context;
use crate::alloc_prelude::*;

/// Trait defining compile-time validated message parsing and validation capabilities.
pub trait XeruneMessage: core::str::FromStr {
    /// List of valid message prefixes.
    const VALID_PREFIXES: &'static [&'static str];
    /// List of valid exact match message strings.
    const VALID_EXACT: &'static [&'static str];
}

/// Trait defining an application Model in the Model-View-Update (Elm) architecture.
pub trait Model {
    /// The message type processed by this model. Must implement `XeruneMessage` to allow compile-time verified HTML interaction mappings.
    type Message: XeruneMessage + Send + Sync + 'static;

    /// Produces the declarative HTML view structure of the model.
    fn view(&self) -> String {
        String::new()
    }

    /// Mutates the application state in response to an incoming message.
    fn update(&mut self, msg: Self::Message, context: &mut Context);
}

/// Events representing user interactions or inputs forwarded to the UI runtime.
pub enum InputEvent {
    /// Click/tap gesture at coordinates.
    Click {
        /// The x coordinate.
        x: f32,
        /// The y coordinate.
        y: f32,
    },
    /// Pointer hover at coordinates.
    Hover {
        /// The x coordinate.
        x: f32,
        /// The y coordinate.
        y: f32,
    },
    /// Scroll offset event.
    Scroll {
        /// The x coordinate where scrolling started.
        x: f32,
        /// The y coordinate where scrolling started.
        y: f32,
        /// Scroll delta along x-axis.
        delta_x: f32,
        /// Scroll delta along y-axis.
        delta_y: f32,
    },
    /// Key down event with key identifier.
    KeyDown(String),
    /// Key up event with key identifier.
    KeyUp(String),
    /// Direct stringified message event.
    Message(String),
    /// Text input event for text fields.
    TextInput {
        /// The input element ID.
        id: String,
        /// The new text value.
        text: String,
    },
    /// Touch start gesture event.
    TouchStart {
        /// The touch pointer unique identifier.
        id: u64,
        /// The x coordinate.
        x: f32,
        /// The y coordinate.
        y: f32,
    },
    /// Touch move gesture event.
    TouchMove {
        /// The touch pointer unique identifier.
        id: u64,
        /// The x coordinate.
        x: f32,
        /// The y coordinate.
        y: f32,
    },
    /// Touch end gesture event.
    TouchEnd {
        /// The touch pointer unique identifier.
        id: u64,
        /// The x coordinate.
        x: f32,
        /// The y coordinate.
        y: f32,
    },
    /// Touch cancel gesture event.
    TouchCancel {
        /// The touch pointer unique identifier.
        id: u64,
        /// The x coordinate.
        x: f32,
        /// The y coordinate.
        y: f32,
    },
}

impl XeruneMessage for String {
    const VALID_PREFIXES: &'static [&'static str] = &[];
    const VALID_EXACT: &'static [&'static str] = &[];
}

/// A no-op message type for models that do not process any interaction messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoMessage {}

impl core::str::FromStr for NoMessage {
    type Err = ();
    fn from_str(_s: &str) -> Result<Self, Self::Err> {
        Err(())
    }
}

impl XeruneMessage for NoMessage {
    const VALID_PREFIXES: &'static [&'static str] = &[];
    const VALID_EXACT: &'static [&'static str] = &[];
}
