//! Here lies the driver framework, and all driver implementations.
//!
//! Most drivers implement the [Driver] trait, but some drivers are for system
//! early initialization and do not fit into the device-driver model, such as
//! those root interrupt controller drivers and system clock source drivers.
//! They won't have a corresponding `dyn Driver` object. (So do their
//! corresponding device objects.)

use core::fmt::Debug;

use crate::{
    device::{
        bus::{platform::PlatformDriver, virtio::VirtIODriver},
        kobject::KObject,
    },
    initcall::{InitCallLevel, run_initcalls},
    prelude::*,
};

// this one must be public for the early boot code to initialize the root
// interrupt controller.
pub mod intc;

mod block;
mod clock_source;
mod power;
mod serial;
pub use serial::ns16550a::Ns16550ARegisters;
mod virtio;

/// Common data shared by all drivers.
#[derive(Debug)]
pub struct DriverBase {
    devices: RwLock<Vec<Arc<dyn Device>>>,
}

impl DriverBase {
    pub fn new() -> Self {
        Self {
            devices: RwLock::new(Vec::new()),
        }
    }
}

pub trait Driver: DriverData + DriverOps {
    fn attach_device(&self, device: Arc<dyn Device>) {
        DriverData::base(self).devices.write_irqsave().push(device);
    }
}

impl dyn Driver {
    pub fn for_each_device<F>(&self, mut f: F)
    where
        F: FnMut(&Arc<dyn Device>),
    {
        for device in DriverData::base(self).devices.read_irqsave().iter() {
            f(device);
        }
    }
}

impl<T: DriverData + DriverOps> Driver for T {}

pub trait DriverData: KObject {
    fn base(&self) -> &DriverBase;
}

pub trait DriverOps {
    fn probe(&self, device: Arc<dyn Device>) -> Result<(), DevError>;

    /// Shutdown the device.
    ///
    /// This method is called when the system is shutting down.
    ///
    /// Note that this does not mean implementations must actually shut down the
    /// device. For example, a driver may choose to simply disable the device's
    /// interrupts and leave it in a quiescent state, or a power-off handler
    /// driver may just do nothing at all. The exact behavior is up to the
    /// driver implementation, and the driver framework does not make any
    /// assumptions on it.
    fn shutdown(&self, device: &dyn Device);

    fn as_platform_driver(&self) -> Option<&dyn PlatformDriver> {
        None
    }

    fn as_virtio_driver(&self) -> Option<&dyn VirtIODriver> {
        None
    }
}

impl Debug for dyn Driver {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("dyn Driver")
            .field("name", &self.name())
            .finish()
    }
}

pub fn init() {
    unsafe {
        run_initcalls(InitCallLevel::Driver);
    }
}
