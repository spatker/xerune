#[cfg(not(target_arch = "wasm32"))]
/// Monotonic time representation using std::time::Instant.
pub type Instant = std::time::Instant;

#[cfg(all(target_arch = "wasm32", feature = "browser"))]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = Date, js_name = now)]
    fn date_now() -> f64;
}

/// Monotonic time in the browser, in milliseconds (`std::time::Instant`
/// panics on `wasm32-unknown-unknown`).
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instant(pub u64); // milliseconds since boot

#[cfg(target_arch = "wasm32")]
impl Instant {
    /// Return the current time: `Date.now()` with `browser`, else zero.
    pub fn now() -> Self {
        #[cfg(all(target_arch = "wasm32", feature = "browser"))]
        {
            Instant(date_now() as u64)
        }
        #[cfg(not(all(target_arch = "wasm32", feature = "browser")))]
        {
            Instant(0)
        }
    }

    /// Create an Instant from a millisecond timestamp.
    pub fn from_millis(ms: u64) -> Self {
        Instant(ms)
    }

    /// Return the raw millisecond count of the timestamp.
    pub fn as_millis(&self) -> u64 {
        self.0
    }

    /// Calculate the elapsed time duration since another timestamp.
    pub fn duration_since(&self, other: Self) -> core::time::Duration {
        core::time::Duration::from_millis(self.0.saturating_sub(other.0))
    }

    /// Calculate the elapsed time duration since another timestamp, saturating at zero.
    pub fn saturating_duration_since(&self, other: Self) -> core::time::Duration {
        if self.0 <= other.0 {
            core::time::Duration::ZERO
        } else {
            core::time::Duration::from_millis(self.0 - other.0)
        }
    }

    /// Calculate the elapsed duration since this timestamp, relative to start.
    pub fn elapsed(&self) -> core::time::Duration {
        Self::now().duration_since(*self)
    }
}

#[cfg(target_arch = "wasm32")]
impl core::ops::Add<core::time::Duration> for Instant {
    type Output = Self;

    fn add(self, rhs: core::time::Duration) -> Self {
        Instant(self.0 + rhs.as_millis() as u64)
    }
}

#[cfg(target_arch = "wasm32")]
impl core::ops::AddAssign<core::time::Duration> for Instant {
    fn add_assign(&mut self, rhs: core::time::Duration) {
        self.0 += rhs.as_millis() as u64;
    }
}

#[cfg(target_arch = "wasm32")]
impl core::ops::Sub<core::time::Duration> for Instant {
    type Output = Self;

    fn sub(self, rhs: core::time::Duration) -> Self {
        Instant(self.0.saturating_sub(rhs.as_millis() as u64))
    }
}

#[cfg(target_arch = "wasm32")]
impl core::ops::Sub<Instant> for Instant {
    type Output = core::time::Duration;

    fn sub(self, rhs: Instant) -> core::time::Duration {
        self.duration_since(rhs)
    }
}

/// Where the runtime reads the time from. Timer deadlines, animation and
/// transition start times and frame times are all read from it, so they
/// stay consistent whatever drives the runtime.
pub trait Clock: Send + 'static {
    /// The current time.
    fn now(&self) -> Instant;
}

/// The platform's monotonic clock: [`Instant::now`] (`Date.now()` in the
/// browser).
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// A clock given as a function returning the time.
impl<F: Fn() -> Instant + Send + 'static> Clock for F {
    fn now(&self) -> Instant {
        self()
    }
}

/// A clock that only moves when told to, for tests and headless snapshots.
/// Clones share the same time.
#[derive(Clone, Debug)]
pub struct ManualClock {
    start: Instant,
    elapsed_nanos: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl ManualClock {
    /// A clock standing at the current time.
    pub fn new() -> Self {
        Self { start: Instant::now(), elapsed_nanos: Default::default() }
    }

    /// Moves the clock on by `by`.
    pub fn advance(&self, by: core::time::Duration) {
        self.elapsed_nanos.fetch_add(by.as_nanos() as u64, std::sync::atomic::Ordering::Relaxed);
    }

    /// Moves the clock on to `time`, if that is later.
    pub fn advance_to(&self, time: Instant) {
        let target = time.saturating_duration_since(self.start).as_nanos() as u64;
        self.elapsed_nanos.fetch_max(target, std::sync::atomic::Ordering::Relaxed);
    }

    /// How far the clock has been moved on.
    pub fn elapsed(&self) -> core::time::Duration {
        core::time::Duration::from_nanos(self.elapsed_nanos.load(std::sync::atomic::Ordering::Relaxed))
    }
}

impl Default for ManualClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Instant {
        self.start + self.elapsed()
    }
}
