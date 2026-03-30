use crate::{mm::layout::KernelLayoutTrait, prelude::*};

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
    pub fn new() -> Result<Self, MmError> {
        let root = alloc_frame_zeroed().ok_or(MmError::OutOfMemory)?.leak();
        Ok(Self { root })
    }

    /// Get the physical page number of the root page directory.
    pub fn root_ppn(&self) -> PhysPageNum {
        self.root
    }

    /// Get a mutable reference to the root [PgDir]
    pub unsafe fn root_pgdir_mut(&mut self) -> &mut PgDir {
        unsafe {
            self.root_ppn()
                .to_hhdm()
                .to_virt_addr()
                .as_ptr_mut::<PgDir>()
                .as_mut()
                .expect("root ppn should not be null")
        }
    }

    /// Get a reference to the root [PgDir]
    pub unsafe fn root_pgdir(&self) -> &PgDir {
        unsafe {
            self.root_ppn()
                .to_hhdm()
                .to_virt_addr()
                .as_ptr::<PgDir>()
                .as_ref()
                .expect("root ppn should not be null")
        }
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
        let root_ppn = self.root_ppn();
        let mut mapper = self.mapper();
        // unmap all userspace pages
        unsafe {
            mapper.try_unmap(Unmapping {
                range: VirtPageRange::new(VirtPageNum::new(0), KernelLayout::USPACE_TOP_VPN.get()),
            });
        }
        let _frame = unsafe { OwnedFrameHandle::from_ppn(self.root) };
        kdebugln!("page table with root ppn {:?} dropped", self.root);
    }
}
