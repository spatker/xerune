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

    /// Called when the rendering viewport changes size (window resize, display
    /// mode switch) and once with the initial size when the backend first
    /// reports it. The default implementation is a no-op.
    ///
    /// Models that adapt their layout structure to screen size should store
    /// the new dimensions here; plain CSS adaptation via `@media` and `vw`/`vh`
    /// units does not require overriding this method.
    fn on_resize(&mut self, _width: f32, _height: f32, _context: &mut Context) {}

    /// Optional content fingerprint used to skip redundant view rebuilds.
    ///
    /// By default this returns `None`, meaning the runtime rebuilds the full
    /// UI tree after every message that mutated the model (classic Elm/MVU
    /// behaviour, always correct).
    ///
    /// A model may override it to return a hash of **all state that its view
    /// reads**. When two consecutive rebuilds are requested while the fingerprint
    /// is unchanged (and no viewport change occurred in between), the runtime
    /// skips the expensive template re-instantiation, layout resolution and text
    /// measurement — a meaningful CPU saving on embedded targets for messages
    /// that only touch state the view does not render (e.g. bookkeeping updated
    /// on hover, or animation clock fields that live in `Context`).
    ///
    /// ```text
    /// fn view_fingerprint(&self) -> Option<u64> {
    ///     // `hash_bytes` is a small FNV-1a helper exported by xerune.
    ///     let mut buf = [0u8; 32]; // or serialize the relevant fields
    ///     // ... fill `buf` from view-relevant state ...
    ///     Some(xerune::model::hash_bytes(&buf))
    /// }
    /// ```
    ///
    /// # Correctness
    /// The runtime trusts this value completely: if it is stale (the view reads
    /// state not covered by the fingerprint) the UI will **not** update. Keep the
    /// fingerprint a function of exactly the fields the view depends on. Canvas
    /// pixels drawn into `Context` are covered separately (dirty flags still force
    /// a redraw even when the fingerprint is unchanged).
    fn view_fingerprint(&self) -> Option<u64> {
        None
    }
}

/// A tiny, dependency-free FNV-1a 64-bit hasher, usable in `no_std`, intended to
/// help models implement [`Model::view_fingerprint`].
pub fn hash_bytes(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(PRIME);
    }
    hash
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
