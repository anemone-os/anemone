use crate::prelude::*;

use super::{
    PollRequest,
    subscription::{PollObserver, PollRoute},
};

struct IomuxWaitObserver {
    /// Sole consumer-side truth for whether source callbacks are accepted.
    /// Latch state remains scheduler-owned and is not mirrored here.
    accepting: AtomicBool,
    trigger: LatchTrigger,
}

impl IomuxWaitObserver {
    /// Returns whether this call performed the only live-to-retired transition.
    fn retire(&self) -> bool {
        self.accepting.swap(false, Ordering::AcqRel)
    }
}

impl PollObserver for IomuxWaitObserver {
    fn notify(&self) {
        // A callback that observed acceptance before retirement may race past
        // finish; LatchTrigger's wait identity makes that late trigger stale.
        if self.accepting.load(Ordering::Acquire) {
            self.trigger.trigger();
        }
    }
}

/// Linear owner of one iomux register/sleep/final-scan round.
///
/// The source-facing route is non-owning. This owner retains callback
/// acceptance and the latch until `finish`, which always retires acceptance
/// before retiring the wait identity.
pub(crate) struct IomuxWaitRound {
    latch: Option<Latch>,
    observer: Arc<IomuxWaitObserver>,
    route: PollRoute,
}

impl IomuxWaitRound {
    pub(crate) fn begin_current() -> Self {
        let latch = Latch::begin_current(true);
        let observer = Arc::new(IomuxWaitObserver {
            accepting: AtomicBool::new(true),
            trigger: latch.make_trigger(),
        });
        let erased: Arc<dyn PollObserver> = observer.clone();
        let route = PollRoute::new(&erased);
        drop(erased);

        Self {
            latch: Some(latch),
            observer,
            route,
        }
    }

    pub(crate) fn poll_request(&self, interests: super::PollEvent) -> PollRequest<'_> {
        PollRequest::register_with_route(interests, &self.route)
    }

    pub(in crate::fs) fn wait_id(&self) -> usize {
        self.observer.trigger.wait_id()
    }

    pub(crate) fn cancel(&self, reason: LatchCancelReason) {
        self.latch
            .as_ref()
            .expect("iomux wait round cancel after finish")
            .cancel(reason);
    }

    pub(crate) fn schedule_with_timeout(&self, timeout: Option<Duration>) -> Duration {
        self.latch
            .as_ref()
            .expect("iomux wait round schedule after finish")
            .schedule_with_timeout(timeout)
    }

    pub(crate) fn finish(mut self) -> LatchWaitOutcome {
        let retired = self.observer.retire();
        let latch = self.latch.take().expect("iomux wait round double finish");
        let outcome = latch.finish();
        assert!(retired, "iomux observer retired before wait-round finish");
        outcome
    }
}

impl Drop for IomuxWaitRound {
    fn drop(&mut self) {
        let Some(latch) = self.latch.take() else {
            return;
        };

        // Missing explicit finish is a bug, but first close both consumer
        // acceptance and the underlying wait so panic cannot leak either.
        let retired = self.observer.retire();
        latch.cancel(LatchCancelReason::Drop);
        let outcome = latch.finish();
        kwarningln!(
            "iomux: wait round dropped without finish wait={:#x} outcome={:?}",
            self.observer.trigger.wait_id(),
            outcome,
        );
        assert!(retired, "iomux observer retired before wait-round drop");
        assert!(false, "iomux wait round dropped without finish");
    }
}

#[derive(Clone, Copy)]
pub(in crate::fs) enum IomuxScanMode<'a> {
    Snapshot,
    Register(&'a IomuxWaitRound),
}

