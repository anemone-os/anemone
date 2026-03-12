use crate::{
    impl_bits64,
    utils::{mem::MemAccessType, privl::PrivilegeFlags},
};

pub struct Dmw(u64);
impl Dmw {
    impl_bits64!(bitflags, u8, plv, PrivilegeFlags, 0, 4);
    impl_bits64!(value, u8, mat, MemAccessType, 4, 6);
    impl_bits64!(number, vseg, u8, 60, 64);
    pub const fn new(plv: PrivilegeFlags, mat: MemAccessType, vseg: u8) -> Dmw{
        let mut res = Dmw(0);
        res.set_plv(plv);
        res.set_mat(mat);
        res.set_vseg(vseg);
        res
    }
    pub const fn vseg_from_addr(addr: u64) -> u8 {
        (addr >> 60) as u8
    }
    pub const fn to_u64(&self) -> u64 {
        self.0
    }
    pub const fn from_u64(val: u64) -> Dmw {
        Dmw(val)
    }
}
