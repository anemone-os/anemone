//! Extension Unit Enable Register (EUEN) definitions and utilities.

use bitflags::bitflags;

bitflags! {
    /// EUEN register wrapper
    pub struct Euen: u64{
        /// Enable floating-point unit
        const FPE = 1 << 0;
        /// Enable SIMD unit
        const SXE = 1 << 1;
        /// Enable 256-bit SIMD instructions
        const ASXE = 1 << 2;
        /// Enable BTE instructions
        const BTE = 1 << 3;
    }
}

impl Euen {
    /// Create a new Euen value.
    pub const fn from_u64(value: u64) -> Self {
        Self::from_bits_retain(value)
    }

    /// Convert the Euen value to a u64.
    pub const fn to_u64(self) -> u64 {
        self.bits()
    }
}
