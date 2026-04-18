use crate::prelude::{
    vmo::{ResolvedFrame, VmObject},
    *,
};

/// A trivial VMO that doesn't have any data and will never resolve any frame.
///
/// Currently used for guard page reservation.
#[derive(Debug)]
pub struct EmptyObject;

impl VmObject for EmptyObject {
    fn resolve_frame(&self, pidx: usize, access: PageFaultType) -> Result<ResolvedFrame, MmError> {
        Err(MmError::NotMapped)
    }
}
