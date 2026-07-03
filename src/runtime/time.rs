#[cfg(feature = "std")]
pub type Instant = std::time::Instant;

#[cfg(not(feature = "std"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instant(pub u64); // milliseconds since boot

#[cfg(not(feature = "std"))]
impl Instant {
    pub fn now() -> Self {
        // Under no_std, there is no real-time clock source by default.
        // The user must drive time by calling Runtime::tick_at_ms or tick_with_time.
        Instant(0)
    }

    pub fn from_millis(ms: u64) -> Self {
        Instant(ms)
    }

    pub fn as_millis(&self) -> u64 {
        self.0
    }

    pub fn duration_since(&self, other: Self) -> core::time::Duration {
        core::time::Duration::from_millis(self.0.saturating_sub(other.0))
    }

    pub fn saturating_duration_since(&self, other: Self) -> core::time::Duration {
        if self.0 <= other.0 {
            core::time::Duration::ZERO
        } else {
            core::time::Duration::from_millis(self.0 - other.0)
        }
    }

    pub fn elapsed(&self) -> core::time::Duration {
        core::time::Duration::from_millis(self.0)
    }
}

#[cfg(not(feature = "std"))]
impl core::ops::Add<core::time::Duration> for Instant {
    type Output = Self;

    fn add(self, rhs: core::time::Duration) -> Self {
        Instant(self.0 + rhs.as_millis() as u64)
    }
}

#[cfg(not(feature = "std"))]
impl core::ops::AddAssign<core::time::Duration> for Instant {
    fn add_assign(&mut self, rhs: core::time::Duration) {
        self.0 += rhs.as_millis() as u64;
    }
}

#[cfg(not(feature = "std"))]
impl core::ops::Sub<core::time::Duration> for Instant {
    type Output = Self;

    fn sub(self, rhs: core::time::Duration) -> Self {
        Instant(self.0.saturating_sub(rhs.as_millis() as u64))
    }
}

#[cfg(not(feature = "std"))]
impl core::ops::Sub<Instant> for Instant {
    type Output = core::time::Duration;

    fn sub(self, rhs: Instant) -> core::time::Duration {
        self.duration_since(rhs)
    }
}
