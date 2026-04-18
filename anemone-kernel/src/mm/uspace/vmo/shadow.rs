//! Shadow virtual memory object.

use crate::prelude::{vmo::*, *};

/// **LOCK ORDERING:**
///
/// **`parent` -> `overlay`**
#[derive(Debug)]
pub struct ShadowObject {
    parent: Arc<dyn VmObject>,
    /// traditional word "override" in Rust is a reserved keyword...
    overlay: RwLock<BTreeMap<usize, FrameHandle>>,
}

impl ShadowObject {
    pub fn new(parent: Arc<dyn VmObject>) -> Self {
        Self {
            parent,
            overlay: RwLock::new(BTreeMap::new()),
        }
    }
}

impl VmObject for ShadowObject {
    fn resolve_frame(&self, pidx: usize, access: PageFaultType) -> Result<ResolvedFrame, MmError> {
        if let Some(frame) = self.overlay.read().get(&pidx) {
            return Ok(ResolvedFrame {
                frame: frame.clone(),
                writable: true,
            });
        }

        match access {
            PageFaultType::Write => {
                let ResolvedFrame { frame, writable: _ } =
                    self.parent.resolve_frame(pidx, PageFaultType::Read)?;

                let mut new_frame = alloc_frame().ok_or(MmError::OutOfMemory)?;
                new_frame.as_bytes_mut().copy_from_slice(frame.as_bytes());
                let new_frame = unsafe { new_frame.into_frame_handle() };

                let resolved = ResolvedFrame {
                    frame: new_frame.clone(),
                    writable: true,
                };
                self.overlay.write().insert(pidx, new_frame);
                Ok(resolved)
            },
            PageFaultType::Read | PageFaultType::Execute => {
                let resolved = self.parent.resolve_frame(pidx, access)?;
                Ok(ResolvedFrame {
                    frame: resolved.frame,
                    writable: false,
                })
            },
        }
    }
}
