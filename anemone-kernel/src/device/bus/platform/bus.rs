use core::any::Any;

use crate::{
    device::{
        bus::{BusType, BusTypeBase, platform::PlatformDevice},
        kobject::{KObjIdent, KObjectBase, KObjectOps},
    },
    prelude::*,
};

#[derive(Debug, KObject)]
pub struct PlatformBusType {
    #[kobject]
    kobj_base: KObjectBase,
    busty_base: BusTypeBase,
}

impl PlatformBusType {
    pub fn new(name: KObjIdent) -> Self {
        Self {
            kobj_base: KObjectBase::new(name),
            busty_base: BusTypeBase::new(),
        }
    }
}

impl KObjectOps for PlatformBusType {}

impl BusType for PlatformBusType {
    fn base(&self) -> &BusTypeBase {
        &self.busty_base
    }

    fn matches(&self, device: &dyn Device, driver: &dyn Driver) -> bool {
        // down cast the general Device and Driver to PlatformDevice and PlatformDriver,
        // then compare their compatible strings.
        let pdev = (device as &dyn Any)
            .downcast_ref::<PlatformDevice>()
            .expect("device on platform bus is not a platform device");
        let pdrv = driver
            .as_platform_driver()
            .expect("driver on platform bus is not a platform driver");

        pdev.compatibles()
            .iter()
            .any(|c| pdrv.match_table().iter().any(|&m| c.as_ref() == m))
    }
}
