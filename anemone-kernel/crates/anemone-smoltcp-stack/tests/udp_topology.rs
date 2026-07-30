mod support;

use anemone_net_api::{
    Instant, InterfaceId, Ipv4Address as ApiIpv4Address, Ipv4Cidr as ApiIpv4Cidr,
    udp::{
        UdpBindError, UdpBindRequest, UdpCreateError, UdpEndpointLimits, UdpNamespacePolicy,
        UdpQueryError,
    },
};
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

fn lifecycle_limits() -> UdpEndpointLimits {
    UdpEndpointLimits::new(1, 1, 32)
}

fn host_stack(capacity: usize, first: u16, last: u16) -> Stack {
    Stack::new_for_host_validation(UdpNamespacePolicy::new(capacity, first, last))
}

fn standard_stack() -> Stack {
    host_stack(64, 32768, 60999)
}

fn create_unbound(stack: &mut Stack) -> anemone_smoltcp_stack::HostEndpointId {
    stack
        .create_unbound_udp_endpoint_for_host_validation(lifecycle_limits())
        .unwrap()
}

fn bind_for_host(
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

#[test]
fn endpoint_capacity_binding_matrix_ephemeral_and_stale_identity() {
    let mut capacity_stack = host_stack(64, 50000, 50001);
    let mut endpoints = Vec::new();
    for _ in 0..64 {
        endpoints.push(create_unbound(&mut capacity_stack));
    }
    assert_eq!(
        capacity_stack.create_unbound_udp_endpoint_for_host_validation(lifecycle_limits()),
        Err(UdpCreateError::EndpointCapacity)
    );
    for endpoint in endpoints {
        capacity_stack
            .retire_udp_endpoint_for_host_validation(endpoint)
            .unwrap();
    }

    let any = [0, 0, 0, 0];
    let first_address = [10, 0, 0, 1];
    let second_address = [10, 0, 0, 2];
    let mut stack = host_stack(64, 50000, 50001);
    let first = create_unbound(&mut stack);
    let second = create_unbound(&mut stack);
    let duplicate = create_unbound(&mut stack);
    let wildcard = create_unbound(&mut stack);
    bind_for_host(&mut stack, first, first_address, 42000).unwrap();
    bind_for_host(&mut stack, second, second_address, 42000).unwrap();
    assert_eq!(
        bind_for_host(&mut stack, duplicate, first_address, 42000),
        Err(UdpBindError::PortInUse)
    );
    assert_eq!(
        stack.udp_binding_for_host_validation(duplicate).unwrap(),
        None
    );
    assert_eq!(
        bind_for_host(&mut stack, wildcard, any, 42000),
        Err(UdpBindError::PortInUse)
    );
    bind_for_host(&mut stack, duplicate, first_address, 42001).unwrap();
    assert_eq!(
        bind_for_host(&mut stack, duplicate, first_address, 42002),
        Err(UdpBindError::AlreadyBound)
    );

    let ephemeral_one = create_unbound(&mut stack);
    let ephemeral_two = create_unbound(&mut stack);
    let exhausted = create_unbound(&mut stack);
    assert_eq!(
        bind_for_host(&mut stack, ephemeral_one, any, 0)
            .unwrap()
            .port(),
        50000
    );
    assert_eq!(
        bind_for_host(&mut stack, ephemeral_two, any, 0)
            .unwrap()
            .port(),
        50001
    );
    assert_eq!(
        bind_for_host(&mut stack, exhausted, any, 0),
        Err(UdpBindError::EphemeralPortsExhausted)
    );
    assert_eq!(
        stack.udp_binding_for_host_validation(exhausted).unwrap(),
        None
    );

    stack
        .retire_udp_endpoint_for_host_validation(ephemeral_one)
        .unwrap();
    assert_eq!(
        stack.udp_binding_for_host_validation(ephemeral_one),
        Err(UdpQueryError::UnknownEndpoint)
    );
    let replacement = create_unbound(&mut stack);
    assert_eq!(
        bind_for_host(&mut stack, replacement, any, 0)
            .unwrap()
            .port(),
        50000
    );
    assert_eq!(
        stack.retire_udp_endpoint_for_host_validation(ephemeral_one),
        Err(anemone_smoltcp_stack::HostRetireError::UnknownEndpoint)
    );

    let mut reverse = host_stack(64, 50000, 50001);
    let wildcard_first = create_unbound(&mut reverse);
    let wildcard_after = create_unbound(&mut reverse);
    let specific_after = create_unbound(&mut reverse);
    bind_for_host(&mut reverse, wildcard_first, any, 43000).unwrap();
    assert_eq!(
        bind_for_host(&mut reverse, wildcard_after, any, 43000),
        Err(UdpBindError::PortInUse)
    );
    assert_eq!(
        bind_for_host(&mut reverse, specific_after, first_address, 43000),
        Err(UdpBindError::PortInUse)
    );
}

#[test]
fn implicit_bind_exhaustion_and_post_bind_send_failure_preserve_binding() {
    let any = [0, 0, 0, 0];
    let mut exhausted = host_stack(4, 50000, 50000);
    let occupied = create_unbound(&mut exhausted);
    let implicit = create_unbound(&mut exhausted);
    bind_for_host(&mut exhausted, occupied, any, 0).unwrap();
    assert_eq!(
        bind_for_host(&mut exhausted, implicit, any, 0),
        Err(UdpBindError::EphemeralPortsExhausted)
    );
    assert_eq!(
        exhausted.udp_binding_for_host_validation(implicit).unwrap(),
        None
    );

    let mut stack = standard_stack();
    let local = stack.add_local_ipv4_for_host_validation(LOCAL_IP, 8, 2, 128, Instant::ZERO);
    let endpoint = create_unbound(&mut stack);
    let binding = bind_for_host(&mut stack, endpoint, any, 0).unwrap();
    assert_eq!(
        stack.send_udp_for_host_validation(
            endpoint,
            Some(selection(local, LOCAL_IP)),
            LOCAL_IP,
            0,
            b"invalid destination",
        ),
        Err(HostSendError::InvalidDestination)
    );
    assert_eq!(
        stack.udp_binding_for_host_validation(endpoint).unwrap(),
        Some(binding)
    );
    assert_eq!(
        stack.send_udp_for_host_validation(
            endpoint,
            Some(selection(local, LOCAL_IP)),
            LOCAL_IP,
            49000,
            &[0; 33],
        ),
        Err(HostSendError::Oversize { maximum: 32 })
    );
    assert_eq!(
        stack.udp_binding_for_host_validation(endpoint).unwrap(),
        Some(binding)
    );
}

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
fn production_ipv4_projection_covers_127_8_self_external_and_default_route() {
    let mut stack = standard_stack();
    let mut external = BoundedProvider::with_mac(FIRST_MAC, 1);
    let external_id = stack.add_interface(
        &mut external,
        anemone_net_api::EthernetAddress::new(FIRST_MAC),
        Instant::ZERO,
    );
    stack
        .configure_external_ipv4(
            external_id,
            ApiIpv4Cidr::new(ApiIpv4Address::new(FIRST_IP), 24).unwrap(),
            Some(ApiIpv4Address::new(FIRST_PEER_IP)),
        )
        .unwrap();
    let local = stack
        .add_local_ipv4(
            ApiIpv4Cidr::new(ApiIpv4Address::new(LOCAL_IP), 8).unwrap(),
            2,
            128,
            Instant::ZERO,
        )
        .unwrap();
    stack
        .add_local_delivery_ipv4(ApiIpv4Address::new(FIRST_IP))
        .unwrap();

    let client = stack
        .create_udp_endpoint_for_host_validation(40500, 2, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();
    let server = stack
        .create_udp_endpoint_for_host_validation(40501, 2, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();
    for (tick, source, destination, payload) in [
        (1, LOCAL_IP, [127, 9, 8, 7], b"127/8".as_slice()),
        (3, FIRST_IP, FIRST_IP, b"self-external".as_slice()),
    ] {
        stack
            .send_udp_for_host_validation(
                client,
                Some(selection(local, source)),
                destination,
                40501,
                payload,
            )
            .unwrap();
        let transferred = stack
            .pump_local(local, Instant::from_micros(tick), PumpBudget::new(32, 32))
            .unwrap();
        assert!(transferred.work_remaining);
        assert_eq!(transferred.recheck, anemone_net_api::Recheck::Immediate);
        let ingressed = stack
            .pump_local(
                local,
                Instant::from_micros(tick + 1),
                PumpBudget::new(32, 32),
            )
            .unwrap();
        assert!(!ingressed.work_remaining);
        assert_eq!(ingressed.recheck, anemone_net_api::Recheck::Idle);
        let received = stack.receive_udp_for_host_validation(server).unwrap();
        assert_eq!(received.payload, payload);
        assert_eq!(received.source_address, source);
    }
}

#[test]
fn selected_external_interface_is_the_only_engine_that_can_consume() {
    let mut stack = standard_stack();
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
    let mut stack = standard_stack();
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
    let rollback_probe = create_unbound(&mut stack);
    stack
        .retire_udp_endpoint_for_host_validation(rollback_probe)
        .unwrap();

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
    let mut stack = standard_stack();
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
    let mut stack = standard_stack();
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
    let mut stack = standard_stack();
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
    // Detaching first-1 restores one aggregate credit and synchronously refills
    // it from the engine; no unrelated pump edge is required for first-2.
    assert_eq!(
        stack
            .receive_udp_for_host_validation(first_server)
            .unwrap()
            .payload,
        b"first-2"
    );
}

#[test]
fn zero_length_detach_abandon_and_receive_order_are_deterministic() {
    let mut stack = standard_stack();
    let local = stack.add_local_ipv4_for_host_validation(LOCAL_IP, 8, 4, 128, Instant::ZERO);
    let client = stack
        .create_udp_endpoint_for_host_validation(46000, 4, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();
    let server = stack
        .create_udp_endpoint_for_host_validation(46001, 4, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();

    stack
        .send_udp_for_host_validation(
            client,
            Some(selection(local, LOCAL_IP)),
            LOCAL_IP,
            46001,
            b"",
        )
        .unwrap();
    pump_local(&mut stack, local, 1);
    pump_local(&mut stack, local, 2);
    let detached = stack.receive_udp_for_host_validation(server).unwrap();
    assert!(detached.payload.is_empty());
    drop(detached);
    assert!(stack.receive_udp_for_host_validation(server).is_none());

    for (tick, payload) in [(3, b"first".as_slice()), (5, b"second".as_slice())] {
        stack
            .send_udp_for_host_validation(
                client,
                Some(selection(local, LOCAL_IP)),
                LOCAL_IP,
                46001,
                payload,
            )
            .unwrap();
        pump_local(&mut stack, local, tick);
        pump_local(&mut stack, local, tick + 1);
    }
    assert_eq!(
        stack
            .receive_udp_for_host_validation(server)
            .unwrap()
            .payload,
        b"first"
    );
    assert_eq!(
        stack
            .receive_udp_for_host_validation(server)
            .unwrap()
            .payload,
        b"second"
    );
    assert!(stack.receive_udp_for_host_validation(server).is_none());
}
