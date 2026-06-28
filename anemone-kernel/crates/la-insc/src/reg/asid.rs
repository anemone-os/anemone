//! ASID register wrapper

use crate::{impl_bits64, impl_const_u64_converter, impl_rw};

/// ASID register wrapper
pub struct Asid(u64);
impl Asid {
    impl_bits64!(number, asid, u16, 0, 10);

    /// Create with the given ASID value.
    ///
    /// Other bits a either write-ignored or zero, so we can safely create it
    /// from zero.
    ///
    /// Reference: `LoongArch-Vol1-v1.10-CN, 7.5.4`
    pub const fn new(asid: u16) -> Asid {
        let mut res = Asid(0);
        res.set_asid(asid);
        res
    }

    impl_const_u64_converter!();
}

impl_rw!(asid, asid, u16);
