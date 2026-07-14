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

mod latch;
pub use latch::{Latch, LatchCancelReason, LatchTrigger, LatchWaitOutcome};

pub mod class;
mod wait;
pub(crate) use wait::assert_current_not_in_active_wait;
pub use wait::{ParkState, TaskSchedState, WaitState, WakeEnqueueResult};

/// Core scheduler loop. Called by bootstrap code.
///
/// Interrupts are disabled all the time in this function.
///
/// **In upper half of scheduler loop, local current task is still the task
/// previously switched out.**
pub unsafe fn scheduler() -> ! {
    // on entering this function from bootstrap code, some invariants of the loop
    // are not satisfied yet.
    assert!(IntrArch::local_intr_disabled());

    // this satisfies the first invariant.
    set_current_task(Some(clone_local_idle_task()));
    // the second invariant is not satisfied, but its fine. since we're now
    // in kernel's mapping.

    kinfoln!("scheduler of {} started", cur_cpu_id());

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
    use super::{
        wait::{WaitReason, WakeMode, WakeResult, WakeToken},
        *,
    };

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum SchedulePreemptResult {
        Scheduled,
        Deferred,
    }

    #[derive(Clone, Copy, Debug)]
    enum ScheduleMode<'a> {
        WaitSleep { token: &'a WakeToken },
        Preempt,
        Runnable,
        Zombie,
    }

    #[derive(Debug)]
    enum ScheduleDecision {
        Runnable,
        WaitCoreParked {
            state: Arc<WaitState>,
            interruptible: bool,
        },
        Zombie,
        AbortWaitSleep {
            wait_id: usize,
        },
        DeferredPreempt {
            wait_id: usize,
        },
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum ScheduleInnerResult {
        Switched,
        DidNotSwitch,
        DeferredPreempt,
    }

    /// Schedule the current wait round after its wake prerequisites are ready.
    ///
    /// `token` must name the current wait round. If the same round already
    /// completed before explicit sleep, this returns without switching so the
    /// waiter can finish the round through the normal abort/no-park path.
    ///
    /// **Interrupts must be disabled when calling this function.**
    pub(super) unsafe fn schedule_wait_sleep(token: &WakeToken) {
        let result = unsafe { schedule_inner(ScheduleMode::WaitSleep { token }) };
        assert!(
            matches!(
                result,
                ScheduleInnerResult::Switched | ScheduleInnerResult::DidNotSwitch
            ),
            "wait-sleep schedule cannot return a preempt-deferred result"
        );
    }

    /// Involuntary preemption entry.
    ///
    /// `Waiting/PrePark` means the current task is still setting up a wait
    /// round, so preemption is deferred without parking, requeueing, or
    /// switching out.
    ///
    /// **Interrupts must be disabled when calling this function.**
    pub unsafe fn schedule_preempt() -> SchedulePreemptResult {
        match unsafe { schedule_inner(ScheduleMode::Preempt) } {
            ScheduleInnerResult::Switched => SchedulePreemptResult::Scheduled,
            ScheduleInnerResult::DeferredPreempt => SchedulePreemptResult::Deferred,
            ScheduleInnerResult::DidNotSwitch => {
                unreachable!("preempt schedule cannot abort wait sleep")
            },
        }
    }

    /// Schedule a current task that must still be runnable.
    ///
    /// **Interrupts must be disabled when calling this function.**
    pub unsafe fn schedule_runnable() {
        let result = unsafe { schedule_inner(ScheduleMode::Runnable) };
        assert_eq!(result, ScheduleInnerResult::Switched);
    }

    /// Schedule the idle task without granting wait-sleep park permission.
    ///
    /// **Interrupts must be disabled when calling this function.**
    pub unsafe fn schedule_idle() {
        assert!(get_current_task().flags().is_idle());
        let result = unsafe { schedule_inner(ScheduleMode::Runnable) };
        assert_eq!(result, ScheduleInnerResult::Switched);
    }

    /// Publish the current task as zombie and schedule away from it.
    ///
    /// The current task must have finished exit cleanup and still be runnable.
    /// The Zombie state is published inside this noirq scheduler transaction so
    /// involuntary preempt cannot observe a zombie current task before this
    /// never-return entry consumes it.
    ///
    /// **Interrupts must be disabled when calling this function.**
    pub unsafe fn schedule_zombie_never_return() -> ! {
        let result = unsafe { schedule_inner(ScheduleMode::Zombie) };
        assert_eq!(result, ScheduleInnerResult::Switched);
        unreachable!("zombie task should never be scheduled again");
    }

    unsafe fn schedule_inner(mode: ScheduleMode<'_>) -> ScheduleInnerResult {
        assert!(IntrArch::local_intr_disabled());
        let curr = get_current_task();
        let task_id = curr.tid();

        let decision = curr.update_sched_state_with(|state| match state {
            TaskSchedState::Runnable => match mode {
                ScheduleMode::WaitSleep { token } if !token.is_armed() => (
                    TaskSchedState::Runnable,
                    ScheduleDecision::AbortWaitSleep {
                        wait_id: token.wait_id(),
                    },
                ),
                ScheduleMode::WaitSleep { token } => {
                    panic!(
                        "schedule_wait_sleep requires current wait round: task={} token_wait={:#x} state=Runnable token_status=armed",
                        task_id,
                        token.wait_id(),
                    );
                },
                ScheduleMode::Preempt | ScheduleMode::Runnable => {
                    (TaskSchedState::Runnable, ScheduleDecision::Runnable)
                },
                ScheduleMode::Zombie => {
                    // The exit path owns task/thread-group cleanup, but the
                    // scheduler owns publishing the final Zombie sched-state.
                    // Publishing and switch-out must stay in this noirq
                    // transaction so trap-tail preempt cannot observe a zombie
                    // current before the never-return schedule entry consumes it.
                    (TaskSchedState::Zombie, ScheduleDecision::Zombie)
                },
            },
            TaskSchedState::Waiting {
                state,
                interruptible,
                park: ParkState::PrePark,
            } => match mode {
                ScheduleMode::WaitSleep { token } => {
                    if !token.matches_wait_state(&state) {
                        panic!(
                            "schedule_wait_sleep token mismatch: task={} token_wait={:#x} current_wait={:#x}",
                            task_id,
                            token.wait_id(),
                            state.debug_id(),
                        );
                    }
                    kdebugln!(
                        "schedule_wait_sleep: entry=WaitSleep task={} wait={:#x} transition=PrePark->Parked",
                        task_id,
                        state.debug_id(),
                    );
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
                        },
                    )
                },
                ScheduleMode::Preempt => (
                    TaskSchedState::Waiting {
                        state: state.clone(),
                        interruptible,
                        park: ParkState::PrePark,
                    },
                    ScheduleDecision::DeferredPreempt {
                        wait_id: state.debug_id(),
                    },
                ),
                ScheduleMode::Runnable => {
                    panic!(
                        "schedule_runnable cannot consume wait-core PrePark: task={} wait={:#x}",
                        task_id,
                        state.debug_id(),
                    );
                },
                ScheduleMode::Zombie => {
                    panic!(
                        "schedule_zombie_never_return cannot consume wait-core PrePark: task={} wait={:#x}",
                        task_id,
                        state.debug_id(),
                    );
                },
            },
            TaskSchedState::Waiting {
                state,
                interruptible,
                park: ParkState::Parked,
            } => match mode {
                ScheduleMode::WaitSleep { token } => {
                    if !token.matches_wait_state(&state) {
                        panic!(
                            "schedule_wait_sleep token mismatch on parked wait: task={} token_wait={:#x} current_wait={:#x}",
                            task_id,
                            token.wait_id(),
                            state.debug_id(),
                        );
                    }
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
                        },
                    )
                },
                ScheduleMode::Preempt => {
                    panic!(
                        "schedule_preempt observed parked wait current task: task={} wait={:#x}",
                        task_id,
                        state.debug_id(),
                    );
                },
                ScheduleMode::Runnable => {
                    panic!(
                        "schedule_runnable cannot consume wait-core Parked state: task={} wait={:#x}",
                        task_id,
                        state.debug_id(),
                    );
                },
                ScheduleMode::Zombie => {
                    panic!(
                        "schedule_zombie_never_return cannot consume wait-core Parked state: task={} wait={:#x}",
                        task_id,
                        state.debug_id(),
                    );
                },
            },
            TaskSchedState::Zombie => match mode {
                ScheduleMode::Zombie => {
                    panic!(
                        "schedule_zombie_never_return cannot re-enter zombie current task: task={}",
                        task_id,
                    );
                },
                ScheduleMode::WaitSleep { token } => {
                    panic!(
                        "schedule_wait_sleep cannot schedule zombie current task: task={} token_wait={:#x}",
                        task_id,
                        token.wait_id(),
                    );
                },
                ScheduleMode::Preempt => {
                    panic!("schedule_preempt cannot preempt zombie current task: task={}", task_id);
                },
                ScheduleMode::Runnable => {
                    panic!(
                        "schedule_runnable requires runnable current task: task={} state=Zombie",
                        task_id,
                    );
                },
            },
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
            } => {
                let wait_id = state.debug_id();
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
                            assert_eq!(observed_interruptible, interruptible);
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
                        assert!(
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
                        assert!(false, "zombie state observed after wait-core park");
                        drop(curr);
                    },
                }
            },
            ScheduleDecision::AbortWaitSleep { wait_id } => {
                kdebugln!(
                    "schedule_wait_sleep: abort sleep for task={} wait={:#x}; wait already completed",
                    task_id,
                    wait_id,
                );
                drop(curr);
                return ScheduleInnerResult::DidNotSwitch;
            },
            ScheduleDecision::DeferredPreempt { wait_id } => {
                // Trap-tail preempt callers commonly clear `need_resched`
                // before entering the scheduler. Restoring it here is
                // idempotent for future callers that preserved the flag and
                // prevents the deferred PrePark window from swallowing the
                // preempt request.
                mark_need_resched();
                kdebugln!(
                    "schedule_preempt: deferred for task={} wait={:#x}; current wait setup is still PrePark",
                    task_id,
                    wait_id,
                );
                drop(curr);
                return ScheduleInnerResult::DeferredPreempt;
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

        ScheduleInnerResult::Switched
    }

    /// Signal or force-complete the currently active wait, if any.
    pub fn notify(task: &Arc<Task>, uninterruptible: bool) {
        let (reason, mode) = if uninterruptible {
            (WaitReason::Force, WakeMode::Force)
        } else {
            (WaitReason::Signal, WakeMode::InterruptibleOnly)
        };

        let result = wait::wake_active_wait(task, reason, mode);
        if result != WakeResult::Stale {
            kdebugln!(
                "{} is notified through wait core, reason={:?}, mode={:?}, uninterruptible={}, result={:?}",
                task.tid(),
                reason,
                mode,
                uninterruptible,
                result,
            );
            return;
        }

        kdebugln!(
            "{} notify found no active wait, reason={:?}, mode={:?}, uninterruptible={}",
            task.tid(),
            reason,
            mode,
            uninterruptible,
        );
    }
}
pub use kore::*;

