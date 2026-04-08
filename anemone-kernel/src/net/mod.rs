//! Minimal kernel network stack built on smoltcp.
//!
//! This module adapts smoltcp to the kernel environment, providing:
//! - A `smoltcp::phy::Device` implementation backed by virtio-net
//! - An `Interface` with static IPv4 configuration
//! - IRQ-driven polling for ARP and ICMP echo (ping) processing

use alloc::vec::Vec;

use smoltcp::{
    iface::{Config, Interface, SocketSet, SocketStorage},
    phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken},
    time::Instant as SmolInstant,
    wire::{EthernetAddress, HardwareAddress, IpCidr, Ipv4Address, Ipv4Cidr},
};
use virtio_drivers::{device::net::VirtIONet, transport::SomeTransport};

use crate::{driver::virtio::VirtIOHalImpl, prelude::*};

const QUEUE_SIZE: usize = crate::driver::net::virtio_net::QUEUE_SIZE;

const DEFAULT_IP: Ipv4Address = Ipv4Address::new(10, 0, 2, 15);
const DEFAULT_PREFIX_LEN: u8 = 24;

type VirtIONetDev = VirtIONet<VirtIOHalImpl, SomeTransport<'static>, QUEUE_SIZE>;

// ---------------------------------------------------------------------------
// smoltcp phy::Device adapter
// ---------------------------------------------------------------------------

struct NetDeviceAdapter {
    inner: Arc<SpinLock<VirtIONetDev>>,
}

struct NetRxToken {
    data: Vec<u8>,
}

struct NetTxToken {
    inner: Arc<SpinLock<VirtIONetDev>>,
}

impl RxToken for NetRxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(&self.data)
    }
}

impl TxToken for NetTxToken {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut dev = self.inner.lock_irqsave();
        let mut tx_buf = dev.new_tx_buffer(len);
        let result = f(tx_buf.packet_mut());
        if let Err(e) = dev.send(tx_buf) {
            kerrln!("net: tx failed: {e}");
        }
        result
    }
}

impl Device for NetDeviceAdapter {
    type RxToken<'a> = NetRxToken;
    type TxToken<'a> = NetTxToken;

    fn receive(
        &mut self,
        _timestamp: SmolInstant,
    ) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let mut dev = self.inner.lock_irqsave();
        if !dev.can_recv() {
            return None;
        }
        let rx_buf = dev.receive().ok()?;
        let data = rx_buf.packet().to_vec();
        if dev.recycle_rx_buffer(rx_buf).is_err() {
            kerrln!("net: failed to recycle rx buffer");
            return None;
        }
        drop(dev);

        Some((
            NetRxToken { data },
            NetTxToken {
                inner: self.inner.clone(),
            },
        ))
    }

    fn transmit(&mut self, _timestamp: SmolInstant) -> Option<Self::TxToken<'_>> {
        let dev = self.inner.lock_irqsave();
        if !dev.can_send() {
            return None;
        }
        drop(dev);
        Some(NetTxToken {
            inner: self.inner.clone(),
        })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = 1514;
        caps.medium = Medium::Ethernet;
        caps
    }
}

// ---------------------------------------------------------------------------
// Network stack singleton
// ---------------------------------------------------------------------------

struct NetStack {
    device: NetDeviceAdapter,
    iface: Interface,
    sockets: SocketSet<'static>,
}

// Safety: NetStack fields are individually Send. The SocketSet uses 'static
// storage. Access is serialized by the SpinLock.
unsafe impl Send for NetStack {}

static NET_STACK: SpinLock<Option<NetStack>> = SpinLock::new(None);

fn now_smoltcp() -> SmolInstant {
    let ticks = TimeArch::current_ticks();
    let freq = TimeArch::hw_freq_hz().unwrap_or(1_000_000);
    let micros = ticks as i64 * 1_000_000 / freq as i64;
    SmolInstant::from_micros(micros)
}

/// Attach a VirtIO-net device to the network stack.
///
/// Called by the virtio-net driver after successful probe. Sets up the smoltcp
/// Interface with a static IPv4 address and prepares the stack for IRQ-driven
/// polling.
pub fn attach_device(dev: Arc<SpinLock<VirtIONetDev>>, mac: [u8; 6]) {
    let mut device = NetDeviceAdapter { inner: dev };

    let hw_addr = HardwareAddress::Ethernet(EthernetAddress(mac));
    let mut config = Config::new(hw_addr);
    config.random_seed = TimeArch::current_ticks();

    let now = now_smoltcp();
    let mut iface = Interface::new(config, &mut device, now);

    iface.update_ip_addrs(|addrs| {
        addrs
            .push(IpCidr::Ipv4(Ipv4Cidr::new(DEFAULT_IP, DEFAULT_PREFIX_LEN)))
            .unwrap();
    });

    let sockets = SocketSet::new(Vec::<SocketStorage<'static>>::new());

    *NET_STACK.lock_irqsave() = Some(NetStack {
        device,
        iface,
        sockets,
    });

    kinfoln!(
        "net: stack attached, ip={}/{}",
        DEFAULT_IP,
        DEFAULT_PREFIX_LEN
    );
}

/// Drive the smoltcp state machine.
///
/// Processes all pending ingress packets (ARP, IPv4/ICMP) and transmits any
/// queued egress packets (ARP replies, ICMP echo replies).
///
/// Called from the virtio-net IRQ handler after acknowledging the device
/// interrupt.
pub fn poll_network() {
    let mut guard = NET_STACK.lock_irqsave();
    let Some(stack) = guard.as_mut() else {
        return;
    };
    let now = now_smoltcp();
    stack
        .iface
        .poll(now, &mut stack.device, &mut stack.sockets);
}
