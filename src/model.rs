use crate::graphics::Context;
use crate::alloc_prelude::*;

/// Trait defining an application Model in the Model-View-Update (Elm) architecture.
pub trait Model {
    /// The message type processed by this model. Must implement `FromStr` to allow stringified HTML interaction mappings.
    type Message: core::str::FromStr + Send + Sync + 'static;

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
