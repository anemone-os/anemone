//! Extremely simple DMA management. IOMMU is not supported.

use core::{
    ptr::NonNull,
    sync::atomic::{Ordering, fence},
};

use crate::prelude::*;

/// An owned DMA region.
///
/// `DmaRegion` does not provide any method like `as_slice` to access the
/// region, because the region is also shared with hardware and thus should not
/// be accessed from a Rust reference. Only raw pointers should be used.
#[derive(Debug)]
pub struct DmaRegion {
    folio: OwnedFolio,
}

impl DmaRegion {
    /// Get the physical page number of the start of this DMA region.
    pub fn ppn(&self) -> PhysPageNum {
        self.folio.range().start()
    }

    /// Get the virtual address of the start of this DMA region.
    ///
    /// The returned pointer is guaranteed to be page-aligned.
    ///
    /// At the moment DMA memory is accessed through the normal kernel direct
    /// mapping rather than a dedicated uncached remap. This is acceptable for
    /// the current virtio-on-QEMU setup, which behaves as a coherent DMA
    /// device, but non-coherent platforms will need explicit cache maintenance
    /// around device ownership transfers.
    pub fn as_ptr(&mut self) -> NonNull<[u8]> {
        unsafe { NonNull::new_unchecked(self.folio.as_bytes_mut()) }
    }

    /// Make CPU writes visible before handing the buffer to a device.
    ///
    /// Currently this is only a fence to ensure ordering of memory operations,
    /// but on non-coherent platforms this may also need to include cache
    /// flushes. We'll implement that later.
    pub fn sync_for_device(&self) {
        fence(Ordering::SeqCst);
    }

    /// Make device writes visible before the CPU reads the buffer.
    ///
    /// Currently this is only a fence to ensure ordering of memory operations,
    /// but on non-coherent platforms this may also need to include cache
    /// invalidations. We'll implement that later.
    pub fn sync_for_cpu(&self) {
        fence(Ordering::SeqCst);
    }
}

/// Allocates a DMA region of the given size in bytes. The region will be
/// zeroed.
///
/// Internally, the requested size will be rounded up to a multiple of the page
/// size. And the returned region will always be page-aligned as well.
pub fn dma_alloc(nbytes: usize) -> Result<DmaRegion, MmError> {
    if nbytes == 0 {
        return Err(MmError::InvalidArgument);
    }

    let npages =
        align_up_power_of_2!(nbytes, PagingArch::PAGE_SIZE_BYTES) / PagingArch::PAGE_SIZE_BYTES;

    let folio = alloc_frames_zeroed(npages).ok_or(MmError::OutOfMemory)?;

    Ok(DmaRegion { folio })
}
