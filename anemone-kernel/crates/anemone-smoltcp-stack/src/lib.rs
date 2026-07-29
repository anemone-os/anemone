#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod adapter;
mod local_link;
mod pump;
mod stack;
// The aggregate Endpoint owner remains dormant outside conditional validation
// until Stage 3 introduces its real kernel consumer.
#[allow(dead_code)]
mod udp;

pub use pump::PumpBudget;
pub use stack::{Ipv4ConfigError, PumpError, Stack};

#[cfg(feature = "udp-validation-probe")]
pub use stack::udp_probe;

#[cfg(feature = "host-test")]
pub use stack::{
    HostEndpointCreateError, HostEndpointId, HostEndpointObservation, HostLocalLinkObservation,
    HostReceivedDatagram, HostRetireError, HostSelection, HostSendError,
};
