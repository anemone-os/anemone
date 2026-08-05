use crate::{prelude::*, task::kworker::local_system_worker};

use super::{TimerEvent, deadline_after, push_timer_event};

/// Schedule a timer event whose completion runs on the local CPU's system
/// worker. Timer core remains the owner of deadline and cancellation semantics;
/// it transfers the callback to the worker queue only after expiry.
pub fn schedule_threaded_timer_event(
    expire: Duration,
    callback: Box<dyn FnOnce() + Send + 'static>,
) {
    push_timer_event(TimerEvent::new_threaded(deadline_after(expire), callback));
}

pub(super) fn enqueue_expired_threaded(callback: Box<dyn FnOnce() + Send + 'static>) {
    debug_assert!(IntrArch::local_intr_disabled());
    local_system_worker().submit_boxed(callback);
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

        schedule_threaded_timer_event(
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
            schedule_threaded_timer_event(
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
