use idalloc::{Bijection, IdAllocator, StackedAlloc};
use spin::Lazy;

use crate::prelude::*;

int_like!(RawDeviceId, u64);

#[derive(Debug)]
pub struct DeviceId(RawDeviceId);

impl DeviceId {
    pub const unsafe fn from_raw(raw: RawDeviceId) -> Self {
        Self(raw)
    }

    pub const fn raw(&self) -> RawDeviceId {
        self.0
    }
}

#[derive(Debug)]
struct DevIdBijection;

impl Bijection for DevIdBijection {
    type X = u64;

    type Y = RawDeviceId;

    fn forward(x: Self::X) -> Self::Y {
        RawDeviceId::new(x)
    }

    fn backward(y: Self::Y) -> Self::X {
        y.get()
    }
}

#[derive(Debug)]
struct DevIdAllocator {
    inner: IdAllocator<StackedAlloc, DevIdBijection>,
}

impl DevIdAllocator {
    fn new() -> Self {
        Self {
            inner: IdAllocator::new(StackedAlloc::new(0)),
        }
    }

    fn alloc(&mut self) -> Option<DeviceId> {
        self.inner
            .alloc()
            .map(|raw| unsafe { DeviceId::from_raw(raw) })
    }

    unsafe fn dealloc(&mut self, id: RawDeviceId) {
        self.inner.dealloc(id);
    }
}

static DEV_ID_ALLOCATOR: Lazy<SpinLock<DevIdAllocator>> =
    Lazy::new(|| SpinLock::new(DevIdAllocator::new()));

impl Drop for DeviceId {
    fn drop(&mut self) {
        unsafe {
            DEV_ID_ALLOCATOR.lock_irqsave().dealloc(self.0);
        }
    }
}

pub fn alloc_device_id() -> Option<DeviceId> {
    DEV_ID_ALLOCATOR.lock_irqsave().alloc()
}
