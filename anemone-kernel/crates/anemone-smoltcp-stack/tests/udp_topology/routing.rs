use super::fixture::*;

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
