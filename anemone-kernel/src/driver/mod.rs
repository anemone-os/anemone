use core::{any::Any, fmt::Debug};

use crate::{
    device::{bus::platform::PlatformDriver, kobject::KObject},
    initcall::{InitCallLevel, run_initcalls},
    prelude::*,
};

pub mod clock_source;
pub mod serial;

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

impl<T: DriverData + DriverOps> Driver for T {}

pub trait DriverData: KObject {
    fn base(&self) -> &DriverBase;
}

pub trait DriverOps {
    fn probe(&self, device: Arc<dyn Device>) -> Result<(), DevError>;

    fn as_platform_driver(&self) -> Option<&dyn PlatformDriver> {
        None
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

impl Debug for dyn Driver {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let base = DriverData::base(self);
        Debug::fmt(base, f)
    }
}

pub trait DriverState: Any + Send + Sync {}

impl Debug for dyn DriverState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "DriverState {{ ... }}")
    }
}

pub fn init() {
    unsafe {
        run_initcalls(InitCallLevel::Driver);
    }
}
