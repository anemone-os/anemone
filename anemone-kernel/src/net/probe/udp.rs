//! UDP probe socket registration and polling.

use alloc::{boxed::Box, vec};

use smoltcp::{
    iface::{SocketHandle, SocketSet},
    socket::udp,
};

use crate::prelude::*;

use super::PROBE_UDP_PORT;

const UDP_PACKET_SLOTS: usize = 8;
const UDP_PAYLOAD_CAP: usize = 2048;

fn make_udp_buffers() -> udp::PacketBuffer<'static> {
    let metadata: &'static mut [udp::PacketMetadata] = Box::leak(
        vec![udp::PacketMetadata::EMPTY; UDP_PACKET_SLOTS].into_boxed_slice(),
    );
    let payload: &'static mut [u8] = Box::leak(
        vec![0u8; UDP_PACKET_SLOTS * UDP_PAYLOAD_CAP].into_boxed_slice(),
    );
    udp::PacketBuffer::new(metadata, payload)
}

pub(crate) fn register_udp_probe_socket(sockets: &mut SocketSet<'static>) -> SocketHandle {
    let udp_h = sockets.add(udp::Socket::new(make_udp_buffers(), make_udp_buffers()));
    let s = sockets.get_mut::<udp::Socket>(udp_h);
    if let Err(e) = s.bind(PROBE_UDP_PORT) {
        kerrln!("net-probe: udp bind failed: {:?}", e);
    }
    udp_h
}

pub(crate) fn poll_udp_probe_socket(
    sockets: &mut SocketSet<'static>,
    udp_h: SocketHandle,
) -> usize {
    let mut udp_datagrams = 0usize;
    let s = sockets.get_mut::<udp::Socket>(udp_h);
    let mut scratch = vec![0u8; UDP_PAYLOAD_CAP];
    while s.can_recv() {
        match s.recv_slice(&mut scratch) {
            Ok((n, meta)) => {
                udp_datagrams += 1;
                let data = &scratch[..n];
                let preview_len = n.min(16);
                match core::str::from_utf8(data) {
                    Ok(text) => {
                        kinfoln!(
                            "net-probe: udp rx {} bytes from {} utf8={:?} hex_prefix={:02x?}",
                            n,
                            meta.endpoint,
                            text,
                            &data[..preview_len]
                        );
                    }
                    Err(_) => {
                        kinfoln!(
                            "net-probe: udp rx {} bytes from {} (non-utf8, first {} bytes: {:02x?})",
                            n,
                            meta.endpoint,
                            preview_len,
                            &data[..preview_len]
                        );
                    }
                }
                if let Err(e) = s.send_slice(data, meta) {
                    kerrln!("net-probe: udp echo send failed: {:?}", e);
                }
            }
            Err(e) => {
                kerrln!("net-probe: udp recv failed: {:?}", e);
                break;
            }
        }
    }
    udp_datagrams
}
