use crate::impl_bits32;

pub struct IpiSend(u32);
impl IpiSend {
    pub const fn new(intvec: u8, cpuid: u16, sync: bool) -> IpiSend {
        let mut res = IpiSend(0);
        res.set_intvec(intvec);
        res.set_cpuid(cpuid);
        res.set_sync(sync);
        res
    }
    impl_bits32!(bool, sync, 31);
    impl_bits32!(number, intvec, u8, 0, 5);
    impl_bits32!(number, cpuid, u16, 16, 26);
    pub const fn to_u32(&self) -> u32 {
        self.0
    }
    pub const fn from_u32(val: u32) -> IpiSend {
        IpiSend(val)
    }
}
