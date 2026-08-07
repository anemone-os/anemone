use crate::prelude::*;

use core::ops::{Add, AddAssign, Sub, SubAssign};

/// A point on the mutable realtime calendar timeline.
///
/// Realtime steps can move this timeline in either direction, so this type
/// deliberately does not provide elapsed-time subtraction. Calendar adjustment
/// and the change-sequence protocol remain owned by the timekeeper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RealtimeInstant {
    ns: u64,
}

impl RealtimeInstant {
    pub fn now() -> Self {
        Self::from_nanos(crate::time::realtime_ns())
    }

    pub const fn from_nanos(ns: u64) -> Self {
        Self { ns }
    }

    pub const fn as_nanos(self) -> u64 {
        self.ns
    }

    /// Projects this calendar point into the existing Unix-epoch duration
    /// representation used by filesystem and ABI timestamp storage.
    pub const fn to_duration(self) -> Duration {
        Duration::from_nanos(self.ns)
    }

    pub fn checked_add(self, duration: Duration) -> Option<Self> {
        let ns = u64::try_from(duration.as_nanos()).ok()?;
        self.ns.checked_add(ns).map(Self::from_nanos)
    }

    pub fn checked_sub(self, duration: Duration) -> Option<Self> {
        let ns = u64::try_from(duration.as_nanos()).ok()?;
        self.ns.checked_sub(ns).map(Self::from_nanos)
    }
}

impl Add<Duration> for RealtimeInstant {
    type Output = Self;

    fn add(self, rhs: Duration) -> Self::Output {
        self.checked_add(rhs)
            .expect("overflow when adding duration to realtime instant")
    }
}

impl AddAssign<Duration> for RealtimeInstant {
    fn add_assign(&mut self, rhs: Duration) {
        *self = *self + rhs;
    }
}

impl Sub<Duration> for RealtimeInstant {
    type Output = Self;

    fn sub(self, rhs: Duration) -> Self::Output {
        self.checked_sub(rhs)
            .expect("underflow when subtracting duration from realtime instant")
    }
}

impl SubAssign<Duration> for RealtimeInstant {
    fn sub_assign(&mut self, rhs: Duration) {
        *self = *self - rhs;
    }
}
