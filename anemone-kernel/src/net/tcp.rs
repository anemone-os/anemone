//! Kernel-private capability for the initial-domain TCP owner.
//!
//! No Socket profile, descriptor, fd, syscall, user pointer, or wait route
//! crosses this fence; callers use only normalized operations and lifecycle
//! reasons.

use anemone_net_api::{
    Ipv4Address, Ipv4EgressSelection,
    tcp::{
        TcpBindError, TcpBindRequest, TcpChildError, TcpConnectError, TcpConnectResult,
        TcpCreateError, TcpEndpointFacts, TcpEndpointId, TcpListenBacklog, TcpListenError,
        TcpLocalBinding, TcpPeer, TcpPendingError, TcpQueryError, TcpReceiveMode,
        TcpReceiveReservation, TcpReceiveResolveError, TcpReleaseReason, TcpShutdownDirection,
        TcpShutdownError, TcpShutdownOutcome, TcpStreamObservation, TcpStreamReceiveError,
        TcpStreamReceiveOutcome, TcpStreamSendError,
    },
};
use anemone_smoltcp_stack::TcpPolicy;

use crate::{kconfig_defs::*, prelude::*};

use super::{
    ACTIVE_PATHS,
    domain::{DomainStack, SelectionError},
};

pub(crate) use super::EventRegistrationError;

pub(in crate::net) const TCP_POLICY: TcpPolicy = TcpPolicy::new(
    NET_TCP_ENDPOINT_CAPACITY,
    NET_TCP_ENGINE_TIMER_CAPACITY,
    NET_TCP_LISTENER_COMPLETED_CAPACITY,
    NET_TCP_LISTENER_PROJECTION_CAPACITY,
    NET_TCP_RX_BUFFER_BYTES,
    NET_TCP_TX_BUFFER_BYTES,
    NET_TCP_MIN_RX_BUFFER_BYTES,
    NET_TCP_MIN_TX_BUFFER_BYTES,
    NET_TCP_DEFERRED_RECLAIM_CAPACITY,
    NET_TCP_CONNECT_TIMEOUT_MS,
    NET_TCP_ORPHAN_TIMEOUT_MS,
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

const fn listener_projection_storage_fits() -> bool {
    let Some(published) =
        NET_TCP_LISTENER_PROJECTION_CAPACITY.checked_mul(NET_TCP_LISTENER_COMPLETED_CAPACITY)
    else {
        return false;
    };
    let Some(with_handoff_headroom) = published.checked_add(1) else {
        return false;
    };
    with_handoff_headroom <= NET_TCP_ENGINE_TIMER_CAPACITY
        && with_handoff_headroom <= NET_TCP_DEFERRED_RECLAIM_CAPACITY
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
    NET_TCP_LISTENER_PROJECTION_CAPACITY >= 2,
    "net_tcp_listener_projection_capacity must cover local and one external ingress path"
);
static_assert!(
    listener_projection_storage_fits(),
    "TCP engine and reclaim capacity must fit every listener projection plus handoff headroom"
);
static_assert!(
    NET_TCP_RX_BUFFER_BYTES > 0 && NET_TCP_TX_BUFFER_BYTES > 0,
    "TCP engine buffers must be nonzero"
);
static_assert!(
    NET_TCP_RX_BUFFER_BYTES <= i32::MAX as usize && NET_TCP_TX_BUFFER_BYTES <= i32::MAX as usize,
    "TCP engine buffers must remain representable by Linux getsockopt int"
);
static_assert!(
    NET_TCP_MIN_RX_BUFFER_BYTES > 0 && NET_TCP_MIN_RX_BUFFER_BYTES <= NET_TCP_RX_BUFFER_BYTES,
    "TCP minimum receive budget must fit the receive ring"
);
static_assert!(
    NET_TCP_MIN_TX_BUFFER_BYTES > 0 && NET_TCP_MIN_TX_BUFFER_BYTES <= NET_TCP_TX_BUFFER_BYTES,
    "TCP minimum send budget must fit the send ring"
);
static_assert!(
    NET_TCP_DEFERRED_RECLAIM_CAPACITY >= NET_TCP_ENGINE_TIMER_CAPACITY,
    "TCP reclaim storage must reserve one infallible slot for every engine"
);
static_assert!(
    NET_TCP_CONNECT_TIMEOUT_MS > 0 && NET_TCP_CONNECT_TIMEOUT_MS <= (i64::MAX as usize) / 1_000,
    "net_tcp_connect_timeout_ms must form a positive signed-microsecond duration"
);
static_assert!(
    NET_TCP_ORPHAN_TIMEOUT_MS > 0 && NET_TCP_ORPHAN_TIMEOUT_MS <= (i64::MAX as usize) / 1_000,
    "net_tcp_orphan_timeout_ms must form a positive signed-microsecond duration"
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
    access: TcpEndpointAccessPort,
    endpoint: Option<TcpEndpointId>,
}

/// Cloneable operation capability without Endpoint lifecycle authority.
///
/// A Socket source may copy this handle out of its publication guard before a
/// Stack mutation. Final release owns the separate move-only `TcpEndpointPort`;
/// a racing access simply observes `UnknownEndpoint` after that retirement.
#[derive(Clone)]
pub(crate) struct TcpEndpointAccessPort {
    stack: Arc<DomainStack>,
    endpoint: TcpEndpointId,
}

pub(crate) trait TcpEndpointInvalidationObserver: Send + Sync {
    fn invalidate(&self);
}

/// Source-owned proof that one weak reverse route is published.
///
/// The boot-unique Endpoint identity selects only the route to withdraw; it
/// carries no readiness, protocol phase, or lifecycle decision.
pub(crate) struct TcpEndpointEventRegistration {
    stack: Arc<DomainStack>,
    endpoint: TcpEndpointId,
    active: bool,
}

impl TcpEndpointEventRegistration {
    pub(crate) fn unregister(mut self) {
        self.stack.unregister_tcp_endpoint_observer(self.endpoint);
        self.active = false;
    }
}

impl Drop for TcpEndpointEventRegistration {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        self.stack.unregister_tcp_endpoint_observer(self.endpoint);
        self.active = false;
        assert!(
            false,
            "TCP endpoint event registration dropped while active"
        );
    }
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

pub(crate) enum TcpReceiveOutcome {
    Data(TcpReceivePort),
    EndOfStream,
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
        access: TcpEndpointAccessPort { stack, endpoint },
        endpoint: Some(endpoint),
    })
}

