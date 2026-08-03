use alloc::{vec, vec::Vec};

use anemone_net_api::{
    InterfaceId, Ipv4Address,
    icmp_raw::{
        IcmpRawAssociation, IcmpRawCreateError, IcmpRawDropDiagnostics, IcmpRawEgressPolicy,
        IcmpRawEndpointConfig, IcmpRawEndpointFacts, IcmpRawEndpointId,
        IcmpRawEndpointInvalidation, IcmpRawEndpointLimits, IcmpRawMutationError,
        IcmpRawNamespacePolicy, IcmpRawQueryError, IcmpRawReceiveError, IcmpRawReceivedPacket,
        IcmpRawRetireError, IcmpRawSendError, IcmpRawTypeFilter,
    },
};
use smoltcp::{
    iface::{SocketHandle, SocketSet},
    socket::raw,
    wire::{IpProtocol, IpVersion, Ipv4Packet},
};

use super::{
    endpoint::Endpoint,
    packet::{IPV4_HEADER_LEN, IPV4_MAX_PACKET_BYTES, PendingPacket, build_packet},
};

#[derive(Clone, Copy)]
pub(crate) struct EngineResource {
    handle: SocketHandle,
}

impl EngineResource {
    pub(crate) const fn handle(self) -> SocketHandle {
        self.handle
    }
}

struct ActiveEgress {
    endpoint: IcmpRawEndpointId,
    interface: InterfaceId,
    packet_len: usize,
}

pub(crate) struct IcmpRawEndpoints {
    endpoints: Vec<Endpoint>,
    policy: IcmpRawNamespacePolicy,
    next_id: u64,
    next_identification: u16,
    next_egress_endpoint: usize,
    active_egress: Vec<ActiveEgress>,
    pending_invalidations: Vec<IcmpRawEndpointInvalidation>,
}

impl IcmpRawEndpoints {
    pub(crate) fn new(policy: IcmpRawNamespacePolicy) -> Self {
        assert!(policy.endpoint_capacity() > 0);
        Self {
            endpoints: Vec::new(),
            policy,
            next_id: 0,
            next_identification: 0,
            next_egress_endpoint: 0,
            active_egress: Vec::new(),
            pending_invalidations: Vec::new(),
        }
    }

    pub(crate) fn add_engine(&mut self, sockets: &mut SocketSet<'static>) -> EngineResource {
        // poll_ingress_single processes one frame and the Stack drains this
        // socket before polling another. One full-size IPv4 slot therefore
        // cannot become a shared fanout bottleneck.
        let socket = raw::Socket::new(
            Some(IpVersion::Ipv4),
            Some(IpProtocol::Icmp),
            raw::PacketBuffer::new(
                vec![raw::PacketMetadata::EMPTY],
                vec![0; IPV4_MAX_PACKET_BYTES],
            ),
            raw::PacketBuffer::new(
                vec![raw::PacketMetadata::EMPTY],
                vec![0; IPV4_MAX_PACKET_BYTES],
            ),
        );
        EngineResource {
            handle: sockets.add(socket),
        }
    }

    pub(crate) fn replace_engine(
        &mut self,
        engine: &mut EngineResource,
        sockets: &mut SocketSet<'static>,
    ) {
        sockets.remove(engine.handle);
        *engine = self.add_engine(sockets);
    }

    pub(crate) fn create(
        &mut self,
        limits: IcmpRawEndpointLimits,
    ) -> Result<IcmpRawEndpointId, IcmpRawCreateError> {
        if self.endpoints.len() >= self.policy.endpoint_capacity() {
            return Err(IcmpRawCreateError::EndpointCapacity);
        }
        let raw = self.next_id;
        self.next_id = raw
            .checked_add(1)
            .expect("boot-local ICMP raw endpoint identity exhausted");
        let endpoint = Endpoint::new(IcmpRawEndpointId::from_owner_raw(raw), limits);
        let id = endpoint.id;
        self.endpoints.push(endpoint);
        self.invalidate(id);
        Ok(id)
    }

    pub(crate) fn set_association(
        &mut self,
        id: IcmpRawEndpointId,
        association: IcmpRawAssociation,
    ) -> Result<(), IcmpRawMutationError> {
        if association
            .local()
            .is_some_and(|address| !address.is_unicast())
            || association
                .peer()
                .is_some_and(|address| !address.is_unicast())
        {
            return Err(IcmpRawMutationError::InvalidAssociation);
        }
        self.endpoint_mut(id)
            .ok_or(IcmpRawMutationError::UnknownEndpoint)?
            .association = association;
        Ok(())
    }

    pub(crate) fn set_filter(
        &mut self,
        id: IcmpRawEndpointId,
        filter: IcmpRawTypeFilter,
    ) -> Result<(), IcmpRawMutationError> {
        self.endpoint_mut(id)
            .ok_or(IcmpRawMutationError::UnknownEndpoint)?
            .filter = filter;
        Ok(())
    }

    pub(crate) fn config(
        &self,
        id: IcmpRawEndpointId,
    ) -> Result<IcmpRawEndpointConfig, IcmpRawQueryError> {
        self.endpoint(id)
            .map(Endpoint::config)
            .ok_or(IcmpRawQueryError::UnknownEndpoint)
    }

