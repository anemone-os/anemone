//! Conditional production-path validation for Stage 2 local UDP handoff.
//!
//! These tests use the real boot-published control plane, DomainStack and
//! sleeping local worker. Stage 3 must delete the operation bridge calls once
//! its real Endpoint consumer covers the same paths.

use anemone_net_api::Ipv4Address;

use super::ACTIVE_PATHS;
use crate::prelude::*;

fn deliver_over_production_local_path(
    destination: Ipv4Address,
    sender_port: u16,
    receiver_port: u16,
    payload: &[u8],
) {
    let (stack, selection) = {
        let authority = ACTIVE_PATHS.lock();
        let selection = authority
            .domain
            .control_plane()
            .expect("KUnit runs after boot control-plane publication")
            .select(destination, None)
            .expect("KUnit destination must select the production local path");
        (authority.domain.stack(), selection)
    };

    let sender = stack
        .create_udp_for_kunit(sender_port)
        .expect("KUnit sender Endpoint must bind");
    let receiver = stack
        .create_udp_for_kunit(receiver_port)
        .expect("KUnit receiver Endpoint must bind");
    stack
        .send_udp_for_kunit(
            sender,
            selection.interface(),
            selection.source(),
            destination,
            receiver_port,
            payload,
        )
        .expect("KUnit local datagram must queue");
    selection.request_pump();

    let mut received = None;
    for _ in 0..10_000 {
        if let Some(datagram) = stack.receive_udp_for_kunit(receiver) {
            received = Some(datagram);
            break;
        }
        yield_now();
    }
    let received = received.expect("bounded local worker did not deliver KUnit datagram");
    assert_eq!(received.payload.as_slice(), payload);
    assert_eq!(received.source_address, selection.source());
    assert_eq!(received.source_port, sender_port);

    stack
        .retire_udp_for_kunit(sender)
        .expect("KUnit sender Endpoint must retire");
    stack
        .retire_udp_for_kunit(receiver)
        .expect("KUnit receiver Endpoint must retire");
}

#[kunit]
fn udp_loopback_worker_127_0_0_1() {
    deliver_over_production_local_path(Ipv4Address::LOOPBACK, 39001, 39002, b"loopback");
}

#[kunit]
fn udp_loopback_worker_other_127_8() {
    deliver_over_production_local_path(
        Ipv4Address::new([127, 17, 23, 42]),
        39003,
        39004,
        b"loopback-cidr",
    );
}

#[kunit]
fn udp_self_external_uses_local_handoff() {
    let Some(destination) = crate::network_defs::STATIC_IPV4_DEPLOYMENT
        .map(|deployment| Ipv4Address::new(deployment.address))
    else {
        kinfoln!("self-external local-handoff KUnit skipped for loopback-only SystemTarget");
        return;
    };
    deliver_over_production_local_path(destination, 39005, 39006, b"self-external");
}
