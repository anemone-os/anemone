pub mod generic;
pub use generic::*;
pub mod sv39;

// we'd better switch to sv48 when possible.
// pub mod sv48;

/// Linux ABI name of the paging mode selected below.
///
/// This diagnostic projection never drives paging behavior and must change
/// together with the selected paging architecture.
pub(super) const SATP_MODE_NAME: &str = "sv39";

pub use sv39::{Sv39KernelLayout as KernelLayout, Sv39PagingArch as RiscV64PagingArch};
