use crate::alloc_prelude::String;
use crate::runtime::time::Instant;
use core::time::Duration;

/// Representation of an active Timer in the runtime.
#[derive(Clone, Debug)]
pub struct Timer {
    /// Unique identifier for the timer.
    pub id: usize,
    /// Message string dispatched when the timer triggers.
    pub message: String,
    /// Delay duration interval of the timer.
    pub interval: Duration,
    /// Next expected trigger timestamp.
    pub next_trigger: Instant,
    /// Flags if this timer repeats recurringly or executes once.
    pub is_recurring: bool,
}

/// The result returned from runtime tick executions indicating state adjustments.
#[derive(Clone, Debug)]
pub struct TickResult {
    /// Flags if updates require a canvas redraw.
    pub needs_redraw: bool,
    /// Time remaining until the next animation/timer tick should trigger.
    pub next_tick_in: Duration,
}
