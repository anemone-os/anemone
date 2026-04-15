//! RAII wrapper around physical frames.

use core::mem::ManuallyDrop;

use crate::{
    mm::frame::{FRAME_ALLOCATOR, memmap::Frame},
    prelude::*,
};

#[derive(Debug)]
pub struct FrameHandle {
    ppn: PhysPageNum,
}

#[derive(Debug)]
pub struct OwnedFrameHandle {
    inner: FrameHandle,
}

impl TryFrom<FrameHandle> for OwnedFrameHandle {
    type Error = (SysError, FrameHandle);

    fn try_from(value: FrameHandle) -> Result<Self, Self::Error> {
        if unsafe { get_frame_raw(value.ppn) }.is_shared() {
            Err((SysError::SharedFrame, value))
        } else {
            Ok(Self { inner: value })
        }
    }
}

impl FrameHandle {
    /// Returns the physical page number of the frame represented by this
    /// handle.
    pub fn ppn(&self) -> PhysPageNum {
        self.ppn
    }

    /// Try to convert this `FrameHandle` into an `OwnedFrameHandle`.
    ///
    /// This will fail if the underlying frame is shared, since we cannot
    /// guarantee the ownership of a shared frame.
    pub fn try_into_owned(self) -> Result<OwnedFrameHandle, (SysError, FrameHandle)> {
        OwnedFrameHandle::try_from(self)
    }

    /// Get the underlying [Frame] of this handle.
    pub fn meta(&self) -> &'static Frame {
        unsafe { get_frame_raw(self.ppn) }
    }

    /// Returns a byte slice that represents the contents of the physical frame.
    ///
    /// **The slice created by this function points to HHDM region.**
    pub fn as_bytes(&self) -> &'_ [u8] {
        unsafe {
            core::slice::from_raw_parts(
                self.ppn.to_phys_addr().to_hhdm().as_ptr(),
                PagingArch::PAGE_SIZE_BYTES,
            )
        }
    }
}

impl Clone for FrameHandle {
    fn clone(&self) -> Self {
        unsafe {
            get_frame_raw(self.ppn).inc_ref();
        }
        Self { ppn: self.ppn }
    }
}

impl OwnedFrameHandle {
    /// Returns the physical page number of the frame represented by this
    /// handle.
    pub fn ppn(&self) -> PhysPageNum {
        self.inner.ppn
    }

    /// Get the underlying [Frame] of this handle.
    pub fn meta(&self) -> &'static Frame {
        unsafe { get_frame_raw(self.inner.ppn) }
    }

    /// Creates a new `OwnedFrameHandle` from the given physical page number.
    ///
    /// # Safety
    ///
    /// **Do not use this.**
    ///
    /// This funciton is only used by frame allocator when creating a new
    /// RAII handle for a newly allocated frame.
    pub unsafe fn new(ppn: PhysPageNum) -> Self {
        unsafe {
            #[cfg(debug_assertions)]
            assert!(get_frame_raw(ppn).is_free());

            get_frame_raw(ppn).inc_ref();
        }

        Self {
            inner: FrameHandle { ppn },
        }
    }

    /// Creates an `OwnedFrameHandle` from the given physical page number.
    ///
    /// # Safety
    ///
    /// `ppn` must be a valid physical page number that was leaked previously by
    /// caller, and caller now has the exclusive ownership of the corresponding
    /// physical frame.
    pub unsafe fn from_ppn(ppn: PhysPageNum) -> Self {
        unsafe {
            #[cfg(debug_assertions)]
            assert!(get_frame_raw(ppn).rc() == 1);
        }
        Self {
            inner: FrameHandle { ppn },
        }
    }

    /// Leak the `OwnedFrameHandle`, returning the underlying physical page
    /// number without deallocating it.
    ///
    /// Rust does not consider leaking memory to be unsafe, so we keep this
    /// function safe. However, one should always be careful when using this
    /// function, as it can easily lead to OOM.
    ///
    /// To transform the leaked frame back into a `OwnedFrameHandle`, use
    /// [OwnedFrameHandle::from_ppn] instead of [OwnedFrameHandle::new].
    pub fn leak(self) -> PhysPageNum {
        ManuallyDrop::new(self).inner.ppn
    }

    /// Returns a byte slice that represents the contents of the physical frame.
    ///
    /// **The slice created by this function points to HHDM region.**
    pub fn as_bytes(&self) -> &'_ [u8] {
        unsafe {
            core::slice::from_raw_parts(
                self.inner.ppn.to_phys_addr().to_hhdm().as_ptr(),
                PagingArch::PAGE_SIZE_BYTES,
            )
        }
    }

    /// Returns a mutable byte slice that represents the contents of the
    /// physical frame.
    ///
    /// **The slice created by this function points to HHDM region.**
    ///
    /// This function is safe because the [OwnedFrameHandle] represents an
    /// **owned** physical page, and thus we have the exclusive right to
    /// access it.
    pub fn as_bytes_mut(&mut self) -> &'_ mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(
                self.inner.ppn.to_phys_addr().to_hhdm().as_ptr_mut(),
                PagingArch::PAGE_SIZE_BYTES,
            )
        }
    }

    /// Converts this [OwnedFrameHandle] into a [FrameHandle].
    ///
    /// # Safety
    ///
    /// Caller should not 'leak' any exclusively accessible resource after
    /// calling this function.
    pub unsafe fn into_frame_handle(self) -> FrameHandle {
        self.inner
    }
}

