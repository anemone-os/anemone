//! TCP probe socket registration and polling.

use alloc::{boxed::Box, vec, vec::Vec};

use smoltcp::{
    iface::{SocketHandle, SocketSet},
    socket::tcp,
    wire::IpListenEndpoint,
};

use crate::prelude::*;

use super::PROBE_TCP_PORT;

const TCP_SOCKET_BUF: usize = 4096;

fn make_tcp_socket() -> tcp::Socket<'static> {
    let rx: &'static mut [u8] = Box::leak(vec![0u8; TCP_SOCKET_BUF].into_boxed_slice());
    let tx: &'static mut [u8] = Box::leak(vec![0u8; TCP_SOCKET_BUF].into_boxed_slice());
    tcp::Socket::new(tcp::SocketBuffer::new(rx), tcp::SocketBuffer::new(tx))
}

fn tcp_flush_pending(
    s: &mut tcp::Socket<'static>,
    pending: &mut Vec<u8>,
    tcp_bytes_echoed: &mut usize,
) {
    let mut off = 0usize;
    while off < pending.len() {
        match s.send_slice(&pending[off..]) {
            Ok(0) => break,
            Ok(sent) => {
                off += sent;
                *tcp_bytes_echoed += sent;
            }
            Err(e) => {
                kerrln!("net-probe: tcp echo pending send failed: {:?}", e);
                break;
            }
        }
    }
    if off > 0 {
        pending.drain(..off);
    }
}

fn tcp_try_send_echo(
    s: &mut tcp::Socket<'static>,
    data: &[u8],
    pending: &mut Vec<u8>,
    tcp_bytes_echoed: &mut usize,
) {
    if data.is_empty() {
        return;
    }
    let mut off = 0usize;
    while off < data.len() {
        match s.send_slice(&data[off..]) {
            Ok(0) => {
                pending.extend_from_slice(&data[off..]);
                break;
            }
            Ok(sent) => {
                off += sent;
                *tcp_bytes_echoed += sent;
            }
            Err(e) => {
                kerrln!("net-probe: tcp echo send failed: {:?}", e);
                pending.extend_from_slice(&data[off..]);
                break;
            }
        }
    }
}

fn probe_tcp_log_and_echo(
    s: &mut tcp::Socket<'static>,
    data: &[u8],
    tcp_pending_tx: &mut Vec<u8>,
    tcp_bytes_echoed: &mut usize,
) {
    let n = data.len();
    let preview_len = n.min(16);
    let peer = s.remote_endpoint();
    match core::str::from_utf8(data) {
        Ok(text) => match peer {
            Some(ep) => {
                kinfoln!(
                    "net-probe: tcp rx {} bytes from {} utf8={:?} hex_prefix={:02x?}",
                    n,
                    ep,
                    text,
                    &data[..preview_len]
                );
            }
            None => {
                kinfoln!(
                    "net-probe: tcp rx {} bytes from (no-remote) utf8={:?} hex_prefix={:02x?}",
                    n,
                    text,
                    &data[..preview_len]
                );
            }
        },
        Err(_) => match peer {
            Some(ep) => {
                kinfoln!(
                    "net-probe: tcp rx {} bytes from {} (non-utf8, first {} bytes: {:02x?})",
                    n,
                    ep,
                    preview_len,
                    &data[..preview_len]
                );
            }
            None => {
                kinfoln!(
                    "net-probe: tcp rx {} bytes from (no-remote) (non-utf8, first {} bytes: {:02x?})",
                    n,
                    preview_len,
                    &data[..preview_len]
                );
            }
        },
    }
    tcp_try_send_echo(s, data, tcp_pending_tx, tcp_bytes_echoed);
}

fn probe_tcp_relisten(s: &mut tcp::Socket<'static>, tcp_pending_tx: &mut Vec<u8>) {
    kinfoln!("net-probe: tcp peer closed recv half");
    tcp_pending_tx.clear();
    s.abort();
    if let Err(e) = s.listen(IpListenEndpoint::from(PROBE_TCP_PORT)) {
        kerrln!("net-probe: tcp re-listen after peer close: {:?}", e);
    }
}

pub(crate) fn register_tcp_probe_socket(sockets: &mut SocketSet<'static>) -> SocketHandle {
    let tcp_h = sockets.add(make_tcp_socket());
    let s = sockets.get_mut::<tcp::Socket>(tcp_h);
    if let Err(e) = s.listen(IpListenEndpoint::from(PROBE_TCP_PORT)) {
        kerrln!("net-probe: tcp listen failed: {:?}", e);
    }
    tcp_h
}

pub(crate) fn poll_tcp_probe_socket(
    sockets: &mut SocketSet<'static>,
    tcp_h: SocketHandle,
    tcp_pending_tx: &mut Vec<u8>,
) -> usize {
    let mut tcp_bytes_echoed = 0usize;
    let s = sockets.get_mut::<tcp::Socket>(tcp_h);
    use smoltcp::socket::tcp::State;

    if s.state() == State::Closed {
        tcp_pending_tx.clear();
        if let Err(e) = s.listen(IpListenEndpoint::from(PROBE_TCP_PORT)) {
            kerrln!("net-probe: tcp re-listen failed: {:?}", e);
        }
    } else if s.state() == State::TimeWait {
        tcp_pending_tx.clear();
        s.abort();
        if let Err(e) = s.listen(IpListenEndpoint::from(PROBE_TCP_PORT)) {
            kerrln!("net-probe: tcp re-listen after timewait: {:?}", e);
        }
    } else {
        if !tcp_pending_tx.is_empty() {
            tcp_flush_pending(s, tcp_pending_tx, &mut tcp_bytes_echoed);
        }
        if s.may_recv() {
            let mut buf = vec![0u8; TCP_SOCKET_BUF];
            while s.can_recv() {
                match s.recv_slice(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        probe_tcp_log_and_echo(s, &buf[..n], tcp_pending_tx, &mut tcp_bytes_echoed);
                    }
                    Err(e) => {
                        if matches!(e, smoltcp::socket::tcp::RecvError::Finished) {
                            probe_tcp_relisten(s, tcp_pending_tx);
                        }
                        break;
                    }
                }
            }
            if s.may_recv() && !s.can_recv() {
                match s.recv_slice(&mut buf) {
                    Ok(0) => {}
                    Ok(n) => {
                        probe_tcp_log_and_echo(s, &buf[..n], tcp_pending_tx, &mut tcp_bytes_echoed);
                    }
                    Err(e) => {
                        if matches!(e, smoltcp::socket::tcp::RecvError::Finished) {
                            probe_tcp_relisten(s, tcp_pending_tx);
                        }
                    }
                }
            }
        }

        if matches!(s.state(), State::CloseWait | State::LastAck | State::FinWait2) {
            if !tcp_pending_tx.is_empty() {
                tcp_flush_pending(s, tcp_pending_tx, &mut tcp_bytes_echoed);
            }
            if tcp_pending_tx.is_empty() {
                let st = s.state();
                tcp_pending_tx.clear();
                s.abort();
                if let Err(e) = s.listen(IpListenEndpoint::from(PROBE_TCP_PORT)) {
                    kerrln!(
                        "net-probe: tcp re-listen after {:?} (dead session): {:?}",
                        st,
                        e
                    );
                }
            }
        }
    }

    tcp_bytes_echoed
}
