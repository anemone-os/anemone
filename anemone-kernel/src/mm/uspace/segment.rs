//! Elf segment as a [vma::VmArea] backing.
//!
//! In future we may discard this kind of vma, and just use file backing? But
//! for now we keep it as a separate type for simplicity.

use crate::prelude::{
    vma::{VmArea, VmAreaBacking},
    *,
};

#[derive(Debug)]
pub struct SegmentBacking {
    frames: Box<[FrameHandle]>,
}

impl VmArea {
    /// Create a new segment VMA with the given range, permission and backing
    /// frames.
    ///
    /// Order of `frames` must match the order of pages in `range`.
    pub fn new_segment(range: VirtPageRange, perm: PteFlags, frames: Box<[FrameHandle]>) -> Self {
        Self {
            range,
            perm,
            backing: VmAreaBacking::Segment(SegmentBacking { frames }),
        }
    }

    pub fn as_segment(&self) -> Option<&SegmentBacking> {
        match &self.backing {
            VmAreaBacking::Segment(seg) => Some(seg),
            _ => None,
        }
    }

    pub fn as_segment_mut(&mut self) -> Option<&mut SegmentBacking> {
        match &mut self.backing {
            VmAreaBacking::Segment(seg) => Some(seg),
            _ => None,
        }
    }
}

impl SegmentBacking {
    pub fn frames(&self) -> &[FrameHandle] {
        &self.frames
    }

    pub fn frames_mut(&mut self) -> &mut [FrameHandle] {
        &mut self.frames
    }

    pub fn get_frame(&self, page_idx: usize) -> Option<&FrameHandle> {
        self.frames.get(page_idx)
    }

    pub fn get_frame_mut(&mut self, page_idx: usize) -> Option<&mut FrameHandle> {
        self.frames.get_mut(page_idx)
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }
}
