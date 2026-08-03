mod support;

use anemone_net_api::{
    EthernetAddress, FrameProvider, Instant, Ipv4Address, TransmitOutcome, TxToken,
    icmp_raw::{
        IcmpRawAssociation, IcmpRawEgressPolicy, IcmpRawEgressSelection, IcmpRawEndpointLimits,
        IcmpRawMutationError, IcmpRawQueryError, IcmpRawReceiveError, IcmpRawRetireError,
        IcmpRawSendError, IcmpRawTypeFilter,
    },
};
use anemone_smoltcp_stack::{HostSelection, PumpBudget, Stack};
use smoltcp::{
    phy::ChecksumCapabilities,
    wire::{
        EthernetAddress as SmoltcpEthernetAddress, EthernetFrame, EthernetProtocol, EthernetRepr,
        Icmpv4Packet, Icmpv4Repr, IpProtocol, Ipv4Address as SmoltcpIpv4Address, Ipv4Packet,
    },
};

use support::{frame::BoundedProvider, packet::prime_bounded_neighbor};

const LOCAL_MAC: [u8; 6] = [0x02, 0, 0, 0, 0, 1];
const PEER_MAC: [u8; 6] = [0x02, 0, 0, 0, 0, 2];
const LOCAL_IP: [u8; 4] = [10, 0, 0, 2];
const PEER_IP: [u8; 4] = [10, 0, 0, 1];

fn limits(
    tx_packets: usize,
    tx_bytes: usize,
    rx_packets: usize,
    rx_bytes: usize,
) -> IcmpRawEndpointLimits {
    IcmpRawEndpointLimits::new(tx_packets, tx_bytes, rx_packets, rx_bytes)
}

fn configured_external() -> (Stack, BoundedProvider, anemone_net_api::InterfaceId) {
    let mut stack = Stack::new();
    let mut provider = BoundedProvider::with_capacities(LOCAL_MAC, 4, 4);
    let interface = stack.add_interface(
        &mut provider,
        EthernetAddress::new(LOCAL_MAC),
        Instant::ZERO,
    );
    stack
        .configure_ipv4_for_host_validation(interface, LOCAL_IP, 24)
        .unwrap();
    (stack, provider, interface)
}

fn icmp_frame(
    destination_ip: [u8; 4],
    tos: u8,
    identification: u16,
    with_options: bool,
    more_fragments: bool,
    valid_icmp_checksum: bool,
) -> Vec<u8> {
    let data = [0xaa, 0xbb, 0xcc, 0xdd];
    let icmp = Icmpv4Repr::EchoRequest {
        ident: 0x1234,
        seq_no: 7,
        data: &data,
    };
    let ethernet = EthernetRepr {
        src_addr: SmoltcpEthernetAddress::from_bytes(&PEER_MAC),
        dst_addr: SmoltcpEthernetAddress::from_bytes(&LOCAL_MAC),
        ethertype: EthernetProtocol::Ipv4,
    };
    let ip_header_len = if with_options { 24 } else { 20 };
    let ip_total_len = ip_header_len + icmp.buffer_len();
    let mut bytes = vec![0; ethernet.buffer_len() + ip_total_len];
    ethernet.emit(&mut EthernetFrame::new_unchecked(&mut bytes[..]));
    let ip_offset = ethernet.buffer_len();
    {
        let mut ipv4 = Ipv4Packet::new_unchecked(&mut bytes[ip_offset..]);
        ipv4.set_version(4);
        ipv4.set_header_len(ip_header_len as u8);
        ipv4.set_dscp(tos >> 2);
        ipv4.set_ecn(tos & 3);
        ipv4.set_total_len(ip_total_len as u16);
        ipv4.set_ident(identification);
        ipv4.clear_flags();
        ipv4.set_dont_frag(true);
        ipv4.set_more_frags(more_fragments);
        ipv4.set_frag_offset(0);
        ipv4.set_hop_limit(37);
        ipv4.set_next_header(IpProtocol::Icmp);
        ipv4.set_src_addr(SmoltcpIpv4Address::from_octets(PEER_IP));
        ipv4.set_dst_addr(SmoltcpIpv4Address::from_octets(destination_ip));
    }
    if with_options {
        bytes[ip_offset + 20..ip_offset + 24].copy_from_slice(&[1, 1, 1, 0]);
    }
    {
        let mut ipv4 = Ipv4Packet::new_unchecked(&mut bytes[ip_offset..]);
        icmp.emit(
            &mut Icmpv4Packet::new_unchecked(ipv4.payload_mut()),
            &ChecksumCapabilities::default(),
        );
        if !valid_icmp_checksum {
            ipv4.payload_mut()[2] ^= 0xff;
        }
        ipv4.fill_checksum();
    }
    bytes
}

