use crate::prelude::{vmo::*, *};

#[derive(Debug)]
pub struct ShmObject {
    /// Resident pages indexed from the beginning of the segment.
    ///
    /// Pages are allocated lazily on first fault. Unlike private anonymous
    /// mappings, shared memory must materialize a real zeroed page even for the
    /// first read so later writes from another attachment become visible
    /// through the same frame.
    pages: RwLock<BTreeMap<usize, FrameHandle>>,
    max_pages: usize,
}

impl ShmObject {
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

    pub fn resident_pages(&self) -> usize {
        self.pages.read().len()
    }
}

impl VmObject for ShmObject {
    fn memory_report_kind(&self) -> Option<VmMemoryReportKind> {
        Some(VmMemoryReportKind::Shm)
    }

    fn fill_memory_report(
        &self,
        range: core::ops::Range<usize>,
        kind: VmMemoryReportKind,
        report: &mut VmMemoryReport,
    ) {
        report.add_shared(kind, self.pages.read().range(range).count());
    }

    fn resolve_frame(
        &self,
        pidx: usize,
        _access: PageFaultType,
    ) -> Result<ResolvedFrame, SysError> {
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
            return Ok(ResolvedFrame {
                frame: frame.clone(),
                writable: true,
            });
        }

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
    }
}
