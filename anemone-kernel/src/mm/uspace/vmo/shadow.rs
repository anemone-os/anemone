//! Shadow virtual memory object.

use crate::prelude::{vmo::*, *};

#[derive(Debug)]
pub struct ShadowObject {
    parent: Arc<RwLock<dyn VmObject>>,
    /// traditional word "override" in Rust is a reserved keyword...
    overlay: BTreeMap<usize, FrameHandle>,
}

impl ShadowObject {
    pub fn new(parent: Arc<RwLock<dyn VmObject>>) -> Self {
        Self {
            parent,
            overlay: BTreeMap::new(),
        }
    }
}

impl VmObject for ShadowObject {
    fn source_frame(&self, pidx: usize) -> Result<FrameSource, MmError> {
        if let Some(frame) = self.overlay.get(&pidx) {
            Ok(FrameSource::Framed(frame.clone()))
        } else {
            self.parent.read().source_frame(pidx)
        }
    }

    fn resolve_frame(
        &mut self,
        pidx: usize,
        access: PageFaultType,
    ) -> Result<ResolvedFrame, MmError> {
        if let Some(frame) = self.overlay.get(&pidx) {
            return Ok(ResolvedFrame {
                frame: frame.clone(),
                writable: true,
            });
        }

        match access {
            PageFaultType::Write => {
                let src = self.parent.write().source_frame(pidx)?;
                let frame = src.instantiate()?;
                let resolved = ResolvedFrame {
                    frame: frame.clone(),
                    writable: true,
                };
                self.overlay.insert(pidx, frame);
                Ok(resolved)
            },
            PageFaultType::Read | PageFaultType::Execute => {
                let resolved = self.parent.write().resolve_frame(pidx, access)?;
                Ok(ResolvedFrame {
                    frame: resolved.frame,
                    writable: false,
                })
            },
        }
    }
}
