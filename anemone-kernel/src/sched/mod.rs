//! Code in this module enormously relies on the fact that we don't support
//! cross-core scheduling yet.

use crate::{
    prelude::*,
    sched::{
        class::idle::clone_local_idle_task,
        processor::{local_pick_next, local_requeue_current, set_current_task},
        switch::{switch_mapping, switch_out, switch_to},
    },
};

mod hal;
pub use hal::*;
mod api;
pub use api::*;

mod processor;
pub use processor::{
    fetch_clear_need_resched, get_current_task, init_routines, local_enqueue, local_sched_tick,
    mark_need_resched, pick_next_cpu, task_enqueue, wake_enqueue,
};
mod switch;
pub use switch::load_context;

mod event;
pub use event::{Event, TimeoutListenException};

pub mod class;
mod wait;
pub use wait::{
    ActiveWait, ParkState, TaskSchedState, WaitOutcome, WaitReason, WaitState,
    WaitStateStatus, WakeEnqueueResult, WakeMode, WakeResult, WakeToken,
};

/// Core scheduler loop. Called by bootstrap code.
///
/// Interrupts are disabled all the time in this function.
///
/// **In upper half of scheduler loop, local current task is still the task
/// previously switched out.**
pub unsafe fn scheduler() -> ! {
    // on entering this function from bootstrap code, some invariants of the loop
    // are not satisfied yet.
    debug_assert!(IntrArch::local_intr_disabled());

    // this satisfies the first invariant.
    set_current_task(Some(clone_local_idle_task()));
    // the second invariant is not satisfied, but its fine. since we're now
    // in kernel's mapping.

    knoticeln!("scheduler started");

    // System Invariants on entering this loop:
    // - current task is the task that right switched out.
    // - scheduler are still in previous task's memory mapping.
    loop {
        {
            let prev = get_current_task();
            let next = local_pick_next();
            unsafe {
                switch_mapping(&prev, &next);
                switch_to(next);
            }
        }

        // free resources.
        dispose_deferred_tasks();
    }
}

/// Core scheduler state transitions.
mod kore {
    use super::*;

    #[derive(Debug)]
    enum ScheduleDecision {
        Runnable,
        WaitCoreParked {
            state: Arc<WaitState>,
            interruptible: bool,
            wait_id: usize,
        },
        Zombie,
    }

    /// Schedule the next task to run.
    ///
    /// Basically, you should never call this function in application code. this
    /// is for those who building synchronization primitives or something
    /// low-level like that. Instead, higher-level encapsulations like
    /// [higher_level::yield_now] should be used in most cases.
    ///
    /// **Interrupts must be disabled when calling this function.**
    pub unsafe fn schedule() {
        debug_assert!(IntrArch::local_intr_disabled());
        let curr = get_current_task();

        let decision = curr.update_sched_state_with(|state| match state {
            TaskSchedState::Runnable => (TaskSchedState::Runnable, ScheduleDecision::Runnable),
            TaskSchedState::Waiting {
                state,
                interruptible,
                park: ParkState::PrePark,
            } => {
                let wait_id = state.debug_id();
                let wait_state = state.clone();
                (
                    TaskSchedState::Waiting {
                        state,
                        interruptible,
                        park: ParkState::Parked,
                    },
                    ScheduleDecision::WaitCoreParked {
                        state: wait_state,
                        interruptible,
                        wait_id,
                    },
                )
            },
            TaskSchedState::Waiting {
                state,
                interruptible,
                park: ParkState::Parked,
            } => {
                let wait_id = state.debug_id();
                let wait_state = state.clone();
                (
                    TaskSchedState::Waiting {
                        state,
                        interruptible,
                        park: ParkState::Parked,
                    },
                    ScheduleDecision::WaitCoreParked {
                        state: wait_state,
                        interruptible,
                        wait_id,
                    },
                )
            },
            TaskSchedState::Zombie => (TaskSchedState::Zombie, ScheduleDecision::Zombie),
        });

        match decision {
            ScheduleDecision::Runnable => {
                if !curr.flags().is_idle() {
                    local_requeue_current(curr);
                } else {
                    drop(curr);
                }
            },
            ScheduleDecision::WaitCoreParked {
                state,
                interruptible,
                wait_id,
            } => {
                let task_id = curr.tid();
                match curr.sched_state() {
                    TaskSchedState::Runnable => {
                        kdebugln!(
                            "schedule: abort park for task={} wait={:#x}; wait already completed",
                            task_id,
                            wait_id,
                        );
                        if !curr.flags().is_idle() {
                            local_requeue_current(curr);
                        } else {
                            drop(curr);
                        }
                    },
                    TaskSchedState::Waiting {
                        state: observed,
                        interruptible: observed_interruptible,
                        park,
                    } if Arc::ptr_eq(&observed, &state) => {
                        if observed_interruptible != interruptible {
                            kwarningln!(
                                "schedule: wait-core interruptible changed while parking task={} wait={:#x}: expected={} observed={}",
                                task_id,
                                wait_id,
                                interruptible,
                                observed_interruptible,
                            );
                            debug_assert_eq!(observed_interruptible, interruptible);
                        }
                        knoticeln!(
                            "{} is wait-core parked (wait={:#x}, interruptible: {}, park: {:?}), not enqueuing it to run queue",
                            task_id,
                            wait_id,
                            observed_interruptible,
                            park,
                        );
                        drop(curr);
                    },
                    TaskSchedState::Waiting {
                        state: observed,
                        interruptible: observed_interruptible,
                        park,
                    } => {
                        kwarningln!(
                            "schedule: unexpected wait-core state after parking task={} wait={:#x}: observed wait={:#x}, interruptible={}, park={:?}",
                            task_id,
                            wait_id,
                            observed.debug_id(),
                            observed_interruptible,
                            park,
                        );
                        debug_assert!(
                            Arc::ptr_eq(&observed, &state),
                            "schedule observed a different wait round after parking"
                        );
                        drop(curr);
                    },
                    TaskSchedState::Zombie => {
                        kwarningln!(
                            "schedule: wait-core park for task={} wait={:#x} became zombie",
                            task_id,
                            wait_id,
                        );
                        debug_assert!(false, "zombie state observed after wait-core park");
                        drop(curr);
                    },
                }
            },
            ScheduleDecision::Zombie => {
                knoticeln!(
                    "{} is zombie, not enqueuing it to run queue",
                    current_task_id(),
                );
                drop(curr);
            },
        }

        unsafe {
            switch_out();
        }
    }

