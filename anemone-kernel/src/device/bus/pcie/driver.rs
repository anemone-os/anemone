use alloc::sync::Arc;

use crate::{
    device::{Device, bus::pcie::ecam::ClassCode},
    driver::Driver,
    prelude::SysError,
};

/// PCIe driver interface for matching and optional initialization hooks.
pub trait PcieDriver: Driver {
    /// Return all class codes supported by this PCIe driver.
    ///
    /// When both class-code and vendor-device tables are non-empty, match
    /// vendor-device entries first.
    fn class_code_table(&self) -> &[ClassCode];

    /// Return all supported `(vendor id, device id)` pairs for this PCIe
    /// driver.
    ///
    /// When both class-code and vendor-device tables are non-empty, match
    /// vendor-device entries first.
    fn vendor_device_table(&self) -> &[(u16, u16)];

    /// Perform optional post-initialization for `device` after bus preinit and
    /// before probing.
    fn postinit(&self, device: Arc<dyn Device>) -> Result<(), SysError> {
        Ok(())
    }
}
