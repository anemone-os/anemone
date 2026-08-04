pub(super) use anemone_net_api::{
    Instant, InterfaceId, Ipv4Address as ApiIpv4Address, Ipv4Cidr as ApiIpv4Cidr,
    udp::{
        UdpBindError, UdpBindRequest, UdpConnectError, UdpCreateError, UdpEndpointLimits,
        UdpNamespacePolicy, UdpQueryError,
    },
};
pub(super) use anemone_smoltcp_stack::{
    HostEndpointCreateError, HostRetireError, HostSelection, HostSendError, PumpBudget, Stack,
};
pub(super) use smoltcp::{
    phy::ChecksumCapabilities,
    wire::{
        EthernetAddress as SmoltcpEthernetAddress, EthernetFrame, EthernetProtocol, EthernetRepr,
        IpAddress, IpProtocol, Ipv4Address, Ipv4Packet, Ipv4Repr, UdpPacket, UdpRepr,
    },
};

pub(super) use crate::support::{frame::BoundedProvider, packet::prime_bounded_neighbor};

pub(super) const FIRST_MAC: [u8; 6] = [0x02, 0, 0, 0, 1, 2];
pub(super) const FIRST_PEER_MAC: [u8; 6] = [0x02, 0, 0, 0, 1, 1];
pub(super) const FIRST_IP: [u8; 4] = [10, 0, 1, 2];
pub(super) const FIRST_PEER_IP: [u8; 4] = [10, 0, 1, 1];
pub(super) const SECOND_MAC: [u8; 6] = [0x02, 0, 0, 0, 2, 2];
pub(super) const SECOND_PEER_MAC: [u8; 6] = [0x02, 0, 0, 0, 2, 1];
pub(super) const SECOND_IP: [u8; 4] = [10, 0, 2, 2];
pub(super) const SECOND_PEER_IP: [u8; 4] = [10, 0, 2, 1];
pub(super) const LOCAL_IP: [u8; 4] = [127, 0, 0, 1];
pub(super) const ENDPOINT_PAYLOAD_CAPACITY: usize = 128;

pub(super) fn lifecycle_limits() -> UdpEndpointLimits {
    UdpEndpointLimits::new(1, 1, 32)
}

pub(super) fn host_stack(capacity: usize, first: u16, last: u16) -> Stack {
    Stack::new_for_host_validation(UdpNamespacePolicy::new(capacity, first, last))
}

pub(super) fn standard_stack() -> Stack {
    host_stack(64, 32768, 60999)
}

pub(super) fn create_unbound(stack: &mut Stack) -> anemone_smoltcp_stack::HostEndpointId {
    stack
        .create_unbound_udp_endpoint_for_host_validation(lifecycle_limits())
        .unwrap()
}

pub(super) fn bind_for_host(
    stack: &mut Stack,
    endpoint: anemone_smoltcp_stack::HostEndpointId,
    address: [u8; 4],
    port: u16,
) -> Result<anemone_net_api::udp::UdpLocalBinding, UdpBindError> {
    stack.bind_udp_endpoint_for_host_validation(
        endpoint,
        UdpBindRequest::new(ApiIpv4Address::new(address), port),
    )
}

pub(super) fn selection(interface: InterfaceId, source: [u8; 4]) -> HostSelection {
    HostSelection { interface, source }
}

pub(super) fn assert_udp_frame(
    frame: &[u8],
    source: [u8; 4],
    destination: [u8; 4],
    source_port: u16,
    destination_port: u16,
    payload: &[u8],
) {
    let ethernet = EthernetFrame::new_checked(frame).unwrap();
    let ipv4 = Ipv4Packet::new_checked(ethernet.payload()).unwrap();
    assert_eq!(ipv4.src_addr(), Ipv4Address::from_octets(source));
    assert_eq!(ipv4.dst_addr(), Ipv4Address::from_octets(destination));
    let udp = UdpPacket::new_checked(ipv4.payload()).unwrap();
    assert_eq!(udp.src_port(), source_port);
    assert_eq!(udp.dst_port(), destination_port);
    assert_eq!(udp.payload(), payload);
}

pub(super) fn udp_datagram(
    source: [u8; 4],
    destination: [u8; 4],
    source_port: u16,
    destination_port: u16,
    payload: &[u8],
) -> Vec<u8> {
    let source = IpAddress::Ipv4(Ipv4Address::from_octets(source));
    let destination = IpAddress::Ipv4(Ipv4Address::from_octets(destination));
    let repr = UdpRepr {
        src_port: source_port,
        dst_port: destination_port,
    };
    let mut bytes = vec![0; repr.header_len() + payload.len()];
    repr.emit(
        &mut UdpPacket::new_unchecked(&mut bytes[..]),
        &source,
        &destination,
        payload.len(),
        |target| target.copy_from_slice(payload),
        &ChecksumCapabilities::default(),
    );
    bytes
}

#[allow(clippy::too_many_arguments)]
pub(super) fn ethernet_ipv4_fragment(
    source_mac: [u8; 6],
    destination_mac: [u8; 6],
    source_ip: [u8; 4],
    destination_ip: [u8; 4],
    ident: u16,
    fragment_offset: u16,
    more_fragments: bool,
    payload: &[u8],
) -> Vec<u8> {
    let ethernet = EthernetRepr {
        src_addr: SmoltcpEthernetAddress::from_bytes(&source_mac),
        dst_addr: SmoltcpEthernetAddress::from_bytes(&destination_mac),
        ethertype: EthernetProtocol::Ipv4,
    };
    let ipv4 = Ipv4Repr {
        src_addr: Ipv4Address::from_octets(source_ip),
        dst_addr: Ipv4Address::from_octets(destination_ip),
        next_header: IpProtocol::Udp,
        payload_len: payload.len(),
        hop_limit: 64,
    };
    let ip_offset = ethernet.buffer_len();
    let payload_offset = ip_offset + ipv4.buffer_len();
    let mut bytes = vec![0; payload_offset + payload.len()];
    ethernet.emit(&mut EthernetFrame::new_unchecked(&mut bytes[..]));
    {
        let mut packet = Ipv4Packet::new_unchecked(&mut bytes[ip_offset..]);
        ipv4.emit(&mut packet, &ChecksumCapabilities::default());
        packet.set_ident(ident);
        packet.set_dont_frag(false);
        packet.set_more_frags(more_fragments);
        packet.set_frag_offset(fragment_offset);
        packet.fill_checksum();
    }
    bytes[payload_offset..].copy_from_slice(payload);
    bytes
}

pub(super) fn pump_local(stack: &mut Stack, interface: InterfaceId, tick: i64) {
    stack
        .pump_local_for_host_validation(
            interface,
            Instant::from_micros(tick),
            PumpBudget::new(1, 1),
        )
        .unwrap();
}
