use crate::{device::bus::pcie::ecam::ClassCode, driver::Driver};

/// PCIe bus driver trait.
pub trait PcieDriver: Driver {
    /// [class_code_table] returns all class codes supported by this PCIe driver.
    fn class_code_table(&self) -> &[ClassCode];
}
