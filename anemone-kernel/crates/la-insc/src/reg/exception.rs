use bitflags::bitflags;

use crate::{impl_bits64, impl_rw};

pub struct Ecfg(u64);
impl Ecfg {
    impl_bits64!(bitflags, u16, local_ie, IntrFlags, 0, 13);
    impl_bits64!(number, vs, u8, 16, 19);
    pub const fn from_u64(val: u64) -> Ecfg {
        Ecfg(val)
    }
    pub const fn to_u64(&self) -> u64 {
        self.0
    }
    pub const fn new(local_ie: IntrFlags, vs: u8) -> Ecfg {
        let mut val = 0;
        val |= local_ie.bits() as u64;
        val |= (vs as u64) << 16;
        Ecfg(val)
    }
}

impl_rw!(ecfg, local_ie, IntrFlags);
impl_rw!(ecfg, vs, u8);

pub struct Estat(u64);

impl Estat {
    impl_bits64!(bitflags, u16, is, IntrFlags, 0, 13);
    impl_bits64!(number, ecode, u8, 16, 22);
    impl_bits64!(number, esubcode, u16, 22, 31);

    pub const fn from_u64(val: u64) -> Estat {
        Estat(val)
    }

    pub const fn to_u64(&self) -> u64 {
        self.0
    }
}

bitflags! {
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
