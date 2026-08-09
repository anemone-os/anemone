//! Protocol-domain TCP values shared by the Stack owner and kernel capability.
//!
//! This module contains no Linux ABI, fd, task, waiter, or smoltcp object. All
//! identities remain opaque and only become useful when presented back to the
//! concrete Stack owner that issued them.

use alloc::vec::Vec;

use crate::{InterfaceId, Ipv4Address};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TcpEndpointId(u64);

impl TcpEndpointId {
    #[doc(hidden)]
    pub const fn from_owner_raw(raw: u64) -> Self {
        Self(raw)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TcpPendingChild {
    listener: TcpEndpointId,
    slot: usize,
    generation: u64,
}

impl TcpPendingChild {
    #[doc(hidden)]
    pub const fn from_owner_observation(
        listener: TcpEndpointId,
        slot: usize,
        generation: u64,
    ) -> Self {
        Self {
            listener,
            slot,
            generation,
        }
    }

    #[doc(hidden)]
    pub const fn owner_parts(self) -> (TcpEndpointId, usize, u64) {
        (self.listener, self.slot, self.generation)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TcpReceiveReservationId(u64);

impl TcpReceiveReservationId {
    #[doc(hidden)]
    pub const fn from_owner_raw(raw: u64) -> Self {
        Self(raw)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct TcpReceiveReservation {
    id: TcpReceiveReservationId,
    bytes: Vec<u8>,
}

impl TcpReceiveReservation {
    #[doc(hidden)]
    pub fn from_owner_reservation(id: TcpReceiveReservationId, bytes: Vec<u8>) -> Self {
        Self { id, bytes }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[doc(hidden)]
    pub fn into_owner_parts(self) -> (TcpReceiveReservationId, Vec<u8>) {
        (self.id, self.bytes)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TcpBindRequest {
    address: Ipv4Address,
    port: u16,
}

/// A listener admission limit already normalized by the Socket ABI owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TcpListenBacklog(usize);

impl TcpListenBacklog {
    pub const fn new(normalized: usize) -> Self {
        Self(normalized)
    }

    pub const fn get(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TcpReceiveMode {
    Consume,
    Peek,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TcpShutdownDirection {
    Read,
    Write,
    ReadWrite,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TcpShutdownOutcome {
    Changed,
    Unchanged,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TcpReleaseReason {
    CreationRollback,
    AcceptedChildRollback,
    ListenerWithdrawal,
    FinalRelease,
}

/// Owner-normalized protocol state for read-only diagnostics.
///
/// This deliberately carries no Linux numeric state, smoltcp handle, endpoint
/// identity, task, or file association. The Socket ABI adapter performs the
/// final UAPI mapping after the Stack owner releases its observation window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TcpDiagnosticState {
    Closed,
    Listen,
    SynSent,
    SynReceived,
    Established,
    FinWait1,
    FinWait2,
    CloseWait,
    Closing,
    LastAck,
    TimeWait,
}

/// One immutable TCP fact projected by the Stack owner for a single dump.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TcpDiagnosticRecord {
    interface: InterfaceId,
    state: TcpDiagnosticState,
    local: TcpLocalBinding,
    peer: Option<TcpPeer>,
    receive_queue: usize,
    send_queue: usize,
}

impl TcpDiagnosticRecord {
    #[doc(hidden)]
    pub const fn from_owner_snapshot(
        interface: InterfaceId,
        state: TcpDiagnosticState,
        local: TcpLocalBinding,
        peer: Option<TcpPeer>,
        receive_queue: usize,
        send_queue: usize,
    ) -> Self {
        Self {
            interface,
            state,
            local,
            peer,
            receive_queue,
            send_queue,
        }
    }

    pub const fn interface(self) -> InterfaceId {
        self.interface
    }

    pub const fn state(self) -> TcpDiagnosticState {
        self.state
    }

    pub const fn local(self) -> TcpLocalBinding {
        self.local
    }

    pub const fn peer(self) -> Option<TcpPeer> {
        self.peer
    }

    pub const fn receive_queue(self) -> usize {
        self.receive_queue
    }

    pub const fn send_queue(self) -> usize {
        self.send_queue
    }
}

impl TcpBindRequest {
    pub const fn new(address: Ipv4Address, port: u16) -> Self {
        Self { address, port }
    }

    pub const fn address(self) -> Ipv4Address {
        self.address
    }

    pub const fn port(self) -> u16 {
        self.port
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TcpLocalBinding {
    address: Ipv4Address,
    port: u16,
}

impl TcpLocalBinding {
    #[doc(hidden)]
    pub const fn from_owner_commit(address: Ipv4Address, port: u16) -> Self {
        assert!(port != 0, "committed TCP port must be nonzero");
        Self { address, port }
    }

    pub const fn address(self) -> Ipv4Address {
        self.address
    }

    pub const fn port(self) -> u16 {
        self.port
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TcpPeer {
    address: Ipv4Address,
    port: u16,
}

impl TcpPeer {
    pub const fn new(address: Ipv4Address, port: u16) -> Self {
        Self { address, port }
    }

    pub const fn address(self) -> Ipv4Address {
        self.address
    }

    pub const fn port(self) -> u16 {
        self.port
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TcpPendingError {
    ConnectionRefused,
    ConnectionReset,
    TimedOut,
}

/// Point-in-time active-open fact owned by the TCP Stack.
///
/// This is an observation, not a second protocol state machine. Callers must
/// obtain a fresh `TcpEndpointFacts` after every recheck hint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TcpConnectFact {
    Connecting,
    Connected,
    Failed,
}

/// Point-in-time facts for one Stack-owned TCP connection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TcpConnectionFacts {
    connect: TcpConnectFact,
    send_capacity: usize,
    received_bytes: usize,
    pending_error: bool,
    local_read_shutdown: bool,
    local_write_shutdown: bool,
    peer_receive_closed: bool,
    terminal: bool,
}

impl TcpConnectionFacts {
    #[doc(hidden)]
    #[allow(clippy::too_many_arguments)]
    pub const fn from_owner_snapshot(
        connect: TcpConnectFact,
        send_capacity: usize,
        received_bytes: usize,
        pending_error: bool,
        local_read_shutdown: bool,
        local_write_shutdown: bool,
        peer_receive_closed: bool,
        terminal: bool,
    ) -> Self {
        Self {
            connect,
            send_capacity,
            received_bytes,
            pending_error,
            local_read_shutdown,
            local_write_shutdown,
            peer_receive_closed,
            terminal,
        }
    }

    pub const fn connect(self) -> TcpConnectFact {
        self.connect
    }

    pub const fn send_capacity(self) -> usize {
        self.send_capacity
    }

    pub const fn received_bytes(self) -> usize {
        self.received_bytes
    }

    pub const fn has_pending_error(self) -> bool {
        self.pending_error
    }

    pub const fn is_local_read_shutdown(self) -> bool {
        self.local_read_shutdown
    }

    pub const fn is_local_write_shutdown(self) -> bool {
        self.local_write_shutdown
    }

    pub const fn is_peer_receive_closed(self) -> bool {
        self.peer_receive_closed
    }

    pub const fn is_terminal(self) -> bool {
        self.terminal
    }
}

/// Role-aware point-in-time facts for one boot-unique TCP Endpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TcpEndpointFacts {
    Idle,
    Bound,
    Listener { has_pending_child: bool },
    Connection(TcpConnectionFacts),
}

/// Recheck-only evidence that an Endpoint fact may have changed.
///
/// It carries no readiness, errno, operation result, or lifecycle authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TcpEndpointInvalidation(TcpEndpointId);

impl TcpEndpointInvalidation {
    #[doc(hidden)]
    pub const fn from_owner_transition(endpoint: TcpEndpointId) -> Self {
        Self(endpoint)
    }

    pub const fn endpoint(self) -> TcpEndpointId {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TcpConnectResult {
    Idle,
    Bound(TcpLocalBinding),
    Connecting {
        local: TcpLocalBinding,
        peer: TcpPeer,
    },
    Connected {
        local: TcpLocalBinding,
        peer: TcpPeer,
    },
    Failed(TcpPendingError),
    Terminal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TcpCreateError {
    EndpointCapacity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TcpBindError {
    UnknownEndpoint,
    WrongRole,
    PortInUse,
    EphemeralPortsExhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TcpConnectError {
    UnknownEndpoint,
    WrongRole,
    InvalidPeer,
    UnknownInterface,
    UnsupportedSource,
    EngineCapacity,
    PortInUse,
    EphemeralPortsExhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TcpListenError {
    UnknownEndpoint,
    UnknownInterface,
    WrongRole,
    EngineCapacity,
    PortInUse,
    EphemeralPortsExhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TcpChildError {
    UnknownEndpoint,
    WrongRole,
    StaleChild,
    ChildNotCompleted,
    EndpointCapacity,
    EngineCapacity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TcpQueryError {
    UnknownEndpoint,
    WrongRole,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TcpReceiveResolveError {
    UnknownReservation,
    InvalidPrefix,
}

#[derive(Debug, Eq, PartialEq)]
pub enum TcpStreamReceiveOutcome {
    Data(TcpReceiveReservation),
    EndOfStream,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TcpStreamReceiveError {
    UnknownEndpoint,
    NotConnected,
    WouldBlock,
    ReservationOutstanding,
    ConnectionRefused,
    ConnectionReset,
    TimedOut,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TcpStreamSendError {
    UnknownEndpoint,
    NotConnected,
    WouldBlock,
    BrokenStream,
    ConnectionRefused,
    ConnectionReset,
    TimedOut,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TcpShutdownError {
    UnknownEndpoint,
    NotConnected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TcpStreamObservation {
    send_capacity: usize,
    has_received_bytes: bool,
    may_receive: bool,
    has_pending_error: bool,
    end_of_stream: bool,
}

impl TcpStreamObservation {
    #[doc(hidden)]
    pub const fn from_owner_fact(
        send_capacity: usize,
        has_received_bytes: bool,
        may_receive: bool,
        has_pending_error: bool,
        end_of_stream: bool,
    ) -> Self {
        Self {
            send_capacity,
            has_received_bytes,
            may_receive,
            has_pending_error,
            end_of_stream,
        }
    }

    pub const fn send_capacity(self) -> usize {
        self.send_capacity
    }

    pub const fn has_received_bytes(self) -> bool {
        self.has_received_bytes
    }

    pub const fn may_receive(self) -> bool {
        self.may_receive
    }

    pub const fn has_pending_error(self) -> bool {
        self.has_pending_error
    }

    pub const fn end_of_stream(self) -> bool {
        self.end_of_stream
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TcpRetireError {
    UnknownEndpoint,
}
