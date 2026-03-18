use core::ops::{Index, IndexMut};

use crate::prelude::*;
use alloc::boxed::Box;
use la_insc::{
    impl_bits64,
    insc::{InvtlbType, invtlb},
    reg::{asid, csr::tlbrsave},
    utils::{mem::MemAccessType, privl::PrivilegeLevel},
};

pub struct LA64PagingArch;
impl LA64PagingArch {
    unsafe fn get_activated_root_dir() -> &'static LA64PageDirectory {
        let addr = PhysAddr::new(unsafe { tlbrsave::csr_read() }).to_hhdm();
        unsafe { &*addr.as_ptr::<LA64PageDirectory>() }
    }
}

impl PagingArchTrait for LA64PagingArch {
    type PgDir = LA64PageDirectory;

    const MAX_HUGE_PAGE_LEVEL: usize = 2;

    const PAGE_LEVELS: usize = 3;

    const MAX_PPN_BITS: usize = 44;

    const PAGE_SIZE_BYTES: usize = 4096;

    unsafe fn activate_addr_space(pgtbl: &PageTable) {
        unsafe {
            tlbrsave::csr_write(pgtbl.root_ppn().get());
        }
    }

    fn prealloc_pgdirs_for_region(
        pgtbl: &mut crate::prelude::PageTable,
        range: crate::prelude::VirtPageRange,
    ) {
        todo!()
    }

    fn tlb_shootdown(vaddr: VirtAddr) {
        unsafe {
            invtlb(InvtlbType::NonGlobalWithAsidAndVaddr {
                asid: 0,
                vaddr: vaddr.get(),
            });
        }
    }

    fn tlb_shootdown_all() {
        unsafe {
            invtlb(InvtlbType::NonGlobal);
        }
    }
}

#[derive(Clone, Copy)]
#[repr(align(4096))]
pub struct LA64PageDirectory {
    entries: [LA64PageTableEntry; LA64PagingArch::PTE_PER_PGDIR],
}

