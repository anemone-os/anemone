use alloc::vec::Vec;

use anemone_net_api::{
    InterfaceId,
    icmp_raw::{
        IcmpRawCreateError, IcmpRawDropDiagnostics, IcmpRawEndpointConfig, IcmpRawEndpointFacts,
        IcmpRawEndpointId, IcmpRawEndpointInvalidation, IcmpRawEndpointLimits,
        IcmpRawMutationError, IcmpRawNamespacePolicy, IcmpRawQueryError, IcmpRawReceiveError,
        IcmpRawReceivedPacket, IcmpRawRetireError, IcmpRawTypeFilter,
    },
};

use super::{egress::ActiveEgress, endpoint::Endpoint};

pub(crate) struct IcmpRawEndpoints {
    pub(super) endpoints: Vec<Endpoint>,
    policy: IcmpRawNamespacePolicy,
    next_id: u64,
    pub(super) next_identification: u16,
    pub(super) next_egress_endpoint: usize,
    pub(super) active_egress: Vec<ActiveEgress>,
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

    pub(crate) fn bind(
        &mut self,
        id: IcmpRawEndpointId,
        local: Option<anemone_net_api::Ipv4Address>,
    ) -> Result<(), IcmpRawMutationError> {
        if local.is_some_and(|address| !address.is_unicast()) {
            return Err(IcmpRawMutationError::InvalidAssociation);
        }
        self.endpoint_mut(id)
            .ok_or(IcmpRawMutationError::UnknownEndpoint)?
            .bind(local)
            .map_err(|()| IcmpRawMutationError::InvalidAssociation)
    }

    pub(crate) fn connect(
        &mut self,
        id: IcmpRawEndpointId,
        selected_source: anemone_net_api::Ipv4Address,
        peer: anemone_net_api::Ipv4Address,
    ) -> Result<(), IcmpRawMutationError> {
        if !selected_source.is_unicast() || !peer.is_unicast() {
            return Err(IcmpRawMutationError::InvalidAssociation);
        }
        self.endpoint_mut(id)
            .ok_or(IcmpRawMutationError::UnknownEndpoint)?
            .connect(selected_source, peer);
        Ok(())
    }

    pub(crate) fn disconnect(&mut self, id: IcmpRawEndpointId) -> Result<(), IcmpRawMutationError> {
        self.endpoint_mut(id)
            .ok_or(IcmpRawMutationError::UnknownEndpoint)?
            .disconnect();
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

    pub(super) fn endpoint(&self, id: IcmpRawEndpointId) -> Option<&Endpoint> {
        self.endpoints.iter().find(|endpoint| endpoint.id == id)
    }

    pub(super) fn endpoint_mut(&mut self, id: IcmpRawEndpointId) -> Option<&mut Endpoint> {
        self.endpoints.iter_mut().find(|endpoint| endpoint.id == id)
    }

    pub(super) fn invalidate(&mut self, id: IcmpRawEndpointId) {
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
