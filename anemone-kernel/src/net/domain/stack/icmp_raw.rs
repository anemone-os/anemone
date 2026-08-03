use anemone_net_api::{
    Ipv4EgressSelection,
    icmp_raw::{
        IcmpRawCreateError, IcmpRawDropDiagnostics, IcmpRawEgressPolicy, IcmpRawEndpointConfig,
        IcmpRawEndpointFacts, IcmpRawEndpointId, IcmpRawEndpointInvalidation,
        IcmpRawEndpointLimits, IcmpRawMutationError, IcmpRawQueryError, IcmpRawReceiveError,
        IcmpRawReceivedPacket, IcmpRawRetireError, IcmpRawSendError, IcmpRawTypeFilter,
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

    pub(in crate::net) fn bind_icmp_raw_endpoint(
        &self,
        endpoint: IcmpRawEndpointId,
        local: Option<anemone_net_api::Ipv4Address>,
    ) -> Result<(), IcmpRawMutationError> {
        self.protocol_transition(|stack| stack.bind_icmp_raw_endpoint(endpoint, local))
    }

    pub(in crate::net) fn connect_icmp_raw_endpoint(
        &self,
        endpoint: IcmpRawEndpointId,
        selected_source: anemone_net_api::Ipv4Address,
        peer: anemone_net_api::Ipv4Address,
    ) -> Result<(), IcmpRawMutationError> {
        self.protocol_transition(|stack| {
            stack.connect_icmp_raw_endpoint(endpoint, selected_source, peer)
        })
    }

    pub(in crate::net) fn disconnect_icmp_raw_endpoint(
        &self,
        endpoint: IcmpRawEndpointId,
    ) -> Result<(), IcmpRawMutationError> {
        self.protocol_transition(|stack| stack.disconnect_icmp_raw_endpoint(endpoint))
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
