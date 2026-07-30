//! Protocol-domain UDP endpoint values shared by the Stack and kernel owner.
//!
//! These types deliberately contain no Linux ABI, fd, task, route-policy, or
//! smoltcp details. The concrete Stack remains the sole endpoint and binding
//! authority.

use alloc::vec::Vec;

use crate::{InterfaceId, Ipv4Address};

/// Opaque boot-local UDP endpoint identity.
///
/// The Stack never reuses the raw value. Consumers may only use this value for
/// lookup through an owner capability; it carries no liveness by itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UdpEndpointId(u64);

impl UdpEndpointId {
    /// Construct an identity at the concrete Stack allocation boundary.
    ///
    /// This is public only because the Stack and value type are separate
    /// crates. Kernel consumers must obtain identities from Stack creation.
    #[doc(hidden)]
    pub const fn from_owner_raw(raw: u64) -> Self {
        Self(raw)
    }
}

/// Domain-wide UDP endpoint and ephemeral-port policy, fixed when the owning
/// Stack is constructed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UdpNamespacePolicy {
    endpoint_capacity: usize,
    ephemeral_port_first: u16,
    ephemeral_port_last: u16,
}

impl UdpNamespacePolicy {
    pub const fn new(
        endpoint_capacity: usize,
        ephemeral_port_first: u16,
        ephemeral_port_last: u16,
    ) -> Self {
        Self {
            endpoint_capacity,
            ephemeral_port_first,
            ephemeral_port_last,
        }
    }

    pub const fn endpoint_capacity(self) -> usize {
        self.endpoint_capacity
    }

    pub const fn ephemeral_port_first(self) -> u16 {
        self.ephemeral_port_first
    }

    pub const fn ephemeral_port_last(self) -> u16 {
        self.ephemeral_port_last
    }
}

/// Bounded resources reserved for one endpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UdpEndpointLimits {
    tx_datagram_capacity: usize,
    rx_datagram_capacity: usize,
    max_payload_bytes: usize,
}

impl UdpEndpointLimits {
    pub const fn new(
        tx_datagram_capacity: usize,
        rx_datagram_capacity: usize,
        max_payload_bytes: usize,
    ) -> Self {
        Self {
            tx_datagram_capacity,
            rx_datagram_capacity,
            max_payload_bytes,
        }
    }

    pub const fn tx_datagram_capacity(self) -> usize {
        self.tx_datagram_capacity
    }

    pub const fn rx_datagram_capacity(self) -> usize {
        self.rx_datagram_capacity
    }

    pub const fn max_payload_bytes(self) -> usize {
        self.max_payload_bytes
    }
}

/// Requested local address constraint and port. Port zero requests ephemeral
/// allocation in the same Stack transaction as conflict checking and commit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UdpBindRequest {
    address: Ipv4Address,
    port: u16,
}

impl UdpBindRequest {
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

/// Committed binding snapshot. Its port is always nonzero.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UdpLocalBinding {
    address: Ipv4Address,
    port: u16,
}

impl UdpLocalBinding {
    #[doc(hidden)]
    pub const fn from_owner_commit(address: Ipv4Address, port: u16) -> Self {
        assert!(port != 0, "committed UDP port must be nonzero");
        Self { address, port }
    }

    pub const fn address(self) -> Ipv4Address {
        self.address
    }

    pub const fn port(self) -> u16 {
        self.port
    }
}

/// Point-in-time route/source selection passed into the protocol owner for
/// commit-time revalidation. It carries no route-table or wake capability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UdpEgressSelection {
    interface: InterfaceId,
    source: Ipv4Address,
}

impl UdpEgressSelection {
    pub const fn new(interface: InterfaceId, source: Ipv4Address) -> Self {
        Self { interface, source }
    }

    pub const fn interface(self) -> InterfaceId {
        self.interface
    }

    pub const fn source(self) -> Ipv4Address {
        self.source
    }
}

/// Protocol-domain IPv4 peer endpoint, independent of Linux sockaddr layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UdpPeer {
    address: Ipv4Address,
    port: u16,
}

impl UdpPeer {
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

/// Datagram detached from the protocol owner before any user-memory copyout.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UdpReceivedDatagram {
    payload: Vec<u8>,
    peer: UdpPeer,
}

impl UdpReceivedDatagram {
    #[doc(hidden)]
    pub fn from_owner_detach(payload: Vec<u8>, peer: UdpPeer) -> Self {
        Self { payload, peer }
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    pub const fn peer(&self) -> UdpPeer {
        self.peer
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UdpCreateError {
    EndpointCapacity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UdpBindError {
    UnknownEndpoint,
    AlreadyBound,
    PortInUse,
    EphemeralPortsExhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UdpQueryError {
    UnknownEndpoint,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UdpSendError {
    UnknownEndpoint,
    UnboundEndpoint,
    UnknownInterface,
    UnsupportedSource,
    InvalidDestination,
    MessageTooLong { maximum: usize },
    WouldBlock,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UdpReceiveError {
    UnknownEndpoint,
    WouldBlock,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UdpRetireError {
    UnknownEndpoint,
}
