use crate::prelude::{
    vmo::{FrameSource, ResolvedFrame, VmObject},
    *,
};

/// A trivial VMO that doesn't have any data and will never resolve any frame.
///
/// Currently used for guard page reservation.
#[derive(Debug)]
pub struct EmptyObject;

impl VmObject for EmptyObject {
    fn source_frame(&self, pidx: usize) -> Result<FrameSource, MmError> {
        Err(MmError::NotMapped)
    }

    fn resolve_frame(
        &mut self,
        pidx: usize,
        access: PageFaultType,
    ) -> Result<ResolvedFrame, MmError> {
        Err(MmError::NotMapped)
    }
}
