#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod adapter;
mod icmp_raw;
mod local_link;
mod pump;
mod stack;
mod tcp;
mod udp;

pub use pump::PumpBudget;
pub use stack::{
    Ipv4ConfigError, ProtocolProgression, PumpError, Stack, StackInvalidations, StackPolicy,
};
pub use tcp::TcpPolicy;

#[cfg(feature = "host-test")]
pub use stack::{
    HostEndpointCreateError, HostEndpointId, HostEndpointObservation, HostLocalLinkObservation,
    HostPeer, HostReceivedDatagram, HostRetireError, HostSelection, HostSendError,
};
