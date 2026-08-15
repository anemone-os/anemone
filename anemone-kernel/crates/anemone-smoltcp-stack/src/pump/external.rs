//! External-provider bounded Stack progression.

use anemone_net_api::{FrameProvider, Instant, InterfaceId, PumpOutcome};
use smoltcp::iface::{PollIngressSingleResult, PollResult};

use crate::{
    adapter::{FrameDevice, from_smoltcp_instant, to_smoltcp_instant},
    stack::{InterfaceEntry, Protocols, PumpError, PumpOrder, Stack},
};

use super::common::{PumpBudget, RoundContinuation, pump_outcome};

impl Stack {
    pub fn pump<P: FrameProvider>(
        &mut self,
        id: InterfaceId,
        provider: &mut P,
        now: Instant,
        budget: PumpBudget,
    ) -> Result<PumpOutcome, PumpError> {
        let Self {
            interfaces,
            protocols,
            ..
        } = self;
        let entry = interfaces
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or(PumpError::UnknownInterface(id))?;

        // smoltcp snapshots this capability when the interface is created. A
        // different value here means the caller supplied the wrong provider.
        assert_eq!(
            entry.frame_capacity,
            provider.capabilities().max_frame_len,
            "frame capacity changed after interface creation"
        );

        let smoltcp_now = to_smoltcp_instant(now);
        let mut device = FrameDevice::new(provider);
        entry.interface.poll_maintenance(smoltcp_now);
        let active = protocols.prepare_egress(id, &mut entry.protocols, &mut entry.sockets);
        let (ingress_may_remain, egress_may_remain) = match entry.next_pump_order {
            PumpOrder::IngressFirst => (
                poll_ingress(
                    entry,
                    protocols,
                    &mut device,
                    smoltcp_now,
                    budget.ingress_frames(),
                ),
                poll_egress(entry, &mut device, smoltcp_now, budget.egress_steps()),
            ),
            PumpOrder::EgressFirst => {
                let egress_may_remain =
                    poll_egress(entry, &mut device, smoltcp_now, budget.egress_steps());
                let ingress_may_remain = poll_ingress(
                    entry,
                    protocols,
                    &mut device,
                    smoltcp_now,
                    budget.ingress_frames(),
                );
                (ingress_may_remain, egress_may_remain)
            },
        };
        let protocol_egress_may_remain =
            protocols.complete_egress(active, id, &entry.protocols, &entry.sockets);
        // Deferred TCP resources stay engine-owned until a pump has emitted
        // their final protocol work; only then may the old generation detach.
        let tcp_progression = protocols.reclaim_tcp(id, &mut entry.sockets);
        // A TCP timer may commit a terminal state before an exhausted TX
        // provider prevents smoltcp from reporting SocketStateChanged. This is
        // a conservative recheck hint; endpoint facts remain the sole truth.
        protocols.invalidate_tcp_interface(id);
        entry.next_pump_order = entry.next_pump_order.next();

        let next_deadline = entry
            .interface
            .poll_at(smoltcp_now, &entry.sockets)
            .map(from_smoltcp_instant);
        let continuation = if device.blocked_work() {
            // External TX credit and link availability are provider-owned.
            // Repeating the same round cannot progress until its durable
            // completion/link edge requests a recheck.
            RoundContinuation::AwaitProviderEdge
        } else if ingress_may_remain
            || egress_may_remain
            || protocol_egress_may_remain
            || tcp_progression
        {
            RoundContinuation::Runnable
        } else {
            RoundContinuation::Quiescent
        };
        Ok(pump_outcome(continuation, now, next_deadline))
    }
}

fn poll_ingress<P: FrameProvider>(
    entry: &mut InterfaceEntry,
    protocols: &mut Protocols,
    device: &mut FrameDevice<'_, P>,
    now: smoltcp::time::Instant,
    budget: usize,
) -> bool {
    protocols.drain_engine_ingress(entry.id, &mut entry.sockets);
    let mut processed = 0;
    while processed < budget {
        let result = entry.interface.poll_ingress_single_with_ipv4_observer(
            now,
            device,
            &mut entry.sockets,
            &mut |packet| protocols.observe_admitted_ipv4(packet),
        );
        match result {
            PollIngressSingleResult::None => break,
            PollIngressSingleResult::PacketProcessed
            | PollIngressSingleResult::SocketStateChanged => processed += 1,
        }
        protocols.drain_engine_ingress(entry.id, &mut entry.sockets);
    }
    processed == budget
}

