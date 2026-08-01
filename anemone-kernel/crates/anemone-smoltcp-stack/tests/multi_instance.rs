mod support;

use anemone_net_api::{Duration, EthernetAddress, Instant, LinkState, Recheck};
use anemone_smoltcp_stack::{PumpBudget, PumpError, Stack};

use support::{
    clock::ManualClock,
    frame::BoundedProvider,
    packet::{
        assert_icmp_echo_reply, build_icmp_echo_request, build_raw_ipv4_packet,
        prime_bounded_neighbor,
    },
};

#[test]
fn blocked_instance_does_not_change_another_instances_progress_or_recheck() {
    const FIRST_MAC: [u8; 6] = [0x02, 0, 0, 0, 9, 1];
    const FIRST_PEER_MAC: [u8; 6] = [0x02, 0, 0, 0, 9, 2];
    const FIRST_IP: [u8; 4] = [10, 0, 9, 2];
    const FIRST_PEER_IP: [u8; 4] = [10, 0, 9, 1];
    const SECOND_MAC: [u8; 6] = [0x02, 0, 0, 0, 10, 1];
    const SECOND_PEER_MAC: [u8; 6] = [0x02, 0, 0, 0, 10, 2];
    const SECOND_IP: [u8; 4] = [10, 0, 10, 2];
    const SECOND_PEER_IP: [u8; 4] = [10, 0, 10, 1];

    let mut first_stack = Stack::new();
    let mut second_stack = Stack::new();
    let mut first_provider = BoundedProvider::with_mac(FIRST_MAC, 1);
    let mut second_provider = BoundedProvider::with_mac(SECOND_MAC, 1);
    let first_id = first_stack.add_interface(
        &mut first_provider,
        EthernetAddress::new(FIRST_MAC),
        Instant::ZERO,
    );
    let second_id = second_stack.add_interface(
        &mut second_provider,
        EthernetAddress::new(SECOND_MAC),
        Instant::ZERO,
    );
    first_stack
        .configure_ipv4_for_host_validation(first_id, FIRST_IP, 24)
        .unwrap();
    second_stack
        .configure_ipv4_for_host_validation(second_id, SECOND_IP, 24)
        .unwrap();
    prime_bounded_neighbor(
        &mut first_stack,
        first_id,
        &mut first_provider,
        FIRST_PEER_MAC,
        FIRST_PEER_IP,
        FIRST_IP,
    );
    prime_bounded_neighbor(
        &mut second_stack,
        second_id,
        &mut second_provider,
        SECOND_PEER_MAC,
        SECOND_PEER_IP,
        SECOND_IP,
    );
    first_provider.reset_observation();
    second_provider.reset_observation();
    let mut first_clock = ManualClock::new();
    let mut second_clock = ManualClock::new();

    let first_packets = [
        build_raw_ipv4_packet(FIRST_IP, FIRST_PEER_IP, 1),
        build_raw_ipv4_packet(FIRST_IP, FIRST_PEER_IP, 2),
    ];
    first_stack
        .queue_ipv4_for_host_validation(
            first_id,
            &first_packets.iter().map(Vec::as_slice).collect::<Vec<_>>(),
        )
        .unwrap();
    first_clock.advance(Duration::from_micros(10));
    let exhausted = first_stack
        .pump(
            first_id,
            &mut first_provider,
            first_clock.now,
            PumpBudget::new(1, 2),
        )
        .unwrap();
    assert_eq!(exhausted.recheck, Recheck::Idle);
    assert!(exhausted.work_remaining);
    assert_eq!(first_provider.submissions(), 1);
    assert_eq!(first_provider.live_tx(), 1);
    assert_eq!(first_provider.normal_exhaustions(), 1);

    // Keep the first completion withheld and its link down while the other
    // owner consumes RX, submits TX, and reclaims the matching credit.
    first_provider.set_link_state(LinkState::Down);
    second_provider.inject(&build_icmp_echo_request(
        SECOND_PEER_MAC,
        SECOND_MAC,
        SECOND_PEER_IP,
        SECOND_IP,
    ));
    second_clock.advance(Duration::from_micros(70));
    second_stack
        .pump(
            second_id,
            &mut second_provider,
            second_clock.now,
            PumpBudget::new(1, 1),
        )
        .unwrap();
    assert_eq!(second_provider.ready_rx(), 0);
    assert_eq!(second_provider.submissions(), 1);
    assert_eq!(second_provider.live_tx(), 1);
    assert_icmp_echo_reply(
        &second_provider.submitted_frames()[0],
        SECOND_MAC,
        SECOND_PEER_MAC,
        SECOND_IP,
        SECOND_PEER_IP,
    );
    second_provider.complete(0);
    assert_eq!(second_provider.live_tx(), 0);
    assert_eq!(first_provider.live_tx(), 1);

    let second_observation = (
        second_provider.submissions(),
        second_provider.live_tx(),
        second_provider.ready_rx(),
        second_provider.observed_at(),
        second_provider.receive_calls(),
        second_provider.transmit_calls(),
        second_provider.recheck_publications(),
    );

    first_provider.complete(0);
    first_provider.publish_recheck();
    assert!(first_provider.take_recheck());
    assert!(!second_provider.take_recheck());
    first_clock.advance(Duration::from_micros(10));
    let link_blocked = first_stack
        .pump(
            first_id,
            &mut first_provider,
            first_clock.now,
            PumpBudget::new(1, 2),
        )
        .unwrap();
    assert_eq!(link_blocked.recheck, Recheck::Idle);
    assert_eq!(first_provider.submissions(), 1);
    assert_eq!(second_observation.0, second_provider.submissions());
    assert_eq!(second_observation.1, second_provider.live_tx());
    assert_eq!(second_observation.2, second_provider.ready_rx());
    assert_eq!(second_observation.3, second_provider.observed_at());
    assert_eq!(second_observation.4, second_provider.receive_calls());
    assert_eq!(second_observation.5, second_provider.transmit_calls());
    assert_eq!(second_observation.6, second_provider.recheck_publications());

    first_provider.set_link_state(LinkState::Up);
    first_provider.publish_recheck();
    assert!(first_provider.take_recheck());
    assert!(!second_provider.take_recheck());
    first_clock.advance(Duration::from_micros(10));
    first_stack
        .pump(
            first_id,
            &mut first_provider,
            first_clock.now,
            PumpBudget::new(1, 2),
        )
        .unwrap();
    assert_eq!(first_provider.submissions(), 2);
    assert_eq!(first_provider.live_tx(), 1);
    first_provider.complete_all();
    assert_eq!(second_observation.3, second_provider.observed_at());
    assert_eq!(second_observation.4, second_provider.receive_calls());
    assert_eq!(second_observation.5, second_provider.transmit_calls());
}

