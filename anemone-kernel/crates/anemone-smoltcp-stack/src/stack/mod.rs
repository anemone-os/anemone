use alloc::vec::Vec;

use anemone_net_api::{InterfaceId, icmp_raw::IcmpRawNamespacePolicy, udp::UdpNamespacePolicy};

use crate::{icmp_raw::IcmpRawEndpoints, local_link::LocalPort, udp::UdpEndpoints};

#[cfg(feature = "host-test")]
mod host_validation;
mod icmp_raw;
mod interfaces;
mod udp;

#[cfg(feature = "host-test")]
pub use host_validation::{
    HostEndpointCreateError, HostEndpointId, HostEndpointObservation, HostLocalLinkObservation,
    HostReceivedDatagram, HostRetireError, HostSelection, HostSendError,
};

pub(crate) use interfaces::{EgressProtocol, InterfaceEntry, PumpOrder};

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
    pub(crate) icmp_raw: IcmpRawEndpoints,
    pub(crate) udp: UdpEndpoints,
    pub(crate) next_interface_id: u32,
}

impl Stack {
    /// Constructs the production protocol owner with one immutable UDP
    /// namespace policy supplied by the kernel configuration owner.
    pub fn new_with_namespace_policies(
        udp_policy: UdpNamespacePolicy,
        icmp_raw_policy: IcmpRawNamespacePolicy,
    ) -> Self {
        Self {
            interfaces: Vec::new(),
            local: None,
            icmp_raw: IcmpRawEndpoints::new(icmp_raw_policy),
            udp: UdpEndpoints::new(udp_policy),
            next_interface_id: 0,
        }
    }

    /// Compatibility constructor for tests that do not exercise namespace
    /// policy. Production kernel code must supply generated policy explicitly.
    #[cfg(any(test, feature = "host-test"))]
    pub fn new() -> Self {
        Self::new_with_namespace_policies(
            UdpNamespacePolicy::new(64, 32768, 60999),
            IcmpRawNamespacePolicy::new(64),
        )
    }

    /// Host-only configuration path for deterministic namespace-policy tests.
    #[cfg(feature = "host-test")]
    pub fn new_for_host_validation(policy: UdpNamespacePolicy) -> Self {
        Self::new_with_namespace_policies(policy, IcmpRawNamespacePolicy::new(64))
    }
}

#[cfg(any(test, feature = "host-test"))]
impl Default for Stack {
    fn default() -> Self {
        Self::new()
    }
}