impl Drop for FrameHandle {
    fn drop(&mut self) {
        unsafe {
            let frame = get_frame_raw(self.ppn);
            frame.dec_ref();
            if frame.is_free() {
                FRAME_ALLOCATOR.dealloc(PhysPageRange::new(self.ppn, 1));
            }
        }
    }
}

/// RAII wrapper around a contiguous range of physical frames.
///
/// `Folio` represents a batched ownership of multiple physical pages, and one
/// should not try to split a `Folio` into multiple `FrameHandle`s, as it can
/// easily lead to undefined behavior if the caller violates the ownership
/// requirement of the `Folio`.
#[derive(Debug)]
pub struct Folio {
    range: PhysPageRange,
}

#[derive(Debug)]
pub struct OwnedFolio {
    inner: Folio,
}

impl TryFrom<Folio> for OwnedFolio {
    type Error = (SysError, Folio);

    fn try_from(value: Folio) -> Result<Self, Self::Error> {
        for i in 0..value.range.npages() {
            if unsafe { get_frame_raw(value.range.start() + i) }.is_shared() {
                return Err((SysError::SharedFrame, value));
            }
        }
        Ok(Self { inner: value })
    }
}

impl Folio {
    /// Returns the physical page range of the folio.
    pub fn range(&self) -> PhysPageRange {
        self.range
    }

    /// Get the underlying [Frame] of this folio.
    pub fn meta(&self) -> &'static Frame {
        unsafe { get_frame_raw(self.range.start()) }
    }

    /// Try to convert this `Folio` into an `OwnedFolio`.
    ///
    /// This will fail if any of the underlying frames is shared, since we
    /// cannot guarantee the ownership of a shared frame.
    pub fn try_into_owned(self) -> Result<OwnedFolio, (SysError, Folio)> {
        OwnedFolio::try_from(self)
    }

    /// Returns a byte slice that represents the contents of the physical folio.
    ///
    /// **The slice created by this function points to HHDM region.**
    pub fn as_bytes(&self) -> &'_ [u8] {
        unsafe {
            core::slice::from_raw_parts(
                self.range.start().to_phys_addr().to_hhdm().as_ptr(),
                (self.range.npages() as usize) * PagingArch::PAGE_SIZE_BYTES,
            )
        }
    }
}

impl Clone for Folio {
    fn clone(&self) -> Self {
        unsafe {
            get_frame_raw(self.range.start()).inc_ref();

            Self { range: self.range }
        }
    }
}

