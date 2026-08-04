#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod adapter;
mod icmp_raw;
mod local_link;
mod pump;
mod stack;
mod udp;

pub use pump::PumpBudget;
pub use stack::{Ipv4ConfigError, PumpError, Stack, StackInvalidations, StackPolicy};

#[cfg(feature = "host-test")]
pub use stack::{
    HostEndpointCreateError, HostEndpointId, HostEndpointObservation, HostLocalLinkObservation,
    HostPeer, HostReceivedDatagram, HostRetireError, HostSelection, HostSendError,
};
