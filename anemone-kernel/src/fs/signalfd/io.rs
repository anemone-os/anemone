use core::mem::size_of;

use anemone_abi::fs::linux::signalfd::SignalFdSigInfo;
use zerocopy::IntoBytes as _;

use crate::{
    fs::iomux::PollRoute,
    prelude::*,
    task::{
        files::{FileStatusFlags, OpenedFileReadUserCtx},
        sig::{SignalFdRecheckObserver, SignalFdRecheckRoute},
    },
};

use super::{file::SignalFdFile, record};

impl SignalFdFile {
    fn next_signal(&self, nonblock: bool) -> Result<crate::task::sig::Signal, SysError> {
        let task = get_current_task();
        let mut previous_outcome = None;

        loop {
            // A matching occurrence wins over an interruption candidate. The
            // live opened-description mask is resampled after every wake so a
            // concurrent reconfiguration is immediately visible.
            let mask = self.mask();
            if let Some(signal) = task.fetch_specific_signal(mask) {
                return Ok(signal);
            }

            if let Some(outcome) = previous_outcome.take() {
                match outcome {
                    LatchWaitOutcome::Triggered => {},
                    LatchWaitOutcome::Signal | LatchWaitOutcome::Force => {
                        return Err(SysError::Interrupted);
                    },
                    LatchWaitOutcome::Cancelled | LatchWaitOutcome::Unexpected => {
                        kwarningln!("signalfd: unexpected read wait outcome={:?}", outcome);
                        return Err(SysError::IO);
                    },
                    LatchWaitOutcome::Timeout => {
                        kwarningln!("signalfd: blocking read wait timed out without timeout");
                        return Err(SysError::IO);
                    },
                }
            }

            if nonblock {
                return Err(SysError::Again);
            }
            if task.has_unmasked_signal() {
                // Recheck the matching set immediately before reporting EINTR.
                if let Some(signal) = task.fetch_specific_signal(self.mask()) {
                    return Ok(signal);
                }
                return Err(SysError::Interrupted);
            }

            let latch = Latch::begin_current(true);
            let trigger = latch.make_trigger();
            let route = match make_read_recheck_route(&trigger) {
                Ok(route) => route,
                Err(err) => {
                    latch.cancel(LatchCancelReason::RegisterError);
                    let _ = latch.finish();
                    return Err(err);
                },
            };

            let thread_group = task.get_thread_group();
            if let Err(err) = thread_group.register_signalfd_recheck(route.clone()) {
                // Registration failure cannot park. Preserve a concurrent
                // matching occurrence if publication raced the allocation.
                if let Some(signal) = task.fetch_specific_signal(self.mask()) {
                    latch.cancel(LatchCancelReason::PredicateReady);
                    let _ = latch.finish();
                    return Ok(signal);
                }
                latch.cancel(LatchCancelReason::RegisterError);
                let _ = latch.finish();
                return Err(err);
            }
            if let Err(err) = self.register_recheck(route) {
                if let Some(signal) = task.fetch_specific_signal(self.mask()) {
                    latch.cancel(LatchCancelReason::PredicateReady);
                    let _ = latch.finish();
                    return Ok(signal);
                }
                latch.cancel(LatchCancelReason::RegisterError);
                let _ = latch.finish();
                return Err(err);
            }

            // Final scan closes publication before/after route registration.
            if let Some(signal) = task.fetch_specific_signal(self.mask()) {
                latch.cancel(LatchCancelReason::PredicateReady);
                let _ = latch.finish();
                return Ok(signal);
            }
            if task.has_unmasked_signal() {
                latch.cancel(LatchCancelReason::SignalPrecheck);
                previous_outcome = Some(latch.finish());
                continue;
            }

            latch.schedule_with_timeout(None);
            previous_outcome = Some(latch.finish());
        }
    }
}

#[derive(Debug)]
struct SignalFdReadRecheck {
    trigger: LatchTrigger,
}

impl SignalFdRecheckObserver for SignalFdReadRecheck {
    fn notify(&self) {
        self.trigger.trigger();
    }

