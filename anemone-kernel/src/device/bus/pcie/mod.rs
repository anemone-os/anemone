//! PCIe bus type, device/driver registration, and bus-wide constants.
//!
//! Naming convention: types specific to PCIe use the `Pcie` prefix; types
//! backward-compatible with legacy PCI use the `Pci` prefix.
use crate::{
    device::{
        bus::{BusType, pcie::ecam::PciClassCode},
        kobject::{KObjIdent, KObject},
    },
    prelude::*,
};

mod bus;
mod device;
pub mod domain;
mod driver;
mod fwnode;
pub mod remap;

pub use bus::PcieBusType;
pub use device::*;
pub use driver::PcieDriver;
pub use fwnode::*;

mod addr;
pub mod ecam;
pub use addr::*;

/// PCI bridge class code (base 0x06, sub 0x04).
pub const CLASSCODE_BRIDGE: PciClassCode = PciClassCode {
    base: 0x06,
    sub: 0x04,
    prog_if: 0x00,
};

/// Host bridge class code (base 0x06, sub 0x00).
pub const CLASSCODE_HOST_BRIDGE: PciClassCode = PciClassCode {
    base: 0x06,
    sub: 0x00,
    prog_if: 0x00,
};

/// Global PCIe bus instance under /sys/bus/pcie.
static PCIE_BUS_TYPE: Lazy<PcieBusType> =
    Lazy::new(|| PcieBusType::new(KObjIdent::try_from("pcie").unwrap()));

/// Register a root PCIe device on the PCIe bus, which is usually a host bridge.
pub fn register_device(device: Arc<PcieDevice>) {
    kinfoln!("device {} registered on pcie bus", device.name());
    PCIE_BUS_TYPE.register_device(device);
}

/// Register a PCIe driver on the PCIe bus.
pub fn register_driver(driver: Arc<dyn PcieDriver>) {
    kinfoln!("driver {} registered on pcie bus", driver.name());
    PCIE_BUS_TYPE.register_driver(driver);
}

/// Prints all registered PCIe devices and drivers.
#[kunit]
fn ls_pcie_bus() {
    kprintln!();
    let bus = &*PCIE_BUS_TYPE;
    kprintln!("pcie bus:");
    (&bus as &PcieBusType as &dyn BusType).for_each_device(|device| {
        kprintln!("  device: {}", device.name());
    });
    (&bus as &PcieBusType as &dyn BusType).for_each_driver(|driver| {
        kprintln!("  driver: {}", driver.name());
    });
}