fn inject_one(
    stack: &mut Stack,
    provider: &mut BoundedProvider,
    interface: anemone_net_api::InterfaceId,
    frame: &[u8],
) {
    provider.inject(frame);
    stack
        .pump(interface, provider, Instant::ZERO, PumpBudget::new(1, 1))
        .unwrap();
}

#[test]
fn post_admission_observer_preserves_original_bytes_and_keeps_ordinary_icmp() {
    let (mut stack, mut provider, interface) = configured_external();
    let endpoint = stack
        .create_icmp_raw_endpoint(limits(2, 256, 2, 256))
        .unwrap();

    let wrong_destination = icmp_frame([10, 0, 0, 99], 0, 1, false, false, true);
    inject_one(&mut stack, &mut provider, interface, &wrong_destination);
    assert_eq!(
        stack.receive_icmp_raw_endpoint(endpoint, false),
        Err(IcmpRawReceiveError::WouldBlock)
    );
    assert_eq!(provider.submissions(), 0);

    let admitted = icmp_frame(LOCAL_IP, 0xb9, 0x4567, true, false, true);
    inject_one(&mut stack, &mut provider, interface, &admitted);

    let ethernet = EthernetFrame::new_checked(&admitted[..]).unwrap();
    let expected = &ethernet.payload()[..usize::from(
        Ipv4Packet::new_checked(ethernet.payload())
            .unwrap()
            .total_len(),
    )];
    let received = stack.receive_icmp_raw_endpoint(endpoint, false).unwrap();
    assert_eq!(received.bytes(), expected);
    let ipv4 = Ipv4Packet::new_checked(received.bytes()).unwrap();
    assert_eq!(ipv4.header_len(), 24);
    assert_eq!(ipv4.dscp(), 0xb9 >> 2);
    assert_eq!(ipv4.ecn(), 0xb9 & 3);
    assert_eq!(ipv4.ident(), 0x4567);
    assert!(ipv4.dont_frag());
    assert_eq!(&received.bytes()[20..24], &[1, 1, 1, 0]);

    // The raw observation is non-exclusive: the ordinary ICMP owner still
    // emits the echo reply through the normal provider path.
    assert_eq!(provider.submissions(), 1);
}

#[test]
fn fragment_is_excluded_while_invalid_icmp_body_remains_raw_visible() {
    let (mut stack, mut provider, interface) = configured_external();
    let endpoint = stack
        .create_icmp_raw_endpoint(limits(1, 128, 2, 256))
        .unwrap();

    let fragment = icmp_frame(LOCAL_IP, 0, 1, false, true, false);
    inject_one(&mut stack, &mut provider, interface, &fragment);
    assert_eq!(
        stack.receive_icmp_raw_endpoint(endpoint, false),
        Err(IcmpRawReceiveError::WouldBlock)
    );

    let invalid_icmp = icmp_frame(LOCAL_IP, 0, 2, false, false, false);
    inject_one(&mut stack, &mut provider, interface, &invalid_icmp);
    let packet = stack.receive_icmp_raw_endpoint(endpoint, false).unwrap();
    assert_eq!(
        packet.bytes(),
        EthernetFrame::new_checked(&invalid_icmp[..])
            .unwrap()
            .payload()
    );
    assert_eq!(provider.submissions(), 0);
}

