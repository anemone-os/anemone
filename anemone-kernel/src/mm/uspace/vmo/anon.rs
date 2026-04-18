//! Anonymous virtual memory object.
//!
//! For stack, heap, mmap, and other similar use cases.

use crate::prelude::{vmo::*, *};

#[derive(Debug)]
pub struct AnonObject {
    // TODO: use radix tree
    pages: RwLock<BTreeMap<usize, FrameHandle>>,
    max_pages: usize,
}

impl AnonObject {
    pub fn new(max_pages: usize) -> Self {
        Self {
            pages: RwLock::new(BTreeMap::new()),
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

// TODO: here exists a concurrency bug. if two threads write to the same page at
// the same time, they may both allocate a new frame and one of the writes will
// be lost.
//
// see ext4 regular file mapping for a solution.
impl VmObject for AnonObject {
    fn resolve_frame(&self, pidx: usize, access: PageFaultType) -> Result<ResolvedFrame, MmError> {
        self.check_pidx(pidx)?;

        {
            let pages = self.pages.read();

            if let Some(frame) = pages.get(&pidx) {
                return Ok(ResolvedFrame {
                    frame: frame.clone(),
                    writable: true,
                });
            }
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

                self.pages.write().insert(pidx, frame);
                Ok(resolved)
            },
        }
    }
}
