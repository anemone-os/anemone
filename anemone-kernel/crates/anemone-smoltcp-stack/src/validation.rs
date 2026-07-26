use alloc::vec;

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
    sequence: u16,
    completed: bool,
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
    ) -> Result<(), PumpError> {
        const IDENT: u16 = 0x4e46;
        const SEQUENCE: u16 = 1;
        const PAYLOAD: &[u8] = b"anemone-frame-stage1";

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

        let rx = icmp::PacketBuffer::new(vec![icmp::PacketMetadata::EMPTY], vec![0; 128]);
        let tx = icmp::PacketBuffer::new(vec![icmp::PacketMetadata::EMPTY], vec![0; 128]);
        let mut socket = icmp::Socket::new(rx, tx);
        socket
            .bind(icmp::Endpoint::Ident(IDENT))
            .expect("fresh validation ICMP socket must bind");
        let repr = Icmpv4Repr::EchoRequest {
            ident: IDENT,
            seq_no: SEQUENCE,
            data: PAYLOAD,
        };
        let payload = socket
            .send(repr.buffer_len(), IpAddress::Ipv4(remote))
            .expect("fresh validation ICMP socket must have TX capacity");
        repr.emit(
            &mut Icmpv4Packet::new_unchecked(payload),
            &ChecksumCapabilities::default(),
        );
        let socket = entry.sockets.add(socket);
        entry.validation_probe = Some(IcmpEchoProbe {
            socket,
            remote,
            ident: IDENT,
            sequence: SEQUENCE,
            completed: false,
        });
        Ok(())
    }

    /// Reaps the validation-only echo reply without exposing a smoltcp handle.
    pub fn icmp_echo_probe_completed(&mut self, id: InterfaceId) -> Result<bool, PumpError> {
        let entry = self.interface_mut(id)?;
        let Some(probe) = entry.validation_probe.as_mut() else {
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
            .expect("validation ICMP socket reported receive readiness without a packet");
        assert_eq!(remote, IpAddress::Ipv4(probe.remote));
        let packet = Icmpv4Packet::new_checked(payload)
            .expect("validation ICMP response must contain a valid packet");
        let repr = Icmpv4Repr::parse(&packet, &ChecksumCapabilities::default())
            .expect("validation ICMP response checksum must be valid");
        assert!(
            matches!(
                repr,
                Icmpv4Repr::EchoReply {
                    ident,
                    seq_no,
                    ..
                } if ident == probe.ident && seq_no == probe.sequence
            ),
            "validation ICMP socket received an unexpected response: {repr:?}"
        );
        probe.completed = true;
        Ok(true)
    }
}
