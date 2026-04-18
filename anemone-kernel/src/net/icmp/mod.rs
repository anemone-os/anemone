//! ICMPv4 echo (ping) reply path via a raw IPv4 socket.

mod raw_poll;

pub(crate) use raw_poll::poll_icmp_raw_socket;

use alloc::{boxed::Box, vec, vec::Vec};
use core::fmt;

use smoltcp::{
    iface::SocketHandle,
    phy::ChecksumCapabilities,
    socket::raw,
    time::Instant as SmolInstant,
    wire::{
        Icmpv4Packet, Icmpv4Repr, IpProtocol, IpVersion, Ipv4Packet, Ipv4Repr,
    },
};

use super::config::{ICMP_PKT_BUF_LEN, ICMP_SOCKETS};

/// Counters for ICMPv4 raw echo handling (diagnostics; see `NETWORK.md`).
#[derive(Clone, Copy, Debug, Default)]
pub struct IcmpEchoStats {
    pub rx_echo_requests: u64,
    pub tx_echo_replies_queued: u64,
    pub rx_not_echo_request: u64,
    pub rx_parse_errors: u64,
    pub tx_enqueue_errors: u64,
    pub tx_rate_limited: u64,
}

impl fmt::Display for IcmpEchoStats {
    /// Compact serial-friendly form: cumulative echo counters, then only non-zero anomaly fields.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "req={} rep={}",
            self.rx_echo_requests, self.tx_echo_replies_queued
        )?;
        if self.rx_not_echo_request != 0 {
            write!(f, " not_echo_req={}", self.rx_not_echo_request)?;
        }
        if self.rx_parse_errors != 0 {
            write!(f, " parse_err={}", self.rx_parse_errors)?;
        }
        if self.tx_enqueue_errors != 0 {
            write!(f, " tx_enq_err={}", self.tx_enqueue_errors)?;
        }
        if self.tx_rate_limited != 0 {
            write!(f, " rate_limited={}", self.tx_rate_limited)?;
        }
        Ok(())
    }
}

/// Simple per-stack rate limiter for ICMP echo replies (amplification mitigation).
pub(crate) struct IcmpEchoLimiter {
    window_start_us: i64,
    replies_in_window: u32,
}

impl Default for IcmpEchoLimiter {
    fn default() -> Self {
        Self {
            window_start_us: i64::MIN,
            replies_in_window: 0,
        }
    }
}

impl IcmpEchoLimiter {
    const WINDOW_US: i64 = 1_000_000;
    const MAX_REPLIES_PER_SEC: u32 = 100;

    pub(crate) fn try_consume_token(&mut self, now: SmolInstant) -> bool {
        let us = now.total_micros();
        if us.saturating_sub(self.window_start_us) >= Self::WINDOW_US {
            self.window_start_us = us;
            self.replies_in_window = 0;
        }
        if self.replies_in_window >= Self::MAX_REPLIES_PER_SEC {
            return false;
        }
        self.replies_in_window += 1;
        true
    }
}

pub(crate) fn make_raw_packet_buffer() -> raw::PacketBuffer<'static> {
    let metadata: &'static mut [raw::PacketMetadata] =
        Box::leak(vec![raw::PacketMetadata::EMPTY; ICMP_SOCKETS].into_boxed_slice());
    let payload: &'static mut [u8] =
        Box::leak(vec![0u8; ICMP_SOCKETS * ICMP_PKT_BUF_LEN].into_boxed_slice());
    raw::PacketBuffer::new(metadata, payload)
}

/// Add the ICMPv4 raw socket used for echo replies. Returns its [`SocketHandle`].
pub(crate) fn add_icmpv4_raw_socket(
    sockets: &mut smoltcp::iface::SocketSet<'static>,
) -> SocketHandle {
    sockets.add(raw::Socket::new(
        Some(IpVersion::Ipv4),
        Some(IpProtocol::Icmp),
        make_raw_packet_buffer(),
        make_raw_packet_buffer(),
    ))
}

pub(crate) fn build_icmpv4_echo_reply(frame: &[u8]) -> Option<Vec<u8>> {
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

#[derive(Clone, Copy)]
pub(crate) enum IcmpRawClass {
    BadParse,
    NotEchoRequest,
    EchoRequest,
}

pub(crate) fn classify_icmpv4_echo_packet(frame: &[u8]) -> IcmpRawClass {
    let Ok(ipv4_pkt) = Ipv4Packet::new_checked(frame) else {
        return IcmpRawClass::BadParse;
    };
    if ipv4_pkt.next_header() != IpProtocol::Icmp {
        return IcmpRawClass::NotEchoRequest;
    }
    let Ok(icmp_pkt) = Icmpv4Packet::new_checked(ipv4_pkt.payload()) else {
        return IcmpRawClass::BadParse;
    };
    let Ok(icmp_repr) = Icmpv4Repr::parse(&icmp_pkt, &ChecksumCapabilities::ignored()) else {
        return IcmpRawClass::BadParse;
    };
    match icmp_repr {
        Icmpv4Repr::EchoRequest { .. } => IcmpRawClass::EchoRequest,
        _ => IcmpRawClass::NotEchoRequest,
    }
}