#[test]
fn fanout_filter_and_full_consumer_are_independently_accounted() {
    let (mut stack, mut provider, interface) = configured_external();
    let full = stack
        .create_icmp_raw_endpoint(limits(1, 128, 1, 128))
        .unwrap();
    let live = stack
        .create_icmp_raw_endpoint(limits(1, 128, 2, 256))
        .unwrap();
    let filtered = stack
        .create_icmp_raw_endpoint(limits(1, 128, 2, 256))
        .unwrap();
    let wrong_peer = stack
        .create_icmp_raw_endpoint(limits(1, 128, 2, 256))
        .unwrap();
    let matching_local = stack
        .create_icmp_raw_endpoint(limits(1, 128, 2, 256))
        .unwrap();
    let wrong_local = stack
        .create_icmp_raw_endpoint(limits(1, 128, 2, 256))
        .unwrap();
    stack
        .set_icmp_raw_filter(filtered, IcmpRawTypeFilter::from_blocked_types(1 << 8))
        .unwrap();
    stack
        .set_icmp_raw_association(
            wrong_peer,
            IcmpRawAssociation::new(None, Some(Ipv4Address::new([10, 0, 0, 77]))),
        )
        .unwrap();
    stack
        .set_icmp_raw_association(
            matching_local,
            IcmpRawAssociation::new(Some(Ipv4Address::new(LOCAL_IP)), None),
        )
        .unwrap();
    stack
        .set_icmp_raw_association(
            wrong_local,
            IcmpRawAssociation::new(Some(Ipv4Address::new([10, 0, 0, 77])), None),
        )
        .unwrap();

    let first = icmp_frame(LOCAL_IP, 0, 1, false, false, false);
    let second = icmp_frame(LOCAL_IP, 0, 2, false, false, false);
    inject_one(&mut stack, &mut provider, interface, &first);
    inject_one(&mut stack, &mut provider, interface, &second);

    assert!(stack.icmp_raw_endpoint_facts(full).unwrap().is_readable());
    assert_eq!(
        stack
            .icmp_raw_endpoint_diagnostics(full)
            .unwrap()
            .rx_capacity_packets(),
        1
    );
    assert_eq!(
        stack
            .receive_icmp_raw_endpoint(full, false)
            .unwrap()
            .bytes(),
        EthernetFrame::new_checked(&first[..]).unwrap().payload()
    );
    assert_eq!(
        stack
            .receive_icmp_raw_endpoint(live, false)
            .unwrap()
            .bytes(),
        EthernetFrame::new_checked(&first[..]).unwrap().payload()
    );
    assert_eq!(
        stack
            .receive_icmp_raw_endpoint(live, false)
            .unwrap()
            .bytes(),
        EthernetFrame::new_checked(&second[..]).unwrap().payload()
    );
    for frame in [&first, &second] {
        assert_eq!(
            stack
                .receive_icmp_raw_endpoint(matching_local, false)
                .unwrap()
                .bytes(),
            EthernetFrame::new_checked(&frame[..]).unwrap().payload()
        );
    }
    for endpoint in [filtered, wrong_peer, wrong_local] {
        assert_eq!(
            stack.receive_icmp_raw_endpoint(endpoint, false),
            Err(IcmpRawReceiveError::WouldBlock)
        );
    }
}

