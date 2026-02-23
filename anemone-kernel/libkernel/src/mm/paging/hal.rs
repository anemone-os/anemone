use core::ops::{Index, IndexMut};

use bitflags::bitflags;

use crate::libmm::addr::PhysPageNum;

/// The architecture-specific traits and types for paging.
pub trait PagingArchTrait: Sized {
    type PgDir: PgDirArch;
    /// The minimum page size supported by the architecture, in bytes.
    const PAGE_SIZE_BYTES: usize;
    /// The number of bits in the page offset, i.e., the number of bits needed
    /// to represent the page size.
    const PAGE_SIZE_BITS: usize = Self::PAGE_SIZE_BYTES.trailing_zeros() as usize;
    /// The number of levels in the page table hierarchy.
    const PAGE_LEVELS: usize;

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

    /// The number of bits in the page table entry flags.
    const PTE_FLAGS_BITS: usize;
    /// The bitmask for the page table entry flags, i.e., the bits that are used
    /// to represent the flags in a page table entry.
    const PTE_FLAGS_MASK: u64 = (1 << Self::PTE_FLAGS_BITS) - 1;

    /// Switch to the given page table.
    ///
    /// This function should always do a TLB shootdown.
    ///
    /// # Safety
    ///
    /// The `pgtbl` must point to a root page directory that is properly
    /// initialized and valid.
    unsafe fn activate_addr_space(pgtbl: PhysPageNum);
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

        // Combination flags
        // TODO
    }
}

pub trait PteArch: Sized + From<u64> + Into<u64> + Copy {
    /// A zeroed page table entry, i.e., an invalid entry with no flags set.
    const ZEROED: Self;

    /// Create a new page table entry with the given physical page number and
    /// flags.
    fn new(ppn: PhysPageNum, flags: PteFlags) -> Self;

    /// Check if this page table entry is empty, i.e., it is equal to ZEROED.
    fn is_empty(&self) -> bool {
        self.ppn() == Self::ZEROED.ppn() && self.flags() == Self::ZEROED.flags()
    }

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
    ///
    /// This often implies that the entry has the valid bit set, but does not
    /// have the read/write/execute bits set.
    fn is_branch(&self) -> bool {
        self.is_valid() && !self.is_leaf()
    }

    /// Set the flags of this page table entry to the given flags.
    ///
    /// This function should not modify the physical page number of the entry.
    fn set_flags(&mut self, flags: PteFlags);
}
pub trait PgDirArch:
    Sized + Copy + Index<usize, Output = Self::Pte> + IndexMut<usize, Output = Self::Pte>
{
    type Pte: PteArch;

    const ZEROED: Self;

    /// Check if this page directory is empty.
    fn is_empty(&self) -> bool;
}
