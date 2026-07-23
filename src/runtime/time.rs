#[cfg(all(feature = "std", not(target_arch = "wasm32")))]
/// Monotonic time representation using std::time::Instant.
pub type Instant = std::time::Instant;

#[cfg(all(target_arch = "wasm32", feature = "browser"))]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = Date, js_name = now)]
    fn date_now() -> f64;
}

/// Monotonic time representation for `no_std` and WASM environments representing milliseconds.
#[cfg(any(not(feature = "std"), target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instant(pub u64); // milliseconds since boot

#[cfg(any(not(feature = "std"), target_arch = "wasm32"))]
impl Instant {
    /// Return the current time. Under no_std or WASM, this defaults to a zero timestamp.
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

#[cfg(any(not(feature = "std"), target_arch = "wasm32"))]
impl core::ops::Add<core::time::Duration> for Instant {
    type Output = Self;

    fn add(self, rhs: core::time::Duration) -> Self {
        Instant(self.0 + rhs.as_millis() as u64)
    }
}

#[cfg(any(not(feature = "std"), target_arch = "wasm32"))]
impl core::ops::AddAssign<core::time::Duration> for Instant {
    fn add_assign(&mut self, rhs: core::time::Duration) {
        self.0 += rhs.as_millis() as u64;
    }
}

#[cfg(any(not(feature = "std"), target_arch = "wasm32"))]
impl core::ops::Sub<core::time::Duration> for Instant {
    type Output = Self;

    fn sub(self, rhs: core::time::Duration) -> Self {
        Instant(self.0.saturating_sub(rhs.as_millis() as u64))
    }
}

#[cfg(any(not(feature = "std"), target_arch = "wasm32"))]
impl core::ops::Sub<Instant> for Instant {
    type Output = core::time::Duration;

    fn sub(self, rhs: Instant) -> core::time::Duration {
        self.duration_since(rhs)
    }
}