#[test]
fn peek_detach_invalidation_retire_and_stale_identity_are_owner_local() {
    let (mut stack, mut provider, interface) = configured_external();
    let endpoint = stack
        .create_icmp_raw_endpoint(limits(1, 128, 1, 128))
        .unwrap();
    assert_eq!(
        stack.take_icmp_raw_endpoint_invalidations(),
        vec![
            anemone_net_api::icmp_raw::IcmpRawEndpointInvalidation::from_owner_transition(endpoint)
        ]
    );

    let frame = icmp_frame(LOCAL_IP, 0, 1, false, false, false);
    inject_one(&mut stack, &mut provider, interface, &frame);
    let first_peek = stack.receive_icmp_raw_endpoint(endpoint, true).unwrap();
    let second_peek = stack.receive_icmp_raw_endpoint(endpoint, true).unwrap();
    assert_eq!(first_peek, second_peek);
    assert!(
        stack
            .icmp_raw_endpoint_facts(endpoint)
            .unwrap()
            .is_readable()
    );
    let detached = stack.receive_icmp_raw_endpoint(endpoint, false).unwrap();
    assert_eq!(detached, first_peek);
    assert!(
        !stack
            .icmp_raw_endpoint_facts(endpoint)
            .unwrap()
            .is_readable()
    );

    stack.retire_icmp_raw_endpoint(endpoint).unwrap();
    assert_eq!(
        stack.icmp_raw_endpoint_facts(endpoint),
        Err(IcmpRawQueryError::UnknownEndpoint)
    );
    assert_eq!(
        stack.retire_icmp_raw_endpoint(endpoint),
        Err(IcmpRawRetireError::UnknownEndpoint)
    );
    let replacement = stack
        .create_icmp_raw_endpoint(limits(1, 128, 1, 128))
        .unwrap();
    assert_ne!(replacement, endpoint);
    assert_eq!(
        stack.set_icmp_raw_filter(endpoint, IcmpRawTypeFilter::default()),
        Err(IcmpRawMutationError::UnknownEndpoint)
    );
}

#[test]
fn tx_commit_preserves_header_policy_capacity_and_normal_provider_path() {
    let (mut stack, mut provider, interface) = configured_external();
    prime_bounded_neighbor(
        &mut stack,
        interface,
        &mut provider,
        PEER_MAC,
        PEER_IP,
        LOCAL_IP,
    );
    provider.reset_observation();
    let endpoint = stack
        .create_icmp_raw_endpoint(limits(1, 128, 1, 128))
        .unwrap();
    let selection = IcmpRawEgressSelection::new(interface, Ipv4Address::new(LOCAL_IP));
    let destination = Ipv4Address::new(PEER_IP);
    let policy = IcmpRawEgressPolicy::new(37, 0xb9).unwrap();

    stack
        .send_icmp_raw_endpoint(endpoint, selection, destination, policy, &[])
        .unwrap();
    assert!(
        !stack
            .icmp_raw_endpoint_facts(endpoint)
            .unwrap()
            .is_writable()
    );
    assert_eq!(
        stack.send_icmp_raw_endpoint(endpoint, selection, destination, policy, &[]),
        Err(IcmpRawSendError::WouldBlock)
    );
    let outcome = stack
        .pump(
            interface,
            &mut provider,
            Instant::ZERO,
            PumpBudget::new(1, 1),
        )
        .unwrap();
    // Reaching the exact egress budget boundary is deliberately conservative:
    // the pump cannot prove that the socket set is empty until one later poll.
    assert!(outcome.work_remaining);
    assert!(
        stack
            .icmp_raw_endpoint_facts(endpoint)
            .unwrap()
            .is_writable()
    );
    assert_eq!(provider.submissions(), 1);
    let drained = stack
        .pump(
            interface,
            &mut provider,
            Instant::ZERO,
            PumpBudget::new(1, 1),
        )
        .unwrap();
    assert!(!drained.work_remaining);
    assert_eq!(provider.submissions(), 1);
    let ethernet = EthernetFrame::new_checked(&provider.submitted_frames()[0][..]).unwrap();
    let ipv4 = Ipv4Packet::new_checked(ethernet.payload()).unwrap();
    assert_eq!(ipv4.src_addr().octets(), LOCAL_IP);
    assert_eq!(ipv4.dst_addr().octets(), PEER_IP);
    assert_eq!(ipv4.hop_limit(), 37);
    assert_eq!(ipv4.dscp(), 0xb9 >> 2);
    assert_eq!(ipv4.ecn(), 0xb9 & 3);
    assert_eq!(ipv4.next_header(), IpProtocol::Icmp);
    assert_eq!(ipv4.header_len(), 20);
    assert_eq!(ipv4.total_len(), 20);
    assert!(ipv4.payload().is_empty());

    let maximum = 128 - EthernetFrame::<&[u8]>::header_len() - 20;
    assert_eq!(
        stack.send_icmp_raw_endpoint(
            endpoint,
            selection,
            destination,
            policy,
            &vec![0; maximum + 1],
        ),
        Err(IcmpRawSendError::MessageTooLong { maximum })
    );
    assert_eq!(
        stack.send_icmp_raw_endpoint(
            endpoint,
            selection,
            Ipv4Address::new([10, 0, 0, 255]),
            policy,
            &[],
        ),
        Err(IcmpRawSendError::InvalidDestination)
    );
    assert_eq!(
        stack.send_icmp_raw_endpoint(
            endpoint,
            IcmpRawEgressSelection::new(interface, Ipv4Address::new([10, 0, 1, 2])),
            destination,
            policy,
            &[],
        ),
        Err(IcmpRawSendError::UnsupportedSource)
    );
}

