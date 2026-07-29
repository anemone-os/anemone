mod support;

use anemone_net_api::{Instant, InterfaceId};
use anemone_smoltcp_stack::{
    HostEndpointCreateError, HostRetireError, HostSelection, HostSendError, PumpBudget, Stack,
};
use smoltcp::wire::{EthernetFrame, Ipv4Address, Ipv4Packet, UdpPacket};

use support::{BoundedProvider, prime_bounded_neighbor};

const FIRST_MAC: [u8; 6] = [0x02, 0, 0, 0, 1, 2];
const FIRST_PEER_MAC: [u8; 6] = [0x02, 0, 0, 0, 1, 1];
const FIRST_IP: [u8; 4] = [10, 0, 1, 2];
const FIRST_PEER_IP: [u8; 4] = [10, 0, 1, 1];
const SECOND_MAC: [u8; 6] = [0x02, 0, 0, 0, 2, 2];
const SECOND_PEER_MAC: [u8; 6] = [0x02, 0, 0, 0, 2, 1];
const SECOND_IP: [u8; 4] = [10, 0, 2, 2];
const SECOND_PEER_IP: [u8; 4] = [10, 0, 2, 1];
const LOCAL_IP: [u8; 4] = [127, 0, 0, 1];
const ENDPOINT_PAYLOAD_CAPACITY: usize = 128;

fn selection(interface: InterfaceId, source: [u8; 4]) -> HostSelection {
    HostSelection { interface, source }
}

