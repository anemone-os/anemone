//! Kernel-local network-device publication and provider handoff.

mod provider;
mod registry;

pub(crate) use provider::{NetdevFrameProvider, RecheckWake};
pub use registry::NetdevSnapshot;
pub(crate) use registry::{
    PendingAttachProvider, PublishError, PublishedNetdev, ReadyNetdev, publish, retain_pending,
    take_pending,
};
