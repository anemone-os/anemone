mod support;

use anemone_net_api::{
    EthernetAddress, FrameProvider, Instant, LinkState, ReceiveOutcome, Recheck, RxToken,
    TransmitOutcome, TxToken,
};
use anemone_smoltcp_stack::{PumpBudget, Stack};
use std::panic::{AssertUnwindSafe, catch_unwind};

use support::{
    BoundedProvider, FRAME_CAPACITY, TxSlot, assert_icmp_echo_reply, build_icmp_echo_request,
    build_raw_ipv4_packet, prime_bounded_neighbor, raw_ipv4_marker,
};

#[test]
fn deterministic_tx_exhaustion_preserves_and_resumes_socket_work() {
    const LOCAL_MAC: [u8; 6] = [0x02, 0, 0, 0, 4, 1];
    const PEER_MAC: [u8; 6] = [0x02, 0, 0, 0, 4, 2];
    const LOCAL_IP: [u8; 4] = [10, 0, 4, 2];
    const PEER_IP: [u8; 4] = [10, 0, 4, 1];

    let mut stack = Stack::new();
    let mut provider = BoundedProvider::with_mac(LOCAL_MAC, 2);
    let interface = stack.add_interface(
        &mut provider,
        EthernetAddress::new(LOCAL_MAC),
        Instant::ZERO,
    );
    stack
        .configure_ipv4_for_host_validation(interface, LOCAL_IP, 24)
        .unwrap();
    prime_bounded_neighbor(
        &mut stack,
        interface,
        &mut provider,
        PEER_MAC,
        PEER_IP,
        LOCAL_IP,
    );
    provider.reset_observation();

    let packets = [
        build_raw_ipv4_packet(LOCAL_IP, PEER_IP, 1),
        build_raw_ipv4_packet(LOCAL_IP, PEER_IP, 2),
        build_raw_ipv4_packet(LOCAL_IP, PEER_IP, 3),
    ];
    let packet_refs = packets.iter().map(Vec::as_slice).collect::<Vec<_>>();
    stack
        .queue_ipv4_for_host_validation(interface, &packet_refs)
        .unwrap();

    let exhausted = stack
        .pump(
            interface,
            &mut provider,
            Instant::from_micros(1),
            PumpBudget::new(1, 3),
        )
        .unwrap();
    assert_eq!(provider.submissions(), 2);
    assert_eq!(provider.live_tx(), 2);
    assert_eq!(provider.normal_exhaustions(), 1);
    assert!(exhausted.work_remaining);
    assert_eq!(exhausted.recheck, Recheck::Idle);

    let still_blocked = stack
        .pump(
            interface,
            &mut provider,
            Instant::from_micros(2),
            PumpBudget::new(1, 3),
        )
        .unwrap();
    assert_eq!(provider.submissions(), 2);
    assert_eq!(provider.live_tx(), 2);
    assert_eq!(provider.normal_exhaustions(), 2);
    assert!(still_blocked.work_remaining);
    assert_eq!(still_blocked.recheck, Recheck::Idle);

    provider.complete(0);
    provider.publish_recheck();
    provider.publish_recheck();
    assert_eq!(provider.recheck_publications(), 2);
    assert!(provider.take_recheck());
    assert!(!provider.take_recheck());

    stack
        .pump(
            interface,
            &mut provider,
            Instant::from_micros(3),
            PumpBudget::new(1, 3),
        )
        .unwrap();
    assert_eq!(provider.submissions(), 3);
    assert_eq!(provider.live_tx(), 2);
    assert_eq!(
        provider
            .submitted_frames()
            .iter()
            .map(|frame| raw_ipv4_marker(frame))
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );

    provider.complete_all();
    assert_eq!(provider.live_tx(), 0);
}

