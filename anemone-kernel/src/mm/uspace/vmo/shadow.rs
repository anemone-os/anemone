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

    fn parent_is_exclusive(&self) -> bool {
        Arc::strong_count(&self.parent) == 1
    }
}

impl VmObject for ShadowObject {
    fn resolve_frame(&self, pidx: usize, access: PageFaultType) -> Result<ResolvedFrame, SysError> {
        if let Some(frame) = self.overlay.read().get(&pidx) {
            return Ok(ResolvedFrame {
                frame: frame.clone(),
                writable: true,
            });
        }

        match access {
            PageFaultType::Write => {
                let mut overlay = self.overlay.write();

                let ResolvedFrame { frame, writable: _ } =
                    self.parent.resolve_frame(pidx, PageFaultType::Read)?;

                let mut new_frame = alloc_frame().ok_or(SysError::OutOfMemory)?;
                new_frame.as_bytes_mut().copy_from_slice(frame.as_bytes());
                let new_frame = unsafe { new_frame.into_frame_handle() };

                let resolved = ResolvedFrame {
                    frame: new_frame.clone(),
                    writable: true,
                };
                overlay.insert(pidx, new_frame);
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

    fn discard_range(&self, range: core::ops::Range<usize>) -> Result<(), SysError> {
        if range.start > range.end {
            return Err(SysError::InvalidArgument);
        }

        let mut overlay = self.overlay.write();
        overlay.retain(|pidx, _| !range.contains(pidx));
        Ok(())
    }

    fn exclusive_physical_pages(&self, range: core::ops::Range<usize>) -> usize {
        if range.start > range.end {
            return 0;
        }

        let overlay_pages = self
            .overlay
            .read()
            .range(range.clone())
            .filter(|(_, frame)| frame.meta().rc() == 1)
            .count();

        if !self.parent_is_exclusive() {
            return overlay_pages;
        }

        // If the parent VMO is only referenced by this shadow object, dropping
        // this address space will drop the parent chain too. Count it after the
        // overlay lock is released so we do not invert the documented
        // parent-before-overlay lock order.
        overlay_pages + self.parent.exclusive_physical_pages(range)
    }
}
