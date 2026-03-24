//! Privilege Types
use bitflags::bitflags;

bitflags! {
    /// Combinable Privilege Flags
    pub struct PrivilegeFlags: u8{
        /// PLV0 Available
        const PLV0 = 0b0001;
        /// PLV1 Available
        const PLV1 = 0b0010;
        /// PLV2 Available
        const PLV2 = 0b0100;
        /// PLV3 Available
        const PLV3 = 0b1000;
    }
}

/// Privilege Levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum PrivilegeLevel{
    /// PLV0
    PLV0 = 0,
    /// PLV1
    PLV1 = 1,
    /// PLV2
    PLV2 = 2,
    /// PLV3
    PLV3 = 3,
}

impl PrivilegeLevel{
    /// From u8, used by macros, returns None if the value is invalid
    pub const fn from_value(value: u8) -> Option<Self>{
        match value{
            0 => Some(Self::PLV0),
            1 => Some(Self::PLV1),
            2 => Some(Self::PLV2),
            3 => Some(Self::PLV3),
            _ => None,
        }
    }

    /// From u8, used by macros
    pub const fn from_value_or_default(value: u8) -> Self{
        match value{
            0 => Self::PLV0,
            1 => Self::PLV1,
            2 => Self::PLV2,
            3 => Self::PLV3,
            _ => Self::PLV0,
        }
    }

    /// Get the u8 value of the enum, used by macros
    pub const fn value(&self) -> u8{
        *self as u8
    }
}
