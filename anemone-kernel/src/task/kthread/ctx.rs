use crate::prelude::*;

use super::control::KThreadControl;

/// Narrow runtime capability passed to a kthread entry.
#[derive(Debug, Clone)]
pub struct KThreadCtx {
    pub(super) control: Arc<KThreadControl>,
}

impl KThreadCtx {
    pub(super) fn new(control: Arc<KThreadControl>) -> Self {
        Self { control }
    }

    pub fn should_stop(&self) -> bool {
        self.control.should_stop()
    }

    /// Wait for a pure wake notification until stop or the consumer predicate.
    pub fn wait_until<P>(&self, predicate: P)
    where
        P: Fn() -> bool,
    {
        self.control.wait_until(predicate);
    }

    /// Compatibility spelling for current consumers. This remains a pure
    /// wake-plus-predicate wait and does not encode request queue semantics.
    pub fn wait_until_woken<P>(&self, predicate: P)
    where
        P: Fn() -> bool,
    {
        self.wait_until(predicate);
    }
}
