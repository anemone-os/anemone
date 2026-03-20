use crate::impl_bits64;

pub struct Tcfg(u64);
impl Tcfg {
    impl_bits64!(bool, periodic, 1);
    impl_bits64!(bool, enabled, 0);
    impl_bits64!(number, initval, u64, 2, 63);
    pub const fn from_u64(val: u64) -> Self {
        Self(val)
    }
    pub const fn to_u64(&self) -> u64 {
        self.0
    }
    pub const fn new(initval: u64, periodic: bool, enabled: bool) -> Self {
        let mut config = Self(0);
        config.set_periodic(periodic);
        config.set_enabled(enabled);
        config.set_initval(initval);
        config
    }
}

pub struct TimerMisc(u64);
impl TimerMisc{
    impl_bits64!(bool, clk_enable, 47);
    pub const fn from_u64(val: u64) -> Self {
        Self(val)
    }
    pub const fn to_u64(&self) -> u64 {
        self.0
    }
}