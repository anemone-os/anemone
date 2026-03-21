use core::ops::{Index, IndexMut};

use crate::prelude::*;

pub const PAGE_SIZE_BYTES: usize = 4096;
pub const PTE_FLAGS_BITS: usize = 10;
const PTE_PER_PGDIR: usize = PAGE_SIZE_BYTES / core::mem::size_of::<RiscV64Pte>();

const PTE_FLAGS_MASK: u64 = (1 << PTE_FLAGS_BITS) - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct RiscV64Pte(u64);

impl RiscV64Pte {
    const fn get(&self) -> u64 {
        self.0
    }

    pub const fn arch_new(ppn: PhysPageNum, flags: RiscV64PteFlags) -> Self {
        Self((ppn.get() << PTE_FLAGS_BITS) | flags.bits())
    }
}

impl From<u64> for RiscV64Pte {
    fn from(value: u64) -> Self {
        RiscV64Pte(value)
    }
}

impl Into<u64> for RiscV64Pte {
    fn into(self) -> u64 {
        self.0
    }
}

bitflags! {
    pub struct RiscV64PteFlags: u64 {
        // Hardware-defined flags
        const VALID = 1 << 0;
        const READ = 1 << 1;
        const WRITE = 1 << 2;
        const EXECUTE = 1 << 3;
        const USER = 1 << 4;
        const GLOBAL = 1 << 5;
        const ACCESSED = 1 << 6;
        const DIRTY = 1 << 7;
        // Software-defined flags
        const SOFT1 = 1 << 8;
        const SOFT2 = 1 << 9;

        // Combination flags
        const BOOTSTRAP_KERNEL =
            Self::VALID.bits() | Self::READ.bits() |
            Self::WRITE.bits() | Self::EXECUTE.bits() |
            Self::ACCESSED.bits() | Self::DIRTY.bits();
        const BOOTSTRAP_RAM =
            Self::VALID.bits() | Self::READ.bits() |
            Self::WRITE.bits() |
            Self::ACCESSED.bits() | Self::DIRTY.bits();
    }
}

impl From<PteFlags> for RiscV64PteFlags {
    fn from(flags: PteFlags) -> Self {
        let mut result = RiscV64PteFlags::empty();
        if flags.contains(PteFlags::VALID) {
            result |= RiscV64PteFlags::VALID;
        }
        if flags.contains(PteFlags::READ) {
            result |= RiscV64PteFlags::READ;
        }
        if flags.contains(PteFlags::WRITE) {
            result |= RiscV64PteFlags::WRITE;
        }
        if flags.contains(PteFlags::EXECUTE) {
            result |= RiscV64PteFlags::EXECUTE;
        }
        if flags.contains(PteFlags::USER) {
            result |= RiscV64PteFlags::USER;
        }
        if flags.contains(PteFlags::GLOBAL) {
            result |= RiscV64PteFlags::GLOBAL;
        }
        // Ignore CACHED and STRONG bits.

        result
    }
}

impl From<RiscV64PteFlags> for PteFlags {
    fn from(flags: RiscV64PteFlags) -> Self {
        let mut result = PteFlags::empty();
        if flags.contains(RiscV64PteFlags::VALID) {
            result |= PteFlags::VALID;
        }
        if flags.contains(RiscV64PteFlags::READ) {
            result |= PteFlags::READ;
        }
        if flags.contains(RiscV64PteFlags::WRITE) {
            result |= PteFlags::WRITE;
        }
        if flags.contains(RiscV64PteFlags::EXECUTE) {
            result |= PteFlags::EXECUTE;
        }
        if flags.contains(RiscV64PteFlags::USER) {
            result |= PteFlags::USER;
        }
        if flags.contains(RiscV64PteFlags::GLOBAL) {
            result |= PteFlags::GLOBAL;
        }
        result
    }
}

impl RiscV64Pte {
    fn arch_flags(&self) -> RiscV64PteFlags {
        RiscV64PteFlags::from_bits_truncate(self.get() & PTE_FLAGS_MASK)
    }
}

impl PteArch for RiscV64Pte {
    const ZEROED: Self = RiscV64Pte(0);

    fn new(ppn: PhysPageNum, flags: PteFlags, _level: usize) -> Self {
        let flags: RiscV64PteFlags = flags.into();
        Self::arch_new(ppn, flags)
    }

    fn flags(&self) -> PteFlags {
        let flags = RiscV64PteFlags::from_bits_truncate(self.get() & PTE_FLAGS_MASK);
        flags.into()
    }

    fn ppn(&self) -> PhysPageNum {
        PhysPageNum::new(self.get() >> PTE_FLAGS_BITS)
    }

    unsafe fn set_flags(&mut self, flags: PteFlags) {
        let flags: RiscV64PteFlags = flags.into();
        self.0 = (self.0 & !PTE_FLAGS_MASK) | flags.bits();
    }

    unsafe fn set_ppn(&mut self, ppn: PhysPageNum) {
        self.0 = (self.0 & PTE_FLAGS_MASK) | (ppn.get() << PTE_FLAGS_BITS);
    }

    fn is_leaf(&self) -> bool {
        self.arch_flags().contains(RiscV64PteFlags::VALID)
            && self.arch_flags().intersects(
                RiscV64PteFlags::READ | RiscV64PteFlags::WRITE | RiscV64PteFlags::EXECUTE,
            )
    }

    fn is_empty(&self) -> bool {
        self.get() == Self::ZEROED.get()
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(align(4096), C)]
pub struct RiscV64PgDir {
    entries: [RiscV64Pte; 512],
}

impl Index<usize> for RiscV64PgDir {
    type Output = RiscV64Pte;

    fn index(&self, idx: usize) -> &Self::Output {
        &self.entries[idx]
    }
}

impl IndexMut<usize> for RiscV64PgDir {
    fn index_mut(&mut self, idx: usize) -> &mut Self::Output {
        &mut self.entries[idx]
    }
}

impl PgDirArch for RiscV64PgDir {
    type Pte = RiscV64Pte;

    const ZEROED: Self = RiscV64PgDir {
        entries: [RiscV64Pte::ZEROED; PAGE_SIZE_BYTES / core::mem::size_of::<RiscV64Pte>()],
    };

    fn is_empty(&self) -> bool {
        for i in 0..PTE_PER_PGDIR {
            if self[i].is_empty() {
                continue;
            } else {
                return false;
            }
        }
        true
    }
}
