//! Minimal kernel network stack built on smoltcp.
//!
//! This module adapts smoltcp to the kernel environment, providing:
//! - A `smoltcp::phy::Device` implementation backed by [`crate::device::net::NetDev`]
//! - Per-interface `Interface` with static IPv4 configuration
//! - IRQ-driven polling for ARP and ICMP echo (ping) processing

use alloc::{boxed::Box, string::String, vec, vec::Vec};

use smoltcp::{
    iface::{Config, Interface, PollResult, SocketHandle, SocketSet, SocketStorage},
    phy::{ChecksumCapabilities, Device, DeviceCapabilities, RxToken, TxToken},
    socket::raw,
    time::Instant as SmolInstant,
    wire::{
        EthernetAddress, HardwareAddress, Icmpv4Packet, Icmpv4Repr, IpCidr, IpProtocol, IpVersion,
        Ipv4Address, Ipv4Cidr, Ipv4Packet, Ipv4Repr,
    },
};

use crate::{
    device::{
        error::DevError,
        net::{get_netdev, NetDev, NetDevClass, NetDevRegistration, LoopbackNetDev, LOOPBACK_MAC},
    },
    prelude::*,
};

const DEFAULT_IP: Ipv4Address = Ipv4Address::new(192, 168, 100, 2);
const DEFAULT_PREFIX_LEN: u8 = 24;

/// Slirp “host” address from the guest; must match QEMU `-netdev user,host=...`
/// in `conf/platforms/qemu-virt-rv64.toml`.
const QEMU_USER_HOST: Ipv4Address = Ipv4Address::new(192, 168, 100, 1);

const LOOPBACK_IP: Ipv4Address = Ipv4Address::new(127, 0, 0, 1);
const LOOPBACK_PREFIX_LEN: u8 = 8;

// ---------------------------------------------------------------------------
// smoltcp phy::Device adapter
// ---------------------------------------------------------------------------

struct NetDeviceAdapter {
    netdev: Arc<dyn NetDev>,
}

impl NetDeviceAdapter {
    fn netdev(&self) -> Arc<dyn NetDev> {
        self.netdev.clone()
    }
}

struct NetRxToken {
    data: Vec<u8>,
}

struct NetTxToken {
    netdev: Arc<dyn NetDev>,
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
        let mut buf = vec![0u8; len];
        let result = f(&mut buf);
        self.netdev.with_phy_mut(&mut |phy| {
            if phy.send_raw(&buf).is_err() {
                kerrln!("net: tx failed on send_raw");
            }
        });
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
        let mut frame: Option<Vec<u8>> = None;
        self.netdev.with_phy_mut(&mut |phy| {
            frame = phy.try_recv_frame();
        });
        let data = frame?;
        Some((
            NetRxToken { data },
            NetTxToken {
                netdev: self.netdev.clone(),
            },
        ))
    }

    fn transmit(&mut self, _timestamp: SmolInstant) -> Option<Self::TxToken<'_>> {
        let mut can = false;
        self.netdev.with_phy_mut(&mut |phy| {
            can = phy.can_send();
        });
        if !can {
            return None;
        }
        Some(NetTxToken {
            netdev: self.netdev.clone(),
        })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        self.netdev.with_phy_mut(&mut |phy| {
            caps = phy.capabilities();
        });
        caps
    }
}

// ---------------------------------------------------------------------------
// Network stack: one smoltcp interface per attached netdev
// ---------------------------------------------------------------------------

struct NetStack {
    name: String,
    device: NetDeviceAdapter,
    iface: Interface,
    sockets: SocketSet<'static>,
    icmp_raw_handle: SocketHandle,
}

// Safety: NetStack fields are individually Send. The SocketSet uses 'static
// storage. Access is serialized by the SpinLock.
unsafe impl Send for NetStack {}

static NET_STACKS: SpinLock<Vec<NetStack>> = SpinLock::new(Vec::new());

