use anemone_net_api::{
    EthernetAddress, Recheck,
    icmp_raw::IcmpRawEndpointLimits,
    udp::{UdpErrorCause, UdpQueryError},
};
use smoltcp::wire::{Icmpv4DstUnreachable, Icmpv4Message, Icmpv4Packet, Icmpv4Repr};

use super::fixture::*;

const ERROR_SOURCE_PORT: u16 = 46_040;
const ERROR_DESTINATION_PORT: u16 = 53;

fn external_error_stack() -> (Stack, BoundedProvider, InterfaceId) {
    let mut stack = Stack::new();
    let mut provider = BoundedProvider::with_capacities(FIRST_MAC, 4, 4);
    let interface = stack.add_interface(
        &mut provider,
        EthernetAddress::new(FIRST_MAC),
        Instant::ZERO,
    );
    stack
        .configure_ipv4_for_host_validation(interface, FIRST_IP, 24)
        .unwrap();
    (stack, provider, interface)
}

fn icmp_error_frame(declared_inner_total_len: u16) -> Vec<u8> {
    let udp = udp_datagram(
        FIRST_IP,
        FIRST_PEER_IP,
        ERROR_SOURCE_PORT,
        ERROR_DESTINATION_PORT,
        b"trailing",
    );
    let inner = Ipv4Repr {
        src_addr: Ipv4Address::from_octets(FIRST_IP),
        dst_addr: Ipv4Address::from_octets(FIRST_PEER_IP),
        next_header: IpProtocol::Udp,
        payload_len: udp.len(),
        hop_limit: 64,
    };
    let icmp = Icmpv4Repr::DstUnreachable {
        reason: Icmpv4DstUnreachable::PortUnreachable,
        header: inner,
        data: &udp,
    };
    let outer = Ipv4Repr {
        src_addr: Ipv4Address::from_octets(FIRST_PEER_IP),
        dst_addr: Ipv4Address::from_octets(FIRST_IP),
        next_header: IpProtocol::Icmp,
        payload_len: icmp.buffer_len(),
        hop_limit: 64,
    };
    let ethernet = EthernetRepr {
        src_addr: SmoltcpEthernetAddress::from_bytes(&FIRST_PEER_MAC),
        dst_addr: SmoltcpEthernetAddress::from_bytes(&FIRST_MAC),
        ethertype: EthernetProtocol::Ipv4,
    };
    let mut frame = vec![0; ethernet.buffer_len() + outer.buffer_len() + icmp.buffer_len()];
    ethernet.emit(&mut EthernetFrame::new_unchecked(&mut frame[..]));
    let mut outer_packet = Ipv4Packet::new_unchecked(&mut frame[ethernet.buffer_len()..]);
    outer.emit(&mut outer_packet, &ChecksumCapabilities::default());
    icmp.emit(
        &mut Icmpv4Packet::new_unchecked(outer_packet.payload_mut()),
        &ChecksumCapabilities::default(),
    );
    let mut icmp_packet = Icmpv4Packet::new_unchecked(outer_packet.payload_mut());
    let mut inner_packet = Ipv4Packet::new_unchecked(icmp_packet.data_mut());
    inner_packet.set_total_len(declared_inner_total_len);
    inner_packet.fill_checksum();
    icmp_packet.fill_checksum();
    frame
}

fn mutate_quoted_ipv4(frame: &mut [u8], mutation: impl FnOnce(&mut Ipv4Packet<&mut [u8]>)) {
    let mut ethernet = EthernetFrame::new_unchecked(frame);
    let mut outer = Ipv4Packet::new_unchecked(ethernet.payload_mut());
    let mut icmp = Icmpv4Packet::new_unchecked(outer.payload_mut());
    let mut inner = Ipv4Packet::new_unchecked(icmp.data_mut());
    mutation(&mut inner);
    inner.fill_checksum();
    icmp.fill_checksum();
}

fn mutate_icmp_header(frame: &mut [u8], message: Icmpv4Message, code: u8) {
    let mut ethernet = EthernetFrame::new_unchecked(frame);
    let mut outer = Ipv4Packet::new_unchecked(ethernet.payload_mut());
    let mut icmp = Icmpv4Packet::new_unchecked(outer.payload_mut());
    icmp.set_msg_type(message);
    icmp.set_msg_code(code);
    icmp.fill_checksum();
}

