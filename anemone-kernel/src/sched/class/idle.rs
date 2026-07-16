//! Idle scheduler class. The last-resort scheduler that runs when there are no
//! other runnable tasks.

use crate::{
    prelude::*,
    sched::class::{PreemptDecision, SchedClassKind, Scheduler, TickAction},
};

pub struct Idle;

impl Scheduler for Idle {
    const KIND: SchedClassKind = SchedClassKind::Idle;

    fn enqueue_new(&mut self, _task: Arc<Task>) {
        panic!("idle scheduler should not be enqueued with any task");
    }

    fn enqueue_woken(&mut self, _task: Arc<Task>) {
        panic!("idle scheduler should not be enqueued with any task");
    }

    fn dequeue(&mut self, _task: &Arc<Task>) -> bool {
        panic!("idle scheduler should not be dequeued with any task");
    }

    fn requeue_yielded_current(&mut self, _task: Arc<Task>, _now: Instant) {
        panic!("idle scheduler should not requeue current task");
    }

    fn requeue_preempted_current(&mut self, _task: Arc<Task>, _now: Instant) {
        panic!("idle scheduler should not requeue current task");
    }

    fn handoff_woken_current(&mut self, _task: Arc<Task>, _now: Instant) {
        panic!("idle scheduler should not requeue current task");
    }

    fn put_prev_blocked(&mut self, _task: &Arc<Task>, _now: Instant) {
        panic!("idle task should not block");
    }

    fn put_prev_exiting(&mut self, _task: &Arc<Task>, _now: Instant) {
        panic!("idle task should not exit");
    }

    fn pick_next_task(&mut self) -> Option<Arc<Task>> {
        Some(IDLE_TASK.with(|task| (**task).clone()))
    }

    fn set_next_task(&mut self, task: &Arc<Task>, _now: Instant) {
        assert!(matches!(task.sched_class_kind(), SchedClassKind::Idle));
    }

    fn task_tick(&mut self, cur_task: &Arc<Task>, _now: Instant) -> TickAction {
        assert!(matches!(cur_task.sched_class_kind(), SchedClassKind::Idle));
        TickAction::RequestResched
    }

    fn decide_preempt_current(
        &mut self,
        _current: &Arc<Task>,
        candidate: &Arc<Task>,
        _now: Instant,
    ) -> PreemptDecision {
        assert!(matches!(candidate.sched_class_kind(), SchedClassKind::Idle));
        PreemptDecision::KeepCurrent
    }
}

mod idle_task {
    use core::hint::spin_loop;

    use super::*;

    extern "C" fn idle_loop() -> ! {
        loop {
            // if kernel preemption is disabled, this step must be taken to ensure other
            // tasks can run.
            unsafe {
                with_intr_disabled(|| {
                    if !take_pending_resched().is_empty() {
                        schedule_idle();
                    }
                })
            }

            spin_loop();
        }
    }

    #[percpu]
    pub static IDLE_TASK: Lazy<Arc<Task>> = Lazy::new(|| unsafe {
        let (mut task, guard) = Task::new_idle(idle_loop as *const ())
            .unwrap_or_else(|e| panic!("failed to create idle tasks: {:?}", e));
        // SAFETY:
        // idle task should not be registered to global task registry.
        unsafe {
            guard.forget();
        }
        Arc::new(task)
    });
}
use idle_task::*;

/// Get a clone of local processor's idle task.
pub fn clone_local_idle_task() -> Arc<Task> {
    IDLE_TASK.with(|task| (**task).clone())
}