#[test]
fn protocol_egress_arbitration_does_not_starve_raw_behind_udp() {
    let (mut stack, mut provider, interface) = configured_external();
    prime_bounded_neighbor(
        &mut stack,
        interface,
        &mut provider,
        PEER_MAC,
        PEER_IP,
        LOCAL_IP,
    );
    provider.reset_observation();

    let udp_first = stack
        .create_udp_endpoint_for_host_validation(42000, 1, 32)
        .unwrap();
    let udp_later = stack
        .create_udp_endpoint_for_host_validation(42001, 1, 32)
        .unwrap();
    let selection = HostSelection {
        interface,
        source: LOCAL_IP,
    };
    stack
        .send_udp_for_host_validation(udp_first, Some(selection), PEER_IP, 43000, b"first")
        .unwrap();
    stack
        .send_udp_for_host_validation(udp_later, Some(selection), PEER_IP, 43001, b"later")
        .unwrap();

    let raw = stack
        .create_icmp_raw_endpoint(limits(1, 128, 1, 128))
        .unwrap();
    stack
        .send_icmp_raw_endpoint(
            raw,
            IcmpRawEgressSelection::new(interface, Ipv4Address::new(LOCAL_IP)),
            Ipv4Address::new(PEER_IP),
            IcmpRawEgressPolicy::new(64, 0).unwrap(),
            &[],
        )
        .unwrap();

    let first_outcome = stack
        .pump(
            interface,
            &mut provider,
            Instant::ZERO,
            PumpBudget::new(1, 4),
        )
        .unwrap();
    assert!(first_outcome.work_remaining);
    let second_outcome = stack
        .pump(
            interface,
            &mut provider,
            Instant::ZERO,
            PumpBudget::new(1, 4),
        )
        .unwrap();
    assert!(second_outcome.work_remaining);

    assert_eq!(provider.submissions(), 2);
    let protocols = provider
        .submitted_frames()
        .iter()
        .map(|frame| {
            let ethernet = EthernetFrame::new_checked(&frame[..]).unwrap();
            Ipv4Packet::new_checked(ethernet.payload())
                .unwrap()
                .next_header()
        })
        .collect::<Vec<_>>();
    assert_eq!(protocols, [IpProtocol::Udp, IpProtocol::Icmp]);
}

