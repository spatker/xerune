use crate::alloc_prelude::String;
use crate::runtime::time::Instant;
use core::time::Duration;

#[derive(Clone, Debug)]
pub struct Timer {
    pub id: usize,
    pub message: String,
    pub interval: Duration,
    pub next_trigger: Instant,
    pub is_recurring: bool,
}

#[derive(Clone, Debug)]
pub struct TickResult {
    pub needs_redraw: bool,
    pub next_tick_in: Duration,
}
