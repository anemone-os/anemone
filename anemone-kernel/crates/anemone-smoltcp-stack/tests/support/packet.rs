use anemone_net_api::Instant;
use anemone_smoltcp_stack::{PumpBudget, Stack};
use smoltcp::{
    phy::ChecksumCapabilities,
    wire::{
        ArpOperation, ArpPacket, ArpRepr, EthernetAddress as SmoltcpEthernetAddress, EthernetFrame,
        EthernetProtocol, EthernetRepr, Icmpv4Packet, Icmpv4Repr, IpProtocol, Ipv4Address,
        Ipv4Packet, Ipv4Repr,
    },
};

use super::frame::{BoundedProvider, DeterministicProvider, TxSlot};

pub(crate) fn build_icmp_echo_request(
    source_mac: [u8; 6],
    destination_mac: [u8; 6],
    source_ip: [u8; 4],
    destination_ip: [u8; 4],
) -> Vec<u8> {
    let payload = [0xaa, 0xbb, 0xcc, 0xdd];
    let icmp = Icmpv4Repr::EchoRequest {
        ident: 0x1234,
        seq_no: 7,
        data: &payload,
    };
    let ipv4 = Ipv4Repr {
        src_addr: Ipv4Address::from_octets(source_ip),
        dst_addr: Ipv4Address::from_octets(destination_ip),
        next_header: IpProtocol::Icmp,
        payload_len: icmp.buffer_len(),
        hop_limit: 64,
    };
    let ethernet = EthernetRepr {
        src_addr: SmoltcpEthernetAddress::from_bytes(&source_mac),
        dst_addr: SmoltcpEthernetAddress::from_bytes(&destination_mac),
        ethertype: EthernetProtocol::Ipv4,
    };
    let ip_offset = ethernet.buffer_len();
    let icmp_offset = ip_offset + ipv4.buffer_len();
    let mut bytes = vec![0; icmp_offset + icmp.buffer_len()];

    ethernet.emit(&mut EthernetFrame::new_unchecked(&mut bytes[..]));
    ipv4.emit(
        &mut Ipv4Packet::new_unchecked(&mut bytes[ip_offset..]),
        &ChecksumCapabilities::default(),
    );
    icmp.emit(
        &mut Icmpv4Packet::new_unchecked(&mut bytes[icmp_offset..]),
        &ChecksumCapabilities::default(),
    );
    bytes
}

pub(crate) fn build_arp_request(
    source_mac: [u8; 6],
    source_ip: [u8; 4],
    destination_ip: [u8; 4],
) -> Vec<u8> {
    let source_mac = SmoltcpEthernetAddress::from_bytes(&source_mac);
    let ethernet = EthernetRepr {
        src_addr: source_mac,
        dst_addr: SmoltcpEthernetAddress::BROADCAST,
        ethertype: EthernetProtocol::Arp,
    };
    let arp = ArpRepr::EthernetIpv4 {
        operation: ArpOperation::Request,
        source_hardware_addr: source_mac,
        source_protocol_addr: Ipv4Address::from_octets(source_ip),
        target_hardware_addr: SmoltcpEthernetAddress::from_bytes(&[0; 6]),
        target_protocol_addr: Ipv4Address::from_octets(destination_ip),
    };
    let mut bytes = vec![0; ethernet.buffer_len() + arp.buffer_len()];
    ethernet.emit(&mut EthernetFrame::new_unchecked(&mut bytes[..]));
    arp.emit(&mut ArpPacket::new_unchecked(
        &mut bytes[ethernet.buffer_len()..],
    ));
    bytes
}

pub(crate) fn build_raw_ipv4_packet(
    source_ip: [u8; 4],
    destination_ip: [u8; 4],
    marker: u8,
) -> Vec<u8> {
    let ipv4 = Ipv4Repr {
        src_addr: Ipv4Address::from_octets(source_ip),
        dst_addr: Ipv4Address::from_octets(destination_ip),
        next_header: IpProtocol::Unknown(253),
        payload_len: 1,
        hop_limit: 64,
    };
    let mut bytes = vec![0; ipv4.buffer_len() + 1];
    ipv4.emit(
        &mut Ipv4Packet::new_unchecked(&mut bytes[..]),
        &ChecksumCapabilities::default(),
    );
    bytes[ipv4.buffer_len()] = marker;
    bytes
}

pub(crate) fn raw_ipv4_marker(frame: &[u8]) -> u8 {
    let ethernet = EthernetFrame::new_checked(frame).unwrap();
    let ipv4 = Ipv4Packet::new_checked(ethernet.payload()).unwrap();
    ipv4.payload()[0]
}

pub(crate) fn prime_neighbor(
    stack: &mut Stack,
    interface: anemone_net_api::InterfaceId,
    provider: &mut DeterministicProvider,
    peer_mac: [u8; 6],
    peer_ip: [u8; 4],
    local_ip: [u8; 4],
) {
    provider
        .rx
        .inject(&build_arp_request(peer_mac, peer_ip, local_ip));
    stack
        .pump(interface, provider, Instant::ZERO, PumpBudget::new(2, 1))
        .unwrap();
    assert_eq!(provider.tx.slot, TxSlot::Submitted);
    provider.tx.complete();
    provider.receive_calls = 0;
}

pub(crate) fn prime_bounded_neighbor(
    stack: &mut Stack,
    interface: anemone_net_api::InterfaceId,
    provider: &mut BoundedProvider,
    peer_mac: [u8; 6],
    peer_ip: [u8; 4],
    local_ip: [u8; 4],
) {
    provider.inject(&build_arp_request(peer_mac, peer_ip, local_ip));
    stack
        .pump(interface, provider, Instant::ZERO, PumpBudget::new(2, 1))
        .unwrap();
    assert_eq!(provider.live_tx(), 1);
    provider.complete_all();
}

pub(crate) fn assert_icmp_echo_reply(
    frame: &[u8],
    source_mac: [u8; 6],
    destination_mac: [u8; 6],
    source_ip: [u8; 4],
    destination_ip: [u8; 4],
) {
    let ethernet = EthernetFrame::new_checked(frame).unwrap();
    assert_eq!(
        ethernet.src_addr(),
        SmoltcpEthernetAddress::from_bytes(&source_mac)
    );
    assert_eq!(
        ethernet.dst_addr(),
        SmoltcpEthernetAddress::from_bytes(&destination_mac)
    );

    let ipv4 = Ipv4Packet::new_checked(ethernet.payload()).unwrap();
    assert_eq!(ipv4.src_addr(), Ipv4Address::from_octets(source_ip));
    assert_eq!(ipv4.dst_addr(), Ipv4Address::from_octets(destination_ip));
    let icmp = Icmpv4Packet::new_checked(ipv4.payload()).unwrap();
    assert!(matches!(
        Icmpv4Repr::parse(&icmp, &ChecksumCapabilities::default()).unwrap(),
        Icmpv4Repr::EchoReply {
            ident: 0x1234,
            seq_no: 7,
            data: [0xaa, 0xbb, 0xcc, 0xdd]
        }
    ));
}
