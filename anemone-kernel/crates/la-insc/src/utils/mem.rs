//! Memory-related utilities for LA instructions

/// MAT Types
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum MemAccessType {
    /// Strongly ordered, non-cacheable memory access.
    ///
    /// It's safe for mmio access.
    StrongNonCache = 0,

    /// Cacheable memory access.
    Cache = 1,

    /// Weakly ordered, non-cacheable memory access.
    WeakNonCache = 2,

    /// Reserved
    Reserved = 3,
}

impl MemAccessType {
    /// From u8, used by macros
    pub const fn from_value_or_default(value: u8) -> Self {
        match value {
            0 => Self::StrongNonCache,
            1 => Self::Cache,
            2 => Self::WeakNonCache,
            _ => Self::Reserved,
        }
    }

    /// From u8, returns None if the value is invalid
    pub const fn from_value(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::StrongNonCache),
            1 => Some(Self::Cache),
            2 => Some(Self::WeakNonCache),
            3 => Some(Self::Reserved),
            _ => None,
        }
    }

    /// Get the u8 value of the enum, used by macros
    pub const fn value(&self) -> u8 {
        *self as u8
    }
}
