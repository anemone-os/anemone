use crate::prelude::*;
use core::{
    fmt::Debug,
    ops::{Index, IndexMut},
};
use la_insc::{
    impl_bits64,
    insc::{InvtlbType, invtlb},
    reg::csr::{pgdh, pgdl},
    utils::{mem::MemAccessType, privl::PrivilegeLevel},
};

pub struct LA64PagingArch;
impl LA64PagingArch {
    unsafe fn get_activated_root_dir() -> &'static LA64PageDirectory {
        let addr = PhysAddr::new(unsafe { pgdl::csr_read() }).to_hhdm();
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
            let value = pgtbl.root_ppn().to_phys_addr().get();
            pgdl::csr_write(value);
            pgdh::csr_write(value);
        }
        Self::tlb_shootdown_all();
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
            invtlb(InvtlbType::All);
        }
    }

    fn setup_direct_mapping_region(pgtable: &mut PageTable) {
        // do nothing
    }
}

#[derive(Clone, Copy)]
#[repr(align(4096))]
#[repr(C)]
pub struct LA64PageDirectory {
    entries: [LA64PageTableEntry; LA64PagingArch::PTE_PER_PGDIR],
}

/// Create a bootstrap page table.
///
/// TODO: support different page size and more mapping types
pub const fn create_bootstrap_ptable() -> LA64PageDirectory {
    let mut pdir = LA64PageDirectory::ZEROED;

    // align to GB
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
#[repr(C)]
pub struct LA64PageTableEntry(u64);

impl LA64PageTableEntry {
    const FLAG_MASK: u64 = 0xFC00_0000_0000_0FFF;
    const PPN_OFFSET: usize = PagingArch::PAGE_SIZE_BITS;
    impl_bits64!(value, u8, la_mat, MemAccessType, 4, 6);
    impl_bits64!(value, u8, la_plv, PrivilegeLevel, 2, 4);

    pub const unsafe fn const_new(
        ppn: PhysPageNum,
        flags: LA64PteFlags,
        mat: MemAccessType,
        plv: PrivilegeLevel,
    ) -> Self {
        let mut entry = LA64PageTableEntry((ppn.get() << Self::PPN_OFFSET) & !Self::FLAG_MASK);
        unsafe {
            entry.set_la_flags_from_empty(flags);
        }
        entry.set_la_mat(mat);
        entry.set_la_plv(plv);
        entry
    }

    const fn la_flags(&self) -> LA64PteFlags {
        LA64PteFlags::from_bits_truncate(self.0)
    }
    const unsafe fn set_la_flags_from_empty(&mut self, flags: LA64PteFlags) {
        self.0 |= flags.bits();
    }
    const fn la_is_valid(&self) -> bool {
        self.la_flags().contains(LA64PteFlags::VALID)
    }
    pub const fn get(&self) -> u64 {
        self.0
    }
    fn la_is_in_leaf_table(&self) -> bool {
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
        let flags_converted = LA64PteFlags::from(flags, level == 0);
        let mut mat = MemAccessType::StrongNonCache;
        let mut plv = PrivilegeLevel::PLV0;
        if !flags.is_branch() {
            // leaf entry
            // set memory access type
            mat = if flags.contains(PteFlags::NONCACHE) {
                if flags.contains(PteFlags::STRONG) {
                    MemAccessType::StrongNonCache
                } else {
                    MemAccessType::WeakNonCache
                }
            } else {
                MemAccessType::Cache
            };
            // set privilege level
            plv = if flags.contains(PteFlags::USER) {
                PrivilegeLevel::PLV3
            } else {
                PrivilegeLevel::PLV0
            };
        }
        unsafe { LA64PageTableEntry::const_new(ppn, flags_converted, mat, plv) }
    }

    fn is_empty(&self) -> bool {
        self.get() == Self::ZEROED.get()
    }

    fn ppn(&self) -> PhysPageNum {
        PhysPageNum::new((self.0 & !Self::FLAG_MASK) >> Self::PPN_OFFSET)
    }

    fn is_leaf(&self) -> bool {
        self.la_is_valid()
            && !(self.la_flags().contains(LA64PteFlags::NREAD)
                & self.la_flags().contains(LA64PteFlags::NEXEC)
                & !self.la_flags().contains(LA64PteFlags::WRITE))
    }

    unsafe fn set_flags(&mut self, flags: PteFlags) {
        *self = LA64PageTableEntry::new(
            self.ppn(),
            flags,
            if self.la_is_in_leaf_table() { 0 } else { 1 },
        );
    }

    unsafe fn set_ppn(&mut self, ppn: PhysPageNum) {
        let mut value = self.0;
        value &= !Self::FLAG_MASK;
        value |= (ppn.get() << Self::PPN_OFFSET) & !Self::FLAG_MASK;
        self.0 = value;
    }
}

impl Debug for LA64PageTableEntry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!(
            "PPN: {:#x}, Flags(Global):{:?}, Flags: {:?}, MAT: {:?}, PLV: {:?}",
            self.ppn().get(),
            self.flags(),
            self.la_flags(),
            self.la_mat(),
            self.la_plv()
        ))
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub struct LA64PteFlags : u64{
        const LA_VALID = 1 << 0;
        const DIRTY = 1 << 1;
        const P_EXIST = 1 << 7;
        const WRITE = 1 << 8;

        const VALID = 1 << 58;
        const IN_LEAF_TABLE = 1 << 59;
        const GLOBAL = 1 << 60;

        const LA_COMMON_GLOBAL = 1<<6;
        const LA_HUGE = 1<<6;
        const LA_HUGE_GLOBAL = 1 << 12;

        /// Not Readable
        const NREAD = 1 << (u64::BITS - 3);
        /// Not Executable
        const NEXEC = 1 << (u64::BITS - 2);
        /// Restricted Privilege Level Enable
        ///
        /// When PRLV=0, this PTE can be accessed by any program whose privilege level is not lower than PLV;
        /// while PRLV=1, this PTE can only be accessed by programs whose privilege level equals PLV.
        const RPLV = 1 << (u64::BITS - 1);

        /// All flags in a common entry
        const ALL_COMMON =  Self::VALID.bits() | Self::DIRTY.bits() | Self::P_EXIST.bits() |
                            Self::WRITE.bits() | Self::IN_LEAF_TABLE.bits() | Self::GLOBAL.bits() |
                            Self::LA_COMMON_GLOBAL.bits() |Self::NREAD.bits() | Self::NEXEC.bits() |
                            Self::RPLV.bits();

        /// All flags in a huge entry
        const ALL_HUGE   =  Self::VALID.bits() | Self::DIRTY.bits() | Self::P_EXIST.bits() |
                            Self::WRITE.bits() | Self::IN_LEAF_TABLE.bits() | Self::GLOBAL.bits() |
                            Self::LA_HUGE.bits() | Self::LA_HUGE_GLOBAL.bits() |Self::NREAD.bits() |
                            Self::NEXEC.bits() | Self::RPLV.bits();

        /// Bootstrap entry flags
        const BOOTSTRAP_KERNEL =
            Self::VALID.bits()
            | Self::LA_VALID.bits()
            | Self::WRITE.bits()
            | Self::DIRTY.bits()
            | Self::P_EXIST.bits()
            | Self::LA_HUGE.bits();
    }
}

