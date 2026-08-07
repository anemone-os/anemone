use crate::{
    prelude::*,
    time::{
        monotonic_ns,
        timer::{
            TimerHandle, schedule_realtime_threaded_timer_event, schedule_threaded_timer_event,
        },
    },
};

use super::{
    TimerFdCore, TimerFdDeadline, TimerFdSchedule, account_due_expiration_locked, deadline_read,
    deadline_timeout, notify_waiters_after_unlock, refresh_cancel_on_change_snapshot,
};

pub(super) fn schedule_timerfd_callback(
    core: &Arc<TimerFdCore>,
    generation: u64,
    deadline: TimerFdDeadline,
) -> TimerHandle {
    // Timerfd submits a bounded threaded completion, not a background job. The
    // timerfd object still owns generation filtering, missed-tick accounting,
    // trigger handoff and periodic rearm under its state lock. Callers may use
    // this before unlocking because this RFC's threaded timer submit has no
    // recoverable failure path; normal return is the queued-event publish point.
    match deadline {
        TimerFdDeadline::Monotonic(deadline_ns) => {
            let weak = Arc::downgrade(core);
            schedule_threaded_timer_event(
                deadline_timeout(monotonic_ns(), deadline_ns),
                Box::new(move || timerfd_expire_callback(weak, generation)),
            )
        },
        TimerFdDeadline::Realtime {
            deadline_ns,
            cancel_on_change_seq,
        } => {
            let expire_core = Arc::downgrade(core);
            let clock_changed = cancel_on_change_seq.map(|_| {
                let changed_core = Arc::downgrade(core);
                Box::new(move || timerfd_clock_changed_callback(changed_core, generation))
                    as Box<dyn FnOnce() + Send + 'static>
            });
            schedule_realtime_threaded_timer_event(
                deadline_ns,
                cancel_on_change_seq,
                Box::new(move || timerfd_expire_callback(expire_core, generation)),
                clock_changed,
            )
        },
    }
}

pub(super) fn timerfd_expire_callback(core: Weak<TimerFdCore>, generation: u64) {
    let Some(core) = core.upgrade() else {
        return;
    };

    let detached = {
        let mut state = core.state.lock();
        if state.generation != generation {
            return;
        }
        assert!(
            state.request.take().is_some(),
            "current timerfd callback is missing its dequeued request handle"
        );

        let TimerFdSchedule::Armed {
            deadline,
            interval_ns,
        } = state.schedule
        else {
            panic!("current timerfd callback observed a disarmed owner schedule");
        };

        // The queue already linearized this request as expired. A realtime
        // backward step after dequeue must not revoke that expiration.
        let (now_ns, _) = deadline_read(deadline);
        let (detached, rearm) = account_due_expiration_locked(
            &mut state,
            now_ns.max(deadline.deadline_ns()),
            deadline,
            interval_ns,
        );
        if rearm.is_some() {
            refresh_cancel_on_change_snapshot(&mut state.schedule);
            let TimerFdSchedule::Armed { deadline, .. } = state.schedule else {
                unreachable!("periodic timerfd lost its armed schedule")
            };
            // Submit the successor event before publishing the updated armed
            // state by unlocking. This keeps the ordinary path from exposing an
            // armed periodic timer without a matching queued timer-core event.
            state.request = Some(schedule_timerfd_callback(&core, generation, deadline));
        }
        detached
    };

    notify_waiters_after_unlock(detached, "expire");
}

pub(super) fn timerfd_clock_changed_callback(core: Weak<TimerFdCore>, generation: u64) {
    let Some(core) = core.upgrade() else {
        return;
    };

    let detached = {
        let mut state = core.state.lock();
        if state.generation != generation {
            return;
        }
        assert!(
            state.request.take().is_some(),
            "current timerfd clock-change callback is missing its request handle"
        );
        assert!(
            matches!(
                state.schedule,
                TimerFdSchedule::Armed {
                    deadline: TimerFdDeadline::Realtime {
                        cancel_on_change_seq: Some(_),
                        ..
                    },
                    ..
                }
            ),
            "clock-change callback observed a non-cancellable timerfd schedule"
        );
        // Logical cancellation belongs to timerfd, not the timer queue. Publish
        // the one-shot read error and readiness only after generation proves
        // this completion still names the current arm.
        state.schedule = TimerFdSchedule::Disarmed;
        state.expirations = 0;
        state.cancelled = true;
        state.collect_readable_waiters()
    };

    notify_waiters_after_unlock(detached, "clock_changed");
}
