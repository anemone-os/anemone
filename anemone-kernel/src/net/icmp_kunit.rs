//! KUnit tests for ICMP echo reply construction.

use alloc::vec;

use smoltcp::{
    phy::ChecksumCapabilities,
    wire::{Icmpv4Packet, Icmpv4Repr, IpProtocol, Ipv4Address, Ipv4Packet, Ipv4Repr},
};

use crate::net::icmp::build_icmpv4_echo_reply;
use crate::prelude::*;

#[kunit]
fn icmpv4_echo_reply_build_synthetic() {
    let remote = Ipv4Address::new(10, 0, 0, 5);
    let local = Ipv4Address::new(192, 168, 100, 2);
    let payload: &[u8] = &[0xaa, 0xbb];
    let icmp_repr = Icmpv4Repr::EchoRequest {
        ident: 0x1234,
        seq_no: 7,
        data: payload,
    };
    let ip_repr = Ipv4Repr {
        src_addr: remote,
        dst_addr: local,
        next_header: IpProtocol::Icmp,
        payload_len: icmp_repr.buffer_len(),
        hop_limit: 64,
    };
    let mut buf = vec![0u8; ip_repr.buffer_len() + icmp_repr.buffer_len()];
    let mut ip_pkg = Ipv4Packet::new_unchecked(&mut buf);
    ip_repr.emit(&mut ip_pkg, &ChecksumCapabilities::default());
    let mut icmp_pkg = Icmpv4Packet::new_unchecked(ip_pkg.payload_mut());
    icmp_repr.emit(&mut icmp_pkg, &ChecksumCapabilities::default());

    let reply = build_icmpv4_echo_reply(&buf).expect("echo reply");
    let rep_ip = Ipv4Packet::new_checked(&reply).expect("reply ip");
    let rep_ip_repr =
        Ipv4Repr::parse(&rep_ip, &ChecksumCapabilities::ignored()).expect("reply ip repr");
    assert_eq!(rep_ip_repr.src_addr, local);
    assert_eq!(rep_ip_repr.dst_addr, remote);
    let rep_icmp = Icmpv4Packet::new_checked(rep_ip.payload()).expect("reply icmp");
    let rep_repr =
        Icmpv4Repr::parse(&rep_icmp, &ChecksumCapabilities::ignored()).expect("icmp repr");
    match rep_repr {
        Icmpv4Repr::EchoReply { ident, seq_no, data } => {
            assert_eq!(ident, 0x1234);
            assert_eq!(seq_no, 7);
            assert_eq!(data, payload);
        }
        _ => panic!("expected echo reply"),
    }
}
