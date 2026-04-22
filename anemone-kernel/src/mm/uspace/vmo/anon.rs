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

    fn check_pidx(&self, pidx: usize) -> Result<(), SysError> {
        if pidx >= self.max_pages {
            return Err(SysError::InvalidArgument);
        }
        Ok(())
    }
}

impl VmObject for AnonObject {
    fn resolve_frame(&self, pidx: usize, access: PageFaultType) -> Result<ResolvedFrame, SysError> {
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

        let mut pages = self.pages.write();

        if let Some(frame) = pages.get(&pidx) {
            // Another thread might have resolved this page while we were waiting for the
            // write lock.
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
                        .ok_or(SysError::OutOfMemory)?
                        .into_frame_handle()
                };
                let resolved = ResolvedFrame {
                    frame: frame.clone(),
                    writable: true,
                };

                pages.insert(pidx, frame);
                Ok(resolved)
            },
        }
    }
}
