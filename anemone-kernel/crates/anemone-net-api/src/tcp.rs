//! Protocol-domain TCP values shared by the Stack owner and kernel capability.
//!
//! This module contains no Linux ABI, fd, task, waiter, or smoltcp object. All
//! identities remain opaque and only become useful when presented back to the
//! concrete Stack owner that issued them.

use alloc::vec::Vec;

use crate::Ipv4Address;

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
pub enum TcpDisconnectCause {
    Reset,
    Timeout,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TcpConnectionObservation {
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
    Failed {
        local: TcpLocalBinding,
        peer: TcpPeer,
        cause: TcpDisconnectCause,
    },
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
pub enum TcpSendError {
    UnknownEndpoint,
    NotConnected,
    WouldBlock,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TcpReceiveError {
    UnknownEndpoint,
    NotConnected,
    WouldBlock,
    ReservationOutstanding,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TcpReceiveResolveError {
    UnknownReservation,
    InvalidPrefix,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TcpRetireError {
    UnknownEndpoint,
}
