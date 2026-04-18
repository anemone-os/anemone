//! Optional in-kernel UDP/TCP echo for bring-up (`net-probe` feature).
//!
//! This module is a façade over protocol-specific implementations in `tcp.rs`
//! and `udp.rs`.

use alloc::vec::Vec;
use smoltcp::iface::{SocketHandle, SocketSet};
use crate::prelude::*;

mod tcp;
mod udp;

/// UDP discard/echo port (IANA discard; used here as echo for probes).
pub(crate) const PROBE_UDP_PORT: u16 = 7;
/// TCP port for the minimal echo service.
pub(crate) const PROBE_TCP_PORT: u16 = 2323;

/// Register probe sockets; binds UDP and starts TCP listen.
pub(crate) fn register_probe_sockets(sockets: &mut SocketSet<'static>) -> (SocketHandle, SocketHandle) {
    let udp_h = udp::register_udp_probe_socket(sockets);
    let tcp_h = tcp::register_tcp_probe_socket(sockets);

    kinfoln!(
        "net-probe: UDP echo on port {}, TCP echo on port {}",
        PROBE_UDP_PORT,
        PROBE_TCP_PORT
    );

    (udp_h, tcp_h)
}

/// Run after `iface.poll`: UDP echo and TCP byte echo; re-listen when the socket is closed.
pub(crate) fn poll_probe_sockets(
    sockets: &mut SocketSet<'static>,
    udp_h: SocketHandle,
    tcp_h: SocketHandle,
    tcp_pending_tx: &mut Vec<u8>,
) {
    udp::poll_udp_probe_socket(sockets, udp_h);
    tcp::poll_tcp_probe_socket(sockets, tcp_h, tcp_pending_tx);
}
