use core::fmt::Debug;

use crate::prelude::*;

/// Firmware node, used by drivers to read device information based on their own
/// needs.
///
/// This is actually an abstraction layer over hardware description nodes
/// provided by firmware interfaces such as ACPI and Device Tree, providing a
/// uniform interface for reading properties from firmware nodes, allowing the
/// driver code to be agnostic of the underlying firmware mechanism.
pub trait FwNode: Sync + Send {
    fn prop_read_u32(&self, prop_name: &str) -> Option<u32>;
    fn prop_read_u64(&self, prop_name: &str) -> Option<u64>;
    fn prop_read_str(&self, prop_name: &str) -> Option<String>;
    fn prop_read_present(&self, prop_name: &str) -> bool;

    // TODO: add more methods for retrieving information about the hardware, on
    // demand.
}

impl Debug for dyn FwNode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("dyn FwNode").finish()
    }
}
