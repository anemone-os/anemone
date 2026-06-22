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

    fn check_pidx(&self, pidx: usize) -> Result<(), SysError> {
        if pidx >= self.pages.len() {
            return Err(SysError::InvalidArgument);
        }
        Ok(())
    }
}

impl VmObject for FixedObject {
    fn memory_report_kind(&self) -> Option<VmMemoryReportKind> {
        Some(VmMemoryReportKind::Static)
    }

    fn fill_memory_report(
        &self,
        range: core::ops::Range<usize>,
        kind: VmMemoryReportKind,
        report: &mut VmMemoryReport,
    ) {
        let start = range.start.min(self.pages.len());
        let end = range.end.min(self.pages.len());
        if start < end {
            report.add_shared(kind, end - start);
        }
    }

    fn resolve_frame(
        &self,
        pidx: usize,
        _access: PageFaultType,
    ) -> Result<ResolvedFrame, SysError> {
        self.check_pidx(pidx)?;
        Ok(ResolvedFrame {
            frame: self.pages[pidx].clone(),
            writable: true,
        })
    }

    fn exclusive_physical_pages(&self, range: core::ops::Range<usize>) -> usize {
        if range.start > range.end {
            return 0;
        }

        let end = range.end.min(self.pages.len());
        if range.start >= end {
            return 0;
        }

        self.pages[range.start..end]
            .iter()
            .filter(|frame| frame.meta().rc() == 1)
            .count()
    }
}
