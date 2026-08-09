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
    /// Stable HHDM address captured while `folio` is exclusively owned. All
    /// later raw subrange pointers derive from this address without recreating
    /// a whole-allocation Rust reference while a DMA token is live.
    base: VirtAddr,
    bytes: usize,
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
    pub fn as_ptr(&self) -> NonNull<[u8]> {
        // SAFETY: `base` and `bytes` were captured from `folio` at allocation;
        // `folio` remains owned for this region's lifetime. `as_ptr_mut` only
        // creates a raw pointer and does not manufacture a Rust reference.
        unsafe {
            NonNull::new_unchecked(core::ptr::slice_from_raw_parts_mut(
                self.base.as_ptr_mut::<u8>(),
                self.bytes,
            ))
        }
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
pub fn dma_alloc(nbytes: usize) -> Result<DmaRegion, SysError> {
    if nbytes == 0 {
        return Err(SysError::InvalidArgument);
    }

    let npages =
        align_up_power_of_2!(nbytes, PagingArch::PAGE_SIZE_BYTES) / PagingArch::PAGE_SIZE_BYTES;

    let folio = alloc_frames_zeroed(npages).ok_or(SysError::OutOfMemory)?;
    let bytes = npages
        .checked_mul(PagingArch::PAGE_SIZE_BYTES)
        .ok_or(SysError::InvalidArgument)?;
    let base = folio.range().start().to_phys_addr().to_hhdm();

    Ok(DmaRegion { folio, base, bytes })
}