fn now_smoltcp() -> SmolInstant {
    let ticks = TimeArch::current_ticks();
    let freq = TimeArch::hw_freq_hz().unwrap_or(1_000_000);
    let micros = ticks as i64 * 1_000_000 / freq as i64;
    SmolInstant::from_micros(micros)
}

const ICMP_SOCKETS: usize = 16;
const ICMP_PKT_BUF_LEN: usize = 1536;
const MAX_EGRESS_FLUSH_ROUNDS: usize = 4;

fn make_raw_packet_buffer() -> raw::PacketBuffer<'static> {
    let metadata: &'static mut [raw::PacketMetadata] =
        Box::leak(vec![raw::PacketMetadata::EMPTY; ICMP_SOCKETS].into_boxed_slice());
    let payload: &'static mut [u8] =
        Box::leak(vec![0u8; ICMP_SOCKETS * ICMP_PKT_BUF_LEN].into_boxed_slice());
    raw::PacketBuffer::new(metadata, payload)
}

fn build_icmpv4_echo_reply(frame: &[u8]) -> Option<Vec<u8>> {
    let ipv4_pkt = Ipv4Packet::new_checked(frame).ok()?;
    if ipv4_pkt.next_header() != IpProtocol::Icmp {
        return None;
    }
    let ipv4_repr = Ipv4Repr::parse(&ipv4_pkt, &ChecksumCapabilities::ignored()).ok()?;
    let icmp_pkt = Icmpv4Packet::new_checked(ipv4_pkt.payload()).ok()?;
    let icmp_repr = Icmpv4Repr::parse(&icmp_pkt, &ChecksumCapabilities::ignored()).ok()?;

    let (ident, seq_no, data) = match icmp_repr {
        Icmpv4Repr::EchoRequest { ident, seq_no, data } => (ident, seq_no, data),
        _ => return None,
    };

    let icmp_reply = Icmpv4Repr::EchoReply {
        ident,
        seq_no,
        data,
    };
    let ip_reply = Ipv4Repr {
        src_addr: ipv4_repr.dst_addr,
        dst_addr: ipv4_repr.src_addr,
        next_header: IpProtocol::Icmp,
        payload_len: icmp_reply.buffer_len(),
        hop_limit: 64,
    };

    let mut out = vec![0u8; ip_reply.buffer_len() + icmp_reply.buffer_len()];
    {
        let mut out_ip = Ipv4Packet::new_unchecked(&mut out);
        ip_reply.emit(&mut out_ip, &ChecksumCapabilities::default());
        let mut out_icmp = Icmpv4Packet::new_unchecked(out_ip.payload_mut());
        icmp_reply.emit(&mut out_icmp, &ChecksumCapabilities::default());
    }
    Some(out)
}

