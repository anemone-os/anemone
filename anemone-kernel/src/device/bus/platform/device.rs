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
    compatibles: Vec<Box<str>>,
}

impl KObjectOps for PlatformDevice {}

impl DeviceOps for PlatformDevice {}

impl PlatformDevice {
    pub fn new(kobj_base: KObjectBase, dev_base: DeviceBase) -> Self {
        Self {
            kobj_base,
            dev_base,
            resources: Vec::new(),
            compatibles: Vec::new(),
        }
    }

    pub fn add_resource(&mut self, resource: Resource) {
        self.resources.push(resource);
    }

    pub fn add_compatible(&mut self, compatible: impl Into<Box<str>>) {
        self.compatibles.push(compatible.into());
    }

    pub fn resources(&self) -> &[Resource] {
        &self.resources
    }

    pub fn compatibles(&self) -> &[Box<str>] {
        &self.compatibles
    }
}
