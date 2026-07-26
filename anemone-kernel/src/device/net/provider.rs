use anemone_net_api::FrameProvider;

use crate::prelude::*;

/// Stateless worker-wake capability installed by the kernel attach authority.
///
/// The provider commits its durable recheck predicate before invoking this
/// edge. Implementations must not read driver state or run protocol work.
pub(crate) trait RecheckWake: Send + Sync {
    fn wake(&self);
}

/// Kernel-local extension of the shared frame capability.
///
/// `FrameProvider` owns frame access semantics shared with host validation.
/// This port adds only the IRQ-to-worker handoff needed by a kernel netdev:
/// the provider remains the sole owner of the durable predicate, while the
/// attach owner supplies a weak, stateless wake edge.
pub(crate) trait NetdevFrameProvider: FrameProvider + Send + 'static {
    fn install_recheck_wake(&self, wake: Weak<dyn RecheckWake>);

    fn recheck_requested(&self) -> bool;

    fn take_recheck_requested(&self) -> bool;
}
