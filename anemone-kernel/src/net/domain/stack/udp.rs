use anemone_net_api::udp::{
    UdpBindError, UdpBindRequest, UdpCreateError, UdpEgressSelection, UdpEndpointFacts,
    UdpEndpointId, UdpEndpointInvalidation, UdpEndpointLimits, UdpLocalBinding, UdpPeer,
    UdpQueryError, UdpReceiveError, UdpReceivedDatagram, UdpRetireError, UdpSendError,
};

use super::*;

use crate::net::udp::{EventRegistrationError, UdpEndpointInvalidationObserver};

struct UdpEndpointEventRoute {
    endpoint: UdpEndpointId,
    observer: Weak<dyn UdpEndpointInvalidationObserver>,
}

pub(super) struct UdpEndpointEventRoutes {
    routes: Vec<UdpEndpointEventRoute>,
}

impl UdpEndpointEventRoutes {
    pub(super) const fn new() -> Self {
        Self { routes: Vec::new() }
    }

    fn register(
        &mut self,
        endpoint: UdpEndpointId,
        observer: &Arc<dyn UdpEndpointInvalidationObserver>,
    ) -> Result<(), EventRegistrationError> {
        assert!(
            self.routes.iter().all(|route| route.endpoint != endpoint),
            "one UDP Endpoint cannot publish two reverse event routes"
        );
        self.routes
            .try_reserve(1)
            .map_err(|_| EventRegistrationError::OutOfMemory)?;
        self.routes.push(UdpEndpointEventRoute {
            endpoint,
            observer: Arc::downgrade(observer),
        });
        Ok(())
    }

    fn unregister(&mut self, endpoint: UdpEndpointId) {
        let index = self
            .routes
            .iter()
            .position(|route| route.endpoint == endpoint)
            .expect("published UDP event route disappeared before unregister");
        self.routes.remove(index);
    }

    fn observer(
        &mut self,
        endpoint: UdpEndpointId,
    ) -> Option<Weak<dyn UdpEndpointInvalidationObserver>> {
        // Pruning is resource hygiene only. Correctness comes from explicit
        // source unregister plus a fresh facts snapshot after every hint.
        self.routes
            .retain(|route| route.observer.strong_count() != 0);
        self.routes
            .iter()
            .find(|route| route.endpoint == endpoint)
            .map(|route| route.observer.clone())
    }
}

impl DomainStack {
    pub(in crate::net) fn register_udp_endpoint_observer(
        &self,
        endpoint: UdpEndpointId,
        observer: &Arc<dyn UdpEndpointInvalidationObserver>,
    ) -> Result<(), EventRegistrationError> {
        self.event_routes.lock().register(endpoint, observer)
    }

    pub(in crate::net) fn unregister_udp_endpoint_observer(&self, endpoint: UdpEndpointId) {
        self.event_routes.lock().unregister(endpoint);
    }

    pub(super) fn route_udp_invalidations(&self, invalidations: Vec<UdpEndpointInvalidation>) {
        for invalidation in invalidations {
            let observer = self.event_routes.lock().observer(invalidation.endpoint());
            if let Some(observer) = observer.and_then(|observer| observer.upgrade()) {
                observer.invalidate();
            }
        }
    }

    pub(super) fn pump_local(
        &self,
        interface: anemone_net_api::InterfaceId,
        now: anemone_net_api::Instant,
        budget: PumpBudget,
    ) -> Result<anemone_net_api::PumpOutcome, PumpError> {
        self.protocol_transition(|stack| stack.pump_local(interface, now, budget))
    }

    pub(super) fn pump_external<P: anemone_net_api::FrameProvider>(
        &self,
        interface: anemone_net_api::InterfaceId,
        provider: &mut P,
        now: anemone_net_api::Instant,
        budget: PumpBudget,
    ) -> Result<anemone_net_api::PumpOutcome, PumpError> {
        self.protocol_transition(|stack| stack.pump(interface, provider, now, budget))
    }

    pub(super) fn rollback_external_mapping(
        &self,
        interface: anemone_net_api::InterfaceId,
    ) -> Result<(), PumpError> {
        self.protocol_transition(|stack| stack.remove_interface(interface))
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
        selection: UdpEgressSelection,
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
