///! This module implements the PCIe bus.
///
/// # Naming
/// * Structures dedicated to the PCIe bus use **PCIe** as its prefix;
/// * Structures derived from and backward-compatible with legacy PCI use
///   **PCI** as its prefix;
use crate::{
    device::{
        bus::{BusType, pcie::ecam::ClassCode},
        kobject::{KObjIdent, KObject},
    },
    prelude::*,
};

mod bus;
mod device;
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

/// Class code for PCI-to-PCI bridges, which introduces a new PCIe bus.
pub const PCI2PCI_BRIDGE_CLASSCODE: ClassCode = ClassCode {
    base: 0x06,
    sub: 0x04,
    prog_if: 0x00,
};

/// Class code for host bridge devices, which are the root of the PCIe
/// hierarchy.
pub const HOST_BRIDGE_CLASSCODE: ClassCode = ClassCode {
    base: 0x06,
    sub: 0x04,
    prog_if: 0x00,
};

/// Global PCIe bus instance under /sys/bus/pcie.
static PCIE_BUS_TYPE: Lazy<PcieBusType> =
    Lazy::new(|| PcieBusType::new(KObjIdent::try_from("platform").unwrap()));

/// Register a PCIe device on the PCIe bus.
///
/// `device` PCIe device object to register.
pub fn register_device(device: Arc<PcieDevice>) {
    kinfoln!("device {} registered on pcie bus", device.name());
    PCIE_BUS_TYPE.register_device(device);
}

/// Register a PCIe driver on the PCIe bus.
///
/// `driver` PCIe driver object to register.
pub fn register_driver(driver: Arc<dyn PcieDriver>) {
    kinfoln!("driver {} registered on pcie bus", driver.name());
    PCIE_BUS_TYPE.register_driver(driver);
}

/// KUnit helper that prints all registered PCIe devices and drivers.
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
