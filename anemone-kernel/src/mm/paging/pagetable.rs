use crate::prelude::*;

/// PageTable. The container of page directories.
///
/// The mapping/unmapping logic is implemented by mappers.
///
/// All the physical frames allocated for page directories are leaked
/// and managed manually, and transformed back to managed frame types
/// ([crate::Frame] and [crate::Folio]) when deallocating page directories.
#[derive(Debug)]
pub struct PageTable {
    root: PhysPageNum,
}

impl PageTable {
    /// Create a new Mapper with a newly allocated root page directory.
    pub fn new() -> Self {
        let root = alloc_frame()
            .expect("failed to allocate frame for root page directory")
            .leak();
        Self { root }
    }

    /// Get the physical page number of the root page directory.
    pub fn root_ppn(&self) -> PhysPageNum {
        self.root
    }

    /// Get a mapper for this page table.
    ///
    /// The lifetime of the returned mapper is tied to the mutable reference of
    /// the page table, which means that we can only have one mutable reference
    /// to the page table at a time, and thus only one mapper at a time. This
    /// is a safety measure to prevent concurrent modification of the page table
    /// by multiple mappers, which can lead to undefined behavior.
    pub fn mapper(&mut self) -> Mapper<'_> {
        Mapper::new(self)
    }
}

impl Drop for PageTable {
    fn drop(&mut self) {
        let mut mapper = self.mapper();

        // unmap all pages
        mapper.unmap(Unmapping {
            range: VirtPageRange::new(
                VirtPageNum::new(0),
                1 << (PagingArch::PAGE_LEVELS * PagingArch::PGDIR_IDX_BITS),
            ),
        });
        let _frame = unsafe { OwnedFrameHandle::from_ppn(self.root) };
    }
}