    /// Signal or force-complete the currently active wait, if any.
    pub fn notify(task: &Arc<Task>, uninterruptible: bool) {
        let mode = if uninterruptible {
            WakeMode::Force
        } else {
            WakeMode::InterruptibleOnly
        };

        let result = wait::wake_active_wait(task, WaitReason::Signal, mode);
        if result != WakeResult::Stale {
            kdebugln!(
                "{} is notified through wait core, uninterruptible={}, result={:?}",
                task.tid(),
                uninterruptible,
                result,
            );
            return;
        }

        kdebugln!(
            "{} notify found no active wait, uninterruptible={}",
            task.tid(),
            uninterruptible,
        );
    }
}
pub use kore::*;

/// Upper-level APIs built upon [kore] functions.
mod higher_level {

    use crate::time::timer::schedule_local_irq_timer_event;

    use super::*;

    /// Schedule the current wait-core round with an optional timeout.
    ///
    /// `token` names the wait round already published through
    /// `ActiveWait::begin()`. A late timer callback races through
    /// `wake_wait()`, so timeout validity is derived from the wait identity
    /// rather than an external cancellation flag.
    pub fn schedule_wait_with_timeout(
        task: &Arc<Task>,
        token: WakeToken,
        timeout: Option<Duration>,
    ) -> Duration {
        let current = get_current_task();
        assert!(
            Arc::ptr_eq(task, &current),
            "schedule_wait_with_timeout only schedules the current task"
        );
        drop(current);

        let cloned_task = task.clone();
        let wait_id = token.wait_id();

        let start = with_intr_disabled(|| {
            if let Some(timeout) = timeout {
                unsafe {
                    schedule_local_irq_timer_event(
                        timeout,
                        Box::new(move || {
                            let result = wait::wake_wait(
                                &cloned_task,
                                &token,
                                WaitReason::Timeout,
                                WakeMode::AnyWait,
                            );
                            kdebugln!(
                                "schedule_wait_with_timeout: timeout task={} wait={:#x} result={:?}",
                                cloned_task.tid(),
                                wait_id,
                                result,
                            );
                        }),
                    );
                }
            }

            let start = Instant::now();
            unsafe {
                schedule();
            }

            start
        });

        let elapsed = start.elapsed();

        if let Some(timeout) = timeout {
            timeout.saturating_sub(elapsed)
        } else {
            Duration::MAX
        }
    }

    /// Yield the current running task to let other tasks run.
    pub fn yield_now() {
        assert!(get_current_task().status() == TaskStatus::Runnable);
        with_intr_disabled(|| unsafe {
            schedule();
        });
    }
}
pub use higher_level::*;

mod helpers {

    use super::*;

    /// Get [Tid] of current task.
    pub fn current_task_id() -> Tid {
        //with_current_task(|task| task.tid())
        get_current_task().tid()
    }
}
pub use helpers::*;