impl OwnedFolio {
    /// Returns the physical page range of the folio.
    pub fn range(&self) -> PhysPageRange {
        self.inner.range()
    }

    /// Get the underlying [Frame] of this folio.
    pub fn meta(&self) -> &'static Frame {
        unsafe { get_frame_raw(self.inner.range.start()) }
    }

    /// Creates a new `OwnedFolio` from the given physical page range.
    ///
    /// # Safety
    ///
    /// **Do not use this.**
    ///
    /// This funciton is only used by frame allocator when creating a new
    /// RAII handle for a newly allocated folio.
    pub unsafe fn new(range: PhysPageRange) -> Self {
        unsafe {
            #[cfg(debug_assertions)]
            {
                for i in 0..range.npages() {
                    assert!(get_frame_raw(range.start() + i).is_free());
                }
            }

            // an dangerous but necessary optimization.
            //
            // we use the first frame's metadata to be on behalf of the whole folio
            //
            // according to Folio's contract, it's always safe unless user split the folio
            // into multiple frames.
            get_frame_raw(range.start()).inc_ref();

            Self {
                inner: Folio { range },
            }
        }
    }

    /// Creates an `OwnedFolio` from the given physical page range.
    ///
    /// # Safety
    ///
    /// `range` must be a valid physical page range that was leaked previously
    /// by caller, and caller now has the exclusive ownership of the
    /// corresponding physical frames.
    pub unsafe fn from_range(range: PhysPageRange) -> Self {
        unsafe {
            #[cfg(debug_assertions)]
            {
                assert_eq!(get_frame_raw(range.start()).rc(), 1,);
            }
        }

        Self {
            inner: Folio { range },
        }
    }

    /// Leaks the `OwnedFolio`, returning the underlying physical page range
    /// without deallocating it.
    ///
    /// Rust does not consider leaking memory to be unsafe, so we keep this
    /// function safe. However, one should always be careful when using this
    /// function, as it can easily lead to OOM.
    ///
    /// To transform the leaked frames back into an `OwnedFolio`, use
    /// [OwnedFolio::from_range] instead of [OwnedFolio::new].
    pub fn leak(self) -> PhysPageRange {
        let folio = ManuallyDrop::new(self);
        folio.inner.range()
    }

    /// Returns a byte slice that represents the contents of the physical folio.
    ///
    /// **The slice created by this function points to HHDM region.**
    pub fn as_bytes(&self) -> &'_ [u8] {
        unsafe {
            core::slice::from_raw_parts(
                self.inner.range.start().to_phys_addr().to_hhdm().as_ptr(),
                (self.inner.range.npages() as usize) * PagingArch::PAGE_SIZE_BYTES,
            )
        }
    }

    /// Returns a mutable byte slice that represents the contents of the
    /// physical folio.
    ///
    /// **The slice created by this function points to HHDM region.**
    ///
    /// This function is safe because the [OwnedFolio] represents an **owned**
    /// physical page range, and thus we have the exclusive right to access it.
    pub fn as_bytes_mut(&mut self) -> &'_ mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(
                self.inner
                    .range
                    .start()
                    .to_phys_addr()
                    .to_hhdm()
                    .as_ptr_mut(),
                (self.inner.range.npages() as usize) * PagingArch::PAGE_SIZE_BYTES,
            )
        }
    }

    /// Converts this [OwnedFolio] into a [Folio].
    ///
    /// # Safety
    ///
    /// Caller should not 'leak' any exclusively accessible resource after
    /// calling this function.
    pub unsafe fn into_folio(self) -> Folio {
        self.inner
    }
}

impl Drop for Folio {
    fn drop(&mut self) {
        unsafe {
            let frame = get_frame_raw(self.range.start());
            let rc = frame.rc();
            frame.dec_ref();
            if rc == 1 {
                FRAME_ALLOCATOR.dealloc(self.range);
            }
        }
    }
}
