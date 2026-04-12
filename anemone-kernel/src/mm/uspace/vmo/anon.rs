//! Anonymous virtual memory object.
//!
//! For stack, heap, mmap, and other similar use cases.

use crate::prelude::{vmo::*, *};

#[derive(Debug)]
pub struct AnonObject {
    // TODO: use radix tree
    pages: BTreeMap<usize, FrameHandle>,
    max_pages: usize,
}

impl AnonObject {
    pub fn new(max_pages: usize) -> Self {
        Self {
            pages: BTreeMap::new(),
            max_pages,
        }
    }

    fn check_pidx(&self, pidx: usize) -> Result<(), MmError> {
        if pidx >= self.max_pages {
            return Err(MmError::InvalidArgument);
        }
        Ok(())
    }
}

impl VmObject for AnonObject {
    fn source_frame(&self, pidx: usize) -> Result<FrameSource, MmError> {
        self.check_pidx(pidx)?;
        Ok(match self.pages.get(&pidx) {
            Some(frame) => FrameSource::Framed(frame.clone()),
            None => FrameSource::Zero,
        })
    }

    fn resolve_frame(
        &mut self,
        pidx: usize,
        access: PageFaultType,
    ) -> Result<ResolvedFrame, MmError> {
        self.check_pidx(pidx)?;

        if let Some(frame) = self.pages.get(&pidx) {
            return Ok(ResolvedFrame {
                frame: frame.clone(),
                writable: true,
            });
        }

        match access {
            PageFaultType::Read | PageFaultType::Execute => Ok(shared_zero_frame()),
            PageFaultType::Write => {
                let frame = unsafe {
                    alloc_frame_zeroed()
                        .ok_or(MmError::OutOfMemory)?
                        .into_frame_handle()
                };
                let resolved = ResolvedFrame {
                    frame: frame.clone(),
                    writable: true,
                };
                self.pages.insert(pidx, frame);
                Ok(resolved)
            },
        }
    }
}
