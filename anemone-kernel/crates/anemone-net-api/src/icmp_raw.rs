//! Protocol-domain IPv4 ICMP raw endpoint values shared by the Stack and
//! kernel.
//!
//! Linux socket representation, credentials, fd/task state, waiters, route
//! policy, and smoltcp objects deliberately do not cross this boundary.

use alloc::vec::Vec;

use crate::Ipv4Address;

/// Opaque boot-local ICMP raw endpoint identity.
///
/// The Stack never reuses the raw value. Identity permits owner lookup only;
/// it does not prove liveness or extend the endpoint lifetime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IcmpRawEndpointId(u64);

impl IcmpRawEndpointId {
    #[doc(hidden)]
    pub const fn from_owner_raw(raw: u64) -> Self {
        Self(raw)
    }
}

/// Point-in-time endpoint facts. Consumers must re-read them after every hint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IcmpRawEndpointFacts {
    live: bool,
    readable: bool,
    writable: bool,
}

impl IcmpRawEndpointFacts {
    #[doc(hidden)]
    pub const fn from_owner_snapshot(readable: bool, writable: bool) -> Self {
        Self {
            live: true,
            readable,
            writable,
        }
    }

    pub const fn is_live(self) -> bool {
        self.live
    }

    pub const fn is_readable(self) -> bool {
        self.readable
    }

    pub const fn is_writable(self) -> bool {
        self.writable
    }
}

/// Recheck-only hint for one endpoint. It carries no readiness or error truth.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IcmpRawEndpointInvalidation {
    endpoint: IcmpRawEndpointId,
}

impl IcmpRawEndpointInvalidation {
    #[doc(hidden)]
    pub const fn from_owner_transition(endpoint: IcmpRawEndpointId) -> Self {
        Self { endpoint }
    }

    #[doc(hidden)]
    pub const fn endpoint(self) -> IcmpRawEndpointId {
        self.endpoint
    }
}

/// Domain-wide endpoint policy fixed when the concrete Stack is constructed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IcmpRawNamespacePolicy {
    endpoint_capacity: usize,
}

impl IcmpRawNamespacePolicy {
    pub const fn new(endpoint_capacity: usize) -> Self {
        Self { endpoint_capacity }
    }

    pub const fn endpoint_capacity(self) -> usize {
        self.endpoint_capacity
    }
}

/// Bounded packet and byte storage reserved for one endpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IcmpRawEndpointLimits {
    tx_packet_capacity: usize,
    tx_byte_capacity: usize,
    rx_packet_capacity: usize,
    rx_byte_capacity: usize,
}

impl IcmpRawEndpointLimits {
    pub const fn new(
        tx_packet_capacity: usize,
        tx_byte_capacity: usize,
        rx_packet_capacity: usize,
        rx_byte_capacity: usize,
    ) -> Self {
        Self {
            tx_packet_capacity,
            tx_byte_capacity,
            rx_packet_capacity,
            rx_byte_capacity,
        }
    }

    pub const fn tx_packet_capacity(self) -> usize {
        self.tx_packet_capacity
    }

    pub const fn tx_byte_capacity(self) -> usize {
        self.tx_byte_capacity
    }

    pub const fn rx_packet_capacity(self) -> usize {
        self.rx_packet_capacity
    }

    pub const fn rx_byte_capacity(self) -> usize {
        self.rx_byte_capacity
    }
}

/// Sole committed local/peer receive association snapshot.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IcmpRawAssociation {
    local: Option<Ipv4Address>,
    peer: Option<Ipv4Address>,
}

impl IcmpRawAssociation {
    pub const fn new(local: Option<Ipv4Address>, peer: Option<Ipv4Address>) -> Self {
        Self { local, peer }
    }

    pub const fn local(self) -> Option<Ipv4Address> {
        self.local
    }

    pub const fn peer(self) -> Option<Ipv4Address> {
        self.peer
    }
}

/// ICMP-type rejection policy. Bits 0..31 reject the corresponding type.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IcmpRawTypeFilter {
    blocked_types: u32,
}

impl IcmpRawTypeFilter {
    pub const fn from_blocked_types(blocked_types: u32) -> Self {
        Self { blocked_types }
    }

    pub const fn blocked_types(self) -> u32 {
        self.blocked_types
    }

    #[doc(hidden)]
    pub const fn allows(self, icmp_type: u8) -> bool {
        icmp_type >= 32 || self.blocked_types & (1u32 << icmp_type) == 0
    }
}

/// Committed association and filter snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IcmpRawEndpointConfig {
    association: IcmpRawAssociation,
    filter: IcmpRawTypeFilter,
}

impl IcmpRawEndpointConfig {
    #[doc(hidden)]
    pub const fn from_owner_snapshot(
        association: IcmpRawAssociation,
        filter: IcmpRawTypeFilter,
    ) -> Self {
        Self {
            association,
            filter,
        }
    }

    pub const fn association(self) -> IcmpRawAssociation {
        self.association
    }

    pub const fn filter(self) -> IcmpRawTypeFilter {
        self.filter
    }
}

/// Immutable IPv4 header policy for one send operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IcmpRawEgressPolicy {
    ttl: u8,
    tos: u8,
}

impl IcmpRawEgressPolicy {
    pub const fn new(ttl: u8, tos: u8) -> Option<Self> {
        if ttl == 0 {
            return None;
        }
        Some(Self { ttl, tos })
    }

    pub const fn ttl(self) -> u8 {
        self.ttl
    }

    pub const fn tos(self) -> u8 {
        self.tos
    }
}

/// Complete original IPv4 datagram detached from protocol-owner storage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IcmpRawReceivedPacket {
    bytes: Vec<u8>,
}

impl IcmpRawReceivedPacket {
    #[doc(hidden)]
    pub fn from_owner_detach(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Owner-local resource diagnostics. These counters never drive admission.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IcmpRawDropDiagnostics {
    rx_capacity_packets: u64,
    rx_capacity_bytes: u64,
}

impl IcmpRawDropDiagnostics {
    #[doc(hidden)]
    pub const fn from_owner_snapshot(rx_capacity_packets: u64, rx_capacity_bytes: u64) -> Self {
        Self {
            rx_capacity_packets,
            rx_capacity_bytes,
        }
    }

    pub const fn rx_capacity_packets(self) -> u64 {
        self.rx_capacity_packets
    }

    pub const fn rx_capacity_bytes(self) -> u64 {
        self.rx_capacity_bytes
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IcmpRawCreateError {
    EndpointCapacity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IcmpRawMutationError {
    UnknownEndpoint,
    InvalidAssociation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IcmpRawQueryError {
    UnknownEndpoint,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IcmpRawSendError {
    UnknownEndpoint,
    UnknownInterface,
    UnsupportedSource,
    InvalidDestination,
    MessageTooLong { maximum: usize },
    WouldBlock,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IcmpRawReceiveError {
    UnknownEndpoint,
    WouldBlock,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IcmpRawRetireError {
    UnknownEndpoint,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_filter_only_blocks_representable_types() {
        let filter = IcmpRawTypeFilter::from_blocked_types(1 << 8);
        assert!(!filter.allows(8));
        assert!(filter.allows(0));
        assert!(filter.allows(32));
    }

    #[test]
    fn zero_ttl_is_not_a_well_formed_egress_policy() {
        assert_eq!(IcmpRawEgressPolicy::new(0, 0), None);
        assert_eq!(IcmpRawEgressPolicy::new(64, 0).unwrap().ttl(), 64);
    }
}
