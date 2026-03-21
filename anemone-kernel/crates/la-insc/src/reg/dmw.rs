//! DMW register wrapper
use crate::{
    impl_bits64, impl_const_u64_converter, utils::{mem::MemAccessType, privl::PrivilegeFlags}
};

/// DMW register wrapper
pub struct Dmw(u64);
impl Dmw {
    impl_bits64!(bitflags, u8, plv, PrivilegeFlags, 0, 4);
    impl_bits64!(value, u8, mat, MemAccessType, 4, 6);
    impl_bits64!(number, vseg, u8, 60, 64);

    /// Create a DMW value.
    pub const fn new(plv: PrivilegeFlags, mat: MemAccessType, vseg: u8) -> Dmw{
        let mut res = Dmw(0);
        res.set_plv(plv);
        res.set_mat(mat);
        res.set_vseg(vseg);
        res
    }

    /// Create a vseg_num from the given address.
    /// 
    /// Lower 60 bits of the addr are ignored.
    pub const fn vseg_from_addr(addr: u64) -> u8 {
        (addr >> 60) as u8
    }

    impl_const_u64_converter!();
}
