use alloc::{vec, vec::Vec};

use anemone_net_api::InterfaceId;
use smoltcp::{
    iface::SocketHandle,
    phy::ChecksumCapabilities,
    socket::icmp,
    wire::{Icmpv4Packet, Icmpv4Repr, IpAddress, IpCidr, Ipv4Address},
};

use crate::stack::{PumpError, Stack};

pub(crate) struct IcmpEchoProbe {
    socket: SocketHandle,
    remote: Ipv4Address,
    ident: u16,
    completed_sequences: Vec<bool>,
    completed_count: usize,
}

impl Stack {
    /// Installs and queues the validation-only ICMP echo probe.
    ///
    /// This feature is absent from production kernels. The socket handle,
    /// address, and endpoint remain inside the stack owner; the caller only
    /// observes the eventual boolean completion fact. Remove this seam when a
    /// production control-plane owner can drive the same RV64 path.
    pub fn start_icmp_echo_probe(
        &mut self,
        id: InterfaceId,
        local: [u8; 4],
        prefix_len: u8,
        remote: [u8; 4],
        sequence_count: usize,
    ) -> Result<(), PumpError> {
        const IDENT: u16 = 0x4e46;
        const PAYLOAD: &[u8] = b"anemone-frame-stage1";

        assert!(sequence_count > 0, "ICMP validation burst must be non-zero");
        assert!(
            sequence_count <= usize::from(u16::MAX) + 1,
            "ICMP validation burst exceeds the sequence namespace"
        );

        let entry = self.interface_mut(id)?;
        assert!(
            entry.validation_probe.is_none(),
            "ICMP validation probe started more than once"
        );

        let local = Ipv4Address::from_octets(local);
        let remote = Ipv4Address::from_octets(remote);
        entry.interface.update_ip_addrs(|addresses| {
            addresses.clear();
            addresses
                .push(IpCidr::new(IpAddress::Ipv4(local), prefix_len))
                .expect("validation IPv4 address slot must be available");
        });

        let packet_len = Icmpv4Repr::EchoRequest {
            ident: IDENT,
            seq_no: 0,
            data: PAYLOAD,
        }
        .buffer_len();
        let storage_len = packet_len
            .checked_mul(sequence_count)
            .expect("ICMP validation storage capacity overflow");
        let rx = icmp::PacketBuffer::new(
            vec![icmp::PacketMetadata::EMPTY; sequence_count],
            vec![0; storage_len],
        );
        let tx = icmp::PacketBuffer::new(
            vec![icmp::PacketMetadata::EMPTY; sequence_count],
            vec![0; storage_len],
        );
        let mut socket = icmp::Socket::new(rx, tx);
        socket
            .bind(icmp::Endpoint::Ident(IDENT))
            .expect("fresh validation ICMP socket must bind");
        for sequence in 0..sequence_count {
            let sequence = u16::try_from(sequence).expect("validated ICMP sequence must fit");
            let repr = Icmpv4Repr::EchoRequest {
                ident: IDENT,
                seq_no: sequence,
                data: PAYLOAD,
            };
            let payload = socket
                .send(repr.buffer_len(), IpAddress::Ipv4(remote))
                .expect("sized validation ICMP socket must accept the entire burst");
            repr.emit(
                &mut Icmpv4Packet::new_unchecked(payload),
                &ChecksumCapabilities::default(),
            );
        }
        let socket = entry.sockets.add(socket);
        entry.validation_probe = Some(IcmpEchoProbe {
            socket,
            remote,
            ident: IDENT,
            completed_sequences: vec![false; sequence_count],
            completed_count: 0,
        });
        Ok(())
    }

    /// Reaps the validation-only echo reply without exposing a smoltcp handle.
    pub fn icmp_echo_probe_completed(&mut self, id: InterfaceId) -> Result<bool, PumpError> {
        let entry = self.interface_mut(id)?;
        let Some(probe) = entry.validation_probe.as_mut() else {
            return Ok(false);
        };
        if probe.completed_count == probe.completed_sequences.len() {
            return Ok(true);
        }

        let socket = entry.sockets.get_mut::<icmp::Socket>(probe.socket);
        while socket.can_recv() {
            let (payload, remote) = socket
                .recv()
                .expect("validation ICMP socket reported receive readiness without a packet");
            assert_eq!(remote, IpAddress::Ipv4(probe.remote));
            let packet = Icmpv4Packet::new_checked(payload)
                .expect("validation ICMP response must contain a valid packet");
            let repr = Icmpv4Repr::parse(&packet, &ChecksumCapabilities::default())
                .expect("validation ICMP response checksum must be valid");
            let Icmpv4Repr::EchoReply { ident, seq_no, .. } = repr else {
                panic!("validation ICMP socket received an unexpected response: {repr:?}")
            };
            assert_eq!(ident, probe.ident);
            let completed = probe
                .completed_sequences
                .get_mut(usize::from(seq_no))
                .expect("validation ICMP reply sequence was outside the requested burst");
            assert!(!*completed, "validation ICMP reply completed twice");
            *completed = true;
            probe.completed_count += 1;
        }
        Ok(probe.completed_count == probe.completed_sequences.len())
    }
}