    pub(crate) fn facts(
        &self,
        id: IcmpRawEndpointId,
    ) -> Result<IcmpRawEndpointFacts, IcmpRawQueryError> {
        let active_packets = self
            .active_egress
            .iter()
            .filter(|active| active.endpoint == id)
            .count();
        self.endpoint(id)
            .map(|endpoint| endpoint.facts(active_packets))
            .ok_or(IcmpRawQueryError::UnknownEndpoint)
    }

    pub(crate) fn diagnostics(
        &self,
        id: IcmpRawEndpointId,
    ) -> Result<IcmpRawDropDiagnostics, IcmpRawQueryError> {
        self.endpoint(id)
            .map(Endpoint::diagnostics)
            .ok_or(IcmpRawQueryError::UnknownEndpoint)
    }

    pub(crate) fn fanout_admitted(&mut self, packet: &[u8]) {
        let ipv4 = Ipv4Packet::new_checked(packet)
            .expect("smoltcp admitted observer emitted an invalid IPv4 packet");
        assert!(!ipv4.more_frags() && ipv4.frag_offset() == 0);
        let source = Ipv4Address::new(ipv4.src_addr().octets());
        let destination = Ipv4Address::new(ipv4.dst_addr().octets());
        let icmp_type = ipv4.payload().first().copied();
        let mut invalidated = Vec::new();
        for endpoint in &mut self.endpoints {
            if endpoint.matches(source, destination, icmp_type) && endpoint.admit_rx(packet) {
                invalidated.push(endpoint.id);
            }
        }
        for endpoint in invalidated {
            self.invalidate(endpoint);
        }
    }

    pub(crate) fn drain_ingress(
        &mut self,
        engine: EngineResource,
        sockets: &mut SocketSet<'static>,
    ) {
        let socket = sockets.get_mut::<raw::Socket>(engine.handle);
        while socket.can_recv() {
            let packet = socket
                .recv()
                .expect("can_recv must imply one observer-owned packet");
            self.fanout_admitted(packet);
        }
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
            .filter(|active| active.interface == interface)
            .map(|active| active.endpoint)
            .next()
    }

    pub(crate) fn prepare_egress(
        &mut self,
        interface: InterfaceId,
        engine: EngineResource,
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
        engine: EngineResource,
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

    pub(crate) fn receive(
        &mut self,
        id: IcmpRawEndpointId,
        peek: bool,
    ) -> Result<IcmpRawReceivedPacket, IcmpRawReceiveError> {
        let endpoint = self
            .endpoint_mut(id)
            .ok_or(IcmpRawReceiveError::UnknownEndpoint)?;
        let packet = if peek {
            endpoint
                .received
                .front()
                .ok_or(IcmpRawReceiveError::WouldBlock)?
                .clone()
        } else {
            let packet = endpoint
                .received
                .pop_front()
                .ok_or(IcmpRawReceiveError::WouldBlock)?;
            endpoint.rx_bytes = endpoint
                .rx_bytes
                .checked_sub(packet.len())
                .expect("detached ICMP raw packet bytes lost owner accounting");
            self.invalidate(id);
            packet
        };
        Ok(IcmpRawReceivedPacket::from_owner_detach(packet))
    }

    pub(crate) fn retire(
        &mut self,
        id: IcmpRawEndpointId,
    ) -> Result<Vec<InterfaceId>, IcmpRawRetireError> {
        let index = self
            .endpoints
            .iter()
            .position(|endpoint| endpoint.id == id)
            .ok_or(IcmpRawRetireError::UnknownEndpoint)?;
        self.endpoints.remove(index);
        let reset_engines = self
            .active_egress
            .iter()
            .filter(|active| active.endpoint == id)
            .map(|active| active.interface)
            .collect::<Vec<_>>();
        self.active_egress.retain(|active| active.endpoint != id);
        if self.next_egress_endpoint > self.endpoints.len() {
            self.next_egress_endpoint = 0;
        }
        self.invalidate(id);
        Ok(reset_engines)
    }

    pub(crate) fn take_invalidations(&mut self) -> Vec<IcmpRawEndpointInvalidation> {
        core::mem::take(&mut self.pending_invalidations)
    }

    fn endpoint(&self, id: IcmpRawEndpointId) -> Option<&Endpoint> {
        self.endpoints.iter().find(|endpoint| endpoint.id == id)
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

    fn endpoint_mut(&mut self, id: IcmpRawEndpointId) -> Option<&mut Endpoint> {
        self.endpoints.iter_mut().find(|endpoint| endpoint.id == id)
    }

    fn invalidate(&mut self, id: IcmpRawEndpointId) {
        let endpoints = &self.endpoints;
        self.pending_invalidations.retain(|pending| {
            pending.endpoint() == id
                || endpoints
                    .iter()
                    .any(|endpoint| endpoint.id == pending.endpoint())
        });
        if self
            .pending_invalidations
            .iter()
            .any(|pending| pending.endpoint() == id)
        {
            return;
        }
        self.pending_invalidations
            .push(IcmpRawEndpointInvalidation::from_owner_transition(id));
    }
}
