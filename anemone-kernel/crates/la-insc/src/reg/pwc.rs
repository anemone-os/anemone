use bitflags::bitflags;

use crate::{impl_bits32, impl_rw};

pub struct Pwcl(u32);
impl Pwcl {
    impl_bits32!(number, ptbase, u8, 0, 5);
    impl_bits32!(number, ptwidth, u8, 5, 10);
    impl_bits32!(number, dir1base, u8, 10, 15);
    impl_bits32!(number, dir1width, u8, 15, 20);
    impl_bits32!(number, dir2base, u8, 20, 25);
    impl_bits32!(number, dir2width, u8, 25, 30);
    impl_bits32!(bitflags, u8, ptewidth, PteWidth, 30, 32);
    pub const fn from_u32(val: u32) -> Pwcl {
        Pwcl(val)
    }
    pub const fn to_u32(&self) -> u32 {
        self.0
    }
    pub const fn new(
        ptbase: u8,
        ptwidth: u8,
        dir1base: u8,
        dir1width: u8,
        dir2base: u8,
        dir2width: u8,
        ptewidth: PteWidth,
    ) -> Pwcl {
        let mut res = Pwcl(0);
        res.set_ptbase(ptbase);
        res.set_ptwidth(ptwidth);
        res.set_dir1base(dir1base);
        res.set_dir1width(dir1width);
        res.set_dir2base(dir2base);
        res.set_dir2width(dir2width);
        res.set_ptewidth(ptewidth);
        res
    }
}

impl_rw!(pwcl, ptbase, u8);
impl_rw!(pwcl, ptwidth, u8);
impl_rw!(pwcl, dir1base, u8);
impl_rw!(pwcl, dir1width, u8);
impl_rw!(pwcl, dir2base, u8);
impl_rw!(pwcl, dir2width, u8);
impl_rw!(pwcl, ptewidth, PteWidth);

pub struct Pwch(u32);
impl Pwch {
    impl_bits32!(number, dir3base, u8, 0, 5);
    impl_bits32!(number, dir3width, u8, 5, 10);
    impl_bits32!(number, dir4base, u8, 10, 15);
    impl_bits32!(number, dir4width, u8, 15, 20);
    impl_bits32!(bool, hptw, 24);
    pub const fn from_u32(val: u32) -> Pwch {
        Pwch(val)
    }
    pub const fn to_u32(&self) -> u32 {
        self.0
    }
    pub const fn new(dir3base: u8, dir3width: u8, dir4base: u8, dir4width: u8, hptw: bool) -> Pwch {
        let mut res = Pwch(0);
        res.set_dir3base(dir3base);
        res.set_dir3width(dir3width);
        res.set_dir4base(dir4base);
        res.set_dir4width(dir4width);
        res.set_hptw(hptw);
        res
    }
}

impl_rw!(pwch, dir3base, u8);
impl_rw!(pwch, dir3width, u8);
impl_rw!(pwch, dir4base, u8);
impl_rw!(pwch, dir4width, u8);
impl_rw!(pwch, hptw, bool);

bitflags! {
    pub struct PteWidth: u8 {
        const WIDTH_64 = 0;
        const WIDTH_128 = 1;
        const WIDTH_256 = 2;
        const WIDTH_512 = 3;
    }
}
