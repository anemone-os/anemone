//! Aggregate UDP operations owned by the protocol Stack.

use anemone_net_api::{
    InterfaceId, Ipv4EgressSelection,
    udp::{
        UdpBindError, UdpBindRequest, UdpConnectError, UdpCreateError, UdpEndpointFacts,
        UdpEndpointId, UdpEndpointLimits, UdpErrorCause, UdpErrorRecord, UdpLocalBinding,
        UdpPeekOutcome, UdpPeer, UdpQueryError, UdpReceiveError, UdpReceiveOutcome, UdpRetireError,
        UdpSendError,
    },
};
use smoltcp::wire::{EthernetFrame, IpAddress, IpEndpoint, Ipv4Address};

use super::{ProtocolProgression, Stack};

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
        let mut endpoint = self.protocols.udp.prepare_endpoint(limits)?;
        for entry in &mut self.interfaces {
            endpoint.add_engine(entry.id, &mut entry.sockets);
        }
        if let Some(local) = &mut self.local {
            endpoint.add_engine(local.id, &mut local.sockets);
        }
        Ok(self.protocols.udp.publish_endpoint(endpoint))
    }

    /// Atomically selects/reserves a port, projects the binding into every
    /// private engine, and commits the endpoint's sole binding truth.
    pub fn bind_udp_endpoint(
        &mut self,
        id: UdpEndpointId,
        request: UdpBindRequest,
    ) -> Result<UdpLocalBinding, UdpBindError> {
        let binding = self.protocols.udp.prepare_binding(id, request)?;
        {
            let endpoint = self
                .protocols
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
        self.protocols.udp.commit_binding(id, binding);
        Ok(binding)
    }

    pub fn udp_endpoint_binding(
        &self,
        id: UdpEndpointId,
    ) -> Result<Option<UdpLocalBinding>, UdpQueryError> {
        self.protocols.udp.binding(id)
    }

    pub fn connect_udp_endpoint(
        &mut self,
        id: UdpEndpointId,
        selection: Ipv4EgressSelection,
        peer: UdpPeer,
    ) -> Result<(), UdpConnectError> {
        let api_source = selection.source();
        let source = Ipv4Address::from_octets(api_source.octets());
        let (source_supported, _) = self
            .interface_ipv4_and_mtu(selection.interface(), source)
            .ok_or(UdpConnectError::UnknownInterface)?;
        if !source_supported {
            return Err(UdpConnectError::UnsupportedSource);
        }
        let existing_binding = self
            .protocols
            .udp
            .binding(id)
            .map_err(|error| match error {
                UdpQueryError::UnknownEndpoint => UdpConnectError::UnknownEndpoint,
            })?;
        if let Some(binding) = existing_binding {
            if !binding.address().is_unspecified() && binding.address() != api_source {
                return Err(UdpConnectError::UnsupportedSource);
            }
        }
        let plan = self.protocols.udp.prepare_connect(id, api_source, peer)?;
        if let Some(binding) = plan.binding() {
            self.project_udp_binding(id, binding);
        }
        self.protocols.udp.commit_connect(id, plan);
        Ok(())
    }

    fn project_udp_binding(&mut self, id: UdpEndpointId, binding: UdpLocalBinding) {
        let endpoint = self
            .protocols
            .udp
            .endpoint(id)
            .expect("prepared UDP endpoint disappeared before bind projection");
        for entry in &mut self.interfaces {
            endpoint.bind_engine_projection(entry.id, &mut entry.sockets, binding);
        }
        if let Some(local) = &mut self.local {
            endpoint.bind_engine_projection(local.id, &mut local.sockets, binding);
        }
    }

    pub fn udp_endpoint_peer(&self, id: UdpEndpointId) -> Result<Option<UdpPeer>, UdpQueryError> {
        self.protocols.udp.peer(id)
    }

    pub fn resolve_udp_endpoint_destination(
        &self,
        id: UdpEndpointId,
        explicit: Option<UdpPeer>,
    ) -> Result<UdpPeer, UdpSendError> {
        self.protocols.udp.resolve_destination(id, explicit)
    }

    pub fn disconnect_udp_endpoint(&mut self, id: UdpEndpointId) -> Result<(), UdpQueryError> {
        self.protocols.udp.disconnect(id)
    }

    pub fn udp_endpoint_facts(&self, id: UdpEndpointId) -> Result<UdpEndpointFacts, UdpQueryError> {
        self.protocols
            .udp
            .endpoint(id)
            .map(|endpoint| endpoint.facts())
            .ok_or(UdpQueryError::UnknownEndpoint)
    }

    pub fn udp_receive_errors_enabled(&self, id: UdpEndpointId) -> Result<bool, UdpQueryError> {
        self.protocols.udp.receive_errors_enabled(id)
    }

    pub fn set_udp_receive_errors(
        &mut self,
        id: UdpEndpointId,
        enabled: bool,
    ) -> Result<(), UdpQueryError> {
        self.protocols.udp.set_receive_errors(id, enabled)
    }

    pub fn take_udp_pending_error(
        &mut self,
        id: UdpEndpointId,
    ) -> Result<Option<UdpErrorCause>, UdpQueryError> {
        self.protocols.udp.take_pending_error(id)
    }

    pub fn detach_udp_error(
        &mut self,
        id: UdpEndpointId,
    ) -> Result<Option<UdpErrorRecord>, UdpQueryError> {
        self.protocols.udp.detach_error(id)
    }

    pub fn send_udp_endpoint(
        &mut self,
        endpoint: UdpEndpointId,
        selection: Ipv4EgressSelection,
        peer: UdpPeer,
        payload: &[u8],
    ) -> Result<ProtocolProgression, UdpSendError> {
        let selected = selection.interface();
        let source = Ipv4Address::from_octets(selection.source().octets());
        let (source_supported, ip_mtu) = self
            .interface_ipv4_and_mtu(selected, source)
            .ok_or(UdpSendError::UnknownInterface)?;
        if !source_supported {
            return Err(UdpSendError::UnsupportedSource);
        }
        self.protocols.udp.queue_send(
            endpoint,
            Some(selected),
            source,
            IpEndpoint::new(
                IpAddress::Ipv4(Ipv4Address::from_octets(peer.address().octets())),
                peer.port(),
            ),
            payload,
            ip_mtu,
        )?;
        // Only the UDP owner can decide that queue admission committed new
        // egress work. The carrier identifies where the Stack must be reread;
        // it does not export queue or deadline truth.
        Ok(ProtocolProgression::committed(selected))
    }

    pub fn receive_udp_endpoint(
        &mut self,
        endpoint: UdpEndpointId,
    ) -> Result<UdpReceiveOutcome, UdpReceiveError> {
        let outcome = self.protocols.udp.receive(endpoint)?;
        // Detach restores one aggregate RX credit. Refill it while the Stack
        // owner is still active: a full aggregate queue intentionally leaves
        // the next datagram in the engine without a durable pump edge.
        for entry in &mut self.interfaces {
            self.protocols
                .udp
                .drain_ingress(entry.id, &mut entry.sockets);
        }
        if let Some(local) = &mut self.local {
            self.protocols
                .udp
                .drain_ingress(local.id, &mut local.sockets);
        }
        Ok(outcome)
    }

    pub fn peek_udp_endpoint(
        &mut self,
        endpoint: UdpEndpointId,
    ) -> Result<UdpPeekOutcome, UdpReceiveError> {
        self.protocols.udp.peek(endpoint)
    }

    pub fn retire_udp_endpoint(&mut self, id: UdpEndpointId) -> Result<(), UdpRetireError> {
        // Withdraw the aggregate owner before touching private engine objects.
        let endpoint = self.protocols.udp.withdraw(id)?;
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
