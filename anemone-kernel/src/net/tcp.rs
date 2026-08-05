//! Kernel-private capability for the initial-domain TCP owner.
//!
//! Stage 2 CKPT 2A deliberately stops at this module. No Socket profile,
//! descriptor, fd, syscall, user pointer, or wait route can reach it.

use anemone_net_api::{
    Ipv4Address, Ipv4EgressSelection,
    tcp::{
        TcpBindError, TcpBindRequest, TcpChildError, TcpConnectError, TcpConnectionObservation,
        TcpCreateError, TcpEndpointId, TcpListenError, TcpLocalBinding, TcpPeer, TcpQueryError,
        TcpReceiveError, TcpReceiveReservation, TcpReceiveResolveError, TcpSendError,
    },
};
use anemone_smoltcp_stack::TcpPolicy;

use crate::{kconfig_defs::*, prelude::*};

use super::{
    ACTIVE_PATHS,
    domain::{DomainStack, SelectionError},
};

pub(in crate::net) const TCP_POLICY: TcpPolicy = TcpPolicy::new(
    NET_TCP_ENDPOINT_CAPACITY,
    NET_TCP_ENGINE_TIMER_CAPACITY,
    NET_TCP_LISTENER_COMPLETED_CAPACITY,
    NET_TCP_RX_BUFFER_BYTES,
    NET_TCP_TX_BUFFER_BYTES,
    NET_TCP_DEFERRED_RECLAIM_CAPACITY,
    NET_TCP_EPHEMERAL_PORT_FIRST,
    NET_TCP_EPHEMERAL_PORT_LAST,
);

const fn engine_storage_fits() -> bool {
    let Some(bytes_per_engine) = NET_TCP_RX_BUFFER_BYTES.checked_add(NET_TCP_TX_BUFFER_BYTES)
    else {
        return false;
    };
    let Some(total) = NET_TCP_ENGINE_TIMER_CAPACITY.checked_mul(bytes_per_engine) else {
        return false;
    };
    total <= isize::MAX as usize
}

static_assert!(
    NET_TCP_ENDPOINT_CAPACITY > 1,
    "net_tcp_endpoint_capacity must fit one listener and one accepted endpoint"
);
static_assert!(
    NET_TCP_ENGINE_TIMER_CAPACITY > 0,
    "net_tcp_engine_timer_capacity must be nonzero"
);
static_assert!(
    NET_TCP_LISTENER_COMPLETED_CAPACITY >= 10,
    "net_tcp_listener_completed_capacity must retain at least ten completed children"
);
static_assert!(
    NET_TCP_ENGINE_TIMER_CAPACITY > NET_TCP_LISTENER_COMPLETED_CAPACITY,
    "net_tcp_engine_timer_capacity must fit one listener and an accepted child"
);
static_assert!(
    NET_TCP_RX_BUFFER_BYTES > 0 && NET_TCP_TX_BUFFER_BYTES > 0,
    "TCP engine buffers must be nonzero"
);
static_assert!(
    NET_TCP_DEFERRED_RECLAIM_CAPACITY >= NET_TCP_ENGINE_TIMER_CAPACITY,
    "TCP reclaim storage must reserve one infallible slot for every engine"
);
static_assert!(
    NET_TCP_EPHEMERAL_PORT_FIRST > 0 && NET_TCP_EPHEMERAL_PORT_FIRST <= NET_TCP_EPHEMERAL_PORT_LAST,
    "TCP ephemeral port range must be nonempty and nonzero"
);
static_assert!(
    engine_storage_fits(),
    "configured TCP engine storage exceeds the contiguous byte-storage bound"
);

pub(crate) struct TcpEndpointPort {
    stack: Arc<DomainStack>,
    endpoint: Option<TcpEndpointId>,
}

impl core::fmt::Debug for TcpEndpointPort {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TcpEndpointPort").finish_non_exhaustive()
    }
}

pub(crate) struct TcpPendingChildPort {
    stack: Arc<DomainStack>,
    child: Option<anemone_net_api::tcp::TcpPendingChild>,
}

