use core::ops::{Index, IndexMut};

use alloc::boxed::Box;
use la_insc::{
    impl_bits64,
    utils::{mem::MemAccessType, privl::PrivilegeLevel},
};
use crate::prelude::*;

pub struct LA64PagingArch;

impl PagingArchTrait for LA64PagingArch{
    
    type PgDir = LA64PageDirectory;
    
    const MAX_HUGE_PAGE_LEVEL: usize = 0;
    
    const PAGE_LEVELS: usize = 3;
    
    const MAX_PPN_BITS: usize = 44;
    
    const PAGE_SIZE_BYTES: usize = 4096;

    unsafe fn activate_addr_space(pgtbl: &crate::prelude::PageTable) {
        todo!()
    }

    fn prealloc_pgdirs_for_region(pgtbl: &mut crate::prelude::PageTable, range: crate::prelude::VirtPageRange) {
        todo!()
    }

    fn tlb_shootdown(vaddr: crate::prelude::VirtAddr) {
        todo!()
    }

    fn tlb_shootdown_all() {
        todo!()
    }
}

#[derive(Clone, Copy)]
pub struct LA64PageDirectory {
    entries: [LA64PageTableEntry; LA64PagingArch::PTE_PER_PGDIR],
}

impl Index<usize> for LA64PageDirectory {
    type Output = LA64PageTableEntry;

    fn index(&self, index: usize) -> &Self::Output {
        &self.entries[index]
    }
}

impl IndexMut<usize> for LA64PageDirectory {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.entries[index]
    }
}

impl PgDirArch for LA64PageDirectory {
    type Pte = LA64PageTableEntry;

    const ZEROED: Self = LA64PageDirectory {
        entries: [LA64PageTableEntry::ZEROED; LA64PagingArch::PTE_PER_PGDIR],
    };

    fn is_empty(&self) -> bool {
        for i in 0..LA64PagingArch::PTE_PER_PGDIR {
            if self[i].is_empty() {
                continue;
            } else {
                return false;
            }
        }
        true
    }
}

/// The format follows LoongArch's TLBELO (TLB entry low) layout.
/// Layout (bit positions):
///
/// ```
/// 64   63  62  61          PALEN           12       7   6     4     2   1   0
/// +----+---+---+-----------+---------------+--------+---+-----+-----+---+---+
/// |RPLV|NEX|NRD|    RSV    |      PPN      |  ZERO  | G | MAT | PLV | D | V |
/// +----+---+---+-----------+---------------+--------+---+-----+-----+---+---+
/// ```
/// 
/// Properties with `la_` prefix are loongarch-specific, 
/// interacting with the corresponding bits in the PTE, 
/// 
/// while those without `la_` are architecture-agnostic, 
/// converted from/to the loongarch-specific properties when necessary.
#[derive(Clone, Copy)]
pub struct LA64PageTableEntry(u64);

impl LA64PageTableEntry {
    const PPN_MASK: u64 = 0x0FFF_FFFF_FFFF_F000;
    impl_bits64!(value, u8, la_mat, MemAccessType, 4, 6);
    impl_bits64!(value, u8, la_plv, PrivilegeLevel, 2, 4);
    pub const fn la_flags(&self) -> LA64PteFlags {
        LA64PteFlags::from_bits_truncate(self.0 & 0x3ff)
    }
    pub const fn set_la_flags(&mut self, flags: LA64PteFlags) {
        let mut value = self.0;
        const FLAG_MASK: u64 = LA64PteFlags::all().bits();
        value &= !FLAG_MASK;
        value |= FLAG_MASK & flags.bits();
        self.0 = value;
    }
    pub const fn la_is_valid(&self) -> bool {
        self.la_flags().contains(LA64PteFlags::VALID)
    }
    pub const fn get(&self) -> u64 {
        self.0
    }
}

impl From<u64> for LA64PageTableEntry {
    fn from(value: u64) -> Self {
        LA64PageTableEntry(value)
    }
}

impl Into<u64> for LA64PageTableEntry {
    fn into(self) -> u64 {
        self.0
    }
}

impl PteArch for LA64PageTableEntry {
    const ZEROED: Self = LA64PageTableEntry(0);

    fn flags(&self) -> PteFlags {
        let mut base = self.la_flags().into();
        match self.la_mat() {
            MemAccessType::WeakNonCache => base |= PteFlags::NONCACHE,
            MemAccessType::StrongNonCache => base |= PteFlags::NONCACHE | PteFlags::STRONG,
            _ => {},
        }
        match self.la_plv() {
            PrivilegeLevel::PLV3 => base |= PteFlags::USER,
            _ => {},
        }
        base
    }