#[test]
fn blocked_raw_provider_does_not_gate_another_interface() {
    const SECOND_MAC: [u8; 6] = [0x02, 0, 0, 0, 1, 1];
    const SECOND_PEER_MAC: [u8; 6] = [0x02, 0, 0, 0, 1, 2];
    const SECOND_IP: [u8; 4] = [10, 0, 1, 2];
    const SECOND_PEER_IP: [u8; 4] = [10, 0, 1, 1];

    let (mut stack, mut first_provider, first_interface) = configured_external();
    let mut second_provider = BoundedProvider::with_capacities(SECOND_MAC, 2, 1);
    let second_interface = stack.add_interface(
        &mut second_provider,
        EthernetAddress::new(SECOND_MAC),
        Instant::ZERO,
    );
    stack
        .configure_ipv4_for_host_validation(second_interface, SECOND_IP, 24)
        .unwrap();
    prime_bounded_neighbor(
        &mut stack,
        first_interface,
        &mut first_provider,
        PEER_MAC,
        PEER_IP,
        LOCAL_IP,
    );
    prime_bounded_neighbor(
        &mut stack,
        second_interface,
        &mut second_provider,
        SECOND_PEER_MAC,
        SECOND_PEER_IP,
        SECOND_IP,
    );
    first_provider.reset_observation();
    second_provider.reset_observation();

    let endpoint = stack
        .create_icmp_raw_endpoint(limits(2, 256, 1, 128))
        .unwrap();
    stack
        .send_icmp_raw_endpoint(
            endpoint,
            IcmpRawEgressSelection::new(first_interface, Ipv4Address::new(LOCAL_IP)),
            Ipv4Address::new(PEER_IP),
            IcmpRawEgressPolicy::new(64, 0).unwrap(),
            &[],
        )
        .unwrap();
    stack
        .send_icmp_raw_endpoint(
            endpoint,
            IcmpRawEgressSelection::new(second_interface, Ipv4Address::new(SECOND_IP)),
            Ipv4Address::new(SECOND_PEER_IP),
            IcmpRawEgressPolicy::new(64, 0).unwrap(),
            &[],
        )
        .unwrap();

    let TransmitOutcome::Ready(blocker) = first_provider.transmit(Instant::ZERO) else {
        panic!("fresh first provider must expose one TX credit")
    };
    blocker.consume(1, |_| ()).unwrap();
    let blocked = stack
        .pump(
            first_interface,
            &mut first_provider,
            Instant::ZERO,
            PumpBudget::new(1, 1),
        )
        .unwrap();
    assert!(blocked.work_remaining);

    stack
        .pump(
            second_interface,
            &mut second_provider,
            Instant::ZERO,
            PumpBudget::new(1, 1),
        )
        .unwrap();
    assert_eq!(second_provider.submissions(), 1);
    assert!(
        stack
            .icmp_raw_endpoint_facts(endpoint)
            .unwrap()
            .is_writable()
    );

    first_provider.complete_all();
    stack
        .pump(
            first_interface,
            &mut first_provider,
            Instant::ZERO,
            PumpBudget::new(1, 1),
        )
        .unwrap();
    assert_eq!(first_provider.submissions(), 2);
    assert!(
        stack
            .icmp_raw_endpoint_facts(endpoint)
            .unwrap()
            .is_writable()
    );
}

