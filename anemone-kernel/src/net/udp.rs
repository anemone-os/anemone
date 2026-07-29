//! Kernel-private UDP endpoint capability for the initial network domain.

use anemone_net_api::{
    Ipv4Address,
    udp::{
        UdpBindError, UdpBindRequest, UdpCreateError, UdpEndpointId, UdpEndpointLimits,
        UdpLocalBinding, UdpQueryError, UdpRetireError,
    },
};

use crate::{kconfig_defs::*, prelude::*};

use super::{ACTIVE_PATHS, domain::DomainStack};

#[derive(Clone)]
pub(crate) struct UdpEndpointPort {
    stack: Arc<DomainStack>,
    endpoint: UdpEndpointId,
}

impl core::fmt::Debug for UdpEndpointPort {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("UdpEndpointPort").finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BindError {
    AddressUnavailable,
    Stack(UdpBindError),
}

pub(crate) fn create_endpoint() -> Result<UdpEndpointPort, UdpCreateError> {
    let stack = {
        let authority = ACTIVE_PATHS.lock();
        assert!(
            !authority.shutdown_started,
            "userspace UDP endpoint creation raced terminal network shutdown"
        );
        assert!(
            authority.domain.control_plane().is_some(),
            "userspace UDP endpoint creation preceded control-plane publication"
        );
        authority.domain.stack()
    };
    let endpoint = stack.create_udp_endpoint(UdpEndpointLimits::new(
        NET_UDP_ENDPOINT_CAPACITY,
        NET_UDP_TX_DATAGRAM_CAPACITY,
        NET_UDP_RX_DATAGRAM_CAPACITY,
        NET_UDP_MAX_PAYLOAD_BYTES,
    ))?;
    Ok(UdpEndpointPort { stack, endpoint })
}

impl UdpEndpointPort {
    pub(crate) fn bind(
        &self,
        address: Ipv4Address,
        port: u16,
    ) -> Result<UdpLocalBinding, BindError> {
        if !address.is_unspecified() {
            let local = {
                let authority = ACTIVE_PATHS.lock();
                authority
                    .domain
                    .control_plane()
                    .expect("published UDP capability lost its control plane")
                    .owns_local_address(address)
            };
            if !local {
                return Err(BindError::AddressUnavailable);
            }
        }
        self.stack
            .bind_udp_endpoint(
                self.endpoint,
                UdpBindRequest::new(address, port),
                NET_UDP_EPHEMERAL_PORT_FIRST,
                NET_UDP_EPHEMERAL_PORT_LAST,
            )
            .map_err(BindError::Stack)
    }

    pub(crate) fn binding(&self) -> Result<Option<UdpLocalBinding>, UdpQueryError> {
        self.stack.udp_endpoint_binding(self.endpoint)
    }

    pub(crate) fn retire(&self) -> Result<(), UdpRetireError> {
        self.stack.retire_udp_endpoint(self.endpoint)
    }
}
