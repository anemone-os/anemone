//! External-provider bounded Stack progression.

use anemone_net_api::{FrameProvider, Instant, InterfaceId, PumpOutcome};
use smoltcp::iface::{PollIngressSingleResult, PollResult};

use crate::{
    adapter::{FrameDevice, from_smoltcp_instant, to_smoltcp_instant},
    stack::{InterfaceEntry, PumpError, PumpOrder, Stack},
    udp::UdpEndpoints,
};

use super::common::{PumpBudget, pump_outcome};

impl Stack {
    pub fn pump<P: FrameProvider>(
        &mut self,
        id: InterfaceId,
        provider: &mut P,
        now: Instant,
        budget: PumpBudget,
    ) -> Result<PumpOutcome, PumpError> {
        let Self {
            interfaces, udp, ..
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
        let active_endpoint = udp.prepare_egress(id, &mut entry.sockets);

        let (ingress_may_remain, egress_may_remain) = match entry.next_pump_order {
            PumpOrder::IngressFirst => (
                poll_ingress(
                    entry,
                    udp,
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
                    udp,
                    &mut device,
                    smoltcp_now,
                    budget.ingress_frames(),
                );
                (ingress_may_remain, egress_may_remain)
            },
        };
        let udp_egress_may_remain = udp.complete_egress(active_endpoint, id, &entry.sockets);
        entry.next_pump_order = entry.next_pump_order.next();

        let next_deadline = entry
            .interface
            .poll_at(smoltcp_now, &entry.sockets)
            .map(from_smoltcp_instant);
        Ok(pump_outcome(
            device.blocked_work(),
            ingress_may_remain,
            egress_may_remain || udp_egress_may_remain,
            now,
            next_deadline,
        ))
    }
}

fn poll_ingress<P: FrameProvider>(
    entry: &mut InterfaceEntry,
    udp: &mut UdpEndpoints,
    device: &mut FrameDevice<'_, P>,
    now: smoltcp::time::Instant,
    budget: usize,
) -> bool {
    udp.drain_ingress(entry.id, &mut entry.sockets);
    let mut processed = 0;
    while processed < budget {
        match entry
            .interface
            .poll_ingress_single(now, device, &mut entry.sockets)
        {
            PollIngressSingleResult::None => break,
            PollIngressSingleResult::PacketProcessed
            | PollIngressSingleResult::SocketStateChanged => processed += 1,
        }
        udp.drain_ingress(entry.id, &mut entry.sockets);
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
        EthernetAddress, FrameCapabilities, FrameSizeError, LinkState, ReceiveOutcome, Recheck,
        RxToken, TransmitOutcome, TxToken,
    };
    use smoltcp::{
        phy::ChecksumCapabilities,
        socket::raw,
        wire::{IpProtocol, IpVersion, Ipv4Address, Ipv4Packet, Ipv4Repr},
    };

    use super::*;

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
}
