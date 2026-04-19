//! MMIO transport layer for VirtIO devices.
//!
//! The tranport driver itself is is a platform driver.
//!
//! Reference:
//! - https://www.kernel.org/doc/Documentation/devicetree/bindings/virtio/mmio.txt

use virtio_drivers::transport::{
    SomeTransport, Transport,
    mmio::{MmioTransport, VirtIOHeader},
};

/// This struct will be set to `drv_state` field of the platform device
/// representing the VirtIO MMIO transport, such that the Mmio remap will remain
/// valid as long as the transport device exists. The actual VirtIO device will
/// hold the transport and use it to access the device registers.
#[derive(Debug, Opaque)]
struct MmioTransportState {
    remap: IoRemap,
}

use crate::{
    device::{
        bus::{
            platform::{self, PlatformDriver},
            virtio::VirtIODevice,
        },
        discovery::fwnode::FwNode,
        kobject::{KObjIdent, KObject, KObjectBase, KObjectOps},
        resource::Resource,
    },
    mm::remap::{IoRemap, ioremap},
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

#[derive(Debug, KObject, Driver)]
struct MmioTransportDriver {
    #[kobject]
    kobj_base: KObjectBase,
    #[driver]
    drv_base: DriverBase,
}

impl KObjectOps for MmioTransportDriver {}

impl DriverOps for MmioTransportDriver {
    fn probe(&self, device: Arc<dyn Device>) -> Result<(), SysError> {
        let pdev = device
            .as_platform_device()
            .expect("platform driver should only be probed with platform device");

        let of_node = pdev
            .fwnode()
            .ok_or(SysError::MissingFwNode)?
            .as_of_node()
            .ok_or(SysError::DriverIncompatible)?;

        if of_node.prop_read_present("iommus") {
            kwarningln!("iommu is not supported for now.");
            return Err(SysError::DriverIncompatible);
        }

        let (base, len) = pdev
            .resources()
            .iter()
            .find_map(|resource| match resource {
                Resource::Mmio { base, len } => Some((*base, *len)),
            })
            .ok_or(SysError::MissingResource)?;

        let remap = unsafe { ioremap(base, len) }?;

        let virtio_hdr = remap.as_ptr().cast::<VirtIOHeader>();

        {
            match unsafe { MmioTransport::new(virtio_hdr, len) } {
                Ok(transport) => {
                    kinfoln!(
                        "detected virtio mmio device: vendor id: {:#x}, device type: {:?}, version: {:?}",
                        transport.vendor_id(),
                        transport.device_type(),
                        transport.version()
                    );

                    let state = MmioTransportState { remap };
                    pdev.set_drv_state(AnyOpaque::new(state));

                    // create virtio device and attach it to virtio bus
                    let kobj_base = KObjectBase::new(ident_format!("{}", pdev.name()).unwrap());
                    let dev_base = DeviceBase::new(None);
                    let mut vdev = VirtIODevice::new(
                        kobj_base,
                        dev_base,
                        transport.device_type() as usize,
                        transport.vendor_id() as usize,
                        SomeTransport::Mmio(transport),
                    );

                    vdev.set_parent(Some(device.clone()));

                    let vdev = Arc::new(vdev);
                    device.add_child(vdev.clone());

                    kinfoln!("{}: probed", pdev.name());
                    bus::virtio::register_device(vdev);
                },
                Err(e) => {
                    kwarningln!("failed to initialize VirtIO MMIO transport: {e}");
                    return Err(SysError::DriverIncompatible);
                },
            }
        }

        Ok(())
    }

    fn shutdown(&self, device: &dyn Device) {}

    fn as_platform_driver(&self) -> Option<&dyn PlatformDriver> {
        Some(self)
    }
}

impl PlatformDriver for MmioTransportDriver {
    fn match_table(&self) -> &[&str] {
        &["virtio,mmio"]
    }
}

#[initcall(driver)]
fn init() {
    let kobj_base = KObjectBase::new(KObjIdent::try_from("virtio-mmio-transport").unwrap());
    let drv_base = DriverBase::new();
    let driver = Arc::new(MmioTransportDriver {
        kobj_base,
        drv_base,
    });
    platform::register_driver(driver);
}
