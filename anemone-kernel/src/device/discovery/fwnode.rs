use core::{any::Any, fmt::Debug};

use crate::{device::discovery::open_firmware::OpenFirmwareNode, prelude::*};

/// Firmware node, used by drivers to read device information based on their own
/// needs.
///
/// This is actually an abstraction layer over hardware description nodes
/// provided by firmware interfaces such as ACPI and Device Tree, providing a
/// uniform interface for reading properties from firmware nodes, allowing the
/// driver code to be agnostic of the underlying firmware mechanism.
///
/// Refer to https://elixir.bootlin.com/linux/v6.6.32/source/include/linux/fwnode.h for more details.
///
/// **One Important Invariant**: each physical device should have exactly one
/// corresponding firmware node. For multiple firmware nodes referring to the
/// same device, [Arc] is used.
pub trait FwNode: Sync + Send + Any {
    /// Check whether two firmware nodes refer to the same hardware entity.
    ///
    /// This can be implemented by pointer comparison. But it is always
    /// preferred to be implemented by a more semantic way, which is less
    /// error-prone and more robust.
    fn equals(&self, other: &dyn FwNode) -> bool;

    fn prop_read_u32(&self, prop_name: &str) -> Option<u32>;
    fn prop_read_u64(&self, prop_name: &str) -> Option<u64>;
    fn prop_read_str(&self, prop_name: &str) -> Option<String>;
    fn prop_read_present(&self, prop_name: &str) -> bool;
    fn prop_read_raw(&self, prop_name: &str) -> Option<&[u8]>;

    fn interrupt_parent(&self) -> Option<Arc<dyn FwNode>>;
    fn interrupt_info(&self) -> Option<&[u8]>;

    // TODO: add more methods for retrieving information about the hardware, on
    // demand.
}

impl dyn FwNode {
    // If some additional information is indeed hard to be abstracted by the above
    // methods, we have following methods as a plan B:

    /// Try to downcast this firmware node to an OpenFirmwareNode.
    pub fn as_of_node(&self) -> Option<&OpenFirmwareNode> {
        (self as &dyn Any).downcast_ref::<OpenFirmwareNode>()
    }

    // this is not that unreasonable, since there are only a very limited number of
    // such FwNode implementations.
}

impl Debug for dyn FwNode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("dyn FwNode").finish()
    }
}
