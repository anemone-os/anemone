//! Protocol-domain UDP endpoint values shared by the Stack and kernel owner.
//!
//! These types deliberately contain no Linux ABI, fd, task, route-policy, or
//! smoltcp details. The concrete Stack remains the sole endpoint and binding
//! authority.

use crate::Ipv4Address;

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

/// Bounded resources reserved for one endpoint and the owning namespace.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UdpEndpointLimits {
    endpoint_capacity: usize,
    tx_datagram_capacity: usize,
    rx_datagram_capacity: usize,
    max_payload_bytes: usize,
}

impl UdpEndpointLimits {
    pub const fn new(
        endpoint_capacity: usize,
        tx_datagram_capacity: usize,
        rx_datagram_capacity: usize,
        max_payload_bytes: usize,
    ) -> Self {
        Self {
            endpoint_capacity,
            tx_datagram_capacity,
            rx_datagram_capacity,
            max_payload_bytes,
        }
    }

    pub const fn endpoint_capacity(self) -> usize {
        self.endpoint_capacity
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
pub enum UdpRetireError {
    UnknownEndpoint,
}
