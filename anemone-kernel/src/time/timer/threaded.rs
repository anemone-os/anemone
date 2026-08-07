use crate::{prelude::*, task::kworker::system_worker_on};

use super::{TimerHandle, TimerLane, deadline_after, push_realtime_timer_event, push_timer_event};

/// Schedule a timer event whose completion runs on the request CPU's system
/// worker. Timer core remains the owner of deadline and cancellation semantics;
/// it transfers the callback only after expiry.
pub fn schedule_threaded_timer_event(
    expire: Duration,
    callback: Box<dyn FnOnce() + Send + 'static>,
) -> TimerHandle {
    let deadline = deadline_after(expire);
    push_timer_event(deadline, TimerLane::Threaded(callback))
}

/// Schedule an absolute realtime request on the local CPU's timer queue.
///
/// `cancel_on_change_seq` is the timekeeper snapshot taken before registration.
/// When present, any later sequence consumes the request through
/// `clock_changed`; otherwise calendar steps only re-evaluate `deadline`.
pub(crate) fn schedule_realtime_threaded_timer_event(
    deadline: RealtimeInstant,
    cancel_on_change_seq: Option<u64>,
    expired: Box<dyn FnOnce() + Send + 'static>,
    clock_changed: Option<Box<dyn FnOnce() + Send + 'static>>,
) -> TimerHandle {
    assert_eq!(
        cancel_on_change_seq.is_some(),
        clock_changed.is_some(),
        "cancel-on-set identity and callback must be installed together"
    );
    push_realtime_timer_event(
        deadline,
        cancel_on_change_seq,
        TimerLane::RealtimeThreaded {
            expired,
            clock_changed,
        },
    )
}

pub(super) fn enqueue_expired_threaded_on(
    owner_cpu: CpuId,
    callback: Box<dyn FnOnce() + Send + 'static>,
) {
    // Realtime clock-step rechecks can dequeue a request from a remote timer
    // queue. Preserve the request's owner-CPU execution lane without giving
    // timer core access to kworker queue or kthread representation.
    system_worker_on(owner_cpu).submit_boxed(callback);
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn threaded_timer_callback_runs_outside_hwirq() {
        let completed = Arc::new(Event::new());
        let done = Arc::new(AtomicBool::new(false));
        let callback_completed = completed.clone();
        let callback_done = done.clone();

        let _request = schedule_threaded_timer_event(
            Duration::from_millis(1),
            Box::new(move || {
                assert!(
                    !crate::percpu::in_hwirq(),
                    "threaded timer callback ran in hwirq context"
                );
                assert!(
                    IntrArch::local_intr_enabled(),
                    "threaded timer callback ran with interrupts disabled"
                );
                assert!(
                    allow_preempt(),
                    "threaded timer callback ran with preemption disabled"
                );
                callback_done.store(true, Ordering::Release);
                callback_completed.publish(usize::MAX, true);
            }),
        );

        let timeout = completed.listen_with_timeout(
            false,
            || done.load(Ordering::Acquire),
            Duration::from_secs(1),
        );
        assert!(
            !matches!(timeout, Some(TimeoutListenException::Timeout)),
            "threaded timer callback did not complete before timeout"
        );
    }

    #[kunit]
    fn threaded_timer_burst_completes_in_process_context() {
        const CALLBACKS: usize = 4;

        let completed = Arc::new(Event::new());
        let done = Arc::new(AtomicUsize::new(0));
        let observed = Arc::new(SpinLock::new(Vec::new()));

        for index in 0..CALLBACKS {
            let callback_completed = completed.clone();
            let callback_done = done.clone();
            let callback_observed = observed.clone();
            let _request = schedule_threaded_timer_event(
                Duration::from_millis(1),
                Box::new(move || {
                    assert!(!crate::percpu::in_hwirq());
                    assert!(IntrArch::local_intr_enabled());
                    assert!(allow_preempt());
                    callback_observed.lock().push(index);
                    if callback_done.fetch_add(1, Ordering::AcqRel) + 1 == CALLBACKS {
                        callback_completed.publish(usize::MAX, true);
                    }
                }),
            );
        }

        let timeout = completed.listen_with_timeout(
            false,
            || done.load(Ordering::Acquire) == CALLBACKS,
            Duration::from_secs(1),
        );
        assert!(
            !matches!(timeout, Some(TimeoutListenException::Timeout)),
            "threaded timer callback burst did not complete before timeout"
        );

        let mut observed = observed.lock();
        observed.sort_unstable();
        assert_eq!(&*observed, &[0, 1, 2, 3]);
    }
}
