use super::fixture::*;

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

#[test]
fn endpoint_facts_and_invalidations_cover_capacity_receive_and_retire() {
    let mut stack = standard_stack();
    let local = stack.add_local_ipv4_for_host_validation(LOCAL_IP, 8, 2, 128, Instant::ZERO);
    let client = stack
        .create_udp_endpoint_for_host_validation(49010, 1, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();
    let server = stack
        .create_udp_endpoint_for_host_validation(49011, 1, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();

    // Create and bind both touch the owner, but the handoff is one coalesced
    // identity per Endpoint and carries no readiness payload.
    let mut initial = stack.take_udp_invalidations_for_host_validation();
    initial.sort_by_key(|endpoint| *endpoint == server);
    assert_eq!(initial, vec![client, server]);
    let facts = stack
        .udp_endpoint_facts_for_host_validation(client)
        .unwrap();
    assert!(facts.is_live());
    assert!(!facts.is_readable());
    assert!(facts.is_writable());

    stack
        .send_udp_for_host_validation(
            client,
            Some(selection(local, LOCAL_IP)),
            LOCAL_IP,
            49011,
            b"facts",
        )
        .unwrap();
    assert!(
        !stack
            .udp_endpoint_facts_for_host_validation(client)
            .unwrap()
            .is_writable()
    );
    assert_eq!(
        stack.take_udp_invalidations_for_host_validation(),
        vec![client]
    );

    pump_local(&mut stack, local, 1);
    assert!(
        stack
            .udp_endpoint_facts_for_host_validation(client)
            .unwrap()
            .is_writable()
    );
    assert_eq!(
        stack.take_udp_invalidations_for_host_validation(),
        vec![client]
    );

    pump_local(&mut stack, local, 2);
    assert!(
        stack
            .udp_endpoint_facts_for_host_validation(server)
            .unwrap()
            .is_readable()
    );
    assert_eq!(
        stack.take_udp_invalidations_for_host_validation(),
        vec![server]
    );

    let received = stack.receive_udp_for_host_validation(server).unwrap();
    assert_eq!(received.payload, b"facts");
    assert!(
        !stack
            .udp_endpoint_facts_for_host_validation(server)
            .unwrap()
            .is_readable()
    );
    assert_eq!(
        stack.take_udp_invalidations_for_host_validation(),
        vec![server]
    );

    stack
        .retire_udp_endpoint_for_host_validation(client)
        .unwrap();
    assert_eq!(
        stack.take_udp_invalidations_for_host_validation(),
        vec![client]
    );
    stack
        .retire_udp_endpoint_for_host_validation(server)
        .unwrap();
    assert_eq!(
        stack.take_udp_invalidations_for_host_validation(),
        vec![server]
    );
    assert_eq!(
        stack.udp_endpoint_facts_for_host_validation(client),
        Err(UdpQueryError::UnknownEndpoint)
    );
}

#[test]
fn connect_transaction_reconnect_disconnect_and_stale_identity_are_atomic() {
    let mut stack = host_stack(16, 50100, 50101);
    let local = stack.add_local_ipv4_for_host_validation(LOCAL_IP, 8, 2, 128, Instant::ZERO);
    let endpoint = create_unbound(&mut stack);
    let first_peer = [127, 0, 0, 2];
    let second_peer = [127, 0, 0, 3];

    stack
        .connect_udp_endpoint_for_host_validation(
            endpoint,
            selection(local, LOCAL_IP),
            first_peer,
            53000,
        )
        .unwrap();
    let binding = stack
        .udp_binding_for_host_validation(endpoint)
        .unwrap()
        .expect("unbound connect must atomically commit an implicit binding");
    assert_eq!(binding.address().octets(), LOCAL_IP);
    assert_eq!(binding.port(), 50100);
    assert_eq!(
        stack.udp_peer_for_host_validation(endpoint).unwrap(),
        Some(anemone_smoltcp_stack::HostPeer {
            address: first_peer,
            port: 53000,
        })
    );

    stack
        .connect_udp_endpoint_for_host_validation(
            endpoint,
            selection(local, LOCAL_IP),
            first_peer,
            53000,
        )
        .unwrap();
    stack
        .connect_udp_endpoint_for_host_validation(
            endpoint,
            selection(local, LOCAL_IP),
            second_peer,
            53001,
        )
        .unwrap();
    assert_eq!(
        stack.udp_binding_for_host_validation(endpoint).unwrap(),
        Some(binding)
    );
    assert_eq!(
        stack.udp_peer_for_host_validation(endpoint).unwrap(),
        Some(anemone_smoltcp_stack::HostPeer {
            address: second_peer,
            port: 53001,
        })
    );

    for (selection, peer, port, expected) in [
        (
            selection(local, LOCAL_IP),
            second_peer,
            0,
            UdpConnectError::InvalidPeer,
        ),
        (
            selection(InterfaceId::from_index(99), LOCAL_IP),
            first_peer,
            53002,
            UdpConnectError::UnknownInterface,
        ),
        (
            selection(local, [10, 0, 0, 9]),
            first_peer,
            53002,
            UdpConnectError::UnsupportedSource,
        ),
    ] {
        assert_eq!(
            stack.connect_udp_endpoint_for_host_validation(endpoint, selection, peer, port),
            Err(expected)
        );
        assert_eq!(
            stack.udp_binding_for_host_validation(endpoint).unwrap(),
            Some(binding)
        );
        assert_eq!(
            stack.udp_peer_for_host_validation(endpoint).unwrap(),
            Some(anemone_smoltcp_stack::HostPeer {
                address: second_peer,
                port: 53001,
            })
        );
    }

    stack
        .disconnect_udp_endpoint_for_host_validation(endpoint)
        .unwrap();
    stack
        .disconnect_udp_endpoint_for_host_validation(endpoint)
        .unwrap();
    assert_eq!(stack.udp_peer_for_host_validation(endpoint).unwrap(), None);
    assert_eq!(
        stack.udp_binding_for_host_validation(endpoint).unwrap(),
        Some(binding)
    );

    let occupied = create_unbound(&mut stack);
    bind_for_host(&mut stack, occupied, LOCAL_IP, 50101).unwrap();
    let exhausted = create_unbound(&mut stack);
    assert_eq!(
        stack.connect_udp_endpoint_for_host_validation(
            exhausted,
            selection(local, LOCAL_IP),
            first_peer,
            53000,
        ),
        Err(UdpConnectError::EphemeralPortsExhausted)
    );
    assert_eq!(
        stack.udp_binding_for_host_validation(exhausted).unwrap(),
        None
    );
    assert_eq!(stack.udp_peer_for_host_validation(exhausted).unwrap(), None);

    stack
        .retire_udp_endpoint_for_host_validation(endpoint)
        .unwrap();
    let replacement = create_unbound(&mut stack);
    assert_ne!(replacement, endpoint);
    assert_eq!(
        stack.udp_peer_for_host_validation(endpoint),
        Err(UdpQueryError::UnknownEndpoint)
    );
    assert_eq!(
        stack.udp_peer_for_host_validation(replacement).unwrap(),
        None
    );
}
