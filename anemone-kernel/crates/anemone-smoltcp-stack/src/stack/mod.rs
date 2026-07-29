use alloc::vec::Vec;

use anemone_net_api::InterfaceId;

use crate::{local_link::LocalPort, udp::UdpEndpoints};

#[cfg(feature = "host-test")]
mod host_validation;
mod interfaces;
mod udp;

#[cfg(feature = "host-test")]
pub use host_validation::{
    HostEndpointCreateError, HostEndpointId, HostEndpointObservation, HostLocalLinkObservation,
    HostReceivedDatagram, HostRetireError, HostSelection, HostSendError,
};

pub(crate) use interfaces::{InterfaceEntry, PumpOrder};

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
#[derive(Default)]
pub struct Stack {
    pub(crate) interfaces: Vec<InterfaceEntry>,
    // The local link and port remain private protocol projections. Route and
    // wake policy belong to the kernel control-plane and worker owners.
    pub(crate) local: Option<LocalPort>,
    pub(crate) udp: UdpEndpoints,
    pub(crate) next_interface_id: u32,
}

impl Stack {
    pub const fn new() -> Self {
        Self {
            interfaces: Vec::new(),
            local: None,
            udp: UdpEndpoints::new(),
            next_interface_id: 0,
        }
    }
}
