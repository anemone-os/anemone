use crate::prelude::*;

use super::control::KThreadControl;

/// Strong lifecycle capability for one ordinary kthread.
///
/// The handle does not expose the underlying `Task`, scheduler state, topology
/// mutation, or business request state. Synchronous stop is deliberately a
/// caller-side sequence of `request_stop()` followed by `wait_exited()`.
#[derive(Debug, Clone)]
pub struct KThreadHandle {
    pub(super) control: Arc<KThreadControl>,
}

impl KThreadHandle {
    pub(super) fn new(control: Arc<KThreadControl>) -> Self {
        Self { control }
    }

    pub fn request_stop(&self) {
        self.control.request_stop();
        self.control.wake();
    }

    /// Pure wake capability. Business request truth stays in the consumer.
    pub fn wake(&self) {
        self.control.wake();
    }

    pub fn wait_exited(&self) -> i32 {
        self.control.wait_exited()
    }

    pub fn has_exited(&self) -> bool {
        self.control.has_exited()
    }
}
