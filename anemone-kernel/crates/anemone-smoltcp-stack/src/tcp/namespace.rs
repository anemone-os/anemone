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
        let binding = self.reserve_binding(request)?;
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
            EndpointRole::Idle => self.reserve_binding(TcpBindRequest::new(implicit_address, 0)),
            EndpointRole::Bound(binding) => Ok(*binding),
            EndpointRole::Vacant
            | EndpointRole::Listener(_)
            | EndpointRole::Connection(_)
            | EndpointRole::Reclaiming { .. } => Err(TcpBindError::WrongRole),
        }
    }

    fn reserve_binding(&self, request: TcpBindRequest) -> Result<TcpLocalBinding, TcpBindError> {
        if request.port() != 0 {
            let binding = TcpLocalBinding::from_owner_commit(request.address(), request.port());
            if self.binding_conflicts(binding) {
                return Err(TcpBindError::PortInUse);
            }
            return Ok(binding);
        }

        for port in self.policy.ephemeral_port_first..=self.policy.ephemeral_port_last {
            let binding = TcpLocalBinding::from_owner_commit(request.address(), port);
            if !self.binding_conflicts(binding) {
                return Ok(binding);
            }
        }
        Err(TcpBindError::EphemeralPortsExhausted)
    }

    fn binding_conflicts(&self, candidate: TcpLocalBinding) -> bool {
        self.endpoints.iter().any(|slot| {
            let existing = match &slot.role {
                EndpointRole::Bound(binding) => Some(*binding),
                EndpointRole::Listener(listener) => Some(listener.binding),
                EndpointRole::Connection(connection) => Some(connection.binding),
                EndpointRole::Reclaiming { binding, .. } => *binding,
                EndpointRole::Vacant | EndpointRole::Idle => None,
            };
            existing.is_some_and(|existing| {
                existing.port() == candidate.port()
                    && (existing.address().is_unspecified()
                        || candidate.address().is_unspecified()
                        || existing.address() == candidate.address())
            })
        })
    }
}
