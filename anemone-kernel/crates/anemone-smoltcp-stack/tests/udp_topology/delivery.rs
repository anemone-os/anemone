use super::fixture::*;

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
