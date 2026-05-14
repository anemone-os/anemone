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
    mark_need_resched, pick_next_cpu, remote_enqueue, task_enqueue,
};
mod switch;
pub use switch::load_context;

mod event;
pub use event::Event;

pub mod class;

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

/// Only 2 functions are considered "core" functions of scheduler,
/// - [schedule] : xxx
/// - [try_to_wake_up] : xxx
/// along with state transition of task.
mod kore {
    use crate::sched::processor::task_enqueue;

    use super::*;

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

        let status = curr.status();

        match status {
            TaskStatus::Runnable => {
                if !curr.flags().is_idle() {
                    local_requeue_current(curr);
                } else {
                    drop(curr);
                }
            },
            TaskStatus::Waiting { interruptible } => {
                knoticeln!(
                    "task {} is waiting (interruptible: {}), not enqueuing it to run queue",
                    current_task_id(),
                    interruptible,
                );
                drop(curr);
            },
            TaskStatus::Zombie => {
                knoticeln!(
                    "task {} is zombie, not enqueuing it to run queue",
                    current_task_id(),
                );
                drop(curr);
            },
        }

        unsafe {
            switch_out();
        }
    }

    #[derive(Debug)]
    pub enum WakeUpError {
        TaskAlreadyRunnable,
        /// The status won't be [TaskStatus::Runnable].
        UnexpectedStatus(TaskStatus),
    }

    /// Try to wake up a task.
    ///
    /// What linux folks often called "ttwp".
    ///
    /// Panics if expected_status contains any non-sleeping status or is empty.
    /// If you just want to wake up a task regardless of its current status,
    /// call [notify] instead.
    ///
    /// TODO: docs.
    pub fn try_to_wake_up(
        task: &Arc<Task>,
        expected_status: &[TaskStatus],
    ) -> Result<(), WakeUpError> {
        assert!(
            !expected_status.is_empty(),
            "expected_status cannot be empty"
        );
        assert!(
            expected_status.iter().all(|s| s.is_sleeping()) && !expected_status.is_empty(),
            "expected_status must be a sleeping status"
        );

        // 1. grab the right to wake up the task.
        task.update_status_with(|status| {
            if !expected_status.contains(&status) {
                knoticeln!(
                    "trying to wake up task {}, but its status is {:?}, which is not in expected_status {:?}",
                    task.tid(),
                    status,
                    expected_status,
                );
                let err = if let TaskStatus::Runnable = status {
                    WakeUpError::TaskAlreadyRunnable
                } else {
                    WakeUpError::UnexpectedStatus(status)
                };
                return (status, Err(err));
            }

            match status {
                TaskStatus::Runnable | TaskStatus::Zombie => unreachable!(/* handled above */),
                TaskStatus::Waiting { .. } => (TaskStatus::Runnable, Ok(())),
            }
        })?;

        kdebugln!(
            "task {} is woken up, enqueueing it to run queue",
            task.tid()
        );

        // 2. enqueue the task to run queue.
        task_enqueue(task.clone());

        Ok(())
    }

    /// Whatever task's status is, try to wake it up.
    ///
    /// If task's status is [TaskStatus::Runnable], [TaskStatus::Zombie], the
    /// no-op.
    ///
    /// If `uninterruptible` is true, even if the task is in uninterruptible
    /// sleep, it will be woken up. This is useful when we want to do a forceful
    /// wake up, e.g., when a thread group is exiting.
    ///
    /// Mainly used by signals.
    pub fn notify(task: &Arc<Task>, uninterruptible: bool) {
        let need_enqueue = task.update_status_with(|prev| match prev {
            TaskStatus::Runnable => (prev, false),
            TaskStatus::Waiting {
                interruptible: true,
            } => (TaskStatus::Runnable, true),
            TaskStatus::Waiting {
                interruptible: false,
            } => {
                if uninterruptible {
                    (TaskStatus::Runnable, true)
                } else {
                    (prev, false)
                }
            },
            TaskStatus::Zombie => (prev, false),
        });
        if need_enqueue {
            kdebugln!(
                "task {} is woken up by notify, enqueueing it to run queue",
                task.tid()
            );
            task_enqueue(task.clone());
        }
    }
}
pub use kore::*;

/// Upper-level APIs built upon [kore] functions.
mod higher_level {

    use crate::time::timer::schedule_local_irq_timer_event;

    use super::*;

    /// Schedule the next task to run, with timeout.
    ///
    /// If `timeout` is [None], then current task will wait indefinitely until
    /// being woken up by other events.
    ///
    /// Returns the remaining time until timeout.
    ///
    /// If the returned duration is zero, it does not necessarily mean that the
    /// timeout has expired.
    ///
    /// Caller should set task's status to [TaskStatus::Waiting] before calling
    /// this function.
    pub fn schedule_with_timeout(timeout: Option<Duration>) -> Duration {
        if let Some(timeout) = timeout {
            if timeout == Duration::ZERO {
                kdebugln!("schedule_with_timeout: timeout is zero, returning immediately");
                return Duration::ZERO;
            }
        }

        let task = get_current_task();
        let cloned_task = task.clone();
        let validness = Arc::new(AtomicBool::new(true));
        let cloned_validness = validness.clone();

        let start = with_intr_disabled(|| {
            if let Some(timeout) = timeout {
                unsafe {
                    schedule_local_irq_timer_event(
                        timeout,
                        Box::new(move || {
                            if cloned_validness.swap(false, Ordering::SeqCst) {
                                kdebugln!(
                                    "schedule_with_timeout: timeout expired, waking up task {}",
                                    cloned_task.tid()
                                );
                                notify(&cloned_task, true);
                            } else {
                                kdebugln!(
                                    "schedule_with_timeout: timer callback called, but timer is already invalid"
                                );
                            }
                        }),
                    );
                }
            }

            let start = Instant::now();
            unsafe {
                schedule();
            }

            // we're back.
            validness.store(false, Ordering::SeqCst);

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
