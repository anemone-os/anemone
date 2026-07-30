use anemone_net_api::udp::{
    UdpBindError, UdpBindRequest, UdpCreateError, UdpEgressSelection, UdpEndpointId,
    UdpEndpointLimits, UdpLocalBinding, UdpPeer, UdpQueryError, UdpReceiveError,
    UdpReceivedDatagram, UdpRetireError, UdpSendError,
};

use super::*;

impl DomainStack {
    pub(in crate::net) fn create_udp_endpoint(
        &self,
        limits: UdpEndpointLimits,
    ) -> Result<UdpEndpointId, UdpCreateError> {
        self.stack.lock().create_udp_endpoint(limits)
    }

    pub(in crate::net) fn bind_udp_endpoint(
        &self,
        endpoint: UdpEndpointId,
        request: UdpBindRequest,
    ) -> Result<UdpLocalBinding, UdpBindError> {
        self.stack.lock().bind_udp_endpoint(endpoint, request)
    }

    pub(in crate::net) fn udp_endpoint_binding(
        &self,
        endpoint: UdpEndpointId,
    ) -> Result<Option<UdpLocalBinding>, UdpQueryError> {
        self.stack.lock().udp_endpoint_binding(endpoint)
    }

    pub(in crate::net) fn send_udp_endpoint(
        &self,
        endpoint: UdpEndpointId,
        selection: UdpEgressSelection,
        peer: UdpPeer,
        payload: &[u8],
    ) -> Result<(), UdpSendError> {
        self.stack
            .lock()
            .send_udp_endpoint(endpoint, selection, peer, payload)
    }

    pub(in crate::net) fn receive_udp_endpoint(
        &self,
        endpoint: UdpEndpointId,
    ) -> Result<UdpReceivedDatagram, UdpReceiveError> {
        self.stack.lock().receive_udp_endpoint(endpoint)
    }

    pub(in crate::net) fn retire_udp_endpoint(
        &self,
        endpoint: UdpEndpointId,
    ) -> Result<(), UdpRetireError> {
        self.stack.lock().retire_udp_endpoint(endpoint)
    }
}
