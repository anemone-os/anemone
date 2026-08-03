use alloc::vec::Vec;

use anemone_net_api::{InterfaceId, icmp_raw::IcmpRawNamespacePolicy, udp::UdpNamespacePolicy};

use crate::local_link::LocalPort;

#[cfg(feature = "host-test")]
mod host_validation;
mod icmp_raw;
mod interfaces;
mod protocols;
mod udp;

#[cfg(feature = "host-test")]
pub use host_validation::{
    HostEndpointCreateError, HostEndpointId, HostEndpointObservation, HostLocalLinkObservation,
    HostReceivedDatagram, HostRetireError, HostSelection, HostSendError,
};

pub(crate) use interfaces::{InterfaceEntry, PumpOrder};
pub use protocols::StackInvalidations;
pub(crate) use protocols::{ActiveEgress, InterfaceProtocols, Protocols};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ipv4ConfigError {
    UnknownInterface(InterfaceId),
    LocalInterfaceAlreadyExists,
    MissingLocalInterface,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PumpError {
    UnknownInterface(InterfaceId),
}

/// Immutable protocol policies supplied when one Stack instance is created.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StackPolicy {
    udp: UdpNamespacePolicy,
    icmp_raw: IcmpRawNamespacePolicy,
}

impl StackPolicy {
    pub const fn new(udp: UdpNamespacePolicy, icmp_raw: IcmpRawNamespacePolicy) -> Self {
        Self { udp, icmp_raw }
    }
}

/// Owns the private smoltcp interface resources and their opaque ID mapping.
///
/// `&mut Stack` is the unique pump capability. The kernel wiring owner may put
/// the stack behind its chosen synchronization primitive, but admission,
/// contention, and requeue policy stay outside this protocol-state owner.
pub struct Stack {
    pub(crate) interfaces: Vec<InterfaceEntry>,
    // The local link and port remain private protocol projections. Route and
    // wake policy belong to the kernel control-plane and worker owners.
    pub(crate) local: Option<LocalPort>,
    pub(crate) protocols: Protocols,
    pub(crate) next_interface_id: u32,
}

impl Stack {
    /// Constructs the production protocol owner from immutable kernel policy.
    pub fn with_policy(policy: StackPolicy) -> Self {
        Self {
            interfaces: Vec::new(),
            local: None,
            protocols: Protocols::new(policy.udp, policy.icmp_raw),
            next_interface_id: 0,
        }
    }

    /// Drains all protocol recheck hints after one exclusive Stack window.
    pub fn take_invalidations(&mut self) -> StackInvalidations {
        self.protocols.take_invalidations()
    }

    /// Compatibility constructor for tests that do not exercise namespace
    /// policy. Production kernel code must supply generated policy explicitly.
    #[cfg(any(test, feature = "host-test"))]
    pub fn new() -> Self {
        Self::with_policy(StackPolicy::new(
            UdpNamespacePolicy::new(64, 32768, 60999),
            IcmpRawNamespacePolicy::new(64),
        ))
    }

    /// Host-only configuration path for deterministic namespace-policy tests.
    #[cfg(feature = "host-test")]
    pub fn new_for_host_validation(policy: UdpNamespacePolicy) -> Self {
        Self::with_policy(StackPolicy::new(policy, IcmpRawNamespacePolicy::new(64)))
    }
}

#[cfg(any(test, feature = "host-test"))]
impl Default for Stack {
    fn default() -> Self {
        Self::new()
    }
}
