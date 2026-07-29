#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod adapter;
// Stage 0's positive decision retains this private no_std candidate as route
// input for the Stage 0 -> 1 resolution gate. The kernel still has no consumer,
// so keep the temporary module-wide allowance until Stage 1 either wires the
// production owner or replaces and removes the candidate.
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
