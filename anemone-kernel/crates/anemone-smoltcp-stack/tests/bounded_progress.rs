mod support;

use anemone_net_api::{EthernetAddress, Instant, Recheck};
use anemone_smoltcp_stack::{PumpBudget, Stack};

use support::{BoundedProvider, build_raw_ipv4_packet, prime_bounded_neighbor, raw_ipv4_marker};

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
    // This is the known Checkpoint 2 input: owner-blocked work still reports
    // an immediate recheck today. Checkpoint 1 records but does not fix it.
    assert_eq!(exhausted.recheck, Recheck::Immediate);

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