fn assert_udp_frame(
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

fn pump_local(stack: &mut Stack, interface: InterfaceId, tick: i64) {
    stack
        .pump_local_for_host_validation(
            interface,
            Instant::from_micros(tick),
            PumpBudget::new(1, 1),
        )
        .unwrap();
}

#[test]
fn selected_external_interface_is_the_only_engine_that_can_consume() {
    let mut stack = Stack::new();
    let mut first = BoundedProvider::with_mac(FIRST_MAC, 1);
    let mut second = BoundedProvider::with_mac(SECOND_MAC, 1);
    let first_id = stack.add_interface(
        &mut first,
        anemone_net_api::EthernetAddress::new(FIRST_MAC),
        Instant::ZERO,
    );
    let second_id = stack.add_interface(
        &mut second,
        anemone_net_api::EthernetAddress::new(SECOND_MAC),
        Instant::ZERO,
    );
    stack
        .configure_ipv4_for_host_validation(first_id, FIRST_IP, 24)
        .unwrap();
    stack
        .configure_ipv4_for_host_validation(second_id, SECOND_IP, 24)
        .unwrap();
    prime_bounded_neighbor(
        &mut stack,
        first_id,
        &mut first,
        FIRST_PEER_MAC,
        FIRST_PEER_IP,
        FIRST_IP,
    );
    prime_bounded_neighbor(
        &mut stack,
        second_id,
        &mut second,
        SECOND_PEER_MAC,
        SECOND_PEER_IP,
        SECOND_IP,
    );
    first.reset_observation();
    second.reset_observation();

    let local_id = stack.add_local_ipv4_for_host_validation(LOCAL_IP, 8, 2, 128, Instant::ZERO);
    let endpoint = stack
        .create_udp_endpoint_for_host_validation(40000, 4, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();
    let mut stale_provider = BoundedProvider::with_mac([0x02, 0, 0, 0, 3, 2], 1);
    let stale_id = stack.add_interface(
        &mut stale_provider,
        anemone_net_api::EthernetAddress::new([0x02, 0, 0, 0, 3, 2]),
        Instant::ZERO,
    );
    stack
        .configure_ipv4_for_host_validation(stale_id, [10, 0, 3, 2], 24)
        .unwrap();
    assert_eq!(
        stack
            .udp_endpoint_observation_for_host_validation(endpoint)
            .unwrap()
            .engine_resources,
        4
    );
    stack.remove_interface(stale_id).unwrap();
    assert_eq!(
        stack
            .udp_endpoint_observation_for_host_validation(endpoint)
            .unwrap()
            .engine_resources,
        3
    );

    assert_eq!(
        stack.send_udp_for_host_validation(endpoint, None, SECOND_PEER_IP, 40001, b"missing"),
        Err(HostSendError::MissingSelection)
    );
    assert_eq!(
        stack.send_udp_for_host_validation(
            endpoint,
            Some(selection(stale_id, [10, 0, 3, 2])),
            SECOND_PEER_IP,
            40001,
            b"stale",
        ),
        Err(HostSendError::UnknownInterface)
    );
    assert_eq!(
        stack.send_udp_for_host_validation(
            endpoint,
            Some(selection(second_id, FIRST_IP)),
            SECOND_PEER_IP,
            40001,
            b"source",
        ),
        Err(HostSendError::UnsupportedSource)
    );
    assert_eq!(
        stack.send_udp_for_host_validation(
            endpoint,
            Some(selection(second_id, SECOND_IP)),
            SECOND_PEER_IP,
            40001,
            &[0; 87],
        ),
        Err(HostSendError::Oversize { maximum: 86 })
    );

    stack
        .send_udp_for_host_validation(
            endpoint,
            Some(selection(second_id, SECOND_IP)),
            SECOND_PEER_IP,
            40001,
            b"second",
        )
        .unwrap();
    assert_eq!(
        stack.send_udp_for_host_validation(
            endpoint,
            Some(selection(second_id, SECOND_IP)),
            SECOND_PEER_IP,
            40001,
            b"full",
        ),
        Err(HostSendError::TxFull)
    );

    // Wrong-interface-first cannot see the selected engine resource.
    stack
        .pump(
            first_id,
            &mut first,
            Instant::from_micros(1),
            PumpBudget::new(2, 2),
        )
        .unwrap();
    assert_eq!(first.submissions(), 0);
    assert_eq!(second.submissions(), 0);
    assert!(
        stack
            .udp_endpoint_observation_for_host_validation(endpoint)
            .unwrap()
            .pending_tx
    );

    stack
        .pump(
            second_id,
            &mut second,
            Instant::from_micros(2),
            PumpBudget::new(2, 2),
        )
        .unwrap();
    assert_eq!(first.submissions(), 0);
    assert_eq!(second.submissions(), 1);
    assert_udp_frame(
        &second.submitted_frames()[0],
        SECOND_IP,
        SECOND_PEER_IP,
        40000,
        40001,
        b"second",
    );
    assert!(
        !stack
            .udp_endpoint_observation_for_host_validation(endpoint)
            .unwrap()
            .pending_tx
    );
    second.complete_all();

    // The same logical Endpoint selects another prefix without changing ID or
    // creating another binding owner.
    stack
        .send_udp_for_host_validation(
            endpoint,
            Some(selection(first_id, FIRST_IP)),
            FIRST_PEER_IP,
            40002,
            b"first",
        )
        .unwrap();
    stack
        .pump(
            first_id,
            &mut first,
            Instant::from_micros(3),
            PumpBudget::new(2, 2),
        )
        .unwrap();
    assert_eq!(first.submissions(), 1);
    assert_udp_frame(
        &first.submitted_frames()[0],
        FIRST_IP,
        FIRST_PEER_IP,
        40000,
        40002,
        b"first",
    );
    assert_eq!(local_id, InterfaceId::from_index(2));
}

#[test]
fn local_link_is_bounded_normal_ingress_and_retire_withdraws_all_resources() {
    let mut stack = Stack::new();
    let mut blocked_external = BoundedProvider::with_mac(FIRST_MAC, 1);
    let external_id = stack.add_interface(
        &mut blocked_external,
        anemone_net_api::EthernetAddress::new(FIRST_MAC),
        Instant::ZERO,
    );
    stack
        .configure_ipv4_for_host_validation(external_id, FIRST_IP, 24)
        .unwrap();
    prime_bounded_neighbor(
        &mut stack,
        external_id,
        &mut blocked_external,
        FIRST_PEER_MAC,
        FIRST_PEER_IP,
        FIRST_IP,
    );
    blocked_external.reset_observation();
    blocked_external.set_link_state(anemone_net_api::LinkState::Down);

    let local_id = stack.add_local_ipv4_for_host_validation(LOCAL_IP, 8, 1, 128, Instant::ZERO);
    let client = stack
        .create_udp_endpoint_for_host_validation(41000, 2, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();
    let server = stack
        .create_udp_endpoint_for_host_validation(41001, 2, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();
    let external = stack
        .create_udp_endpoint_for_host_validation(41002, 2, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();
    assert_eq!(
        stack.create_udp_endpoint_for_host_validation(41001, 2, ENDPOINT_PAYLOAD_CAPACITY),
        Err(HostEndpointCreateError::PortInUse)
    );

    // A blocked external provider retains its own Endpoint datagram but does
    // not prevent another Endpoint from progressing through the local port.
    stack
        .send_udp_for_host_validation(
            external,
            Some(selection(external_id, FIRST_IP)),
            FIRST_PEER_IP,
            41003,
            b"blocked",
        )
        .unwrap();
    let external_outcome = stack
        .pump(
            external_id,
            &mut blocked_external,
            Instant::from_micros(1),
            PumpBudget::new(1, 1),
        )
        .unwrap();
    assert!(external_outcome.work_remaining);
    assert!(
        stack
            .udp_endpoint_observation_for_host_validation(external)
            .unwrap()
            .pending_tx
    );

    stack
        .send_udp_for_host_validation(
            client,
            Some(selection(local_id, LOCAL_IP)),
            LOCAL_IP,
            41001,
            b"one",
        )
        .unwrap();
    stack
        .pump_local_for_host_validation(local_id, Instant::from_micros(2), PumpBudget::new(1, 1))
        .unwrap();
    assert_eq!(
        stack
            .local_link_observation_for_host_validation()
            .unwrap()
            .occupied_packets,
        1
    );
    assert_eq!(
        stack
            .udp_endpoint_observation_for_host_validation(server)
            .unwrap()
            .received_datagrams,
        0
    );
    stack
        .pump_local_for_host_validation(local_id, Instant::from_micros(3), PumpBudget::new(1, 1))
        .unwrap();
    let received = stack
        .receive_udp_for_host_validation(server)
        .expect("normal local IP/UDP ingress must reach the aggregate Endpoint owner");
    assert_eq!(received.payload, b"one");
    assert_eq!(received.source_address, LOCAL_IP);
    assert_eq!(received.source_port, 41000);

    // Fill the one-packet link, then prove a later datagram remains in the
    // engine until ingress frees credit and a finite later round retries it.
    stack
        .send_udp_for_host_validation(
            client,
            Some(selection(local_id, LOCAL_IP)),
            LOCAL_IP,
            41001,
            b"two",
        )
        .unwrap();
    stack
        .pump_local_for_host_validation(local_id, Instant::from_micros(4), PumpBudget::new(1, 1))
        .unwrap();
    stack
        .send_udp_for_host_validation(
            client,
            Some(selection(local_id, LOCAL_IP)),
            LOCAL_IP,
            41001,
            b"three",
        )
        .unwrap();
    let recovery = stack
        .pump_local_for_host_validation(local_id, Instant::from_micros(5), PumpBudget::new(1, 1))
        .unwrap();
    assert!(recovery.work_remaining);
    assert!(
        stack
            .udp_endpoint_observation_for_host_validation(client)
            .unwrap()
            .pending_tx
    );
    assert_eq!(
        stack
            .receive_udp_for_host_validation(server)
            .unwrap()
            .payload,
        b"two"
    );
    stack
        .pump_local_for_host_validation(local_id, Instant::from_micros(6), PumpBudget::new(1, 1))
        .unwrap();
    stack
        .pump_local_for_host_validation(local_id, Instant::from_micros(7), PumpBudget::new(1, 1))
        .unwrap();
    assert_eq!(
        stack
            .receive_udp_for_host_validation(server)
            .unwrap()
            .payload,
        b"three"
    );

    // A packet accepted by local egress remains tagged with its aggregate
    // owner until ingress. Retirement removes that packet, every engine
    // resource, and the provisional identity before any stale lookup.
    stack
        .send_udp_for_host_validation(
            client,
            Some(selection(local_id, LOCAL_IP)),
            LOCAL_IP,
            41001,
            b"retire",
        )
        .unwrap();
    stack
        .pump_local_for_host_validation(local_id, Instant::from_micros(8), PumpBudget::new(1, 1))
        .unwrap();
    assert_eq!(
        stack
            .local_link_observation_for_host_validation()
            .unwrap()
            .occupied_packets,
        1
    );
    stack
        .retire_udp_endpoint_for_host_validation(client)
        .unwrap();
    assert!(
        stack
            .udp_endpoint_observation_for_host_validation(client)
            .is_none()
    );
    assert_eq!(
        stack
            .local_link_observation_for_host_validation()
            .unwrap()
            .occupied_packets,
        0
    );
    assert_eq!(
        stack.send_udp_for_host_validation(
            client,
            Some(selection(local_id, LOCAL_IP)),
            LOCAL_IP,
            41001,
            b"stale",
        ),
        Err(HostSendError::UnknownEndpoint)
    );
    assert_eq!(
        stack.retire_udp_endpoint_for_host_validation(client),
        Err(HostRetireError::UnknownEndpoint)
    );
}

#[test]
fn engine_payload_capacity_is_part_of_precommit_admission() {
    let mut stack = Stack::new();
    let local = stack.add_local_ipv4_for_host_validation(LOCAL_IP, 8, 2, 128, Instant::ZERO);
    let client = stack
        .create_udp_endpoint_for_host_validation(43000, 1, 32)
        .unwrap();
    let server = stack
        .create_udp_endpoint_for_host_validation(43001, 1, 32)
        .unwrap();

    assert_eq!(
        stack.send_udp_for_host_validation(
            client,
            Some(selection(local, LOCAL_IP)),
            LOCAL_IP,
            43001,
            &[0; 33],
        ),
        Err(HostSendError::Oversize { maximum: 32 })
    );
    assert!(
        !stack
            .udp_endpoint_observation_for_host_validation(client)
            .unwrap()
            .pending_tx
    );

    stack
        .send_udp_for_host_validation(
            client,
            Some(selection(local, LOCAL_IP)),
            LOCAL_IP,
            43001,
            &[0x5a; 32],
        )
        .unwrap();
    pump_local(&mut stack, local, 1);
    pump_local(&mut stack, local, 2);
    assert_eq!(
        stack
            .receive_udp_for_host_validation(server)
            .unwrap()
            .payload,
        [0x5a; 32]
    );
}

#[test]
fn mtu_must_fit_headers_before_zero_length_payload_is_admitted() {
    let mut stack = Stack::new();
    let local = stack.add_local_ipv4_for_host_validation(LOCAL_IP, 8, 1, 27, Instant::ZERO);
    let client = stack
        .create_udp_endpoint_for_host_validation(43500, 1, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();

    assert_eq!(
        stack.send_udp_for_host_validation(
            client,
            Some(selection(local, LOCAL_IP)),
            LOCAL_IP,
            43501,
            b"",
        ),
        Err(HostSendError::Oversize { maximum: 0 })
    );
    assert!(
        !stack
            .udp_endpoint_observation_for_host_validation(client)
            .unwrap()
            .pending_tx
    );
}

#[test]
fn full_receive_queue_does_not_gate_another_endpoint() {
    let mut stack = Stack::new();
    let local = stack.add_local_ipv4_for_host_validation(LOCAL_IP, 8, 4, 128, Instant::ZERO);
    let first_client = stack
        .create_udp_endpoint_for_host_validation(44000, 1, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();
    let first_server = stack
        .create_udp_endpoint_for_host_validation(44001, 1, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();
    let second_client = stack
        .create_udp_endpoint_for_host_validation(45000, 1, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();
    let second_server = stack
        .create_udp_endpoint_for_host_validation(45001, 1, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();

    stack
        .send_udp_for_host_validation(
            first_client,
            Some(selection(local, LOCAL_IP)),
            LOCAL_IP,
            44001,
            b"first-1",
        )
        .unwrap();
    pump_local(&mut stack, local, 1);
    pump_local(&mut stack, local, 2);

    // Keep one datagram in the aggregate queue and one in the same Endpoint's
    // engine. This backpressure belongs to first_server only.
    stack
        .send_udp_for_host_validation(
            first_client,
            Some(selection(local, LOCAL_IP)),
            LOCAL_IP,
            44001,
            b"first-2",
        )
        .unwrap();
    pump_local(&mut stack, local, 3);
    pump_local(&mut stack, local, 4);
    assert_eq!(
        stack
            .udp_endpoint_observation_for_host_validation(first_server)
            .unwrap()
            .received_datagrams,
        1
    );
    let quiescent = stack
        .pump_local_for_host_validation(local, Instant::from_micros(5), PumpBudget::new(1, 1))
        .unwrap();
    assert!(!quiescent.work_remaining);
    assert_eq!(quiescent.recheck, anemone_net_api::Recheck::Idle);

    stack
        .send_udp_for_host_validation(
            second_client,
            Some(selection(local, LOCAL_IP)),
            LOCAL_IP,
            45001,
            b"second",
        )
        .unwrap();
    for tick in 6..10 {
        pump_local(&mut stack, local, tick);
    }
    assert_eq!(
        stack
            .receive_udp_for_host_validation(second_server)
            .unwrap()
            .payload,
        b"second"
    );

    assert_eq!(
        stack
            .receive_udp_for_host_validation(first_server)
            .unwrap()
            .payload,
        b"first-1"
    );
    pump_local(&mut stack, local, 10);
    assert_eq!(
        stack
            .receive_udp_for_host_validation(first_server)
            .unwrap()
            .payload,
        b"first-2"
    );
}
