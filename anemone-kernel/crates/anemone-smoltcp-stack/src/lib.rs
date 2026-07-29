#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod adapter;
// Checkpoint 0B intentionally leaves this no_std candidate without a kernel
// consumer. Checkpoint 0C must either retain it for the resolved Stage 1 route
// or remove it; until then, kernel builds should not turn that boundary into
// misleading per-item dead-code noise.
#[allow(dead_code)]
mod local_link;
mod pump;
mod stack;
#[allow(dead_code)]
mod udp;

pub use pump::PumpBudget;
pub use stack::{PumpError, Stack};

#[cfg(feature = "host-test")]
pub use stack::{
    HostEndpointCreateError, HostEndpointId, HostEndpointObservation, HostLocalLinkObservation,
    HostReceivedDatagram, HostRetireError, HostSelection, HostSendError,
};