impl<'a> IomuxScanMode<'a> {
    pub(in crate::fs) fn poll_request(self, interests: super::PollEvent) -> PollRequest<'a> {
        match self {
            Self::Snapshot => PollRequest::snapshot(interests),
            Self::Register(round) => round.poll_request(interests),
        }
    }

    pub(in crate::fs) const fn is_register(self) -> bool {
        matches!(self, Self::Register(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::fs) enum IomuxScanOutcome {
    Ready(usize),
    Recheck,
    NotReady,
    NoSources,
    Unsupported,
}

impl IomuxScanOutcome {
    pub(in crate::fs) fn from_ready_count(nready: usize) -> Self {
        if nready > 0 {
            Self::Ready(nready)
        } else {
            Self::NotReady
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::fs) enum IomuxWaitOutcome {
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
    pub(in crate::fs) fn into_result_without_temporary_mask(self) -> Result<usize, SysError> {
        match self {
            Self::Ready(nready) => Ok(nready),
            Self::Timeout => Ok(0),
            Self::Error(err) => Err(err),
            Self::Signal | Self::Force => Err(SysError::Interrupted),
        }
    }
}

/// Run one source-neutral snapshot/register/schedule/final-snapshot protocol.
///
/// Callers own predicate projection and operation retry. This owner alone
/// owns latch, signal, timeout and register-abort cleanup semantics.
pub(in crate::fs) fn wait_for_iomux_ready<F>(
    context: &'static str,
    task: &Arc<Task>,
    timeout: Option<Duration>,
    mut scan: F,
) -> IomuxWaitOutcome
where
    F: for<'a> FnMut(IomuxScanMode<'a>) -> Result<IomuxScanOutcome, SysError>,
{
    let deadline = timeout.map(|timeout| MonotonicInstant::now() + timeout);

    loop {
        let no_sources = match snapshot_scan(context, &mut scan) {
            Ok(SnapshotScanOutcome::Ready(nready)) => return IomuxWaitOutcome::Ready(nready),
            Ok(SnapshotScanOutcome::NotReady) => false,
            Ok(SnapshotScanOutcome::NoSources) => true,
            Err(err) => return IomuxWaitOutcome::Error(err),
        };

        if task.has_unmasked_signal() {
            kdebugln!("{}: interrupted by signal before latch begin", context);
            return IomuxWaitOutcome::Signal;
        }

        if no_sources {
            return wait_without_iomux_sources(context, timeout);
        }

        let remaining = match deadline {
            Some(deadline) => {
                let now = MonotonicInstant::now();
                if now >= deadline {
                    kdebugln!("{}: timeout expired before latch begin", context);
                    return IomuxWaitOutcome::Timeout;
                }
                Some(deadline.saturating_duration_since(now))
            },
            None => None,
        };

        let round = IomuxWaitRound::begin_current();
        let wait_id = round.wait_id();

        match scan(IomuxScanMode::Register(&round)) {
            Ok(IomuxScanOutcome::Ready(nready)) if nready > 0 => {
                round.cancel(LatchCancelReason::PredicateReady);
                let outcome = round.finish();
                kdebugln!(
                    "{}: register scan found ready wait={:#x} nready={} wait outcome={:?}",
                    context,
                    wait_id,
                    nready,
                    outcome,
                );
                match snapshot_scan(context, &mut scan) {
                    Ok(SnapshotScanOutcome::Ready(nready)) => {
                        return IomuxWaitOutcome::Ready(nready);
                    },
                    Ok(SnapshotScanOutcome::NotReady | SnapshotScanOutcome::NoSources) => {
                        match map_register_ready_outcome(context, outcome) {
                            IomuxWaitDisposition::Retry => continue,
                            IomuxWaitDisposition::Done(outcome) => return outcome,
                        }
                    },
                    Err(err) => return IomuxWaitOutcome::Error(err),
                }
            },
            Ok(IomuxScanOutcome::Recheck) => {
                // PredicateReady is the existing non-error latch cancellation
                // carrier. SubscribedRecheck is not counted as readiness: it
                // only prevents parking until this round is retired and the
                // final snapshot has classified the predicate.
                round.cancel(LatchCancelReason::PredicateReady);
                let outcome = round.finish();
                kdebugln!(
                    "{}: register scan requested recheck wait={:#x} outcome={:?}",
                    context,
                    wait_id,
                    outcome,
                );
                match snapshot_scan(context, &mut scan) {
                    Ok(SnapshotScanOutcome::Ready(nready)) => {
                        return IomuxWaitOutcome::Ready(nready);
                    },
                    Ok(SnapshotScanOutcome::NotReady | SnapshotScanOutcome::NoSources) => {
                        match map_register_ready_outcome(context, outcome) {
                            IomuxWaitDisposition::Retry => continue,
                            IomuxWaitDisposition::Done(outcome) => return outcome,
                        }
                    },
                    Err(err) => return IomuxWaitOutcome::Error(err),
                }
            },
            Ok(
                register_outcome @ (IomuxScanOutcome::Ready(_)
                | IomuxScanOutcome::NotReady
                | IomuxScanOutcome::NoSources),
            ) => {
                kdebugln!(
                    "{}: register scan completed wait={:#x} outcome={:?} remaining={:?}",
                    context,
                    wait_id,
                    register_outcome,
                    remaining,
                );
            },
            Ok(IomuxScanOutcome::Unsupported) => {
                round.cancel(LatchCancelReason::RegisterError);
                let outcome = round.finish();
                kwarningln!(
                    "{}: unsupported poll source during register scan wait={:#x} outcome={:?}",
                    context,
                    wait_id,
                    outcome,
                );
                match final_scan_after_register_abort(context, &mut scan) {
                    Ok(Some(nready)) => return IomuxWaitOutcome::Ready(nready),
                    Ok(None) => return IomuxWaitOutcome::Error(SysError::NotSupported),
                    Err(err) => return IomuxWaitOutcome::Error(err),
                }
            },
            Err(err) => {
                round.cancel(LatchCancelReason::SyscallError);
                let outcome = round.finish();
                kwarningln!(
                    "{}: register scan failed wait={:#x} err={:?} outcome={:?}",
                    context,
                    wait_id,
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

        let remaining_after_wait = round.schedule_with_timeout(remaining);
        let outcome = round.finish();
        kdebugln!(
            "{}: latch wait finished outcome={:?} remaining={:?}",
            context,
            outcome,
            remaining_after_wait,
        );

        match snapshot_scan(context, &mut scan) {
            Ok(SnapshotScanOutcome::Ready(nready)) => return IomuxWaitOutcome::Ready(nready),
            Ok(SnapshotScanOutcome::NotReady | SnapshotScanOutcome::NoSources) => {},
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
    if let SnapshotScanOutcome::Ready(nready) = result {
        kdebugln!(
            "{}: final snapshot after register abort found {} ready",
            context,
            nready,
        );
        Ok(Some(nready))
    } else {
        Ok(None)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SnapshotScanOutcome {
    Ready(usize),
    NotReady,
    NoSources,
}

fn snapshot_scan<F>(context: &'static str, scan: &mut F) -> Result<SnapshotScanOutcome, SysError>
where
    F: for<'a> FnMut(IomuxScanMode<'a>) -> Result<IomuxScanOutcome, SysError>,
{
    match scan(IomuxScanMode::Snapshot)? {
        IomuxScanOutcome::Ready(nready) if nready > 0 => Ok(SnapshotScanOutcome::Ready(nready)),
        IomuxScanOutcome::Ready(_) | IomuxScanOutcome::NotReady => {
            Ok(SnapshotScanOutcome::NotReady)
        },
        IomuxScanOutcome::NoSources => Ok(SnapshotScanOutcome::NoSources),
        IomuxScanOutcome::Recheck => {
            kwarningln!("{}: snapshot scan returned recheck", context);
            Err(SysError::IO)
        },
        IomuxScanOutcome::Unsupported => {
            kwarningln!("{}: snapshot scan returned unsupported", context);
            Err(SysError::NotSupported)
        },
    }
}

fn wait_without_iomux_sources(
    context: &'static str,
    timeout: Option<Duration>,
) -> IomuxWaitOutcome {
    if matches!(timeout, Some(timeout) if timeout == Duration::ZERO) {
        return IomuxWaitOutcome::Timeout;
    }

    // Linux treats select/poll with no armable fd sources as a timeout sleep:
    // zero timeout probes return immediately, but positive or NULL timeouts
    // must still enter an interruptible wait so tiny sleeps do not become
    // pure userspace-visible busy polling.
    let latch = Latch::begin_current(true);
    let remaining_after_wait = latch.schedule_with_timeout(timeout);
    let outcome = latch.finish();
    kdebugln!(
        "{}: no-source wait finished outcome={:?} remaining={:?}",
        context,
        outcome,
        remaining_after_wait,
    );

    match outcome {
        LatchWaitOutcome::Timeout => IomuxWaitOutcome::Timeout,
        LatchWaitOutcome::Signal => IomuxWaitOutcome::Signal,
        LatchWaitOutcome::Force => IomuxWaitOutcome::Force,
        LatchWaitOutcome::Triggered
        | LatchWaitOutcome::Cancelled
        | LatchWaitOutcome::Unexpected => {
            kwarningln!(
                "{}: unexpected no-source latch outcome after schedule: {:?}",
                context,
                outcome,
            );
            IomuxWaitOutcome::Error(SysError::IO)
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IomuxWaitDisposition {
    Retry,
    Done(IomuxWaitOutcome),
}

fn map_register_ready_outcome(
    context: &'static str,
    outcome: LatchWaitOutcome,
) -> IomuxWaitDisposition {
    match outcome {
        // The register snapshot was only a hint. If the final predicate is no
        // longer ready, an accepted producer hint or our own predicate cancel
        // simply starts a fresh snapshot/register round.
        LatchWaitOutcome::Triggered | LatchWaitOutcome::Cancelled => IomuxWaitDisposition::Retry,
        LatchWaitOutcome::Signal => IomuxWaitDisposition::Done(IomuxWaitOutcome::Signal),
        LatchWaitOutcome::Force => IomuxWaitDisposition::Done(IomuxWaitOutcome::Force),
        LatchWaitOutcome::Timeout | LatchWaitOutcome::Unexpected => {
            kwarningln!(
                "{}: unexpected latch outcome after register-ready recheck: {:?}",
                context,
                outcome,
            );
            IomuxWaitDisposition::Done(IomuxWaitOutcome::Error(SysError::IO))
        },
    }
}

fn map_latch_outcome(context: &'static str, outcome: LatchWaitOutcome) -> IomuxWaitDisposition {
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
