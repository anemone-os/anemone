pub mod platform;

use core::{any::Any, fmt::Debug};

use spin::Lazy;

use crate::{
    device::{
        bus::platform::PlatformDevice,
        idalloc::alloc_device_id,
        kobject::{KObjIdent, KObject, KObjectBase, KSet},
    },
    prelude::*,
};

#[derive(Debug)]
pub struct BusTypeBase {
    devices: RwLock<KSet<dyn Device>>,
    drivers: RwLock<KSet<dyn Driver>>,
}

impl BusTypeBase {
    pub fn new() -> Self {
        Self {
            devices: RwLock::new(KSet::new(KObjIdent::try_from("devices").unwrap())),
            drivers: RwLock::new(KSet::new(KObjIdent::try_from("drivers").unwrap())),
        }
    }
}

pub trait BusType: KObject {
    fn base(&self) -> &BusTypeBase;
    fn matches(&self, device: &dyn Device, driver: &dyn Driver) -> bool;

    fn register_device(&mut self, device: Arc<dyn Device>) {
        for driver in BusType::base(self).drivers.read_irqsave().iter() {
            if self.matches(device.as_ref(), driver.as_ref()) {
                // TODO: probe defer
                driver.attach_device(device.clone());
                device.set_driver(Some(driver.clone()));
                driver.probe(device.clone()).unwrap_or_else(|e| {
                    kerrln!(
                        "failed to probe device {} with driver {}: {:?}",
                        device.name(),
                        driver.name(),
                        e
                    );
                });
                break;
            }
        }
        BusType::base(self)
            .devices
            .write_irqsave()
            .add_kobject(device);
    }

    fn register_driver(&mut self, driver: Arc<dyn Driver>) {
        for device in BusType::base(self).devices.read_irqsave().iter() {
            if self.matches(device.as_ref(), driver.as_ref()) {
                // TODO: probe defer
                driver.attach_device(device.clone());
                device.set_driver(Some(driver.clone()));
                driver.probe(device.clone()).unwrap_or_else(|e| {
                    kerrln!(
                        "failed to probe device {} with driver {}: {:?}",
                        device.name(),
                        driver.name(),
                        e
                    );
                    // TODO: reclaim resources and cleanup state
                });
                break;
            }
        }
        BusType::base(self)
            .drivers
            .write_irqsave()
            .add_kobject(driver);
    }
    // currently we don't support detaching.
}

impl dyn BusType {
    pub fn for_each_device<F>(&self, mut f: F)
    where
        F: FnMut(&Arc<dyn Device>),
    {
        for device in BusType::base(self).devices.read_irqsave().iter() {
            f(device);
        }
    }

    pub fn for_each_driver<F>(&self, mut f: F)
    where
        F: FnMut(&Arc<dyn Driver>),
    {
        for driver in BusType::base(self).drivers.read_irqsave().iter() {
            f(driver);
        }
    }
}

impl Debug for dyn BusType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let base = BusType::base(self);
        Debug::fmt(base, f)
    }
}

/// /sys/devices/platform
pub static ROOT_BUS: Lazy<Arc<PlatformDevice>> = Lazy::new(|| {
    Arc::new(PlatformDevice::new(
        KObjectBase::new(KObjIdent::try_from("platform").unwrap()),
        DeviceBase::new(alloc_device_id().unwrap(), None),
    ))
});

// TODO: implement /sys/bus/
// currently we have no file system and can't create symlinks.
// so only /sys/devices/ is available, and all dicovered devices are put under
// /sys/devices/platform/. in the future when we have a more complete file
// system, we can create /sys/bus/, /sys/class/, etc. and create symlinks to
// devices and drivers accordingly.

#[kunit]
fn ls_devices() {
    fn ls_devices_inner(device: &dyn Device, prefix: &str) {
        kprintln!("{}{}", prefix, device.name());
        let new_prefix = format!("{}{}/", prefix, device.name());
        if let Some(pdev) = (device as &dyn Any).downcast_ref::<PlatformDevice>() {
            kprintln!("\tresources: {:x?}", pdev.resources());
        }
        device.for_each_child(|child| {
            ls_devices_inner(child.as_ref(), &new_prefix);
        });
    }
    kprintln!();
    ls_devices_inner(ROOT_BUS.as_ref(), "");
}
