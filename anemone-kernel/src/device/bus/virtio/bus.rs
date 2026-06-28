use crate::{
    device::{
        bus::{BusType, BusTypeBase},
        kobject::{KObjIdent, KObjectBase, KObjectOps},
    },
    prelude::*,
};

#[derive(Debug, KObject)]
pub struct VirtIOBusType {
    #[kobject]
    kobj_base: KObjectBase,
    busty_base: BusTypeBase,
}

impl VirtIOBusType {
    pub fn new(name: KObjIdent) -> Self {
        Self {
            kobj_base: KObjectBase::new(name),
            busty_base: BusTypeBase::new(),
        }
    }
}

impl KObjectOps for VirtIOBusType {}

impl BusType for VirtIOBusType {
    fn base(&self) -> &BusTypeBase {
        &self.busty_base
    }

    fn matches(&self, device: &dyn Device, driver: &dyn Driver) -> bool {
        let vdev = device
            .as_virtio_device()
            .expect("device on virtio bus is not a virtio device");
        let vdrv = driver
            .as_virtio_driver()
            .expect("driver on virtio bus is not a virtio driver");

        vdrv.id_table().iter().any(|&id| id == vdev.device_id())
    }
}
