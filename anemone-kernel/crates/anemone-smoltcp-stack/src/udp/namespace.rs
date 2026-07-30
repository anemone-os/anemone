use alloc::vec::Vec;

use anemone_net_api::{
    InterfaceId,
    udp::{
        UdpBindError, UdpBindRequest, UdpCreateError, UdpEndpointId, UdpEndpointInvalidation,
        UdpEndpointLimits, UdpLocalBinding, UdpNamespacePolicy, UdpQueryError, UdpRetireError,
    },
};
use smoltcp::iface::SocketSet;

use super::Endpoint;

/// Owns the domain-wide endpoint identity and committed binding namespace.
///
/// Engine sockets remain inside their `SocketSet`; the endpoint collection is
/// the only authority that can allocate, conflict-check, publish, or withdraw
/// a binding. No derived binding index exists in this first bounded version.
pub(crate) struct UdpEndpoints {
    pub(super) endpoints: Vec<Endpoint>,
    policy: UdpNamespacePolicy,
    next_id: u64,
    next_ephemeral: Option<u16>,
    pub(super) next_egress_endpoint: usize,
    /// Bounded, coalesced recheck hints. Endpoint facts remain authoritative
    /// in `endpoints`; this queue carries no readiness payload.
    pending_invalidations: Vec<UdpEndpointInvalidation>,
}

impl UdpEndpoints {
    pub(crate) fn new(policy: UdpNamespacePolicy) -> Self {
        assert!(policy.endpoint_capacity() > 0);
        assert!(
            policy.ephemeral_port_first() != 0
                && policy.ephemeral_port_first() <= policy.ephemeral_port_last()
        );
        Self {
            endpoints: Vec::new(),
            policy,
            next_id: 0,
            next_ephemeral: None,
            next_egress_endpoint: 0,
            pending_invalidations: Vec::new(),
        }
    }

    pub(crate) fn prepare_endpoint(
        &mut self,
        limits: UdpEndpointLimits,
    ) -> Result<Endpoint, UdpCreateError> {
        if self.endpoints.len() >= self.policy.endpoint_capacity() {
            return Err(UdpCreateError::EndpointCapacity);
        }
        let raw = self.next_id;
        self.next_id = raw
            .checked_add(1)
            .expect("boot-local UDP endpoint identity exhausted");
        Ok(Endpoint::new(UdpEndpointId::from_owner_raw(raw), limits))
    }

    pub(crate) fn publish_endpoint(&mut self, endpoint: Endpoint) -> UdpEndpointId {
        let id = endpoint.id;
        self.endpoints.push(endpoint);
        self.invalidate(id);
        id
    }

    pub(crate) fn prepare_binding(
        &mut self,
        id: UdpEndpointId,
        request: UdpBindRequest,
    ) -> Result<UdpLocalBinding, UdpBindError> {
        let endpoint = self.endpoint(id).ok_or(UdpBindError::UnknownEndpoint)?;
        if endpoint.binding.is_some() {
            return Err(UdpBindError::AlreadyBound);
        }

        let port = if request.port() != 0 {
            if self.conflicts(request.address(), request.port()) {
                return Err(UdpBindError::PortInUse);
            }
            request.port()
        } else {
            self.allocate_ephemeral(request.address())?
        };

        Ok(UdpLocalBinding::from_owner_commit(request.address(), port))
    }

    pub(crate) fn commit_binding(&mut self, id: UdpEndpointId, binding: UdpLocalBinding) {
        self.endpoint_mut(id)
            .expect("prepared UDP endpoint disappeared before binding commit")
            .commit_binding(binding);
        self.invalidate(id);
    }

    pub(crate) fn binding(
        &self,
        id: UdpEndpointId,
    ) -> Result<Option<UdpLocalBinding>, UdpQueryError> {
        self.endpoint(id)
            .map(|endpoint| endpoint.binding)
            .ok_or(UdpQueryError::UnknownEndpoint)
    }

    fn allocate_ephemeral(
        &mut self,
        address: anemone_net_api::Ipv4Address,
    ) -> Result<u16, UdpBindError> {
        let first = self.policy.ephemeral_port_first();
        let last = self.policy.ephemeral_port_last();
        let start = self
            .next_ephemeral
            .filter(|port| (first..=last).contains(port))
            .unwrap_or(first);
        let range_len = u32::from(last) - u32::from(first) + 1;
        let start_offset = u32::from(start) - u32::from(first);
        for offset in 0..range_len {
            let raw = u32::from(first) + (start_offset + offset) % range_len;
            let port = raw as u16;
            if self.conflicts(address, port) {
                continue;
            }
            self.next_ephemeral = Some(if port == last { first } else { port + 1 });
            return Ok(port);
        }
        Err(UdpBindError::EphemeralPortsExhausted)
    }

    fn conflicts(&self, address: anemone_net_api::Ipv4Address, port: u16) -> bool {
        self.endpoints.iter().any(|endpoint| {
            let Some(existing) = endpoint.binding else {
                return false;
            };
            existing.port() == port
                && (existing.address().is_unspecified()
                    || address.is_unspecified()
                    || existing.address() == address)
        })
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
        let mut invalidated = Vec::new();
        for endpoint in &mut self.endpoints {
            if endpoint.remove_engine(interface, sockets) {
                invalidated.push(endpoint.id());
            }
        }
        for endpoint in invalidated {
            self.invalidate(endpoint);
        }
    }

    pub(crate) fn withdraw(&mut self, id: UdpEndpointId) -> Result<Endpoint, UdpRetireError> {
        let index = self
            .endpoints
            .iter()
            .position(|endpoint| endpoint.id == id)
            .ok_or(UdpRetireError::UnknownEndpoint)?;
        let endpoint = self.endpoints.remove(index);
        self.invalidate(id);
        if self.next_egress_endpoint > self.endpoints.len() {
            self.next_egress_endpoint = 0;
        }
        Ok(endpoint)
    }

    pub(crate) fn endpoint(&self, id: UdpEndpointId) -> Option<&Endpoint> {
        self.endpoints.iter().find(|endpoint| endpoint.id == id)
    }

    pub(crate) fn endpoint_mut(&mut self, id: UdpEndpointId) -> Option<&mut Endpoint> {
        self.endpoints.iter_mut().find(|endpoint| endpoint.id == id)
    }

    pub(crate) fn invalidate(&mut self, id: UdpEndpointId) {
        // A correctly wired consumer drains after every exclusive Stack
        // window. Still keep the owner queue intrinsically bounded if a host
        // fixture batches operations: retired identities have no live source
        // and may be discarded when a later identity needs a hint.
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
            .push(UdpEndpointInvalidation::from_owner_transition(id));
    }

    pub(crate) fn take_invalidations(&mut self) -> Vec<UdpEndpointInvalidation> {
        core::mem::take(&mut self.pending_invalidations)
    }
}
