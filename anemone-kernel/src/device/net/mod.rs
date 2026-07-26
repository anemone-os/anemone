//! Kernel-local network-device publication and provider handoff.

mod provider;
mod registry;

pub(crate) use provider::{NetdevFrameProvider, RecheckWake};
pub use registry::NetdevSnapshot;
pub(crate) use registry::{PublishError, PublishedNetdev, ReadyNetdev, publish};
