//! Firmware-node wrapper for PCIe devices. Carries interrupt mapping info
//! from the device tree into the device model.

use core::any::Any;

use crate::device::{bus::pcie::domain::PcieIntrInfo, discovery::fwnode::FwNode};

/// Firmware node for a PCIe device, carrying optional interrupt routing info.
#[derive(Debug)]
pub struct PcieFwNode {
    intr: Option<PcieIntrInfo>,
}

impl PcieFwNode {
    /// Create a firmware node with optional interrupt info.
    pub fn new(intr: Option<PcieIntrInfo>) -> Self {
        Self { intr }
    }
}

impl FwNode for PcieFwNode {
    fn equals(&self, other: &dyn FwNode) -> bool {
        (other as &dyn Any)
            .downcast_ref::<PcieFwNode>()
            .map(|o| self.intr == o.intr)
            .unwrap_or(false)
    }

    fn prop_read_u32(&self, prop_name: &str) -> Option<u32> {
        None
    }

    fn prop_read_u64(&self, prop_name: &str) -> Option<u64> {
        None
    }

    fn prop_read_str(&self, prop_name: &str) -> Option<alloc::string::String> {
        None
    }

    fn prop_read_present(&self, prop_name: &str) -> bool {
        false
    }

    fn prop_read_raw(&self, prop_name: &str) -> Option<&[u8]> {
        None
    }

    fn interrupt_parent(&self) -> Option<alloc::sync::Arc<dyn FwNode>> {
        self.intr.as_ref().map(|intr| intr.parent.clone())
    }

    fn interrupt_info(&self) -> Option<&[u8]> {
        self.intr
            .as_ref()
            .map(|intr| (&intr.parent_intr_spec).as_ref())
    }

    fn is_stdout(&self) -> bool {
        false
    }
}