    fn is_prunable(&self) -> bool {
        self.trigger.is_prunable()
    }
}

fn make_read_recheck_route(trigger: &LatchTrigger) -> Result<SignalFdRecheckRoute, SysError> {
    let observer: Arc<dyn SignalFdRecheckObserver> = Arc::try_new(SignalFdReadRecheck {
        trigger: trigger.clone(),
    })
    .map_err(|_| SysError::OutOfMemory)?;
    Ok(SignalFdRecheckRoute::for_read(observer))
}

#[derive(Debug)]
struct SignalFdPollRecheck {
    route: PollRoute,
}

impl SignalFdRecheckObserver for SignalFdPollRecheck {
    fn notify(&self) {
        self.route.notify();
    }

    fn is_prunable(&self) -> bool {
        self.route.is_prunable()
    }
}

fn make_poll_recheck_route(route: &PollRoute) -> Result<SignalFdRecheckRoute, SysError> {
    let hygiene_key = route.hygiene_key();
    let observer: Arc<dyn SignalFdRecheckObserver> = Arc::try_new(SignalFdPollRecheck {
        route: route.clone(),
    })
    .map_err(|_| SysError::OutOfMemory)?;
    Ok(SignalFdRecheckRoute::for_poll(observer, hygiene_key))
}

pub(super) fn signalfd_read_user_transaction(
    ctx: OpenedFileReadUserCtx<'_, '_>,
) -> Result<usize, SysError> {
    assert!(
        ctx.notification_suppressed,
        "signalfd reads must remain outside ordinary file-access notification"
    );
    if ctx.dst.remaining() < size_of::<SignalFdSigInfo>() {
        return Err(SysError::InvalidArgument);
    }

    let signalfd = SignalFdFile::from_file(ctx.file)
        .expect("signalfd description hook used with another file kind");
    let nonblock = ctx.status_flags.contains(FileStatusFlags::NONBLOCK);
    let slots = ctx.dst.remaining() / size_of::<SignalFdSigInfo>();
    let mut copied = 0;

    for slot in 0..slots {
        let signal = match signalfd.next_signal(nonblock || slot != 0) {
            Ok(signal) => signal,
            Err(SysError::Again) if copied != 0 => return Ok(copied),
            Err(err) if copied != 0 => return Ok(copied),
            Err(err) => return Err(err),
        };
        let record = record::from_signal(&signal);
        if let Err(err) = ctx.dst.exact_record().write_exact(record.as_bytes()) {
            // Dequeue precedes user copy by Linux ABI necessity. Preserve
            // already-published whole records as a short successful read.
            if copied != 0 {
                return Ok(copied);
            }
            return Err(err);
        }
        copied += size_of::<SignalFdSigInfo>();
    }

    Ok(copied)
}

pub(super) fn signalfd_poll(
    file: &File,
    request: &PollRequest<'_>,
) -> Result<PollRegisterResult, SysError> {
    let signalfd = SignalFdFile::from_file(file).expect("signalfd file without private state");
    let interests = request.interests();
    let readable = || {
        if interests.contains(PollEvent::READABLE)
            && get_current_task().has_dequeueable_specific_signal(signalfd.mask())
        {
            PollEvent::READABLE
        } else {
            PollEvent::empty()
        }
    };

    if !request.is_register() {
        return Ok(PollRegisterResult::Ready(readable()));
    }
    if !interests.contains(PollEvent::READABLE) {
        return Ok(PollRegisterResult::Unsupported);
    }

    let poll_route = request
        .route()
        .expect("signalfd register request disappeared after is_register");
    let route = make_poll_recheck_route(poll_route)?;
    get_current_task()
        .get_thread_group()
        .register_signalfd_recheck(route.clone())?;
    signalfd.register_recheck(route)?;

    // The source registration is caller-relative. For epoll this executes at
    // ADD/MOD and therefore binds the watch to that caller's thread group.
    Ok(PollRegisterResult::Subscribed(readable()))
}
