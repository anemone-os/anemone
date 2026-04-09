//! Virtual memory area.

use crate::prelude::{heap::HeapBacking, segment::SegmentBacking, stack::StackBacking, *};

/// Backing of a [VmArea].
///
/// Backings don't care about virtual mapping, they just manage physical frames.
/// Mapping is handled by the frontend [VmArea] and top-level [UserSpace].
#[derive(Debug)]
pub enum VmAreaBacking {
    Segment(SegmentBacking),
    Stack(StackBacking),
    Heap(HeapBacking),
    // TODO: FileShared, FilePrivate, Anonymous, etc.
}

/// Virtual memory area.
///
/// Reference:
/// - https://elixir.bootlin.com/linux/v6.6.32/source/include/linux/mm_types.h#L565
#[derive(Debug)]
pub struct VmArea {
    pub(super) range: VirtPageRange,
    pub(super) perm: PteFlags,
    pub(super) backing: VmAreaBacking,
}

impl VmArea {
    /// Get the range of this VMA.
    pub fn range(&self) -> &VirtPageRange {
        &self.range
    }

    /// Get the permission of this VMA.
    pub fn perm(&self) -> PteFlags {
        self.perm
    }

    /// Get a reference to the backing of this VMA.
    pub fn backing(&self) -> &VmAreaBacking {
        &self.backing
    }

    /// Get a mutable reference to the backing of this VMA.
    pub fn backing_mut(&mut self) -> &mut VmAreaBacking {
        &mut self.backing
    }

    /// Handle a page fault in this VMA.
    ///
    /// Address of faulting page is guaranteed to be in the range of this VMA.
    pub fn handle_page_fault(&mut self, fault_info: &PageFaultInfo) -> Result<(), MmError> {
        debug_assert!(self.range.contains(fault_info.fault_addr().page_down()));

        todo!()
    }
}

impl VmArea {
    // should we put these methods here?
    // - copy_on_write
    // - lazy_alloc
    // etc.
}