fn inject_external(
    stack: &mut Stack,
    provider: &mut BoundedProvider,
    interface: InterfaceId,
    frame: &[u8],
) {
    provider.inject(frame);
    stack
        .pump(interface, provider, Instant::ZERO, PumpBudget::new(1, 1))
        .unwrap();
}

#[test]
fn local_closed_port_drives_endpoint_pending_and_error_fifo_through_normal_ingress() {
    let mut stack = standard_stack();
    let local = stack.add_local_ipv4_for_host_validation(LOCAL_IP, 8, 4, 128, Instant::ZERO);
    let client = stack
        .create_udp_endpoint_for_host_validation(46_000, 2, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();
    stack
        .set_udp_receive_errors_for_host_validation(client, true)
        .unwrap();

    stack
        .send_udp_for_host_validation(
            client,
            Some(selection(local, LOCAL_IP)),
            LOCAL_IP,
            46_001,
            b"marker",
        )
        .unwrap();
    for tick in 1..=4 {
        pump_local(&mut stack, local, tick);
    }

    let facts = stack
        .udp_endpoint_facts_for_host_validation(client)
        .unwrap();
    assert!(facts.has_error());
    assert_eq!(
        stack
            .take_udp_pending_error_for_host_validation(client)
            .unwrap(),
        Some(UdpErrorCause::PortUnreachable)
    );
    let record = stack
        .detach_udp_error_for_host_validation(client)
        .unwrap()
        .expect("normal local ICMP ingress must enqueue one UDP error record");
    assert_eq!(record.cause(), UdpErrorCause::PortUnreachable);
    assert_eq!((record.icmp_type(), record.icmp_code()), (3, 3));
    assert_eq!(record.original_destination().address().octets(), LOCAL_IP);
    assert_eq!(record.original_destination().port(), 46_001);
    assert_eq!(record.offender().octets(), LOCAL_IP);
    assert_eq!(record.quoted_payload(), b"marker");
    assert_eq!(
        stack.detach_udp_error_for_host_validation(client).unwrap(),
        None
    );
    assert!(
        !stack
            .udp_endpoint_facts_for_host_validation(client)
            .unwrap()
            .has_error()
    );
}

#[test]
fn full_local_link_repolls_transferred_icmp_without_an_external_edge() {
    let mut stack = standard_stack();
    let local = stack.add_local_ipv4_for_host_validation(LOCAL_IP, 8, 1, 128, Instant::ZERO);
    let client = stack
        .create_udp_endpoint_for_host_validation(46_080, 2, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();
    stack
        .set_udp_receive_errors_for_host_validation(client, true)
        .unwrap();

    stack
        .send_udp_for_host_validation(
            client,
            Some(selection(local, LOCAL_IP)),
            LOCAL_IP,
            46_081,
            b"first",
        )
        .unwrap();
    let first = stack
        .pump_local_for_host_validation(local, Instant::from_micros(1), PumpBudget::new(1, 1))
        .unwrap();
    assert_eq!(first.recheck, Recheck::Immediate);
    assert_eq!(
        stack
            .local_link_observation_for_host_validation()
            .unwrap()
            .occupied_packets,
        1
    );

    stack
        .send_udp_for_host_validation(
            client,
            Some(selection(local, LOCAL_IP)),
            LOCAL_IP,
            46_081,
            b"second",
        )
        .unwrap();
    let blocked = stack
        .pump_local_for_host_validation(local, Instant::from_micros(2), PumpBudget::new(1, 1))
        .unwrap();
    assert!(blocked.work_remaining);
    assert_eq!(
        blocked.recheck,
        Recheck::Immediate,
        "normal ingress published by local transfer must drive a later bounded round"
    );
    assert_eq!(
        stack
            .take_udp_pending_error_for_host_validation(client)
            .unwrap(),
        None
    );

    // This call models the production worker consuming only the Immediate
    // continuation above; no protocol mutation, timer, or provider edge
    // occurs between the two rounds.
    stack
        .pump_local_for_host_validation(local, Instant::from_micros(3), PumpBudget::new(1, 1))
        .unwrap();
    assert_eq!(
        stack
            .take_udp_pending_error_for_host_validation(client)
            .unwrap(),
        Some(UdpErrorCause::PortUnreachable)
    );
}

#[test]
fn disabled_channel_drops_new_records_and_disable_purges_only_fifo() {
    let mut stack = standard_stack();
    let local = stack.add_local_ipv4_for_host_validation(LOCAL_IP, 8, 4, 128, Instant::ZERO);
    let client = stack
        .create_udp_endpoint_for_host_validation(46_010, 2, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();

    stack
        .send_udp_for_host_validation(
            client,
            Some(selection(local, LOCAL_IP)),
            LOCAL_IP,
            46_011,
            b"disabled",
        )
        .unwrap();
    for tick in 1..=4 {
        pump_local(&mut stack, local, tick);
    }
    assert_eq!(
        stack
            .take_udp_pending_error_for_host_validation(client)
            .unwrap(),
        None
    );
    assert_eq!(
        stack.detach_udp_error_for_host_validation(client).unwrap(),
        None
    );

    stack
        .set_udp_receive_errors_for_host_validation(client, true)
        .unwrap();
    stack
        .send_udp_for_host_validation(
            client,
            Some(selection(local, LOCAL_IP)),
            LOCAL_IP,
            46_011,
            b"enabled",
        )
        .unwrap();
    for tick in 5..=8 {
        pump_local(&mut stack, local, tick);
    }
    stack
        .set_udp_receive_errors_for_host_validation(client, false)
        .unwrap();
    assert_eq!(
        stack.detach_udp_error_for_host_validation(client).unwrap(),
        None
    );
    assert_eq!(
        stack
            .take_udp_pending_error_for_host_validation(client)
            .unwrap(),
        Some(UdpErrorCause::PortUnreachable)
    );
}

#[test]
fn fifo_is_ordered_bounded_drop_new_while_pending_tracks_latest_error() {
    let mut stack = standard_stack();
    let local = stack.add_local_ipv4_for_host_validation(LOCAL_IP, 8, 4, 128, Instant::ZERO);
    let client = stack
        .create_udp_endpoint_for_host_validation(46_020, 1, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();
    stack
        .set_udp_receive_errors_for_host_validation(client, true)
        .unwrap();

    for (marker, first_tick) in [(b"first".as_slice(), 1), (b"second".as_slice(), 5)] {
        stack
            .send_udp_for_host_validation(
                client,
                Some(selection(local, LOCAL_IP)),
                LOCAL_IP,
                46_021,
                marker,
            )
            .unwrap();
        for tick in first_tick..first_tick + 4 {
            pump_local(&mut stack, local, tick);
        }
        assert_eq!(
            stack
                .take_udp_pending_error_for_host_validation(client)
                .unwrap(),
            Some(UdpErrorCause::PortUnreachable)
        );
    }

    let retained = stack
        .detach_udp_error_for_host_validation(client)
        .unwrap()
        .unwrap();
    assert_eq!(retained.quoted_payload(), b"first");
    assert_eq!(
        stack.detach_udp_error_for_host_validation(client).unwrap(),
        None
    );
}

#[test]
fn connected_peer_filter_rejects_error_for_an_explicit_other_destination() {
    let mut stack = standard_stack();
    let local = stack.add_local_ipv4_for_host_validation(LOCAL_IP, 8, 4, 128, Instant::ZERO);
    let client = stack
        .create_udp_endpoint_for_host_validation(46_030, 2, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();
    stack
        .connect_udp_endpoint_for_host_validation(
            client,
            selection(local, LOCAL_IP),
            LOCAL_IP,
            46_031,
        )
        .unwrap();
    stack
        .set_udp_receive_errors_for_host_validation(client, true)
        .unwrap();
    stack
        .send_udp_for_host_validation(
            client,
            Some(selection(local, LOCAL_IP)),
            LOCAL_IP,
            46_032,
            b"wrong-peer",
        )
        .unwrap();
    for tick in 1..=4 {
        pump_local(&mut stack, local, tick);
    }
    assert_eq!(
        stack
            .take_udp_pending_error_for_host_validation(client)
            .unwrap(),
        None
    );
    assert_eq!(
        stack.detach_udp_error_for_host_validation(client).unwrap(),
        None
    );
}

#[test]
fn quoted_total_len_must_contain_the_complete_udp_header() {
    let (mut stack, mut provider, interface) = external_error_stack();
    let endpoint = stack
        .create_udp_endpoint_for_host_validation(ERROR_SOURCE_PORT, 2, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();
    stack
        .set_udp_receive_errors_for_host_validation(endpoint, true)
        .unwrap();

    let frame = icmp_error_frame(20 + 7);
    inject_external(&mut stack, &mut provider, interface, &frame);
    assert_eq!(
        stack
            .take_udp_pending_error_for_host_validation(endpoint)
            .unwrap(),
        None
    );
    assert_eq!(
        stack
            .detach_udp_error_for_host_validation(endpoint)
            .unwrap(),
        None
    );
}

#[test]
fn quoted_total_len_excludes_trailing_icmp_bytes_from_udp_payload() {
    let (mut stack, mut provider, interface) = external_error_stack();
    let endpoint = stack
        .create_udp_endpoint_for_host_validation(ERROR_SOURCE_PORT, 2, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();
    stack
        .set_udp_receive_errors_for_host_validation(endpoint, true)
        .unwrap();

    let frame = icmp_error_frame(20 + 8);
    inject_external(&mut stack, &mut provider, interface, &frame);
    let record = stack
        .detach_udp_error_for_host_validation(endpoint)
        .unwrap()
        .expect("a complete quoted UDP header must still identify its Endpoint");
    assert_eq!(record.quoted_payload(), b"");
}

#[test]
fn malformed_non_udp_and_fragmented_quotes_do_not_publish_errors() {
    let (mut stack, mut provider, interface) = external_error_stack();
    let endpoint = stack
        .create_udp_endpoint_for_host_validation(ERROR_SOURCE_PORT, 2, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();
    stack
        .set_udp_receive_errors_for_host_validation(endpoint, true)
        .unwrap();

    let mut bad_checksum = icmp_error_frame(20 + 8);
    let icmp_checksum = EthernetFrame::<&[u8]>::header_len() + 20 + 2;
    bad_checksum[icmp_checksum] ^= 0xff;
    inject_external(&mut stack, &mut provider, interface, &bad_checksum);

    let mut non_udp = icmp_error_frame(20 + 8);
    mutate_quoted_ipv4(&mut non_udp, |inner| inner.set_next_header(IpProtocol::Tcp));
    inject_external(&mut stack, &mut provider, interface, &non_udp);

    let mut fragmented = icmp_error_frame(20 + 8);
    mutate_quoted_ipv4(&mut fragmented, |inner| inner.set_more_frags(true));
    inject_external(&mut stack, &mut provider, interface, &fragmented);

    assert_eq!(
        stack
            .take_udp_pending_error_for_host_validation(endpoint)
            .unwrap(),
        None
    );
    assert_eq!(
        stack
            .detach_udp_error_for_host_validation(endpoint)
            .unwrap(),
        None
    );
}

#[test]
fn unsupported_icmp_error_codes_do_not_publish_errors() {
    let (mut stack, mut provider, interface) = external_error_stack();
    let endpoint = stack
        .create_udp_endpoint_for_host_validation(ERROR_SOURCE_PORT, 2, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();
    stack
        .set_udp_receive_errors_for_host_validation(endpoint, true)
        .unwrap();

    let mut unknown_destination_unreachable = icmp_error_frame(20 + 8);
    mutate_icmp_header(
        &mut unknown_destination_unreachable,
        Icmpv4Message::DstUnreachable,
        16,
    );
    inject_external(
        &mut stack,
        &mut provider,
        interface,
        &unknown_destination_unreachable,
    );

    let mut fragment_reassembly_timeout = icmp_error_frame(20 + 8);
    mutate_icmp_header(
        &mut fragment_reassembly_timeout,
        Icmpv4Message::TimeExceeded,
        1,
    );
    inject_external(
        &mut stack,
        &mut provider,
        interface,
        &fragment_reassembly_timeout,
    );

    assert_eq!(
        stack
            .take_udp_pending_error_for_host_validation(endpoint)
            .unwrap(),
        None
    );
    assert_eq!(
        stack
            .detach_udp_error_for_host_validation(endpoint)
            .unwrap(),
        None
    );
}

#[test]
fn specific_binding_raw_fanout_and_retire_reuse_keep_independent_owners() {
    let mut stack = standard_stack();
    let local = stack.add_local_ipv4_for_host_validation(LOCAL_IP, 8, 4, 128, Instant::ZERO);
    let raw = stack
        .create_icmp_raw_endpoint(IcmpRawEndpointLimits::new(1, 128, 1, 128))
        .unwrap();
    let endpoint = create_unbound(&mut stack);
    bind_for_host(&mut stack, endpoint, LOCAL_IP, 46_050).unwrap();
    stack
        .set_udp_receive_errors_for_host_validation(endpoint, true)
        .unwrap();
    stack
        .send_udp_for_host_validation(
            endpoint,
            Some(selection(local, LOCAL_IP)),
            LOCAL_IP,
            46_051,
            b"shared-admission",
        )
        .unwrap();
    for tick in 1..=4 {
        pump_local(&mut stack, local, tick);
    }

    assert!(stack.receive_icmp_raw_endpoint(raw, false).is_ok());
    assert!(
        stack
            .detach_udp_error_for_host_validation(endpoint)
            .unwrap()
            .is_some()
    );
    stack
        .retire_udp_endpoint_for_host_validation(endpoint)
        .unwrap();

    let replacement = create_unbound(&mut stack);
    bind_for_host(&mut stack, replacement, LOCAL_IP, 46_050).unwrap();
    stack
        .set_udp_receive_errors_for_host_validation(replacement, true)
        .unwrap();
    assert_eq!(
        stack
            .take_udp_pending_error_for_host_validation(replacement)
            .unwrap(),
        None
    );
    assert_eq!(
        stack
            .detach_udp_error_for_host_validation(replacement)
            .unwrap(),
        None
    );
    assert_eq!(
        stack.udp_endpoint_facts_for_host_validation(endpoint),
        Err(UdpQueryError::UnknownEndpoint)
    );
}

#[test]
fn ordinary_send_consumes_pending_without_consuming_error_fifo() {
    let mut stack = standard_stack();
    let local = stack.add_local_ipv4_for_host_validation(LOCAL_IP, 8, 4, 128, Instant::ZERO);
    let client = stack
        .create_udp_endpoint_for_host_validation(46_060, 2, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();
    stack
        .set_udp_receive_errors_for_host_validation(client, true)
        .unwrap();
    stack
        .send_udp_for_host_validation(
            client,
            Some(selection(local, LOCAL_IP)),
            LOCAL_IP,
            46_061,
            b"first",
        )
        .unwrap();
    for tick in 1..=4 {
        pump_local(&mut stack, local, tick);
    }

    assert_eq!(
        stack.send_udp_for_host_validation(
            client,
            Some(selection(local, LOCAL_IP)),
            LOCAL_IP,
            46_061,
            b"must-not-commit",
        ),
        Err(HostSendError::Pending(UdpErrorCause::PortUnreachable))
    );
    assert!(
        stack
            .detach_udp_error_for_host_validation(client)
            .unwrap()
            .is_some()
    );
}

#[test]
fn queued_datagram_precedes_pending_error_without_consuming_it() {
    let mut stack = standard_stack();
    let local = stack.add_local_ipv4_for_host_validation(LOCAL_IP, 8, 8, 256, Instant::ZERO);
    let client = stack
        .create_udp_endpoint_for_host_validation(46_070, 2, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();
    let sender = stack
        .create_udp_endpoint_for_host_validation(46_071, 2, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();
    stack
        .set_udp_receive_errors_for_host_validation(client, true)
        .unwrap();

    stack
        .send_udp_for_host_validation(
            sender,
            Some(selection(local, LOCAL_IP)),
            LOCAL_IP,
            46_070,
            b"queued-data",
        )
        .unwrap();
    for tick in 1..=2 {
        pump_local(&mut stack, local, tick);
    }
    stack
        .send_udp_for_host_validation(
            client,
            Some(selection(local, LOCAL_IP)),
            LOCAL_IP,
            46_072,
            b"error",
        )
        .unwrap();
    for tick in 3..=6 {
        pump_local(&mut stack, local, tick);
    }

    let datagram = stack
        .receive_udp_for_host_validation(client)
        .expect("queued data must be delivered before consulting pending error");
    assert_eq!(datagram.payload, b"queued-data");
    assert_eq!(
        stack
            .take_udp_pending_error_for_host_validation(client)
            .unwrap(),
        Some(UdpErrorCause::PortUnreachable)
    );
}
