//! ICMP raw TX admission and private smoltcp egress handoff.

use alloc::vec;

use anemone_net_api::{
    InterfaceId, Ipv4Address,
    icmp_raw::{IcmpRawEgressPolicy, IcmpRawEndpointId, IcmpRawSendError},
};
use smoltcp::{
    iface::{SocketHandle, SocketSet},
    socket::raw,
    wire::{IpProtocol, IpVersion},
};

use super::{
    IcmpRawEndpoints,
    packet::{IPV4_HEADER_LEN, IPV4_MAX_PACKET_BYTES, PendingPacket, build_packet},
};

#[derive(Clone, Copy)]
pub(crate) struct EgressResource {
    handle: SocketHandle,
}

impl EgressResource {
    pub(crate) const fn handle(self) -> SocketHandle {
        self.handle
    }
}

pub(super) struct ActiveEgress {
    pub(super) endpoint: IcmpRawEndpointId,
    pub(super) interface: InterfaceId,
    pub(super) packet_len: usize,
}

impl IcmpRawEndpoints {
    pub(crate) fn add_egress_engine(&mut self, sockets: &mut SocketSet<'static>) -> EgressResource {
        let socket = raw::Socket::new_egress_only(
            Some(IpVersion::Ipv4),
            Some(IpProtocol::Icmp),
            raw::PacketBuffer::new(vec![], vec![]),
            raw::PacketBuffer::new(
                vec![raw::PacketMetadata::EMPTY],
                vec![0; IPV4_MAX_PACKET_BYTES],
            ),
        );
        EgressResource {
            handle: sockets.add(socket),
        }
    }

    pub(crate) fn replace_egress_engine(
        &mut self,
        engine: &mut EgressResource,
        sockets: &mut SocketSet<'static>,
    ) {
        sockets.remove(engine.handle);
        *engine = self.add_egress_engine(sockets);
    }

    pub(crate) fn queue_send(
        &mut self,
        id: IcmpRawEndpointId,
        interface: InterfaceId,
        source: Ipv4Address,
        destination: Ipv4Address,
        policy: IcmpRawEgressPolicy,
        message: &[u8],
        ip_mtu: usize,
    ) -> Result<(), IcmpRawSendError> {
        if !destination.is_unicast() {
            return Err(IcmpRawSendError::InvalidDestination);
        }
        let interface_maximum = ip_mtu
            .min(IPV4_MAX_PACKET_BYTES)
            .saturating_sub(IPV4_HEADER_LEN);
        if message.len() > interface_maximum {
            return Err(IcmpRawSendError::MessageTooLong {
                maximum: interface_maximum,
            });
        }
        let endpoint_maximum = self
            .endpoint(id)
            .ok_or(IcmpRawSendError::UnknownEndpoint)?
            .limits
            .tx_byte_capacity()
            .saturating_sub(IPV4_HEADER_LEN);
        let maximum = interface_maximum.min(endpoint_maximum);
        if message.len() > maximum {
            return Err(IcmpRawSendError::MessageTooLong { maximum });
        }
        let total_len = IPV4_HEADER_LEN + message.len();
        let active_packets = self
            .active_egress
            .iter()
            .filter(|active| active.endpoint == id)
            .count();
        let endpoint = self
            .endpoint_mut(id)
            .ok_or(IcmpRawSendError::UnknownEndpoint)?;
        let tx_packets = endpoint.pending_tx.len() + active_packets;
        let fits_bytes = endpoint
            .tx_bytes
            .checked_add(total_len)
            .is_some_and(|bytes| bytes <= endpoint.limits.tx_byte_capacity());
        if tx_packets >= endpoint.limits.tx_packet_capacity() || !fits_bytes {
            return Err(IcmpRawSendError::WouldBlock);
        }

        let identification = self.next_identification;
        self.next_identification = identification.wrapping_add(1);
        let packet = build_packet(identification, source, destination, policy, message);
        let endpoint = self
            .endpoint_mut(id)
            .expect("validated ICMP raw endpoint disappeared before commit");
        endpoint.tx_bytes += packet.len();
        endpoint.pending_tx.push_back(PendingPacket {
            interface,
            bytes: packet,
        });
        self.invalidate(id);
        Ok(())
    }

    pub(crate) fn active_egress(&self, interface: InterfaceId) -> Option<IcmpRawEndpointId> {
        self.active_egress
            .iter()
            .find(|active| active.interface == interface)
            .map(|active| active.endpoint)
    }

    pub(crate) fn prepare_egress(
        &mut self,
        interface: InterfaceId,
        engine: EgressResource,
        sockets: &mut SocketSet<'static>,
    ) -> Option<IcmpRawEndpointId> {
        if let Some(endpoint) = self.active_egress(interface) {
            return Some(endpoint);
        }
        if self.endpoints.is_empty() {
            return None;
        }
        let len = self.endpoints.len();
        for offset in 0..len {
            let index = (self.next_egress_endpoint + offset) % len;
            let endpoint = &mut self.endpoints[index];
            let selected = endpoint
                .pending_tx
                .iter()
                .position(|packet| packet.interface == interface);
            let Some(selected) = selected else {
                continue;
            };
            // Preserve FIFO within one interface while allowing an unrelated
            // provider to progress the same Endpoint's later packet.
            let packet = endpoint.pending_tx.remove(selected).unwrap();
            sockets
                .get_mut::<raw::Socket>(engine.handle)
                .send_slice(&packet.bytes)
                .expect("one-at-a-time raw engine admission must have capacity");
            self.active_egress.push(ActiveEgress {
                endpoint: endpoint.id,
                interface,
                packet_len: packet.bytes.len(),
            });
            self.next_egress_endpoint = (index + 1) % len;
            return Some(endpoint.id);
        }
        None
    }

    pub(crate) fn complete_egress(
        &mut self,
        interface: InterfaceId,
        engine: EgressResource,
        sockets: &SocketSet<'static>,
    ) -> bool {
        let Some(index) = self
            .active_egress
            .iter()
            .position(|active| active.interface == interface)
        else {
            return self.egress_pending(interface);
        };
        if sockets.get::<raw::Socket>(engine.handle).send_queue() != 0 {
            return true;
        }
        let active = self.active_egress.remove(index);
        let endpoint = self
            .endpoint_mut(active.endpoint)
            .expect("active ICMP raw egress owner disappeared during pump");
        endpoint.tx_bytes = endpoint
            .tx_bytes
            .checked_sub(active.packet_len)
            .expect("active ICMP raw packet bytes lost owner accounting");
        self.invalidate(active.endpoint);
        self.egress_pending(interface)
    }

    fn egress_pending(&self, interface: InterfaceId) -> bool {
        self.active_egress(interface).is_some()
            || self.endpoints.iter().any(|endpoint| {
                endpoint
                    .pending_tx
                    .iter()
                    .any(|packet| packet.interface == interface)
            })
    }

    pub(crate) fn assert_interface_idle(&self, interface: InterfaceId) {
        assert!(
            self.active_egress(interface).is_none()
                && self.endpoints.iter().all(|endpoint| {
                    endpoint
                        .pending_tx
                        .iter()
                        .all(|packet| packet.interface != interface)
                }),
            "unpublished interface still owns ICMP raw egress"
        );
    }
}
