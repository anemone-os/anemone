//! Bus matching for cards whose protocol identity has already been committed.

use super::{MmcCardDevice, MmcCardKind};
use crate::{
    device::{
        bus::{BusType, BusTypeBase},
        kobject::{KObjIdent, KObject, KObjectBase, KObjectOps},
    },
    prelude::*,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MmcCardMatch {
    pub kind: MmcCardKind,
}

pub trait MmcCardDriver: Driver {
    fn match_table(&self) -> &[MmcCardMatch];
}

#[derive(Debug, KObject)]
struct MmcBusType {
    #[kobject]
    kobj_base: KObjectBase,
    bus_base: BusTypeBase,
}

impl MmcBusType {
    fn new() -> Self {
        Self {
            kobj_base: KObjectBase::new(KObjIdent::try_from("mmc").unwrap()),
            bus_base: BusTypeBase::new(),
        }
    }
}

impl KObjectOps for MmcBusType {}

impl BusType for MmcBusType {
    fn base(&self) -> &BusTypeBase {
        &self.bus_base
    }

    fn matches(&self, device: &dyn Device, driver: &dyn Driver) -> bool {
        let card = device
            .as_mmc_card_device()
            .expect("device on MMC bus is not an MMC card");
        let driver = driver
            .as_mmc_card_driver()
            .expect("driver on MMC bus is not an MMC card driver");
        driver
            .match_table()
            .iter()
            .any(|entry| entry.kind == card.kind())
    }
}

static MMC_BUS_TYPE: Lazy<RwLock<MmcBusType>> = Lazy::new(|| RwLock::new(MmcBusType::new()));

pub fn register_device(device: Arc<MmcCardDevice>) {
    kinfoln!(
        "device {} registered on mmc bus (kind={:?})",
        device.name(),
        device.kind()
    );
    MMC_BUS_TYPE.write_irqsave().register_device(device);
}

pub fn register_driver(driver: Arc<dyn MmcCardDriver>) {
    kdebugln!("driver {} registered on mmc bus", driver.name());
    MMC_BUS_TYPE.write_irqsave().register_driver(driver);
}
