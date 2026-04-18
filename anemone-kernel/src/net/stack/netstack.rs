//! Per-interface smoltcp [`Interface`](smoltcp::iface::Interface) and [`SocketSet`].

use alloc::{string::String, vec::Vec};

use smoltcp::iface::{Interface, SocketHandle, SocketSet};

use crate::net::icmp::{IcmpEchoLimiter, IcmpEchoStats};

use super::adapter::NetDeviceAdapter;

pub(crate) struct NetStack {
    pub(crate) name: String,
    pub(crate) device: NetDeviceAdapter,
    pub(crate) iface: Interface,
    pub(crate) sockets: SocketSet<'static>,
    pub(crate) icmp_raw_handle: SocketHandle,
    pub(crate) icmp_stats: IcmpEchoStats,
    pub(crate) icmp_echo_limiter: IcmpEchoLimiter,
    #[cfg(feature = "net-probe")]
    pub(crate) probe_udp: ProbeUdpState,
    #[cfg(feature = "net-probe")]
    pub(crate) probe_tcp: ProbeTcpState,
}

#[cfg(feature = "net-probe")]
pub(crate) struct ProbeUdpState {
    pub(crate) handle: SocketHandle,
}

#[cfg(feature = "net-probe")]
pub(crate) struct ProbeTcpState {
    pub(crate) handle: SocketHandle,
    /// Bytes accepted from TCP recv but not yet fully enqueued on TX (`send_slice` was partial).
    pub(crate) pending_tx: Vec<u8>,
}

// Safety: NetStack fields are individually Send. The SocketSet uses 'static
// storage. Access is serialized by the per-stack SpinLock.
unsafe impl Send for NetStack {}
