#[derive(Clone, Debug)]
pub struct Timer {
    pub id: usize,
    pub message: String,
    pub interval: std::time::Duration,
    pub next_trigger: std::time::Instant,
    pub is_recurring: bool,
}

#[derive(Clone, Debug)]
pub struct TickResult {
    pub needs_redraw: bool,
    pub next_tick_in: std::time::Duration,
}
