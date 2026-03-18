use bitflags::bitflags;

use crate::impl_bits64;

pub struct Ecfg(u64);
impl Ecfg {
    impl_bits64!(bitflags, u16, local_ie, IntrFlags, 0, 13);
    pub const fn from_u64(val: u64) -> Ecfg {
        Ecfg(val)
    }
    pub const fn to_u64(&self) -> u64 {
        self.0
    }
}

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
        const InterProessorIntr = 1 << 2;
        const TimerIntr = 1 << 3;
        const PerfMonOverflowIntr = 1 << 4;
        const HardwareIntr0 = 1 << 5;
        const HardwareIntr1 = 1 << 6;
        const HardwareIntr2 = 1 << 7;
        const HardwareIntr3 = 1 << 8;
        const HardwareIntr4 = 1 << 9;
        const HardwareIntr5 = 1 << 10;
        const HardwareIntr6 = 1 << 11;
        const HardwareIntr7 = 1 << 12;
    }
}
