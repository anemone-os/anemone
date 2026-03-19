use core::ops::{Index, IndexMut};

use crate::prelude::*;

/// The architecture-specific traits and types for paging.
pub trait PagingArchTrait: Sized {
    type PgDir: PgDirArch;

    // region: Paging

    /// The maximum level of huge page supported by this architecture.
    const MAX_HUGE_PAGE_LEVEL: usize;

    /// The number of levels in the page table hierarchy.
    const PAGE_LEVELS: usize;

    /// The maximum number of bits in the physical page number supported by this
    /// architecture.
    const MAX_PPN_BITS: usize;

    // endregion

    // region: Page Size

    /// The minimum page size supported by the architecture, in bytes.
    const PAGE_SIZE_BYTES: usize;

    /// The number of bits in the page offset, i.e., the number of bits needed
    /// to represent the page size.
    const PAGE_SIZE_BITS: usize = const {
        assert!(
            Self::PAGE_SIZE_BYTES.is_power_of_two(),
            "page size must be a power of two"
        );
        Self::PAGE_SIZE_BYTES.trailing_zeros() as usize
    };

    /// The number of pages per megabyte.
    const NPAGES_PER_MB: usize = 1024 * 1024 / Self::PAGE_SIZE_BYTES;

    /// The number of pages per gigabyte.
    const NPAGES_PER_GB: usize = 1024 * 1024 * 1024 / Self::PAGE_SIZE_BYTES;

    // endregion

    // region: Page Level Structure

    /// The number of page table entries per page directory, i.e., the number of
    /// entries in a page directory.
    ///
    /// Currently on 64-bit architectures, the size of a page table entry is
    /// always 8 bytes, so this is simply the page size divided by 8.
    const PTE_PER_PGDIR: usize = {
        assert!(
            core::mem::size_of::<<Self::PgDir as PgDirArch>::Pte>() == 8,
            "unsupported page table entry size"
        );
        Self::PAGE_SIZE_BYTES / core::mem::size_of::<<Self::PgDir as PgDirArch>::Pte>()
    };

    /// Number of bits needed to represent the number of page table entries per
    /// page directory.
    const PGDIR_IDX_BITS: usize = {
        assert!(
            Self::PTE_PER_PGDIR.is_power_of_two(),
            "number of page table entries per page directory must be a power of two"
        );
        Self::PTE_PER_PGDIR.trailing_zeros() as usize
    };

    // endregion

    /// Set up the direct mapping region.
    ///
    /// This function is called during kernel page table initialization.
    fn setup_direct_mapping_region(pgtbl: &mut PageTable);

    /// Switch to the given page table.
    ///
    /// This function should always do a TLB shootdown.
    unsafe fn activate_addr_space(pgtbl: &PageTable);

    /// Perform a TLB shootdown for the given virtual address in all virtual
    /// address spaces on current core.
    ///
    /// TODO: extend this to support ASID/PCID-based shootdowns.
    fn tlb_shootdown(vaddr: VirtAddr);

    /// Perform a TLB shootdown for the whole address space, in all virtual
    /// address spaces on current core.
    ///
    /// TODO: extend this to support ASID/PCID-based shootdowns.
    fn tlb_shootdown_all();
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct PteFlags: u64 {
        // Atomic flags
        /// Marks whether the leaf or branch entry is valid.
        ///
        /// On some architectures, this bit might not exist.
        ///
        /// This bit is only for internal use in the page table implementation,
        /// and should not be set by the caller when creating a mapping.
        const VALID = 1 << 0;
        const READ = 1 << 1;
        const WRITE = 1 << 2;
        const EXECUTE = 1 << 3;
        const USER = 1 << 4;

        /// Indicates whether this page is global and shared by multiple address spaces.
        /// An entry marked as global should not be unmapped or have its content changed
        /// in one address space, since it may affect other address spaces that share
        /// the same global page.
        const GLOBAL = 1 << 5;

        /// Indicates whether memory accesses is cacheable.
        ///
        /// **This bit only makes sense in systems that explicitly
        /// specify the memory access type in the PTE or other metadata describing virtual-address attributes,
        /// such as Loongarch, otherwise this bit is ignored.**
        const NONCACHE = 1 << 6;

        /// Indicates whether memory accesses is strong ordered in uncached mode.
        /// **This bit only makes sense when [Self::NONCACHE] is set.**
        const STRONG = 1 << 7;

        // Combination flags
        // TODO
    }
}

