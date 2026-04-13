use crate::prelude::{vmo::VmObject, *};

/// T.B.D.
#[derive(Debug)]
pub struct InodeObject {
    inode: InodeRef,
    page_cache: RwLock<BTreeMap<usize, FrameHandle>>,
}

impl InodeObject {
    /// **This method is not intended for public use. An [InodeRef] has only one
    /// [InodeObject] tied with it. Always use [InodeRef::vm_object] to access
    /// it. Thus, all processes see one page cache for the same inode.**
    pub fn new(inode: InodeRef) -> Self {
        Self {
            inode,
            page_cache: RwLock::new(BTreeMap::new()),
        }
    }
}

impl VmObject for InodeObject {
    fn source_frame(&self, pidx: usize) -> Result<vmo::FrameSource, MmError> {
        todo!()
    }

    fn resolve_frame(
        &mut self,
        pidx: usize,
        access: PageFaultType,
    ) -> Result<vmo::ResolvedFrame, MmError> {
        todo!()
    }
}