fn build_stack(netdev: Arc<dyn NetDev>, name: &str) -> Result<NetStack, DevError> {
    let mac = netdev.mac().unwrap_or(LOOPBACK_MAC);
    let hw_addr = HardwareAddress::Ethernet(EthernetAddress(mac));
    let mut config = Config::new(hw_addr);
    config.random_seed = TimeArch::current_ticks();

    let mut device = NetDeviceAdapter {
        netdev: netdev.clone(),
    };
    let now = now_smoltcp();
    let mut iface = Interface::new(config, &mut device, now);

    match netdev.class() {
        NetDevClass::Ethernet => {
            iface.update_ip_addrs(|addrs| {
                addrs
                    .push(IpCidr::Ipv4(Ipv4Cidr::new(DEFAULT_IP, DEFAULT_PREFIX_LEN)))
                    .unwrap();
            });
            if let Err(e) = iface
                .routes_mut()
                .add_default_ipv4_route(QEMU_USER_HOST)
            {
                kerrln!("net: failed to add default ipv4 route: {}", e);
            }
        }
        NetDevClass::Loopback => {
            iface.update_ip_addrs(|addrs| {
                addrs
                    .push(IpCidr::Ipv4(Ipv4Cidr::new(LOOPBACK_IP, LOOPBACK_PREFIX_LEN)))
                    .unwrap();
            });
        }
    }

    let mut sockets = SocketSet::new(Vec::<SocketStorage<'static>>::new());
    let icmp_raw_handle = sockets.add(raw::Socket::new(
        Some(IpVersion::Ipv4),
        Some(IpProtocol::Icmp),
        make_raw_packet_buffer(),
        make_raw_packet_buffer(),
    ));

    Ok(NetStack {
        name: String::from(name),
        device,
        iface,
        sockets,
        icmp_raw_handle,
    })
}

/// Attach a registered netdev (by canonical name) to the smoltcp stack.
///
/// Call after [`crate::device::net::register_net_device`]. The `name` must match
/// the return value from registration.
pub fn attach_netdev_by_name(name: &str) -> Result<(), DevError> {
    let netdev = get_netdev(name).ok_or(DevError::NoSuchDevice)?;

    let mut stacks = NET_STACKS.lock_irqsave();
    if stacks.iter().any(|s| s.name == name) {
        return Err(DevError::DevAlreadyRegistered);
    }

    let stack = build_stack(netdev, name)?;
    let class = stack.device.netdev().class();
    stacks.push(stack);

    match class {
        NetDevClass::Ethernet => {
            kinfoln!(
                "net: stack attached on {} ip={}/{} gateway={}",
                name,
                DEFAULT_IP,
                DEFAULT_PREFIX_LEN,
                QEMU_USER_HOST,
            );
        }
        NetDevClass::Loopback => {
            kinfoln!(
                "net: stack attached on {} ip={}/{}",
                name,
                LOOPBACK_IP,
                LOOPBACK_PREFIX_LEN
            );
        }
    }

    Ok(())
}

/// Drive the smoltcp state machine for every attached interface.
///
/// Processes pending ingress and transmits queued egress. Typically called from
/// a NIC IRQ handler after the device acknowledges the interrupt.
pub fn poll_network() {
    let mut stacks = NET_STACKS.lock_irqsave();
    if stacks.is_empty() {
        return;
    }

    let mut req = 0usize;
    let mut rsp = 0usize;
    let mut poll_changed = 0usize;
    let mut flush_rounds = 0usize;

    for stack in stacks.iter_mut() {
        for round in 0..=MAX_EGRESS_FLUSH_ROUNDS {
            let now = now_smoltcp();
            if matches!(
                stack
                    .iface
                    .poll(now, &mut stack.device, &mut stack.sockets),
                PollResult::SocketStateChanged
            ) {
                poll_changed += 1;
            }

            let mut queued_this_round = 0usize;
            {
                let socket = stack
                    .sockets
                    .get_mut::<raw::Socket>(stack.icmp_raw_handle);
                while socket.can_recv() {
                    let frame = match socket.recv() {
                        Ok(frame) => frame.to_vec(),
                        Err(_) => break,
                    };
                    if let Some(reply) = build_icmpv4_echo_reply(&frame) {
                        req += 1;
                        if socket.send_slice(&reply).is_ok() {
                            rsp += 1;
                            queued_this_round += 1;
                        } else {
                            kerrln!("net: failed to enqueue icmp echo reply");
                        }
                    }
                }
            }

            if queued_this_round == 0 {
                break;
            }
            if round < MAX_EGRESS_FLUSH_ROUNDS {
                flush_rounds += 1;
            }
        }
    }

    if req > 0 || poll_changed > 0 {
        kinfoln!(
            "net: handled {req} icmp echo request(s), queued {rsp} reply(ies), egress_flush_rounds={flush_rounds}, poll_changed={poll_changed}"
        );
    }
}

#[initcall(probe)]
fn loopback_init() {
    let dev = Arc::new(LoopbackNetDev::new());
    match crate::device::net::register_net_device(NetDevRegistration {
        class: NetDevClass::Loopback,
        device: dev.clone(),
    }) {
        Ok(name) => {
            if let Err(e) = attach_netdev_by_name(name.as_str()) {
                kerrln!("net: failed to attach loopback: {:?}", e);
            }
        }
        Err(e) => {
            knoticeln!("net: loopback register failed: {:?}", e);
        }
    }
}
