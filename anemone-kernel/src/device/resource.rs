//! Resource management for devices. This includes things like MMIO regions,
//! IRQs, etc.

use crate::prelude::*;

#[derive(Debug)]
pub enum Resource {
    Mmio { base: PhysAddr, len: usize },
}

impl Resource {
    pub const fn mmio(base: PhysAddr, len: usize) -> Self {
        Self::Mmio { base, len }
    }
}