impl PteFlags {
    pub fn is_valid(&self) -> bool {
        self.contains(PteFlags::VALID)
    }
    /// **Ignoring the VALID bit, this function checks if the flags indicate a
    /// directory entry.**
    pub fn is_branch(&self) -> bool {
        !self.contains(PteFlags::READ)
            && !self.contains(PteFlags::WRITE)
            && !self.contains(PteFlags::EXECUTE)
    }
    /// **Ignoring the VALID bit, this function checks if the flags indicate a
    /// leaf entry.**
    pub fn is_leaf(&self) -> bool {
        (self.contains(PteFlags::READ)
            || self.contains(PteFlags::WRITE)
            || self.contains(PteFlags::EXECUTE))
    }
}

pub trait PteArch: Sized + From<u64> + Into<u64> + Copy {
    /// A zeroed page table entry, i.e., an invalid entry with no flags set.
    const ZEROED: Self;

    /// Create a new page table entry with the given physical page number and
    /// flags.
    /// ## Implementation
    ///
    /// * If flags contain [PteFlags::READ], [PteFlags::WRITE] or
    ///   [PteFlags::EXECUTE], this entry is a leaf entry that points to a
    ///   physical page.
    ///
    /// * Otherwise, this entry is a branch entry that points to a page
    ///   directory.
    ///
    /// If the entry is a branch entry, some flags like [PteFlags::USER],
    /// [PteFlags::NONCACHE] and [PteFlags::STRONG] (if available) are
    /// ignored.     **However, [PteFlags::GLOBAL] is still meaningful for
    /// branch entries, and should be kept if set.**
    fn new(ppn: PhysPageNum, flags: PteFlags, level: usize) -> Self;

    /// Check if this page table entry is empty, i.e., it is equal to ZEROED.
    fn is_empty(&self) -> bool;

    /// Get the flags of this page table entry.
    fn flags(&self) -> PteFlags;

    /// Get the physical page number of this page table entry.
    fn ppn(&self) -> PhysPageNum;

    /// Check the validity of this page table entry.
    fn is_valid(&self) -> bool {
        self.flags().contains(PteFlags::VALID)
    }

    /// Check if this page table entry is a leaf entry, i.e., it points to a
    /// physical page rather than a page directory.
    fn is_leaf(&self) -> bool;

    /// Check if this page table entry is a branch entry, i.e., it points to a
    /// page directory rather than a physical page.
    fn is_branch(&self) -> bool {
        self.is_valid() && !self.is_leaf()
    }

    /// Check if this page table entry is a global page.
    fn is_global(&self) -> bool {
        self.flags().contains(PteFlags::GLOBAL)
    }

    /// Set the flags of this page table entry to the given flags, while keeping
    /// the physical page number unchanged.
    ///
    /// # Safety
    ///
    /// In most cases, a TLB shootdown should be performed after this operation.
    unsafe fn set_flags(&mut self, flags: PteFlags);

    /// Set the physical page number of this page table entry to the given
    /// physical page number, while keeping the flags unchanged.
    ///
    /// # Safety
    ///
    /// In most cases, a TLB shootdown should be performed after this operation.
    unsafe fn set_ppn(&mut self, ppn: PhysPageNum);
}

/// A Page Directory should always take just a full page.
pub trait PgDirArch:
    Sized + Copy + Index<usize, Output = Self::Pte> + IndexMut<usize, Output = Self::Pte>
{
    type Pte: PteArch;

    const ZEROED: Self;

    /// Check if this page directory is empty.
    fn is_empty(&self) -> bool;
}
