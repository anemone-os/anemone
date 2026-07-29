#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod adapter;
// The production global Stack now owns this private no_std candidate, but a
// functional software link remains Stage 2 work. Remove the module allowance
// when Stage 2 wires or replaces that path.
#[allow(dead_code)]
mod local_link;
mod pump;
mod stack;
// The production global Stack now owns the aggregate Endpoint candidate, but
// Endpoint operations stay dormant until Stage 3 introduces their real
// consumer. Reassess the allowance at that gate.
#[allow(dead_code)]
mod udp;

pub use pump::PumpBudget;
pub use stack::{PumpError, Stack};

#[cfg(feature = "host-test")]
pub use stack::{
    HostEndpointCreateError, HostEndpointId, HostEndpointObservation, HostLocalLinkObservation,
    HostReceivedDatagram, HostRetireError, HostSelection, HostSendError,
};
