//! TODO: reference counting for shared ownership. idk whether simply using Arc
//! is sufficient or we need to implement an intrusive one.

use core::mem::ManuallyDrop;

use crate::{mm::frame::FRAME_ALLOCATOR, prelude::*};

/// A physical frame of memory.
///
/// RAII wrapper around a [`PhysPageNum`] that represents a minimal unit of
/// physical memory that can be allocated and deallocated, i.e. PAGE_SIZE_BYTES.
#[derive(Debug, PartialEq, Eq)]
pub struct Frame {
    ppn: PhysPageNum,
}

impl Frame {
    /// Creates a new `Frame` from the given physical page number.
    ///
    /// # Safety
    ///
    /// The caller must ensure that it has the ownership of the physical page
    /// specified by `ppn`.
    pub unsafe fn from_ppn(ppn: PhysPageNum) -> Self {
        Self { ppn }
    }

    pub fn ppn(&self) -> PhysPageNum {
        self.ppn
    }

    /// Leaks the `Frame`, returning the underlying physical page number without
    /// deallocating it.
    ///
    /// Rust does not consider leaking memory to be unsafe, so we keep this
    /// function safe. However, one should always be careful when using this
    /// function, as it can easily lead to OOM.
    pub fn leak(self) -> PhysPageNum {
        let frame = ManuallyDrop::new(self);
        frame.ppn
    }

    /// Returns a byte slice that represents the contents of the physical frame.
    ///
    /// **The slice created by this function points to HHDM region.**
    ///
    /// This function is safe because the [crate::Frame] represents an **owned**
    /// physical page, and thus we have the exclusive right to access it.
    ///
    /// However, one should be careful when using this function, as it can
    /// easily lead to undefined behavior if the caller violates the ownership
    /// requirement of the [crate::Frame].
    pub fn as_bytes(&self) -> &'_ [u8] {
        unsafe {
            core::slice::from_raw_parts(
                self.ppn.to_phys_addr().to_hhdm().as_ptr(),
                PagingArch::PAGE_SIZE_BYTES,
            )
        }
    }

    /// Returns a mutable byte slice that represents the contents of the
    /// physical frame.
    ///
    /// **The slice created by this function points to HHDM region.**
    ///
    /// This function is safe because the [crate::Frame] represents an **owned**
    /// physical page, and thus we have the exclusive right to access it.
    ///
    /// However, one should be careful when using this function, as it can
    /// easily lead to undefined behavior if the caller violates the ownership
    /// requirement of the [crate::Frame].
    pub fn as_bytes_mut(&mut self) -> &'_ mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(
                self.ppn.to_phys_addr().to_hhdm().as_ptr_mut(),
                PagingArch::PAGE_SIZE_BYTES,
            )
        }
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        unsafe {
            FRAME_ALLOCATOR.dealloc(PhysPageRange::new(self.ppn, 1));
        }
    }
}

/// RAII wrapper around a contiguous range of physical frames.
///
/// Currently no huge page support is implemented, so a `Folio` is just a
/// wrapper around a vector of `Frame`s.
#[derive(Debug, PartialEq, Eq)]
pub struct Folio {
    start_ppn: PhysPageNum,
    npages: u64,
}

impl Folio {
    /// Creates a `Folio` from the given physical page range.
    ///
    /// # Safety
    ///
    /// The caller must ensure that it has the ownership of the physical pages
    /// specified by `range`. The behavior is undefined if the caller violates
    /// this requirement.
    pub unsafe fn from_range(range: PhysPageRange) -> Self {
        Self {
            start_ppn: range.start(),
            npages: range.npages(),
        }
    }

    pub fn range(&self) -> PhysPageRange {
        PhysPageRange::new(self.start_ppn, self.npages)
    }

    /// Leaks the `Folio`, returning the underlying physical page range without
    /// deallocating it.
    ///
    /// Rust does not consider leaking memory to be unsafe, so we keep this
    /// function safe. However, one should always be careful when using this
    /// function, as it can easily lead to OOM.
    pub fn leak(self) -> PhysPageRange {
        let folio = ManuallyDrop::new(self);
        PhysPageRange::new(folio.start_ppn, folio.npages)
    }

    /// Returns a byte slice that represents the contents of the physical folio.
    ///
    /// **The slice created by this function points to HHDM region.**
    ///
    /// See [`Frame::as_bytes`] for safety and usage notes.
    pub fn as_bytes(&self) -> &'_ [u8] {
        unsafe {
            core::slice::from_raw_parts(
                self.start_ppn.to_phys_addr().to_hhdm().as_ptr(),
                (self.npages as usize) * PagingArch::PAGE_SIZE_BYTES,
            )
        }
    }

    /// Returns a mutable byte slice that represents the contents of the
    /// physical folio.
    ///
    /// **The slice created by this function points to HHDM region.**
    ///
    /// See [`Frame::as_bytes_mut`] for safety and usage notes.
    pub fn as_bytes_mut(&mut self) -> &'_ mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(
                self.start_ppn.to_phys_addr().to_hhdm().as_ptr_mut(),
                (self.npages as usize) * PagingArch::PAGE_SIZE_BYTES,
            )
        }
    }
}

impl Drop for Folio {
    fn drop(&mut self) {
        unsafe {
            FRAME_ALLOCATOR.dealloc(PhysPageRange::new(self.start_ppn, self.npages));
        }
    }
}
