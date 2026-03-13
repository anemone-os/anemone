//! Platform bus.

use spin::Lazy;

use crate::{
    device::{bus::BusType, kobject::KObjIdent},
    prelude::*,
};

mod bus;
pub use bus::PlatformBusType;
mod device;
pub use device::PlatformDevice;
mod driver;
pub use driver::PlatformDriver;

/// /sys/bus/platform
static PLATFORM_BUS_TYPE: Lazy<Arc<RwLock<PlatformBusType>>> = Lazy::new(|| {
    Arc::new(RwLock::new(PlatformBusType::new(
        KObjIdent::try_from("platform").unwrap(),
    )))
});

pub fn register_device(device: Arc<dyn Device>) {
    kinfoln!("device {} registered on platform bus", device.name());
    PLATFORM_BUS_TYPE.write_irqsave().register_device(device);
}

pub fn register_driver(driver: Arc<dyn Driver>) {
    kinfoln!("driver {} registered on platform bus", driver.name());
    PLATFORM_BUS_TYPE.write_irqsave().register_driver(driver);
}

#[kunit]
fn ls_platform_bus() {
    kprintln!();
    let bus = PLATFORM_BUS_TYPE.read_irqsave();
    kprintln!("platform bus:");
    (&bus as &PlatformBusType as &dyn BusType).for_each_device(|device| {
        kprintln!("  device: {}", device.name());
    });
    (&bus as &PlatformBusType as &dyn BusType).for_each_driver(|driver| {
        kprintln!("  driver: {}", driver.name());
    });
}