    fn new(ppn: PhysPageNum, flags: PteFlags) -> Self {
        let mut entry = LA64PageTableEntry((ppn.get() << 12) & Self::PPN_MASK);
        unsafe{
            entry.set_flags(flags);
        }
        entry
    }

    fn is_empty(&self) -> bool {
        self.get() == Self::ZEROED.get()
    }

    fn ppn(&self) -> PhysPageNum {
        PhysPageNum::new((self.0 & Self::PPN_MASK) >> 12)
    }

    fn is_leaf(&self) -> bool {
        !self.la_flags().contains(LA64PteFlags::DIR)
    }

    unsafe fn set_flags(&mut self, flags: PteFlags) {
        let flags_converted = LA64PteFlags::from(flags);
        self.set_la_flags(flags_converted);
        if !flags_converted.contains(LA64PteFlags::DIR) {
            // leaf entry
            // set memory access type
            self.set_la_mat(if flags.contains(PteFlags::NONCACHE) {
                if flags.contains(PteFlags::STRONG) {
                    MemAccessType::StrongNonCache
                } else {
                    MemAccessType::WeakNonCache
                }
            } else {
                MemAccessType::Cache
            });
            // set privilege level
            self.set_la_plv(if flags.contains(PteFlags::USER) {
                PrivilegeLevel::PLV3
            } else {
                PrivilegeLevel::PLV0
            });
        }
    }

    unsafe fn set_ppn(&mut self, ppn: PhysPageNum) {
        let mut value = self.0;
        value &= !Self::PPN_MASK;
        value |= (ppn.get() << 12) & Self::PPN_MASK;
        self.0 = value;
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub struct LA64PteFlags : u64{
        const VALID = 1<<0;
        const DIRTY = 1<<1;
        const GLOBAL = 1<<6;

        /// Software defined flag, used by the OS to indicate this PTE is a directory entry,
        /// otherwise it's a leaf entry.
        /// This bit in TLBELO is reserved and ignored by hardware.
        const DIR = 1<<7;

        /// Not Readable
        const NREAD = 1 << (u64::BITS - 3);
        /// Not Executable
        const NEXEC = 1 << (u64::BITS - 2);
        /// Restricted Privilege Level Enable
        ///
        /// When PRLV=0, this PTE can be accessed by any program whose privilege level is not lower than PLV;
        /// while PRLV=1, this PTE can only be accessed by programs whose privilege level equals PLV.
        const RPLV = 1 << (u64::BITS - 1);
    }
}

impl From<PteFlags> for LA64PteFlags {
    /// Convert generic PteFlags to LA64PteFlags.
    ///
    /// Only [PteFlags::VALID], [PteFlags::READ], [PteFlags::WRITE] and
    /// [PteFlags::EXECUTE] flags are recognized.     Other flags are
    /// ignored since they are not contained in [LA64PteFlags]. They must be
    /// manually handled.
    ///
    /// * If none of [PteFlags::READ], [PteFlags::WRITE] or [PteFlags::EXECUTE]
    ///   is set, the resulting LA64PteFlags will have the [LA64PteFlags::DIR]
    ///   flag set, indicating it's a directory entry.
    /// * Otherwise, the corresponding [LA64PteFlags::NREAD],
    ///   [LA64PteFlags::DIRTY] and [LA64PteFlags::NEXEC] flags will be set
    ///   according to the absence of READ, presence of WRITE and absence of
    ///   EXECUTE flags in the input PteFlags.
    fn from(value: PteFlags) -> Self {
        let mut flags = LA64PteFlags::empty();
        if value.contains(PteFlags::VALID) {
            flags |= LA64PteFlags::VALID;
        }
        if !value.contains(PteFlags::READ)
            && !value.contains(PteFlags::WRITE)
            && !value.contains(PteFlags::EXECUTE)
        {
            // if it's not readable, writable or executable, it's a directory entry.
            flags |= LA64PteFlags::DIR;
        } else {
            if !value.contains(PteFlags::READ) {
                flags |= LA64PteFlags::NREAD;
            }
            if value.contains(PteFlags::WRITE) {
                flags |= LA64PteFlags::DIRTY;
            }
            if !value.contains(PteFlags::EXECUTE) {
                flags |= LA64PteFlags::NEXEC;
            }
        }

        flags
    }
}

impl Into<PteFlags> for LA64PteFlags {
    fn into(self) -> PteFlags {
        let mut flags = PteFlags::empty();
        if self.contains(LA64PteFlags::VALID) {
            flags |= PteFlags::VALID;
        }
        if !self.contains(LA64PteFlags::NREAD) {
            flags |= PteFlags::READ;
        }
        if self.contains(LA64PteFlags::DIRTY) {
            flags |= PteFlags::WRITE;
        }
        if !self.contains(LA64PteFlags::NEXEC) {
            flags |= PteFlags::EXECUTE;
        }
        flags
    }
}
