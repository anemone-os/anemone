//! Virtual memory object.
//!
//! TODO: currently we use shadow object, which comes from Mach microkernel. But
//! actually Zircon adopts a more flexible approach - hidden object, as an
//! advanced version of shadow object. We may want to switch to that in the
//! future, but for now shadow object is good enough for our use cases.
//!
//! Reference:
//! - https://fuchsia.dev/fuchsia-src/reference/kernel_objects/vm_object

pub mod anon;
pub mod fixed;
pub mod inode;
pub mod shadow;

use core::fmt::Debug;

use crate::prelude::*;

#[derive(Debug)]
pub enum FrameSource {
    Zero,
    Framed(FrameHandle),
}

fn shared_zero_frame() -> ResolvedFrame {
    static ZERO_FRAME: Lazy<FrameHandle> = Lazy::new(|| unsafe {
        alloc_frame_zeroed()
            .expect("failed to allocate zero frame")
            .into_frame_handle()
    });

    ResolvedFrame {
        frame: ZERO_FRAME.clone(),
        writable: false,
    }
}

impl FrameSource {
    /// Instantiate this frame source into a real frame.
    ///
    /// - For [FrameSource::Zero], this will allocate a fresh zeroed frame.
    /// - For [FrameSource::Framed], this will allocate a new frame and copy the
    ///   contents.
    pub fn instantiate(self) -> Result<FrameHandle, MmError> {
        match self {
            FrameSource::Zero => Ok(unsafe {
                alloc_frame_zeroed()
                    .ok_or(MmError::OutOfMemory)?
                    .into_frame_handle()
            }),
            FrameSource::Framed(frame) => {
                let mut new = alloc_frame().ok_or(MmError::OutOfMemory)?;
                new.as_bytes_mut().copy_from_slice(frame.as_bytes());
                Ok(unsafe { new.into_frame_handle() })
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedFrame {
    pub frame: FrameHandle,
    /// Whether this frame can be mapped writable if the VMA allows write.
    pub writable: bool,
}

pub trait VmObject: Send + Sync {
    /// Return the visible contents source for `pidx` without creating a local
    /// copy in the current object.
    ///
    /// Called by [VmObject] themselves when they need to peek into the parent
    /// object. More specifically, this is almost always used by
    /// [shadow::ShadowObject].
    fn source_frame(&self, pidx: usize) -> Result<FrameSource, MmError>;

    /// Resolve the frame at `pidx` for the given access type.
    ///
    /// `VmObject` are allowed to create a local copy of the frame in this
    /// method, which will be used for the current and future accesses to this
    /// page. This is how copy-on-write is implemented in
    /// [shadow::ShadowObject].
    ///
    /// Called by page fault handler.
    fn resolve_frame(
        &mut self,
        pidx: usize,
        access: PageFaultType,
    ) -> Result<ResolvedFrame, MmError>;

    fn read_frame(
        &self,
        pidx: usize,
        buffer: &mut [u8; PagingArch::PAGE_SIZE_BYTES],
    ) -> Result<(), MmError> {
        match self.source_frame(pidx)? {
            FrameSource::Zero => buffer.fill(0),
            FrameSource::Framed(frame) => buffer.copy_from_slice(frame.as_bytes()),
        }
        Ok(())
    }

    fn write_frame(
        &mut self,
        pidx: usize,
        data: &[u8; PagingArch::PAGE_SIZE_BYTES],
    ) -> Result<(), MmError> {
        let resolved = self.resolve_frame(pidx, PageFaultType::Write)?;
        if !resolved.writable {
            return Err(MmError::PermissionDenied);
        }

        let dst = unsafe {
            core::slice::from_raw_parts_mut(
                resolved.frame.ppn().to_phys_addr().to_hhdm().as_ptr_mut(),
                PagingArch::PAGE_SIZE_BYTES,
            )
        };
        dst.copy_from_slice(data);

        Ok(())
    }

    fn read(&self, offset: usize, buffer: &mut [u8]) -> Result<(), MmError> {
        let mut remaining = buffer;
        let mut cur_offset = offset;
        while !remaining.is_empty() {
            let pidx = cur_offset >> PagingArch::PAGE_SIZE_BITS;
            let page_offset = cur_offset & (PagingArch::PAGE_SIZE_BYTES - 1);
            let copy_len = remaining
                .len()
                .min(PagingArch::PAGE_SIZE_BYTES - page_offset);

            let mut page = [0u8; PagingArch::PAGE_SIZE_BYTES];
            self.read_frame(pidx, &mut page)?;
            remaining[..copy_len].copy_from_slice(&page[page_offset..page_offset + copy_len]);

            remaining = &mut remaining[copy_len..];
            cur_offset = cur_offset
                .checked_add(copy_len)
                .ok_or(MmError::InvalidArgument)?;
        }

        Ok(())
    }

    fn write(&mut self, offset: usize, data: &[u8]) -> Result<(), MmError> {
        let mut remaining = data;
        let mut cur_offset = offset;

        while !remaining.is_empty() {
            let pidx = cur_offset >> PagingArch::PAGE_SIZE_BITS;
            let page_offset = cur_offset & (PagingArch::PAGE_SIZE_BYTES - 1);
            let copy_len = remaining
                .len()
                .min(PagingArch::PAGE_SIZE_BYTES - page_offset);

            let mut page = [0u8; PagingArch::PAGE_SIZE_BYTES];
            if page_offset != 0 || copy_len != PagingArch::PAGE_SIZE_BYTES {
                self.read_frame(pidx, &mut page)?;
            }
            page[page_offset..page_offset + copy_len].copy_from_slice(&remaining[..copy_len]);
            self.write_frame(pidx, &page)?;

            remaining = &remaining[copy_len..];
            cur_offset = cur_offset
                .checked_add(copy_len)
                .ok_or(MmError::InvalidArgument)?;
        }

        Ok(())
    }
}

impl Debug for dyn VmObject {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "VmObject {{ ... }}")
    }
}
