//! Shared latch wait loop for iomux syscalls.
//!
//! This helper only deals in kernel fd readiness, typed poll registration
//! results, and scheduler latch outcomes. Linux `pollfd` and `fd_set` layout
//! conversion stays in the syscall adapters.

use crate::prelude::*;
use crate::task::sig::{
    TemporaryMaskWaitCandidate, TemporaryMaskWaitContext, TemporaryMaskWaitDecision,
    TemporaryMaskWaitReturn, TemporarySigMaskToken,
};

#[derive(Clone, Copy, Debug)]
pub(super) enum IomuxScanMode<'a> {
    Snapshot,
    Register(&'a LatchTrigger),
}

impl<'a> IomuxScanMode<'a> {
    pub(super) fn poll_request(self, interests: PollEvent) -> PollRequest<'a> {
        match self {
            Self::Snapshot => PollRequest::snapshot(interests),
            Self::Register(trigger) => PollRequest::register(interests, trigger),
        }
    }

    pub(super) const fn is_register(self) -> bool {
        matches!(self, Self::Register(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum IomuxScanOutcome {
    Ready(usize),
    NotReady,
    Unsupported,
}

impl IomuxScanOutcome {
    pub(super) fn from_ready_count(nready: usize) -> Self {
        if nready > 0 {
            Self::Ready(nready)
        } else {
            Self::NotReady
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum IomuxWaitOutcome {
    Ready(usize),
    Timeout,
    Error(SysError),
    Signal,
    Force,
}

impl IomuxWaitOutcome {
    /// Map the typed wait result for callers without a temporary signal-mask
    /// token. Token-active paths must classify `Signal` / `Force` through the
    /// signal subsystem before choosing their errno/result mapping.
    pub(super) fn into_result_without_temporary_mask(self) -> Result<usize, SysError> {
        match self {
            Self::Ready(nready) => Ok(nready),
            Self::Timeout => Ok(0),
            Self::Error(err) => Err(err),
            Self::Signal | Self::Force => Err(SysError::Interrupted),
        }
    }
}

pub(super) fn finish_temporary_iomux_wait(
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
                TemporaryMaskWaitDecision::RestoreThenReturn(ret) => {
                    token.restore_now();
                    match ret {
                        TemporaryMaskWaitReturn::OriginalOutcome => {
                            kwarningln!(
                                "{}: classifier returned original outcome for signal candidate task={} outcome={:?}",
                                context,
                                task.tid(),
                                outcome,
                            );
                            Err(SysError::IO)
                        },
                        TemporaryMaskWaitReturn::Err(err) => Err(err),
                    }
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

pub(super) fn wait_for_iomux_ready<F>(
    context: &'static str,
    task: &Arc<Task>,
    timeout: Option<Duration>,
    mut scan: F,
) -> IomuxWaitOutcome
where
    F: for<'a> FnMut(IomuxScanMode<'a>) -> Result<IomuxScanOutcome, SysError>,
{
    let deadline = timeout.map(|timeout| Instant::now() + timeout);

    loop {
        match snapshot_scan(context, &mut scan) {
            Ok(Some(nready)) => return IomuxWaitOutcome::Ready(nready),
            Ok(None) => {},
            Err(err) => return IomuxWaitOutcome::Error(err),
        }

        if task.has_unmasked_signal() {
            kdebugln!("{}: interrupted by signal before latch begin", context);
            return IomuxWaitOutcome::Signal;
        }

        let remaining = match deadline {
            Some(deadline) => {
                let now = Instant::now();
                if now >= deadline {
                    kdebugln!("{}: timeout expired before latch begin", context);
                    return IomuxWaitOutcome::Timeout;
                }
                Some(deadline.saturating_duration_since(now))
            },
            None => None,
        };

        let latch = Latch::begin_current(true);
        let trigger = latch.make_trigger();

        match scan(IomuxScanMode::Register(&trigger)) {
            Ok(IomuxScanOutcome::Ready(nready)) if nready > 0 => {
                latch.cancel(LatchCancelReason::PredicateReady);
                let outcome = latch.finish();
                kdebugln!(
                    "{}: register scan found ready wait outcome={:?}",
                    context,
                    outcome,
                );
                return IomuxWaitOutcome::Ready(nready);
            },
            Ok(IomuxScanOutcome::Ready(_)) | Ok(IomuxScanOutcome::NotReady) => {},
            Ok(IomuxScanOutcome::Unsupported) => {
                latch.cancel(LatchCancelReason::RegisterError);
                let outcome = latch.finish();
                kwarningln!(
                    "{}: unsupported poll source during register scan, outcome={:?}",
                    context,
                    outcome,
                );
                match final_scan_after_register_abort(context, &mut scan) {
                    Ok(Some(nready)) => return IomuxWaitOutcome::Ready(nready),
                    Ok(None) => return IomuxWaitOutcome::Error(SysError::NotSupported),
                    Err(err) => return IomuxWaitOutcome::Error(err),
                }
            },
            Err(err) => {
                latch.cancel(LatchCancelReason::SyscallError);
                let outcome = latch.finish();
                kwarningln!(
                    "{}: register scan failed err={:?} outcome={:?}",
                    context,
                    err,
                    outcome,
                );
                match final_scan_after_register_abort(context, &mut scan) {
                    Ok(Some(nready)) => return IomuxWaitOutcome::Ready(nready),
                    Ok(None) => return IomuxWaitOutcome::Error(err),
                    Err(err) => return IomuxWaitOutcome::Error(err),
                }
            },
        }

        let remaining_after_wait = latch.schedule_with_timeout(remaining);
        let outcome = latch.finish();
        kdebugln!(
            "{}: latch wait finished outcome={:?} remaining={:?}",
            context,
            outcome,
            remaining_after_wait,
        );

        match snapshot_scan(context, &mut scan) {
            Ok(Some(nready)) => return IomuxWaitOutcome::Ready(nready),
            Ok(None) => {},
            Err(err) => return IomuxWaitOutcome::Error(err),
        }

        match map_latch_outcome(context, outcome) {
            IomuxWaitDisposition::Retry => {},
            IomuxWaitDisposition::Done(outcome) => return outcome,
        }
    }
}

fn final_scan_after_register_abort<F>(
    context: &'static str,
    scan: &mut F,
) -> Result<Option<usize>, SysError>
where
    F: for<'a> FnMut(IomuxScanMode<'a>) -> Result<IomuxScanOutcome, SysError>,
{
    let result = snapshot_scan(context, scan)?;
    if let Some(nready) = result {
        kdebugln!(
            "{}: final snapshot after register abort found {} ready",
            context,
            nready,
        );
    }
    Ok(result)
}

fn snapshot_scan<F>(context: &'static str, scan: &mut F) -> Result<Option<usize>, SysError>
where
    F: for<'a> FnMut(IomuxScanMode<'a>) -> Result<IomuxScanOutcome, SysError>,
{
    match scan(IomuxScanMode::Snapshot)? {
        IomuxScanOutcome::Ready(nready) if nready > 0 => Ok(Some(nready)),
        IomuxScanOutcome::Ready(_) | IomuxScanOutcome::NotReady => Ok(None),
        IomuxScanOutcome::Unsupported => {
            kwarningln!("{}: snapshot scan returned unsupported", context);
            Err(SysError::NotSupported)
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IomuxWaitDisposition {
    Retry,
    Done(IomuxWaitOutcome),
}

fn map_latch_outcome(
    context: &'static str,
    outcome: LatchWaitOutcome,
) -> IomuxWaitDisposition {
    match outcome {
        LatchWaitOutcome::Triggered => IomuxWaitDisposition::Retry,
        LatchWaitOutcome::Timeout => IomuxWaitDisposition::Done(IomuxWaitOutcome::Timeout),
        LatchWaitOutcome::Signal => IomuxWaitDisposition::Done(IomuxWaitOutcome::Signal),
        LatchWaitOutcome::Force => IomuxWaitDisposition::Done(IomuxWaitOutcome::Force),
        LatchWaitOutcome::Cancelled | LatchWaitOutcome::Unexpected => {
            kwarningln!(
                "{}: unexpected latch outcome after schedule: {:?}",
                context,
                outcome,
            );
            IomuxWaitDisposition::Done(IomuxWaitOutcome::Error(SysError::IO))
        },
    }
}
