mod support;

use anemone_net_api::{EthernetAddress, Instant, LinkState, Recheck};
use anemone_smoltcp_stack::{PumpBudget, PumpError, Stack};

use support::{
    frame::BoundedProvider,
    packet::{assert_icmp_echo_reply, build_icmp_echo_request, prime_bounded_neighbor},
};

const FIRST_MAC: [u8; 6] = [0x02, 0, 0, 0, 13, 1];
const FIRST_PEER_MAC: [u8; 6] = [0x02, 0, 0, 0, 13, 2];
const FIRST_IP: [u8; 4] = [10, 0, 13, 2];
const FIRST_PEER_IP: [u8; 4] = [10, 0, 13, 1];
const SECOND_MAC: [u8; 6] = [0x02, 0, 0, 0, 14, 1];
const SECOND_PEER_MAC: [u8; 6] = [0x02, 0, 0, 0, 14, 2];
const SECOND_IP: [u8; 4] = [10, 0, 14, 2];
const SECOND_PEER_IP: [u8; 4] = [10, 0, 14, 1];

fn two_interface_stack() -> (
    Stack,
    anemone_net_api::InterfaceId,
    BoundedProvider,
    anemone_net_api::InterfaceId,
    BoundedProvider,
) {
    let mut stack = Stack::new();
    let mut first = BoundedProvider::with_mac(FIRST_MAC, 1);
    let mut second = BoundedProvider::with_mac(SECOND_MAC, 1);
    let first_id = stack.add_interface(&mut first, EthernetAddress::new(FIRST_MAC), Instant::ZERO);
    let second_id =
        stack.add_interface(&mut second, EthernetAddress::new(SECOND_MAC), Instant::ZERO);
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
    (stack, first_id, first, second_id, second)
}

#[test]
fn one_stack_advances_two_providers_without_cross_provider_consumption() {
    let (mut stack, first_id, mut first, second_id, mut second) = two_interface_stack();

    first.inject(&build_icmp_echo_request(
        FIRST_PEER_MAC,
        FIRST_MAC,
        FIRST_PEER_IP,
        FIRST_IP,
    ));
    first.set_link_state(LinkState::Down);
    let blocked = stack
        .pump(
            first_id,
            &mut first,
            Instant::from_micros(1),
            PumpBudget::new(1, 1),
        )
        .unwrap();
    assert!(blocked.work_remaining);
    assert_eq!(blocked.recheck, Recheck::Idle);
    assert_eq!(first.ready_rx(), 1);
    assert_eq!(first.submissions(), 0);

    second.inject(&build_icmp_echo_request(
        SECOND_PEER_MAC,
        SECOND_MAC,
        SECOND_PEER_IP,
        SECOND_IP,
    ));
    stack
        .pump(
            second_id,
            &mut second,
            Instant::from_micros(2),
            PumpBudget::new(1, 1),
        )
        .unwrap();
    assert_eq!(second.ready_rx(), 0);
    assert_eq!(second.submissions(), 1);
    assert_icmp_echo_reply(
        &second.submitted_frames()[0],
        SECOND_MAC,
        SECOND_PEER_MAC,
        SECOND_IP,
        SECOND_PEER_IP,
    );
    second.complete_all();

    // Pumping the second mapping cannot consume the first provider's frame or
    // publish its resource truth, even though both mappings share one Stack.
    assert_eq!(first.ready_rx(), 1);
    assert_eq!(first.submissions(), 0);
    first.set_link_state(LinkState::Up);
    stack
        .pump(
            first_id,
            &mut first,
            Instant::from_micros(3),
            PumpBudget::new(1, 1),
        )
        .unwrap();
    assert_eq!(first.ready_rx(), 0);
    assert_eq!(first.submissions(), 1);
    assert_icmp_echo_reply(
        &first.submitted_frames()[0],
        FIRST_MAC,
        FIRST_PEER_MAC,
        FIRST_IP,
        FIRST_PEER_IP,
    );
}

#[test]
fn mapping_rollback_isolated_and_interface_ids_remain_monotonic() {
    let (mut stack, first_id, mut first, second_id, mut second) = two_interface_stack();
    assert_eq!(first_id.index(), 0);
    assert_eq!(second_id.index(), 1);

    stack.remove_interface(first_id).unwrap();
    assert_eq!(
        stack.pump(
            first_id,
            &mut first,
            Instant::from_micros(1),
            PumpBudget::new(1, 1),
        ),
        Err(PumpError::UnknownInterface(first_id))
    );

    second.inject(&build_icmp_echo_request(
        SECOND_PEER_MAC,
        SECOND_MAC,
        SECOND_PEER_IP,
        SECOND_IP,
    ));
    stack
        .pump(
            second_id,
            &mut second,
            Instant::from_micros(2),
            PumpBudget::new(1, 1),
        )
        .unwrap();
    assert_eq!(second.ready_rx(), 0);
    assert_eq!(second.submissions(), 1);
    assert_icmp_echo_reply(
        &second.submitted_frames()[0],
        SECOND_MAC,
        SECOND_PEER_MAC,
        SECOND_IP,
        SECOND_PEER_IP,
    );

    let replacement = stack.add_interface(
        &mut first,
        EthernetAddress::new(FIRST_MAC),
        Instant::from_micros(3),
    );
    assert_eq!(replacement.index(), 2);
    assert_ne!(replacement, second_id);
}
