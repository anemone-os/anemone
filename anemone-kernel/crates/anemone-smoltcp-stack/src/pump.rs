#[cfg(feature = "kunit-probe")]
use alloc::vec;
use alloc::vec::Vec;

use anemone_net_api::{EthernetAddress, FrameProvider, Instant, InterfaceId, PumpOutcome, Recheck};
use smoltcp::{
    iface::{Config, Interface, PollIngressSingleResult, PollResult, SocketSet},
    wire::{EthernetAddress as SmoltcpEthernetAddress, HardwareAddress},
};

#[cfg(feature = "kunit-probe")]
use smoltcp::{
    iface::SocketHandle,
    phy::ChecksumCapabilities,
    socket::icmp,
    wire::{Icmpv4Packet, Icmpv4Repr, IpAddress, IpCidr, Ipv4Address},
};

use crate::adapter::{FrameDevice, from_smoltcp_instant, to_smoltcp_instant};

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PumpError {
    UnknownInterface(InterfaceId),
}

struct InterfaceEntry {
    id: InterfaceId,
    // Stable attach-time snapshot required by smoltcp. The provider remains
    // the truth source; every pump asserts that this snapshot is not stale.
    frame_capacity: usize,
    interface: Interface,
    sockets: SocketSet<'static>,
    #[cfg(feature = "kunit-probe")]
    kunit_probe: Option<KunitIcmpProbe>,
}

#[cfg(feature = "kunit-probe")]
struct KunitIcmpProbe {
    socket: SocketHandle,
    remote: Ipv4Address,
    ident: u16,
    sequence: u16,
    completed: bool,
}

/// Owns the private smoltcp interface resources and their opaque ID mapping.
///
/// `&mut Stack` is the unique pump capability. The kernel wiring owner may put
/// the stack behind its chosen synchronization primitive, but admission,
/// contention, and requeue policy stay outside this protocol-state owner.
#[derive(Default)]
pub struct Stack {
    interfaces: Vec<InterfaceEntry>,
    next_interface_id: u32,
}

impl Stack {
    pub const fn new() -> Self {
        Self {
            interfaces: Vec::new(),
            next_interface_id: 0,
        }
    }

    pub fn add_interface<P: FrameProvider>(
        &mut self,
        provider: &mut P,
        ethernet_address: EthernetAddress,
        now: Instant,
    ) -> InterfaceId {
        let raw_id = self.next_interface_id;
        self.next_interface_id = raw_id
            .checked_add(1)
            .expect("InterfaceId namespace exhausted");
        let id = InterfaceId::from_index(raw_id);
        let frame_capacity = provider.capabilities().max_frame_len;
        let mut device = FrameDevice::new(provider);
        let hardware_address = HardwareAddress::Ethernet(SmoltcpEthernetAddress::from_bytes(
            &ethernet_address.octets(),
        ));
        let interface = Interface::new(
            Config::new(hardware_address),
            &mut device,
            to_smoltcp_instant(now),
        );

        self.interfaces.push(InterfaceEntry {
            id,
            frame_capacity,
            interface,
            sockets: SocketSet::new(Vec::new()),
            #[cfg(feature = "kunit-probe")]
            kunit_probe: None,
        });
        id
    }

    /// Withdraws a transaction-local mapping before active publication.
    ///
    /// Interface IDs remain monotonic and are not reused. Runtime detach is not
    /// part of R0; the kernel attach authority only uses this for rollback when
    /// worker/wake/time preparation fails.
    pub fn remove_interface(&mut self, id: InterfaceId) -> Result<(), PumpError> {
        let Some(index) = self.interfaces.iter().position(|entry| entry.id == id) else {
            return Err(PumpError::UnknownInterface(id));
        };
        self.interfaces.remove(index);
        Ok(())
    }

