use alloc::sync::Arc;

use crate::{
    device::{Device, bus::pcie::ecam::ClassCode},
    driver::Driver,
    prelude::SysError,
};

/// PCIe bus driver trait.
pub trait PcieDriver: Driver {
    /// [class_code_table] returns all class codes supported by this PCIe
    /// driver.
    ///
    /// When both class code table and vendor-device table are non-empty,
    /// vendor-device table is matched first, then class code table is matched
    /// if no match is found in vendor-device table.
    fn class_code_table(&self) -> &[ClassCode];

    /// [vendor_device_table] returns all (vendor id, device id) pairs supported
    /// by this PCIe driver.
    ///
    /// When both class code table and vendor-device table are non-empty,
    /// vendor-device table is matched first, then class code table is matched
    /// if no match is found in vendor-device table.
    fn vendor_device_table(&self) -> &[(u16, u16)];

    /// [preinit] is called when building the pcie device tree after
    /// [super::bus::preinit_pci_dev()] is called, before any drivers are
    /// probed.
    fn postinit(&self, device: Arc<dyn Device>) -> Result<(), SysError> {
        Ok(())
    }
}
