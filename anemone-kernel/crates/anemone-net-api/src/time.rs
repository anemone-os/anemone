/// Monotonic time relative to an arbitrary boot-local epoch.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct Instant(i64);

impl Instant {
    pub const ZERO: Self = Self::from_micros(0);

    pub const fn from_micros(micros: i64) -> Self {
        Self(micros)
    }

    pub const fn total_micros(self) -> i64 {
        self.0
    }
}

/// Non-negative monotonic duration.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct Duration(u64);

impl Duration {
    pub const ZERO: Self = Self::from_micros(0);

    pub const fn from_micros(micros: u64) -> Self {
        Self(micros)
    }

    pub const fn total_micros(self) -> u64 {
        self.0
    }
}
