use crate::{
    device::{
        kobject::{KObjectBase, KObjectOps},
        resource::Resource,
    },
    prelude::*,
};

#[derive(Debug, KObject, Device)]
pub struct PlatformDevice {
    #[kobject]
    kobj_base: KObjectBase,
    #[device]
    dev_base: DeviceBase,
    resources: Vec<Resource>,
}

impl KObjectOps for PlatformDevice {}

impl DeviceOps for PlatformDevice {}

impl PlatformDevice {
    pub fn new(kobj_base: KObjectBase, dev_base: DeviceBase) -> Self {
        Self {
            kobj_base,
            dev_base,
            resources: Vec::new(),
        }
    }

    pub fn add_resource(&mut self, resource: Resource) {
        self.resources.push(resource);
    }

    pub fn resources(&self) -> &[Resource] {
        &self.resources
    }

    pub fn compatibles(&self) -> impl Iterator<Item = &str> {
        self.fwnode()
            .expect("platform device has no fwnode")
            .as_of_node()
            .expect("platform device not associated with a device tree node")
            .node()
            .compatible()
            .expect("device tree node of a platform device has no compatible property")
    }
}