pub const fn create_bootstrap_ptable() -> LA64PageDirectory {
    let mut pdir = LA64PageDirectory::ZEROED;

    let k_phys_align_down = align_down_power_of_2!(KERNEL_LA_BASE, 1 << 30);
    let k_phys_ppn = k_phys_align_down as u64 >> 12;
    let k_virt_idx = (KERNEL_VA_BASE >> 30) as usize & 0x1ff;

    // 1. map kernel image to -2gb ~ 0
    assert!(k_virt_idx == 510);
    pdir.entries[k_virt_idx] = unsafe {
        LA64PageTableEntry::const_new(
            PhysPageNum::new(k_phys_ppn),
            LA64PteFlags::BOOTSTRAP_KERNEL,
            MemAccessType::Cache,
            PrivilegeLevel::PLV0,
        )
    };
    pdir.entries[k_virt_idx + 1] = unsafe {
        LA64PageTableEntry::const_new(
            PhysPageNum::new(k_phys_ppn + 512 * 512),
            LA64PteFlags::BOOTSTRAP_KERNEL,
            MemAccessType::Cache,
            PrivilegeLevel::PLV0,
        )
    };

    // 2. direct mapping for code running without page fault
    let direct_idx = k_phys_align_down as usize >> 30;
    pdir.entries[direct_idx] = unsafe {
        LA64PageTableEntry::const_new(
            PhysPageNum::new(k_phys_ppn),
            LA64PteFlags::BOOTSTRAP_KERNEL,
            MemAccessType::Cache,
            PrivilegeLevel::PLV0,
        )
    };
    pdir.entries[direct_idx + 1] = unsafe {
        LA64PageTableEntry::const_new(
            PhysPageNum::new(k_phys_ppn + 512 * 512),
            LA64PteFlags::BOOTSTRAP_KERNEL,
            MemAccessType::Cache,
            PrivilegeLevel::PLV0,
        )
    };
    pdir
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
/// BASIC:
/// 64   63  62  61          PALEN           12       9   8   7   6     4     2   1   0
/// +----+---+---+-----------+---------------+--------+---+---+---+-----+-----+---+---+
/// |RPLV|NEX|NRD|    RSV    |      PPN      |  ZERO  | W | P | G | MAT | PLV | D | V |
/// +----+---+---+-----------+---------------+--------+---+---+---+-----+-----+---+---+
/// HUGE PAGE:
/// 64   63  62  61          PALEN       13  12       9   8   7   6     4     2   1   0
/// +----+---+---+-----------+-----+-----+---+--------+---+---+---+-----+-----+---+---+
/// |RPLV|NEX|NRD|    RSV    | PPN |     | G |  ZERO  | W | P | H | MAT | PLV | D | V |
/// +----+---+---+-----------+-----+-----+---+--------+---+---+---+-----+-----+---+---+
/// ```
///  * **For leaf page directories, all entries are implemented as BASIC
///    entries**
///  * **For non-leaf page directories, branch entries are implemented as BASIC
///    entries, where [G] bits are always 0. So [G] bit is used as huge page
///    indicator, called [H]. Leaf entries should be implemented as HUGE PAGE
///    entries.**
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

    pub const unsafe fn const_new(
        ppn: PhysPageNum,
        flags: LA64PteFlags,
        mat: MemAccessType,
        plv: PrivilegeLevel,
    ) -> Self {
        let mut entry = LA64PageTableEntry((ppn.get() << 12) & Self::PPN_MASK);
        entry.set_la_flags(flags);
        entry.set_la_mat(mat);
        entry.set_la_plv(plv);
        entry
    }

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
    pub fn la_is_in_leaf_table(&self) -> bool {
        self.la_flags().contains(LA64PteFlags::IN_LEAF_TABLE)
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

    fn new(ppn: PhysPageNum, flags: PteFlags, level: usize) -> Self {
        let mut entry = LA64PageTableEntry((ppn.get() << 12) & Self::PPN_MASK);
        if level == 0 {
            entry.set_la_flags(LA64PteFlags::IN_LEAF_TABLE);
        }
        unsafe {
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
        self.la_is_valid()
            && self
                .la_flags()
                .contains(LA64PteFlags::NREAD | LA64PteFlags::NEXEC)
            && !self.la_flags().contains(LA64PteFlags::WRITE)
    }

    unsafe fn set_flags(&mut self, flags: PteFlags) {
        let flags_converted = LA64PteFlags::from(flags, self.la_is_in_leaf_table());
        self.set_la_flags(flags_converted);
        if !flags.is_branch() {
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
        const VALID = 1 << 0;
        const DIRTY = 1 << 1;
        const LEAF_GLOBAL = 1 << 6;
        const BRANCH_HUGE = 1 << 6;
        const P_EXIST = 1 << 7;
        const WRITE = 1 << 8;

        const IN_LEAF_TABLE = 1 << 9;

        /// Not Readable
        const NREAD = 1 << (u64::BITS - 3);
        /// Not Executable
        const NEXEC = 1 << (u64::BITS - 2);
        /// Restricted Privilege Level Enable
        ///
        /// When PRLV=0, this PTE can be accessed by any program whose privilege level is not lower than PLV;
        /// while PRLV=1, this PTE can only be accessed by programs whose privilege level equals PLV.
        const RPLV = 1 << (u64::BITS - 1);

        const BOOTSTRAP_KERNEL =
            Self::VALID.bits()
            | Self::WRITE.bits()
            | Self::DIRTY.bits()
            | Self::P_EXIST.bits()
            | Self::BRANCH_HUGE.bits();
    }
}

impl LA64PteFlags {
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
    pub fn from(value: PteFlags, in_leaf_table: bool) -> Self {
        let mut flags = LA64PteFlags::empty();
        if value.contains(PteFlags::VALID) {
            flags |= LA64PteFlags::VALID;
        }
        if !value.contains(PteFlags::READ) {
            flags |= LA64PteFlags::NREAD;
        }
        if value.contains(PteFlags::WRITE) {
            flags |= LA64PteFlags::DIRTY | LA64PteFlags::WRITE;
        }
        if !value.contains(PteFlags::EXECUTE) {
            flags |= LA64PteFlags::NEXEC;
        }

        if !in_leaf_table && value.is_leaf() {
            flags |= LA64PteFlags::BRANCH_HUGE;
        } else if in_leaf_table {
            debug_assert!(value.is_leaf());
            flags |= LA64PteFlags::IN_LEAF_TABLE;
        }

        flags
    }
    pub fn into(self) -> PteFlags {
        let mut flags = PteFlags::empty();
        if self.contains(LA64PteFlags::VALID) {
            flags |= PteFlags::VALID;
        }
        if !self.contains(LA64PteFlags::NREAD) {
            flags |= PteFlags::READ;
        }
        if self.contains(LA64PteFlags::WRITE) {
            flags |= PteFlags::WRITE;
        }
        if !self.contains(LA64PteFlags::NEXEC) {
            flags |= PteFlags::EXECUTE;
        }
        #[cfg(debug_assertions)]
        {
            if self.contains(LA64PteFlags::BRANCH_HUGE) {
                debug_assert!(!self.contains(LA64PteFlags::IN_LEAF_TABLE));
                debug_assert!(flags.is_leaf());
            }
            if self.contains(LA64PteFlags::IN_LEAF_TABLE) {
                debug_assert!(!self.contains(LA64PteFlags::BRANCH_HUGE));
                debug_assert!(flags.is_leaf());
            }
        }
        flags
    }
}
