//! VirtIO network driver.

use virtio_drivers::{device::net::VirtIONet, transport::SomeTransport};

use crate::{
    device::{
        bus::virtio::VirtIODriver,
        kobject::{KObjIdent, KObject, KObjectBase, KObjectOps},
    },
    driver::virtio::VirtIOHalImpl,
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

pub const QUEUE_SIZE: usize = 8;
const NET_BUF_LEN: usize = 2048;

pub type VirtIONetDev = VirtIONet<VirtIOHalImpl, SomeTransport<'static>, QUEUE_SIZE>;

#[derive(Opaque)]
struct VirtIONetState {
    net: Arc<SpinLock<VirtIONetDev>>,
}

impl Clone for VirtIONetState {
    fn clone(&self) -> Self {
        Self {
            net: self.net.clone(),
        }
    }
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
    fn probe(&self, device: Arc<dyn Device>) -> Result<(), DevError> {
        let vdev = device
            .as_virtio_device()
            .expect("virtio driver should only be probed with virtio device");

        let transport = vdev
            .take_transport()
            .expect("virtio device should have transport");

        let net = VirtIONet::<VirtIOHalImpl, _, QUEUE_SIZE>::new(transport, NET_BUF_LEN).map_err(
            |e| {
                kerrln!("failed to initialize virtio-net device: {e}");
                DevError::ProbeFailed
            },
        )?;

        let mac = net.mac_address();
        let dev_name = device::net::register_net_device(mac, 1500);

        let state = VirtIONetState {
            net: Arc::new(SpinLock::new(net)),
        };

        vdev.request_irq(&IRQ_HANDLER, Some(AnyOpaque::new(state.clone())))?;
        vdev.set_drv_state(AnyOpaque::new(state.clone()));

        kinfoln!(
            "virtio-net device {} registered as {}",
            vdev.name(),
            dev_name
        );

        crate::net::attach_device(state.net.clone(), mac);

        Ok(())
    }

    fn shutdown(&self, device: &dyn Device) {
        let state = device
            .drv_state()
            .cast::<VirtIONetState>()
            .expect("virtio-net device should have VirtIONetState as driver state");
        state.net.lock_irqsave().disable_interrupts();
    }

    fn as_virtio_driver(&self) -> Option<&dyn VirtIODriver> {
        Some(self)
    }
}

impl VirtIODriver for VirtIONetDriver {
    fn id_table(&self) -> &'static [usize] {
        &[virtio_drivers::transport::DeviceType::Network as usize]
    }
}

static IRQ_HANDLER: IrqHandler = IrqHandler::new(irq_handler);

fn irq_handler(prv_data: &AnyOpaque) {
    let state = prv_data
        .cast::<VirtIONetState>()
        .expect("virtio-net irq: invalid private data");
    state.net.lock_irqsave().ack_interrupt();
    crate::net::poll_network();
}

#[initcall(driver)]
fn init() {
    let kobj_base = KObjectBase::new(KObjIdent::try_from("virtio-net").unwrap());
    let drv_base = DriverBase::new();
    let driver = Arc::new(VirtIONetDriver {
        kobj_base,
        drv_base,
    });
    bus::virtio::register_driver(driver);
}