fn poll_egress<P: FrameProvider>(
    entry: &mut InterfaceEntry,
    device: &mut FrameDevice<'_, P>,
    now: smoltcp::time::Instant,
    budget: usize,
) -> bool {
    let mut processed = 0;
    while processed < budget {
        match entry.interface.poll_egress(now, device, &mut entry.sockets) {
            PollResult::None => break,
            PollResult::SocketStateChanged => processed += 1,
        }
    }
    processed == budget
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use anemone_net_api::{
        EthernetAddress, FrameCapabilities, FrameSizeError, Ipv4Address as ApiIpv4Address,
        Ipv4Cidr, Ipv4EgressSelection, LinkState, ReceiveOutcome, Recheck, RxToken,
        TransmitOutcome, TxToken,
        icmp_raw::IcmpRawNamespacePolicy,
        tcp::{TcpConnectFact, TcpEndpointFacts, TcpPeer},
        udp::UdpNamespacePolicy,
    };
    use smoltcp::{
        phy::ChecksumCapabilities,
        socket::raw,
        wire::{IpProtocol, IpVersion, Ipv4Address, Ipv4Packet, Ipv4Repr},
    };

    use super::*;

    use crate::{stack::StackPolicy, tcp::TcpPolicy};

    struct UnusedRxToken;

    impl RxToken for UnusedRxToken {
        fn consume<R, F>(self, _f: F) -> R
        where
            F: FnOnce(&[u8]) -> R,
        {
            panic!("unavailable provider cannot issue an RX token")
        }
    }

    struct UnusedTxToken;

    impl TxToken for UnusedTxToken {
        fn capacity(&self) -> usize {
            128
        }

        fn consume<R, F>(self, _len: usize, _f: F) -> Result<R, FrameSizeError>
        where
            F: FnOnce(&mut [u8]) -> R,
        {
            panic!("unavailable provider cannot issue a TX token")
        }
    }

    struct UnavailableProvider {
        transmit_calls: usize,
    }

    impl FrameProvider for UnavailableProvider {
        type RxToken<'a> = UnusedRxToken;
        type TxToken<'a> = UnusedTxToken;

        fn receive(
            &mut self,
            _now: Instant,
        ) -> ReceiveOutcome<Self::RxToken<'_>, Self::TxToken<'_>> {
            ReceiveOutcome::Empty
        }

        fn transmit(&mut self, _now: Instant) -> TransmitOutcome<Self::TxToken<'_>> {
            self.transmit_calls += 1;
            TransmitOutcome::Exhausted
        }

        fn capabilities(&self) -> FrameCapabilities {
            FrameCapabilities { max_frame_len: 128 }
        }

        fn link_state(&self) -> LinkState {
            LinkState::Up
        }
    }

    #[test]
    fn queued_real_socket_waits_for_owner_when_provider_is_blocked() {
        let mut stack = Stack::new();
        let mut provider = UnavailableProvider { transmit_calls: 0 };
        let interface = stack.add_interface(
            &mut provider,
            EthernetAddress::new([0x02, 0, 0, 0, 0, 1]),
            Instant::ZERO,
        );
        let mut raw_socket = raw::Socket::new(
            Some(IpVersion::Ipv4),
            None,
            raw::PacketBuffer::new(vec![raw::PacketMetadata::EMPTY], vec![0; 64]),
            raw::PacketBuffer::new(vec![raw::PacketMetadata::EMPTY], vec![0; 64]),
        );
        let ipv4 = Ipv4Repr {
            src_addr: Ipv4Address::new(10, 0, 0, 2),
            dst_addr: Ipv4Address::new(10, 0, 0, 1),
            next_header: IpProtocol::Unknown(253),
            payload_len: 0,
            hop_limit: 64,
        };
        let mut packet = vec![0; ipv4.buffer_len()];
        ipv4.emit(
            &mut Ipv4Packet::new_unchecked(&mut packet[..]),
            &ChecksumCapabilities::default(),
        );
        raw_socket.send_slice(&packet).unwrap();
        stack
            .interface_mut(interface)
            .unwrap()
            .sockets
            .add(raw_socket);

        let outcome = stack
            .pump(
                interface,
                &mut provider,
                Instant::ZERO,
                PumpBudget::new(1, 1),
            )
            .unwrap();

        assert!(outcome.work_remaining);
        assert_eq!(outcome.recheck, Recheck::Idle);
        assert_eq!(outcome.next_deadline, None);
        assert_eq!(provider.transmit_calls, 1);
    }

    #[test]
    fn tcp_timeout_invalidates_even_when_provider_is_exhausted() {
        let mut stack = Stack::with_policy(StackPolicy::new(
            UdpNamespacePolicy::new(4, 30000, 30003),
            IcmpRawNamespacePolicy::new(4),
            TcpPolicy::new(4, 4, 1, 2, 64, 64, 4, 1, 60_000, 40000, 40003),
        ));
        let mut provider = UnavailableProvider { transmit_calls: 0 };
        let interface = stack.add_interface(
            &mut provider,
            EthernetAddress::new([0x02, 0, 0, 0, 0, 1]),
            Instant::ZERO,
        );
        let local = ApiIpv4Address::new([10, 0, 0, 2]);
        stack
            .configure_external_ipv4(interface, Ipv4Cidr::new(local, 24).unwrap(), None)
            .unwrap();
        let endpoint = stack.create_tcp_endpoint().unwrap();
        let _ = stack
            .start_tcp_connect(
                endpoint,
                Ipv4EgressSelection::new(interface, local),
                TcpPeer::new(ApiIpv4Address::new([10, 0, 0, 1]), 80),
            )
            .unwrap();
        let _ = stack.take_invalidations();

        stack
            .pump(
                interface,
                &mut provider,
                Instant::ZERO,
                PumpBudget::new(1, 1),
            )
            .unwrap();
        let _ = stack.take_invalidations();
        stack
            .pump(
                interface,
                &mut provider,
                Instant::from_micros(2_000),
                PumpBudget::new(1, 1),
            )
            .unwrap();

        let invalidations = stack.take_invalidations().into_parts().2;
        assert!(
            invalidations
                .iter()
                .any(|invalidation| invalidation.endpoint() == endpoint)
        );
        let TcpEndpointFacts::Connection(facts) = stack.tcp_endpoint_facts(endpoint).unwrap()
        else {
            panic!("timed-out active open lost connection facts");
        };
        assert_eq!(facts.connect(), TcpConnectFact::Failed);
        assert!(facts.has_pending_error());
        assert!(facts.is_terminal());
    }
}
