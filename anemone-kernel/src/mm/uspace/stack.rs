//! Stack as a [vma::VmArea] backing.

use crate::prelude::{
    vma::{VmArea, VmAreaBacking},
    *,
};

#[derive(Debug)]
pub struct StackBacking {
    /// Frames backing the stack.
    ///
    /// Descending order: frames[0] is the top page of the stack,
    /// frames[frames.len() - 1] is the bottom page.
    frames: Vec<FrameHandle>,
}

impl StackBacking {
    // TODO
}

impl VmArea {
    pub fn new_stack(
        range: VirtPageRange,
        perm: PteFlags,
        prealloc: usize,
    ) -> Result<Self, MmError> {
        let mut frames = vec![];
        unsafe {
            for _ in 0..prealloc {
                frames.push(
                    alloc_frame()
                        .ok_or(MmError::OutOfMemory)?
                        .into_frame_handle(),
                );
            }
        }

        Ok(Self {
            range,
            perm,
            backing: VmAreaBacking::Stack(StackBacking { frames }),
        })
    }

    pub fn as_stack(&self) -> Option<&StackBacking> {
        match &self.backing {
            VmAreaBacking::Stack(stack) => Some(stack),
            _ => None,
        }
    }

    pub fn as_stack_mut(&mut self) -> Option<&mut StackBacking> {
        match &mut self.backing {
            VmAreaBacking::Stack(stack) => Some(stack),
            _ => None,
        }
    }
}