impl LA64PteFlags {
    // leaf entry:
    //      global -> LA_COMMON_GLOBAL
    // non-leaf entry:
    //   huge + global -> LA_HUGE + LA_HUGE_GLOBAL
    //   non-huge + global -> none
    //   huge + non-global -> LA_HUGE

    /// Convert generic [PteFlags] into LoongArch-specific [LA64PteFlags].
    ///
    /// Entry kind specific mapping:
    /// - If `in_leaf_table` is `true`, set [LA64PteFlags::IN_LEAF_TABLE]. For
    ///   valid entries, also set [LA64PteFlags::LA_VALID] and
    ///   [LA64PteFlags::P_EXIST]. If global, set
    ///   [LA64PteFlags::LA_COMMON_GLOBAL].
    ///
    /// - If `in_leaf_table` is `false` and `value.is_leaf()` is `true`, treat
    ///   it as a huge page entry and set [LA64PteFlags::LA_HUGE]. For valid
    ///   entries, also set [LA64PteFlags::LA_VALID] and
    ///   [LA64PteFlags::P_EXIST]. If global, set
    ///   [LA64PteFlags::LA_HUGE_GLOBAL].
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
        if value.contains(PteFlags::GLOBAL) {
            flags |= LA64PteFlags::GLOBAL;
        }
        if in_leaf_table {
            flags |= LA64PteFlags::IN_LEAF_TABLE;
            if value.contains(PteFlags::VALID) {
                flags |= LA64PteFlags::LA_VALID | LA64PteFlags::P_EXIST;
            }
            if value.contains(PteFlags::GLOBAL) {
                flags |= LA64PteFlags::LA_COMMON_GLOBAL;
            }
        } else if value.is_leaf() {
            flags |= LA64PteFlags::LA_HUGE;

            if value.contains(PteFlags::VALID) {
                flags |= LA64PteFlags::LA_VALID | LA64PteFlags::P_EXIST;
            }
            if value.contains(PteFlags::GLOBAL) {
                flags |= LA64PteFlags::LA_HUGE_GLOBAL;
            }
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
        if self.contains(LA64PteFlags::GLOBAL) {
            flags |= PteFlags::GLOBAL;
        }
        flags
    }
}