impl TcpEndpointPort {
    pub(crate) fn access(&self) -> TcpEndpointAccessPort {
        self.access.clone()
    }

    pub(crate) fn release(mut self, reason: TcpReleaseReason) {
        self.release_inner(reason);
    }

    fn release_inner(&mut self, reason: TcpReleaseReason) {
        let endpoint = self
            .endpoint
            .take()
            .expect("TCP capability retired more than once");
        self.access
            .stack
            .release_tcp_endpoint(endpoint, reason)
            .expect("live TCP capability lost its owner before retirement");
    }
}

impl TcpEndpointAccessPort {
    fn id(&self) -> TcpEndpointId {
        self.endpoint
    }

    pub(crate) fn register_invalidation_observer(
        &self,
        observer: &Arc<dyn TcpEndpointInvalidationObserver>,
    ) -> Result<TcpEndpointEventRegistration, EventRegistrationError> {
        let endpoint = self.id();
        self.stack
            .register_tcp_endpoint_observer(endpoint, observer)?;
        Ok(TcpEndpointEventRegistration {
            stack: self.stack.clone(),
            endpoint,
            active: true,
        })
    }

    pub(crate) fn facts(&self) -> Result<TcpEndpointFacts, TcpQueryError> {
        self.stack.tcp_endpoint_facts(self.id())
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

    pub(crate) fn reuse_address(&self) -> Result<bool, TcpBindError> {
        self.stack.tcp_reuse_address(self.id())
    }

    pub(crate) fn set_reuse_address(&self, enabled: bool) -> Result<(), TcpBindError> {
        self.stack.set_tcp_reuse_address(self.id(), enabled)
    }

    pub(crate) fn no_delay(&self) -> Result<bool, TcpQueryError> {
        self.stack.tcp_no_delay(self.id())
    }

    pub(crate) fn set_no_delay(&self, enabled: bool) -> Result<(), TcpQueryError> {
        self.stack.set_tcp_no_delay(self.id(), enabled)
    }

    pub(crate) fn receive_buffer(&self) -> Result<usize, TcpQueryError> {
        self.stack.tcp_receive_buffer(self.id())
    }

    pub(crate) fn send_buffer(&self) -> Result<usize, TcpQueryError> {
        self.stack.tcp_send_buffer(self.id())
    }

    pub(crate) fn set_receive_buffer_hint(&self, requested: usize) -> Result<(), TcpQueryError> {
        self.stack.set_tcp_receive_buffer_hint(self.id(), requested)
    }

    pub(crate) fn set_send_buffer_hint(&self, requested: usize) -> Result<(), TcpQueryError> {
        self.stack.set_tcp_send_buffer_hint(self.id(), requested)
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

    pub(crate) fn is_listening(&self) -> Result<bool, TcpQueryError> {
        self.stack.tcp_endpoint_is_listening(self.id())
    }

    pub(crate) fn peer(&self) -> Result<Option<TcpPeer>, TcpQueryError> {
        self.stack.tcp_endpoint_peer(self.id())
    }

    pub(crate) fn connect_result(&self) -> Result<TcpConnectResult, TcpQueryError> {
        self.stack.tcp_connect_result(self.id())
    }

    pub(crate) fn consume_pending_error(&self) -> Result<Option<TcpPendingError>, TcpQueryError> {
        self.stack.consume_tcp_pending_error(self.id())
    }

    pub(crate) fn stream_observation(&self) -> Result<TcpStreamObservation, TcpQueryError> {
        self.stack.observe_tcp_stream(self.id())
    }

    pub(crate) fn listen(&self) -> Result<(), ListenError> {
        self.listen_with_backlog(TcpListenBacklog::new(NET_TCP_LISTENER_COMPLETED_CAPACITY))
    }

    pub(crate) fn listen_with_backlog(&self, backlog: TcpListenBacklog) -> Result<(), ListenError> {
        let binding = self.binding().map_err(|error| match error {
            TcpQueryError::UnknownEndpoint | TcpQueryError::WrongRole => {
                ListenError::Stack(TcpListenError::UnknownEndpoint)
            },
        })?;
        if binding.is_some_and(|binding| {
            !binding.address().is_unspecified() && !owns_local_address(binding.address())
        }) {
            return Err(ListenError::AddressUnavailable);
        }
        self.stack
            .listen_tcp_endpoint_with_backlog(self.id(), Ipv4Address::UNSPECIFIED, backlog)
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

    pub(crate) fn send_stream(&self, bytes: &[u8]) -> Result<usize, TcpStreamSendError> {
        self.stack.send_tcp_stream(self.id(), bytes)
    }

    pub(crate) fn receive_stream(
        &self,
        maximum: usize,
        mode: TcpReceiveMode,
    ) -> Result<TcpReceiveOutcome, TcpStreamReceiveError> {
        match self.stack.receive_tcp_stream(self.id(), maximum, mode)? {
            TcpStreamReceiveOutcome::Data(reservation) => {
                Ok(TcpReceiveOutcome::Data(TcpReceivePort {
                    stack: self.stack.clone(),
                    reservation: Some(reservation),
                }))
            },
            TcpStreamReceiveOutcome::EndOfStream => Ok(TcpReceiveOutcome::EndOfStream),
        }
    }

    pub(crate) fn shutdown(
        &self,
        direction: TcpShutdownDirection,
    ) -> Result<TcpShutdownOutcome, TcpShutdownError> {
        self.stack.shutdown_tcp_endpoint(self.id(), direction)
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
        self.access
            .stack
            .release_tcp_endpoint(endpoint, TcpReleaseReason::CreationRollback)
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
            access: TcpEndpointAccessPort {
                stack: self.stack.clone(),
                endpoint,
            },
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