/// Upper-level APIs built upon [kore] functions.
mod higher_level {

    use crate::time::timer::schedule_local_irq_timer_event;

    use super::{
        wait::{WaitOutcome, WaitReason, WakeMode, WakeToken},
        *,
    };

    /// Immediate completion requested by a current-task wait precheck.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum CurrentWaitPrecheck {
        PredicateReady,
        Signal,
        Timeout,
    }

    /// Restricted outcome for one current-task wait round.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum CurrentWaitOutcome {
        PredicateReady,
        Timeout,
        Signal,
        Force,
        Cancelled,
        Unexpected,
    }

    /// Schedule the current wait-core round with an optional timeout.
    ///
    /// `token` names the wait round already published through
    /// `ActiveWait::begin()`. A late timer callback races through
    /// `wake_wait()`, so timeout validity is derived from the wait identity
    /// rather than an external cancellation flag.
    pub(super) fn schedule_wait_with_timeout(
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

        let start = with_intr_disabled(|| {
            let wait_id = token.wait_id();
            // `is_armed()` is used here only as the completion-open check: if
            // it is already false, some source/signal/force won before timer
            // install, so explicit wait sleep must prove the no-park abort
            // path instead of adding a stale timeout prerequisite.
            if !token.is_armed() {
                kdebugln!(
                    "schedule_wait_with_timeout: no-park before timeout install task={} wait={:#x} timeout={:?}",
                    task.tid(),
                    wait_id,
                    timeout,
                );
                let start = Instant::now();
                unsafe {
                    schedule_wait_sleep(&token);
                }
                return start;
            }

            if let Some(timeout) = timeout {
                // Timer events are not cancellable. Keep only a weak task
                // target so an early-finished long timeout does not pin the
                // whole task until expiry; `WakeToken` remains the wait-round
                // identity for stale/retired checks.
                let timeout_task = Arc::downgrade(task);
                let timeout_token = token.clone();
                let diagnostic_tid = task.tid();
                unsafe {
                    schedule_local_irq_timer_event(
                        timeout,
                        Box::new(move || {
                            let wait_id = timeout_token.wait_id();
                            let Some(task) = timeout_task.upgrade() else {
                                kdebugln!(
                                    "schedule_wait_with_timeout: timeout task={} wait={:#x} result=task_gone",
                                    diagnostic_tid,
                                    wait_id,
                                );
                                return;
                            };
                            let result = wait::wake_wait(
                                &task,
                                &timeout_token,
                                WaitReason::Timeout,
                                WakeMode::AnyWait,
                            );
                            kdebugln!(
                                "schedule_wait_with_timeout: timeout task={} wait={:#x} result={:?}",
                                diagnostic_tid,
                                wait_id,
                                result,
                            );
                        }),
                    );
                }
                kdebugln!(
                    "schedule_wait_with_timeout: timeout installed task={} wait={:#x} timeout={:?}",
                    task.tid(),
                    wait_id,
                    timeout,
                );
            } else {
                kdebugln!(
                    "schedule_wait_with_timeout: no timeout requested task={} wait={:#x}",
                    task.tid(),
                    wait_id,
                );
            }

            let start = Instant::now();
            unsafe {
                schedule_wait_sleep(&token);
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

    /// Begin, schedule, and finish one current-task wait round.
    ///
    /// This is the narrow public adapter for non-Event syscall waits. Callers
    /// can provide a precheck that wins the round by cancellation, but they
    /// never receive the wait token or raw lifecycle operations.
    #[track_caller]
    pub fn wait_current_with_timeout<F>(
        task: &Arc<Task>,
        interruptible: bool,
        timeout: Option<Duration>,
        precheck: F,
    ) -> (CurrentWaitOutcome, Duration)
    where
        F: FnOnce() -> Option<CurrentWaitPrecheck>,
    {
        let active_wait = wait::ActiveWait::begin(task, interruptible);
        let token = active_wait.token();

        if let Some(precheck) = precheck() {
            let reason = match precheck {
                CurrentWaitPrecheck::PredicateReady => WaitReason::PredicateReady,
                CurrentWaitPrecheck::Signal => WaitReason::Signal,
                CurrentWaitPrecheck::Timeout => WaitReason::Timeout,
            };
            active_wait.cancel(reason);
            let outcome = active_wait.finish();
            kdebugln!(
                "wait_current_with_timeout: precheck task={} request={:?} outcome={:?}",
                task.tid(),
                precheck,
                outcome,
            );
            return (precheck.into(), timeout.unwrap_or(Duration::MAX));
        }

        let rem = schedule_wait_with_timeout(task, token, timeout);
        let outcome = active_wait.finish();
        let outcome = CurrentWaitOutcome::from(outcome);
        kdebugln!(
            "wait_current_with_timeout: finished task={} outcome={:?} rem={:?}",
            task.tid(),
            outcome,
            rem,
        );
        (outcome, rem)
    }

    impl From<CurrentWaitPrecheck> for CurrentWaitOutcome {
        fn from(value: CurrentWaitPrecheck) -> Self {
            match value {
                CurrentWaitPrecheck::PredicateReady => Self::PredicateReady,
                CurrentWaitPrecheck::Signal => Self::Signal,
                CurrentWaitPrecheck::Timeout => Self::Timeout,
            }
        }
    }

    impl From<WaitOutcome> for CurrentWaitOutcome {
        fn from(value: WaitOutcome) -> Self {
            match value {
                WaitOutcome::Completed(WaitReason::Timeout) => Self::Timeout,
                WaitOutcome::Completed(WaitReason::Signal) => Self::Signal,
                WaitOutcome::Completed(WaitReason::Force) => Self::Force,
                WaitOutcome::Cancelled(_) => Self::Cancelled,
                _ => Self::Unexpected,
            }
        }
    }

    /// Yield the current running task to let other tasks run.
    pub fn yield_now() {
        assert!(get_current_task().is_sched_runnable());
        with_intr_disabled(|| unsafe {
            schedule_runnable();
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
