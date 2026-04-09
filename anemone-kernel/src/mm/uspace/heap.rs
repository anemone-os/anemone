//! Heap as a [vma::VmArea] backing.
//!
//! Note that we can't abstract both heap and stack into something like
//! "single-direction growable memory area", cz heap is expanded at user's
//! request, while stack is passively expanded by page fault. So we keep them as
//! separate types.

use crate::prelude::{
    vma::{VmArea, VmAreaBacking},
    *,
};

#[derive(Debug)]
pub struct HeapBacking {
    /// Frames backing the heap.
    ///
    /// Ascending order: frames[0] is the bottom page of the heap,
    /// frames[frames.len() - 1] is the top page.
    frames: Vec<FrameHandle>,
}

impl HeapBacking {
    pub fn grow_one_page(&mut self) -> Result<(), MmError> {
        unsafe {
            self.frames.push(
                alloc_frame()
                    .ok_or(MmError::OutOfMemory)?
                    .into_frame_handle(),
            );
        }
        Ok(())
    }

    pub fn try_grow_by(&mut self, npages: usize) -> Result<(), MmError> {
        struct GrowTransaction<'a> {
            heap: &'a mut HeapBacking,
            new_frames: Vec<FrameHandle>,
            committed: bool,
        }

        todo!()
    }
}

impl VmArea {
    pub fn new_heap(range: VirtPageRange, perm: PteFlags) -> Self {
        Self {
            range,
            perm,
            backing: VmAreaBacking::Heap(HeapBacking { frames: vec![] }),
        }
    }

    pub fn as_heap(&self) -> Option<&HeapBacking> {
        match &self.backing {
            VmAreaBacking::Heap(heap) => Some(heap),
            _ => None,
        }
    }

    pub fn as_heap_mut(&mut self) -> Option<&mut HeapBacking> {
        match &mut self.backing {
            VmAreaBacking::Heap(heap) => Some(heap),
            _ => None,
        }
    }
}
