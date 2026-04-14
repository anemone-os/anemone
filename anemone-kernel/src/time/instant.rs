use crate::{
    prelude::*,
    time::timekeeper::{self, monotonic_uptime},
};

use core::ops::{Add, AddAssign, Sub, SubAssign};

use super::timekeeper::{duration_from_mono, duration_to_mono};

/// Under no-std, Rust's core library doesn't have a built-in `Instant` type, cz
/// it heavily relies on existing OS time APIs. We define our own `Instant` type
/// here, with our own timekeeping support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instant {
    mono: u64,
}

impl Instant {
    pub const ZERO: Self = Self { mono: 0 };

    /// Returns an instant corresponding to "now" on the kernel monotonic
    /// timeline.
    pub fn now() -> Self {
        Self {
            mono: monotonic_uptime(),
        }
    }

    /// Converts this instant to the number of ticks since boot, rounding down.
    pub fn to_ticks(self) -> u64 {
        self.mono / timekeeper::mono_per_tick()
    }

    /// Converts this instant to a `Duration` **since boot**.
    pub fn to_duration(self) -> Duration {
        duration_from_mono(self.mono)
    }

    pub(crate) const fn from_mono(mono: u64) -> Self {
        Self { mono }
    }

    pub(crate) const fn mono(&self) -> u64 {
        self.mono
    }

    /// Returns the amount of time elapsed from `earlier` to `self`.
    ///
    /// Panics if `earlier` is later than `self`.
    pub fn duration_since(&self, earlier: Self) -> Duration {
        self.checked_duration_since(earlier)
            .expect("supplied instant is later than self")
    }

    /// Returns the amount of time elapsed from `earlier` to `self`, or `None`
    /// if `earlier` is later than `self`.
    pub fn checked_duration_since(&self, earlier: Self) -> Option<Duration> {
        self.mono.checked_sub(earlier.mono).map(duration_from_mono)
    }

    /// Returns the amount of time elapsed from `earlier` to `self`, saturating
    /// at zero if `earlier` is later than `self`.
    pub fn saturating_duration_since(&self, earlier: Self) -> Duration {
        self.checked_duration_since(earlier)
            .unwrap_or(Duration::ZERO)
    }

    /// Returns the amount of time elapsed since this instant was created.
    pub fn elapsed(&self) -> Duration {
        Self::now().saturating_duration_since(*self)
    }

    /// Returns `self + duration` if it can be represented.
    pub fn checked_add(&self, duration: Duration) -> Option<Self> {
        let delta = duration_to_mono(duration)?;
        self.mono.checked_add(delta).map(Self::from_mono)
    }

    /// Returns `self - duration` if it can be represented.
    pub fn checked_sub(&self, duration: Duration) -> Option<Self> {
        let delta = duration_to_mono(duration)?;
        self.mono.checked_sub(delta).map(Self::from_mono)
    }
}

impl Add<Duration> for Instant {
    type Output = Self;

    fn add(self, rhs: Duration) -> Self::Output {
        self.checked_add(rhs)
            .expect("overflow when adding duration to instant")
    }
}

impl AddAssign<Duration> for Instant {
    fn add_assign(&mut self, rhs: Duration) {
        *self = *self + rhs;
    }
}

impl Sub<Duration> for Instant {
    type Output = Self;

    fn sub(self, rhs: Duration) -> Self::Output {
        self.checked_sub(rhs)
            .expect("overflow when subtracting duration from instant")
    }
}

impl SubAssign<Duration> for Instant {
    fn sub_assign(&mut self, rhs: Duration) {
        *self = *self - rhs;
    }
}

impl Sub<Instant> for Instant {
    type Output = Duration;

    fn sub(self, rhs: Instant) -> Self::Output {
        self.duration_since(rhs)
    }
}
