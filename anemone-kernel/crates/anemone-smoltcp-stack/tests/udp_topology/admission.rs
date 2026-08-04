use super::fixture::*;

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
fn writable_tracks_endpoint_admission_across_selection_and_provider_backpressure() {
    let mut stack = standard_stack();
    let mut provider = BoundedProvider::with_mac(FIRST_MAC, 1);
    let interface = stack.add_interface(
        &mut provider,
        anemone_net_api::EthernetAddress::new(FIRST_MAC),
        Instant::ZERO,
    );
    stack
        .configure_ipv4_for_host_validation(interface, FIRST_IP, 24)
        .unwrap();
    prime_bounded_neighbor(
        &mut stack,
        interface,
        &mut provider,
        FIRST_PEER_MAC,
        FIRST_PEER_IP,
        FIRST_IP,
    );
    provider.reset_observation();

    let endpoint = create_unbound(&mut stack);
    assert!(
        stack
            .udp_endpoint_facts_for_host_validation(endpoint)
            .unwrap()
            .is_writable()
    );
    stack.take_udp_invalidations_for_host_validation();
    bind_for_host(&mut stack, endpoint, [0; 4], 49100).unwrap();
    stack.take_udp_invalidations_for_host_validation();

    assert_eq!(
        stack.send_udp_for_host_validation(
            endpoint,
            None,
            FIRST_PEER_IP,
            49101,
            b"selection failure",
        ),
        Err(HostSendError::MissingSelection)
    );
    assert_eq!(
        stack.send_udp_for_host_validation(
            endpoint,
            Some(selection(interface, FIRST_IP)),
            FIRST_PEER_IP,
            49101,
            &[0; 33],
        ),
        Err(HostSendError::Oversize { maximum: 32 })
    );
    assert!(
        stack
            .udp_endpoint_facts_for_host_validation(endpoint)
            .unwrap()
            .is_writable()
    );
    assert!(
        stack
            .take_udp_invalidations_for_host_validation()
            .is_empty()
    );

    stack
        .send_udp_for_host_validation(
            endpoint,
            Some(selection(interface, FIRST_IP)),
            FIRST_PEER_IP,
            49101,
            b"first",
        )
        .unwrap();
    assert!(
        !stack
            .udp_endpoint_facts_for_host_validation(endpoint)
            .unwrap()
            .is_writable()
    );
    assert_eq!(
        stack.take_udp_invalidations_for_host_validation(),
        vec![endpoint]
    );

    stack
        .pump(
            interface,
            &mut provider,
            Instant::from_micros(1),
            PumpBudget::new(2, 2),
        )
        .unwrap();
    assert_eq!(provider.live_tx(), 1);
    assert!(
        stack
            .udp_endpoint_facts_for_host_validation(endpoint)
            .unwrap()
            .is_writable()
    );
    assert_eq!(
        stack.take_udp_invalidations_for_host_validation(),
        vec![endpoint]
    );

    stack
        .send_udp_for_host_validation(
            endpoint,
            Some(selection(interface, FIRST_IP)),
            FIRST_PEER_IP,
            49101,
            b"second",
        )
        .unwrap();
    stack.take_udp_invalidations_for_host_validation();
    let blocked = stack
        .pump(
            interface,
            &mut provider,
            Instant::from_micros(2),
            PumpBudget::new(2, 2),
        )
        .unwrap();
    assert!(blocked.work_remaining);
    assert!(provider.normal_exhaustions() > 0);
    assert!(
        !stack
            .udp_endpoint_facts_for_host_validation(endpoint)
            .unwrap()
            .is_writable()
    );
    assert!(
        stack
            .take_udp_invalidations_for_host_validation()
            .is_empty()
    );

    provider.complete_all();
    stack
        .pump(
            interface,
            &mut provider,
            Instant::from_micros(3),
            PumpBudget::new(2, 2),
        )
        .unwrap();
    assert!(
        stack
            .udp_endpoint_facts_for_host_validation(endpoint)
            .unwrap()
            .is_writable()
    );
    assert_eq!(
        stack.take_udp_invalidations_for_host_validation(),
        vec![endpoint]
    );
}

