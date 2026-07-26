use anemone_net_api::{FrameProvider, Instant, InterfaceId, PumpOutcome, Recheck};
use smoltcp::iface::{PollIngressSingleResult, PollResult};

use crate::{
    adapter::{FrameDevice, from_smoltcp_instant, to_smoltcp_instant},
    stack::{PumpError, Stack},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PumpBudget {
    ingress_frames: usize,
    egress_steps: usize,
}

impl PumpBudget {
    pub const fn new(ingress_frames: usize, egress_steps: usize) -> Self {
        assert!(ingress_frames > 0, "ingress pump budget must be non-zero");
        assert!(egress_steps > 0, "egress pump budget must be non-zero");
        Self {
            ingress_frames,
            egress_steps,
        }
    }
}

impl Stack {
    pub fn pump<P: FrameProvider>(
        &mut self,
        id: InterfaceId,
        provider: &mut P,
        now: Instant,
        budget: PumpBudget,
    ) -> Result<PumpOutcome, PumpError> {
        let entry = self.interface_mut(id)?;

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

        let mut ingress_processed = 0;
        while ingress_processed < budget.ingress_frames {
            match entry
                .interface
                .poll_ingress_single(smoltcp_now, &mut device, &mut entry.sockets)
            {
                PollIngressSingleResult::None => break,
                PollIngressSingleResult::PacketProcessed
                | PollIngressSingleResult::SocketStateChanged => ingress_processed += 1,
            }
        }
        let ingress_may_remain = ingress_processed == budget.ingress_frames;

        let mut egress_processed = 0;
        while egress_processed < budget.egress_steps {
            match entry
                .interface
                .poll_egress(smoltcp_now, &mut device, &mut entry.sockets)
            {
                PollResult::None => break,
                PollResult::SocketStateChanged => egress_processed += 1,
            }
        }
        let egress_may_remain = egress_processed == budget.egress_steps;

        let next_deadline = entry
            .interface
            .poll_at(smoltcp_now, &entry.sockets)
            .map(from_smoltcp_instant);
        let deadline_due = next_deadline.is_some_and(|deadline| deadline <= now);
        let immediate = ingress_may_remain || egress_may_remain || deadline_due;

        Ok(PumpOutcome {
            work_remaining: immediate || device.blocked_work(),
            recheck: if immediate {
                Recheck::Immediate
            } else {
                Recheck::Idle
            },
            next_deadline,
        })
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use anemone_net_api::{
        EthernetAddress, FrameCapabilities, FrameSizeError, LinkState, ReceiveOutcome, RxToken,
        TransmitOutcome, TxToken,
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
    fn queued_real_socket_reports_due_deadline_when_provider_is_blocked() {
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
        assert_eq!(outcome.recheck, Recheck::Immediate);
        assert_eq!(outcome.next_deadline, Some(Instant::ZERO));
        assert_eq!(provider.transmit_calls, 1);
    }
}
