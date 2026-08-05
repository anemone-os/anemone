//! Endpoint identity, binding, and ephemeral-port ownership.

use anemone_net_api::tcp::{
    TcpBindError, TcpBindRequest, TcpCreateError, TcpEndpointId, TcpLocalBinding,
};

use super::{EndpointRole, TcpEndpoints};

impl TcpEndpoints {
    pub(crate) fn create_endpoint(&mut self) -> Result<TcpEndpointId, TcpCreateError> {
        let slot = self
            .endpoints
            .iter_mut()
            .find(|slot| matches!(slot.role, EndpointRole::Vacant))
            .ok_or(TcpCreateError::EndpointCapacity)?;
        let raw = self.next_endpoint_id;
        self.next_endpoint_id = raw
            .checked_add(1)
            .expect("TCP Endpoint identity namespace exhausted");
        let id = TcpEndpointId::from_owner_raw(raw);
        slot.id = Some(id);
        slot.role = EndpointRole::Idle;
        Ok(id)
    }

    pub(crate) fn bind_endpoint(
        &mut self,
        id: TcpEndpointId,
        request: TcpBindRequest,
    ) -> Result<TcpLocalBinding, TcpBindError> {
        let role = &self.endpoint(id).ok_or(TcpBindError::UnknownEndpoint)?.role;
        if !matches!(role, EndpointRole::Idle) {
            return Err(TcpBindError::WrongRole);
        }
        let binding = self.reserve_binding(id, request)?;
        self.endpoint_mut(id)
            .expect("validated TCP Endpoint disappeared before bind commit")
            .role = EndpointRole::Bound(binding);
        Ok(binding)
    }

    pub(crate) fn current_binding(&self, id: TcpEndpointId) -> Option<TcpLocalBinding> {
        match &self.endpoint(id)?.role {
            EndpointRole::Bound(binding) => Some(*binding),
            EndpointRole::Listener(listener) => Some(listener.binding),
            EndpointRole::Connection(connection) => Some(connection.binding),
            EndpointRole::Vacant | EndpointRole::Idle | EndpointRole::Reclaiming { .. } => None,
        }
    }

    pub(crate) fn prepare_binding(
        &self,
        id: TcpEndpointId,
        implicit_address: anemone_net_api::Ipv4Address,
    ) -> Result<TcpLocalBinding, TcpBindError> {
        match &self.endpoint(id).ok_or(TcpBindError::UnknownEndpoint)?.role {
            EndpointRole::Idle => {
                self.reserve_binding(id, TcpBindRequest::new(implicit_address, 0))
            },
            EndpointRole::Bound(binding) => Ok(*binding),
            EndpointRole::Listener(listener) => Ok(listener.binding),
            EndpointRole::Vacant
            | EndpointRole::Connection(_)
            | EndpointRole::Reclaiming { .. } => Err(TcpBindError::WrongRole),
        }
    }

    pub(crate) fn reuse_address(&self, id: TcpEndpointId) -> Result<bool, TcpBindError> {
        let endpoint = self.endpoint(id).ok_or(TcpBindError::UnknownEndpoint)?;
        if matches!(endpoint.role, EndpointRole::Reclaiming { .. }) {
            return Err(TcpBindError::UnknownEndpoint);
        }
        Ok(endpoint.reuse_address)
    }

    pub(crate) fn set_reuse_address(
        &mut self,
        id: TcpEndpointId,
        enabled: bool,
    ) -> Result<(), TcpBindError> {
        let endpoint = self.endpoint_mut(id).ok_or(TcpBindError::UnknownEndpoint)?;
        if matches!(endpoint.role, EndpointRole::Reclaiming { .. }) {
            return Err(TcpBindError::UnknownEndpoint);
        }
        endpoint.reuse_address = enabled;
        Ok(())
    }

    fn reserve_binding(
        &self,
        id: TcpEndpointId,
        request: TcpBindRequest,
    ) -> Result<TcpLocalBinding, TcpBindError> {
        if request.port() != 0 {
            let binding = TcpLocalBinding::from_owner_commit(request.address(), request.port());
            if self.binding_conflicts(id, binding) {
                return Err(TcpBindError::PortInUse);
            }
            return Ok(binding);
        }

        for port in self.policy.ephemeral_port_first..=self.policy.ephemeral_port_last {
            let binding = TcpLocalBinding::from_owner_commit(request.address(), port);
            if !self.binding_conflicts(id, binding) {
                return Ok(binding);
            }
        }
        Err(TcpBindError::EphemeralPortsExhausted)
    }

    pub(super) fn binding_conflicts(
        &self,
        candidate_owner: TcpEndpointId,
        candidate: TcpLocalBinding,
    ) -> bool {
        let candidate_reuse = self
            .endpoint(candidate_owner)
            .expect("binding candidate must have a live TCP owner")
            .reuse_address;
        self.endpoints.iter().any(|slot| {
            if slot.id == Some(candidate_owner) {
                return false;
            }
            let existing = match &slot.role {
                EndpointRole::Bound(binding) => Some(*binding),
                EndpointRole::Listener(listener) => Some(listener.binding),
                EndpointRole::Connection(connection) => Some(connection.binding),
                EndpointRole::Reclaiming { binding, .. } => *binding,
                EndpointRole::Vacant | EndpointRole::Idle => None,
            };
            existing.is_some_and(|existing| {
                let overlaps = existing.port() == candidate.port()
                    && (existing.address().is_unspecified()
                        || candidate.address().is_unspecified()
                        || existing.address() == candidate.address());
                if !overlaps {
                    return false;
                }
                // Linux SO_REUSEADDR relaxes reservation admission only when
                // every overlapping owner opted in. A live listener remains
                // unique; listen admission rechecks the full set as well.
                !(candidate_reuse
                    && slot.reuse_address
                    && !matches!(slot.role, EndpointRole::Listener(_)))
            })
        })
    }

    pub(crate) fn listener_binding_conflicts(
        &self,
        candidate_owner: TcpEndpointId,
        candidate: TcpLocalBinding,
    ) -> bool {
        self.endpoints.iter().any(|slot| {
            if slot.id == Some(candidate_owner) {
                return false;
            }
            let EndpointRole::Listener(listener) = &slot.role else {
                return false;
            };
            listener.binding.port() == candidate.port()
                && (listener.binding.address().is_unspecified()
                    || candidate.address().is_unspecified()
                    || listener.binding.address() == candidate.address())
        })
    }
}
