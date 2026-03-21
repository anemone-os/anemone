//! IPI send register wrapper
use crate::{impl_bits32, impl_const_u32_converter};

/// IPI send register wrapper
pub struct IpiSend(u32);
impl IpiSend {
    /// Create an IPI send value.
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
    impl_const_u32_converter!();
}
