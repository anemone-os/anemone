//! Extremely simple DMA management. IOMMU is not supported.

use core::ptr::NonNull;

use crate::{
    mm::remap::{IoRemap, ioremap},
    prelude::*,
};

/// An owned DMA region.
///
/// `DmaRegion` does not provide any method like `as_slice` to access the
/// region, because the region is also shared with hardware and thus should not
/// be accessed from a Rust reference. Only raw pointers should be used.
#[derive(Debug)]
pub struct DmaRegion {
    folio: OwnedFolio,
    // buddy system allocates pages in DM region, where no uncached flag is
    // set, so we need to do a separate mapping to get an uncached virtual
    // mapping for this region.
    // remap: IoRemap,
}

impl DmaRegion {
    /// Get the physical page number of the start of this DMA region.
    pub fn ppn(&self) -> PhysPageNum {
        self.folio.range().start()
    }

    /// Get the virtual address of the start of this DMA region.
    ///
    /// The returned pointer is guaranteed to be page-aligned. And the whole
    /// slice is uncached.
    pub fn vaddr(&self) -> NonNull<[u8]> {
        NonNull::from(self.folio.as_bytes())
        //        self.remap.as_ptr()
    }
}

/// Allocates a DMA region of the given size in bytes. The region will be
/// zeroed.
///
/// Internally, the requested size will be rounded up to a multiple of the page
/// size. And the returned region will always be page-aligned as well.
pub fn dma_alloc(nbytes: usize) -> Result<DmaRegion, MmError> {
    let npages =
        align_up_power_of_2!(nbytes, PagingArch::PAGE_SIZE_BYTES) / PagingArch::PAGE_SIZE_BYTES;

    let folio = alloc_frames_zeroed(npages).ok_or(MmError::OutOfMemory)?;

    // let remap = unsafe {
    //     ioremap(
    //         folio.range().start().to_phys_addr(),
    //         npages * PagingArch::PAGE_SIZE_BYTES,
    //     )
    // }?;

    Ok(DmaRegion { folio })
}
