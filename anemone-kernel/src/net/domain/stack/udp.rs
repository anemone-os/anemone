use anemone_net_api::{
    Ipv4EgressSelection,
    udp::{
        UdpBindError, UdpBindRequest, UdpConnectError, UdpCreateError, UdpEndpointFacts,
        UdpEndpointId, UdpEndpointInvalidation, UdpEndpointLimits, UdpLocalBinding,
        UdpPeekedDatagram, UdpPeer, UdpQueryError, UdpReceiveError, UdpReceivedDatagram,
        UdpRetireError, UdpSendError,
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

    pub(in crate::net) fn connect_udp_endpoint(
        &self,
        endpoint: UdpEndpointId,
        selection: Ipv4EgressSelection,
        peer: UdpPeer,
    ) -> Result<(), UdpConnectError> {
        self.protocol_transition(|stack| stack.connect_udp_endpoint(endpoint, selection, peer))
    }

    pub(in crate::net) fn udp_endpoint_peer(
        &self,
        endpoint: UdpEndpointId,
    ) -> Result<Option<UdpPeer>, UdpQueryError> {
        self.stack.lock().udp_endpoint_peer(endpoint)
    }

    pub(in crate::net) fn resolve_udp_endpoint_destination(
        &self,
        endpoint: UdpEndpointId,
        explicit: Option<UdpPeer>,
    ) -> Result<UdpPeer, UdpSendError> {
        self.stack
            .lock()
            .resolve_udp_endpoint_destination(endpoint, explicit)
    }

    pub(in crate::net) fn disconnect_udp_endpoint(
        &self,
        endpoint: UdpEndpointId,
    ) -> Result<(), UdpQueryError> {
        self.protocol_transition(|stack| stack.disconnect_udp_endpoint(endpoint))
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
        let progression = self.protocol_transition(|stack| {
            stack.send_udp_endpoint(endpoint, selection, peer, payload)
        })?;
        crate::net::submit_protocol_progression(progression);
        Ok(())
    }

    pub(in crate::net) fn receive_udp_endpoint(
        &self,
        endpoint: UdpEndpointId,
    ) -> Result<UdpReceivedDatagram, UdpReceiveError> {
        self.protocol_transition(|stack| stack.receive_udp_endpoint(endpoint))
    }

    pub(in crate::net) fn peek_udp_endpoint(
        &self,
        endpoint: UdpEndpointId,
    ) -> Result<UdpPeekedDatagram, UdpReceiveError> {
        self.stack.lock().peek_udp_endpoint(endpoint)
    }

    pub(in crate::net) fn retire_udp_endpoint(
        &self,
        endpoint: UdpEndpointId,
    ) -> Result<(), UdpRetireError> {
        self.protocol_transition(|stack| stack.retire_udp_endpoint(endpoint))
    }
}