#[test]
fn mapping_rollback_keeps_namespaces_monotonic_and_other_stack_live() {
    const FAILED_MAC: [u8; 6] = [0x02, 0, 0, 0, 11, 1];
    const FAILED_PEER_MAC: [u8; 6] = [0x02, 0, 0, 0, 11, 2];
    const FAILED_IP: [u8; 4] = [10, 0, 11, 2];
    const FAILED_PEER_IP: [u8; 4] = [10, 0, 11, 1];
    const LIVE_MAC: [u8; 6] = [0x02, 0, 0, 0, 12, 1];
    const LIVE_PEER_MAC: [u8; 6] = [0x02, 0, 0, 0, 12, 2];
    const LIVE_IP: [u8; 4] = [10, 0, 12, 2];
    const LIVE_PEER_IP: [u8; 4] = [10, 0, 12, 1];

    let mut failed_stack = Stack::new();
    let mut live_stack = Stack::new();
    let mut failed_provider = BoundedProvider::with_mac(FAILED_MAC, 1);
    let mut live_provider = BoundedProvider::with_mac(LIVE_MAC, 1);
    let failed_id = failed_stack.add_interface(
        &mut failed_provider,
        EthernetAddress::new(FAILED_MAC),
        Instant::ZERO,
    );
    let live_id = live_stack.add_interface(
        &mut live_provider,
        EthernetAddress::new(LIVE_MAC),
        Instant::ZERO,
    );
    // Equal indices are scoped to different Stack owners; neither ID is used
    // to dereference the other owner's private mapping.
    assert_eq!(failed_id.index(), 0);
    assert_eq!(live_id.index(), 0);

    failed_stack
        .configure_ipv4_for_host_validation(failed_id, FAILED_IP, 24)
        .unwrap();
    live_stack
        .configure_ipv4_for_host_validation(live_id, LIVE_IP, 24)
        .unwrap();
    prime_bounded_neighbor(
        &mut live_stack,
        live_id,
        &mut live_provider,
        LIVE_PEER_MAC,
        LIVE_PEER_IP,
        LIVE_IP,
    );
    live_provider.reset_observation();
    let queued = build_raw_ipv4_packet(FAILED_IP, FAILED_PEER_IP, 0x11);
    failed_stack
        .queue_ipv4_for_host_validation(failed_id, &[queued.as_slice()])
        .unwrap();
    failed_provider.inject(&build_icmp_echo_request(
        FAILED_PEER_MAC,
        FAILED_MAC,
        FAILED_PEER_IP,
        FAILED_IP,
    ));

    failed_stack.remove_interface(failed_id).unwrap();
    assert_eq!(
        failed_stack.pump(
            failed_id,
            &mut failed_provider,
            Instant::from_micros(1),
            PumpBudget::new(1, 1),
        ),
        Err(PumpError::UnknownInterface(failed_id))
    );
    assert_eq!(failed_provider.ready_rx(), 1);
    assert_eq!(failed_provider.submissions(), 0);
    assert_eq!(failed_provider.live_tx(), 0);
    assert_eq!(failed_provider.receive_calls(), 0);
    assert_eq!(failed_provider.transmit_calls(), 0);

    let replacement_id = failed_stack.add_interface(
        &mut failed_provider,
        EthernetAddress::new(FAILED_MAC),
        Instant::from_micros(2),
    );
    assert_eq!(replacement_id.index(), 1);
    assert_eq!(failed_provider.ready_rx(), 1);
    assert_eq!(failed_provider.live_tx(), 0);

    live_provider.inject(&build_icmp_echo_request(
        LIVE_PEER_MAC,
        LIVE_MAC,
        LIVE_PEER_IP,
        LIVE_IP,
    ));
    live_stack
        .pump(
            live_id,
            &mut live_provider,
            Instant::from_micros(99),
            PumpBudget::new(1, 1),
        )
        .unwrap();
    assert_eq!(live_provider.ready_rx(), 0);
    assert_eq!(live_provider.submissions(), 1);
    assert_icmp_echo_reply(
        &live_provider.submitted_frames()[0],
        LIVE_MAC,
        LIVE_PEER_MAC,
        LIVE_IP,
        LIVE_PEER_IP,
    );
    live_provider.complete_all();
    assert_eq!(failed_provider.ready_rx(), 1);
    assert_eq!(failed_provider.submissions(), 0);
    assert_eq!(failed_provider.live_tx(), 0);
}
