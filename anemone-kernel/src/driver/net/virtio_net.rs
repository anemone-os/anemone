//! VirtIO network driver.

use virtio_drivers::{device::net::VirtIONet, transport::SomeTransport};

use crate::{
    device::{
        bus::virtio::VirtIODriver,
        kobject::{KObjIdent, KObject, KObjectBase, KObjectOps},
        net::{
            LinkState, NetDev, NetDevClass, NetDevRegistration, NetPhyIo, PhyCapabilities, PhyMedium,
            register_net_device,
        },
    },
    driver::virtio::VirtIOHalImpl,
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

pub const QUEUE_SIZE: usize = 8;
const NET_BUF_LEN: usize = 2048;

pub type VirtIONetDev = VirtIONet<VirtIOHalImpl, SomeTransport<'static>, QUEUE_SIZE>;

/// Wraps virtio-net hardware; exposed to the stack as [`NetDev`].
struct VirtioNetDev {
    inner: Arc<SpinLock<VirtIONetDev>>,
}

struct VirtioNetPhy<'a> {
    dev: &'a mut VirtIONetDev,
}

impl NetPhyIo for VirtioNetPhy<'_> {
    fn try_recv_frame(&mut self) -> Option<Vec<u8>> {
        if !self.dev.can_recv() {
            return None;
        }
        let rx_buf = self.dev.receive().ok()?;
        let data = rx_buf.packet().to_vec();
        if self.dev.recycle_rx_buffer(rx_buf).is_err() {
            kerrln!("net: failed to recycle rx buffer");
            return None;
        }
        Some(data)
    }

    fn can_send(&self) -> bool {
        self.dev.can_send()
    }

    fn send_raw(&mut self, frame: &[u8]) -> Result<(), ()> {
        let mut tx_buf = self.dev.new_tx_buffer(frame.len());
        tx_buf.packet_mut().copy_from_slice(frame);
        self.dev.send(tx_buf).map_err(|_| ())
    }

    fn capabilities(&self) -> PhyCapabilities {
        PhyCapabilities {
            max_transmission_unit: 1514,
            medium: PhyMedium::Ethernet,
        }
    }

    fn ack_interrupt(&mut self) {
        self.dev.ack_interrupt();
    }

    fn disable_interrupts(&mut self) {
        self.dev.disable_interrupts();
    }
}

impl NetDev for VirtioNetDev {
    fn class(&self) -> NetDevClass {
        NetDevClass::Ethernet
    }

    fn mac(&self) -> Option<[u8; 6]> {
        Some(self.inner.lock_irqsave().mac_address())
    }

    fn mtu(&self) -> usize {
        1500
    }

    fn link_state(&self) -> LinkState {
        LinkState::Up
    }

    fn with_phy_mut(&self, f: &mut dyn FnMut(&mut dyn NetPhyIo)) {
        let mut guard = self.inner.lock_irqsave();
        let mut phy = VirtioNetPhy {
            dev: &mut *guard,
        };
        f(&mut phy);
    }
}

#[derive(Opaque)]
struct VirtIONetState {
    netdev: Arc<VirtioNetDev>,
    iface_name: String,
}

impl Clone for VirtIONetState {
    fn clone(&self) -> Self {
        Self {
            netdev: self.netdev.clone(),
            iface_name: self.iface_name.clone(),
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

        let inner = Arc::new(SpinLock::new(net));
        let netdev = Arc::new(VirtioNetDev {
            inner: inner.clone(),
        });

        let dev_name = register_net_device(NetDevRegistration {
            class: NetDevClass::Ethernet,
            device: netdev.clone(),
        })?;

        crate::net::attach_netdev_by_name(dev_name.as_str())?;

        let state = VirtIONetState {
            netdev: netdev.clone(),
            iface_name: dev_name.clone(),
        };

        vdev.request_irq(&IRQ_HANDLER, Some(AnyOpaque::new(state.clone())))?;
        vdev.set_drv_state(AnyOpaque::new(state));

        kinfoln!(
            "virtio-net device {} registered as {}",
            vdev.name(),
            dev_name
        );

        Ok(())
    }

    fn shutdown(&self, device: &dyn Device) {
        let state = device
            .drv_state()
            .cast::<VirtIONetState>()
            .expect("virtio-net device should have VirtIONetState as driver state");
        state
            .netdev
            .with_phy_mut(&mut |phy| phy.disable_interrupts());
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
    state
        .netdev
        .with_phy_mut(&mut |phy| phy.ack_interrupt());
    crate::net::poll_network_for(state.iface_name.as_str());
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
