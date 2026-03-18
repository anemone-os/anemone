use crate::{impl_bits64, impl_rw};

pub struct Asid(u64);
impl Asid {
    impl_bits64!(number, asid, u16, 0, 10);
    pub const fn new(asid: u16) -> Asid {
        let mut res = Asid(0);
        res.set_asid(asid);
        res
    }
    pub const fn to_u64(&self) -> u64 {
        self.0 & Self::MASK_ASID
    }
    pub const fn from_u64(val: u64) -> Asid {
        Asid(val)
    }
}

impl_rw!(asid, asid, u16);