pub(crate) struct TcpReceivePort {
    stack: Arc<DomainStack>,
    reservation: Option<TcpReceiveReservation>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BindError {
    AddressUnavailable,
    Stack(TcpBindError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConnectError {
    NoRoute,
    SourceUnavailable,
    InterfaceUnavailable,
    Stack(TcpConnectError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ListenError {
    AddressUnavailable,
    Stack(TcpListenError),
}

pub(crate) fn create_endpoint() -> Result<TcpEndpointPort, TcpCreateError> {
    let stack = {
        let authority = ACTIVE_PATHS.lock();
        assert!(
            !authority.shutdown_started,
            "kernel TCP endpoint creation raced terminal network shutdown"
        );
        assert!(
            authority.domain.control_plane().is_some(),
            "kernel TCP endpoint creation preceded control-plane publication"
        );
        authority.domain.stack()
    };
    let endpoint = stack.create_tcp_endpoint()?;
    Ok(TcpEndpointPort {
        stack,
        endpoint: Some(endpoint),
    })
}

impl TcpEndpointPort {
    fn id(&self) -> TcpEndpointId {
        self.endpoint
            .expect("retired TCP capability cannot issue another operation")
    }

    pub(crate) fn bind(
        &self,
        address: Ipv4Address,
        port: u16,
    ) -> Result<TcpLocalBinding, BindError> {
        if !address.is_unspecified() && !owns_local_address(address) {
            return Err(BindError::AddressUnavailable);
        }
        self.stack
            .bind_tcp_endpoint(self.id(), TcpBindRequest::new(address, port))
            .map_err(BindError::Stack)
    }

    pub(crate) fn binding(&self) -> Result<Option<TcpLocalBinding>, TcpQueryError> {
        self.stack.tcp_endpoint_binding(self.id())
    }

    pub(crate) fn connect(&self, peer: TcpPeer) -> Result<(), ConnectError> {
        let binding = self.binding().map_err(|error| match error {
            TcpQueryError::UnknownEndpoint | TcpQueryError::WrongRole => {
                ConnectError::Stack(TcpConnectError::UnknownEndpoint)
            },
        })?;
        let selection = select_ipv4(
            peer.address(),
            binding.and_then(|binding| {
                (!binding.address().is_unspecified()).then_some(binding.address())
            }),
        )?;
        self.stack
            .start_tcp_connect(
                self.id(),
                Ipv4EgressSelection::new(selection.interface, selection.source),
                peer,
            )
            .map_err(ConnectError::Stack)
    }

    pub(crate) fn connection(&self) -> Result<TcpConnectionObservation, TcpQueryError> {
        self.stack.observe_tcp_connection(self.id())
    }

    pub(crate) fn listen(&self) -> Result<(), ListenError> {
        let binding = self.binding().map_err(|error| match error {
            TcpQueryError::UnknownEndpoint | TcpQueryError::WrongRole => {
                ListenError::Stack(TcpListenError::UnknownEndpoint)
            },
        })?;
        let (destination, explicit_source) = match binding {
            Some(binding) if !binding.address().is_unspecified() => {
                if !owns_local_address(binding.address()) {
                    return Err(ListenError::AddressUnavailable);
                }
                (binding.address(), Some(binding.address()))
            },
            _ => (Ipv4Address::LOOPBACK, None),
        };
        let selection = select_ipv4(destination, explicit_source).map_err(|error| match error {
            ConnectError::NoRoute
            | ConnectError::SourceUnavailable
            | ConnectError::InterfaceUnavailable
            | ConnectError::Stack(_) => ListenError::AddressUnavailable,
        })?;
        self.stack
            .listen_tcp_endpoint(self.id(), selection.interface, Ipv4Address::UNSPECIFIED)
            .map_err(ListenError::Stack)
    }

    pub(crate) fn claim_child(&self) -> Result<Option<TcpPendingChildPort>, TcpChildError> {
        Ok(self
            .stack
            .claim_tcp_pending_child(self.id())?
            .map(|child| TcpPendingChildPort {
                stack: self.stack.clone(),
                child: Some(child),
            }))
    }

    pub(crate) fn send(&self, bytes: &[u8]) -> Result<usize, TcpSendError> {
        self.stack.send_tcp_endpoint(self.id(), bytes)
    }

    pub(crate) fn receive(&self, maximum: usize) -> Result<TcpReceivePort, TcpReceiveError> {
        let reservation = self.stack.reserve_tcp_receive(self.id(), maximum)?;
        Ok(TcpReceivePort {
            stack: self.stack.clone(),
            reservation: Some(reservation),
        })
    }

    pub(crate) fn retire(mut self) {
        let endpoint = self
            .endpoint
            .take()
            .expect("TCP capability retired more than once");
        self.stack
            .retire_tcp_endpoint(endpoint)
            .expect("live TCP capability lost its owner before retirement");
    }
}

impl Drop for TcpEndpointPort {
    fn drop(&mut self) {
        let Some(endpoint) = self.endpoint.take() else {
            return;
        };
        // Cleanup is non-blocking and reclaim storage was reserved at Stack
        // construction. The move-only capability makes a stale identity an
        // owner invariant violation, not a recoverable cleanup outcome.
        self.stack
            .retire_tcp_endpoint(endpoint)
            .expect("dropped TCP capability lost its owner before retirement");
    }
}

impl TcpPendingChildPort {
    pub(crate) fn accept(mut self) -> Result<TcpEndpointPort, TcpChildError> {
        let child = self
            .child
            .take()
            .expect("TCP child capability resolved more than once");
        let endpoint = match self.stack.take_tcp_child(child) {
            Ok(endpoint) => endpoint,
            Err(error) => {
                self.child = Some(child);
                return Err(error);
            },
        };
        Ok(TcpEndpointPort {
            stack: self.stack.clone(),
            endpoint: Some(endpoint),
        })
    }
}

impl Drop for TcpPendingChildPort {
    fn drop(&mut self) {
        let Some(child) = self.child.take() else {
            return;
        };
        // Listener retirement or owner-driven closed-generation rearm may
        // already have subsumed this exact child. In either case a stale
        // result proves the old capability no longer owns an engine slot.
        let _ = self.stack.cancel_tcp_child(child);
    }
}

impl TcpReceivePort {
    pub(crate) fn bytes(&self) -> &[u8] {
        self.reservation
            .as_ref()
            .expect("resolved TCP receive capability has no bytes")
            .bytes()
    }

    pub(crate) fn commit(mut self, prefix: usize) -> Result<(), TcpReceiveResolveError> {
        let reservation = self
            .reservation
            .take()
            .expect("TCP receive capability resolved more than once");
        if prefix > reservation.bytes().len() {
            self.reservation = Some(reservation);
            return Err(TcpReceiveResolveError::InvalidPrefix);
        }
        let (id, bytes) = reservation.into_owner_parts();
        match self.stack.resolve_tcp_receive(id, prefix) {
            Ok(()) => Ok(()),
            Err(error) => {
                self.reservation = Some(TcpReceiveReservation::from_owner_reservation(id, bytes));
                Err(error)
            },
        }
    }
}

impl Drop for TcpReceivePort {
    fn drop(&mut self) {
        let Some(reservation) = self.reservation.take() else {
            return;
        };
        let (id, _) = reservation.into_owner_parts();
        self.stack
            .resolve_tcp_receive(id, 0)
            .expect("dropped TCP receive reservation lost its owner before rollback");
    }
}

struct SelectedIpv4 {
    interface: anemone_net_api::InterfaceId,
    source: Ipv4Address,
}

fn select_ipv4(
    destination: Ipv4Address,
    explicit_source: Option<Ipv4Address>,
) -> Result<SelectedIpv4, ConnectError> {
    let selection = {
        let authority = ACTIVE_PATHS.lock();
        authority
            .domain
            .control_plane()
            .expect("published TCP capability lost its control plane")
            .select(destination, explicit_source)
            .map_err(|error| match error {
                SelectionError::NoRoute => ConnectError::NoRoute,
                SelectionError::SourceUnavailable => ConnectError::SourceUnavailable,
                SelectionError::InterfaceUnavailable => ConnectError::InterfaceUnavailable,
            })?
    };
    Ok(SelectedIpv4 {
        interface: selection.interface(),
        source: selection.source(),
    })
}

fn owns_local_address(address: Ipv4Address) -> bool {
    let authority = ACTIVE_PATHS.lock();
    authority
        .domain
        .control_plane()
        .expect("published TCP capability lost its control plane")
        .owns_local_address(address)
}
