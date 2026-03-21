pub mod platform;
pub mod virtio;

use core::fmt::Debug;

use crate::{
    device::kobject::{KObjIdent, KObject, KSet},
    prelude::*,
};

/// Common data shared by all bus types.
///
/// **LOCK ORDERING**:
/// **`devices` -> `drivers`**
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
                match driver.probe(device.clone()) {
                    Ok(()) => {
                        device.set_driver(Some(driver.clone()));
                        driver.attach_device(device.clone());
                    },
                    Err(e) => {
                        kerrln!(
                            "failed to probe device {} with driver {}: {:?}",
                            device.name(),
                            driver.name(),
                            e
                        );
                    },
                }
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
            if device.driver().is_some() {
                continue;
            }

            if self.matches(device.as_ref(), driver.as_ref()) {
                // TODO: probe defer
                match driver.probe(device.clone()) {
                    Ok(()) => {
                        device.set_driver(Some(driver.clone()));
                        driver.attach_device(device.clone());
                    },
                    Err(e) => {
                        kerrln!(
                            "failed to probe device {} with driver {}: {:?}",
                            device.name(),
                            driver.name(),
                            e
                        );
                        // TODO: reclaim resources and cleanup state
                    },
                }
                break;
            }
        }
        BusType::base(self)
            .drivers
            .write_irqsave()
            .add_kobject(driver);
    }
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
        f.debug_struct("dyn BusType")
            .field("name", &self.name())
            .finish()
    }
}
