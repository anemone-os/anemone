//! VirtIO-Net driver registration, publication, and shutdown.

use core::mem::size_of;

use anemone_net_api::FrameProvider;
use virtio_drivers::{device::net::VirtioNetHdr, transport::DeviceType};

use crate::{
    device::{
        bus::virtio::VirtIODriver,
        kobject::{KObjIdent, KObject, KObjectBase, KObjectOps},
        net::{PublishError, PublishedNetdev, ReadyNetdev, publish},
    },
    prelude::*,
    utils::{any_opaque::AnyOpaque, identity::AnyIdentity},
};

mod device;
mod frame;

pub(crate) use frame::VirtIONetProvider;

const QUEUE_SIZE: usize = VIRTIO_NET_QUEUE_SIZE;
const BACKING_CAPACITY: usize = VIRTIO_NET_FRAME_CAPACITY_BYTES;
const HEADER_RESERVE: usize = size_of::<VirtioNetHdr>();
const FRAME_CAPACITY: usize = BACKING_CAPACITY - HEADER_RESERVE;
const RX_SLOT_COUNT: usize = QUEUE_SIZE / 2;
const TX_SLOT_COUNT: usize = QUEUE_SIZE / 2;

static_assert!(
    QUEUE_SIZE >= 4 && QUEUE_SIZE <= 1024,
    "VIRTIO_NET_QUEUE_SIZE must be in 4..=1024"
);
static_assert!(
    QUEUE_SIZE.is_power_of_two(),
    "VIRTIO_NET_QUEUE_SIZE must be a power of two"
);
static_assert!(
    BACKING_CAPACITY >= 1526,
    "VIRTIO_NET_FRAME_CAPACITY_BYTES must satisfy VirtIONetRaw's 1526-byte minimum"
);
static_assert!(
    BACKING_CAPACITY > HEADER_RESERVE,
    "VirtIO-Net backing must leave room for an Ethernet frame"
);

#[derive(Opaque)]
struct VirtIONetState {
    /// Non-owning shutdown capability. The provider is the only durable strong
    /// owner so driver state cannot keep RawNet alive beyond its slot backing.
    device: Weak<device::VirtIONetDevice>,
    /// One-shot typed capability, not a second lifecycle truth. Checkpoint 4
    /// moves it to the attach authority before any protocol callback runs.
    published: SpinLock<Option<PublishedNetdev<VirtIONetProvider>>>,
}

#[derive(Opaque)]
struct VirtIONetIrq {
    /// Non-owning IRQ capability. See `VirtIONetProvider` for the boot-only
    /// lifetime rule that prevents an upgrade from racing provider teardown.
    device: Weak<device::VirtIONetDevice>,
}

#[derive(Debug, KObject, Driver)]
struct VirtIONetDriver {
    #[kobject]
    kobj_base: KObjectBase,
    #[driver]
    drv_base: DriverBase,
}

impl KObjectOps for VirtIONetDriver {}

impl DriverOps for VirtIONetDriver {
    fn probe(&self, device: Arc<dyn Device>) -> Result<(), SysError> {
        let vdev = device
            .as_virtio_device()
            .expect("VirtIO-Net driver must only receive a VirtIO device");
        let transport = vdev.take_transport().ok_or(SysError::ProbeFailed)?;
        let (provider, mac) = VirtIONetProvider::new(transport).map_err(|error| {
            kerrln!("failed to initialize VirtIO-Net queues: {error}");
            SysError::ProbeFailed
        })?;
        let device = Arc::downgrade(&provider.device);

        vdev.request_irq(
            &IRQ_HANDLER,
            Some(AnyOpaque::new(VirtIONetIrq {
                device: device.clone(),
            })),
        )?;
        provider.device.enable_interrupts();

        let origin = AnyIdentity::try_from(vdev.name())
            .expect("VirtIO device name must fit the kernel identity limit");
        let ready = ReadyNetdev::new(
            origin,
            Some(mac),
            provider.capabilities(),
            provider.link_state(),
            provider,
        );
        let published = publish(ready).map_err(|(error, ready)| {
            // IRQ registration is not removable. Retain the ready provider so
            // its RX mappings/backing stay valid until reset instead of
            // freeing memory that the device may still access. Suppress further
            // queue notifications first so the failed device remains inert.
            if let Some(device) = device.upgrade() {
                device.disable_interrupts();
            }
            core::mem::forget(ready);
            match error {
                PublishError::DuplicateOrigin => {
                    kerrln!("VirtIO-Net device {} was published twice", vdev.name());
                },
                PublishError::IdentityExhausted => {
                    kerrln!("network-device identity space exhausted");
                },
                PublishError::NameTooLong => {
                    kerrln!("network-device name exceeded its capacity");
                },
            }
            SysError::ProbeFailed
        })?;
        let snapshot = published.snapshot();
        kinfoln!(
            "VirtIO-Net {} published as {} (ifindex {}, MAC {:?}, frame capacity {})",
            vdev.name(),
            snapshot.name(),
            snapshot.ifindex(),
            snapshot.facts().ethernet_address,
            snapshot.facts().max_frame_len,
        );

        vdev.set_drv_state(AnyOpaque::new(VirtIONetState {
            device,
            published: SpinLock::new(Some(published)),
        }));
        Ok(())
    }

    fn shutdown(&self, device: &dyn Device) {
        let state = device
            .drv_state()
            .cast::<VirtIONetState>()
            .expect("VirtIO-Net device must carry VirtIONetState");
        if let Some(device) = state.device.upgrade() {
            device.disable_interrupts();
        }
    }

    fn as_virtio_driver(&self) -> Option<&dyn VirtIODriver> {
        Some(self)
    }
}

impl VirtIODriver for VirtIONetDriver {
    fn id_table(&self) -> &'static [usize] {
        &[DeviceType::Network as usize]
    }
}

static IRQ_HANDLER: IrqHandler = IrqHandler::new(irq_handler);

static VIRTIO_NET_DRIVER: Lazy<Arc<VirtIONetDriver>> = Lazy::new(|| {
    Arc::new(VirtIONetDriver {
        kobj_base: KObjectBase::new(KObjIdent::try_from("virtio-net").unwrap()),
        drv_base: DriverBase::new(),
    })
});

pub(crate) fn take_published_netdevs() -> Vec<PublishedNetdev<VirtIONetProvider>> {
    let mut published = Vec::new();
    let driver: &dyn Driver = VIRTIO_NET_DRIVER.as_ref();
    driver.for_each_device(|device| {
        let state = device
            .drv_state()
            .cast::<VirtIONetState>()
            .expect("VirtIO-Net device must carry VirtIONetState");
        if let Some(netdev) = state.published.lock().take() {
            published.push(netdev);
        }
    });
    published
}

fn irq_handler(prv_data: &AnyOpaque) {
    let irq = prv_data
        .cast::<VirtIONetIrq>()
        .expect("VirtIO-Net IRQ must carry VirtIONetIrq");
    if let Some(device) = irq.device.upgrade() {
        device.handle_irq();
    }
}

#[initcall(driver)]
fn init() {
    bus::virtio::register_driver(VIRTIO_NET_DRIVER.clone());
}