#[test]
fn token_cancel_reject_unwind_and_paired_consume_restore_one_owner() {
    let mut provider = BoundedProvider::with_mac([0x02, 0, 0, 0, 5, 1], 1);

    let TransmitOutcome::Ready(tx) = provider.transmit(Instant::ZERO) else {
        panic!("initial TX credit must be available")
    };
    drop(tx);
    assert_eq!(provider.tx_cancellations(), 1);
    assert_eq!(provider.live_tx(), 0);

    let TransmitOutcome::Ready(tx) = provider.transmit(Instant::ZERO) else {
        panic!("cancelled TX credit must be reusable")
    };
    let error = tx.consume(FRAME_CAPACITY + 1, |_| ()).unwrap_err();
    assert_eq!(error.requested(), FRAME_CAPACITY + 1);
    assert_eq!(provider.tx_rejections(), 1);
    assert_eq!(provider.live_tx(), 0);

    let TransmitOutcome::Ready(tx) = provider.transmit(Instant::ZERO) else {
        panic!("rejected TX credit must be reusable")
    };
    let tx_unwind = catch_unwind(AssertUnwindSafe(|| {
        let _ = tx.consume(1, |_| panic!("TX callback unwind probe"));
    }));
    assert!(tx_unwind.is_err());
    assert_eq!(provider.tx_cancellations(), 2);
    assert_eq!(provider.live_tx(), 0);

    provider.inject(&[1, 2, 3]);
    let ReceiveOutcome::Ready { rx, tx } = provider.receive(Instant::ZERO) else {
        panic!("paired RX/TX reservation must be available")
    };
    rx.consume(|frame| {
        assert_eq!(frame, [1, 2, 3]);
        tx.consume(1, |output| output[0] = 9).unwrap();
    });
    assert_eq!(provider.rx_recycles(), 1);
    assert_eq!(provider.live_tx(), 1);
    provider.complete(0);

    provider.inject(&[4, 5, 6]);
    let ReceiveOutcome::Ready { rx, tx } = provider.receive(Instant::ZERO) else {
        panic!("completed TX credit must pair with retained RX")
    };
    drop(tx);
    let rx_unwind = catch_unwind(AssertUnwindSafe(|| {
        rx.consume(|_| panic!("RX callback unwind probe"));
    }));
    assert!(rx_unwind.is_err());
    assert_eq!(provider.ready_rx(), 1);
    assert_eq!(provider.rx_cancellations(), 1);
}

#[test]
fn matching_completion_and_link_recheck_preserve_pending_frame() {
    const LOCAL_MAC: [u8; 6] = [0x02, 0, 0, 0, 6, 1];
    const PEER_MAC: [u8; 6] = [0x02, 0, 0, 0, 6, 2];
    const LOCAL_IP: [u8; 4] = [10, 0, 6, 2];
    const PEER_IP: [u8; 4] = [10, 0, 6, 1];

    let mut provider = BoundedProvider::with_mac(LOCAL_MAC, 2);
    for marker in [1, 2] {
        let TransmitOutcome::Ready(tx) = provider.transmit(Instant::ZERO) else {
            panic!("both TX credits must be reservable")
        };
        tx.consume(1, |frame| frame[0] = marker).unwrap();
    }
    provider.complete(0);
    assert_eq!(provider.tx_slot(0), TxSlot::Available);
    assert_eq!(provider.tx_slot(1), TxSlot::Submitted);
    provider.complete(1);

    let mut stack = Stack::new();
    let interface = stack.add_interface(
        &mut provider,
        EthernetAddress::new(LOCAL_MAC),
        Instant::ZERO,
    );
    stack
        .configure_ipv4_for_host_validation(interface, LOCAL_IP, 24)
        .unwrap();
    prime_bounded_neighbor(
        &mut stack,
        interface,
        &mut provider,
        PEER_MAC,
        PEER_IP,
        LOCAL_IP,
    );
    provider.reset_observation();
    provider.inject(&build_icmp_echo_request(
        PEER_MAC, LOCAL_MAC, PEER_IP, LOCAL_IP,
    ));
    provider.set_link_state(LinkState::Down);

    let blocked = stack
        .pump(
            interface,
            &mut provider,
            Instant::from_micros(1),
            PumpBudget::new(1, 1),
        )
        .unwrap();
    assert!(blocked.work_remaining);
    assert_eq!(blocked.recheck, Recheck::Idle);
    assert_eq!(provider.ready_rx(), 1);
    assert_eq!(provider.submissions(), 0);

    provider.set_link_state(LinkState::Up);
    provider.publish_recheck();
    provider.publish_recheck();
    assert!(provider.take_recheck());
    assert!(!provider.take_recheck());
    stack
        .pump(
            interface,
            &mut provider,
            Instant::from_micros(2),
            PumpBudget::new(1, 1),
        )
        .unwrap();
    assert_eq!(provider.ready_rx(), 0);
    assert_eq!(provider.submissions(), 1);
    assert_icmp_echo_reply(
        &provider.submitted_frames()[0],
        LOCAL_MAC,
        PEER_MAC,
        LOCAL_IP,
        PEER_IP,
    );
    provider.complete_all();
}

