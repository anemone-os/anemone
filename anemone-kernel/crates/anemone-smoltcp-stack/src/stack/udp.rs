//! Aggregate UDP operations owned by the protocol Stack.

use alloc::vec::Vec;

use anemone_net_api::{
    InterfaceId, Ipv4Address as ApiIpv4Address,
    udp::{
        UdpBindError, UdpBindRequest, UdpCreateError, UdpEgressSelection, UdpEndpointFacts,
        UdpEndpointId, UdpEndpointInvalidation, UdpEndpointLimits, UdpLocalBinding, UdpPeer,
        UdpQueryError, UdpReceiveError, UdpReceivedDatagram, UdpRetireError, UdpSendError,
    },
};
use smoltcp::wire::{EthernetFrame, IpAddress, IpEndpoint, Ipv4Address};

use super::Stack;

impl Stack {
    #[allow(dead_code)]
    pub(super) fn interface_ipv4_and_mtu(
        &self,
        id: InterfaceId,
        source: Ipv4Address,
    ) -> Option<(bool, usize)> {
        if let Some(entry) = self.interfaces.iter().find(|entry| entry.id == id) {
            let ip_mtu = entry
                .frame_capacity
                .checked_sub(EthernetFrame::<&[u8]>::header_len())?;
            return Some((entry.interface.has_ip_addr(source), ip_mtu));
        }
        self.local
            .as_ref()
            .filter(|local| local.id == id)
            .map(|local| (local.interface.has_ip_addr(source), local.ip_mtu()))
    }

    pub fn create_udp_endpoint(
        &mut self,
        limits: UdpEndpointLimits,
    ) -> Result<UdpEndpointId, UdpCreateError> {
        let mut endpoint = self.udp.prepare_endpoint(limits)?;
        for entry in &mut self.interfaces {
            endpoint.add_engine(entry.id, &mut entry.sockets);
        }
        if let Some(local) = &mut self.local {
            endpoint.add_engine(local.id, &mut local.sockets);
        }
        Ok(self.udp.publish_endpoint(endpoint))
    }

    /// Atomically selects/reserves a port, projects the binding into every
    /// private engine, and commits the endpoint's sole binding truth.
    pub fn bind_udp_endpoint(
        &mut self,
        id: UdpEndpointId,
        request: UdpBindRequest,
    ) -> Result<UdpLocalBinding, UdpBindError> {
        let binding = self.udp.prepare_binding(id, request)?;
        {
            let endpoint = self
                .udp
                .endpoint(id)
                .expect("prepared UDP endpoint disappeared before projection");
            for entry in &mut self.interfaces {
                endpoint.bind_engine_projection(entry.id, &mut entry.sockets, binding);
            }
            if let Some(local) = &mut self.local {
                endpoint.bind_engine_projection(local.id, &mut local.sockets, binding);
            }
        }
        self.udp.commit_binding(id, binding);
        Ok(binding)
    }

    pub fn udp_endpoint_binding(
        &self,
        id: UdpEndpointId,
    ) -> Result<Option<UdpLocalBinding>, UdpQueryError> {
        self.udp.binding(id)
    }

    pub fn udp_endpoint_facts(&self, id: UdpEndpointId) -> Result<UdpEndpointFacts, UdpQueryError> {
        self.udp
            .endpoint(id)
            .map(|endpoint| endpoint.facts())
            .ok_or(UdpQueryError::UnknownEndpoint)
    }

    /// Drain coalesced Endpoint recheck hints after the caller's exclusive
    /// Stack window ends. The returned tokens contain no readiness payload.
    pub fn take_udp_endpoint_invalidations(&mut self) -> Vec<UdpEndpointInvalidation> {
        self.udp.take_invalidations()
    }

    pub fn send_udp_endpoint(
        &mut self,
        endpoint: UdpEndpointId,
        selection: UdpEgressSelection,
        peer: UdpPeer,
        payload: &[u8],
    ) -> Result<(), UdpSendError> {
        let selected = selection.interface();
        let source = Ipv4Address::from_octets(selection.source().octets());
        let (source_supported, ip_mtu) = self
            .interface_ipv4_and_mtu(selected, source)
            .ok_or(UdpSendError::UnknownInterface)?;
        if !source_supported {
            return Err(UdpSendError::UnsupportedSource);
        }
        self.udp.queue_send(
            endpoint,
            Some(selected),
            source,
            IpEndpoint::new(
                IpAddress::Ipv4(Ipv4Address::from_octets(peer.address().octets())),
                peer.port(),
            ),
            payload,
            ip_mtu,
        )
    }

    pub fn receive_udp_endpoint(
        &mut self,
        endpoint: UdpEndpointId,
    ) -> Result<UdpReceivedDatagram, UdpReceiveError> {
        let datagram = self.udp.receive(endpoint)?;
        // Detach restores one aggregate RX credit. Refill it while the Stack
        // owner is still active: a full aggregate queue intentionally leaves
        // the next datagram in the engine without a durable pump edge.
        for entry in &mut self.interfaces {
            self.udp.drain_ingress(entry.id, &mut entry.sockets);
        }
        if let Some(local) = &mut self.local {
            self.udp.drain_ingress(local.id, &mut local.sockets);
        }
        let IpAddress::Ipv4(source) = datagram.source.addr;
        Ok(UdpReceivedDatagram::from_owner_detach(
            datagram.payload,
            UdpPeer::new(ApiIpv4Address::new(source.octets()), datagram.source.port),
        ))
    }

    pub fn retire_udp_endpoint(&mut self, id: UdpEndpointId) -> Result<(), UdpRetireError> {
        // Withdraw the aggregate owner before touching private engine objects.
        let endpoint = self.udp.withdraw(id)?;
        for engine in endpoint.engines() {
            if let Some(entry) = self
                .interfaces
                .iter_mut()
                .find(|entry| entry.id == engine.interface())
            {
                entry.sockets.remove(engine.handle());
                continue;
            }
            if let Some(local) = self
                .local
                .as_mut()
                .filter(|local| local.id == engine.interface())
            {
                local.sockets.remove(engine.handle());
            }
        }
        if let Some(local) = &mut self.local {
            local.link.remove_udp_owner(endpoint.id());
        }
        Ok(())
    }
}
