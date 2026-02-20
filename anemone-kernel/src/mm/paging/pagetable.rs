use core::marker::PhantomData;

use crate::prelude::*;

/// PageTable. The container of page directories.
///
/// The mapping/unmapping logic is implemented by mappers.
///
/// All the physical frames allocated for page directories are leaked
/// and managed manually, and transformed back to managed frame types
/// ([crate::Frame] and [crate::Folio]) when deallocating page directories.
#[derive(Debug)]
pub struct PageTable<P: PagingArch> {
    root: PhysPageNum,
    _ty: PhantomData<P>,
}

impl<P: PagingArch> PageTable<P> {
    /// Create a new Mapper with a newly allocated root page directory.
    pub fn new() -> Self {
        let root = alloc_frame()
            .expect("failed to allocate frame for root page directory")
            .leak();
        Self {
            root,
            _ty: PhantomData,
        }
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
    pub fn mapper(&mut self) -> Mapper<'_, P> {
        Mapper::new(self)
    }
}

impl<P: PagingArch> Drop for PageTable<P> {
    fn drop(&mut self) {
        let mut mapper = self.mapper();

        // unmap all pages
        match mapper.unmap(Unmapping {
            range: VirtPageRange::new(
                VirtPageNum::new(0),
                1 << (P::PAGE_LEVELS * P::PGDIR_IDX_BITS),
            ),
        }) {
            Ok(()) => (),
            Err(e) => {
                kerrln!("failed to unmap page table: {:#?}", e);
            }
        }
        let _frame = unsafe { Frame::from_ppn(self.root) };
    }
}
