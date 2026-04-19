//! Construct a [`NetStack`](super::netstack::NetStack) for a registered [`NetDev`](crate::device::net::NetDev).

use alloc::{string::String, sync::Arc, vec::Vec};

use smoltcp::{
    iface::{Config, Interface, SocketSet, SocketStorage},
    wire::{EthernetAddress, HardwareAddress, IpCidr, Ipv4Cidr},
};

use crate::{
    device::{
        error::DevError,
        net::{NetDev, NetDevClass, LOOPBACK_MAC},
    },
    prelude::*,
    LocalClockSource,
};

use super::super::{
    config::{
        self, DEFAULT_IP, DEFAULT_PREFIX_LEN, LOOPBACK_IP, LOOPBACK_PREFIX_LEN, QEMU_USER_HOST,
    },
    icmp::{self, IcmpEchoLimiter, IcmpEchoStats},
};
#[cfg(feature = "net-probe")]
use super::super::probe;
use super::{
    adapter::NetDeviceAdapter,
    netstack::{NetStack, ProbeTcpState, ProbeUdpState},
};

pub(crate) fn build_stack(netdev: Arc<dyn NetDev>, name: &str) -> Result<NetStack, DevError> {
    let mac = netdev.mac().unwrap_or(LOOPBACK_MAC);
    let hw_addr = HardwareAddress::Ethernet(EthernetAddress(mac));
    let mut config = Config::new(hw_addr);
    config.random_seed = LocalClockSource::curr_monotonic_time();

    let mut device = NetDeviceAdapter {
        netdev: netdev.clone(),
    };
    let now = config::now_smoltcp();
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
    let icmp_raw_handle = icmp::add_icmpv4_raw_socket(&mut sockets);

    #[cfg(feature = "net-probe")]
    let (probe_udp, probe_tcp) = probe::register_probe_sockets(&mut sockets);

    Ok(NetStack {
        name: String::from(name),
        device,
        iface,
        sockets,
        user_socket_entries: Vec::new(),
        icmp_raw_handle,
        icmp_stats: IcmpEchoStats::default(),
        icmp_echo_limiter: IcmpEchoLimiter::default(),
        #[cfg(feature = "net-probe")]
        probe_udp: ProbeUdpState { handle: probe_udp },
        #[cfg(feature = "net-probe")]
        probe_tcp: ProbeTcpState {
            handle: probe_tcp,
            pending_tx: Vec::new(),
        },
    })
}
