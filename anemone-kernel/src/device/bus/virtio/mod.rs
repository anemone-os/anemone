mod bus;
pub use bus::VirtIOBusType;
mod device;
pub use device::VirtIODevice;
mod driver;
pub use driver::VirtIODriver;

use crate::{
    device::{
        bus::BusType,
        kobject::{KObjIdent, KObject},
    },
    prelude::*,
};


/// /sys/bus/virtio
static VIRTIO_BUS_TYPE: Lazy<RwLock<VirtIOBusType>> =
    Lazy::new(|| RwLock::new(VirtIOBusType::new(KObjIdent::try_from("virtio").unwrap())));

/// Register a VirtIO device to the VirtIO bus.
pub fn register_device(device: Arc<VirtIODevice>) {
    kinfoln!("device {} registered on virtio bus", device.name());
    VIRTIO_BUS_TYPE.write_irqsave().register_device(device);
}

/// Register a VirtIO driver to the VirtIO bus.
pub fn register_driver(driver: Arc<dyn VirtIODriver>) {
    kinfoln!("driver {} registered on virtio bus", driver.name());
    VIRTIO_BUS_TYPE.write_irqsave().register_driver(driver);
}

#[kunit]
fn ls_virtio_devices() {
    kprintln!();
    let bus = VIRTIO_BUS_TYPE.read_irqsave();
    kprintln!("virtio bus:");
    (&bus as &VirtIOBusType as &dyn BusType).for_each_device(|device| {
        kprintln!("  device: {}", device.name());
    });
    (&bus as &VirtIOBusType as &dyn BusType).for_each_driver(|driver| {
        kprintln!("  driver: {}", driver.name());
    });
}
