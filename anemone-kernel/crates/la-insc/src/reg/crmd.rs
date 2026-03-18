use crate::{impl_bits64, impl_rw};

pub struct Crmd(u64);
impl Crmd {
    impl_bits64!(bool, ie, 2);
    pub const fn from_u64(val: u64) -> Crmd {
        Crmd(val)
    }
    pub const fn to_u64(&self) -> u64 {
        self.0
    }
}

impl_rw!(crmd, ie, bool);