    pub fn pump<P: FrameProvider>(
        &mut self,
        id: InterfaceId,
        provider: &mut P,
        now: Instant,
        budget: PumpBudget,
    ) -> Result<PumpOutcome, PumpError> {
        let Some(entry) = self.interfaces.iter_mut().find(|entry| entry.id == id) else {
            return Err(PumpError::UnknownInterface(id));
        };

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

    /// Installs an IPv4 address solely for the deterministic host fixture.
    ///
    /// `host-test` is absent from the kernel dependency, so this control cannot
    /// become a production address API. Remove it when the fixture can stay
    /// crate-private or an accepted control-plane owner replaces it.
    #[cfg(feature = "host-test")]
    pub fn configure_ipv4_for_host_validation(
        &mut self,
        id: InterfaceId,
        address: [u8; 4],
        prefix_len: u8,
    ) -> Result<(), PumpError> {
        use smoltcp::wire::{IpAddress, IpCidr, Ipv4Address};

        let Some(entry) = self.interfaces.iter_mut().find(|entry| entry.id == id) else {
            return Err(PumpError::UnknownInterface(id));
        };
        let cidr = IpCidr::new(
            IpAddress::Ipv4(Ipv4Address::from_octets(address)),
            prefix_len,
        );
        entry.interface.update_ip_addrs(|addresses| {
            addresses.clear();
            assert!(addresses.push(cidr).is_ok());
        });
        Ok(())
    }

    /// Installs and queues the single RV64 Stage 1 ICMP validation probe.
    ///
    /// This feature is absent from production kernels. The socket handle,
    /// address, and endpoint remain inside the stack owner; the kernel only
    /// observes the eventual boolean completion fact.
    #[cfg(feature = "kunit-probe")]
    pub fn start_kunit_icmp_echo(
        &mut self,
        id: InterfaceId,
        local: [u8; 4],
        prefix_len: u8,
        remote: [u8; 4],
    ) -> Result<(), PumpError> {
        const IDENT: u16 = 0x4e46;
        const SEQUENCE: u16 = 1;
        const PAYLOAD: &[u8] = b"anemone-frame-stage1";

        let Some(entry) = self.interfaces.iter_mut().find(|entry| entry.id == id) else {
            return Err(PumpError::UnknownInterface(id));
        };
        assert!(
            entry.kunit_probe.is_none(),
            "KUnit ICMP probe started more than once"
        );

        let local = Ipv4Address::from_octets(local);
        let remote = Ipv4Address::from_octets(remote);
        entry.interface.update_ip_addrs(|addresses| {
            addresses.clear();
            addresses
                .push(IpCidr::new(IpAddress::Ipv4(local), prefix_len))
                .expect("KUnit IPv4 address slot must be available");
        });

        let rx = icmp::PacketBuffer::new(vec![icmp::PacketMetadata::EMPTY], vec![0; 128]);
        let tx = icmp::PacketBuffer::new(vec![icmp::PacketMetadata::EMPTY], vec![0; 128]);
        let mut socket = icmp::Socket::new(rx, tx);
        socket
            .bind(icmp::Endpoint::Ident(IDENT))
            .expect("fresh KUnit ICMP socket must bind");
        let repr = Icmpv4Repr::EchoRequest {
            ident: IDENT,
            seq_no: SEQUENCE,
            data: PAYLOAD,
        };
        let payload = socket
            .send(repr.buffer_len(), IpAddress::Ipv4(remote))
            .expect("fresh KUnit ICMP socket must have TX capacity");
        repr.emit(
            &mut Icmpv4Packet::new_unchecked(payload),
            &ChecksumCapabilities::default(),
        );
        let socket = entry.sockets.add(socket);
        entry.kunit_probe = Some(KunitIcmpProbe {
            socket,
            remote,
            ident: IDENT,
            sequence: SEQUENCE,
            completed: false,
        });
        Ok(())
    }

    /// Reaps the validation-only echo reply without exposing a smoltcp handle.
    #[cfg(feature = "kunit-probe")]
    pub fn kunit_icmp_echo_completed(&mut self, id: InterfaceId) -> Result<bool, PumpError> {
        let Some(entry) = self.interfaces.iter_mut().find(|entry| entry.id == id) else {
            return Err(PumpError::UnknownInterface(id));
        };
        let Some(probe) = entry.kunit_probe.as_mut() else {
            return Ok(false);
        };
        if probe.completed {
            return Ok(true);
        }

        let socket = entry.sockets.get_mut::<icmp::Socket>(probe.socket);
        if !socket.can_recv() {
            return Ok(false);
        }
        let (payload, remote) = socket
            .recv()
            .expect("KUnit ICMP socket reported receive readiness without a packet");
        assert_eq!(remote, IpAddress::Ipv4(probe.remote));
        let packet = Icmpv4Packet::new_checked(payload)
            .expect("KUnit ICMP response must contain a valid packet");
        let repr = Icmpv4Repr::parse(&packet, &ChecksumCapabilities::default())
            .expect("KUnit ICMP response checksum must be valid");
        assert!(
            matches!(
                repr,
                Icmpv4Repr::EchoReply {
                    ident,
                    seq_no,
                    ..
                } if ident == probe.ident && seq_no == probe.sequence
            ),
            "KUnit ICMP socket received an unexpected response: {repr:?}"
        );
        probe.completed = true;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use anemone_net_api::{
        FrameCapabilities, FrameSizeError, LinkState, ReceiveOutcome, RxToken, TransmitOutcome,
        TxToken,
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

        fn capabilities(&self) -> anemone_net_api::FrameCapabilities {
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
        {
            let entry = stack
                .interfaces
                .iter_mut()
                .find(|entry| entry.id == interface)
                .unwrap();
            entry.sockets.add(raw_socket);
        }

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
