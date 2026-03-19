//! Device module, containing code for initializing and managing devices.

use core::{any::Any, fmt::Debug};

use crate::{
    device::{
        bus::{platform::PlatformDevice, virtio::VirtIODevice},
        discovery::fwnode::FwNode,
        kobject::{KObjIdent, KObject, KObjectBase},
    },
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

pub mod devnum;
pub use devnum::{DevNum, MajorNum, MinorNum};

// subsystems
pub mod block;
pub mod char;
pub mod console;

/// Common data shared by all devices.
///
/// **LOCK ORDERING**:
/// **`drv_state` -> `children` -> `driver`**
#[derive(Debug)]
pub struct DeviceBase {
    /// Firmware node associated with this device.
    ///
    /// For most virtual or software-emulated devices, this will be `None`.
    fwnode: Option<Arc<dyn FwNode>>,
    children: RwLock<Vec<Arc<dyn Device>>>,
    driver: RwLock<Option<Arc<dyn Driver>>>,
    drv_state: RwLock<Option<Box<dyn PrvData>>>,
}

impl DeviceBase {
    pub fn new(fwnode: Option<Arc<dyn FwNode>>) -> Self {
        Self {
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

    pub fn as_virtio_device(&self) -> Option<&VirtIODevice> {
        (self as &dyn Any).downcast_ref::<VirtIODevice>()
    }
}

impl Debug for dyn Device {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("dyn Device")
            .field("name", &self.name())
            .finish()
    }
}

/// /sys/devices
pub static ROOT: Lazy<Arc<PlatformDevice>> = Lazy::new(|| {
    Arc::new(PlatformDevice::new(
        KObjectBase::new(KObjIdent::try_from("devices").unwrap()),
        DeviceBase::new(None),
    ))
});

/// Shutdown all devices. Called before powering off or rebooting the system.
///
/// Internally, this function will do a depth-first traversal of the device tree
/// and call the shutdown method of each device's driver. This ensures that
/// child devices are shut down before their parents, which is important for
/// proper resource cleanup and to avoid potential issues with dependencies
/// between devices.
///
/// This notifies all drivers to clean up their state.
/// - For block devices, this will flush all pending writes to the storage
///   device.
/// - For network devices, this will close all network connections and release
///   all buffers.
/// - For USB devices, this will send USB reset signals to the devices, etc.
pub unsafe fn shutdown() {
    fn shutdown_from(parent: &dyn Device) {
        parent.for_each_child(|child| {
            shutdown_from(child.as_ref());
        });
        if let Some(driver) = parent.driver() {
            driver.shutdown(parent);
            knoticeln!("{}: shutdown", parent.name());
        }
    }
    shutdown_from(ROOT.as_ref());
}

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
    ls_devices_inner(ROOT.as_ref(), "");
}
