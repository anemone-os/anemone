//! Shared latch wait loop for iomux syscalls.
//!
//! This helper only deals in kernel fd readiness, typed poll registration
//! results, and scheduler latch outcomes. Linux `pollfd` and `fd_set` layout
//! conversion stays in the syscall adapters.

use crate::prelude::*;

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

pub(super) fn wait_for_iomux_ready<F>(
    context: &'static str,
    task: &Arc<Task>,
    timeout: Option<Duration>,
    mut scan: F,
) -> Result<usize, SysError>
where
    F: for<'a> FnMut(IomuxScanMode<'a>) -> Result<IomuxScanOutcome, SysError>,
{
    let deadline = timeout.map(|timeout| Instant::now() + timeout);

    loop {
        if let Some(nready) = snapshot_scan(context, &mut scan)? {
            return Ok(nready);
        }

        if task.has_unmasked_signal() {
            kdebugln!("{}: interrupted by signal before latch begin", context);
            return Err(SysError::Interrupted);
        }

        let remaining = match deadline {
            Some(deadline) => {
                let now = Instant::now();
                if now >= deadline {
                    kdebugln!("{}: timeout expired before latch begin", context);
                    return Ok(0);
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
                return Ok(nready);
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
                if let Some(nready) = final_scan_after_register_abort(context, &mut scan)? {
                    return Ok(nready);
                }
                return Err(SysError::NotSupported);
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
                if let Some(nready) = final_scan_after_register_abort(context, &mut scan)? {
                    return Ok(nready);
                }
                return Err(err);
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

        if let Some(nready) = snapshot_scan(context, &mut scan)? {
            return Ok(nready);
        }

        match map_latch_outcome(context, outcome)? {
            IomuxWaitDisposition::Retry => {},
            IomuxWaitDisposition::Done(nready) => return Ok(nready),
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
    Done(usize),
}

fn map_latch_outcome(
    context: &'static str,
    outcome: LatchWaitOutcome,
) -> Result<IomuxWaitDisposition, SysError> {
    match outcome {
        LatchWaitOutcome::Triggered => Ok(IomuxWaitDisposition::Retry),
        LatchWaitOutcome::Timeout => Ok(IomuxWaitDisposition::Done(0)),
        LatchWaitOutcome::Signal | LatchWaitOutcome::Force => Err(SysError::Interrupted),
        LatchWaitOutcome::Cancelled | LatchWaitOutcome::Unexpected => {
            kwarningln!(
                "{}: unexpected latch outcome after schedule: {:?}",
                context,
                outcome,
            );
            Err(SysError::IO)
        },
    }
}
