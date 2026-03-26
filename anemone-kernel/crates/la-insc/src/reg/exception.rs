//! Exception-related register wrappers

#![allow(missing_docs)]

use crate::{impl_bits64, impl_const_u64_converter, impl_rw};
use bitflags::bitflags;

/// ECFG register wrapper
pub struct Ecfg(u64);
impl Ecfg {
    impl_bits64!(bitflags, u16, local_ie, IntrFlags, 0, 13);
    impl_bits64!(number, vs, u8, 16, 19);
    impl_const_u64_converter!();
    /// Create an Ecfg value with the given local IE and vector number.
    ///
    /// Other bits are zero, so we can safely create it from zero.
    ///
    /// Reference: `LoongArch-Vol1-v1.10-CN, 7.4.5`
    pub const fn new(local_ie: IntrFlags, vs: u8) -> Ecfg {
        let mut val = 0;
        val |= local_ie.bits() as u64;
        val |= (vs as u64) << 16;
        Ecfg(val)
    }
}

impl_rw!(ecfg, local_ie, IntrFlags);
impl_rw!(ecfg, vs, u8);

/// ESTAT register wrapper
///
/// In fact, ESTAT is readonly, but write implementation is still provided,
///     and all writes will be **ignored**.
pub struct Estat(u64);

impl Estat {
    impl_bits64!(bitflags, u16, is, IntrFlags, 0, 13);
    impl_bits64!(number, ecode, u8, 16, 22);
    impl_bits64!(number, esubcode, u16, 22, 31);
    impl_const_u64_converter!();
}

bitflags! {
    /// Interrupt flags indicating interrupt types.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct IntrFlags: u16 {
        const SoftwareIntr0 = 1 << 0;
        const SoftwareIntr1 = 1 << 1;
        const InterProessorIntr = 1 << 12;
        const TimerIntr = 1 << 11;
        const PerfMonOverflowIntr = 1 << 10;
        const HardwareIntr0 = 1 << 9;
        const HardwareIntr1 = 1 << 8;
        const HardwareIntr2 = 1 << 7;
        const HardwareIntr3 = 1 << 6;
        const HardwareIntr4 = 1 << 5;
        const HardwareIntr5 = 1 << 4;
        const HardwareIntr6 = 1 << 3;
        const HardwareIntr7 = 1 << 2;
    }
}