#[test]
fn tx_admission_caps_configured_mtu_at_the_ipv4_length_ceiling() {
    let mut stack = Stack::new();
    let interface =
        stack.add_local_ipv4_for_host_validation([127, 0, 0, 1], 8, 2, 70_000, Instant::ZERO);
    let endpoint = stack
        .create_icmp_raw_endpoint(limits(1, u16::MAX as usize, 1, 128))
        .unwrap();
    let maximum = u16::MAX as usize - 20;
    assert_eq!(
        stack.send_icmp_raw_endpoint(
            endpoint,
            IcmpRawEgressSelection::new(interface, Ipv4Address::LOOPBACK),
            Ipv4Address::new([127, 0, 0, 2]),
            IcmpRawEgressPolicy::new(64, 0).unwrap(),
            &vec![0; maximum + 1],
        ),
        Err(IcmpRawSendError::MessageTooLong { maximum })
    );
    stack
        .send_icmp_raw_endpoint(
            endpoint,
            IcmpRawEgressSelection::new(interface, Ipv4Address::LOOPBACK),
            Ipv4Address::new([127, 0, 0, 2]),
            IcmpRawEgressPolicy::new(64, 0).unwrap(),
            &vec![0; maximum],
        )
        .unwrap();
    stack
        .pump_local_for_host_validation(interface, Instant::ZERO, PumpBudget::new(1, 1))
        .unwrap();
    assert!(
        stack
            .icmp_raw_endpoint_facts(endpoint)
            .unwrap()
            .is_writable()
    );
}

#[test]
fn tx_endpoint_byte_ceiling_is_permanent_while_occupancy_recovers() {
    let mut stack = Stack::new();
    let interface =
        stack.add_local_ipv4_for_host_validation([127, 0, 0, 1], 8, 2, 128, Instant::ZERO);
    let endpoint = stack
        .create_icmp_raw_endpoint(limits(1, 24, 1, 128))
        .unwrap();
    let selection = IcmpRawEgressSelection::new(interface, Ipv4Address::LOOPBACK);
    let destination = Ipv4Address::new([127, 0, 0, 2]);
    let policy = IcmpRawEgressPolicy::new(64, 0).unwrap();
    assert_eq!(
        stack.send_icmp_raw_endpoint(endpoint, selection, destination, policy, &[0; 5]),
        Err(IcmpRawSendError::MessageTooLong { maximum: 4 })
    );
    stack
        .send_icmp_raw_endpoint(endpoint, selection, destination, policy, &[0; 4])
        .unwrap();
    assert_eq!(
        stack.send_icmp_raw_endpoint(endpoint, selection, destination, policy, &[]),
        Err(IcmpRawSendError::WouldBlock)
    );
    stack
        .pump_local_for_host_validation(interface, Instant::ZERO, PumpBudget::new(1, 1))
        .unwrap();
    stack
        .send_icmp_raw_endpoint(endpoint, selection, destination, policy, &[])
        .unwrap();
}

#[test]
fn local_tx_requires_a_later_bounded_round_before_raw_ingress() {
    let mut stack = Stack::new();
    let interface =
        stack.add_local_ipv4_for_host_validation([127, 0, 0, 1], 8, 4, 128, Instant::ZERO);
    let endpoint = stack
        .create_icmp_raw_endpoint(limits(1, 128, 1, 128))
        .unwrap();
    stack
        .send_icmp_raw_endpoint(
            endpoint,
            IcmpRawEgressSelection::new(interface, Ipv4Address::LOOPBACK),
            Ipv4Address::new([127, 0, 0, 2]),
            IcmpRawEgressPolicy::new(64, 0).unwrap(),
            &[],
        )
        .unwrap();

    let first = stack
        .pump_local_for_host_validation(interface, Instant::ZERO, PumpBudget::new(1, 1))
        .unwrap();
    assert!(first.work_remaining);
    assert_eq!(
        stack.receive_icmp_raw_endpoint(endpoint, false),
        Err(IcmpRawReceiveError::WouldBlock)
    );
    stack
        .pump_local_for_host_validation(interface, Instant::ZERO, PumpBudget::new(1, 1))
        .unwrap();
    let packet = stack.receive_icmp_raw_endpoint(endpoint, false).unwrap();
    let ipv4 = Ipv4Packet::new_checked(packet.bytes()).unwrap();
    assert_eq!(ipv4.src_addr().octets(), [127, 0, 0, 1]);
    assert_eq!(ipv4.dst_addr().octets(), [127, 0, 0, 2]);
}
