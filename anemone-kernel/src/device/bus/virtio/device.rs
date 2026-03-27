use core::any::Any;

use virtio_drivers::transport::SomeTransport;

use crate::{
    device::{
        bus::platform::PlatformDevice,
        kobject::{KObject, KObjectBase, KObjectOps},
    },
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

#[derive(Debug, KObject, Device)]
pub struct VirtIODevice {
    #[kobject]
    kobj_base: KObjectBase,
    #[device]
    dev_base: DeviceBase,

    device_id: usize,
    vendor_id: usize,

    transport: SpinLock<Option<SomeTransport<'static>>>,
}

impl KObjectOps for VirtIODevice {}

impl DeviceOps for VirtIODevice {}

impl VirtIODevice {
    pub fn new(
        kobj_base: KObjectBase,
        dev_base: DeviceBase,
        device_id: usize,
        vendor_id: usize,
        transport: SomeTransport<'static>,
    ) -> Self {
        Self {
            kobj_base,
            dev_base,
            device_id,
            vendor_id,
            transport: SpinLock::new(Some(transport)),
        }
    }

    /// VirtIO device ID.
    pub fn device_id(&self) -> usize {
        self.device_id
    }

    /// VirtIO device vendor ID.
    pub fn vendor_id(&self) -> usize {
        self.vendor_id
    }

    /// Take the transport object from the device. This is used by virtio
    /// drivers to access virtio device registers and features.
    ///
    /// The transport object will be stored in [VirtIODevice] when transport
    /// layer drivers probe the transport device.
    pub fn take_transport(&self) -> Option<SomeTransport<'static>> {
        self.transport.lock_irqsave().take()
    }

    /// VirtIO devices are created dynamically when transport layer drivers
    /// probe the platform device (virtio-mmio, virtio-pci, etc.), and they do
    /// not own a [device::FwNode]. Instead, these virtio devices must
    /// request interrupts from their parent transport device, which is
    /// responsible for providing the [device::FwNode] and interrupt
    /// information to the irq subsystem.
    pub fn request_irq(
        &self,
        handler: &'static IrqHandler,
        prv_data: Option<AnyOpaque>,
    ) -> Result<(), DevError> {
        let kobj = self
            .parent()
            .expect("virtio device should have parent transport device")
            .upgrade()
            .expect("parent device should be alive");
        let transport_dev = (kobj.as_ref() as &dyn Any)
            .downcast_ref::<PlatformDevice>()
            .expect("transport device should be a platform device");

        request_irq(transport_dev, handler, prv_data)
    }
}
