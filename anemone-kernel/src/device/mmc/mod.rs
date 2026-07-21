//! MMC host, cold-discovery, and published-card infrastructure.
//!
//! Host-controller MMIO remains behind the protocol-neutral `MmcHost`
//! contract. Protocol discovery constructs immutable card identity, and the
//! card bus dispatches concrete endpoint drivers from that committed identity.

mod bus;
mod card;
mod discovery;
mod host;
mod registry;

pub use bus::*;
pub use card::*;
pub use discovery::*;
pub use host::*;
pub use registry::*;
