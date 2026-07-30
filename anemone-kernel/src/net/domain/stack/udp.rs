use anemone_net_api::udp::{
    UdpBindError, UdpBindRequest, UdpCreateError, UdpEndpointId, UdpEndpointLimits,
    UdpLocalBinding, UdpQueryError, UdpRetireError,
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

    pub(in crate::net) fn retire_udp_endpoint(
        &self,
        endpoint: UdpEndpointId,
    ) -> Result<(), UdpRetireError> {
        self.stack.lock().retire_udp_endpoint(endpoint)
    }
}
