pub mod generic;
pub use generic::*;
pub mod sv39;

// we'd better switch to sv48 when possible.
// pub mod sv48;

pub use sv39::{Sv39KernelLayout as KernelLayout, Sv39PagingArch as RiscV64PagingArch};
