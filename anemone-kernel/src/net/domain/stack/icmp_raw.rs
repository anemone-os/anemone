use anemone_net_api::{
    Ipv4EgressSelection,
    icmp_raw::{
        IcmpRawAssociation, IcmpRawCreateError, IcmpRawDropDiagnostics, IcmpRawEgressPolicy,
        IcmpRawEndpointConfig, IcmpRawEndpointFacts, IcmpRawEndpointId,
        IcmpRawEndpointInvalidation, IcmpRawEndpointLimits, IcmpRawMutationError,
        IcmpRawQueryError, IcmpRawReceiveError, IcmpRawReceivedPacket, IcmpRawRetireError,
        IcmpRawSendError, IcmpRawTypeFilter,
    },
};

use crate::{
    net::icmp_raw::{EventRegistrationError, IcmpRawEndpointInvalidationObserver},
    prelude::*,
};

use super::{DomainStack, RecheckRoutes};

pub(super) type IcmpRawEndpointEventRoutes =
    RecheckRoutes<IcmpRawEndpointId, dyn IcmpRawEndpointInvalidationObserver>;

impl DomainStack {
    pub(in crate::net) fn register_icmp_raw_endpoint_observer(
        &self,
        endpoint: IcmpRawEndpointId,
        observer: &Arc<dyn IcmpRawEndpointInvalidationObserver>,
    ) -> Result<(), EventRegistrationError> {
        self.icmp_raw_event_routes
            .lock()
            .register(endpoint, observer)
    }

    pub(in crate::net) fn unregister_icmp_raw_endpoint_observer(
        &self,
        endpoint: IcmpRawEndpointId,
    ) {
        self.icmp_raw_event_routes.lock().unregister(endpoint);
    }

    pub(super) fn route_icmp_raw_invalidations(
        &self,
        invalidations: Vec<IcmpRawEndpointInvalidation>,
    ) {
        for invalidation in invalidations {
            let observer = self
                .icmp_raw_event_routes
                .lock()
                .observer(invalidation.endpoint());
            if let Some(observer) = observer.and_then(|observer| observer.upgrade()) {
                observer.invalidate();
            }
        }
    }

    pub(in crate::net) fn create_icmp_raw_endpoint(
        &self,
        limits: IcmpRawEndpointLimits,
    ) -> Result<IcmpRawEndpointId, IcmpRawCreateError> {
        self.protocol_transition(|stack| stack.create_icmp_raw_endpoint(limits))
    }

    pub(in crate::net) fn set_icmp_raw_association(
        &self,
        endpoint: IcmpRawEndpointId,
        association: IcmpRawAssociation,
    ) -> Result<(), IcmpRawMutationError> {
        self.protocol_transition(|stack| stack.set_icmp_raw_association(endpoint, association))
    }

    pub(in crate::net) fn set_icmp_raw_filter(
        &self,
        endpoint: IcmpRawEndpointId,
        filter: IcmpRawTypeFilter,
    ) -> Result<(), IcmpRawMutationError> {
        self.protocol_transition(|stack| stack.set_icmp_raw_filter(endpoint, filter))
    }

    pub(in crate::net) fn icmp_raw_endpoint_config(
        &self,
        endpoint: IcmpRawEndpointId,
    ) -> Result<IcmpRawEndpointConfig, IcmpRawQueryError> {
        self.stack.lock().icmp_raw_endpoint_config(endpoint)
    }

    pub(in crate::net) fn icmp_raw_endpoint_facts(
        &self,
        endpoint: IcmpRawEndpointId,
    ) -> Result<IcmpRawEndpointFacts, IcmpRawQueryError> {
        self.stack.lock().icmp_raw_endpoint_facts(endpoint)
    }

    pub(in crate::net) fn icmp_raw_endpoint_diagnostics(
        &self,
        endpoint: IcmpRawEndpointId,
    ) -> Result<IcmpRawDropDiagnostics, IcmpRawQueryError> {
        self.stack.lock().icmp_raw_endpoint_diagnostics(endpoint)
    }

    pub(in crate::net) fn send_icmp_raw_endpoint(
        &self,
        endpoint: IcmpRawEndpointId,
        selection: Ipv4EgressSelection,
        destination: anemone_net_api::Ipv4Address,
        policy: IcmpRawEgressPolicy,
        message: &[u8],
    ) -> Result<(), IcmpRawSendError> {
        self.protocol_transition(|stack| {
            stack.send_icmp_raw_endpoint(endpoint, selection, destination, policy, message)
        })
    }

    pub(in crate::net) fn receive_icmp_raw_endpoint(
        &self,
        endpoint: IcmpRawEndpointId,
        peek: bool,
    ) -> Result<IcmpRawReceivedPacket, IcmpRawReceiveError> {
        self.protocol_transition(|stack| stack.receive_icmp_raw_endpoint(endpoint, peek))
    }

    pub(in crate::net) fn retire_icmp_raw_endpoint(
        &self,
        endpoint: IcmpRawEndpointId,
    ) -> Result<(), IcmpRawRetireError> {
        self.protocol_transition(|stack| stack.retire_icmp_raw_endpoint(endpoint))
    }
}
