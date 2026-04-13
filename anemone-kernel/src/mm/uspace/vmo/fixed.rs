//! Fixed length page set as a virtual memory object.
//!
//! Use cases include elf segments.

use crate::prelude::{vmo::*, *};

#[derive(Debug)]
pub struct FixedObject {
    pages: Box<[FrameHandle]>,
}

impl FixedObject {
    pub fn new(pages: Box<[FrameHandle]>) -> Self {
        Self { pages }
    }

    fn check_pidx(&self, pidx: usize) -> Result<(), MmError> {
        if pidx >= self.pages.len() {
            return Err(MmError::InvalidArgument);
        }
        Ok(())
    }
}

impl VmObject for FixedObject {
    fn resolve_frame(&self, pidx: usize, _access: PageFaultType) -> Result<ResolvedFrame, MmError> {
        self.check_pidx(pidx)?;
        Ok(ResolvedFrame {
            frame: self.pages[pidx].clone(),
            writable: true,
        })
    }
}
