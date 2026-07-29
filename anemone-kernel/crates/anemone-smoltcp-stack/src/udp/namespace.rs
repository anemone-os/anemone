use alloc::vec::Vec;

use anemone_net_api::InterfaceId;
use smoltcp::iface::SocketSet;

use super::{Endpoint, EndpointCreateError, EndpointId, RetireError};

/// Owns the provisional domain-wide Endpoint namespace and aggregate state.
///
/// Engine sockets remain inside their `SocketSet`; the mapping here is the
/// only authority that can bind, select, drain, or retire them as one Endpoint.
pub(crate) struct UdpEndpoints {
    pub(super) endpoints: Vec<Endpoint>,
    next_id: u32,
    pub(super) next_egress_endpoint: usize,
}

impl UdpEndpoints {
    pub(crate) const fn new() -> Self {
        Self {
            endpoints: Vec::new(),
            next_id: 0,
            next_egress_endpoint: 0,
        }
    }

    pub(crate) fn prepare_endpoint(
        &mut self,
        port: u16,
        receive_packet_capacity: usize,
        engine_payload_capacity: usize,
    ) -> Result<Endpoint, EndpointCreateError> {
        if port == 0 {
            return Err(EndpointCreateError::InvalidPort);
        }
        if self.endpoints.iter().any(|endpoint| endpoint.port == port) {
            return Err(EndpointCreateError::PortInUse);
        }
        let raw = self.next_id;
        self.next_id = raw.checked_add(1).expect("EndpointId namespace exhausted");
        Ok(Endpoint::new(
            EndpointId(raw),
            port,
            receive_packet_capacity,
            engine_payload_capacity,
        ))
    }

    pub(crate) fn publish_endpoint(&mut self, endpoint: Endpoint) -> EndpointId {
        let id = endpoint.id;
        self.endpoints.push(endpoint);
        id
    }

    pub(crate) fn add_interface(
        &mut self,
        interface: InterfaceId,
        sockets: &mut SocketSet<'static>,
    ) {
        for endpoint in &mut self.endpoints {
            endpoint.add_engine(interface, sockets);
        }
    }

    pub(crate) fn remove_interface(
        &mut self,
        interface: InterfaceId,
        sockets: &mut SocketSet<'static>,
    ) {
        // Withdraw the owner mapping before removing the private engine object.
        for endpoint in &mut self.endpoints {
            endpoint.remove_engine(interface, sockets);
        }
    }

    pub(crate) fn withdraw(&mut self, id: EndpointId) -> Result<Endpoint, RetireError> {
        let index = self
            .endpoints
            .iter()
            .position(|endpoint| endpoint.id == id)
            .ok_or(RetireError::UnknownEndpoint)?;
        let endpoint = self.endpoints.remove(index);
        if self.next_egress_endpoint > self.endpoints.len() {
            self.next_egress_endpoint = 0;
        }
        Ok(endpoint)
    }

    pub(crate) fn endpoint(&self, id: EndpointId) -> Option<&Endpoint> {
        self.endpoints.iter().find(|endpoint| endpoint.id == id)
    }
}

impl Default for UdpEndpoints {
    fn default() -> Self {
        Self::new()
    }
}