#[test]
fn finite_budgets_and_alternating_order_admit_both_directions() {
    const LOCAL_MAC: [u8; 6] = [0x02, 0, 0, 0, 7, 1];
    const PEER_MAC: [u8; 6] = [0x02, 0, 0, 0, 7, 2];
    const LOCAL_IP: [u8; 4] = [10, 0, 7, 2];
    const PEER_IP: [u8; 4] = [10, 0, 7, 1];

    let mut stack = Stack::new();
    let mut provider = BoundedProvider::with_capacities(LOCAL_MAC, 2, 1);
    let interface = stack.add_interface(
        &mut provider,
        EthernetAddress::new(LOCAL_MAC),
        Instant::ZERO,
    );
    stack
        .configure_ipv4_for_host_validation(interface, LOCAL_IP, 24)
        .unwrap();
    prime_bounded_neighbor(
        &mut stack,
        interface,
        &mut provider,
        PEER_MAC,
        PEER_IP,
        LOCAL_IP,
    );
    provider.reset_observation();

    let queued_egress = build_raw_ipv4_packet(LOCAL_IP, PEER_IP, 0xe1);
    stack
        .queue_ipv4_for_host_validation(interface, &[queued_egress.as_slice()])
        .unwrap();
    let ingress = build_icmp_echo_request(PEER_MAC, LOCAL_MAC, PEER_IP, LOCAL_IP);
    provider.inject(&ingress);
    provider.inject(&ingress);

    let first = stack
        .pump(
            interface,
            &mut provider,
            Instant::from_micros(1),
            PumpBudget::new(1, 1),
        )
        .unwrap();
    assert!(first.work_remaining);
    assert_eq!(first.recheck, Recheck::Idle);
    assert_eq!(provider.submissions(), 1);
    assert_eq!(raw_ipv4_marker(&provider.submitted_frames()[0]), 0xe1);
    assert_eq!(provider.ready_rx(), 2);

    provider.complete(0);
    let second = stack
        .pump(
            interface,
            &mut provider,
            Instant::from_micros(2),
            PumpBudget::new(1, 1),
        )
        .unwrap();
    assert!(second.work_remaining);
    assert_eq!(second.recheck, Recheck::Immediate);
    assert_eq!(provider.submissions(), 2);
    assert_eq!(provider.ready_rx(), 1);
    assert_icmp_echo_reply(
        &provider.submitted_frames()[1],
        LOCAL_MAC,
        PEER_MAC,
        LOCAL_IP,
        PEER_IP,
    );
    provider.complete_all();
}

#[test]
fn egress_budget_stops_at_the_exact_nonzero_boundary() {
    const LOCAL_MAC: [u8; 6] = [0x02, 0, 0, 0, 8, 1];
    const PEER_MAC: [u8; 6] = [0x02, 0, 0, 0, 8, 2];
    const LOCAL_IP: [u8; 4] = [10, 0, 8, 2];
    const PEER_IP: [u8; 4] = [10, 0, 8, 1];

    let mut stack = Stack::new();
    let mut provider = BoundedProvider::with_mac(LOCAL_MAC, 4);
    let interface = stack.add_interface(
        &mut provider,
        EthernetAddress::new(LOCAL_MAC),
        Instant::ZERO,
    );
    stack
        .configure_ipv4_for_host_validation(interface, LOCAL_IP, 24)
        .unwrap();
    prime_bounded_neighbor(
        &mut stack,
        interface,
        &mut provider,
        PEER_MAC,
        PEER_IP,
        LOCAL_IP,
    );
    provider.reset_observation();

    let packets = [1, 2, 3].map(|marker| build_raw_ipv4_packet(LOCAL_IP, PEER_IP, marker));
    let packet_refs = packets.iter().map(Vec::as_slice).collect::<Vec<_>>();
    stack
        .queue_ipv4_for_host_validation(interface, &packet_refs)
        .unwrap();
    let boundary = stack
        .pump(
            interface,
            &mut provider,
            Instant::from_micros(1),
            PumpBudget::new(1, 2),
        )
        .unwrap();
    assert_eq!(provider.submissions(), 2);
    assert_eq!(boundary.recheck, Recheck::Immediate);

    stack
        .pump(
            interface,
            &mut provider,
            Instant::from_micros(2),
            PumpBudget::new(1, 2),
        )
        .unwrap();
    assert_eq!(provider.submissions(), 3);
    provider.complete_all();
}

#[test]
#[should_panic(expected = "ingress pump budget must be non-zero")]
fn zero_ingress_budget_is_rejected() {
    let _ = PumpBudget::new(0, 1);
}

#[test]
#[should_panic(expected = "egress pump budget must be non-zero")]
fn zero_egress_budget_is_rejected() {
    let _ = PumpBudget::new(1, 0);
}
