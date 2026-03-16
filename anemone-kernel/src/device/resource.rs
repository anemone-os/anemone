//! Resource management for devices. This includes things like MMIO regions,
//! IRQs, etc.

use crate::prelude::*;

/// POD representation of a device resource.
///
/// This structure only contains those resoureces that cannot be lazily
/// retrieved from firmware nodes.
///
/// We might optimize this out in future by lazily resolving all resources from
/// firmware nodes, but for now we just want to keep it simple.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Resource {
    Mmio { base: PhysAddr, len: usize },
}

impl Resource {
    pub const fn mmio(base: PhysAddr, len: usize) -> Self {
        Self::Mmio { base, len }
    }
}
