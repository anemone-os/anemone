use alloc::{vec, vec::Vec};

use anemone_net_api::{InterfaceId, Ipv4Address, icmp_raw::IcmpRawEgressPolicy};
use smoltcp::wire::{IpProtocol, Ipv4Address as SmoltcpIpv4Address, Ipv4Packet};

pub(super) const IPV4_HEADER_LEN: usize = 20;
pub(super) const IPV4_MAX_PACKET_BYTES: usize = u16::MAX as usize;

pub(super) struct PendingPacket {
    pub(super) interface: InterfaceId,
    pub(super) bytes: Vec<u8>,
}

pub(super) fn build_packet(
    identification: u16,
    source: Ipv4Address,
    destination: Ipv4Address,
    policy: IcmpRawEgressPolicy,
    message: &[u8],
) -> Vec<u8> {
    let total_len = IPV4_HEADER_LEN + message.len();
    assert!(total_len <= IPV4_MAX_PACKET_BYTES);
    let mut bytes = vec![0; total_len];
    let mut packet = Ipv4Packet::new_unchecked(&mut bytes[..]);
    packet.set_version(4);
    packet.set_header_len(IPV4_HEADER_LEN as u8);
    packet.set_dscp(policy.tos() >> 2);
    packet.set_ecn(policy.tos() & 0x03);
    packet.set_total_len(total_len as u16);
    packet.set_ident(identification);
    packet.clear_flags();
    packet.set_frag_offset(0);
    packet.set_hop_limit(policy.ttl());
    packet.set_next_header(IpProtocol::Icmp);
    packet.set_src_addr(SmoltcpIpv4Address::from_octets(source.octets()));
    packet.set_dst_addr(SmoltcpIpv4Address::from_octets(destination.octets()));
    packet.payload_mut().copy_from_slice(message);
    packet.fill_checksum();
    bytes
}
