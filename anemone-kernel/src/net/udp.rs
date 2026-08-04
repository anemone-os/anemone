//! Kernel-private UDP endpoint capability for the initial network domain.

use anemone_net_api::{
    Ipv4Address, Ipv4EgressSelection,
    udp::{
        UdpBindError, UdpBindRequest, UdpConnectError, UdpCreateError, UdpEndpointFacts,
        UdpEndpointId, UdpEndpointLimits, UdpLocalBinding, UdpNamespacePolicy, UdpPeekedDatagram,
        UdpPeer, UdpQueryError, UdpReceiveError, UdpReceivedDatagram, UdpRetireError, UdpSendError,
    },
};

use crate::{kconfig_defs::*, prelude::*};

use super::{ACTIVE_PATHS, domain::DomainStack};

pub(crate) use super::EventRegistrationError;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SendError {
    Bind(UdpBindError),
    NoRoute,
    SourceUnavailable,
    InterfaceUnavailable,
    Stack(UdpSendError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConnectError {
    NoRoute,
    SourceUnavailable,
    InterfaceUnavailable,
    Stack(UdpConnectError),
}

pub(crate) trait UdpEndpointInvalidationObserver: Send + Sync {
    fn invalidate(&self);
}

/// Source-owned proof that the reverse event route is published.
///
/// The endpoint identity is protocol state, not a diagnostic token: it is the
/// key used to withdraw exactly the route installed for this boot-unique
/// Endpoint. It never carries readiness or liveness truth.
pub(crate) struct UdpEndpointEventRegistration {
    stack: Arc<DomainStack>,
    endpoint: UdpEndpointId,
    active: bool,
}

impl UdpEndpointEventRegistration {
    pub(crate) fn unregister(mut self) {
        self.stack.unregister_udp_endpoint_observer(self.endpoint);
        self.active = false;
    }
}

impl Drop for UdpEndpointEventRegistration {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        // Withdraw the weak publication before exposing the owner bug. This
        // cannot retain the Socket source, but leaving it registered would
        // accumulate stale reverse entries until another transition.
        self.stack.unregister_udp_endpoint_observer(self.endpoint);
        self.active = false;
        assert!(
            false,
            "UDP endpoint event registration dropped while active"
        );
    }
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
    pub(crate) fn register_invalidation_observer(
        &self,
        observer: &Arc<dyn UdpEndpointInvalidationObserver>,
    ) -> Result<UdpEndpointEventRegistration, EventRegistrationError> {
        self.stack
            .register_udp_endpoint_observer(self.endpoint, observer)?;
        Ok(UdpEndpointEventRegistration {
            stack: self.stack.clone(),
            endpoint: self.endpoint,
            active: true,
        })
    }

    pub(crate) fn facts(&self) -> Result<UdpEndpointFacts, UdpQueryError> {
        self.stack.udp_endpoint_facts(self.endpoint)
    }

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

    pub(crate) fn peer(&self) -> Result<Option<UdpPeer>, UdpQueryError> {
        self.stack.udp_endpoint_peer(self.endpoint)
    }

    pub(crate) fn connect(&self, peer: UdpPeer) -> Result<(), ConnectError> {
        let binding = self.binding().map_err(|error| match error {
            UdpQueryError::UnknownEndpoint => ConnectError::Stack(UdpConnectError::UnknownEndpoint),
        })?;
        let selection = {
            let authority = ACTIVE_PATHS.lock();
            authority
                .domain
                .control_plane()
                .expect("published UDP capability lost its control plane")
                .select(
                    peer.address(),
                    binding.and_then(|binding| {
                        (!binding.address().is_unspecified()).then_some(binding.address())
                    }),
                )
                .map_err(|error| match error {
                    super::domain::SelectionError::NoRoute => ConnectError::NoRoute,
                    super::domain::SelectionError::SourceUnavailable => {
                        ConnectError::SourceUnavailable
                    },
                    super::domain::SelectionError::InterfaceUnavailable => {
                        ConnectError::InterfaceUnavailable
                    },
                })?
        };
        self.stack
            .connect_udp_endpoint(
                self.endpoint,
                Ipv4EgressSelection::new(selection.interface(), selection.source()),
                peer,
            )
            .map_err(ConnectError::Stack)
    }

    pub(crate) fn disconnect(&self) -> Result<(), UdpQueryError> {
        self.stack.disconnect_udp_endpoint(self.endpoint)
    }

    pub(crate) fn ensure_bound(&self) -> Result<UdpLocalBinding, SendError> {
        let binding = match self.binding().map_err(|error| match error {
            UdpQueryError::UnknownEndpoint => SendError::Stack(UdpSendError::UnknownEndpoint),
        })? {
            Some(binding) => binding,
            None => self
                .stack
                .bind_udp_endpoint(
                    self.endpoint,
                    UdpBindRequest::new(Ipv4Address::UNSPECIFIED, 0),
                )
                .map_err(SendError::Bind)?,
        };
        Ok(binding)
    }

    pub(crate) fn send(&self, explicit: Option<UdpPeer>, payload: &[u8]) -> Result<(), SendError> {
        let peer = self
            .stack
            .resolve_udp_endpoint_destination(self.endpoint, explicit)
            .map_err(SendError::Stack)?;
        let binding = self.ensure_bound()?;

        let selection = {
            let authority = ACTIVE_PATHS.lock();
            authority
                .domain
                .control_plane()
                .expect("published UDP capability lost its control plane")
                .select(
                    peer.address(),
                    (!binding.address().is_unspecified()).then_some(binding.address()),
                )
                .map_err(|error| match error {
                    super::domain::SelectionError::NoRoute => SendError::NoRoute,
                    super::domain::SelectionError::SourceUnavailable => {
                        SendError::SourceUnavailable
                    },
                    super::domain::SelectionError::InterfaceUnavailable => {
                        SendError::InterfaceUnavailable
                    },
                })?
        };
        let stack_selection = Ipv4EgressSelection::new(selection.interface(), selection.source());
        self.stack
            .send_udp_endpoint(self.endpoint, stack_selection, peer, payload)
            .map_err(SendError::Stack)?;
        selection.request_pump();
        Ok(())
    }

    pub(crate) fn receive(&self) -> Result<UdpReceivedDatagram, UdpReceiveError> {
        self.stack.receive_udp_endpoint(self.endpoint)
    }

    pub(crate) fn peek(&self) -> Result<UdpPeekedDatagram, UdpReceiveError> {
        self.stack.peek_udp_endpoint(self.endpoint)
    }

    pub(crate) fn retire(&self) -> Result<(), UdpRetireError> {
        self.stack.retire_udp_endpoint(self.endpoint)
    }
}
