use kernel_macros::KObject;

use crate::{
    device::{
        Device,
        bus::{BusType, BusTypeBase},
        kobject::{KObjIdent, KObjectBase, KObjectOps},
    },
    prelude::*,
};

#[derive(Debug, KObject)]
pub struct PcieBusType {
    #[kobject]
    kobj_base: KObjectBase,
    busty_base: BusTypeBase,
}

impl PcieBusType {
    pub fn new(name: KObjIdent) -> Self {
        Self {
            kobj_base: KObjectBase::new(name),
            busty_base: BusTypeBase::new(),
        }
    }
}

impl KObjectOps for PcieBusType {}

impl BusType for PcieBusType {
    fn base(&self) -> &BusTypeBase {
        &self.busty_base
    }

    fn matches(&self, device: &dyn Device, driver: &dyn Driver) -> bool {
        let pdev = device
            .as_pcie_device()
            .expect("device on PCIe bus is not a PCIe device");
        let pdrv = driver
            .as_pcie_driver()
            .expect("driver on PCIe bus is not a PCIe driver");

        let class_code = pdev.class_code();
        pdrv.class_code_table().iter().any(|&m| class_code == m)
    }
}