#[test]
fn provider_ingress_rejects_first_later_and_complete_fragment_pair() {
    let mut stack = standard_stack();
    let mut provider = BoundedProvider::with_mac(FIRST_MAC, 1);
    let interface = stack.add_interface(
        &mut provider,
        anemone_net_api::EthernetAddress::new(FIRST_MAC),
        Instant::ZERO,
    );
    stack
        .configure_ipv4_for_host_validation(interface, FIRST_IP, 24)
        .unwrap();
    let server = stack
        .create_udp_endpoint_for_host_validation(49200, 4, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();
    stack.take_udp_invalidations_for_host_validation();

    let payload = b"fragment-proof";
    let datagram = udp_datagram(FIRST_PEER_IP, FIRST_IP, 49201, 49200, payload);
    let first = ethernet_ipv4_fragment(
        FIRST_PEER_MAC,
        FIRST_MAC,
        FIRST_PEER_IP,
        FIRST_IP,
        0x4c01,
        0,
        true,
        &datagram[..16],
    );
    let later = ethernet_ipv4_fragment(
        FIRST_PEER_MAC,
        FIRST_MAC,
        FIRST_PEER_IP,
        FIRST_IP,
        0x4c01,
        16,
        false,
        &datagram[16..],
    );

    for frame in [&first, &later, &first, &later] {
        provider.inject(frame);
        let tick = i64::try_from(provider.rx_recycles() + 1).unwrap();
        stack
            .pump(
                interface,
                &mut provider,
                Instant::from_micros(tick),
                PumpBudget::new(2, 2),
            )
            .unwrap();
        assert!(
            !stack
                .udp_endpoint_facts_for_host_validation(server)
                .unwrap()
                .is_readable()
        );
        assert!(stack.receive_udp_for_host_validation(server).is_none());
        assert!(
            stack
                .take_udp_invalidations_for_host_validation()
                .is_empty()
        );
    }

    let complete = ethernet_ipv4_fragment(
        FIRST_PEER_MAC,
        FIRST_MAC,
        FIRST_PEER_IP,
        FIRST_IP,
        0x4c02,
        0,
        false,
        &datagram,
    );
    provider.inject(&complete);
    stack
        .pump(
            interface,
            &mut provider,
            Instant::from_micros(5),
            PumpBudget::new(2, 2),
        )
        .unwrap();
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
    assert_eq!(received.payload, payload);
    assert_eq!(received.source_address, FIRST_PEER_IP);
    assert_eq!(received.source_port, 49201);
}

#[test]
fn connected_admission_preserves_old_queue_and_peek_never_consumes() {
    let mut stack = standard_stack();
    let local = stack.add_local_ipv4_for_host_validation(LOCAL_IP, 8, 4, 128, Instant::ZERO);
    let first = stack
        .create_udp_endpoint_for_host_validation(49300, 4, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();
    let second = stack
        .create_udp_endpoint_for_host_validation(49301, 4, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();
    let server = stack
        .create_udp_endpoint_for_host_validation(49302, 4, ENDPOINT_PAYLOAD_CAPACITY)
        .unwrap();

    stack
        .send_udp_for_host_validation(
            second,
            Some(selection(local, LOCAL_IP)),
            LOCAL_IP,
            49302,
            b"queued-before-connect",
        )
        .unwrap();
    pump_local(&mut stack, local, 1);
    pump_local(&mut stack, local, 2);
    stack
        .connect_udp_endpoint_for_host_validation(
            server,
            selection(local, LOCAL_IP),
            LOCAL_IP,
            49300,
        )
        .unwrap();
    stack.take_udp_invalidations_for_host_validation();

    let first_peek = stack.peek_udp_for_host_validation(server).unwrap();
    let second_peek = stack.peek_udp_for_host_validation(server).unwrap();
    assert_eq!(first_peek, second_peek);
    assert_eq!(first_peek.payload, b"queued-before-connect");
    assert_eq!(first_peek.source_port, 49301);
    assert!(
        stack
            .udp_endpoint_facts_for_host_validation(server)
            .unwrap()
            .is_readable()
    );
    assert!(
        stack
            .take_udp_invalidations_for_host_validation()
            .is_empty()
    );
    assert_eq!(
        stack
            .receive_udp_for_host_validation(server)
            .unwrap()
            .payload,
        b"queued-before-connect"
    );
    stack.take_udp_invalidations_for_host_validation();

    stack
        .send_udp_for_host_validation(
            second,
            Some(selection(local, LOCAL_IP)),
            LOCAL_IP,
            49302,
            b"wrong-peer",
        )
        .unwrap();
    pump_local(&mut stack, local, 3);
    pump_local(&mut stack, local, 4);
    assert!(stack.receive_udp_for_host_validation(server).is_none());
    assert!(
        !stack
            .udp_endpoint_facts_for_host_validation(server)
            .unwrap()
            .is_readable()
    );
    assert!(
        !stack
            .take_udp_invalidations_for_host_validation()
            .contains(&server)
    );

    stack
        .send_udp_for_host_validation(
            first,
            Some(selection(local, LOCAL_IP)),
            LOCAL_IP,
            49302,
            b"right-peer",
        )
        .unwrap();
    pump_local(&mut stack, local, 5);
    pump_local(&mut stack, local, 6);
    assert_eq!(
        stack
            .receive_udp_for_host_validation(server)
            .unwrap()
            .payload,
        b"right-peer"
    );

    stack
        .connect_udp_endpoint_for_host_validation(
            server,
            selection(local, LOCAL_IP),
            LOCAL_IP,
            49301,
        )
        .unwrap();
    stack.take_udp_invalidations_for_host_validation();
    stack
        .send_udp_for_host_validation(
            first,
            Some(selection(local, LOCAL_IP)),
            LOCAL_IP,
            49302,
            b"old-peer-after-reconnect",
        )
        .unwrap();
    pump_local(&mut stack, local, 7);
    pump_local(&mut stack, local, 8);
    assert!(stack.receive_udp_for_host_validation(server).is_none());
    stack
        .send_udp_default_for_host_validation(
            server,
            Some(selection(local, LOCAL_IP)),
            b"default-peer",
        )
        .unwrap();
    pump_local(&mut stack, local, 9);
    pump_local(&mut stack, local, 10);
    assert_eq!(
        stack
            .receive_udp_for_host_validation(second)
            .unwrap()
            .payload,
        b"default-peer"
    );

    stack
        .send_udp_for_host_validation(
            server,
            Some(selection(local, LOCAL_IP)),
            LOCAL_IP,
            49300,
            b"explicit-override",
        )
        .unwrap();
    pump_local(&mut stack, local, 11);
    pump_local(&mut stack, local, 12);
    assert_eq!(
        stack
            .receive_udp_for_host_validation(first)
            .unwrap()
            .payload,
        b"explicit-override"
    );
    assert_eq!(
        stack.udp_peer_for_host_validation(server).unwrap(),
        Some(anemone_smoltcp_stack::HostPeer {
            address: LOCAL_IP,
            port: 49301,
        })
    );

    stack
        .disconnect_udp_endpoint_for_host_validation(server)
        .unwrap();
    assert_eq!(
        stack.send_udp_default_for_host_validation(
            server,
            Some(selection(local, LOCAL_IP)),
            b"no-default",
        ),
        Err(HostSendError::MissingDestination)
    );
    stack.take_udp_invalidations_for_host_validation();
    stack
        .send_udp_for_host_validation(
            first,
            Some(selection(local, LOCAL_IP)),
            LOCAL_IP,
            49302,
            b"admitted-after-disconnect",
        )
        .unwrap();
    pump_local(&mut stack, local, 13);
    pump_local(&mut stack, local, 14);
    assert_eq!(
        stack
            .receive_udp_for_host_validation(server)
            .unwrap()
            .payload,
        b"admitted-after-disconnect"
    );
}
