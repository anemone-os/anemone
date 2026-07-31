//! Shared latch wait loop for iomux syscalls.
//!
//! This helper only deals in kernel fd readiness, typed poll registration
//! results, and scheduler latch outcomes. Linux `pollfd` and `fd_set` layout
//! conversion stays in the syscall adapters.

use crate::{
    prelude::*,
    task::sig::{
        TemporaryMaskWaitCandidate, TemporaryMaskWaitContext, TemporaryMaskWaitDecision,
        TemporaryMaskWaitReturn, TemporarySigMaskToken,
    },
};

pub(super) use crate::fs::iomux::{
    IomuxScanMode, IomuxScanOutcome, IomuxWaitOutcome, wait_for_iomux_ready,
};

pub(in crate::fs) fn finish_temporary_iomux_wait(
    context: &'static str,
    task: &Arc<Task>,
    token: TemporarySigMaskToken,
    outcome: IomuxWaitOutcome,
    signal_context: TemporaryMaskWaitContext,
) -> Result<usize, SysError> {
    match outcome {
        IomuxWaitOutcome::Ready(nready) => {
            token.restore_now();
            Ok(nready)
        },
        IomuxWaitOutcome::Timeout => {
            token.restore_now();
            Ok(0)
        },
        IomuxWaitOutcome::Error(err) => {
            token.restore_now();
            Err(err)
        },
        IomuxWaitOutcome::Signal | IomuxWaitOutcome::Force => {
            let candidate = match outcome {
                IomuxWaitOutcome::Signal => TemporaryMaskWaitCandidate::Signal,
                IomuxWaitOutcome::Force => TemporaryMaskWaitCandidate::Force,
                _ => unreachable!(),
            };
            match task.classify_temporary_mask_wait(candidate, signal_context) {
                TemporaryMaskWaitDecision::DeferToTrapReturnDelivery => {
                    token.defer_to_signal_delivery();
                    Err(SysError::Interrupted)
                },
                TemporaryMaskWaitDecision::RestoreThenReturn(
                    TemporaryMaskWaitReturn::OriginalOutcome,
                ) => {
                    token.restore_now();
                    kwarningln!(
                        "{}: classifier returned original outcome for signal candidate task={} outcome={:?}",
                        context,
                        task.tid(),
                        outcome,
                    );
                    Err(SysError::IO)
                },
                TemporaryMaskWaitDecision::RestoreThenFailClosed(err) => {
                    token.restore_now();
                    Err(err)
                },
                TemporaryMaskWaitDecision::NoReturnForce => {
                    token.restore_now();
                    // There is no syscall-side no-return force helper yet.
                    // Keep this distinct from the ordinary EINTR carrier; trap
                    // return should consume the reserved force target.
                    kwarningln!(
                        "{}: no-return force candidate task={} outcome={:?}",
                        context,
                        task.tid(),
                        outcome,
                    );
                    Err(SysError::IO)
                },
            }
        },
    }
}
