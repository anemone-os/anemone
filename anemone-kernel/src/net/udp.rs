//! Kernel-private UDP endpoint capability for the initial network domain.

use anemone_net_api::{
    Ipv4Address,
    udp::{
        UdpBindError, UdpBindRequest, UdpCreateError, UdpEndpointId, UdpEndpointLimits,
        UdpLocalBinding, UdpNamespacePolicy, UdpQueryError, UdpRetireError,
    },
};

use crate::{kconfig_defs::*, prelude::*};

use super::{ACTIVE_PATHS, domain::DomainStack};

pub(in crate::net) const UDP_NAMESPACE_POLICY: UdpNamespacePolicy = UdpNamespacePolicy::new(
    NET_UDP_ENDPOINT_CAPACITY,
    NET_UDP_EPHEMERAL_PORT_FIRST,
    NET_UDP_EPHEMERAL_PORT_LAST,
);
const UDP_ENDPOINT_LIMITS: UdpEndpointLimits = UdpEndpointLimits::new(
    NET_UDP_TX_DATAGRAM_CAPACITY,
    NET_UDP_RX_DATAGRAM_CAPACITY,
    NET_UDP_MAX_PAYLOAD_BYTES,
);

const fn payload_storage_fits(datagram_capacity: usize, max_payload_bytes: usize) -> bool {
    match datagram_capacity.checked_mul(max_payload_bytes) {
        Some(bytes) => bytes <= isize::MAX as usize,
        None => false,
    }
}

static_assert!(
    NET_UDP_ENDPOINT_CAPACITY > 0,
    "net_udp_endpoint_capacity must be nonzero"
);
static_assert!(
    NET_UDP_TX_DATAGRAM_CAPACITY > 0,
    "net_udp_tx_datagram_capacity must be nonzero"
);
static_assert!(
    NET_UDP_RX_DATAGRAM_CAPACITY > 0,
    "net_udp_rx_datagram_capacity must be nonzero"
);
static_assert!(
    NET_UDP_MAX_PAYLOAD_BYTES > 0 && NET_UDP_MAX_PAYLOAD_BYTES <= 65_507,
    "net_udp_max_payload_bytes must be in 1..=65507"
);
static_assert!(
    payload_storage_fits(NET_UDP_TX_DATAGRAM_CAPACITY, NET_UDP_MAX_PAYLOAD_BYTES,),
    "configured UDP TX payload storage exceeds the contiguous byte-storage bound"
);
static_assert!(
    payload_storage_fits(NET_UDP_RX_DATAGRAM_CAPACITY, NET_UDP_MAX_PAYLOAD_BYTES,),
    "configured UDP RX payload storage exceeds the contiguous byte-storage bound"
);
static_assert!(
    NET_UDP_EPHEMERAL_PORT_FIRST > 0,
    "net_udp_ephemeral_port_first must be nonzero"
);
static_assert!(
    NET_UDP_EPHEMERAL_PORT_FIRST <= NET_UDP_EPHEMERAL_PORT_LAST,
    "net_udp_ephemeral_port_first must not exceed net_udp_ephemeral_port_last"
);
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
    let endpoint = stack.create_udp_endpoint(UDP_ENDPOINT_LIMITS)?;
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
            .bind_udp_endpoint(self.endpoint, UdpBindRequest::new(address, port))
            .map_err(BindError::Stack)
    }

    pub(crate) fn binding(&self) -> Result<Option<UdpLocalBinding>, UdpQueryError> {
        self.stack.udp_endpoint_binding(self.endpoint)
    }

    pub(crate) fn retire(&self) -> Result<(), UdpRetireError> {
        self.stack.retire_udp_endpoint(self.endpoint)
    }
}
