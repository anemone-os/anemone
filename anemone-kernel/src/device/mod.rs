//! Device module, containing code for initializing and managing devices.

use core::{any::Any, fmt::Debug};

use crate::{
    device::{bus::platform::PlatformDevice, discovery::fwnode::FwNode, kobject::KObject},
    prelude::*,
    utils::prv_data::PrvData,
};

pub mod discovery;

pub mod bus;
mod cpu;
pub use cpu::CpuArchTrait;
pub mod error;
pub mod kobject;
pub mod resource;

mod idalloc;
pub use idalloc::{DeviceId, RawDeviceId};
//mod registry;

#[derive(Debug)]
pub struct DeviceBase {
    id: DeviceId,
    /// Firmware node associated with this device.
    ///
    /// For most virtual or software-emulated devices, this will be `None`.
    fwnode: Option<Arc<dyn FwNode>>,
    children: RwLock<Vec<Arc<dyn Device>>>,
    driver: RwLock<Option<Arc<dyn Driver>>>,
    drv_state: RwLock<Option<Box<dyn PrvData>>>,
}

impl DeviceBase {
    pub fn new(id: DeviceId, fwnode: Option<Arc<dyn FwNode>>) -> Self {
        Self {
            id,
            fwnode,
            children: RwLock::new(Vec::new()),
            driver: RwLock::new(None),
            drv_state: RwLock::new(None),
        }
    }
}

pub trait DeviceData: KObject {
    fn base(&self) -> &DeviceBase;
}

pub trait DeviceOps {}

impl<T: DeviceData + DeviceOps> Device for T {}

pub trait Device: DeviceData + DeviceOps {
    fn id(&self) -> RawDeviceId {
        DeviceData::base(self).id.raw()
    }

    fn driver(&self) -> Option<Arc<dyn Driver>> {
        DeviceData::base(self).driver.read_irqsave().clone()
    }

    fn set_driver(&self, driver: Option<Arc<dyn Driver>>) {
        *DeviceData::base(self).driver.write_irqsave() = driver;
    }

    fn set_drv_state(&self, state: Option<Box<dyn PrvData>>) {
        *DeviceData::base(self).drv_state.write_irqsave() = state;
    }

    fn add_child(&self, child: Arc<dyn Device>) {
        DeviceData::base(self).children.write_irqsave().push(child);
    }

    fn remove_child(&self, child_id: RawDeviceId) -> Option<Arc<dyn Device>> {
        // Vec is not appropriate here, we shall switch to linked list or something else
        // later
        let mut children = DeviceData::base(self).children.write_irqsave();
        if let Some(pos) = children.iter().position(|c| c.id() == child_id) {
            Some(children.remove(pos))
        } else {
            None
        }
    }

    fn fwnode(&self) -> Option<&dyn FwNode> {
        DeviceData::base(self).fwnode.as_deref()
    }
}

impl dyn Device {
    pub fn with_drv_state<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&dyn PrvData) -> R,
    {
        DeviceData::base(self)
            .drv_state
            .read_irqsave()
            .as_ref()
            .map(|s| f(s.as_ref()))
    }

    pub fn with_drv_state_mut<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&mut dyn PrvData) -> R,
    {
        DeviceData::base(self)
            .drv_state
            .write_irqsave()
            .as_mut()
            .map(|s| f(s.as_mut()))
    }

    pub fn for_each_child<F>(&self, mut f: F)
    where
        F: FnMut(&Arc<dyn Device>),
    {
        for child in DeviceData::base(self).children.read_irqsave().iter() {
            f(child);
        }
    }

    pub fn as_platform_device(&self) -> Option<&PlatformDevice> {
        (self as &dyn Any).downcast_ref::<PlatformDevice>()
    }
}

impl Debug for dyn Device {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let base = DeviceData::base(self);
        Debug::fmt(base, f)
    }
}
