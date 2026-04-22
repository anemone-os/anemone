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
pub mod remap;

pub use bus::PcieBusType;
pub use device::*;
pub use driver::PcieDriver;

mod addr;
pub mod ecam;
pub use addr::*;

/// [PCI2PCI_BRIDGE_CLASSCODE] is the class code used to match PCI-to-PCI bridge
/// devices.
pub const PCI2PCI_BRIDGE_CLASSCODE: ClassCode = ClassCode {
    base: 0x06,
    sub: 0x04,
    prog_if: 0x00,
};

/// [HOST_BRIDGE_CLASSCODE] is the class code used to match host bridge devices.
pub const HOST_BRIDGE_CLASSCODE: ClassCode = ClassCode {
    base: 0x06,
    sub: 0x04,
    prog_if: 0x00,
};

/// [PCIE_BUS_TYPE] is the global PCIe bus instance under /sys/bus/pcie.
static PCIE_BUS_TYPE: Lazy<PcieBusType> =
    Lazy::new(|| PcieBusType::new(KObjIdent::try_from("platform").unwrap()));

/// [register_device] registers a PCIe device on the PCIe bus.
///
/// `device` is the PCIe device object to be registered.
pub fn register_device(device: Arc<PcieDevice>) {
    kinfoln!("device {} registered on pcie bus", device.name());
    PCIE_BUS_TYPE.register_device(device);
}

/// [register_driver] registers a PCIe driver on the PCIe bus.
///
/// `driver` is the PCIe driver object to be registered.
pub fn register_driver(driver: Arc<dyn PcieDriver>) {
    kinfoln!("driver {} registered on pcie bus", driver.name());
    PCIE_BUS_TYPE.register_driver(driver);
}

/// [ls_pcie_bus] is a KUnit helper that prints all registered PCIe devices and
/// drivers.
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
