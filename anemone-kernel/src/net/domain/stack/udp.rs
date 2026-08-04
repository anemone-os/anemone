use anemone_net_api::{
    Ipv4EgressSelection,
    udp::{
        UdpBindError, UdpBindRequest, UdpCreateError, UdpEndpointFacts, UdpEndpointId,
        UdpEndpointInvalidation, UdpEndpointLimits, UdpLocalBinding, UdpPeer, UdpQueryError,
        UdpReceiveError, UdpReceivedDatagram, UdpRetireError, UdpSendError,
    },
};

use super::*;

use crate::net::udp::{EventRegistrationError, UdpEndpointInvalidationObserver};

pub(super) type UdpEndpointEventRoutes =
    RecheckRoutes<UdpEndpointId, dyn UdpEndpointInvalidationObserver>;

impl DomainStack {
    pub(in crate::net) fn register_udp_endpoint_observer(
        &self,
        endpoint: UdpEndpointId,
        observer: &Arc<dyn UdpEndpointInvalidationObserver>,
    ) -> Result<(), EventRegistrationError> {
        self.udp_event_routes.lock().register(endpoint, observer)
    }

    pub(in crate::net) fn unregister_udp_endpoint_observer(&self, endpoint: UdpEndpointId) {
        self.udp_event_routes.lock().unregister(endpoint);
    }

    pub(super) fn route_udp_invalidations(&self, invalidations: Vec<UdpEndpointInvalidation>) {
        for invalidation in invalidations {
            let observer = self
                .udp_event_routes
                .lock()
                .observer(invalidation.endpoint());
            if let Some(observer) = observer.and_then(|observer| observer.upgrade()) {
                observer.invalidate();
            }
        }
    }

    pub(in crate::net) fn create_udp_endpoint(
        &self,
        limits: UdpEndpointLimits,
    ) -> Result<UdpEndpointId, UdpCreateError> {
        self.protocol_transition(|stack| stack.create_udp_endpoint(limits))
    }

    pub(in crate::net) fn bind_udp_endpoint(
        &self,
        endpoint: UdpEndpointId,
        request: UdpBindRequest,
    ) -> Result<UdpLocalBinding, UdpBindError> {
        self.protocol_transition(|stack| stack.bind_udp_endpoint(endpoint, request))
    }

    pub(in crate::net) fn udp_endpoint_binding(
        &self,
        endpoint: UdpEndpointId,
    ) -> Result<Option<UdpLocalBinding>, UdpQueryError> {
        self.stack.lock().udp_endpoint_binding(endpoint)
    }

    pub(in crate::net) fn udp_endpoint_facts(
        &self,
        endpoint: UdpEndpointId,
    ) -> Result<UdpEndpointFacts, UdpQueryError> {
        self.stack.lock().udp_endpoint_facts(endpoint)
    }

    pub(in crate::net) fn send_udp_endpoint(
        &self,
        endpoint: UdpEndpointId,
        selection: Ipv4EgressSelection,
        peer: UdpPeer,
        payload: &[u8],
    ) -> Result<(), UdpSendError> {
        self.protocol_transition(|stack| {
            stack.send_udp_endpoint(endpoint, selection, peer, payload)
        })
    }

    pub(in crate::net) fn receive_udp_endpoint(
        &self,
        endpoint: UdpEndpointId,
    ) -> Result<UdpReceivedDatagram, UdpReceiveError> {
        self.protocol_transition(|stack| stack.receive_udp_endpoint(endpoint))
    }

    pub(in crate::net) fn retire_udp_endpoint(
        &self,
        endpoint: UdpEndpointId,
    ) -> Result<(), UdpRetireError> {
        self.protocol_transition(|stack| stack.retire_udp_endpoint(endpoint))
    }
}
