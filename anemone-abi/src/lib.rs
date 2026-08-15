//! The Application Binary Interface for Anemone.
#![no_std]

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// Raw, unvalidated address bits supplied through a 64-bit userspace ABI.
///
/// This is a wire representation, not a pointer or an access capability. The
/// kernel must hand the bits to its user-access owner before reading or writing
/// the referenced memory.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    FromBytes,
    Immutable,
    IntoBytes,
    KnownLayout,
    Ord,
    PartialEq,
    PartialOrd,
)]
#[repr(transparent)]
pub struct RawUserAddr64(u64);

impl RawUserAddr64 {
    pub const NULL: Self = Self(0);

    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u64 {
        self.0
    }

    pub const fn is_null(self) -> bool {
        self.0 == 0
    }
}

impl<T: ?Sized> From<*const T> for RawUserAddr64 {
    fn from(pointer: *const T) -> Self {
        Self(pointer.cast::<()>() as u64)
    }
}

impl<T: ?Sized> From<*mut T> for RawUserAddr64 {
    fn from(pointer: *mut T) -> Self {
        Self(pointer.cast::<()>() as u64)
    }
}

const _: () = assert!(size_of::<RawUserAddr64>() == size_of::<u64>());
const _: () = assert!(align_of::<RawUserAddr64>() == align_of::<u64>());

pub mod capability;
pub mod errno;
pub mod fs;
pub mod hwprobe;
pub mod nemophila;
pub mod net;
pub mod process;
pub mod syscall;
pub mod system;
pub mod time;
pub mod tty;
pub mod uts;
